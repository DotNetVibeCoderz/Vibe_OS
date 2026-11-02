// Buitenzorg.UI -- overlay controls: flyouts, menus (bar, flyout, context),
// tooltips, notifications, a modal dialog, and the calendar pickers.

namespace Buitenzorg.UI;

public sealed class FlyoutPresenter : Border { }
public sealed class MenuFlyoutPresenter : StackPanelHost { }
public sealed class ContextMenu : StackPanelHost { }
public sealed class MenuDropDown : StackPanelHost { }

/// <summary>A border hosting a vertical stack (menu presenters).</summary>
public class StackPanelHost : Border
{
    public StackPanel Stack { get; } = new StackPanel();
    public StackPanelHost() { Child = Stack; MinWidth = 160; }
}

/// <summary>Something shown in a popup next to a control (Avalonia FlyoutBase).</summary>
public abstract class FlyoutBase
{
    Popup? _popup;
    public PlacementMode Placement { get; set; } = PlacementMode.Bottom;
    public bool IsOpen => _popup != null && _popup.IsOpen;
    public event Action? Opened;
    public event Action? Closed;

    protected abstract Control CreatePresenter();

    public void ShowAt(Control target) => ShowAt(target, Placement, new Point(0, 0));

    internal void ShowAt(Control target, PlacementMode placement, Point pointer)
    {
        var w = target.Window;
        if (w == null || IsOpen) return;
        _popup = new Popup(CreatePresenter()) { PlacementTarget = placement == PlacementMode.Pointer ? null : target, Placement = placement, PointerPosition = pointer };
        _popup.Closed += () => { _popup = null; Closed?.Invoke(); };
        _popup.Open(w);
        Opened?.Invoke();
    }

    public void Hide() => _popup?.Close();
}

/// <summary>Any content in a flyout (Avalonia Flyout).</summary>
public sealed class Flyout : FlyoutBase
{
    public Control Content { get; set; }
    public Flyout(Control content) { Content = content; }
    protected override Control CreatePresenter() => new FlyoutPresenter { Child = Content };
}

/// <summary>A command row of a menu (Avalonia MenuItem); with <see cref="Items"/> it opens a submenu.</summary>
public sealed class MenuItem : Control
{
    public string Header { get; set; }
    public string InputGesture { get; set; } = "";
    public IconKind Glyph { get; set; }
    public ICommand? Command { get; set; }
    public object? CommandParameter { get; set; }
    public List<MenuItem> Items { get; } = new List<MenuItem>();
    public bool IsSeparator { get; }
    public event Action<MenuItem>? Click;
    internal Action? CloseMenu;
    internal bool InBar;
    Popup? _sub;

    public MenuItem(string header, Action<MenuItem>? click = null, string gesture = "", IconKind glyph = IconKind.None)
    {
        Header = header;
        InputGesture = gesture;
        Glyph = glyph;
        if (click != null) Click += click;
        Focusable = true;
    }

    MenuItem() { Header = ""; IsSeparator = true; IsHitTestVisible = false; }

    public static MenuItem Separator() => new MenuItem();

    public MenuItem Add(MenuItem child) { Items.Add(child); return this; }

    protected override Size MeasureOverride(Size a)
    {
        if (IsSeparator) return new Size(0, 9);
        int st = TextStyle;
        int w = Fonts.Measure(Header, st) + Padding.Horizontal + (InBar ? 0 : 28);
        if (InputGesture.Length > 0) w += Fonts.Measure(InputGesture, 0) + 24;
        if (Items.Count > 0 && !InBar) w += 18;
        return new Size(w, Fonts.LineHeight(st) + Padding.Vertical);
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        if (IsSeparator) { dc.FillRect(new Rect(b.X + 8, b.Y + 4, b.Width - 16, 1), Theme.StrokeSoft); return; }
        if (IsFocused && !IsPointerOver) dc.FillRoundRect(b, CornerRadius, Theme.Surface);
        base.Render(dc);
        var r = ContentRect(b);
        int x = r.X;
        if (!InBar)
        {
            if (Glyph != IconKind.None) Icon.Draw(dc, Glyph, new Rect(x, r.Y + 1, 14, 14), IsEffectivelyEnabled ? Theme.Accent : Theme.TextFaint);
            x += 28;
        }
        dc.Text(x, r.Y, Header, Foreground, TextStyle);
        if (InputGesture.Length > 0) dc.Text(r.Right - Fonts.Measure(InputGesture, 0) - (Items.Count > 0 ? 18 : 0), r.Y, InputGesture, Theme.TextFaint, 0);
        if (Items.Count > 0 && !InBar) Icon.Draw(dc, IconKind.ChevronRight, new Rect(r.Right - 12, r.Y + 2, 12, 12), Theme.TextMuted);
    }

