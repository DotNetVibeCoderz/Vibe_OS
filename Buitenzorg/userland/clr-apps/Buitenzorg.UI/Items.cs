// Buitenzorg.UI -- items controls: ItemsControl (ItemsSource + ItemTemplate +
// ItemsPanel, live on ObservableCollection), ListBox, ComboBox, TabControl,
// TreeView, DataGrid and AutoCompleteBox.

using System.Collections;

namespace Buitenzorg.UI;

/// <summary>Shows a collection through a template (Avalonia ItemsControl).</summary>
public class ItemsControl : Control
{
    IEnumerable? _source;
    Panel _panel;
    bool _customPanel;
    readonly List<object?> _items = new List<object?>();

    /// <summary>Builds the control for one item (Avalonia DataTemplate); default: its text.</summary>
    public Func<object, Control>? ItemTemplate
    {
        get => _template;
        set { _template = value; if (_source != null) Regenerate(); }
    }
    Func<object, Control>? _template;
    /// <summary>The panel the item containers go in (default: a vertical StackPanel).</summary>
    public Func<Panel>? ItemsPanel { get; set; }

    public ItemsControl()
    {
        _panel = new StackPanel();
        AddVisualChild(_panel);
    }

    protected Panel ItemsHost => _panel;
    public IReadOnlyList<object?> Items => _items;
    public int ItemCount => _items.Count;

    public IEnumerable? ItemsSource
    {
        get => _source;
        set
        {
            if (_source is INotifyCollectionChanged old) old.CollectionChanged -= OnCollectionChanged;
            _source = value;
            if (value is INotifyCollectionChanged n) n.CollectionChanged += OnCollectionChanged;
            Regenerate();
        }
    }

    void OnCollectionChanged(object sender, CollectionChange change, int index) => Regenerate();

    /// <summary>Rebuild every container (after a change of the source or the template).</summary>
    public void Regenerate()
    {
        if (ItemsPanel != null && !_customPanel)
        {
            var old = _panel;
            _panel = ItemsPanel();
            _customPanel = true;
            ReplacePanel(old, _panel);
        }
        _panel.Children.Clear();
        _items.Clear();
        if (_source == null) { OnItemsChanged(); return; }
        int i = 0;
        foreach (var item in _source)
        {
            _items.Add(item);
            var c = CreateContainer(item, i);
            c.DataContext = item;
            _panel.Children.Add(c);
            i++;
        }
        OnItemsChanged();
    }

    protected virtual void ReplacePanel(Panel old, Panel p)
    {
        int idx = VisualChildren.IndexOf(old);
        RemoveVisualChild(old);
        AddVisualChild(p, idx);
    }

    protected virtual void OnItemsChanged() { }

    protected Control BuildContent(object? item)
    {
        if (item is Control c) return c;
        if (item != null && ItemTemplate != null) return ItemTemplate(item);
        return new TextBlock(item?.ToString() ?? "");
    }

    protected virtual Control CreateContainer(object? item, int index) => BuildContent(item);

    protected Control ContainerAt(int i) => _panel.Children[i];
}

public class ListBoxItem : ContentControl
{
    public bool IsSelected { get => PseudoClasses.Contains(":selected"); internal set => SetPseudo(":selected", value); }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        if (IsSelected)
        {
            // the leaf-vein marker of the selected row
            var b = Bounds;
            dc.FillRoundRect(new Rect(b.X + 2, b.Y + 6, 3, Math.Max(4, b.Height - 12)), 1, Theme.Accent);
        }
    }
}

/// <summary>A selectable list (Avalonia ListBox), scrolled, keyboard-navigable.</summary>
public class ListBox : ItemsControl
{
    public static readonly StyledProperty<int> SelectedIndexProperty = UIProperties.Register("SelectedIndex", -1);
    readonly ScrollViewer _scroll = new ScrollViewer();
    public event Action<ListBox>? SelectionChanged;
    /// <summary>Raised on Enter or a second click on the selected item.</summary>
    public event Action<ListBox>? ItemActivated;

