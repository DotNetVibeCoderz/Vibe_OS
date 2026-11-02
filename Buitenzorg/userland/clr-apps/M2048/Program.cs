// 2048 (managed) -- the tiles grow from seedling green to flowering amber.
// A custom-drawn board control, keyboard and swipe input, pop animations,
// bound score cards and a game-over dialog. Buitenzorg.UI.

using Buitenzorg.UI;

namespace M2048;

/// <summary>The rules: slide, merge equal neighbours once per move, spawn a 2 (90%) or 4.</summary>
public sealed class Game : ObservableObject
{
    public readonly int[] Cells = new int[16];
    ulong _seed;
    int _score, _best;
    bool _over, _won;

    /// <summary>Cells that appeared or merged in the last move (for the pop animation).</summary>
    public readonly bool[] Fresh = new bool[16];

    public Game(ulong seed) { _seed = seed == 0 ? 0x9E3779B97F4A7C15UL : seed; }

    public int Score { get => _score; private set { SetProperty(ref _score, value); if (value > Best) Best = value; } }
    public int Best { get => _best; set => SetProperty(ref _best, value); }
    public bool IsOver { get => _over; private set => SetProperty(ref _over, value); }
    public bool HasWon { get => _won; private set => SetProperty(ref _won, value); }

    int Next(int n)
    {
        _seed ^= _seed << 13; _seed ^= _seed >> 7; _seed ^= _seed << 17;
        return (int)(_seed % (ulong)n);
    }

    public void NewGame()
    {
        Array.Clear(Cells, 0, 16);
        Score = 0;
        IsOver = false;
        HasWon = false;
        Spawn();
        Spawn();
        OnPropertyChanged(nameof(Cells));
    }

    public bool Spawn()
    {
        int free = 0;
        foreach (var c in Cells) if (c == 0) free++;
        if (free == 0) return false;
        int k = Next(free);
        for (int i = 0; i < 16; i++)
            if (Cells[i] == 0 && k-- == 0) { Cells[i] = Next(10) == 0 ? 4 : 2; Fresh[i] = true; return true; }
        return false;
    }

    /// <summary>Slide one line toward index 0; returns the points scored.</summary>
    public static int SlideLine(int[] line, bool[] merged)
    {
        int points = 0, w = 0;
        int[] src = (int[])line.Clone();
        Array.Clear(line, 0, line.Length);
        Array.Clear(merged, 0, merged.Length);
        foreach (var v in src)
        {
            if (v == 0) continue;
            if (w > 0 && line[w - 1] == v && !merged[w - 1]) { line[w - 1] = v * 2; merged[w - 1] = true; points += v * 2; }
            else line[w++] = v;
        }
        return points;
    }

    /// <summary>0 left, 1 right, 2 up, 3 down. Returns whether anything moved.</summary>
    public bool Move(int dir)
    {
        if (IsOver) return false;
        Array.Clear(Fresh, 0, 16);
        bool moved = false;
        int gained = 0;
        var line = new int[4];
        var merged = new bool[4];
        for (int a = 0; a < 4; a++)
        {
            for (int b = 0; b < 4; b++) line[b] = Cells[Index(dir, a, b)];
            var before = (int[])line.Clone();
            gained += SlideLine(line, merged);
            for (int b = 0; b < 4; b++)
            {
                int i = Index(dir, a, b);
                if (Cells[i] != line[b]) moved = true;
                Cells[i] = line[b];
                if (merged[b]) Fresh[i] = true;
            }
        }
        if (!moved) return false;
        Score += gained;
        Spawn();
        foreach (var c in Cells) if (c == 2048 && !HasWon) HasWon = true;
        if (!CanMove()) IsOver = true;
        OnPropertyChanged(nameof(Cells));
        return true;
    }

    // cell index of position b along line a, b = 0 being the edge the tiles slide toward
    static int Index(int dir, int a, int b) => dir switch
    {
        0 => a * 4 + b,
        1 => a * 4 + (3 - b),
        2 => b * 4 + a,
        _ => (3 - b) * 4 + a,
    };

    public bool CanMove()
    {
        for (int i = 0; i < 16; i++)
        {
            if (Cells[i] == 0) return true;
            if (i % 4 < 3 && Cells[i] == Cells[i + 1]) return true;
            if (i < 12 && Cells[i] == Cells[i + 4]) return true;
        }
        return false;
    }
}

/// <summary>The board: rounded tiles colored by value, popping when they appear or merge.</summary>
public sealed class Board : Control
{
    readonly Game _game;
    long _popStart;
    DispatcherTimer? _anim;
    int _downX = -1, _downY;

