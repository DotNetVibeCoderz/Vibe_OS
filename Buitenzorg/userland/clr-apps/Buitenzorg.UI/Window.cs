// Buitenzorg.UI -- Window (the root: layout/render passes, pointer and
// keyboard routing, focus + Tab navigation, the popup/overlay layer for
// flyouts, menus, dropdowns and tooltips), Application (theme + styles + the
// event loop) and DispatcherTimer.

namespace Buitenzorg.UI;

/// <summary>A light-dismiss overlay (Avalonia Popup): flyouts, menus, dropdowns, tooltips.</summary>
public sealed class Popup
{
    public Control Child { get; }
    public Control? PlacementTarget { get; set; }
    public PlacementMode Placement { get; set; } = PlacementMode.Bottom;
    public bool IsLightDismiss { get; set; } = true;
    /// <summary>Pointer position for <see cref="PlacementMode.Pointer"/>.</summary>
    public Point PointerPosition { get; set; }
    public bool IsOpen { get; internal set; }
    public event Action? Closed;
    internal Window? Host;

    public Popup(Control child) { Child = child; }

    public void Open(Window w) => w.OpenPopup(this);
    public void Close() => Host?.ClosePopup(this);
    internal void RaiseClosed() => Closed?.Invoke();
}

public sealed class Window : Control
{
    public static readonly StyledProperty<string> TitleProperty = UIProperties.Register("Title", "");

    readonly int _id;
    readonly List<Popup> _popups = new List<Popup>();
    Control? _content;
    Control? _hover;
    Control? _captured;
    Control? _focused;
    bool _layoutDirty = true;
    bool _renderDirty = true;
    long _pressAt;
    Point _pressPos;
    bool _holdFired;
    Control? _tipOwner;
    long _tipSince;
    Popup? _tip;
    Point _pointer;

    public int ClientWidth { get; }
    public int ClientHeight { get; }
    public bool IsClosed { get; private set; }
    public Control? FocusedControl => _focused;
    public IReadOnlyList<Popup> Popups => _popups;
    public int Frames { get; private set; }
    public int LastFrameOps { get; private set; }
    /// <summary>Raised for keys nobody handled (shortcuts).</summary>
    public event Action<KeyEventArgs>? UnhandledKey;
    public event Action? Closing;

    /// <summary>A window with a client area of <paramref name="width"/> x <paramref name="height"/>.
    /// <paramref name="headless"/> renders nowhere (tests).</summary>
    public Window(string title, int width, int height, bool headless = false)
    {
        ClientWidth = width;
        ClientHeight = height;
        SetValue(TitleProperty, title);
        _id = headless ? 0 : Native.WinCreateEx(title, width, height, 1);
        if (!headless && _id == 0) throw new InvalidOperationException("Buitenzorg.UI: no desktop to open a window on");
        Attach(this);
        Classes.Add("window");
    }

    public int Id => _id;

    public Control? Content
    {
        get => _content;
        set
        {
            if (_content != null) RemoveVisualChild(_content);
            _content = value;
            if (value != null) AddVisualChild(value);
        }
    }

    public void InvalidateLayout() { _layoutDirty = true; _renderDirty = true; }
    public new void InvalidateVisual() { _renderDirty = true; }
    public bool NeedsRender => _renderDirty || _layoutDirty;

    public void Close()
    {
        Closing?.Invoke();
        IsClosed = true;
    }

    // ---- layout + render
    public void UpdateLayout()
    {
        if (StyleVersion != Styles.Version) RestyleAll();
        if (!_layoutDirty) return;
        _layoutDirty = false;
        Measure(new Size(ClientWidth, ClientHeight));
        Arrange(new Rect(0, 0, ClientWidth, ClientHeight));
        foreach (var p in _popups) PlacePopup(p);
    }

