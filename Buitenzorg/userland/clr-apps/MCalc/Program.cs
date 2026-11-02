// Kalkulator -- the managed calculator: a view model with bindings, a keypad
// Grid, a history flyout, keyboard input. Buitenzorg.UI.

using Buitenzorg.UI;

namespace MCalc;

public sealed class HistoryEntry
{
    public string Expression { get; }
    public string Result { get; }
    public HistoryEntry(string e, string r) { Expression = e; Result = r; }
    public override string ToString() => Expression + " = " + Result;
}

/// <summary>The calculator state (no UI): enter digits, apply operators, evaluate left to right
/// with × and ÷ before + and −.</summary>
public sealed class CalcViewModel : ObservableObject
{
    string _display = "0";
    string _expression = "";
    readonly List<double> _values = new List<double>();
    readonly List<char> _ops = new List<char>();
    bool _fresh = true;      // the next digit starts a new number
    bool _justEvaluated;

    public ObservableCollection<HistoryEntry> History { get; } = new ObservableCollection<HistoryEntry>();

    public string Display { get => _display; private set => SetProperty(ref _display, value); }
    public string Expression { get => _expression; private set => SetProperty(ref _expression, value); }

    public static string Format(double v)
    {
        if (double.IsNaN(v) || double.IsInfinity(v)) return "Tidak terdefinisi";
        if (Math.Abs(v) >= 1e12) return v.ToString("E4");
        double r = Math.Round(v, 9);
        if (r == Math.Floor(r) && Math.Abs(r) < 1e15) return ((long)r).ToString();
        string s = r.ToString("F9").TrimEnd('0');
        return s.EndsWith(".") ? s.Substring(0, s.Length - 1) : s;
    }

    static string Sym(char op) => op == '*' ? "×" : op == '/' ? "÷" : op == '-' ? "-" : "+";

    double Current => double.TryParse(_display, out double d) ? d : 0;

    public void Digit(char d)
    {
        if (_justEvaluated) Clear();
        if (_fresh || _display == "0" || _display == "Tidak terdefinisi") { Display = d.ToString(); _fresh = false; }
        else if (_display.Length < 16) Display = _display + d;
    }

    public void Dot()
    {
        if (_justEvaluated) Clear();
        if (_fresh) { Display = "0."; _fresh = false; return; }
        if (!_display.Contains('.')) Display = _display + ".";
    }

    public void Backspace()
    {
        if (_fresh || _justEvaluated) return;
        Display = _display.Length > 1 ? _display.Substring(0, _display.Length - 1) : "0";
        if (_display == "0" || _display == "-") { Display = "0"; }
    }

    public void Negate()
    {
        if (_display == "0") return;
        Display = _display.StartsWith("-") ? _display.Substring(1) : "-" + _display;
    }

    public void Percent() => Display = Format(Current / 100);

    public void Operator(char op)
    {
        if (_justEvaluated) { _justEvaluated = false; _values.Clear(); _ops.Clear(); }
        if (_fresh && _ops.Count > 0 && _values.Count == _ops.Count)
        {
            _ops[_ops.Count - 1] = op;             // replace the last operator
        }
        else
        {
            _values.Add(Current);
            _ops.Add(op);
        }
        _fresh = true;
        Expression = Describe();
    }

    string Describe()
    {
        var parts = new List<string>();
        for (int i = 0; i < _values.Count; i++)
        {
            parts.Add(Format(_values[i]));
            if (i < _ops.Count) parts.Add(Sym(_ops[i]));
        }
        return string.Join(" ", parts);
    }

    public void Evaluate()
    {
        if (_ops.Count == 0) return;
        if (!_fresh || _values.Count == _ops.Count) _values.Add(Current);
        string expr = Describe();
        // × and ÷ first
        var vals = new List<double>(_values);
        var ops = new List<char>(_ops);
        for (int i = 0; i < ops.Count;)
        {
            if (ops[i] == '*' || ops[i] == '/')
            {
                vals[i] = ops[i] == '*' ? vals[i] * vals[i + 1] : vals[i] / vals[i + 1];
                vals.RemoveAt(i + 1);
                ops.RemoveAt(i);
            }
            else i++;
        }
        double result = vals[0];
        for (int i = 0; i < ops.Count; i++) result = ops[i] == '+' ? result + vals[i + 1] : result - vals[i + 1];
        string text = Format(result);
        Expression = expr + " =";
        Display = text;
        History.Insert(0, new HistoryEntry(expr, text));
        _values.Clear();
        _ops.Clear();
        _fresh = true;
        _justEvaluated = true;
    }

