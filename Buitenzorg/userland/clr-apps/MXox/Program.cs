// XOX -- managed tic-tac-toe against the computer (random or minimax), with a
// custom-drawn board, radio buttons, a toggle switch and a score strip.
// Buitenzorg.UI.

using Buitenzorg.UI;

namespace MXox;

public sealed class XoxGame : ObservableObject
{
    public readonly char[] Cells = new char[9];
    static readonly int[][] Lines =
    {
        new[] { 0, 1, 2 }, new[] { 3, 4, 5 }, new[] { 6, 7, 8 }, new[] { 0, 3, 6 },
        new[] { 1, 4, 7 }, new[] { 2, 5, 8 }, new[] { 0, 4, 8 }, new[] { 2, 4, 6 },
    };
    ulong _seed = 0x2545F4914F6CDD1DUL;
    string _status = "";
    int _you, _draws, _cpu;
    public int[]? WinLine { get; private set; }
    public bool Hard { get; set; } = true;
    public bool ComputerStarts { get; set; }
    public bool Over { get; private set; }

    public string Status { get => _status; private set => SetProperty(ref _status, value); }
    public int You { get => _you; private set => SetProperty(ref _you, value); }
    public int Draws { get => _draws; private set => SetProperty(ref _draws, value); }
    public int Cpu { get => _cpu; private set => SetProperty(ref _cpu, value); }

    public void NewRound()
    {
        for (int i = 0; i < 9; i++) Cells[i] = ' ';
        WinLine = null;
        Over = false;
        if (ComputerStarts) ComputerMove();
        Status = "Giliranmu (X)";
        OnPropertyChanged(nameof(Cells));
    }

    public static char Winner(char[] b, out int[]? line)
    {
        foreach (var l in Lines)
            if (b[l[0]] != ' ' && b[l[0]] == b[l[1]] && b[l[1]] == b[l[2]]) { line = l; return b[l[0]]; }
        line = null;
        foreach (var c in b) if (c == ' ') return ' ';
        return '=';
    }

    /// <summary>Alpha-beta minimax: positive when O wins (sooner is better), negative when X wins.</summary>
    static int Search(char[] b, bool oTurn, int depth, int alpha, int beta)
    {
        char w = Winner(b, out _);
        if (w == 'O') return 10 - depth;
        if (w == 'X') return depth - 10;
        if (w == '=') return 0;
        for (int i = 0; i < 9; i++)
        {
            if (b[i] != ' ') continue;
            b[i] = oTurn ? 'O' : 'X';
            int v = Search(b, !oTurn, depth + 1, alpha, beta);
            b[i] = ' ';
            if (oTurn) { if (v > alpha) alpha = v; }
            else if (v < beta) beta = v;
            if (alpha >= beta) break;
        }
        return oTurn ? alpha : beta;
    }

    public int BestMove()
    {
        int free = 0;
        foreach (var c in Cells) if (c == ' ') free++;
        // the optimal opening replies, without searching the largest trees
        if (free >= 8) return Cells[4] == ' ' ? 4 : 0;
        int move = -1, best = int.MinValue;
        for (int i = 0; i < 9; i++)
        {
            if (Cells[i] != ' ') continue;
            Cells[i] = 'O';
            int v = Search(Cells, false, 0, -100, 100);
            Cells[i] = ' ';
            if (v > best) { best = v; move = i; }
        }
        return move;
    }

    int RandomMove()
    {
        int free = 0;
        foreach (var c in Cells) if (c == ' ') free++;
        _seed ^= _seed << 13; _seed ^= _seed >> 7; _seed ^= _seed << 17;
        int k = (int)(_seed % (ulong)Math.Max(1, free));
        for (int i = 0; i < 9; i++) if (Cells[i] == ' ' && k-- == 0) return i;
        return -1;
    }

    void ComputerMove()
    {
        int m = Hard ? BestMove() : RandomMove();
        if (m >= 0) Cells[m] = 'O';
    }

