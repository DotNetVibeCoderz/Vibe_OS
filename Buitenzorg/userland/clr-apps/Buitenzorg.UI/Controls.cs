// Buitenzorg.UI -- the basic controls.

namespace Buitenzorg.UI;

public class TextBlock : Control
{
    public static readonly StyledProperty<string> TextProperty = UIProperties.Register("Text", "", affectsMeasure: true);
    public static readonly StyledProperty<TextWrapping> TextWrappingProperty = UIProperties.Register("TextWrapping", TextWrapping.NoWrap, affectsMeasure: true);
    public static readonly StyledProperty<TextAlignment> TextAlignmentProperty = UIProperties.Register("TextAlignment", TextAlignment.Left);
    public static readonly StyledProperty<TextTrimming> TextTrimmingProperty = UIProperties.Register("TextTrimming", TextTrimming.None);

    List<string> _lines = new List<string>();
    string? _linesText;
    int _linesChars = -1, _linesStyle = -1;
    bool _linesWrap;

    public TextBlock() { IsHitTestVisible = false; }
    public TextBlock(string text) : this() { Text = text; }

    public string Text { get => GetValue(TextProperty); set => SetValue(TextProperty, value ?? ""); }
    public TextWrapping TextWrapping { get => GetValue(TextWrappingProperty); set => SetValue(TextWrappingProperty, value); }
    public TextAlignment TextAlignment { get => GetValue(TextAlignmentProperty); set => SetValue(TextAlignmentProperty, value); }
    public TextTrimming TextTrimming { get => GetValue(TextTrimmingProperty); set => SetValue(TextTrimmingProperty, value); }

    /// <summary>Break text into lines that fit <paramref name="maxChars"/> (word wrap, hard breaks on '\n').</summary>
    public static List<string> Wrap(string text, int maxChars, bool wrap)
    {
        var lines = new List<string>();
        foreach (var para in text.Split('\n'))
        {
            if (!wrap || para.Length <= maxChars || maxChars <= 0) { lines.Add(para); continue; }
            int start = 0;
            while (start < para.Length)
            {
                int len = Math.Min(maxChars, para.Length - start);
                if (start + len < para.Length)
                {
                    int sp = para.LastIndexOf(' ', start + len, len + 1);
                    if (sp > start) len = sp - start;
                }
                lines.Add(para.Substring(start, len).TrimEnd());
                start += len;
                while (start < para.Length && para[start] == ' ') start++;
            }
        }
        return lines;
    }

    protected override Size MeasureOverride(Size a)
    {
        int style = TextStyle;
        int gw = Fonts.GlyphWidth(style);
        var pad = Padding;
        int maxChars = Math.Max(1, (a.Width - pad.Horizontal) / gw);
        bool wrap = TextWrapping == TextWrapping.Wrap;
        string text = Text;
        if (!ReferenceEquals(text, _linesText) || (wrap && maxChars != _linesChars) || style != _linesStyle || wrap != _linesWrap)
        {
            _lines = Wrap(text, maxChars, wrap);
            _linesText = text; _linesChars = maxChars; _linesStyle = style; _linesWrap = wrap;
        }
        int w = 0;
        foreach (var l in _lines) w = Math.Max(w, l.Length * gw);
        return new Size(w + pad.Horizontal, _lines.Count * (Fonts.LineHeight(style) + 2) - 2 + pad.Vertical);
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        int style = TextStyle;
        int gw = Fonts.GlyphWidth(style), lh = Fonts.LineHeight(style) + 2;
        var r = ContentRect(Bounds);
        int y = r.Y;
        int maxChars = Math.Max(0, r.Width / gw);
        foreach (var raw in _lines)
        {
            string line = raw;
            if (TextTrimming == TextTrimming.CharacterEllipsis && line.Length > maxChars && maxChars > 1)
                line = line.Substring(0, maxChars - 1) + "~";
            int lw = line.Length * gw;
            int x = TextAlignment == TextAlignment.Center ? r.X + (r.Width - lw) / 2 : TextAlignment == TextAlignment.Right ? r.Right - lw : r.X;
            dc.Text(x, y, line, Foreground, style);
            y += lh;
        }
    }
}

/// <summary>Text that focuses <see cref="Target"/> when clicked (Avalonia Label).</summary>
public sealed class Label : ContentControl
{
    public Control? Target { get; set; }
    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        Target?.Focus();
        base.OnPointerPressed(e);
    }
}

public class Button : ContentControl
{
    public static readonly StyledProperty<ICommand?> CommandProperty = UIProperties.Register<ICommand?>("Command", null);

    public event Action<Button>? Click;
    public object? CommandParameter { get; set; }
    /// <summary>Clicked by Enter anywhere in the window (Avalonia IsDefault).</summary>
    public bool IsDefault { get; set; }
    /// <summary>Clicked by Escape anywhere in the window.</summary>
    public bool IsCancel { get; set; }
    /// <summary>A flyout shown under the button when clicked.</summary>
    public FlyoutBase? Flyout { get; set; }

    public Button()
    {
        Focusable = true;
        Transitions = new List<Transition> { new Transition(BackgroundProperty, 110), new Transition(BorderBrushProperty, 110) };
    }

    public Button(object content, Action<Button>? click = null) : this()
    {
        Content = content;
        if (click != null) Click += click;
    }

    public ICommand? Command
    {
        get => GetValue(CommandProperty);
        set
        {
            SetValue(CommandProperty, value);
            if (value != null)
            {
                value.CanExecuteChanged += () => IsEnabled = value.CanExecute(CommandParameter);
                IsEnabled = value.CanExecute(CommandParameter);
            }
        }
    }