    /// <summary>Lay out if needed, draw everything, and present the window.</summary>
    public void RenderFrame()
    {
        UpdateLayout();
        var dc = new DrawingContext(_id, ClientWidth, ClientHeight);
        dc.Clear(Background >= 0 ? Background : 0);
        if (Background >= 0 && BackgroundBottom >= 0) dc.FillGradient(new Rect(0, 0, ClientWidth, ClientHeight), 0, Background, BackgroundBottom);
        RenderChildren(dc);
        if (_focused != null && _focused.PseudoClasses.Contains(":focus-visible")) RenderFocusRing(dc, _focused);
        foreach (var p in _popups) p.Child.RenderTree(dc);
        if (_id != 0) Native.WinPresent(_id);
        _renderDirty = false;
        Frames++;
        LastFrameOps = dc.Ops;
    }

    static void RenderFocusRing(DrawingContext dc, Control c)
    {
        var r = c.Bounds.Inflate(2);
        dc.StrokeRoundRect(r, c.CornerRadius + 2, Theme.FocusRing, 2);
    }

    public override void Render(DrawingContext dc) { }

    /// <summary>Re-apply styles to the whole tree (after the style set or theme changed).</summary>
    public void RestyleAll()
    {
        ApplyStyles(false);
        foreach (var d in Descendants()) d.ApplyStyles(false);
        InvalidateSubtree();
        InvalidateLayout();
    }

    // ---- popups
    internal void OpenPopup(Popup p)
    {
        if (p.IsOpen) return;
        p.Host = this;
        p.IsOpen = true;
        _popups.Add(p);
        AddVisualChild(p.Child);
        // popups are rendered by the window after the tree, not as normal children
        VisualChildren.Remove(p.Child);
        VisualChildren.Add(p.Child);
        InvalidateLayout();
    }

    internal void ClosePopup(Popup p)
    {
        if (!p.IsOpen) return;
        p.IsOpen = false;
        _popups.Remove(p);
        RemoveVisualChild(p.Child);
        if (_hover != null && !_hover.IsAttached) _hover = null;
        if (_focused != null && !_focused.IsAttached) _focused = null;
        p.RaiseClosed();
        InvalidateLayout();
    }

    protected override void RenderChildren(DrawingContext dc)
    {
        foreach (var c in VisualChildren)
            if (!IsPopupChild(c)) c.RenderTree(dc);
    }

    bool IsPopupChild(Control c)
    {
        foreach (var p in _popups) if (p.Child == c) return true;
        return false;
    }

    protected override Size MeasureOverride(Size available)
    {
        _content?.Measure(available);
        return available;
    }

    protected override void ArrangeOverride(Rect bounds)
    {
        _content?.Arrange(bounds);
    }

    void PlacePopup(Popup p)
    {
        var c = p.Child;
        c.Measure(new Size(ClientWidth, ClientHeight));
        int w = Math.Min(c.DesiredSize.Width, ClientWidth), h = Math.Min(c.DesiredSize.Height, ClientHeight);
        var t = p.PlacementTarget?.Bounds ?? new Rect(p.PointerPosition.X, p.PointerPosition.Y, 0, 0);
        int x, y;
        switch (p.Placement)
        {
            case PlacementMode.Top: x = t.X; y = t.Y - h - 4; break;
            case PlacementMode.Right: x = t.Right + 2; y = t.Y; break;
            case PlacementMode.Left: x = t.X - w - 2; y = t.Y; break;
            case PlacementMode.Pointer: x = p.PointerPosition.X + 8; y = p.PointerPosition.Y + 16; break;
            case PlacementMode.Center: x = (ClientWidth - w) / 2; y = (ClientHeight - h) / 2; break;
            default: x = t.X; y = t.Bottom + 4; break;
        }
        if (p.Placement == PlacementMode.Bottom && y + h > ClientHeight) y = t.Y - h - 4;
        x = Math.Max(2, Math.Min(x, ClientWidth - w - 2));
        y = Math.Max(2, Math.Min(y, ClientHeight - h - 2));
        c.Arrange(new Rect(x, y, w, h));
    }

    // ---- input routing
    Control? Hit(int x, int y)
    {
        for (int i = _popups.Count - 1; i >= 0; i--)
        {
            if (_popups[i] == _tip) continue;
            var h = _popups[i].Child.HitTest(x, y);
            if (h != null) return h;
        }
        return _content?.HitTest(x, y);
    }

