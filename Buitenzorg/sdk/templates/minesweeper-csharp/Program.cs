// __APPNAME__ -- Minesweeper made with bz new minesweeper-csharp.
//
// Open every square that hides no mine. A number tells how many of the eight
// neighbours are mines; an empty square opens its neighbours by itself.
// Mark a suspected mine with a flag: switch "Mode bendera" on (or press F)
// and click. The first click is always safe.
//
// The rules live in Field (no UI), the window is a Buitenzorg.UI Grid of
// buttons. `dotnet bin/.../__NAME83__.dll --selftest` drives both without a
// screen; on the device `run __NAME83__`.

using Buitenzorg.UI;

namespace __NAME83__;

/// <summary>The rules: a W x H field with N mines, placed on the first click.</summary>
public sealed class Field
{
    public readonly int W, H, Mines;
    readonly bool[] _mine, _open, _flag;
    readonly Random _rng;
    public bool Placed { get; private set; }
    public bool Lost { get; private set; }
    public int Opened { get; private set; }
    public bool Won => !Lost && Opened == W * H - MineTotal;
    int MineTotal { get { int n = 0; foreach (var m in _mine) if (m) n++; return n; } }

    public Field(int w, int h, int mines, int seed)
    {
        W = w; H = h; Mines = Math.Min(mines, w * h - 9);
        _mine = new bool[w * h]; _open = new bool[w * h]; _flag = new bool[w * h];
        _rng = new Random(seed);
    }

    public bool IsMine(int x, int y) => _mine[y * W + x];
    public bool IsOpen(int x, int y) => _open[y * W + x];
    public bool IsFlag(int x, int y) => _flag[y * W + x];

    /// <summary>Mines around (x, y).</summary>
    public int Count(int x, int y)
    {
        int n = 0;
        for (int dy = -1; dy <= 1; dy++)
            for (int dx = -1; dx <= 1; dx++)
            {
                int nx = x + dx, ny = y + dy;
                if ((dx != 0 || dy != 0) && nx >= 0 && ny >= 0 && nx < W && ny < H && _mine[ny * W + nx]) n++;
            }
        return n;
    }

    /// <summary>Mines anywhere but around the first click.</summary>
    void Place(int fx, int fy)
    {
        int left = Mines;
        while (left > 0)
        {
            int x = _rng.Next(W), y = _rng.Next(H);
            if (_mine[y * W + x] || (Math.Abs(x - fx) <= 1 && Math.Abs(y - fy) <= 1)) continue;
            _mine[y * W + x] = true;
            left--;
        }
        Placed = true;
    }

    /// <summary>Lay mines by hand (tests).</summary>
    public void PlaceAt(params (int X, int Y)[] mines)
    {
        foreach (var (x, y) in mines) _mine[y * W + x] = true;
        Placed = true;
    }

    /// <summary>Open a square; false when it was a mine.</summary>
    public bool Open(int x, int y)
    {
        if (Lost || Won || _flag[y * W + x] || _open[y * W + x]) return true;
        if (!Placed) Place(x, y);
        if (_mine[y * W + x]) { Lost = true; return false; }
        // flood fill without recursion: a stack of squares still to open
        var todo = new Stack<int>();
        todo.Push(y * W + x);
        while (todo.Count > 0)
        {
            int i = todo.Pop();
            if (_open[i] || _flag[i]) continue;
            _open[i] = true;
            Opened++;
            int cx = i % W, cy = i / W;
            if (Count(cx, cy) != 0) continue;
            for (int dy = -1; dy <= 1; dy++)
                for (int dx = -1; dx <= 1; dx++)
                {
                    int nx = cx + dx, ny = cy + dy;
                    if (nx >= 0 && ny >= 0 && nx < W && ny < H && !_open[ny * W + nx]) todo.Push(ny * W + nx);
                }
        }
        return true;
    }

    public void ToggleFlag(int x, int y)
    {
        if (!_open[y * W + x] && !Lost && !Won) _flag[y * W + x] = !_flag[y * W + x];
    }

    public int Flags { get { int n = 0; foreach (var f in _flag) if (f) n++; return n; } }
}

public sealed class MinesWindow
{
    public const int Cols = 9, Rows = 9, MineCount = 10;
    public readonly Window W;
    public Field Game;
    public readonly CheckBox FlagMode = new CheckBox("Mode bendera (F)");
    readonly Button[] _cells = new Button[Cols * Rows];
    readonly TextBlock _status = new TextBlock("");
    int _seed;