    protected override void OnAttached()
    {
        base.OnAttached();
        if ((IsDefault || IsCancel) && Window != null)
            Window.UnhandledKey += e =>
            {
                if (!IsEffectivelyEnabled || !IsVisible) return;
                if ((IsDefault && e.Key == Keys.Enter) || (IsCancel && e.Key == Keys.Escape)) { e.Handled = true; PerformClick(); }
            };
    }

    public void PerformClick()
    {
        if (!IsEffectivelyEnabled) return;
        OnClick();
    }

    protected virtual void OnClick()
    {
        Click?.Invoke(this);
        var cmd = Command;
        if (cmd != null && cmd.CanExecute(CommandParameter)) cmd.Execute(CommandParameter);
        if (Flyout != null && Window != null) Flyout.ShowAt(this);
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        SetPseudo(":pressed", true);
        e.Handled = true;
        base.OnPointerPressed(e);
    }

    protected internal override void OnPointerReleased(PointerEventArgs e)
    {
        bool was = IsPressed;
        SetPseudo(":pressed", false);
        e.Handled = true;
        if (was && Bounds.Contains(e.X, e.Y)) OnClick();
        base.OnPointerReleased(e);
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.Enter || e.Key == Keys.Space) { e.Handled = true; PerformClick(); return; }
        base.OnKeyDown(e);
    }

    protected override Control? MakePresenter(object? content)
    {
        var p = base.MakePresenter(content);
        if (p is TextBlock t) { t.HorizontalAlignment = HorizontalAlignment.Center; t.VerticalAlignment = VerticalAlignment.Center; }
        return p;
    }
}

/// <summary>A button that keeps clicking while held (Avalonia RepeatButton).</summary>
public sealed class RepeatButton : Button
{
    DispatcherTimer? _timer;
    public int Delay { get; set; } = 400;
    public int Interval { get; set; } = 90;

    public RepeatButton() { }
    public RepeatButton(object content, Action<Button>? click = null) : base(content, click) { }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        base.OnPointerPressed(e);
        PerformClick();
        _timer?.Stop();
        _timer = new DispatcherTimer(Delay);
        _timer.Tick += () => { if (!IsPressed) { _timer.Stop(); return; } _timer.IntervalMs = Interval; PerformClick(); };
        _timer.Start();
    }

    protected internal override void OnPointerReleased(PointerEventArgs e)
    {
        _timer?.Stop();
        SetPseudo(":pressed", false);
        e.Handled = true;
    }
}

public class ToggleButton : Button
{
    public static readonly StyledProperty<bool> IsCheckedProperty = UIProperties.Register("IsChecked", false);
    public event Action<ToggleButton>? IsCheckedChanged;

    public ToggleButton() { }
    public ToggleButton(object content) : base(content) { }

    public bool IsChecked { get => GetValue(IsCheckedProperty); set => SetValue(IsCheckedProperty, value); }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == IsCheckedProperty)
        {
            SetPseudo(":checked", IsChecked);
            IsCheckedChanged?.Invoke(this);
        }
    }

    protected override void OnClick()
    {
        Toggle();
        base.OnClick();
    }

    protected virtual void Toggle() => IsChecked = !IsChecked;
}

public class CheckBox : ToggleButton
{
    public CheckBox() { }
    public CheckBox(string text, bool isChecked = false) { Content = text; IsChecked = isChecked; }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(new Size(Math.Max(0, a.Width - 28), a.Height));
        return new Size(s.Width + 28, Math.Max(s.Height, 20));
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        Presenter?.Arrange(new Rect(r.X + 28, r.Y, Math.Max(0, r.Width - 28), r.Height));
    }

    public override void Render(DrawingContext dc)
    {
        var r = ContentRect(Bounds);
        var box = new Rect(r.X, r.Y + (r.Height - 18) / 2, 18, 18);
        if (IsChecked)
        {
            dc.FillRoundRect(box, 5, IsEffectivelyEnabled ? Theme.Accent : Theme.TextFaint);
            int x = box.X, y = box.Y;
            for (int i = 0; i < 2; i++)
            {
                dc.Line(x + 4, y + 9 + i, x + 7, y + 12 + i, Theme.OnAccent);
                dc.Line(x + 7, y + 12 + i, x + 13, y + 5 + i, Theme.OnAccent);
            }
        }
        else
        {
            dc.FillRoundRect(box, 5, BorderBrush);
            dc.FillRoundRect(box.Inflate(-1), 4, Background);
        }
    }
}

public sealed class RadioButton : ToggleButton
{
    public string GroupName { get; set; } = "";

    public RadioButton() { }
    public RadioButton(string text, string group = "") { Content = text; GroupName = group; }

    protected override void Toggle()
    {
        if (IsChecked) return;
        IsChecked = true;
        // uncheck the others of the group (same parent when no group name)
        var scope = GroupName.Length > 0 ? (Control?)Window : Parent;
        if (scope == null) return;
        foreach (var d in scope.Descendants())
            if (d is RadioButton rb && rb != this && rb.GroupName == GroupName && (GroupName.Length > 0 || rb.Parent == Parent)) rb.IsChecked = false;
    }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(new Size(Math.Max(0, a.Width - 28), a.Height));
        return new Size(s.Width + 28, Math.Max(s.Height, 20));
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        Presenter?.Arrange(new Rect(r.X + 28, r.Y, Math.Max(0, r.Width - 28), r.Height));
    }

    public override void Render(DrawingContext dc)
    {
        var r = ContentRect(Bounds);
        var o = new Rect(r.X, r.Y + (r.Height - 18) / 2, 18, 18);
        dc.FillRoundRect(o, 9, IsChecked ? Theme.Accent : BorderBrush);
        dc.FillRoundRect(o.Inflate(-2), 7, IsChecked ? Theme.Accent : Background);
        if (IsChecked) dc.FillRoundRect(o.Inflate(-5), 4, Theme.OnAccent);
    }
}