    public ListBox()
    {
        Focusable = true;
        ClipToBounds = true;
        // the items panel lives inside a ScrollViewer
        var panel = ItemsHost;
        RemoveVisualChild(panel);
        _scroll.Content = panel;
        AddVisualChild(_scroll);
    }

    protected override void ReplacePanel(Panel old, Panel p) => _scroll.Content = p;

    public ScrollViewer ScrollViewer => _scroll;

    public int SelectedIndex
    {
        get => GetValue(SelectedIndexProperty);
        set => SetValue(SelectedIndexProperty, value < -1 || value >= ItemCount ? -1 : value);
    }

    public object? SelectedItem
    {
        get => SelectedIndex >= 0 && SelectedIndex < ItemCount ? Items[SelectedIndex] : null;
        set
        {
            for (int i = 0; i < ItemCount; i++) if (Equals(Items[i], value)) { SelectedIndex = i; return; }
            SelectedIndex = -1;
        }
    }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p != SelectedIndexProperty) return;
        UpdateContainers();
        SelectionChanged?.Invoke(this);
    }

    void UpdateContainers()
    {
        for (int i = 0; i < ItemCount; i++)
            if (ContainerAt(i) is ListBoxItem li) li.IsSelected = i == SelectedIndex;
    }

    protected override void OnItemsChanged()
    {
        if (SelectedIndex >= ItemCount) SelectedIndex = ItemCount - 1;
        UpdateContainers();
    }

    protected override Control CreateContainer(object? item, int index)
    {
        var li = new ListBoxItem { Content = BuildContent(item) };
        li.PointerPressed += (_, e) =>
        {
            bool again = SelectedIndex == index;
            SelectedIndex = index;
            Focus();
            if (again) ItemActivated?.Invoke(this);
        };
        return li;
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        int n = ItemCount;
        if (n > 0)
        {
            if (e.Key == Keys.Down) { SelectedIndex = Math.Min(n - 1, SelectedIndex + 1); e.Handled = true; }
            else if (e.Key == Keys.Up) { SelectedIndex = Math.Max(0, SelectedIndex - 1); e.Handled = true; }
            else if (e.Key == Keys.Home) { SelectedIndex = 0; e.Handled = true; }
            else if (e.Key == Keys.End) { SelectedIndex = n - 1; e.Handled = true; }
            else if (e.Key == Keys.Enter && SelectedIndex >= 0) { ItemActivated?.Invoke(this); e.Handled = true; }
            if (e.Handled && SelectedIndex >= 0)
            {
                Window?.UpdateLayout();
                _scroll.ScrollIntoView(ContainerAt(SelectedIndex));
            }
        }
        if (!e.Handled) base.OnKeyDown(e);
    }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(a);
        return new Size(Math.Max(s.Width, 120), s.Height);
    }
}

public sealed class ComboBoxItem : ListBoxItem { }
public sealed class ComboBoxDropDown : Border { }

/// <summary>A closed button showing the selection; opens a list in a popup (Avalonia ComboBox).</summary>
public sealed class ComboBox : Control
{
    public static readonly StyledProperty<int> SelectedIndexProperty = UIProperties.Register("SelectedIndex", -1);
    readonly List<object> _items = new List<object>();
    Popup? _popup;
    ListBox? _list;
    public string PlaceholderText { get; set; } = "Pilih...";
    public Func<object, string>? DisplayText { get; set; }
    public event Action<ComboBox>? SelectionChanged;

    public ComboBox() { Focusable = true; MinWidth = 120; }

    public ComboBox(IEnumerable items, int selected = -1) : this()
    {
        foreach (var i in items) _items.Add(i);
        SelectedIndex = selected;
    }

    public List<object> Items => _items;
    public bool IsDropDownOpen => _popup != null && _popup.IsOpen;

    public int SelectedIndex
    {
        get => GetValue(SelectedIndexProperty);
        set => SetValue(SelectedIndexProperty, value < -1 || value >= _items.Count ? -1 : value);
    }