    static Control? Enabled(Control? c)
    {
        // a disabled control and its children do not take input; the nearest enabled ancestor does
        while (c != null && !c.IsEffectivelyEnabled) c = c.Parent;
        return c;
    }

    void SetHover(Control? target)
    {
        if (target == _hover) return;
        // ancestors of the new target keep :pointerover
        var keep = new List<Control>();
        for (var c = target; c != null; c = c.Parent) keep.Add(c);
        for (var c = _hover; c != null; c = c.Parent)
            if (!keep.Contains(c)) { c.SetPseudo(":pointerover", false); c.OnPointerExited(); }
        for (int i = keep.Count - 1; i >= 0; i--)
            if (!keep[i].IsPointerOver) { keep[i].SetPseudo(":pointerover", true); keep[i].OnPointerEntered(); }
        _hover = target;
    }

    static void Bubble(Control? c, PointerEventArgs e, int kind)
    {
        e.Source = c;
        for (; c != null && !e.Handled; c = c.Parent)
        {
            if (kind == 0) c.OnPointerPressed(e);
            else if (kind == 1) c.OnPointerReleased(e);
            else if (kind == 3) c.OnPointerWheelChanged(e);
            else c.OnPointerMoved(e);
        }
    }

    /// <summary>The wheel turned over (x, y): bubbles from the control under the pointer
    /// until a scrollable one takes it (bz-abi pointer_event WHEEL).</summary>
    public void DispatchWheel(int x, int y, int notches)
    {
        _pointer = new Point(x, y);
        HideTip();
        var e = new PointerEventArgs { X = x, Y = y, WheelDelta = notches };
        Bubble(Enabled(Hit(x, y)), e, 3);
        UpdateLayout();
    }

    /// <summary>Handle a packed pointer event (bz-abi pointer_event).</summary>
    public void DispatchPointer(int kind, int x, int y, bool left)
    {
        _pointer = new Point(x, y);
        var e = new PointerEventArgs { X = x, Y = y, LeftButton = left };
        switch (kind)
        {
            case 1: // move
            {
                if (_captured != null)
                {
                    if (Math.Abs(x - _pressPos.X) + Math.Abs(y - _pressPos.Y) > 4) _pressAt = 0;
                    Bubble(_captured, e, 2);
                    break;
                }
                var t = Enabled(Hit(x, y));
                SetHover(t);
                HideTip();
                _tipOwner = FindTipOwner(t);
                _tipSince = Clock.Now;
                Bubble(t, e, 2);
                break;
            }
            case 2: // down
            {
                HideTip();
                _tipOwner = null;
                var t = Enabled(Hit(x, y));
                // a press outside the top light-dismiss popup closes it and is consumed
                if (_popups.Count > 0)
                {
                    var top = _popups[_popups.Count - 1];
                    if (top.IsLightDismiss && !IsInside(t, top.Child))
                    {
                        CloseLightDismissFrom(t);
                        if (t == null || !IsInsideAnyPopup(t)) return;
                    }
                }
                SetHover(t);
                _captured = t;
                _pressAt = Clock.Now;
                _pressPos = new Point(x, y);
                _holdFired = false;
                for (var c = t; c != null; c = c.Parent)
                    if (c.Focusable && c.IsEffectivelyEnabled) { SetFocus(c, false); break; }
                Bubble(t, e, 0);
                break;
            }
            case 3: // up
            {
                var c = _captured;
                _captured = null;
                _pressAt = 0;
                if (c != null && !_holdFired) Bubble(c, e, 1);
                SetHover(Enabled(Hit(x, y)));
                break;
            }
            case 4: // leave
                SetHover(null);
                HideTip();
                _tipOwner = null;
                break;
        }
    }

    void CloseLightDismissFrom(Control? target)
    {
        for (int i = _popups.Count - 1; i >= 0; i--)
        {
            var p = _popups[i];
            if (!p.IsLightDismiss || IsInside(target, p.Child)) break;
            ClosePopup(p);
        }
    }