    internal void Invoke()
    {
        if (!IsEffectivelyEnabled || IsSeparator) return;
        if (Items.Count > 0) { OpenSub(); return; }
        CloseMenu?.Invoke();
        Click?.Invoke(this);
        if (Command != null && Command.CanExecute(CommandParameter)) Command.Execute(CommandParameter);
    }

    internal void OpenSub()
    {
        if (Window == null || (_sub != null && _sub.IsOpen)) return;
        var host = new MenuDropDown();
        Menus.Fill(host.Stack, Items, () => { _sub?.Close(); CloseMenu?.Invoke(); });
        _sub = new Popup(host) { PlacementTarget = this, Placement = InBar ? PlacementMode.Bottom : PlacementMode.Right };
        _sub.Open(Window);
        SetPseudo(":open", true);
        _sub.Closed += () => SetPseudo(":open", false);
        if (host.Stack.Children.Count > 0) host.Stack.Children[0].Focus();
    }

    internal void CloseSub() { _sub?.Close(); _sub = null; }
    internal bool SubOpen => _sub != null && _sub.IsOpen;

    protected internal override void OnPointerReleased(PointerEventArgs e) { e.Handled = true; Invoke(); }
    protected internal override void OnPointerPressed(PointerEventArgs e) { e.Handled = true; }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.Enter || e.Key == Keys.Space || (e.Key == Keys.Right && Items.Count > 0 && !InBar)) { Invoke(); e.Handled = true; return; }
        if (e.Key == Keys.Down || e.Key == Keys.Up)
        {
            if (InBar && e.Key == Keys.Down && Items.Count > 0) { OpenSub(); e.Handled = true; return; }
            var stack = Parent;
            if (stack != null)
            {
                var list = stack.VisualChildren;
                int i = list.IndexOf(this);
                for (int step = 1; step <= list.Count; step++)
                {
                    int j = (i + (e.Key == Keys.Down ? step : -step) + list.Count) % list.Count;
                    if (list[j] is MenuItem m && !m.IsSeparator && m.IsEffectivelyEnabled) { m.Focus(); m.SetPseudo(":focus-visible", true); break; }
                }
                e.Handled = true;
            }
        }
        base.OnKeyDown(e);
    }
}

internal static class Menus
{
    public static void Fill(StackPanel stack, List<MenuItem> items, Action close)
    {
        foreach (var m in items)
        {
            m.CloseMenu = close;
            m.InBar = false;
            if (m.Parent != null) (m.Parent as StackPanel)?.Children.Remove(m);
            stack.Children.Add(m);
        }
    }
}

/// <summary>A menu of commands in a flyout (Avalonia MenuFlyout).</summary>
public sealed class MenuFlyout : FlyoutBase
{
    public List<MenuItem> Items { get; } = new List<MenuItem>();
    public MenuFlyout Add(MenuItem m) { Items.Add(m); return this; }

    protected override Control CreatePresenter()
    {
        var p = new MenuFlyoutPresenter();
        Menus.Fill(p.Stack, Items, Hide);
        return p;
    }
}

/// <summary>A horizontal menu bar (Avalonia Menu).</summary>
public sealed class Menu : Control
{
    readonly StackPanel _bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2 };
    public Menu() { AddVisualChild(_bar); }

    public Menu Add(MenuItem top)
    {
        top.InBar = true;
        top.Padding = new Thickness(10, 5);
        top.CloseMenu = () => top.CloseSub();
        _bar.Children.Add(top);
        return this;
    }

    public IReadOnlyList<Control> Items => _bar.Children.Count == 0 ? new List<Control>() : _bar.VisualChildren;
}

/// <summary>Attaches a context menu that opens on press-and-hold (the pointer has one button).</summary>
public static class ContextMenus
{
    public static T WithContextMenu<T>(this T c, MenuFlyout menu) where T : Control
    {
        c.Hold += (_, e) => { e.Handled = true; menu.ShowAt(c, PlacementMode.Pointer, new Point(e.X, e.Y)); };
        return c;
    }
}