    public MinesWindow(bool headless = false, int seed = 0)
    {
        _seed = seed == 0 ? Environment.TickCount : seed;
        Game = new Field(Cols, Rows, MineCount, _seed);
        W = new Window("__APPNAME__", 420, 500, headless);

        var grid = new Grid { ColumnSpacing = 3, RowSpacing = 3 };
        string defs = "*";
        for (int i = 1; i < Cols; i++) defs += ",*";
        grid.ColumnDefinitions = defs;
        defs = "*";
        for (int i = 1; i < Rows; i++) defs += ",*";
        grid.RowDefinitions = defs;
        for (int i = 0; i < _cells.Length; i++)
        {
            var b = new Button("") { HorizontalAlignment = HorizontalAlignment.Stretch, VerticalAlignment = VerticalAlignment.Stretch, Focusable = false, Tag = i };
            b.Click += x => Click((int)x.Tag!);
            _cells[i] = b;
            grid.Children.Add(b.Cell(i / Cols, i % Cols));
        }

        var again = new Button("Main lagi", _ => NewGame()).WithClass("accent");
        var bar = new DockPanel { Margin = new Thickness(0, 10, 0, 0) };
        bar.Children.Add(again.DockTo(Dock.Right));
        bar.Children.Add(FlagMode);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(new TextBlock("__APPNAME__").WithClass("eyebrow").DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Top));
        root.Children.Add(bar.DockTo(Dock.Bottom));
        root.Children.Add(grid);
        W.Content = root;
        // keys nobody else took (the cells are not focusable)
        W.UnhandledKey += e =>
        {
            if (e.Key == 'f' || e.Key == 'F') { FlagMode.IsChecked = !FlagMode.IsChecked; e.Handled = true; }
        };
        Application.Current?.Apply(W);
        Refresh();
        W.UpdateLayout();
    }

    public void NewGame()
    {
        Game = new Field(Cols, Rows, MineCount, ++_seed);
        Refresh();
    }

    public void Click(int i)
    {
        int x = i % Cols, y = i / Cols;
        if (FlagMode.IsChecked) Game.ToggleFlag(x, y);
        else Game.Open(x, y);
        Refresh();
    }

    /// <summary>Show the field: numbers, flags, and every mine once the game is over.</summary>
    public void Refresh()
    {
        bool over = Game.Lost || Game.Won;
        for (int i = 0; i < _cells.Length; i++)
        {
            int x = i % Cols, y = i / Cols;
            var b = _cells[i];
            b.Classes.Remove("accent");
            b.Classes.Remove("danger");
            string face = "";
            if (Game.IsOpen(x, y))
            {
                int n = Game.Count(x, y);
                face = n == 0 ? "" : n.ToString();
                b.Classes.Add("accent");
            }
            else if (over && Game.IsMine(x, y)) { face = "*"; b.Classes.Add("danger"); }
            else if (Game.IsFlag(x, y)) face = "F";
            b.Content = face;
        }
        _status.Text = Game.Lost ? "Kena ranjau! Tekan Main lagi."
            : Game.Won ? "Menang: semua kotak aman terbuka."
            : (MineCount - Game.Flags) + " ranjau belum ditandai";
    }

    public string Face(int x, int y) => _cells[y * Cols + x].Content as string ?? "";

    public static Checks SelfTest()
    {
        var c = new Checks("__APPNAME__");
        // the rules, with mines laid by hand
        var f = new Field(4, 4, 2, 1);
        f.PlaceAt((0, 0), (3, 3));
        c.That(f.Count(1, 1) == 1 && f.Count(2, 2) == 1 && f.Count(3, 0) == 0, "neighbour counts");
        f.Open(3, 0);
        c.That(f.IsOpen(0, 3) && !f.IsOpen(0, 0) && f.Opened == 14, "an empty square opens its whole area (" + f.Opened + ")");
        c.That(f.Won, "every safe square open = won");
        var n = new Field(4, 1, 1, 1);
        n.PlaceAt((0, 0));
        n.Open(1, 0);
        c.That(n.Opened == 1 && !n.Won, "a numbered square opens only itself");
        var g = new Field(4, 4, 2, 1);
        g.PlaceAt((0, 0));
        g.ToggleFlag(0, 0);
        c.That(g.Open(0, 0) && !g.Lost, "a flagged mine does not explode");
        g.ToggleFlag(0, 0);
        c.That(!g.Open(0, 0) && g.Lost, "opening a mine loses");
        var h = new Field(9, 9, 10, 7);
        h.Open(4, 4);
        c.That(!h.Lost && h.Opened > 0, "the first click is always safe");

        // the window, clicked like a user would
        var w = new MinesWindow(headless: true, seed: 42);
        w.Game.PlaceAt((0, 0));
        w.Click(Cols * Rows - 1);
        c.That(w.Game.Won && w.Face(1, 0) == "1" && w.Face(8, 8) == "", "one click on a field with one mine opens it all");
        w.NewGame();
        w.FlagMode.IsChecked = true;
        w.Click(0);
        c.That(w.Face(0, 0) == "F" && w.Game.Flags == 1, "flag mode puts a flag");
        w.W.RenderFrame();
        c.That(w.W.LastFrameOps > 81, "the window drew the field (" + w.W.LastFrameOps + " ops)");
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
            var c = MinesWindow.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        app.Run(new MinesWindow().W);
        return 0;
    }
}