    public bool Play(int i)
    {
        if (Over || i < 0 || i > 8 || Cells[i] != ' ') return false;
        Cells[i] = 'X';
        if (!Finish()) { ComputerMove(); Finish(); }
        OnPropertyChanged(nameof(Cells));
        return true;
    }

    bool Finish()
    {
        char w = Winner(Cells, out var line);
        if (w == ' ') { Status = "Giliranmu (X)"; return false; }
        Over = true;
        WinLine = line;
        if (w == 'X') { You++; Status = "Kamu menang!"; }
        else if (w == 'O') { Cpu++; Status = "Komputer menang"; }
        else { Draws++; Status = "Seri"; }
        return true;
    }
}

public sealed class XoxBoard : Control
{
    readonly XoxGame _g;
    int _hover = -1;

    public XoxBoard(XoxGame g)
    {
        _g = g;
        Focusable = true;
        g.PropertyChanged += (_, n) => InvalidateVisual();
    }

    protected override Size MeasureOverride(Size a) { int s = Math.Min(Math.Min(a.Width, a.Height), 300); return new Size(s, s); }

    Rect Area()
    {
        var b = Bounds;
        int s = Math.Min(b.Width, b.Height);
        return new Rect(b.X + (b.Width - s) / 2, b.Y + (b.Height - s) / 2, s, s);
    }

    int CellAt(int x, int y)
    {
        var a = Area();
        if (!a.Contains(x, y)) return -1;
        return (y - a.Y) * 3 / a.Height * 3 + (x - a.X) * 3 / a.Width;
    }

    public Point CellCenter(int i)
    {
        var a = Area();
        int c = a.Width / 3;
        return new Point(a.X + (i % 3) * c + c / 2, a.Y + (i / 3) * c + c / 2);
    }

    public override void Render(DrawingContext dc)
    {
        var a = Area();
        dc.FillRoundRect(a, 16, Theme.SurfaceLo);
        int c = a.Width / 3;
        for (int i = 0; i < 9; i++)
        {
            var r = new Rect(a.X + (i % 3) * c + 5, a.Y + (i / 3) * c + 5, c - 10, c - 10);
            bool win = _g.WinLine != null && Array.IndexOf(_g.WinLine, i) >= 0;
            int bg = win ? Color.Mix(Theme.Surface, Theme.Sun, 70) : i == _hover && _g.Cells[i] == ' ' && !_g.Over ? Theme.SurfaceHi : Theme.Surface;
            dc.FillRoundRect(r, 12, bg);
            int m = c / 4;
            var inner = r.Inflate(-m + 4);
            if (_g.Cells[i] == 'X')
            {
                for (int t = -2; t <= 2; t++)
                {
                    dc.Line(inner.X + t, inner.Y, inner.Right + t, inner.Bottom, Theme.Accent);
                    dc.Line(inner.Right + t, inner.Y, inner.X + t, inner.Bottom, Theme.Accent);
                }
            }
            else if (_g.Cells[i] == 'O')
            {
                dc.Ellipse(inner, Theme.Sun, true);
                dc.Ellipse(inner.Inflate(-6), bg, true);
            }
        }
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        int h = CellAt(e.X, e.Y);
        if (h != _hover) { _hover = h; InvalidateVisual(); }
        base.OnPointerMoved(e);
    }

    protected override void OnPointerExited() { _hover = -1; InvalidateVisual(); base.OnPointerExited(); }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        int i = CellAt(e.X, e.Y);
        if (i >= 0) _g.Play(i);
        e.Handled = true;
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        // 1-9 laid out like a phone keypad: 1 2 3 on top
        if (e.Key >= '1' && e.Key <= '9') { _g.Play(e.Key - '1'); e.Handled = true; }
        base.OnKeyDown(e);
    }
}

