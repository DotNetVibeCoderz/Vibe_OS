// Buitenzorg.UI -- panels and content hosts (Avalonia.Controls layout):
// Panel/StackPanel/Grid/DockPanel/WrapPanel/UniformGrid/Canvas, Border,
// ContentControl, ScrollViewer, SplitView, GridSplitter, and fluent helpers
// for the attached properties (Grid.Row, DockPanel.Dock, Canvas.Left, ...).

using System.Collections;

namespace Buitenzorg.UI;

/// <summary>The children of a panel (collection-initializable).</summary>
public sealed class ControlList : IEnumerable<Control>
{
    readonly Control _owner;
    internal ControlList(Control owner) { _owner = owner; }

    public int Count => _owner.VisualChildren.Count;
    public Control this[int i] => _owner.VisualChildren[i];
    public void Add(Control c) => _owner.AddVisualChild(c);
    public void Insert(int index, Control c) => _owner.AddVisualChild(c, index);
    public void Remove(Control c) => _owner.RemoveVisualChild(c);
    public void Clear() => _owner.ClearVisualChildren();
    public int IndexOf(Control c) => _owner.VisualChildren.IndexOf(c);
    public void AddRange(IEnumerable<Control> cs) { foreach (var c in cs) Add(c); }
    public IEnumerator<Control> GetEnumerator() => _owner.VisualChildren.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => _owner.VisualChildren.GetEnumerator();
}

public class Panel : Control
{
    public ControlList Children { get; }
    public Panel() { Children = new ControlList(this); }
}

public class StackPanel : Panel
{
    public static readonly StyledProperty<Orientation> OrientationProperty = UIProperties.Register("Orientation", Orientation.Vertical, affectsMeasure: true);
    public static readonly StyledProperty<int> SpacingProperty = UIProperties.Register("Spacing", 0, affectsMeasure: true);
    public Orientation Orientation { get => GetValue(OrientationProperty); set => SetValue(OrientationProperty, value); }
    public int Spacing { get => GetValue(SpacingProperty); set => SetValue(SpacingProperty, value); }

    protected override Size MeasureOverride(Size a)
    {
        bool h = Orientation == Orientation.Horizontal;
        var pad = Padding;
        int along = 0, across = 0, n = 0;
        var avail = h ? new Size(Size.Infinite, a.Height - pad.Vertical) : new Size(a.Width - pad.Horizontal, Size.Infinite);
        foreach (var c in VisualChildren)
        {
            c.Measure(avail);
            if (!c.IsVisible) continue;
            along += h ? c.DesiredSize.Width : c.DesiredSize.Height;
            across = Math.Max(across, h ? c.DesiredSize.Height : c.DesiredSize.Width);
            n++;
        }
        if (n > 1) along += Spacing * (n - 1);
        return h ? new Size(along + pad.Horizontal, across + pad.Vertical) : new Size(across + pad.Horizontal, along + pad.Vertical);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = b.Deflate(Padding);
        bool h = Orientation == Orientation.Horizontal;
        int pos = h ? r.X : r.Y;
        foreach (var c in VisualChildren)
        {
            if (!c.IsVisible) { c.Arrange(new Rect(r.X, r.Y, 0, 0)); continue; }
            if (h) { c.Arrange(new Rect(pos, r.Y, c.DesiredSize.Width, r.Height)); pos += c.DesiredSize.Width + Spacing; }
            else { c.Arrange(new Rect(r.X, pos, r.Width, c.DesiredSize.Height)); pos += c.DesiredSize.Height + Spacing; }
        }
    }
}

public enum GridUnit { Pixel, Auto, Star }

public struct GridLength
{
    public int Value;
    public GridUnit Unit;
    public GridLength(int value, GridUnit unit) { Value = value; Unit = unit; }
    public static GridLength Auto => new GridLength(1, GridUnit.Auto);
    public static GridLength Star(int weight = 1) => new GridLength(weight, GridUnit.Star);
    public static GridLength Pixel(int px) => new GridLength(px, GridUnit.Pixel);

    /// <summary>"Auto", "*", "2*", "120".</summary>
    public static GridLength Parse(string s)
    {
        s = s.Trim();
        if (s == "Auto" || s == "auto") return Auto;
        if (s.EndsWith("*")) return Star(s.Length == 1 ? 1 : int.Parse(s.Substring(0, s.Length - 1)));
        return Pixel(int.Parse(s));
    }
}