    bool IsInsideAnyPopup(Control t)
    {
        foreach (var p in _popups) if (IsInside(t, p.Child)) return true;
        return false;
    }

    static bool IsInside(Control? c, Control? root)
    {
        if (root == null) return false;
        for (; c != null; c = c.Parent) if (c == root) return true;
        return false;
    }

    /// <summary>A key from KEY_READ: to the focused control (bubbling), then Tab / Escape / shortcuts.</summary>
    /// <summary>Games: deliver key presses and releases (KEY_EVENT) to the focused
    /// control's <see cref="Control.OnKeyEvent"/> instead of characters.</summary>
    public bool RawKeys { get; set; }

    readonly HashSet<int> _down = new HashSet<int>();

    /// <summary>Whether a raw key is held (from the events this window got).</summary>
    public bool IsKeyDown(int code) => _down.Contains(code);

    /// <summary>A raw key press / release (the device's KEY_EVENT, or a test).</summary>
    public void DispatchKeyEvent(int code, bool pressed)
    {
        if (pressed) _down.Add(code); else _down.Remove(code);
        var e = new RawKeyEventArgs { Code = code, Pressed = pressed };
        for (var c = _focused; c != null && !e.Handled; c = c.Parent) c.OnKeyEvent(e);
    }

    /// <summary>Tests: press and release keys by character ("aa d").</summary>
    public void SimulateKeyEvents(string keys)
    {
        foreach (char k in keys) { int code = RawKey.FromChar(k); DispatchKeyEvent(code, true); DispatchKeyEvent(code, false); }
    }

    public void DispatchKey(char key)
    {
        if (key == Keys.Rubout) key = Keys.Delete;
        if (key == '\r') key = Keys.Enter;
        HideTip();
        var e = new KeyEventArgs { Key = key, Source = _focused };
        if (_focused != null && (key != Keys.Tab || _focused.AcceptsTab))
        {
            if (!_focused.PseudoClasses.Contains(":focus-visible") && !Keys.IsPrintable(key)) _focused.SetPseudo(":focus-visible", true);
            for (var c = _focused; c != null && !e.Handled; c = c.Parent) c.OnKeyDown(e);
        }
        if (e.Handled) return;
        if (key == Keys.Tab) { MoveFocus(); return; }
        if (key == Keys.Escape && _popups.Count > 0) { ClosePopup(_popups[_popups.Count - 1]); return; }
        UnhandledKey?.Invoke(e);
    }

    internal void SetFocus(Control? c, bool keyboard)
    {
        if (c == _focused) { if (keyboard) c?.SetPseudo(":focus-visible", true); return; }
        var old = _focused;
        _focused = c;
        old?.OnLostFocus();
        if (c != null)
        {
            c.OnGotFocus();
            if (keyboard) c.SetPseudo(":focus-visible", true);
        }
        _renderDirty = true;
    }

    /// <summary>Tab navigation: the next focusable control in tree order (popups first when open).</summary>
    public void MoveFocus()
    {
        Control root = _popups.Count > 0 ? _popups[_popups.Count - 1].Child : (Control?)_content ?? this;
        var order = new List<Control>();
        if (root.Focusable) order.Add(root);
        foreach (var d in root.Descendants())
            if (d.Focusable && d.IsVisible && d.IsEffectivelyEnabled && !d.Bounds.IsEmpty && IsShown(d)) order.Add(d);
        if (order.Count == 0) return;
        int i = _focused == null ? -1 : order.IndexOf(_focused);
        SetFocus(order[(i + 1) % order.Count], true);
    }

    static bool IsShown(Control c)
    {
        for (var p = c; p != null; p = p.Parent) if (!p.IsVisible) return false;
        return true;
    }

    internal void OnControlDetached(Control c)
    {
        if (_hover == c) _hover = null;
        if (_captured == c) _captured = null;
        if (_focused == c) _focused = null;
        if (_tipOwner == c) _tipOwner = null;
    }