/// <summary>An on/off switch with a sliding knob (Avalonia ToggleSwitch).</summary>
public sealed class ToggleSwitch : ToggleButton
{
    public static readonly StyledProperty<int> KnobProperty = UIProperties.Register("Knob", 0);
    public string OnContent { get; set; } = "Aktif";
    public string OffContent { get; set; } = "Nonaktif";

    public ToggleSwitch() { }
    public ToggleSwitch(string header, bool on = false) { Content = header; IsChecked = on; SetValue(KnobProperty, on ? 256 : 0); }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == IsCheckedProperty)
        {
            int from = GetValue(KnobProperty), to = IsChecked ? 256 : 0;
            SetValue(KnobProperty, to);
            if (IsAttached) Animator.Start(this, KnobProperty, from, to, 160, false);
        }
    }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(a);
        int gw = Fonts.GlyphWidth(0);
        return new Size(Math.Max(s.Width, 44 + 10 + Math.Max(OnContent.Length, OffContent.Length) * gw), s.Height + 28);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        Presenter?.Arrange(new Rect(r.X, r.Y, r.Width, Math.Min(r.Height, Presenter.DesiredSize.Height)));
    }

    public override void Render(DrawingContext dc)
    {
        var r = ContentRect(Bounds);
        int top = r.Bottom - 22;
        var track = new Rect(r.X, top, 44, 22);
        int k = GetValue(KnobProperty);
        int on = IsEffectivelyEnabled ? Theme.Accent : Theme.TextFaint;
        dc.FillRoundRect(track, 11, Color.Mix(BorderBrush, on, k));
        if (k < 200) dc.FillRoundRect(track.Inflate(-1), 10, Color.Mix(Background, on, k));
        int kx = track.X + 3 + (track.Width - 22) * k / 256;
        dc.FillRoundRect(new Rect(kx, top + 3, 16, 16), 8, k > 128 ? Theme.OnAccent : Theme.TextMuted);
        dc.Text(track.Right + 10, top + 3, IsChecked ? OnContent : OffContent, Foreground, 0);
    }
}

public sealed class HyperlinkButton : Button
{
    public HyperlinkButton() { }
    public HyperlinkButton(string text, Action<Button>? click = null) : base(text, click) { }

    public override void Render(DrawingContext dc)
    {
        var r = ContentRect(Bounds);
        if (IsPointerOver) dc.FillRect(new Rect(r.X, r.Bottom - 1, r.Width, 1), Foreground);
    }
}

/// <summary>A button with a chevron that opens its <see cref="Button.Flyout"/> (Avalonia DropDownButton).</summary>
public sealed class DropDownButton : Button
{
    public DropDownButton() { }
    public DropDownButton(object content) : base(content) { }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(a);
        return new Size(s.Width + 18, s.Height);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        Presenter?.Arrange(new Rect(r.X, r.Y, Math.Max(0, r.Width - 18), r.Height));
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        Icon.Draw(dc, IconKind.ChevronDown, new Rect(r.Right - 12, r.Y + r.Height / 2 - 6, 12, 12), Foreground);
    }
}

/// <summary>A primary action plus a chevron that opens more (Avalonia SplitButton).</summary>
public sealed class SplitButton : Button
{
    bool _onChevron;
    public SplitButton() { }
    public SplitButton(object content, Action<Button>? click = null) : base(content, click) { }

    protected override Size MeasureOverride(Size a)
    {
        var s = base.MeasureOverride(a);
        return new Size(s.Width + 30, s.Height);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        Presenter?.Arrange(new Rect(r.X, r.Y, Math.Max(0, r.Width - 30), r.Height));
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        _onChevron = e.X >= Bounds.Right - 30;
        base.OnPointerPressed(e);
    }

    protected override void OnClick()
    {
        if (_onChevron) { _onChevron = false; if (Flyout != null) Flyout.ShowAt(this); return; }
        var f = Flyout;
        Flyout = null;
        base.OnClick();
        Flyout = f;
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var b = Bounds;
        dc.FillRect(new Rect(b.Right - 30, b.Y + 6, 1, b.Height - 12), BorderBrush);
        Icon.Draw(dc, IconKind.ChevronDown, new Rect(b.Right - 21, b.Y + b.Height / 2 - 6, 12, 12), Foreground);
    }
}

public sealed class Separator : Control
{
    public Separator() { Height = 1; IsHitTestVisible = false; }
}

/// <summary>Minimum..Maximum with a draggable thumb (Avalonia Slider).</summary>
public sealed class Slider : Control
{
    public static readonly StyledProperty<double> ValueProperty = UIProperties.Register("Value", 0.0);
    public double Minimum { get; set; }
    public double Maximum { get; set; } = 100;
    public double SmallChange { get; set; } = 1;
    public double TickFrequency { get; set; }
    public bool IsSnapToTickEnabled { get; set; }
    public event Action<Slider>? ValueChanged;
    bool _drag;

    public Slider() { Focusable = true; MinWidth = 80; }