/// <summary>A hover hint (Avalonia ToolTip). Attach with <see cref="SetTip"/>.</summary>
public sealed class ToolTip : ContentControl
{
    public static readonly StyledProperty<object?> TipProperty = UIProperties.Register<object?>("ToolTip.Tip", null);
    public static void SetTip(Control c, object? tip) => c.SetValue(TipProperty, tip);
    public static object? GetTip(Control c) => c.GetValue(TipProperty);
    public ToolTip() { IsHitTestVisible = false; MaxWidth = 280; }

    protected override Control? MakePresenter(object? content)
    {
        var p = base.MakePresenter(content);
        if (p is TextBlock t) t.TextWrapping = TextWrapping.Wrap;
        return p;
    }
}

public sealed class Notification
{
    public string Title { get; }
    public string Message { get; }
    public NotificationType Type { get; }
    public int DurationMs { get; }
    public Notification(string title, string message, NotificationType type = NotificationType.Information, int durationMs = 3500)
    {
        Title = title; Message = message; Type = type; DurationMs = durationMs;
    }
}

public sealed class NotificationCard : Control
{
    internal Notification N = null!;

    protected override Size MeasureOverride(Size a)
    {
        int w = Width > 0 ? Width : 260;
        var lines = TextBlock.Wrap(N.Message, Math.Max(1, (w - Padding.Horizontal - 30) / Fonts.GlyphWidth(0)), true);
        return new Size(w, Padding.Vertical + 18 + lines.Count * 18);
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        int c = N.Type == NotificationType.Success ? Theme.Accent : N.Type == NotificationType.Warning ? Theme.Sun
              : N.Type == NotificationType.Error ? Theme.Danger : Theme.Info;
        dc.FillRoundRect(new Rect(Bounds.X + 4, Bounds.Y + 10, 3, Bounds.Height - 20), 1, c);
        var kind = N.Type == NotificationType.Success ? IconKind.Check : N.Type == NotificationType.Information ? IconKind.Info : IconKind.Warning;
        Icon.Draw(dc, kind, new Rect(r.X, r.Y + 1, 16, 16), c);
        dc.Text(r.X + 26, r.Y, N.Title, Theme.Text, 1);
        int y = r.Y + 20;
        foreach (var l in TextBlock.Wrap(N.Message, Math.Max(1, (r.Width - 26) / Fonts.GlyphWidth(0)), true))
        {
            dc.Text(r.X + 26, y, l, Theme.TextMuted, 0);
            y += 18;
        }
    }
}

/// <summary>Toasts in the bottom-right corner (Avalonia WindowNotificationManager).</summary>
public sealed class WindowNotificationManager
{
    readonly Window _w;
    readonly List<Popup> _open = new List<Popup>();
    public int MaxItems { get; set; } = 3;

    public WindowNotificationManager(Window w) { _w = w; }

    public void Show(Notification n)
    {
        var card = new NotificationCard { N = n };
        var p = new Popup(card) { IsLightDismiss = false, Placement = PlacementMode.Pointer };
        _open.Add(p);
        if (_open.Count > MaxItems) { _open[0].Close(); _open.RemoveAt(0); }
        p.Open(_w);
        card.PointerPressed += (_, e) => { p.Close(); e.Handled = true; };
        p.Closed += () => { _open.Remove(p); Stack(); };
        Stack();
        DispatcherTimer.RunOnce(n.DurationMs, () => p.Close());
    }

    void Stack()
    {
        int y = _w.ClientHeight - 12;
        for (int i = _open.Count - 1; i >= 0; i--)
        {
            var c = _open[i].Child;
            c.Measure(Size.Unbounded);
            y -= c.DesiredSize.Height;
            _open[i].PointerPosition = new Point(_w.ClientWidth - c.DesiredSize.Width - 12 - 8, y - 16);
            y -= 8;
        }
        _w.InvalidateLayout();
    }
}

/// <summary>A modal dialog with a dimmed backdrop (FluentAvalonia ContentDialog).</summary>
public sealed class Dialog : Control
{
    readonly Border _card = new Border();
    Popup? _popup;

    Dialog(string title, Control body, string primary, string? secondary, Action<bool> done)
    {
        _card.Classes.Add("card");
        _card.BoxShadow = new BoxShadow(0, 8, 18, 0x000000, 140);
        _card.MaxWidth = 420;
        _card.MinWidth = 300;
        _card.HorizontalAlignment = HorizontalAlignment.Center;
        _card.VerticalAlignment = VerticalAlignment.Center;
        _card.Padding = new Thickness(20);
        var stack = new StackPanel { Spacing = 14 };
        stack.Children.Add(new TextBlock(title).WithClass("h1"));
        stack.Children.Add(body);
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
        if (secondary != null) buttons.Children.Add(new Button(secondary, _ => { Close(); done(false); }) { IsCancel = true });
        buttons.Children.Add(new Button(primary, _ => { Close(); done(true); }) { IsDefault = true }.WithClass("accent"));
        stack.Children.Add(buttons);
        _card.Child = stack;
        AddVisualChild(_card);
    }