public sealed class Grid : Panel
{
    public static readonly StyledProperty<int> RowProperty = UIProperties.Register("Grid.Row", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> ColumnProperty = UIProperties.Register("Grid.Column", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> RowSpanProperty = UIProperties.Register("Grid.RowSpan", 1, affectsMeasure: true);
    public static readonly StyledProperty<int> ColumnSpanProperty = UIProperties.Register("Grid.ColumnSpan", 1, affectsMeasure: true);
    public static readonly StyledProperty<int> RowSpacingProperty = UIProperties.Register("RowSpacing", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> ColumnSpacingProperty = UIProperties.Register("ColumnSpacing", 0, affectsMeasure: true);

    public List<GridLength> Columns { get; private set; } = new List<GridLength>();
    public List<GridLength> Rows { get; private set; } = new List<GridLength>();
    int[] _colW = new int[0], _rowH = new int[0];
    int[] _colX = new int[0], _rowY = new int[0];

    public int RowSpacing { get => GetValue(RowSpacingProperty); set => SetValue(RowSpacingProperty, value); }
    public int ColumnSpacing { get => GetValue(ColumnSpacingProperty); set => SetValue(ColumnSpacingProperty, value); }

    /// <summary>Column definitions as text: "Auto,*,2*,120".</summary>
    public string ColumnDefinitions { set { Columns = ParseDefs(value); InvalidateMeasure(); } }
    public string RowDefinitions { set { Rows = ParseDefs(value); InvalidateMeasure(); } }

    static List<GridLength> ParseDefs(string s)
    {
        var l = new List<GridLength>();
        foreach (var p in s.Split(',')) if (p.Trim().Length > 0) l.Add(GridLength.Parse(p));
        return l;
    }

    public static void SetRow(Control c, int v) => c.SetValue(RowProperty, v);
    public static void SetColumn(Control c, int v) => c.SetValue(ColumnProperty, v);
    public static void SetRowSpan(Control c, int v) => c.SetValue(RowSpanProperty, v);
    public static void SetColumnSpan(Control c, int v) => c.SetValue(ColumnSpanProperty, v);

    /// <summary>The actual width of a column after the last layout.</summary>
    public int ActualColumnWidth(int i) => i < _colW.Length ? _colW[i] : 0;

    internal void ResizeColumn(int i, int width)
    {
        if (i < 0 || i >= Columns.Count) return;
        Columns[i] = GridLength.Pixel(Math.Max(24, width));
        InvalidateMeasure();
    }

    List<GridLength> Defs(bool cols)
    {
        var d = cols ? Columns : Rows;
        return d.Count == 0 ? new List<GridLength> { GridLength.Star() } : d;
    }

    static int Clamp(int v, int max) => Math.Max(0, Math.Min(v, max - 1));

    int[] Solve(bool cols, int available, bool measure, int[] across)
    {
        var defs = Defs(cols);
        int n = defs.Count;
        var size = new int[n];
        int spacing = (cols ? ColumnSpacing : RowSpacing) * Math.Max(0, n - 1);
        int fixedSum = 0, stars = 0;
        for (int i = 0; i < n; i++)
        {
            if (defs[i].Unit == GridUnit.Pixel) size[i] = defs[i].Value;
            else if (defs[i].Unit == GridUnit.Star) stars += defs[i].Value;
        }
        // Auto (and, when measuring unbounded, Star) take their largest single-span child
        bool unbounded = available >= Size.Infinite / 2;
        foreach (var c in VisualChildren)
        {
            if (!c.IsVisible) continue;
            int idx = Clamp(c.GetValue(cols ? ColumnProperty : RowProperty), n);
            int span = c.GetValue(cols ? ColumnSpanProperty : RowSpanProperty);
            if (span != 1) continue;
            var u = defs[idx].Unit;
            if (u == GridUnit.Auto || (u == GridUnit.Star && (unbounded || measure)))
            {
                if (measure) MeasureChild(c, cols, across);
                int d = cols ? c.DesiredSize.Width : c.DesiredSize.Height;
                if (u == GridUnit.Auto) size[idx] = Math.Max(size[idx], d);
            }
        }
        for (int i = 0; i < n; i++) if (defs[i].Unit != GridUnit.Star) fixedSum += size[i];
        if (stars > 0)
        {
            if (unbounded)
            {
                foreach (var c in VisualChildren)
                {
                    int idx = Clamp(c.GetValue(cols ? ColumnProperty : RowProperty), n);
                    if (defs[idx].Unit == GridUnit.Star && c.GetValue(cols ? ColumnSpanProperty : RowSpanProperty) == 1)
                        size[idx] = Math.Max(size[idx], cols ? c.DesiredSize.Width : c.DesiredSize.Height);
                }
            }
            else
            {
                int rest = Math.Max(0, available - fixedSum - spacing);
                int given = 0, last = -1;
                for (int i = 0; i < n; i++)
                    if (defs[i].Unit == GridUnit.Star) { size[i] = rest * defs[i].Value / stars; given += size[i]; last = i; }
                if (last >= 0) size[last] += rest - given;
            }
        }
        return size;
    }

    void MeasureChild(Control c, bool colsPass, int[] colW)
    {
        // during the column pass children get unbounded width; during the row pass their column width
        int w = Size.Infinite;
        if (!colsPass && colW.Length > 0)
        {
            int ci = Clamp(c.GetValue(ColumnProperty), colW.Length);
            int span = Math.Max(1, c.GetValue(ColumnSpanProperty));
            w = 0;
            for (int i = ci; i < Math.Min(colW.Length, ci + span); i++) w += colW[i];
            w += ColumnSpacing * (Math.Min(colW.Length, ci + span) - ci - 1);
        }
        c.Measure(new Size(w, Size.Infinite));
    }

    protected override Size MeasureOverride(Size a)
    {
        var pad = Padding;
        int aw = a.Width - pad.Horizontal, ah = a.Height - pad.Vertical;
        _colW = Solve(true, aw, true, new int[0]);
        foreach (var c in VisualChildren) MeasureChild(c, false, _colW);
        _rowH = Solve(false, ah, false, _colW);
        // desired size: a star track asks for its largest child, like Auto (Avalonia)
        var cw = Solve(true, Size.Infinite, false, new int[0]);
        var rh = Solve(false, Size.Infinite, false, _colW);
        int w = 0, h = 0;
        foreach (var x in cw) w += x;
        foreach (var y in rh) h += y;
        w += ColumnSpacing * Math.Max(0, _colW.Length - 1);
        h += RowSpacing * Math.Max(0, _rowH.Length - 1);
        return new Size(w + pad.Horizontal, h + pad.Vertical);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = b.Deflate(Padding);
        _colW = Solve(true, r.Width, false, new int[0]);
        _rowH = Solve(false, r.Height, false, _colW);
        _colX = new int[_colW.Length];
        _rowY = new int[_rowH.Length];
        int x = r.X, y = r.Y;
        for (int i = 0; i < _colW.Length; i++) { _colX[i] = x; x += _colW[i] + ColumnSpacing; }
        for (int i = 0; i < _rowH.Length; i++) { _rowY[i] = y; y += _rowH[i] + RowSpacing; }
        foreach (var c in VisualChildren)
        {
            int ci = Clamp(c.GetValue(ColumnProperty), _colW.Length), ri = Clamp(c.GetValue(RowProperty), _rowH.Length);
            int ce = Math.Min(_colW.Length, ci + Math.Max(1, c.GetValue(ColumnSpanProperty))) - 1;
            int re = Math.Min(_rowH.Length, ri + Math.Max(1, c.GetValue(RowSpanProperty))) - 1;
            c.Arrange(new Rect(_colX[ci], _rowY[ri], _colX[ce] + _colW[ce] - _colX[ci], _rowY[re] + _rowH[re] - _rowY[ri]));
        }
    }
}

/// <summary>Drag to resize the grid column to its left (Avalonia GridSplitter, columns).</summary>
public sealed class GridSplitter : Control
{
    int _startX, _startW, _col;

    public GridSplitter()
    {
        Width = 6;
        Focusable = true;
        CornerRadius = 3;
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        var g = Parent as Grid;
        if (g == null) return;
        _col = GetValue(Grid.ColumnProperty) - 1;
        _startX = e.X;
        _startW = g.ActualColumnWidth(_col);
        SetPseudo(":pressed", true);
        e.Handled = true;
    }

    protected internal override void OnPointerMoved(PointerEventArgs e)
    {
        if (!IsPressed) return;
        (Parent as Grid)?.ResizeColumn(_col, _startW + e.X - _startX);
        e.Handled = true;
    }

    protected internal override void OnPointerReleased(PointerEventArgs e) => SetPseudo(":pressed", false);

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        var g = Parent as Grid;
        if (g == null) return;
        int col = GetValue(Grid.ColumnProperty) - 1;
        if (e.Key == Keys.Left) { g.ResizeColumn(col, g.ActualColumnWidth(col) - 16); e.Handled = true; }
        if (e.Key == Keys.Right) { g.ResizeColumn(col, g.ActualColumnWidth(col) + 16); e.Handled = true; }
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(new Rect(b.X + b.Width / 2 - 1, b.Y + 4, 2, Math.Max(0, b.Height - 8)), 1, Background);
        int cy = b.Y + b.Height / 2;
        for (int i = -1; i <= 1; i++) dc.FillRoundRect(new Rect(b.X + b.Width / 2 - 2, cy + i * 6 - 2, 4, 4), 2, Background);
    }
}

public sealed class DockPanel : Panel
{
    public static readonly StyledProperty<Dock> DockProperty = UIProperties.Register("DockPanel.Dock", Dock.Left, affectsMeasure: true);
    public bool LastChildFill { get; set; } = true;
    public static void SetDock(Control c, Dock d) => c.SetValue(DockProperty, d);

    protected override Size MeasureOverride(Size a)
    {
        int usedW = 0, usedH = 0, maxW = 0, maxH = 0;
        var pad = Padding;
        int aw = a.Width - pad.Horizontal, ah = a.Height - pad.Vertical;
        for (int i = 0; i < VisualChildren.Count; i++)
        {
            var c = VisualChildren[i];
            c.Measure(new Size(Math.Max(0, aw - usedW), Math.Max(0, ah - usedH)));
            if (!c.IsVisible) continue;
            var d = c.GetValue(DockProperty);
            bool fill = LastChildFill && i == VisualChildren.Count - 1;
            if (fill || d == Dock.Left || d == Dock.Right)
            {
                maxH = Math.Max(maxH, usedH + c.DesiredSize.Height);
                usedW += c.DesiredSize.Width;
            }
            else
            {
                maxW = Math.Max(maxW, usedW + c.DesiredSize.Width);
                usedH += c.DesiredSize.Height;
            }
        }
        return new Size(Math.Max(maxW, usedW) + pad.Horizontal, Math.Max(maxH, usedH) + pad.Vertical);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = b.Deflate(Padding);
        for (int i = 0; i < VisualChildren.Count; i++)
        {
            var c = VisualChildren[i];
            if (!c.IsVisible) { c.Arrange(new Rect(r.X, r.Y, 0, 0)); continue; }
            if (LastChildFill && i == VisualChildren.Count - 1) { c.Arrange(r); break; }
            var d = c.GetValue(DockProperty);
            int w = Math.Min(c.DesiredSize.Width, r.Width), h = Math.Min(c.DesiredSize.Height, r.Height);
            switch (d)
            {
                case Dock.Left: c.Arrange(new Rect(r.X, r.Y, w, r.Height)); r = new Rect(r.X + w, r.Y, r.Width - w, r.Height); break;
                case Dock.Right: c.Arrange(new Rect(r.Right - w, r.Y, w, r.Height)); r = new Rect(r.X, r.Y, r.Width - w, r.Height); break;
                case Dock.Top: c.Arrange(new Rect(r.X, r.Y, r.Width, h)); r = new Rect(r.X, r.Y + h, r.Width, r.Height - h); break;
                default: c.Arrange(new Rect(r.X, r.Bottom - h, r.Width, h)); r = new Rect(r.X, r.Y, r.Width, r.Height - h); break;
            }
        }
    }
}

public sealed class WrapPanel : Panel
{
    public Orientation Orientation { get; set; } = Orientation.Horizontal;
    public int ItemSpacing { get; set; }
    public int LineSpacing { get; set; }
    public int ItemWidth { get; set; } = -1;
    public int ItemHeight { get; set; } = -1;

    Size ItemSize(Control c) => new Size(ItemWidth >= 0 ? ItemWidth : c.DesiredSize.Width, ItemHeight >= 0 ? ItemHeight : c.DesiredSize.Height);

    protected override Size MeasureOverride(Size a)
    {
        var pad = Padding;
        int limit = (Orientation == Orientation.Horizontal ? a.Width - pad.Horizontal : a.Height - pad.Vertical);
        int line = 0, lineAcross = 0, total = 0, maxLine = 0;
        bool first = true;
        foreach (var c in VisualChildren)
        {
            c.Measure(new Size(ItemWidth >= 0 ? ItemWidth : a.Width, ItemHeight >= 0 ? ItemHeight : a.Height));
            if (!c.IsVisible) continue;
            var s = ItemSize(c);
            int along = Orientation == Orientation.Horizontal ? s.Width : s.Height;
            int across = Orientation == Orientation.Horizontal ? s.Height : s.Width;
            if (!first && line + ItemSpacing + along > limit)
            {
                total += lineAcross + LineSpacing;
                maxLine = Math.Max(maxLine, line);
                line = 0; lineAcross = 0; first = true;
            }
            line += (first ? 0 : ItemSpacing) + along;
            lineAcross = Math.Max(lineAcross, across);
            first = false;
        }
        total += lineAcross;
        maxLine = Math.Max(maxLine, line);
        return Orientation == Orientation.Horizontal ? new Size(maxLine + pad.Horizontal, total + pad.Vertical) : new Size(total + pad.Horizontal, maxLine + pad.Vertical);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = b.Deflate(Padding);
        bool h = Orientation == Orientation.Horizontal;
        int along = 0, acrossPos = 0, lineAcross = 0;
        bool first = true;
        int limit = h ? r.Width : r.Height;
        foreach (var c in VisualChildren)
        {
            if (!c.IsVisible) { c.Arrange(new Rect(r.X, r.Y, 0, 0)); continue; }
            var s = ItemSize(c);
            int a = h ? s.Width : s.Height, x = h ? s.Height : s.Width;
            if (!first && along + ItemSpacing + a > limit) { acrossPos += lineAcross + LineSpacing; along = 0; lineAcross = 0; first = true; }
            if (!first) along += ItemSpacing;
            if (h) c.Arrange(new Rect(r.X + along, r.Y + acrossPos, s.Width, s.Height));
            else c.Arrange(new Rect(r.X + acrossPos, r.Y + along, s.Width, s.Height));
            along += a;
            lineAcross = Math.Max(lineAcross, x);
            first = false;
        }
    }
}

public sealed class UniformGrid : Panel
{
    public int Rows { get; set; }
    public int Columns { get; set; }
    public int Spacing { get; set; }

    void Dims(out int rows, out int cols)
    {
        int n = 0;
        foreach (var c in VisualChildren) if (c.IsVisible) n++;
        cols = Columns > 0 ? Columns : Rows > 0 ? (n + Rows - 1) / Rows : Math.Max(1, (int)Math.Ceiling(Math.Sqrt(n)));
        rows = Rows > 0 ? Rows : Math.Max(1, (n + cols - 1) / cols);
    }

    protected override Size MeasureOverride(Size a)
    {
        Dims(out int rows, out int cols);
        var pad = Padding;
        int cw = 0, ch = 0;
        int aw = a.Width >= Size.Infinite / 2 ? Size.Infinite : (a.Width - pad.Horizontal - Spacing * (cols - 1)) / cols;
        int ah = a.Height >= Size.Infinite / 2 ? Size.Infinite : (a.Height - pad.Vertical - Spacing * (rows - 1)) / rows;
        foreach (var c in VisualChildren)
        {
            c.Measure(new Size(aw, ah));
            cw = Math.Max(cw, c.DesiredSize.Width);
            ch = Math.Max(ch, c.DesiredSize.Height);
        }
        return new Size(cw * cols + Spacing * (cols - 1) + pad.Horizontal, ch * rows + Spacing * (rows - 1) + pad.Vertical);
    }

    protected override void ArrangeOverride(Rect b)
    {
        Dims(out int rows, out int cols);
        var r = b.Deflate(Padding);
        int cw = (r.Width - Spacing * (cols - 1)) / cols, ch = (r.Height - Spacing * (rows - 1)) / rows;
        int i = 0;
        foreach (var c in VisualChildren)
        {
            if (!c.IsVisible) { c.Arrange(new Rect(r.X, r.Y, 0, 0)); continue; }
            int row = i / cols, col = i % cols;
            c.Arrange(new Rect(r.X + col * (cw + Spacing), r.Y + row * (ch + Spacing), cw, ch));
            i++;
        }
    }
}

public sealed class Canvas : Panel
{
    public static readonly StyledProperty<int> LeftProperty = UIProperties.Register("Canvas.Left", 0, affectsMeasure: true);
    public static readonly StyledProperty<int> TopProperty = UIProperties.Register("Canvas.Top", 0, affectsMeasure: true);
    public static void SetLeft(Control c, int v) => c.SetValue(LeftProperty, v);
    public static void SetTop(Control c, int v) => c.SetValue(TopProperty, v);

    protected override Size MeasureOverride(Size a)
    {
        foreach (var c in VisualChildren) c.Measure(Size.Unbounded);
        return new Size(0, 0);
    }

    protected override void ArrangeOverride(Rect b)
    {
        foreach (var c in VisualChildren)
            c.Arrange(new Rect(b.X + c.GetValue(LeftProperty), b.Y + c.GetValue(TopProperty), c.DesiredSize.Width, c.DesiredSize.Height));
    }
}

/// <summary>A decorator with background, border, corner radius, padding and shadow.</summary>
public class Border : Control
{
    Control? _child;
    public Control? Child
    {
        get => _child;
        set
        {
            if (_child != null) RemoveVisualChild(_child);
            _child = value;
            if (value != null) AddVisualChild(value);
        }
    }
}

/// <summary>Hosts one piece of content: a control, or any object shown through
/// <see cref="ContentTemplate"/> (or as text).</summary>
public class ContentControl : Control
{
    object? _content;
    Control? _presenter;

    public Func<object, Control>? ContentTemplate { get; set; }

    public object? Content
    {
        get => _content;
        set
        {
            _content = value;
            if (_presenter != null) { RemoveVisualChild(_presenter); _presenter = null; }
            _presenter = MakePresenter(value);
            if (_presenter != null) AddVisualChild(_presenter);
            OnContentChanged();
        }
    }

    protected Control? Presenter => _presenter;

    protected virtual void OnContentChanged() { }

    protected virtual Control? MakePresenter(object? content)
    {
        if (content == null) return null;
        if (content is Control c) return c;
        if (ContentTemplate != null) return ContentTemplate(content);
        return new TextBlock { Text = content.ToString() ?? "" };
    }
}

public sealed class ScrollViewer : ContentControl
{
    public static readonly StyledProperty<int> VerticalOffsetProperty = UIProperties.Register("VerticalOffset", 0, affectsMeasure: true);
    public bool HorizontalScroll { get; set; }
    public int HorizontalOffset { get; set; }
    int _extent, _viewport, _dragStartY = -1, _dragStartOffset;
    const int Bar = 8;

    public ScrollViewer()
    {
        ClipToBounds = true;
    }

    public int VerticalOffset
    {
        get => GetValue(VerticalOffsetProperty);
        set => SetValue(VerticalOffsetProperty, Math.Max(0, Math.Min(value, Math.Max(0, _extent - _viewport))));
    }

    public int Extent => _extent;
    public int Viewport => _viewport;
    public bool CanScroll => _extent > _viewport;

    protected override Size MeasureOverride(Size a)
    {
        var p = Presenter;
        if (p == null) return new Size(0, 0);
        p.Measure(new Size(HorizontalScroll ? Size.Infinite : Math.Max(0, a.Width - Bar - 2), Size.Infinite));
        return new Size(Math.Min(p.DesiredSize.Width + Bar + 2, a.Width), Math.Min(p.DesiredSize.Height, a.Height));
    }

    protected override void ArrangeOverride(Rect b)
    {
        var p = Presenter;
        if (p == null) return;
        _viewport = b.Height;
        _extent = p.DesiredSize.Height;
        int max = Math.Max(0, _extent - _viewport);
        if (VerticalOffset > max) SetValue(VerticalOffsetProperty, max);
        int w = HorizontalScroll ? Math.Max(p.DesiredSize.Width, b.Width - Bar - 2) : b.Width - Bar - 2;
        p.Arrange(new Rect(b.X - HorizontalOffset, b.Y - VerticalOffset, w, Math.Max(_extent, b.Height)));
    }

    Rect Thumb()
    {
        var b = Bounds;
        if (_extent <= _viewport) return new Rect(0, 0, 0, 0);
        int h = Math.Max(24, b.Height * _viewport / _extent);
        int y = b.Y + (b.Height - h) * VerticalOffset / Math.Max(1, _extent - _viewport);
        return new Rect(b.Right - Bar - 1, y, Bar - 2, h);
    }

    protected override void RenderOverlay(DrawingContext dc)
    {
        if (!CanScroll) return;
        var t = Thumb();
        int c = _dragStartY >= 0 || IsPointerOver ? Theme.TextFaint : Theme.Stroke;
        dc.FillRoundRect(t, 3, c);
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        var b = Bounds;
        if (CanScroll && e.X >= b.Right - Bar - 2)
        {
            var t = Thumb();
            if (!t.Contains(e.X, e.Y)) VerticalOffset += e.Y < t.Y ? -_viewport : _viewport;
            _dragStartY = e.Y;
            _dragStartOffset = VerticalOffset;
            e.Handled = true;
        }
        base.OnPointerPressed(e);
    }

    protected internal override void OnPointerMoved(PointerEventArgs e)
    {
        if (_dragStartY >= 0 && e.LeftButton)
        {
            int track = Math.Max(1, Bounds.Height - Thumb().Height);
            VerticalOffset = _dragStartOffset + (e.Y - _dragStartY) * Math.Max(1, _extent - _viewport) / track;
            e.Handled = true;
        }
        base.OnPointerMoved(e);
    }

    protected internal override void OnPointerReleased(PointerEventArgs e)
    {
        _dragStartY = -1;
        base.OnPointerReleased(e);
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        int step = 40;
        switch (e.Key)
        {
            case Keys.PageDown: VerticalOffset += _viewport - step; e.Handled = true; break;
            case Keys.PageUp: VerticalOffset -= _viewport - step; e.Handled = true; break;
            case Keys.Down: if (CanScroll) { VerticalOffset += step; e.Handled = true; } break;
            case Keys.Up: if (CanScroll) { VerticalOffset -= step; e.Handled = true; } break;
        }
        base.OnKeyDown(e);
    }

    /// <summary>Scroll so that <paramref name="c"/> is visible.</summary>
    public void ScrollIntoView(Control c)
    {
        var b = Bounds;
        if (c.Bounds.Y < b.Y) VerticalOffset -= b.Y - c.Bounds.Y;
        else if (c.Bounds.Bottom > b.Bottom) VerticalOffset += c.Bounds.Bottom - b.Bottom;
    }
}

/// <summary>A pane beside (or over) the content that opens and closes with an animation (Avalonia SplitView).</summary>
public sealed class SplitView : Control
{
    public static readonly StyledProperty<bool> IsPaneOpenProperty = UIProperties.Register("IsPaneOpen", true, affectsMeasure: true);
    public static readonly StyledProperty<int> PaneWidthProperty = UIProperties.Register("PaneWidth", 220, affectsMeasure: true);
    Control? _pane, _content;

    public int OpenPaneLength { get; set; } = 220;
    public int CompactPaneLength { get; set; } = 48;
    public SplitViewDisplayMode DisplayMode { get; set; } = SplitViewDisplayMode.CompactInline;
    public int PaneBackground { get; set; } = Color.Transparent;

    public SplitView()
    {
        ClipToBounds = true;
        Transitions = new List<Transition> { new Transition(PaneWidthProperty, 180) };
    }

    public bool IsPaneOpen
    {
        get => GetValue(IsPaneOpenProperty);
        set
        {
            SetValue(IsPaneOpenProperty, value);
            int from = GetValue(PaneWidthProperty), to = TargetPaneWidth();
            SetValue(PaneWidthProperty, to);
            if (IsAttached) Animator.Start(this, PaneWidthProperty, from, to, 180, false);
        }
    }

    int TargetPaneWidth() => IsPaneOpen ? OpenPaneLength :
        DisplayMode == SplitViewDisplayMode.CompactInline || DisplayMode == SplitViewDisplayMode.CompactOverlay ? CompactPaneLength : 0;

    public Control? Pane
    {
        get => _pane;
        set { if (_pane != null) RemoveVisualChild(_pane); _pane = value; if (value != null) AddVisualChild(value); Reorder(); }
    }

    public Control? Content
    {
        get => _content;
        set { if (_content != null) RemoveVisualChild(_content); _content = value; if (value != null) AddVisualChild(value, 0); Reorder(); }
    }

    void Reorder()
    {
        // the pane draws (and hit-tests) above the content
        if (_pane != null && VisualChildren.Remove(_pane)) VisualChildren.Add(_pane);
        SetValue(PaneWidthProperty, TargetPaneWidth());
    }

    bool Overlay => DisplayMode == SplitViewDisplayMode.Overlay || DisplayMode == SplitViewDisplayMode.CompactOverlay;

    protected override Size MeasureOverride(Size a)
    {
        int pw = GetValue(PaneWidthProperty);
        _pane?.Measure(new Size(pw, a.Height));
        int cw = Overlay ? a.Width - (DisplayMode == SplitViewDisplayMode.CompactOverlay ? CompactPaneLength : 0) : a.Width - pw;
        _content?.Measure(new Size(Math.Max(0, cw), a.Height));
        return a;
    }

    protected override void ArrangeOverride(Rect b)
    {
        int pw = GetValue(PaneWidthProperty);
        int inset = Overlay ? (DisplayMode == SplitViewDisplayMode.CompactOverlay ? CompactPaneLength : 0) : pw;
        _content?.Arrange(new Rect(b.X + inset, b.Y, Math.Max(0, b.Width - inset), b.Height));
        _pane?.Arrange(new Rect(b.X, b.Y, pw, b.Height));
    }

    protected override void RenderChildren(DrawingContext dc)
    {
        _content?.RenderTree(dc);
        if (_pane == null) return;
        var r = _pane.Bounds;
        if (PaneBackground >= 0) dc.FillRect(r, PaneBackground);
        dc.PushClip(r);
        _pane.RenderTree(dc);
        dc.PopClip();
        dc.FillRect(new Rect(r.Right - 1, r.Y, 1, r.Height), Theme.StrokeSoft);
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        // a press on the content closes an overlay pane (light dismiss)
        if (Overlay && IsPaneOpen && _pane != null && !_pane.Bounds.Contains(e.X, e.Y)) { IsPaneOpen = false; e.Handled = true; }
        base.OnPointerPressed(e);
    }
}

/// <summary>Fluent setters for layout and attached properties, so trees can be written in code.</summary>
public static class LayoutExtensions
{
    public static T Row<T>(this T c, int row, int span = 1) where T : Control { c.SetValue(Grid.RowProperty, row); c.SetValue(Grid.RowSpanProperty, span); return c; }
    public static T Col<T>(this T c, int col, int span = 1) where T : Control { c.SetValue(Grid.ColumnProperty, col); c.SetValue(Grid.ColumnSpanProperty, span); return c; }
    public static T Cell<T>(this T c, int row, int col) where T : Control { c.SetValue(Grid.RowProperty, row); c.SetValue(Grid.ColumnProperty, col); return c; }
    public static T DockTo<T>(this T c, Dock d) where T : Control { c.SetValue(DockPanel.DockProperty, d); return c; }
    public static T At<T>(this T c, int left, int top) where T : Control { c.SetValue(Canvas.LeftProperty, left); c.SetValue(Canvas.TopProperty, top); return c; }
    public static T WithClass<T>(this T c, string classes) where T : Control
    {
        foreach (var k in classes.Split(' ')) if (k.Length > 0) c.Classes.Add(k);
        return c;
    }
    public static T WithMargin<T>(this T c, Thickness m) where T : Control { c.Margin = m; return c; }
    public static T WithTip<T>(this T c, object tip) where T : Control { ToolTip.SetTip(c, tip); return c; }
    public static T Named<T>(this T c, string name) where T : Control { c.Name = name; return c; }
}