    public double Value
    {
        get => GetValue(ValueProperty);
        set
        {
            double v = Math.Max(Minimum, Math.Min(Maximum, value));
            if (IsSnapToTickEnabled && TickFrequency > 0) v = Minimum + Math.Round((v - Minimum) / TickFrequency) * TickFrequency;
            SetValue(ValueProperty, v);
        }
    }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == ValueProperty) ValueChanged?.Invoke(this);
    }

    protected override Size MeasureOverride(Size a) => new Size(160, 24);

    int ThumbX()
    {
        var b = Bounds;
        double t = Maximum > Minimum ? (Value - Minimum) / (Maximum - Minimum) : 0;
        return b.X + 9 + (int)((b.Width - 18) * t);
    }

    void SetFromX(int x)
    {
        var b = Bounds;
        double t = Math.Max(0, Math.Min(1, (x - b.X - 9) / (double)Math.Max(1, b.Width - 18)));
        Value = Minimum + t * (Maximum - Minimum);
    }

    protected internal override void OnPointerPressed(PointerEventArgs e) { _drag = true; SetPseudo(":pressed", true); SetFromX(e.X); e.Handled = true; }
    protected internal override void OnPointerMoved(PointerEventArgs e) { if (_drag && e.LeftButton) { SetFromX(e.X); e.Handled = true; } }
    protected internal override void OnPointerReleased(PointerEventArgs e) { _drag = false; SetPseudo(":pressed", false); }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        double step = TickFrequency > 0 ? TickFrequency : SmallChange;
        if (e.Key == Keys.Right || e.Key == Keys.Up) { Value += step; e.Handled = true; }
        else if (e.Key == Keys.Left || e.Key == Keys.Down) { Value -= step; e.Handled = true; }
        else if (e.Key == Keys.Home) { Value = Minimum; e.Handled = true; }
        else if (e.Key == Keys.End) { Value = Maximum; e.Handled = true; }
        base.OnKeyDown(e);
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        int cy = b.Y + b.Height / 2;
        int tx = ThumbX();
        int accent = IsEffectivelyEnabled ? Theme.Accent : Theme.TextFaint;
        dc.FillRoundRect(new Rect(b.X + 4, cy - 2, b.Width - 8, 4), 2, Background);
        dc.FillRoundRect(new Rect(b.X + 4, cy - 2, Math.Max(0, tx - b.X - 4), 4), 2, accent);
        if (TickFrequency > 0 && Maximum > Minimum)
        {
            int n = (int)((Maximum - Minimum) / TickFrequency);
            for (int i = 0; i <= n && n <= 40; i++)
                dc.FillRect(new Rect(b.X + 9 + (b.Width - 18) * i / Math.Max(1, n), cy + 7, 1, 3), Theme.TextFaint);
        }
        int r = IsPressed ? 8 : IsPointerOver ? 9 : 8;
        dc.FillRoundRect(new Rect(tx - 9, cy - 9, 18, 18), 9, Theme.Variant == ThemeVariant.Dark ? 0xF4FAF6 : 0xFFFFFF);
        dc.FillRoundRect(new Rect(tx - r + 3, cy - r + 3, 2 * r - 6, 2 * r - 6), r - 3, accent);
    }
}

public sealed class ProgressBar : Control
{
    public static readonly StyledProperty<double> ValueProperty = UIProperties.Register("Value", 0.0);
    public double Minimum { get; set; }
    public double Maximum { get; set; } = 100;
    public bool ShowProgressText { get; set; }
    bool _indeterminate;
    DispatcherTimer? _timer;
    int _phase;

    public ProgressBar() { MinWidth = 60; }

    public double Value { get => GetValue(ValueProperty); set => SetValue(ValueProperty, Math.Max(Minimum, Math.Min(Maximum, value))); }

    /// <summary>A band sweeping back and forth while the amount of work is unknown.</summary>
    public bool IsIndeterminate
    {
        get => _indeterminate;
        set
        {
            _indeterminate = value;
            if (value && _timer == null)
            {
                _timer = new DispatcherTimer(60);
                _timer.Tick += () => { _phase = (_phase + 6) % 512; InvalidateVisual(); };
                _timer.Start();
            }
            else if (!value && _timer != null) { _timer.Stop(); _timer = null; }
        }
    }

    protected override Size MeasureOverride(Size a) => new Size(120, ShowProgressText ? 18 : 8);

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        int h = Math.Min(b.Height, 8);
        var track = new Rect(b.X, b.Y + (b.Height - h) / 2, b.Width, h);
        dc.FillRoundRect(track, h / 2, Background);
        if (_indeterminate)
        {
            int p = _phase < 256 ? _phase : 511 - _phase;
            int w = b.Width / 3;
            dc.FillRoundRect(new Rect(b.X + (b.Width - w) * p / 256, track.Y, w, h), h / 2, Theme.Accent);
            return;
        }
        double t = Maximum > Minimum ? (Value - Minimum) / (Maximum - Minimum) : 0;
        int fw = (int)(b.Width * t);
        if (fw > 0) dc.FillGradient(new Rect(b.X, track.Y, Math.Max(fw, h), h), h / 2, Theme.AccentHi, Theme.Accent);
        if (ShowProgressText)
        {
            string s = ((int)(t * 100)) + "%";
            dc.Text(b.Right - Fonts.Measure(s, 0), b.Y, s, Theme.TextMuted, 0);
        }
    }
}

/// <summary>Editable text with a caret (Avalonia TextBox): single or multi-line, watermark,
/// password char, max length, read-only.</summary>
public class TextBox : Control
{
    public static readonly StyledProperty<string> TextProperty = UIProperties.Register("Text", "", affectsMeasure: true);
    public string Watermark { get; set; } = "";
    public bool AcceptsReturn { get; set; }
    public bool IsReadOnly { get; set; }
    public char PasswordChar { get; set; }
    public int MaxLength { get; set; }
    public int CaretIndex { get; set; }
    public event Action<TextBox>? TextChanged;
    int _scrollX, _scrollLine;
    int _followedCaret = -1, _followedLen = -1;
    /// <summary>First line shown by a multi-line box (scrolled by the caret or the wheel).</summary>
    public int FirstVisibleLine => _scrollLine;
    bool _caretOn = true;
    DispatcherTimer? _blink;
    long _lastKey;

    public TextBox() { Focusable = true; MinWidth = 60; }