    public object? SelectedItem => SelectedIndex >= 0 ? _items[SelectedIndex] : null;

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == SelectedIndexProperty) SelectionChanged?.Invoke(this);
    }

    string Text(object o) => DisplayText != null ? DisplayText(o) : o.ToString() ?? "";

    protected override Size MeasureOverride(Size a)
    {
        int w = Fonts.Measure(PlaceholderText, 0);
        foreach (var i in _items) w = Math.Max(w, Fonts.Measure(Text(i), 0));
        var pad = Padding;
        return new Size(w + pad.Horizontal + 26, Fonts.LineHeight(0) + pad.Vertical + 2);
    }

    public void Open()
    {
        if (Window == null || IsDropDownOpen) return;
        _list = new ListBox { MaxHeight = 220, BorderThickness = 0, Background = Color.Transparent, Padding = new Thickness(0) };
        _list.ItemsSource = _items;
        _list.SelectedIndex = SelectedIndex;
        _list.SelectionChanged += l => { SelectedIndex = l.SelectedIndex; Close(); };
        _list.ItemActivated += _ => Close();
        var host = new ComboBoxDropDown { Child = _list, MinWidth = Bounds.Width };
        _popup = new Popup(host) { PlacementTarget = this };
        _popup.Closed += () => Focus();
        _popup.Open(Window);
        _list.Focus();
        SetPseudo(":open", true);
    }

    public void Close()
    {
        _popup?.Close();
        _popup = null;
        SetPseudo(":open", false);
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        if (IsDropDownOpen) Close(); else Open();
        e.Handled = true;
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.Enter || e.Key == Keys.Space) { Open(); e.Handled = true; }
        else if (e.Key == Keys.Down) { SelectedIndex = Math.Min(_items.Count - 1, SelectedIndex + 1); e.Handled = true; }
        else if (e.Key == Keys.Up) { SelectedIndex = Math.Max(0, SelectedIndex - 1); e.Handled = true; }
        base.OnKeyDown(e);
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        string t = SelectedItem != null ? Text(SelectedItem) : PlaceholderText;
        dc.PushClip(new Rect(r.X, r.Y, Math.Max(0, r.Width - 20), r.Height));
        dc.Text(r.X, r.Y + (r.Height - Fonts.LineHeight(0)) / 2, t, SelectedItem != null ? Foreground : Theme.TextFaint, 0);
        dc.PopClip();
        Icon.Draw(dc, IsDropDownOpen ? IconKind.ChevronUp : IconKind.ChevronDown, new Rect(r.Right - 13, r.Y + r.Height / 2 - 6, 12, 12), Theme.TextMuted);
    }
}

public sealed class TabItem : ContentControl
{
    public string Header { get; set; }
    public IconKind HeaderIcon { get; set; }
    internal Control? Page;

    public TabItem(string header, Control content, IconKind icon = IconKind.None)
    {
        Header = header;
        Page = content;
        HeaderIcon = icon;
        Content = new TextBlock(header);
        if (icon != IconKind.None)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
            row.Children.Add(new Icon(icon, 14) { VerticalAlignment = VerticalAlignment.Center });
            row.Children.Add(new TextBlock(header));
            Content = row;
        }
        Focusable = true;
    }

    public bool IsSelected { get => PseudoClasses.Contains(":selected"); internal set => SetPseudo(":selected", value); }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        if (IsSelected)
        {
            var b = Bounds;
            dc.FillRoundRect(new Rect(b.X + 8, b.Bottom - 3, b.Width - 16, 3), 1, Theme.Accent);
        }
    }
}