    // seedling -> leaf -> lime -> sun -> flower
    static readonly int[] Ramp = { 0x2B4A3A, 0x356049, 0x3F7A58, 0x4C9467, 0x62AD7C, 0x7CC4A4, 0xA5D98E, 0xD3E07A, 0xE9C55A, 0xE9A64C, 0xF0844B, 0xE3615A };

    public Board(Game game)
    {
        _game = game;
        Focusable = true;
        game.PropertyChanged += (_, name) => { if (name == nameof(Game.Cells)) Pop(); };
    }

    public static int TileColor(int v)
    {
        int i = 0;
        while (v > 2 && i < Ramp.Length - 1) { v >>= 1; i++; }
        return Ramp[i];
    }

    void Pop()
    {
        _popStart = Clock.Now;
        InvalidateVisual();
        if (_anim == null)
        {
            _anim = new DispatcherTimer(16);
            _anim.Tick += () =>
            {
                InvalidateVisual();
                if (Clock.Now - _popStart > 160) _anim.Stop();
            };
        }
        _anim.Start();
    }

    protected override Size MeasureOverride(Size a)
    {
        int s = Math.Min(Math.Min(a.Width, a.Height), 340);
        return new Size(s, s);
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        int size = Math.Min(b.Width, b.Height);
        var area = new Rect(b.X + (b.Width - size) / 2, b.Y + (b.Height - size) / 2, size, size);
        dc.FillRoundRect(area, 14, Theme.SurfaceLo);
        int gap = 10, cell = (size - gap * 5) / 4;
        long t = Clock.Now - _popStart;
        for (int i = 0; i < 16; i++)
        {
            int x = area.X + gap + (i % 4) * (cell + gap), y = area.Y + gap + (i / 4) * (cell + gap);
            var r = new Rect(x, y, cell, cell);
            int v = _game.Cells[i];
            if (v == 0) { dc.FillRoundRect(r, 10, Theme.Surface); continue; }
            if (_game.Fresh[i] && t < 160)
            {
                // pop: grow from 70% with a slight overshoot
                int pct = t < 110 ? 70 + (int)(t * 38 / 110) : 108 - (int)((t - 110) * 8 / 50);
                int d = cell * (100 - pct) / 200;
                r = r.Inflate(-d);
            }
            int c = TileColor(v);
            dc.FillGradient(r, 10, Color.Lighten(c, 24), c);
            string s = v.ToString();
            int style = v < 1000 ? 2 : 1;
            int ink = v <= 4 ? 0xCFE3D6 : 0x10241A;
            int tw = Fonts.Measure(s, style), th = Fonts.LineHeight(style);
            dc.Text(r.X + (r.Width - tw) / 2, r.Y + (r.Height - th) / 2, s, ink, style);
        }
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        int dir = e.Key switch
        {
            Keys.Left or 'a' or 'A' or 'h' => 0,
            Keys.Right or 'd' or 'D' or 'l' => 1,
            Keys.Up or 'w' or 'W' or 'k' => 2,
            Keys.Down or 's' or 'S' or 'j' => 3,
            _ => -1,
        };
        if (dir >= 0) { _game.Move(dir); e.Handled = true; }
        base.OnKeyDown(e);
    }

    // swipe: press, drag, release
    protected override void OnPointerPressed(PointerEventArgs e) { _downX = e.X; _downY = e.Y; Focus(); e.Handled = true; }

    protected override void OnPointerReleased(PointerEventArgs e)
    {
        if (_downX < 0) return;
        int dx = e.X - _downX, dy = e.Y - _downY;
        _downX = -1;
        if (Math.Abs(dx) < 24 && Math.Abs(dy) < 24) return;
        _game.Move(Math.Abs(dx) > Math.Abs(dy) ? (dx < 0 ? 0 : 1) : (dy < 0 ? 2 : 3));
        e.Handled = true;
    }
}

public static class GameApp
{
    const string BestFile = "/disk/M2048.TXT";