    public string Text
    {
        get => GetValue(TextProperty);
        set
        {
            SetValue(TextProperty, value ?? "");
            CaretIndex = Math.Min(CaretIndex, Text.Length);
        }
    }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == TextProperty) TextChanged?.Invoke(this);
    }

    protected internal override void OnGotFocus()
    {
        base.OnGotFocus();
        _blink ??= new DispatcherTimer(530);
        _blink.Tick += Blink;
        _blink.Start();
        _caretOn = true;
        _lastKey = Clock.Now;
    }

    protected internal override void OnLostFocus()
    {
        base.OnLostFocus();
        if (_blink != null) { _blink.Tick -= Blink; _blink.Stop(); }
    }

    // Every blink recomposes the desktop, so an idle caret stops and stays on.
    void Blink()
    {
        if (Clock.Now - _lastKey > 10000) { _caretOn = true; _blink?.Stop(); }
        else _caretOn = !_caretOn;
        InvalidateVisual();
    }

    int Lh => Fonts.LineHeight(TextStyle) + 2;
    int Gw => Fonts.GlyphWidth(TextStyle);

    protected override Size MeasureOverride(Size a)
    {
        var pad = Padding;
        int lines = AcceptsReturn ? Math.Max(3, Text.Split('\n').Length) : 1;
        if (AcceptsReturn && Height < 0) lines = Math.Min(lines, 12);
        return new Size(Math.Min(a.Width, Math.Max(MinWidth, Fonts.Measure(Text.Length > 0 ? Text : Watermark, TextStyle) + 4)) + 2 * BorderThickness + pad.Horizontal,
            lines * Lh + pad.Vertical + 2 * BorderThickness);
    }

    void Edit(string text, int caret)
    {
        if (MaxLength > 0 && text.Length > MaxLength) return;
        CaretIndex = caret;
        Text = text;
        _caretOn = true;
    }

    (int line, int col) LineCol(int index)
    {
        int line = 0, col = 0;
        for (int i = 0; i < index && i < Text.Length; i++)
        {
            if (Text[i] == '\n') { line++; col = 0; } else col++;
        }
        return (line, col);
    }

    int IndexOf(int line, int col)
    {
        int l = 0, i = 0;
        while (i < Text.Length && l < line) { if (Text[i] == '\n') l++; i++; }
        int c = 0;
        while (i < Text.Length && Text[i] != '\n' && c < col) { i++; c++; }
        return i;
    }

    protected internal override void OnKeyDown(KeyEventArgs e)
    {
        _lastKey = Clock.Now;
        _blink?.Start();
        string t = Text;
        int c = Math.Min(CaretIndex, t.Length);
        char k = e.Key;
        if (k == Keys.Left) { CaretIndex = Math.Max(0, c - 1); }
        else if (k == Keys.Right) { CaretIndex = Math.Min(t.Length, c + 1); }
        else if (k == Keys.Home) { var lc = LineCol(c); CaretIndex = IndexOf(lc.line, 0); }
        else if (k == Keys.End) { var lc = LineCol(c); CaretIndex = IndexOf(lc.line, int.MaxValue); }
        else if ((k == Keys.Up || k == Keys.Down) && AcceptsReturn)
        {
            var lc = LineCol(c);
            int target = lc.line + (k == Keys.Up ? -1 : 1);
            if (target < 0) target = 0;
            CaretIndex = IndexOf(target, lc.col);
        }
        else if (IsReadOnly) { base.OnKeyDown(e); return; }
        else if (k == Keys.Backspace) { if (c > 0) Edit(t.Remove(c - 1, 1), c - 1); }
        else if (k == Keys.Delete) { if (c < t.Length) Edit(t.Remove(c, 1), c); }
        else if (k == Keys.Enter && AcceptsReturn) Edit(t.Insert(c, "\n"), c + 1);
        else if (Keys.IsPrintable(k)) Edit(t.Insert(c, k.ToString()), c + 1);
        else { base.OnKeyDown(e); return; }
        e.Handled = true;
        _caretOn = true;
        InvalidateVisual();
    }

    protected internal override void OnPointerPressed(PointerEventArgs e)
    {
        var r = ContentRect(Bounds);
        int line = AcceptsReturn ? _scrollLine + Math.Max(0, (e.Y - r.Y) / Lh) : 0;
        int col = Math.Max(0, (e.X - r.X + Gw / 2) / Gw + (AcceptsReturn ? 0 : _scrollX));
        CaretIndex = IndexOf(line, col);
        _caretOn = true;
        _lastKey = Clock.Now;
        _blink?.Start();
        InvalidateVisual();
        e.Handled = true;
        base.OnPointerPressed(e);
    }

    protected internal override void OnPointerWheelChanged(PointerEventArgs e)
    {
        if (AcceptsReturn)
        {
            int lines = LineCol(Text.Length).Item1 + 1;
            int visible = Math.Max(1, ContentRect(Bounds).Height / Lh);
            int max = Math.Max(0, lines - visible);
            int next = Math.Max(0, Math.Min(max, _scrollLine - e.WheelDelta * 3));
            if (next != _scrollLine)
            {
                _scrollLine = next;
                e.Handled = true;
                InvalidateVisual();
            }
        }
        base.OnPointerWheelChanged(e);
    }

    public override void Render(DrawingContext dc)
    {
        base.Render(dc);
        var r = ContentRect(Bounds);
        int style = TextStyle, gw = Gw, lh = Lh;
        string text = PasswordChar != '\0' ? new string(PasswordChar, Text.Length) : Text;
        var (cl, cc) = LineCol(CaretIndex);
        int visibleCols = Math.Max(1, r.Width / gw), visibleLines = Math.Max(1, r.Height / lh);
        if (!AcceptsReturn)
        {
            if (cc < _scrollX) _scrollX = cc;
            if (cc > _scrollX + visibleCols - 1) _scrollX = cc - visibleCols + 1;
        }
        else if (CaretIndex != _followedCaret || Text.Length != _followedLen)
        {
            // follow the caret only when it moved or the text changed, so the wheel can
            // scroll away from it
            if (cl < _scrollLine) _scrollLine = cl;
            if (cl > _scrollLine + visibleLines - 1) _scrollLine = cl - visibleLines + 1;
        }
        _followedCaret = CaretIndex;
        _followedLen = Text.Length;
        dc.PushClip(r);
        if (text.Length == 0 && Watermark.Length > 0 && !IsFocused)
            dc.Text(r.X, r.Y + (AcceptsReturn ? 0 : (r.Height - lh) / 2 + 1), Watermark, Theme.TextFaint, style);
        else
        {
            var lines = text.Split('\n');
            int y = r.Y + (AcceptsReturn ? 0 : (r.Height - lh) / 2 + 1);
            for (int i = AcceptsReturn ? _scrollLine : 0; i < lines.Length && y < r.Bottom; i++)
            {
                string s = lines[i];
                if (!AcceptsReturn) s = _scrollX < s.Length ? s.Substring(_scrollX) : "";
                dc.Text(r.X, y, s, IsEffectivelyEnabled ? Foreground : Theme.TextFaint, style);
                y += lh;
            }
        }
        if (IsFocused && _caretOn && !IsReadOnly)
        {
            int x = r.X + (cc - (AcceptsReturn ? 0 : _scrollX)) * gw;
            int y = r.Y + (AcceptsReturn ? (cl - _scrollLine) * lh : (r.Height - lh) / 2 + 1);
            dc.FillRect(new Rect(x, y, 2, lh - 2), Theme.Accent);
        }
        dc.PopClip();
    }
}