/// <summary>Tabs over pages (Avalonia TabControl).</summary>
public sealed class TabControl : Control
{
    public static readonly StyledProperty<int> SelectedIndexProperty = UIProperties.Register("SelectedIndex", 0, affectsMeasure: true);
    readonly StackPanel _strip = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2 };
    readonly Border _host = new Border();
    readonly List<TabItem> _tabs = new List<TabItem>();
    public event Action<TabControl>? SelectionChanged;

    public TabControl()
    {
        AddVisualChild(_strip);
        AddVisualChild(_host);
    }

    public IReadOnlyList<TabItem> Tabs => _tabs;

    public TabControl Add(string header, Control content, IconKind icon = IconKind.None)
    {
        var t = new TabItem(header, content, icon);
        int index = _tabs.Count;
        t.PointerPressed += (_, e) => { SelectedIndex = index; e.Handled = true; };
        t.KeyDown += (_, e) =>
        {
            if (e.Key == Keys.Right) { SelectedIndex = (SelectedIndex + 1) % _tabs.Count; _tabs[SelectedIndex].Focus(); e.Handled = true; }
            if (e.Key == Keys.Left) { SelectedIndex = (SelectedIndex + _tabs.Count - 1) % _tabs.Count; _tabs[SelectedIndex].Focus(); e.Handled = true; }
        };
        _tabs.Add(t);
        _strip.Children.Add(t);
        Select();
        return this;
    }

    public int SelectedIndex { get => GetValue(SelectedIndexProperty); set => SetValue(SelectedIndexProperty, value); }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == SelectedIndexProperty) { Select(); SelectionChanged?.Invoke(this); }
    }

    void Select()
    {
        for (int i = 0; i < _tabs.Count; i++) _tabs[i].IsSelected = i == SelectedIndex;
        _host.Child = SelectedIndex >= 0 && SelectedIndex < _tabs.Count ? _tabs[SelectedIndex].Page : null;
    }

    protected override Size MeasureOverride(Size a)
    {
        _strip.Measure(a);
        _host.Measure(new Size(a.Width, Math.Max(0, a.Height - _strip.DesiredSize.Height - 8)));
        return new Size(Math.Max(_strip.DesiredSize.Width, _host.DesiredSize.Width), _strip.DesiredSize.Height + 8 + _host.DesiredSize.Height);
    }

    protected override void ArrangeOverride(Rect b)
    {
        int sh = _strip.DesiredSize.Height;
        _strip.Arrange(new Rect(b.X, b.Y, b.Width, sh));
        _host.Arrange(new Rect(b.X, b.Y + sh + 8, b.Width, Math.Max(0, b.Height - sh - 8)));
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var s = _strip.Bounds;
        dc.FillRect(new Rect(Bounds.X, s.Bottom - 1, Bounds.Width, 1), Theme.StrokeSoft);
    }
}

public sealed class TreeViewItemHeader : ContentControl
{
    public bool IsSelected { get => PseudoClasses.Contains(":selected"); internal set => SetPseudo(":selected", value); }
}

/// <summary>A node of a <see cref="TreeView"/>: a header and child nodes.</summary>
public sealed class TreeViewItem : Control
{
    readonly TreeViewItemHeader _header = new TreeViewItemHeader();
    readonly StackPanel _children = new StackPanel();
    bool _expanded;
    public object? Header { get; }
    public object? Value { get; set; }
    public List<TreeViewItem> Items { get; } = new List<TreeViewItem>();
    internal int Depth;

    public TreeViewItem(object header, IconKind icon = IconKind.None)
    {
        Header = header;
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        if (icon != IconKind.None) row.Children.Add(new Icon(icon, 14) { VerticalAlignment = VerticalAlignment.Center });
        row.Children.Add(header as Control ?? new TextBlock(header.ToString() ?? ""));
        _header.Content = row;
        AddVisualChild(_header);
        AddVisualChild(_children);
        _children.IsVisible = false;
    }

    public TreeViewItem Add(TreeViewItem child)
    {
        Items.Add(child);
        child.Depth = Depth + 1;
        _children.Children.Add(child);
        return this;
    }

    public bool IsExpanded
    {
        get => _expanded;
        set { _expanded = value; _children.IsVisible = value; SetPseudo(":expanded", value); }
    }

    internal TreeViewItemHeader HeaderControl => _header;

    protected override Size MeasureOverride(Size a)
    {
        int indent = 18 + Depth * 16;
        _header.Measure(new Size(Math.Max(0, a.Width - indent), a.Height));
        _children.Measure(a);
        return new Size(Math.Max(_header.DesiredSize.Width + indent, _children.DesiredSize.Width), _header.DesiredSize.Height + (_expanded ? _children.DesiredSize.Height : 0));
    }