public static class XoxApp
{
    public static (Window, XoxGame, XoxBoard) Build(bool headless = false)
    {
        var g = new XoxGame();
        var w = new Window("XOX", 360, 540, headless);
        var board = new XoxBoard(g);

        TextBlock Score(Func<int> get, string name) => new TextBlock { HorizontalAlignment = HorizontalAlignment.Center }.Bind(TextBlock.TextProperty, g, () => get().ToString(), name);
        var strip = new UniformGrid { Columns = 3, Spacing = 8, Margin = new Thickness(0, 0, 0, 10) };
        strip.Children.Add(Kit.Stat("Kamu", Score(() => g.You, nameof(XoxGame.You)).WithClass("accent")));
        strip.Children.Add(Kit.Stat("Seri", Score(() => g.Draws, nameof(XoxGame.Draws))));
        strip.Children.Add(Kit.Stat("Komputer", Score(() => g.Cpu, nameof(XoxGame.Cpu)).WithClass("sun")));

        var status = new TextBlock { HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 10, 0, 0), FontWeight = FontWeight.Bold }
            .Bind(TextBlock.TextProperty, g, () => g.Status, nameof(XoxGame.Status));

        var easy = new RadioButton("Mudah", "level");
        var hard = new RadioButton("Sulit", "level") { IsChecked = true };
        easy.IsCheckedChanged += r => { if (r.IsChecked) g.Hard = false; };
        hard.IsCheckedChanged += r => { if (r.IsChecked) g.Hard = true; };
        var starts = new ToggleSwitch("", false) { OnContent = "Komputer mulai", OffContent = "Kamu mulai", Padding = new Thickness(0) };
        starts.IsCheckedChanged += t => { g.ComputerStarts = t.IsChecked; g.NewRound(); };
        var options = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 14, Margin = new Thickness(0, 12, 0, 0) };
        options.Children.Add(easy);
        options.Children.Add(hard);
        options.Children.Add(starts);

        var again = new Button("Ronde baru", _ => { g.NewRound(); board.Focus(); }) { HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 10, 0, 0) }.WithClass("accent");

        var bottom = new StackPanel();
        bottom.Children.Add(status);
        bottom.Children.Add(options);
        bottom.Children.Add(again);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Permainan", "XOX", IconKind.Close, 0x7CC4A4).DockTo(Dock.Top));
        root.Children.Add(strip.DockTo(Dock.Top));
        root.Children.Add(bottom.DockTo(Dock.Bottom));
        root.Children.Add(board);
        w.Content = root;
        g.NewRound();
        Application.Current?.Apply(w);
        w.UpdateLayout();
        board.Focus();
        return (w, g, board);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("XOX");
        var (w, g, board) = Build(headless: true);
        // the computer blocks: X on 0 and 1 threatens 2
        for (int i = 0; i < 9; i++) g.Cells[i] = ' ';
        g.Cells[0] = 'X'; g.Cells[1] = 'X'; g.Cells[4] = 'O';
        c.That(g.BestMove() == 2, "minimax blocks the open row");
        g.Cells[0] = 'O'; g.Cells[1] = 'X'; g.Cells[8] = ' ';
        g.Cells[4] = 'O';
        c.That(g.BestMove() == 8, "minimax takes the winning diagonal");
        g.NewRound();
        var p = board.CellCenter(4);
        w.SimulateClickAt(p.X, p.Y);
        int marks = 0;
        foreach (var ch in g.Cells) if (ch != ' ') marks++;
        c.That(g.Cells[4] == 'X' && marks == 2, "a click plays X and the computer answers");
        // a full game against minimax never ends in a win for X
        int guard = 0;
        while (!g.Over && guard++ < 9)
        {
            int free = Array.IndexOf(g.Cells, ' ');
            w.SimulateKeys(((char)('1' + free)).ToString());
        }
        c.That(g.Over && g.You == 0, "hard computer does not lose (" + g.Status + ")");
        w.RenderFrame();
        c.That(w.LastFrameOps > 20, "frame drew " + w.LastFrameOps + " ops");
        return c;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = XoxApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = XoxApp.Build();
        app.Run(w);
        return 0;
    }
}