/// <summary>A number with +/- spinner buttons (Avalonia NumericUpDown).</summary>
public sealed class NumericUpDown : Control
{
    public static readonly StyledProperty<double> ValueProperty = UIProperties.Register("Value", 0.0);
    readonly TextBox _box = new TextBox();
    readonly RepeatButton _up = new RepeatButton("+");
    readonly RepeatButton _down = new RepeatButton("-");
    public double Minimum { get; set; } = double.MinValue;
    public double Maximum { get; set; } = double.MaxValue;
    public double Increment { get; set; } = 1;
    public int Decimals { get; set; }
    public event Action<NumericUpDown>? ValueChanged;
    bool _sync;

    public NumericUpDown()
    {
        foreach (var b in new[] { _up, _down })
        {
            b.Classes.Add("subtle");
            b.Padding = new Thickness(8, 2);
            b.Focusable = false;
        }
        _up.Click += _ => Value += Increment;
        _down.Click += _ => Value -= Increment;
        _box.TextChanged += _ =>
        {
            if (_sync) return;
            if (double.TryParse(_box.Text, out double v)) { _sync = true; SetValue(ValueProperty, Math.Max(Minimum, Math.Min(Maximum, v))); _sync = false; }
        };
        _box.KeyDown += (_, e) =>
        {
            if (e.Key == Keys.Up) { Value += Increment; e.Handled = true; }
            else if (e.Key == Keys.Down) { Value -= Increment; e.Handled = true; }
        };
        AddVisualChild(_box);
        AddVisualChild(_down);
        AddVisualChild(_up);
        Sync();
    }

    public double Value
    {
        get => GetValue(ValueProperty);
        set => SetValue(ValueProperty, Math.Max(Minimum, Math.Min(Maximum, Math.Round(value, Decimals))));
    }

    protected override void OnPropertyChanged(UIProperty p)
    {
        base.OnPropertyChanged(p);
        if (p == ValueProperty) { if (!_sync) Sync(); ValueChanged?.Invoke(this); }
    }

    void Sync()
    {
        _sync = true;
        _box.Text = Decimals == 0 ? ((long)Math.Round(Value)).ToString() : Value.ToString("F" + Decimals);
        _box.CaretIndex = _box.Text.Length;
        _sync = false;
    }

    protected override Size MeasureOverride(Size a)
    {
        var r = Padding;
        _box.Measure(new Size(Math.Max(0, a.Width - 70), a.Height));
        _up.Measure(a);
        _down.Measure(a);
        return new Size(Math.Max(120, _box.DesiredSize.Width + 70) + r.Horizontal, Math.Max(_box.DesiredSize.Height, _up.DesiredSize.Height) + r.Vertical);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = ContentRect(b);
        int bw = 30;
        _box.Arrange(new Rect(r.X, r.Y, r.Width - 2 * bw, r.Height));
        _down.Arrange(new Rect(r.Right - 2 * bw, b.Y + 2, bw, b.Height - 4));
        _up.Arrange(new Rect(r.Right - bw, b.Y + 2, bw, b.Height - 4));
    }
}

/// <summary>A header that expands to show its content (Avalonia Expander).</summary>
public sealed class Expander : Control
{
    public static readonly StyledProperty<bool> IsExpandedProperty = UIProperties.Register("IsExpanded", false, affectsMeasure: true);
    readonly Button _header;
    Control? _content;
    public event Action<Expander>? Expanded;

    public Expander(string header, Control? content = null, bool expanded = false)
    {
        _header = new Button(new TextBlock(header) { FontWeight = FontWeight.Bold });
        _header.Classes.Add("subtle");
        _header.HorizontalAlignment = HorizontalAlignment.Stretch;
        _header.Padding = new Thickness(12, 9, 36, 9);
        _header.Click += _ => IsExpanded = !IsExpanded;
        AddVisualChild(_header);
        Content = content;
        IsExpanded = expanded;
    }

    public bool IsExpanded
    {
        get => GetValue(IsExpandedProperty);
        set { SetValue(IsExpandedProperty, value); SetPseudo(":expanded", value); if (_content != null) _content.IsVisible = value; if (value) Expanded?.Invoke(this); }
    }

    public Control? Content
    {
        get => _content;
        set
        {
            if (_content != null) RemoveVisualChild(_content);
            _content = value;
            if (value != null) { value.Margin = new Thickness(12, 0, 12, 12); value.IsVisible = IsExpanded; AddVisualChild(value); }
        }
    }