    protected override void ArrangeOverride(Rect b)
    {
        int indent = 18 + Depth * 16;
        int hh = _header.DesiredSize.Height;
        _header.Arrange(new Rect(b.X + indent, b.Y, b.Width - indent, hh));
        _children.Arrange(new Rect(b.X, b.Y + hh, b.Width, Math.Max(0, b.Height - hh)));
    }

    protected override void RenderOverlay(DrawingContext dc)
    {
        if (Items.Count == 0) return;
        var h = _header.Bounds;
        Icon.Draw(dc, _expanded ? IconKind.ChevronDown : IconKind.ChevronRight, new Rect(h.X - 16, h.Y + h.Height / 2 - 6, 12, 12), Theme.TextMuted);
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        var tree = FindAncestor<TreeView>();
        if (_header.Bounds.Inflate(0).Contains(e.X, e.Y) || (e.X < _header.Bounds.X && e.Y < _header.Bounds.Bottom && e.Y >= _header.Bounds.Y))
        {
            if (e.X < _header.Bounds.X || (tree != null && tree.SelectedItem == this)) IsExpanded = !IsExpanded;
            tree?.Select(this);
            tree?.Focus();
            e.Handled = true;
        }
        base.OnPointerPressed(e);
    }
}

/// <summary>A hierarchy of expandable nodes (Avalonia TreeView).</summary>
public sealed class TreeView : Control
{
    readonly StackPanel _roots = new StackPanel();
    readonly ScrollViewer _scroll = new ScrollViewer();
    public TreeViewItem? SelectedItem { get; private set; }
    public event Action<TreeView>? SelectionChanged;

    public TreeView()
    {
        Focusable = true;
        _scroll.Content = _roots;
        AddVisualChild(_scroll);
    }

    public TreeView Add(TreeViewItem item)
    {
        _roots.Children.Add(item);
        return this;
    }

    public void Select(TreeViewItem item)
    {
        if (SelectedItem != null) SelectedItem.HeaderControl.IsSelected = false;
        SelectedItem = item;
        item.HeaderControl.IsSelected = true;
        SelectionChanged?.Invoke(this);
    }

    List<TreeViewItem> Visible()
    {
        var l = new List<TreeViewItem>();
        void Walk(TreeViewItem t) { l.Add(t); if (t.IsExpanded) foreach (var c in t.Items) Walk(c); }
        foreach (var c in _roots.Children) if (c is TreeViewItem t) Walk(t);
        return l;
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        var v = Visible();
        int i = SelectedItem == null ? -1 : v.IndexOf(SelectedItem);
        if (e.Key == Keys.Down && i + 1 < v.Count) { Select(v[i + 1]); e.Handled = true; }
        else if (e.Key == Keys.Up && i > 0) { Select(v[i - 1]); e.Handled = true; }
        else if (e.Key == Keys.Right && SelectedItem != null) { SelectedItem.IsExpanded = true; e.Handled = true; }
        else if (e.Key == Keys.Left && SelectedItem != null) { SelectedItem.IsExpanded = false; e.Handled = true; }
        else if (e.Key == Keys.Enter && SelectedItem != null) { SelectedItem.IsExpanded = !SelectedItem.IsExpanded; e.Handled = true; }
        if (!e.Handled) base.OnKeyDown(e);
    }
}

/// <summary>A column of a <see cref="DataGrid"/>: header, text of a row, width (Pixel/Star).</summary>
public sealed class DataGridColumn
{
    public string Header { get; }
    public Func<object, string> Binding { get; }
    public GridLength Width { get; }
    /// <summary>A numeric sort key; without one the column sorts by its text.</summary>
    public Func<object, long>? SortKey { get; set; }
    public TextAlignment Alignment { get; set; }

    public DataGridColumn(string header, Func<object, string> binding, string width = "*")
    {
        Header = header;
        Binding = binding;
        Width = GridLength.Parse(width);
    }
}