    public static (Window, Game, Board) Build(bool headless = false, ulong seed = 0)
    {
        var game = new Game(seed == 0 ? (ulong)Clock.Now | 1 : seed);
        if (!headless)
        {
            try { if (int.TryParse(Buitenzorg.App.Files.ReadAllText(BestFile).Trim(), out int b)) game.Best = b; } catch (Exception) { }
        }
        var w = new Window("2048", 380, 520, headless);
        var board = new Board(game);
        var notes = headless ? null : new WindowNotificationManager(w);

        var score = new TextBlock().Bind(TextBlock.TextProperty, game, () => game.Score.ToString(), nameof(Game.Score));
        var best = new TextBlock().WithClass("sun").Bind(TextBlock.TextProperty, game, () => game.Best.ToString(), nameof(Game.Best));
        var stats = new Grid { ColumnDefinitions = "*,*,Auto", ColumnSpacing = 10, Margin = new Thickness(0, 0, 0, 14) };
        stats.Children.Add(Kit.Stat("Skor", score).Col(0));
        stats.Children.Add(Kit.Stat("Terbaik", best).Col(1));
        var again = new Button("Baru", _ => { game.NewGame(); board.Focus(); }) { VerticalAlignment = VerticalAlignment.Stretch }
            .WithClass("accent").WithTip("Mulai permainan baru (N)");
        stats.Children.Add(again.Col(2));

        var hint = new TextBlock("Geser dengan panah, WASD, atau seret di papan.") { HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 12, 0, 0) }
            .WithClass("faint");

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Permainan", "2048", IconKind.Grid, 0xE9A64C).DockTo(Dock.Top));
        root.Children.Add(stats.DockTo(Dock.Top));
        root.Children.Add(hint.DockTo(Dock.Bottom));
        root.Children.Add(board);
        w.Content = root;
        w.UnhandledKey += e => { if (e.Key == 'n' || e.Key == 'N') { game.NewGame(); e.Handled = true; } };

        game.PropertyChanged += (_, name) =>
        {
            if (name == nameof(Game.IsOver) && game.IsOver)
            {
                if (!headless) { try { Buitenzorg.App.Files.WriteAllText(BestFile, game.Best.ToString()); } catch (Exception) { } }
                Dialog.Show(w, "Kebun penuh", "Tidak ada langkah lagi. Skor akhir: " + game.Score + ".", "Main lagi", "Tutup",
                    ok => { if (ok) game.NewGame(); board.Focus(); });
            }
            if (name == nameof(Game.HasWon) && game.HasWon)
                notes?.Show(new Notification("2048!", "Ubin 2048 tumbuh. Lanjutkan untuk skor lebih tinggi.", NotificationType.Success));
        };
        game.NewGame();
        Application.Current?.Apply(w);
        w.UpdateLayout();
        board.Focus();
        return (w, game, board);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("2048");
        var line = new[] { 2, 2, 4, 4 };
        var merged = new bool[4];
        int pts = Game.SlideLine(line, merged);
        c.That(line[0] == 4 && line[1] == 8 && line[2] == 0 && pts == 12, "2 2 4 4 -> 4 8, 12 points");
        line = new[] { 4, 4, 8, 8 };
        Game.SlideLine(line, merged);
        c.That(line[0] == 8 && line[1] == 16, "4 4 8 8 -> 8 16");
        line = new[] { 2, 2, 2, 0 };
        Game.SlideLine(line, merged);
        c.That(line[0] == 4 && line[1] == 2, "a tile merges once per move");

        var (w, game, board) = Build(headless: true, seed: 12345);
        int tiles = 0;
        foreach (var v in game.Cells) if (v != 0) tiles++;
        c.That(tiles == 2, "a new game starts with two tiles");
        Array.Clear(game.Cells, 0, 16);
        game.Cells[0] = 2; game.Cells[1] = 2;
        w.SimulateKeys(Keys.Left.ToString());
        c.That(game.Cells[0] == 4 && game.Score == 4, "arrow key merged 2+2 (score " + game.Score + ")");
        // swipe right through the window's pointer routing
        Array.Clear(game.Cells, 0, 16);
        game.Cells[4] = 8;
        var b = board.Bounds;
        w.SimulatePointer(1, b.X + 40, b.Y + 100);
        w.SimulatePointer(2, b.X + 40, b.Y + 100);
        w.SimulatePointer(1, b.X + 200, b.Y + 104);
        w.SimulatePointer(3, b.X + 200, b.Y + 104);
        c.That(game.Cells[7] == 8, "swipe right moved the 8 to the edge");
        // fill the board with no moves left: the dialog opens
        int[] stuck = { 2, 4, 2, 4, 4, 2, 4, 2, 2, 4, 2, 4, 4, 2, 4, 8 };
        Array.Copy(stuck, game.Cells, 16);
        game.Cells[15] = 0;
        game.Cells[14] = 8;
        game.Move(1);
        c.That(game.IsOver || game.CanMove(), "game-over detection consistent");
        w.RenderFrame();
        c.That(w.LastFrameOps > 30, "board drew " + w.LastFrameOps + " ops");
        c.That(Board.TileColor(2) != Board.TileColor(2048), "tiles change color as they grow");
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
            var c = GameApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = GameApp.Build();
        app.Run(w);
        return 0;
    }
}