    protected override Size MeasureOverride(Size a)
    {
        _header.Measure(a);
        int w = _header.DesiredSize.Width, h = _header.DesiredSize.Height;
        if (_content != null && IsExpanded)
        {
            _content.Measure(new Size(a.Width, Size.Infinite));
            w = Math.Max(w, _content.DesiredSize.Width);
            h += _content.DesiredSize.Height;
        }
        return new Size(w + 2 * BorderThickness, h + 2 * BorderThickness);
    }

    protected override void ArrangeOverride(Rect b)
    {
        var r = b.Deflate(new Thickness(BorderThickness));
        int hh = _header.DesiredSize.Height;
        _header.Arrange(new Rect(r.X, r.Y, r.Width, hh));
        _content?.Arrange(new Rect(r.X, r.Y + hh, r.Width, Math.Max(0, r.Height - hh)));
    }

    protected override void RenderOverlay(DrawingContext dc)
    {
        var h = _header.Bounds;
        Icon.Draw(dc, IsExpanded ? IconKind.ChevronUp : IconKind.ChevronDown, new Rect(h.Right - 26, h.Y + h.Height / 2 - 6, 12, 12), Theme.TextMuted);
    }
}

/// <summary>Shows a <see cref="Bitmap"/> (Avalonia Image), scaled once per size.</summary>
public sealed class Image : Control
{
    Bitmap? _source, _scaled;
    public Stretch Stretch { get; set; } = Stretch.Uniform;

    public Image() { IsHitTestVisible = false; }

    public Bitmap? Source { get => _source; set { _source = value; _scaled = null; InvalidateMeasure(); } }

    protected override Size MeasureOverride(Size a)
    {
        if (_source == null) return new Size(0, 0);
        if (Stretch == Stretch.None) return new Size(_source.Width, _source.Height);
        int w = Math.Min(a.Width, _source.Width * 4), h = Math.Min(a.Height, _source.Height * 4);
        if (Stretch == Stretch.Uniform)
        {
            if (w * _source.Height > h * _source.Width) w = h * _source.Width / _source.Height;
            else h = w * _source.Height / _source.Width;
        }
        return new Size(w, h);
    }

    public override void Render(DrawingContext dc)
    {
        if (_source == null) return;
        var b = Bounds;
        int w = b.Width, h = b.Height;
        if (Stretch == Stretch.None) { w = _source.Width; h = _source.Height; }
        else if (Stretch == Stretch.Uniform)
        {
            if (w * _source.Height > h * _source.Width) w = h * _source.Width / _source.Height;
            else h = w * _source.Height / _source.Width;
        }
        if (w <= 0 || h <= 0) return;
        if (_scaled == null || _scaled.Width != w || _scaled.Height != h)
            _scaled = w == _source.Width && h == _source.Height ? _source : _source.Scale(w, h);
        dc.Blit(b.X + (b.Width - w) / 2, b.Y + (b.Height - h) / 2, w, h, _scaled.Pixels);
    }
}

public enum IconKind
{
    None, ChevronDown, ChevronUp, ChevronLeft, ChevronRight, Plus, Minus, Check, Close, Search, Menu,
    Folder, File, Gear, Play, Pause, Refresh, Sun, Moon, Leaf, Trash, Save, Clock, Grid, Cpu, Info, Warning, Home,
}

/// <summary>A small vector glyph drawn with lines and shapes (Avalonia PathIcon).</summary>
public sealed class Icon : Control
{
    public IconKind Kind { get; set; }
    public int IconSize { get; set; } = 16;

    public Icon() { IsHitTestVisible = false; }
    public Icon(IconKind kind, int size = 16) : this() { Kind = kind; IconSize = size; }