public sealed class DataGridColumnHeader : Control
{
    internal DataGridColumn Column = null!;
    internal int Sort; // 0 none, 1 asc, -1 desc

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        int st = TextStyle;
        string text = Column.Header;
        int x = Column.Alignment == TextAlignment.Right ? r.Right - Fonts.Measure(text, st) - (Sort != 0 ? 14 : 0) : r.X;
        dc.Text(x, r.Y, text, Foreground, st);
        if (Sort != 0)
            Icon.Draw(dc, Sort > 0 ? IconKind.ChevronUp : IconKind.ChevronDown, new Rect(x + Fonts.Measure(text, st) + 4, r.Y + 2, 10, 10), Theme.Accent);
    }

    protected override Size MeasureOverride(Size a) => new Size(Fonts.Measure(Column.Header, TextStyle) + Padding.Horizontal + 14, Fonts.LineHeight(TextStyle) + Padding.Vertical);
}

public sealed class DataGridRow : Control
{
    internal DataGrid Grid = null!;
    internal object Item = null!;
    internal int Index;
    public bool IsSelected { get => PseudoClasses.Contains(":selected"); internal set => SetPseudo(":selected", value); }

    protected override Size MeasureOverride(Size a) => new Size(a.Width, Fonts.LineHeight(0) + 12);

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var b = Bounds;
        if (IsSelected) dc.FillRoundRect(new Rect(b.X + 1, b.Y + 6, 3, b.Height - 12), 1, Theme.Accent);
        int x = b.X;
        var widths = Grid.ColumnWidths;
        for (int i = 0; i < Grid.Columns.Count; i++)
        {
            var col = Grid.Columns[i];
            string t = col.Binding(Item);
            var cell = new Rect(x + 8, b.Y + 6, Math.Max(0, widths[i] - 16), b.Height - 6);
            dc.PushClip(cell);
            int tx = col.Alignment == TextAlignment.Right ? cell.Right - Fonts.Measure(t, 0) : cell.X;
            dc.Text(tx, cell.Y, t, i == 0 ? Foreground : Theme.TextMuted, 0);
            dc.PopClip();
            x += widths[i];
        }
    }
}

/// <summary>Rows of items in sortable columns (Avalonia DataGrid, read-only).</summary>
public sealed class DataGrid : Control
{
    readonly StackPanel _headerRow = new StackPanel { Orientation = Orientation.Horizontal };
    readonly StackPanel _rows = new StackPanel();
    readonly ScrollViewer _scroll = new ScrollViewer();
    readonly List<object> _items = new List<object>();
    IEnumerable? _source;
    int _sortCol = -1, _sortDir;
    public List<DataGridColumn> Columns { get; } = new List<DataGridColumn>();
    internal int[] ColumnWidths = new int[0];
    public int SelectedIndex { get; private set; } = -1;
    public object? SelectedItem => SelectedIndex >= 0 && SelectedIndex < _items.Count ? _items[SelectedIndex] : null;
    public event Action<DataGrid>? SelectionChanged;

    public DataGrid()
    {
        Focusable = true;
        _scroll.Content = _rows;
        AddVisualChild(_headerRow);
        AddVisualChild(_scroll);
    }

    public DataGrid AddColumn(DataGridColumn c)
    {
        Columns.Add(c);
        var h = new DataGridColumnHeader { Column = c };
        int idx = Columns.Count - 1;
        h.PointerPressed += (_, e) => { SortBy(idx); e.Handled = true; };
        _headerRow.Children.Add(h);
        return this;
    }

    public IEnumerable? ItemsSource
    {
        get => _source;
        set
        {
            if (_source is INotifyCollectionChanged o) o.CollectionChanged -= Changed;
            _source = value;
            if (value is INotifyCollectionChanged n) n.CollectionChanged += Changed;
            Rebuild();
        }
    }

    void Changed(object s, CollectionChange c, int i) => Rebuild();

    public void SortBy(int col)
    {
        if (_sortCol == col) _sortDir = -_sortDir; else { _sortCol = col; _sortDir = 1; }
        Rebuild();
    }

