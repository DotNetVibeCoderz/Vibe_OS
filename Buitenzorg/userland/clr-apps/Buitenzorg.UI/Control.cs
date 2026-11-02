// Buitenzorg.UI -- the Control base class: visual tree, styled properties,
// Measure/Arrange layout (margin, alignment, explicit and min/max sizes),
// styles + pseudo-classes + transitions, rendering, hit-testing and the
// input hooks (pointer, keyboard, focus) that the Window routes.

namespace Buitenzorg.UI;

public class PointerEventArgs
{
    public int X { get; internal set; }
    public int Y { get; internal set; }
    public bool LeftButton { get; internal set; }
    public bool Handled { get; set; }
    public Control? Source { get; internal set; }
}

public class KeyEventArgs
{
    public char Key { get; internal set; }
    public bool Handled { get; set; }
    public Control? Source { get; internal set; }
}

public class Control : UIObject
{
    // ---- layout
    public static readonly StyledProperty<Thickness> MarginProperty = UIProperties.Register("Margin", new Thickness(0), affectsMeasure: true);
    public static readonly StyledProperty<Thickness> PaddingProperty = UIProperties.Register("Padding", new Thickness(0), affectsMeasure: true);
    public static readonly StyledProperty<int> WidthProperty = UIProperties.Register("Width", -1, affectsMeasure: true);
    public static readonly StyledProperty<int> HeightProperty = UIProperties.Register("Height", -1, affectsMeasure: true);
    public static readonly StyledProperty<int> MinWidthProperty = UIProperties.Register("MinWidth", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> MinHeightProperty = UIProperties.Register("MinHeight", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> MaxWidthProperty = UIProperties.Register("MaxWidth", Size.Infinite, affectsMeasure: true);
    public static readonly StyledProperty<int> MaxHeightProperty = UIProperties.Register("MaxHeight", Size.Infinite, affectsMeasure: true);
    public static readonly StyledProperty<HorizontalAlignment> HorizontalAlignmentProperty =
        UIProperties.Register("HorizontalAlignment", HorizontalAlignment.Stretch, affectsMeasure: true);
    public static readonly StyledProperty<VerticalAlignment> VerticalAlignmentProperty =
        UIProperties.Register("VerticalAlignment", VerticalAlignment.Stretch, affectsMeasure: true);
    public static readonly StyledProperty<bool> IsVisibleProperty = UIProperties.Register("IsVisible", true, affectsMeasure: true);
    public static readonly StyledProperty<bool> IsEnabledProperty = UIProperties.Register("IsEnabled", true);

    // ---- appearance
    public static readonly StyledProperty<int> BackgroundProperty = UIProperties.Register("Background", Color.Transparent);
    /// <summary>Bottom color of a vertical gradient background (Transparent = solid).</summary>
    public static readonly StyledProperty<int> BackgroundBottomProperty = UIProperties.Register("BackgroundBottom", Color.Transparent);
    public static readonly StyledProperty<int> BorderBrushProperty = UIProperties.Register("BorderBrush", Color.Transparent);
    public static readonly StyledProperty<int> BorderThicknessProperty = UIProperties.Register("BorderThickness", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> CornerRadiusProperty = UIProperties.Register("CornerRadius", 0);
    public static readonly StyledProperty<BoxShadow> BoxShadowProperty = UIProperties.Register("BoxShadow", BoxShadow.None);
    public static readonly StyledProperty<int> ForegroundProperty = UIProperties.Register("Foreground", 0xE8F1EA, inherits: true);
    public static readonly StyledProperty<FontWeight> FontWeightProperty = UIProperties.Register("FontWeight", FontWeight.Normal, inherits: true, affectsMeasure: true);
    public static readonly StyledProperty<int> FontSizeProperty = UIProperties.Register("FontSize", 16, inherits: true, affectsMeasure: true);

    readonly List<Control> _children = new List<Control>();
    Classes? _classes;
    Classes? _pseudo;
    object? _dataContext;
    bool _hasDataContext;
    Window? _root;
    string[]? _typeChain;

    public Control? Parent { get; private set; }
    public string? Name { get; set; }
    public object? Tag { get; set; }
    public bool Focusable { get; set; }
    public bool IsHitTestVisible { get; set; } = true;
    public bool ClipToBounds { get; set; }
    public Styles? Styles { get; set; }
    public List<Transition>? Transitions { get; set; }

    /// <summary>The arranged rectangle in window client coordinates.</summary>
    public Rect Bounds { get; private set; }
    public Size DesiredSize { get; private set; }

    public event Action<Control, PointerEventArgs>? PointerPressed;
    public event Action<Control, PointerEventArgs>? PointerReleased;
    public event Action<Control, PointerEventArgs>? PointerMoved;
    public event Action<Control>? PointerEntered;
    public event Action<Control>? PointerExited;
    public event Action<Control, KeyEventArgs>? KeyDown;
    public event Action<Control>? GotFocus;
    public event Action<Control>? LostFocus;
    public event Action? DataContextChanged;
    public event Action<Control>? AttachedToWindow;

    public Classes Classes => _classes ??= new Classes(this);
    public Classes PseudoClasses => _pseudo ??= new Classes(this);

    internal override UIObject? InheritanceParent => Parent;
    internal List<Control> VisualChildren => _children;
    public Window? Window => _root;
    public bool IsAttached => _root != null;

    // ---- property shortcuts
    public Thickness Margin { get => GetValue(MarginProperty); set => SetValue(MarginProperty, value); }
    public Thickness Padding { get => GetValue(PaddingProperty); set => SetValue(PaddingProperty, value); }
    public int Width { get => GetValue(WidthProperty); set => SetValue(WidthProperty, value); }
    public int Height { get => GetValue(HeightProperty); set => SetValue(HeightProperty, value); }
    public int MinWidth { get => GetValue(MinWidthProperty); set => SetValue(MinWidthProperty, value); }
    public int MinHeight { get => GetValue(MinHeightProperty); set => SetValue(MinHeightProperty, value); }
    public int MaxWidth { get => GetValue(MaxWidthProperty); set => SetValue(MaxWidthProperty, value); }
    public int MaxHeight { get => GetValue(MaxHeightProperty); set => SetValue(MaxHeightProperty, value); }
    public HorizontalAlignment HorizontalAlignment { get => GetValue(HorizontalAlignmentProperty); set => SetValue(HorizontalAlignmentProperty, value); }
    public VerticalAlignment VerticalAlignment { get => GetValue(VerticalAlignmentProperty); set => SetValue(VerticalAlignmentProperty, value); }
    public bool IsVisible { get => GetValue(IsVisibleProperty); set => SetValue(IsVisibleProperty, value); }
    public bool IsEnabled { get => GetValue(IsEnabledProperty); set => SetValue(IsEnabledProperty, value); }
    public int Background { get => GetValue(BackgroundProperty); set => SetValue(BackgroundProperty, value); }
    public int BackgroundBottom { get => GetValue(BackgroundBottomProperty); set => SetValue(BackgroundBottomProperty, value); }
    public int BorderBrush { get => GetValue(BorderBrushProperty); set => SetValue(BorderBrushProperty, value); }
    public int BorderThickness { get => GetValue(BorderThicknessProperty); set => SetValue(BorderThicknessProperty, value); }
    public int CornerRadius { get => GetValue(CornerRadiusProperty); set => SetValue(CornerRadiusProperty, value); }
    public BoxShadow BoxShadow { get => GetValue(BoxShadowProperty); set => SetValue(BoxShadowProperty, value); }
    public int Foreground { get => GetValue(ForegroundProperty); set => SetValue(ForegroundProperty, value); }
    public FontWeight FontWeight { get => GetValue(FontWeightProperty); set => SetValue(FontWeightProperty, value); }
    public int FontSize { get => GetValue(FontSizeProperty); set => SetValue(FontSizeProperty, value); }

    /// <summary>Kernel text style for this control's font (0 body, 1 strong, 2 heading).</summary>
    public int TextStyle => Fonts.StyleOf(FontWeight, FontSize);

    public object? DataContext
    {
        get => _hasDataContext ? _dataContext : Parent?.DataContext;
        set
        {
            _dataContext = value;
            _hasDataContext = true;
            RaiseDataContextChanged();
        }
    }

    void RaiseDataContextChanged()
    {
        DataContextChanged?.Invoke();
        foreach (var c in _children)
            if (!c._hasDataContext) c.RaiseDataContextChanged();
    }

    public bool IsEffectivelyEnabled => IsEnabled && (Parent == null || Parent.IsEffectivelyEnabled);
    public bool IsPointerOver => PseudoClasses.Contains(":pointerover");
    public bool IsPressed => PseudoClasses.Contains(":pressed");
    public bool IsFocused => _root != null && _root.FocusedControl == this;

    // ---- tree
    protected internal void AddVisualChild(Control c, int index = -1)
    {
        if (c.Parent != null) c.Parent.RemoveVisualChild(c);
        c.Parent = this;
        InheritEpoch++;
        if (index < 0 || index > _children.Count) _children.Add(c); else _children.Insert(index, c);
        if (_root != null) c.Attach(_root);
        if (!c._hasDataContext && DataContext != null) c.RaiseDataContextChanged();
        InvalidateMeasure();
    }

    protected internal void RemoveVisualChild(Control c)
    {
        if (!_children.Remove(c)) return;
        if (c._root != null) c.Detach();
        c.Parent = null;
        InheritEpoch++;
        InvalidateMeasure();
    }

    protected internal void ClearVisualChildren()
    {
        foreach (var c in _children)
        {
            if (c._root != null) c.Detach();
            c.Parent = null;
        }
        _children.Clear();
        InvalidateMeasure();
    }

    internal void Attach(Window w)
    {
        _root = w;
        _measureValid = false;
        _arrangeValid = false;
        ApplyStyles(false);
        foreach (var c in _children) c.Attach(w);
        OnAttached();
        AttachedToWindow?.Invoke(this);
    }

    internal void Detach()
    {
        _root?.OnControlDetached(this);
        foreach (var c in _children) c.Detach();
        _root = null;
    }

    protected virtual void OnAttached() { }

    public IEnumerable<Control> Descendants()
    {
        foreach (var c in _children)
        {
            yield return c;
            foreach (var d in c.Descendants()) yield return d;
        }
    }

    public T? FindControl<T>(string name) where T : Control
    {
        foreach (var d in Descendants())
            if (d.Name == name && d is T) return (T)d;
        return null;
    }

    public T? FindAncestor<T>() where T : Control
    {
        for (var p = Parent; p != null; p = p.Parent) if (p is T) return (T)p;
        return null;
    }

    // ---- styling
    static readonly Dictionary<Type, string[]> _chains = new Dictionary<Type, string[]>();
    static readonly Dictionary<Type, List<StyleCandidate>> _candidates = new Dictionary<Type, List<StyleCandidate>>();
    static int _candidatesVersion = -1;
    List<UIProperty>? _styled;
    internal int StyleVersion = -1;

    string[] TypeChain()
    {
        if (_typeChain != null) return _typeChain;
        var type = GetType();
        if (!_chains.TryGetValue(type, out var chain))
        {
            var names = new List<string>();
            for (var t = type; t != null && t != typeof(UIObject); t = t.BaseType) names.Add(t.Name);
            chain = names.ToArray();
            _chains[type] = chain;
        }
        _typeChain = chain;
        return chain;
    }

    /// <summary>The control's own type name (what an exact selector matches).</summary>
    internal string TypeName => TypeChain()[0];

    internal bool IsOfType(string name)
    {
        foreach (var n in TypeChain())
            if (n == name) return true;
        return false;
    }

    /// <summary>The application styles that can match this type, each with only the selector
    /// alternatives whose type part matches (cached per type and style-set version).</summary>
    List<StyleCandidate> Candidates(Styles all)
    {
        if (_candidatesVersion != Styles.Version) { _candidates.Clear(); _candidatesVersion = Styles.Version; }
        var type = GetType();
        if (_candidates.TryGetValue(type, out var list)) return list;
        list = new List<StyleCandidate>();
        foreach (var st in all)
        {
            List<Selector>? sels = null;
            foreach (var sel in st.Selectors)
                if (sel.CouldMatchType(this)) (sels ??= new List<Selector>()).Add(sel);
            if (sels != null) list.Add(new StyleCandidate(st, sels.ToArray()));
        }
        _candidates[type] = list;
        return list;
    }

    public void InvalidateStyle()
    {
        if (_root != null) ApplyStyles(true);
    }

    internal void SetPseudo(string name, bool on)
    {
        if (PseudoClasses.Contains(name) == on) return;
        PseudoClasses.Set(name, on);
    }

    /// <summary>Recompute this control's style layer from the application's and its ancestors' styles.</summary>
    internal void ApplyStyles(bool animate)
    {
        StyleVersion = Styles.Version;
        var props = new List<UIProperty>();
        var vals = new List<object?>();
        var app = Application.Current;
        if (app != null)
            foreach (var cand in Candidates(app.Styles))
                if (cand.Matches(this)) AddSetters(cand.Style, props, vals);
        // local styles of the ancestors, outermost first
        List<Control>? chain = null;
        for (var p = this; p != null; p = p.Parent)
            if (p.Styles != null) (chain ??= new List<Control>()).Add(p);
        if (chain != null)
            for (int i = chain.Count - 1; i >= 0; i--)
                foreach (var st in chain[i].Styles!)
                    if (st.Matches(this)) AddSetters(st, props, vals);

        // properties no style sets any more fall back to their default or inherited value
        if (_styled != null)
            foreach (var old in _styled)
                if (IndexOf(props, old) < 0)
                {
                    object? before = GetBoxed(old);
                    SetStyle(old, null, false);
                    Changed(old, before);
                }
        for (int i = 0; i < props.Count; i++)
        {
            var p = props[i];
            object? oldEffective = GetBoxed(p);
            SetStyle(p, vals[i], true);
            Transition? tr = animate ? FindTransition(p) : null;
            if (tr != null && !IsSet(p) && oldEffective is int from && vals[i] is int to && from != to)
                Animator.Start(this, p, from, to, tr.DurationMs, p.Name.Contains("Background") || p.Name.Contains("Brush") || p.Name.Contains("Foreground"));
            else
                Changed(p, oldEffective);
        }
        _styled = props;
        _root?.InvalidateVisual();
    }

    Transition? FindTransition(UIProperty p)
    {
        if (Transitions == null) return null;
        foreach (var t in Transitions)
            if (t.Property == p) return t;
        return null;
    }

    static int IndexOf(List<UIProperty> props, UIProperty p)
    {
        for (int i = 0; i < props.Count; i++) if (ReferenceEquals(props[i], p)) return i;
        return -1;
    }

    static void AddSetters(Style s, List<UIProperty> props, List<object?> vals)
    {
        foreach (var setter in s.Setters)
        {
            int i = IndexOf(props, setter.Property);
            if (i >= 0) vals[i] = setter.Value;
            else { props.Add(setter.Property); vals.Add(setter.Value); }
        }
    }

    protected override void OnPropertyChanged(UIProperty p)
    {
        if (p == IsEnabledProperty) SetPseudo(":disabled", !IsEnabled);
        if (p.AffectsMeasure) InvalidateMeasure(); else InvalidateVisual();
    }

    // Layout caching (Avalonia's IsMeasureValid / IsArrangeValid): a control is
    // measured again only when invalidated or offered a different size, and
    // invalidation walks up to the root -- so typing into one TextBlock does
    // not re-measure the twenty buttons next to it.
    bool _measureValid, _arrangeValid;
    public static int MeasureCount, ArrangeCount;
    Size _lastAvailable;
    Rect _lastRect;

    public void InvalidateMeasure()
    {
        // stop at an ancestor that is already invalid: its ancestors are too
        for (Control? c = this; c != null; c = c.Parent)
        {
            if (!c._measureValid && !c._arrangeValid && c != this) break;
            c._measureValid = false;
            c._arrangeValid = false;
        }
        _root?.InvalidateLayout();
    }

    /// <summary>Forget every cached layout result below (and including) this control.</summary>
    internal void InvalidateSubtree()
    {
        _measureValid = false;
        _arrangeValid = false;
        foreach (var c in _children) c.InvalidateSubtree();
    }
    public void InvalidateVisual() => _root?.InvalidateVisual();

    // ---- layout
    public void Measure(Size available)
    {
        if (_measureValid && available.Width == _lastAvailable.Width && available.Height == _lastAvailable.Height) return;
        _measureValid = true;
        _lastAvailable = available;
        MeasureCount++;
        if (!IsVisible) { DesiredSize = new Size(0, 0); return; }
        var m = Margin;
        int w = Width, h = Height;
        int aw = w >= 0 ? w : available.Width - m.Horizontal;
        int ah = h >= 0 ? h : available.Height - m.Vertical;
        aw = Math.Max(0, Math.Min(Math.Max(aw, MinWidth), MaxWidth));
        ah = Math.Max(0, Math.Min(Math.Max(ah, MinHeight), MaxHeight));
        var s = MeasureOverride(new Size(aw, ah));
        int dw = w >= 0 ? w : s.Width;
        int dh = h >= 0 ? h : s.Height;
        dw = Math.Min(Math.Max(dw, MinWidth), MaxWidth);
        dh = Math.Min(Math.Max(dh, MinHeight), MaxHeight);
        DesiredSize = new Size(dw + m.Horizontal, dh + m.Vertical);
    }

    public void Arrange(Rect rect)
    {
        if (_arrangeValid && rect.X == _lastRect.X && rect.Y == _lastRect.Y && rect.Width == _lastRect.Width && rect.Height == _lastRect.Height) return;
        _arrangeValid = true;
        _lastRect = rect;
        ArrangeCount++;
        if (!IsVisible) { Bounds = new Rect(rect.X, rect.Y, 0, 0); return; }
        var inner = rect.Deflate(Margin);
        var m = Margin;
        int dw = DesiredSize.Width - m.Horizontal, dh = DesiredSize.Height - m.Vertical;
        var ha = HorizontalAlignment;
        var va = VerticalAlignment;
        int w = Width >= 0 ? Width : ha == HorizontalAlignment.Stretch ? inner.Width : Math.Min(dw, inner.Width);
        int h = Height >= 0 ? Height : va == VerticalAlignment.Stretch ? inner.Height : Math.Min(dh, inner.Height);
        w = Math.Min(Math.Max(w, MinWidth), MaxWidth);
        h = Math.Min(Math.Max(h, MinHeight), MaxHeight);
        int x = inner.X, y = inner.Y;
        if (ha == HorizontalAlignment.Center) x += (inner.Width - w) / 2;
        else if (ha == HorizontalAlignment.Right) x += inner.Width - w;
        else if (ha == HorizontalAlignment.Stretch && w < inner.Width) x += (inner.Width - w) / 2;
        if (va == VerticalAlignment.Center) y += (inner.Height - h) / 2;
        else if (va == VerticalAlignment.Bottom) y += inner.Height - h;
        else if (va == VerticalAlignment.Stretch && h < inner.Height) y += (inner.Height - h) / 2;
        Bounds = new Rect(x, y, w, h);
        ArrangeOverride(Bounds);
    }

    /// <summary>The content box: bounds minus border and padding.</summary>
    protected Rect ContentRect(Rect bounds) => bounds.Deflate(new Thickness(BorderThickness)).Deflate(Padding);

    protected virtual Size MeasureOverride(Size available)
    {
        var chrome = Padding;
        int b = BorderThickness;
        var inner = new Size(Math.Max(0, available.Width - chrome.Horizontal - 2 * b), Math.Max(0, available.Height - chrome.Vertical - 2 * b));
        int w = 0, h = 0;
        foreach (var c in _children)
        {
            c.Measure(inner);
            w = Math.Max(w, c.DesiredSize.Width);
            h = Math.Max(h, c.DesiredSize.Height);
        }
        return new Size(w + chrome.Horizontal + 2 * b, h + chrome.Vertical + 2 * b);
    }

    protected virtual void ArrangeOverride(Rect bounds)
    {
        var inner = ContentRect(bounds);
        foreach (var c in _children) c.Arrange(inner);
    }

    // ---- rendering
    internal void RenderTree(DrawingContext dc)
    {
        if (!IsVisible || Bounds.IsEmpty) return;
        var shadowPad = BoxShadow.IsEmpty ? 0 : BoxShadow.Blur + Math.Max(Math.Abs(BoxShadow.OffsetX), Math.Abs(BoxShadow.OffsetY));
        if (dc.Clip.Intersect(Bounds.Inflate(shadowPad)).IsEmpty) return;
        Render(dc);
        if (ClipToBounds) dc.PushClip(ContentClip());
        RenderChildren(dc);
        if (ClipToBounds) dc.PopClip();
        RenderOverlay(dc);
    }

    protected virtual Rect ContentClip() => Bounds;

    protected virtual void RenderChildren(DrawingContext dc)
    {
        foreach (var c in _children) c.RenderTree(dc);
    }

    /// <summary>Draws the control itself (default: shadow, background, border).</summary>
    public virtual void Render(DrawingContext dc) => RenderChrome(dc, Bounds);

    protected void RenderChrome(DrawingContext dc, Rect r)
    {
        int radius = CornerRadius;
        dc.Shadow(r, radius, BoxShadow);
        int bg = Background;
        int bt = BorderThickness, bb = BorderBrush;
        if (bt > 0 && bb >= 0 && bg >= 0 && radius > 0)
        {
            // border as an outer rounded fill, background inset: crisp at any radius
            dc.FillRoundRect(r, radius, bb);
            dc.FillGradient(r.Inflate(-bt), Math.Max(0, radius - bt), bg, BackgroundBottom);
            return;
        }
        dc.FillGradient(r, radius, bg, BackgroundBottom);
        if (bt > 0) dc.StrokeRoundRect(r, radius, bb, bt);
    }

    /// <summary>Drawn after the children (adorners).</summary>
    protected virtual void RenderOverlay(DrawingContext dc) { }

    // ---- hit testing
    public virtual Control? HitTest(int x, int y)
    {
        if (!IsVisible || !Bounds.Contains(x, y)) return null;
        for (int i = _children.Count - 1; i >= 0; i--)
        {
            var h = _children[i].HitTest(x, y);
            if (h != null) return h;
        }
        return IsHitTestVisible ? this : null;
    }

    // ---- input (the Window calls these; they bubble to the parent unless handled)
    protected internal virtual void OnPointerPressed(PointerEventArgs e) => PointerPressed?.Invoke(this, e);
    protected internal virtual void OnPointerReleased(PointerEventArgs e) => PointerReleased?.Invoke(this, e);
    protected internal virtual void OnPointerMoved(PointerEventArgs e) => PointerMoved?.Invoke(this, e);
    protected internal virtual void OnPointerEntered() => PointerEntered?.Invoke(this);
    protected internal virtual void OnPointerExited() => PointerExited?.Invoke(this);
    protected internal virtual void OnKeyDown(KeyEventArgs e) => KeyDown?.Invoke(this, e);
    protected internal virtual void OnGotFocus() { SetPseudo(":focus", true); GotFocus?.Invoke(this); }
    protected internal virtual void OnLostFocus() { SetPseudo(":focus", false); SetPseudo(":focus-visible", false); LostFocus?.Invoke(this); }
    /// <summary>A long press (600 ms without moving) -- opens the context menu.</summary>
    protected internal virtual void OnHold(PointerEventArgs e) => Hold?.Invoke(this, e);
    /// <summary>Press-and-hold (the context-menu gesture).</summary>
    public event Action<Control, PointerEventArgs>? Hold;

    public bool Focus()
    {
        if (_root == null || !Focusable || !IsEffectivelyEnabled || !IsVisible) return false;
        _root.SetFocus(this, false);
        return true;
    }
}