    protected override Size MeasureOverride(Size a) => new Size(IconSize, IconSize);

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        Draw(dc, Kind, new Rect(b.X + (b.Width - IconSize) / 2, b.Y + (b.Height - IconSize) / 2, IconSize, IconSize), Foreground);
    }

    public static void Draw(DrawingContext dc, IconKind kind, Rect r, int c)
    {
        int x = r.X, y = r.Y, w = r.Width, h = r.Height, cx = x + w / 2, cy = y + h / 2, s = Math.Min(w, h);
        void L(int x0, int y0, int x1, int y1) { dc.Line(x0, y0, x1, y1, c); dc.Line(x0 + 1, y0, x1 + 1, y1, c); }
        switch (kind)
        {
            case IconKind.ChevronDown: L(x + 1, cy - 2, cx, cy + 3); L(cx, cy + 3, x + w - 2, cy - 2); break;
            case IconKind.ChevronUp: L(x + 1, cy + 2, cx, cy - 3); L(cx, cy - 3, x + w - 2, cy + 2); break;
            case IconKind.ChevronLeft: L(cx + 2, y + 1, cx - 3, cy); L(cx - 3, cy, cx + 2, y + h - 2); break;
            case IconKind.ChevronRight: L(cx - 2, y + 1, cx + 3, cy); L(cx + 3, cy, cx - 2, y + h - 2); break;
            case IconKind.Plus: dc.FillRect(new Rect(x + 2, cy - 1, w - 4, 2), c); dc.FillRect(new Rect(cx - 1, y + 2, 2, h - 4), c); break;
            case IconKind.Minus: dc.FillRect(new Rect(x + 2, cy - 1, w - 4, 2), c); break;
            case IconKind.Check: L(x + 2, cy, cx - 1, y + h - 3); L(cx - 1, y + h - 3, x + w - 2, y + 3); break;
            case IconKind.Close: L(x + 3, y + 3, x + w - 4, y + h - 4); L(x + w - 4, y + 3, x + 3, y + h - 4); break;
            case IconKind.Search:
                dc.Ellipse(new Rect(x + 1, y + 1, s * 2 / 3, s * 2 / 3), c, false);
                L(x + s * 2 / 3 - 1, y + s * 2 / 3 - 1, x + w - 2, y + h - 2); break;
            case IconKind.Menu: for (int i = 0; i < 3; i++) dc.FillRoundRect(new Rect(x + 2, y + 3 + i * (h - 6) / 2 - 1, w - 4, 2), 1, c); break;
            case IconKind.Folder:
                dc.FillRoundRect(new Rect(x + 1, y + 2, w / 2, 4), 1, c);
                dc.FillRoundRect(new Rect(x + 1, y + 4, w - 2, h - 6), 2, c); break;
            case IconKind.File:
                dc.StrokeRoundRect(new Rect(x + 3, y + 1, w - 6, h - 2), 2, c);
                dc.FillRect(new Rect(x + 6, y + 6, w - 12, 1), c); dc.FillRect(new Rect(x + 6, y + 9, w - 12, 1), c); break;
            case IconKind.Gear:
                dc.Ellipse(new Rect(x + 3, y + 3, w - 6, h - 6), c, false);
                dc.FillRoundRect(new Rect(cx - 2, cy - 2, 4, 4), 2, c);
                dc.FillRect(new Rect(cx - 1, y, 2, 3), c); dc.FillRect(new Rect(cx - 1, y + h - 3, 2, 3), c);
                dc.FillRect(new Rect(x, cy - 1, 3, 2), c); dc.FillRect(new Rect(x + w - 3, cy - 1, 3, 2), c); break;
            case IconKind.Play: for (int i = 0; i < w - 4; i++) dc.FillRect(new Rect(x + 3 + i, y + 2 + i / 2, 1, Math.Max(1, h - 4 - i)), c); break;
            case IconKind.Pause: dc.FillRect(new Rect(x + 3, y + 2, 3, h - 4), c); dc.FillRect(new Rect(x + w - 6, y + 2, 3, h - 4), c); break;
            case IconKind.Refresh:
                dc.Ellipse(new Rect(x + 2, y + 2, w - 4, h - 4), c, false);
                dc.FillRect(new Rect(x + w - 6, y + 1, 5, 2), c); dc.FillRect(new Rect(x + w - 3, y + 1, 2, 5), c); break;
            case IconKind.Sun:
                dc.Ellipse(new Rect(cx - s / 4, cy - s / 4, s / 2, s / 2), c, true);
                dc.FillRect(new Rect(cx - 1, y, 2, 2), c); dc.FillRect(new Rect(cx - 1, y + h - 2, 2, 2), c);
                dc.FillRect(new Rect(x, cy - 1, 2, 2), c); dc.FillRect(new Rect(x + w - 2, cy - 1, 2, 2), c); break;
            case IconKind.Moon:
                dc.Ellipse(new Rect(x + 2, y + 2, w - 4, h - 4), c, true);
                dc.Ellipse(new Rect(x + 6, y, w - 4, h - 5), Theme.Surface, true); break;
            case IconKind.Leaf:
                dc.Ellipse(new Rect(x + 2, y + 1, w - 4, h - 3), c, true);
                L(x + 2, y + h - 1, cx + 2, cy - 2); break;
            case IconKind.Trash:
                dc.FillRect(new Rect(x + 2, y + 3, w - 4, 2), c); dc.FillRect(new Rect(cx - 2, y + 1, 4, 2), c);
                dc.StrokeRoundRect(new Rect(x + 4, y + 5, w - 8, h - 6), 2, c); break;
            case IconKind.Save:
                dc.StrokeRoundRect(new Rect(x + 1, y + 1, w - 2, h - 2), 2, c);
                dc.FillRect(new Rect(x + 4, y + 1, w - 8, 4), c); dc.FillRect(new Rect(x + 4, y + h - 6, w - 8, 4), c); break;
            case IconKind.Clock:
                dc.Ellipse(new Rect(x + 1, y + 1, w - 2, h - 2), c, false);
                dc.FillRect(new Rect(cx - 1, y + 4, 2, cy - y - 3), c); dc.FillRect(new Rect(cx, cy - 1, w / 3, 2), c); break;
            case IconKind.Grid:
                for (int i = 0; i < 2; i++) for (int j = 0; j < 2; j++)
                    dc.FillRoundRect(new Rect(x + 1 + i * (w / 2), y + 1 + j * (h / 2), w / 2 - 2, h / 2 - 2), 2, c);
                break;
            case IconKind.Cpu:
                dc.StrokeRoundRect(new Rect(x + 3, y + 3, w - 6, h - 6), 2, c);
                dc.FillRect(new Rect(cx - 2, cy - 2, 4, 4), c);
                for (int i = 0; i < 3; i++) { dc.FillRect(new Rect(x + 5 + i * (w - 10) / 2, y, 1, 3), c); dc.FillRect(new Rect(x + 5 + i * (w - 10) / 2, y + h - 3, 1, 3), c); }
                break;
            case IconKind.Info:
                dc.Ellipse(new Rect(x + 1, y + 1, w - 2, h - 2), c, false);
                dc.FillRect(new Rect(cx - 1, cy - 1, 2, h / 3), c); dc.FillRect(new Rect(cx - 1, y + 4, 2, 2), c); break;
            case IconKind.Warning:
                for (int i = 0; i < h - 2; i++) dc.FillRect(new Rect(cx - i / 2, y + 1 + i, Math.Max(1, i), 1), c);
                break;
            case IconKind.Home:
                for (int i = 0; i < h / 2; i++) dc.FillRect(new Rect(cx - i, y + 1 + i, 2 * i + 1, 1), c);
                dc.FillRect(new Rect(x + 3, cy, w - 6, h / 2 - 1), c); break;
        }
    }
}