    void Rebuild()
    {
        object? keep = SelectedItem;
        _items.Clear();
        if (_source != null) foreach (var o in _source) if (o != null) _items.Add(o);
        if (_sortCol >= 0 && _sortCol < Columns.Count)
        {
            var c = Columns[_sortCol];
            // insertion sort: small lists; numeric keys compare as numbers, text ordinally
            var key = c.SortKey;
            for (int i = 1; i < _items.Count; i++)
            {
                var x = _items[i];
                int j = i - 1;
                while (j >= 0 && Compare(c, key, _items[j], x) * _sortDir > 0) { _items[j + 1] = _items[j]; j--; }
                _items[j + 1] = x;
            }
        }
        for (int i = 0; i < _headerRow.Children.Count; i++)
            ((DataGridColumnHeader)_headerRow.Children[i]).Sort = i == _sortCol ? _sortDir : 0;
        _rows.Children.Clear();
        SelectedIndex = -1;
        for (int i = 0; i < _items.Count; i++)
        {
            var row = new DataGridRow { Grid = this, Item = _items[i], Index = i };
            int idx = i;
            row.PointerPressed += (_, e) => { Select(idx); Focus(); };
            _rows.Children.Add(row);
            if (keep != null && ReferenceEquals(_items[i], keep)) { SelectedIndex = i; row.IsSelected = true; }
        }
        InvalidateMeasure();
    }

    static int Compare(DataGridColumn c, Func<object, long>? key, object a, object b)
    {
        if (key != null) { long x = key(a), y = key(b); return x < y ? -1 : x > y ? 1 : 0; }
        return string.CompareOrdinal(c.Binding(a), c.Binding(b));
    }

    public void Select(int i)
    {
        if (SelectedIndex >= 0 && SelectedIndex < _rows.Children.Count) ((DataGridRow)_rows.Children[SelectedIndex]).IsSelected = false;
        SelectedIndex = i;
        if (i >= 0 && i < _rows.Children.Count) ((DataGridRow)_rows.Children[i]).IsSelected = true;
        SelectionChanged?.Invoke(this);
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.Down && SelectedIndex + 1 < _items.Count) { Select(SelectedIndex + 1); e.Handled = true; }
        else if (e.Key == Keys.Up && SelectedIndex > 0) { Select(SelectedIndex - 1); e.Handled = true; }
        if (e.Handled) { Window?.UpdateLayout(); _scroll.ScrollIntoView(_rows.Children[SelectedIndex]); }
        else base.OnKeyDown(e);
    }

    void ComputeWidths(int total)
    {
        int n = Columns.Count;
        ColumnWidths = new int[n];
        int fixedW = 0, stars = 0;
        for (int i = 0; i < n; i++)
        {
            if (Columns[i].Width.Unit == GridUnit.Pixel) { ColumnWidths[i] = Columns[i].Width.Value; fixedW += ColumnWidths[i]; }
            else if (Columns[i].Width.Unit == GridUnit.Auto) { ColumnWidths[i] = Fonts.Measure(Columns[i].Header, 1) + 30; fixedW += ColumnWidths[i]; }
            else stars += Columns[i].Width.Value;
        }
        int rest = Math.Max(0, total - fixedW);
        for (int i = 0; i < n; i++) if (Columns[i].Width.Unit == GridUnit.Star) ColumnWidths[i] = rest * Columns[i].Width.Value / Math.Max(1, stars);
    }

    protected override Size MeasureOverride(Size a)
    {
        var r = Padding;
        int w = a.Width - r.Horizontal - 2 * BorderThickness;
        ComputeWidths(w - 10);
        for (int i = 0; i < _headerRow.Children.Count; i++) _headerRow.Children[i].Width = ColumnWidths[i];
        _headerRow.Measure(new Size(w, a.Height));
        _scroll.Measure(new Size(w, Math.Max(0, a.Height - _headerRow.DesiredSize.Height)));
        return new Size(a.Width, Math.Min(a.Height, _headerRow.DesiredSize.Height + _scroll.DesiredSize.Height + r.Vertical));
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        int hh = _headerRow.DesiredSize.Height;
        _headerRow.Arrange(new Rect(r.X, r.Y, r.Width, hh));
        _scroll.Arrange(new Rect(r.X, r.Y + hh + 2, r.Width, Math.Max(0, r.Height - hh - 2)));
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var h = _headerRow.Bounds;
        dc.FillRect(new Rect(h.X, h.Bottom, h.Width, 1), Theme.StrokeSoft);
    }
}