    public static Dialog Show(Window w, string title, string message, string primary = "OK", string? secondary = null, Action<bool>? done = null)
    {
        var body = new TextBlock(message) { TextWrapping = TextWrapping.Wrap };
        body.Classes.Add("muted");
        return Show(w, title, body, primary, secondary, done);
    }

    public static Dialog Show(Window w, string title, Control body, string primary, string? secondary, Action<bool>? done)
    {
        var d = new Dialog(title, body, primary, secondary, done ?? (_ => { }));
        d._popup = new Popup(d) { IsLightDismiss = false, Placement = PlacementMode.Center };
        d.Width = w.ClientWidth;
        d.Height = w.ClientHeight;
        d._popup.Open(w);
        w.UpdateLayout();
        w.MoveFocus();
        return d;
    }

    public void Close() => _popup?.Close();

    protected override void ArrangeOverride(Rect b) => _card.Arrange(b);

    public override void Render(DrawingContext dc) => dc.BlendRoundRect(Bounds, 0, Theme.Overlay, 130);

    protected internal override void OnPointerPressed(PointerEventArgs e) => e.Handled = true;
}

/// <summary>A civil date without DateTime arithmetic (proleptic Gregorian).</summary>
public struct Date
{
    public int Year, Month, Day;
    public Date(int y, int m, int d) { Year = y; Month = m; Day = d; }

    public static Date Today
    {
        get { var n = DateTime.Now; return new Date(n.Year, n.Month, n.Day); }
    }

    public static bool IsLeap(int y) => (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    public static int DaysIn(int y, int m) => m == 2 ? (IsLeap(y) ? 29 : 28) : (m == 4 || m == 6 || m == 9 || m == 11) ? 30 : 31;

    /// <summary>0 = Monday ... 6 = Sunday.</summary>
    public int DayOfWeek
    {
        get
        {
            int y = Year, m = Month;
            if (m < 3) { m += 12; y--; }
            int h = (Day + 13 * (m + 1) / 5 + y + y / 4 - y / 100 + y / 400) % 7; // 0 = Saturday
            return (h + 5) % 7;
        }
    }

    public Date AddMonths(int n)
    {
        int m = Month - 1 + n;
        int y = Year + (m >= 0 ? m / 12 : (m - 11) / 12);
        m = ((m % 12) + 12) % 12 + 1;
        return new Date(y, m, Math.Min(Day, DaysIn(y, m)));
    }

    public bool SameDay(Date o) => o.Year == Year && o.Month == Month && o.Day == Day;

    public static readonly string[] MonthNames = { "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember" };
    public override string ToString() => Day.ToString("00") + "/" + Month.ToString("00") + "/" + Year;
    public string LongText => Day + " " + MonthNames[Month - 1] + " " + Year;
}

public sealed class CalendarDayButton : Button
{
    internal Date Date;
    public CalendarDayButton(int day) : base(day.ToString()) { Focusable = false; Transitions = null; }
}

/// <summary>A month view to pick a date (Avalonia Calendar).</summary>
public sealed class Calendar : Control
{
    readonly TextBlock _title = new TextBlock { FontWeight = FontWeight.Bold, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
    readonly UniformGrid _days = new UniformGrid { Columns = 7, Spacing = 2 };
    readonly Grid _root = new Grid { RowSpacing = 6 };
    Date _display;
    Date? _selected;
    public event Action<Calendar>? SelectedDateChanged;

    public Calendar()
    {
        _display = Date.Today;
        _root.RowDefinitions = "Auto,Auto,*";
        _root.ColumnDefinitions = "Auto,*,Auto";
        var prev = new Button(new Icon(IconKind.ChevronLeft, 12)) { Padding = new Thickness(8, 6) }.WithClass("subtle");
        var next = new Button(new Icon(IconKind.ChevronRight, 12)) { Padding = new Thickness(8, 6) }.WithClass("subtle");
        prev.Click += _ => { _display = _display.AddMonths(-1); Build(); };
        next.Click += _ => { _display = _display.AddMonths(1); Build(); };
        _root.Children.Add(prev.Cell(0, 0));
        _root.Children.Add(_title.Cell(0, 1));
        _root.Children.Add(next.Cell(0, 2));
        var heads = new UniformGrid { Columns = 7, Spacing = 2 };
        foreach (var d in new[] { "Sn", "Sl", "Rb", "Km", "Jm", "Sb", "Mg" })
            heads.Children.Add(new TextBlock(d) { HorizontalAlignment = HorizontalAlignment.Center }.WithClass("faint"));
        _root.Children.Add(heads.Row(1).Col(0, 3));
        _root.Children.Add(_days.Row(2).Col(0, 3));
        AddVisualChild(_root);
        Focusable = true;
        Build();
    }

    public Date DisplayDate { get => _display; set { _display = value; Build(); } }
    public Date? SelectedDate
    {
        get => _selected;
        set { _selected = value; if (value.HasValue) _display = value.Value; Build(); SelectedDateChanged?.Invoke(this); }
    }

    void Build()
    {
        _title.Text = Date.MonthNames[_display.Month - 1] + " " + _display.Year;
        _days.Children.Clear();
        var first = new Date(_display.Year, _display.Month, 1);
        int lead = first.DayOfWeek;
        var prevMonth = _display.AddMonths(-1);
        int prevDays = Date.DaysIn(prevMonth.Year, prevMonth.Month);
        var today = Date.Today;
        for (int i = 0; i < 42; i++)
        {
            Date d;
            bool other = false;
            if (i < lead) { d = new Date(prevMonth.Year, prevMonth.Month, prevDays - lead + 1 + i); other = true; }
            else if (i - lead < Date.DaysIn(_display.Year, _display.Month)) d = new Date(_display.Year, _display.Month, i - lead + 1);
            else { var nm = _display.AddMonths(1); d = new Date(nm.Year, nm.Month, i - lead - Date.DaysIn(_display.Year, _display.Month) + 1); other = true; }
            var b = new CalendarDayButton(d.Day) { Date = d };
            b.Classes.Add("subtle");
            if (other) b.Classes.Add("other");
            if (d.SameDay(today)) b.Classes.Add("today");
            if (_selected.HasValue && d.SameDay(_selected.Value)) b.SetPseudo(":selected", true);
            b.Click += x => SelectedDate = ((CalendarDayButton)x).Date;
            _days.Children.Add(b);
        }
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.PageUp) { DisplayDate = _display.AddMonths(-1); e.Handled = true; }
        else if (e.Key == Keys.PageDown) { DisplayDate = _display.AddMonths(1); e.Handled = true; }
        base.OnKeyDown(e);
    }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(new Size(Math.Min(a.Width, 300), a.Height));
        return new Size(Math.Max(s.Width, 260), s.Height);
    }
}