    // ---- tooltips + press-and-hold
    static Control? FindTipOwner(Control? c)
    {
        for (; c != null; c = c.Parent)
            if (ToolTip.GetTip(c) != null) return c;
        return null;
    }

    void HideTip()
    {
        if (_tip != null) { var t = _tip; _tip = null; ClosePopup(t); }
    }

    /// <summary>Timed work of the window (called by the event loop).</summary>
    internal void Tick()
    {
        long now = Clock.Now;
        if (_tipOwner != null && _tip == null && _captured == null && now - _tipSince > 700)
        {
            var tip = ToolTip.GetTip(_tipOwner);
            if (tip != null)
            {
                _tip = new Popup(new ToolTip { Content = tip }) { Placement = PlacementMode.Pointer, PointerPosition = _pointer, IsLightDismiss = false };
                OpenPopup(_tip);
            }
            _tipOwner = null;
        }
        if (_captured != null && _pressAt != 0 && !_holdFired && now - _pressAt > 650)
        {
            _holdFired = true;
            var e = new PointerEventArgs { X = _pressPos.X, Y = _pressPos.Y, LeftButton = true, Source = _captured };
            for (var c = _captured; c != null && !e.Handled; c = c.Parent) c.OnHold(e);
            if (!e.Handled) _holdFired = false;
        }
    }

    // ---- simulated input (self-tests drive the real routing with these)
    public void SimulatePointer(int kind, int x, int y) => DispatchPointer(kind, x, y, kind == 2 || kind == 1 && _captured != null);

    /// <summary>Move to, press and release over the centre of a control.</summary>
    public void SimulateClick(Control c)
    {
        UpdateLayout();
        var p = c.Bounds.Center;
        DispatchPointer(1, p.X, p.Y, false);
        DispatchPointer(2, p.X, p.Y, true);
        DispatchPointer(3, p.X, p.Y, false);
        UpdateLayout();
    }

    public void SimulateClickAt(int x, int y)
    {
        UpdateLayout();
        DispatchPointer(1, x, y, false);
        DispatchPointer(2, x, y, true);
        DispatchPointer(3, x, y, false);
        UpdateLayout();
    }

    public void SimulateKeys(string keys)
    {
        foreach (char k in keys) DispatchKey(k);
        UpdateLayout();
    }

    internal bool PumpEvents()
    {
        bool any = false;
        if (_id != 0)
        {
            for (int n = 0; n < 32; n++)
            {
                long ev = Native.WinEvent(_id);
                if (ev == 0) break;
                any = true;
                int kind = (int)((ev >> 56) & 0xFF);
                int x = (short)(ev & 0xFFFF), y = (short)((ev >> 16) & 0xFFFF);
                bool left = ((ev >> 32) & 1) != 0;
                if (kind == 5) DispatchWheel(x, y, (short)((ev >> 40) & 0xFFFF));
                else DispatchPointer(kind, x, y, left);
            }
            if (RawKeys)
            {
                for (int n = 0; n < 32; n++)
                {
                    int ev = Native.KeyEvent();
                    if (ev == 0) break;
                    any = true;
                    DispatchKeyEvent(ev & 0xFFFF, (ev & 0x10000) != 0);
                }
                // a release that went elsewhere (focus moved): ask the device
                if (_down.Count > 0)
                    foreach (var code in _down.ToArray())
                        if (Native.KeyState(code) == 0) DispatchKeyEvent(code, false);
            }
            else
                for (int n = 0; n < 32; n++)
                {
                    int k = Native.KeyRead();
                    if (k == 0) break;
                    any = true;
                    DispatchKey((char)k);
                }
        }
        return any;
    }
}

/// <summary>A timer ticking on the UI loop (Avalonia DispatcherTimer).</summary>
public sealed class DispatcherTimer
{
    static readonly List<DispatcherTimer> _active = new List<DispatcherTimer>();
    long _next;

    public int IntervalMs { get; set; }
    public bool IsEnabled { get; private set; }
    public event Action? Tick;