    public void Clear()
    {
        _values.Clear();
        _ops.Clear();
        Display = "0";
        Expression = "";
        _fresh = true;
        _justEvaluated = false;
    }

    public void Recall(HistoryEntry e)
    {
        Clear();
        Display = e.Result;
        _fresh = false;
    }

    /// <summary>Keyboard: digits, + - * / x, . , Enter =, Backspace, Escape/C, %.</summary>
    public bool Key(char k)
    {
        if (k >= '0' && k <= '9') Digit(k);
        else if (k == '.' || k == ',') Dot();
        else if (k == '+' || k == '-' || k == '*' || k == '/') Operator(k);
        else if (k == 'x' || k == 'X') Operator('*');
        else if (k == '=' || k == Keys.Enter) Evaluate();
        else if (k == Keys.Backspace) Backspace();
        else if (k == Keys.Escape || k == 'c' || k == 'C' || k == Keys.Delete) Clear();
        else if (k == '%') Percent();
        else return false;
        return true;
    }
}

public static class CalcApp
{
    public static (Window, CalcViewModel) Build(bool headless = false)
    {
        var vm = new CalcViewModel();
        // app-level styles first: operators carry the leaf accent, the function row is quieter
        var app = Application.Current;
        if (app != null && !HasStyle(app, "Button.op"))
        {
            app.Styles.Add(new Style("Button.op") { { Control.ForegroundProperty, Theme.Accent }, { Control.FontWeightProperty, FontWeight.Bold } });
            app.Styles.Add(new Style("Button.fn") { { Control.ForegroundProperty, Theme.TextMuted } });
        }
        var w = new Window("Kalkulator", 340, 470, headless);

        var history = new ListBox { ItemsSource = vm.History, MaxHeight = 240, Width = 260 };
        history.ItemTemplate = o =>
        {
            var e = (HistoryEntry)o;
            var s = new StackPanel { Spacing = 2 };
            s.Children.Add(new TextBlock(e.Expression + " =").WithClass("muted"));
            s.Children.Add(new TextBlock(e.Result) { FontWeight = FontWeight.Bold });
            return s;
        };
        var historyButton = new DropDownButton(new Icon(IconKind.Clock, 14)).WithTip("Riwayat perhitungan");
        var empty = new TextBlock("Belum ada perhitungan.") { Margin = new Thickness(4) }.WithClass("muted");
        var historyPanel = new StackPanel { Spacing = 6 };
        historyPanel.Children.Add(new TextBlock("Riwayat").WithClass("strong"));
        historyPanel.Children.Add(empty);
        historyPanel.Children.Add(history);
        var flyout = new Flyout(historyPanel);
        historyButton.Flyout = flyout;
        flyout.Opened += () => { empty.IsVisible = vm.History.Count == 0; history.IsVisible = vm.History.Count > 0; };
        history.ItemActivated += l => { if (l.SelectedItem is HistoryEntry h) { vm.Recall(h); flyout.Hide(); } };
        history.SelectionChanged += l => { if (l.SelectedItem is HistoryEntry h) vm.Recall(h); };

        // the display: a specimen label -- expression above, the value large below
        var expr = new TextBlock { TextAlignment = TextAlignment.Right, TextTrimming = TextTrimming.CharacterEllipsis }.WithClass("muted")
            .Bind(TextBlock.TextProperty, vm, () => vm.Expression, nameof(vm.Expression));
        var value = new TextBlock { TextAlignment = TextAlignment.Right, FontSize = 24, TextTrimming = TextTrimming.CharacterEllipsis }
            .Bind(TextBlock.TextProperty, vm, () => vm.Display, nameof(vm.Display));
        var displayStack = new StackPanel { Spacing = 8 };
        displayStack.Children.Add(expr);
        displayStack.Children.Add(value);
        var display = new Border { Child = displayStack, Padding = new Thickness(14, 12), Margin = new Thickness(0, 0, 0, 12) }.WithClass("inset");
        expr.MinHeight = 18;

        var keys = new Grid { ColumnDefinitions = "*,*,*,*", RowDefinitions = "*,*,*,*,*", ColumnSpacing = 8, RowSpacing = 8 };
        string[] layout = { "C", "±", "%", "÷", "7", "8", "9", "×", "4", "5", "6", "-", "1", "2", "3", "+", "0", ".", "<", "=" };
        for (int i = 0; i < layout.Length; i++)
        {
            string k = layout[i];
            object face = k == "<" ? new Icon(IconKind.ChevronLeft, 14) : (object)k;
            var b = new Button(face) { HorizontalAlignment = HorizontalAlignment.Stretch, VerticalAlignment = VerticalAlignment.Stretch, Focusable = false, Tag = k };
            if (k == "=") b.Classes.Add("sun");
            else if (k == "÷" || k == "×" || k == "-" || k == "+") b.Classes.Add("op");
            else if (k == "C") b.Classes.Add("danger");
            else if (k == "±" || k == "%" || k == "<") b.Classes.Add("fn");
            if (k == "<") ToolTip.SetTip(b, "Hapus satu angka");
            b.Click += x => Press(vm, (string)x.Tag!);
            keys.Children.Add(b.Cell(i / 4, i % 4));
        }

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Alat", "Kalkulator", IconKind.Grid, 0xE9B44C, historyButton).DockTo(Dock.Top));
        root.Children.Add(display.DockTo(Dock.Top));
        root.Children.Add(keys);
        w.Content = root;
        w.UnhandledKey += e => { if (vm.Key(e.Key)) e.Handled = true; };

        app?.Apply(w);
        return (w, vm);
    }