/// <summary>A date field that opens a calendar flyout (Avalonia CalendarDatePicker).</summary>
public sealed class CalendarDatePicker : Control
{
    readonly Calendar _cal = new Calendar();
    readonly Flyout _flyout;
    public string Watermark { get; set; } = "Pilih tanggal";
    public event Action<CalendarDatePicker>? SelectedDateChanged;

    public CalendarDatePicker()
    {
        Focusable = true;
        _flyout = new Flyout(_cal);
        _cal.SelectedDateChanged += c => { _flyout.Hide(); InvalidateVisual(); SelectedDateChanged?.Invoke(this); };
    }

    public Date? SelectedDate { get => _cal.SelectedDate; set => _cal.SelectedDate = value; }
    public Calendar Calendar => _cal;

    protected override Size MeasureOverride(Size a) =>
        new Size(Math.Max(Fonts.Measure(Watermark, 0), Fonts.Measure("00/00/0000", 0)) + Padding.Horizontal + 30, Fonts.LineHeight(0) + Padding.Vertical + 2);

    protected internal override void OnPointerPressed(PointerEventArgs e) { if (_flyout.IsOpen) _flyout.Hide(); else _flyout.ShowAt(this); e.Handled = true; }
    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.Enter || e.Key == Keys.Space || e.Key == Keys.Down) { _flyout.ShowAt(this); e.Handled = true; }
        base.OnKeyDown(e);
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        var sel = SelectedDate;
        dc.Text(r.X, r.Y + (r.Height - Fonts.LineHeight(0)) / 2, sel.HasValue ? sel.Value.ToString() : Watermark, sel.HasValue ? Foreground : Theme.TextFaint, 0);
        Icon.Draw(dc, IconKind.Grid, new Rect(r.Right - 14, r.Y + r.Height / 2 - 7, 14, 14), Theme.TextMuted);
    }
}