    public DispatcherTimer(int intervalMs) { IntervalMs = intervalMs; }

    public void Start()
    {
        if (IsEnabled) return;
        IsEnabled = true;
        _next = Clock.Now + IntervalMs;
        _active.Add(this);
    }

    public void Stop()
    {
        IsEnabled = false;
        _active.Remove(this);
    }

    /// <summary>Call <paramref name="action"/> once after <paramref name="ms"/> milliseconds.</summary>
    public static DispatcherTimer RunOnce(int ms, Action action)
    {
        var t = new DispatcherTimer(ms);
        t.Tick += () => { t.Stop(); action(); };
        t.Start();
        return t;
    }

    internal static long NextDue()
    {
        long due = long.MaxValue;
        foreach (var t in _active) due = Math.Min(due, t._next);
        return due;
    }

    internal static void RunDue()
    {
        long now = Clock.Now;
        var due = new List<DispatcherTimer>();
        foreach (var t in _active) if (t._next <= now) due.Add(t);
        foreach (var t in due)
        {
            t._next = now + Math.Max(1, t.IntervalMs);
            t.Tick?.Invoke();
        }
    }
}

/// <summary>Work posted to the UI loop from other threads (Avalonia Dispatcher.UIThread.Post).</summary>
public static class Dispatcher
{
    static readonly object _lock = new object();
    static readonly List<Action> _queue = new List<Action>();

    public static void Post(Action a)
    {
        lock (_lock) _queue.Add(a);
    }

    internal static bool RunPending()
    {
        List<Action>? work = null;
        lock (_lock)
        {
            if (_queue.Count == 0) return false;
            work = new List<Action>(_queue);
            _queue.Clear();
        }
        foreach (var a in work) a();
        return true;
    }
}

/// <summary>The application: theme variant, global styles, the event loop.</summary>
public class Application
{
    public static Application? Current { get; private set; }

    public Styles Styles { get; } = new Styles();
    public ThemeVariant ThemeVariant { get; private set; }
    readonly List<Window> _windows = new List<Window>();

    public Application(ThemeVariant variant = ThemeVariant.Dark)
    {
        Current = this;
        SetTheme(variant);
    }

    /// <summary>True on the live desktop; false during the headless boot self-tests.</summary>
    public static bool IsInteractive
    {
        get { try { return Native.IsInteractive() != 0; } catch (Exception) { return false; } }
    }

    /// <summary>Switch light/dark: rebuild the theme styles and restyle every open window.</summary>
    public void SetTheme(ThemeVariant variant)
    {
        ThemeVariant = variant;
        Theme.Use(variant);
        // keep app styles added after the theme
        var extra = new List<Style>();
        foreach (var s in Styles) if (!Theme.IsThemeStyle(s)) extra.Add(s);
        Styles.Clear();
        Styles.AddRange(Theme.BuildStyles());
        Styles.AddRange(extra);
        foreach (var w in _windows) w.RestyleAll();
    }

    internal void Register(Window w)
    {
        if (!_windows.Contains(w)) _windows.Add(w);
    }

    /// <summary>Track a window (theme switches restyle it).</summary>
    public void Apply(Window w) => Register(w);

    /// <summary>One iteration of the loop: input, timers, animations, layout, render.</summary>
    public bool RunOnce(Window w)
    {
        Register(w);
        bool any = w.PumpEvents();
        any |= Dispatcher.RunPending();
        DispatcherTimer.RunDue();
        if (Animator.Active) { Animator.Tick(); any = true; }
        w.Tick();
        if (w.NeedsRender) w.RenderFrame();
        return any;
    }

    /// <summary>Run the event loop until the window closes.</summary>
    public void Run(Window w)
    {
        Register(w);
        w.RenderFrame();
        while (!w.IsClosed)
        {
            bool busy = RunOnce(w);
            if (!busy && !Animator.Active)
            {
                long wait = Math.Min(16, Math.Max(0, DispatcherTimer.NextDue() - Clock.Now));
                Thread.Sleep((int)Math.Max(5, wait));
            }
        }
    }
}