    static bool HasStyle(Application a, string sel)
    {
        foreach (var s in a.Styles) if (s.Text == sel) return true;
        return false;
    }

    static void Press(CalcViewModel vm, string k)
    {
        switch (k)
        {
            case "C": vm.Clear(); break;
            case "±": vm.Negate(); break;
            case "%": vm.Percent(); break;
            case "÷": vm.Operator('/'); break;
            case "×": vm.Operator('*'); break;
            case "-": vm.Operator('-'); break;
            case "+": vm.Operator('+'); break;
            case ".": vm.Dot(); break;
            case "<": vm.Backspace(); break;
            case "=": vm.Evaluate(); break;
            default: vm.Digit(k[0]); break;
        }
    }

    static Button Key(Window w, string k)
    {
        foreach (var d in w.Descendants()) if (d is Button b && (b.Tag as string) == k) return b;
        throw new InvalidOperationException("no key " + k);
    }

    /// <summary>Drive the real UI headlessly: clicks through the window's routing, keys, bindings.</summary>
    public static Checks SelfTest()
    {
        var c = new Checks("Kalkulator");
        var (w, vm) = Build(headless: true);
        w.UpdateLayout();
        foreach (var k in new[] { "1", "2", "+", "3", "=" }) w.SimulateClick(Key(w, k));
        c.Equal("15", vm.Display, "12 + 3 by clicks");
        var value = (TextBlock)FindDisplay(w);
        c.Equal("15", value.Text, "display bound to the view model");
        w.SimulateKeys("7*6\n");
        c.Equal("42", vm.Display, "7*6 by keyboard");
        w.SimulateKeys("2+3*4=");
        c.Equal("14", vm.Display, "precedence 2+3*4");
        w.SimulateKeys("1/0\n");
        c.Equal("Tidak terdefinisi", vm.Display, "divide by zero");
        w.SimulateKeys("9.5-0.25=");
        c.Equal("9.25", vm.Display, "decimals");
        c.That(vm.History.Count == 5, "history has 5 entries");
        w.RenderFrame();
        c.That(w.LastFrameOps > 40, "frame drew " + w.LastFrameOps + " ops");
        return c;
    }

    static Control FindDisplay(Window w)
    {
        foreach (var d in w.Descendants()) if (d is TextBlock t && t.FontSize == 24 && t.TextAlignment == TextAlignment.Right) return t;
        throw new InvalidOperationException("no display");
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = CalcApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = CalcApp.Build();
        app.Run(w);
        return 0;
    }
}