/// <summary>A text box that suggests matching items in a dropdown (Avalonia AutoCompleteBox).</summary>
public sealed class AutoCompleteBox : Control
{
    readonly TextBox _box = new TextBox();
    Popup? _popup;
    ListBox? _list;
    bool _choosing;
    public List<string> ItemsSource { get; set; } = new List<string>();
    public int MinimumPrefixLength { get; set; } = 1;
    public int MaxDropDownItems { get; set; } = 6;
    public event Action<AutoCompleteBox>? SelectionChanged;

    public AutoCompleteBox()
    {
        AddVisualChild(_box);
        _box.GotFocus += _ => SetPseudo(":focus", true);
        _box.LostFocus += _ => SetPseudo(":focus", false);
        _box.TextChanged += _ => { if (!_choosing) Suggest(); };
        _box.KeyDown += (_, e) =>
        {
            if (_list == null || !(_popup?.IsOpen ?? false)) return;
            if (e.Key == Keys.Down) { _list.SelectedIndex = Math.Min(_list.ItemCount - 1, _list.SelectedIndex + 1); e.Handled = true; }
            else if (e.Key == Keys.Up) { _list.SelectedIndex = Math.Max(0, _list.SelectedIndex - 1); e.Handled = true; }
            else if (e.Key == Keys.Enter && _list.SelectedItem != null) { Choose(_list.SelectedItem.ToString() ?? ""); e.Handled = true; }
            else if (e.Key == Keys.Escape) { ClosePopup(); e.Handled = true; }
        };
    }

    public string Text { get => _box.Text; set => _box.Text = value; }
    public string Watermark { get => _box.Watermark; set => _box.Watermark = value; }
    public bool IsDropDownOpen => _popup != null && _popup.IsOpen;

    public List<string> Matches(string text)
    {
        var l = new List<string>();
        string t = text.ToLowerInvariant();
        foreach (var s in ItemsSource) if (s.ToLowerInvariant().StartsWith(t)) l.Add(s);
        foreach (var s in ItemsSource) if (!l.Contains(s) && s.ToLowerInvariant().Contains(t)) l.Add(s);
        if (l.Count > MaxDropDownItems) l.RemoveRange(MaxDropDownItems, l.Count - MaxDropDownItems);
        return l;
    }

    void Suggest()
    {
        var text = _box.Text;
        var m = text.Length >= MinimumPrefixLength ? Matches(text) : new List<string>();
        if (m.Count == 0 || (m.Count == 1 && m[0] == text)) { ClosePopup(); return; }
        if (_popup == null || !_popup.IsOpen)
        {
            if (Window == null) return;
            _list = new ListBox { Background = Color.Transparent, BorderThickness = 0, Padding = new Thickness(0), Focusable = false };
            _list.ItemActivated += l => { if (l.SelectedItem != null) Choose(l.SelectedItem.ToString() ?? ""); };
            _list.SelectionChanged += l => { };
            _popup = new Popup(new ComboBoxDropDown { Child = _list, MinWidth = Bounds.Width }) { PlacementTarget = this };
            _popup.Open(Window);
        }
        _list!.ItemsSource = m;
        _list.SelectedIndex = 0;
    }

    void Choose(string s)
    {
        _choosing = true;
        _box.Text = s;
        _box.CaretIndex = s.Length;
        _choosing = false;
        ClosePopup();
        SelectionChanged?.Invoke(this);
    }

    void ClosePopup()
    {
        _popup?.Close();
        _popup = null;
    }

    protected override Size MeasureOverride(Size a)
    {
        _box.Measure(new Size(a.Width - Padding.Horizontal - 2 * BorderThickness, a.Height));
        return new Size(_box.DesiredSize.Width + Padding.Horizontal + 2 * BorderThickness, _box.DesiredSize.Height + Padding.Vertical + 2 * BorderThickness);
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        Icon.Draw(dc, IconKind.Search, new Rect(r.Right - 14, r.Y + r.Height / 2 - 7, 14, 14), Theme.TextFaint);
    }

    public new bool Focus() => _box.Focus();
}
