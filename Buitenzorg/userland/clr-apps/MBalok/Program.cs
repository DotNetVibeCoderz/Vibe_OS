// Balok -- stack the falling seed-crates: a full row is harvested. Left /
// Right move, Up / X rotate, Down drops faster, Space drops at once, P pause.
// The pace rises every 10 rows. Buitenzorg.Game.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace MBalok;

public sealed class Pit
{
    public const int Cols = 10, Rows = 20;
    public readonly int[] Cells = new int[Cols * Rows]; // 0 empty, else color index 1..7
    // seven shapes on a 4x4 grid, each as four (x, y) cells
    static readonly int[][] Shapes =
    {
        new[] { 0,1, 1,1, 2,1, 3,1 }, // I
        new[] { 1,0, 2,0, 1,1, 2,1 }, // O
        new[] { 1,0, 0,1, 1,1, 2,1 }, // T
        new[] { 1,0, 2,0, 0,1, 1,1 }, // S
        new[] { 0,0, 1,0, 1,1, 2,1 }, // Z
        new[] { 0,0, 0,1, 1,1, 2,1 }, // J
        new[] { 2,0, 0,1, 1,1, 2,1 }, // L
    };
    public int Kind { get; private set; }
    public int NextKind { get; private set; }
    public int Rot { get; private set; }
    public int PX { get; private set; }
    public int PY { get; private set; }
    public int Lines { get; private set; }
    public int Score { get; private set; }
    public bool Over { get; private set; }
    public int Level => 1 + Lines / 10;
    /// <summary>Seconds per row of fall.</summary>
    public double Gravity => Math.Max(0.08, 0.8 - (Level - 1) * 0.07);
    readonly Rng _rng;

    public Pit(ulong seed) { _rng = new Rng(seed); NextKind = _rng.Next(7); Spawn(); }

    /// <summary>The four cells of a piece (rotation r: 0..3, clockwise).</summary>
    public static IEnumerable<(int X, int Y)> CellsOf(int kind, int r)
    {
        var s = Shapes[kind];
        for (int i = 0; i < 4; i++)
        {
            int x = s[i * 2], y = s[i * 2 + 1];
            for (int k = 0; k < r; k++) (x, y) = (kind == 1 ? x : 3 - y, kind == 1 ? y : x); // O does not turn
            if (kind != 0 && kind != 1 && r % 4 != 0)
            {
                // 3x3 pieces turn inside their 3x3 box
                x = s[i * 2]; y = s[i * 2 + 1];
                for (int k = 0; k < r; k++) (x, y) = (2 - y, x);
            }
            yield return (x, y);
        }
    }

    public IEnumerable<(int X, int Y)> Current() { foreach (var (x, y) in CellsOf(Kind, Rot)) yield return (PX + x, PY + y); }

    bool Fits(int kind, int rot, int px, int py)
    {
        foreach (var (x, y) in CellsOf(kind, rot))
        {
            int cx = px + x, cy = py + y;
            if (cx < 0 || cx >= Cols || cy >= Rows) return false;
            if (cy >= 0 && Cells[cy * Cols + cx] != 0) return false;
        }
        return true;
    }

    void Spawn()
    {
        Kind = NextKind;
        NextKind = _rng.Next(7);
        Rot = 0;
        PX = 3;
        PY = -1;
        if (!Fits(Kind, Rot, PX, PY + 1)) Over = true; else PY = 0;
    }

    public bool Move(int dx)
    {
        if (Over || !Fits(Kind, Rot, PX + dx, PY)) return false;
        PX += dx;
        return true;
    }

    /// <summary>Rotate clockwise, nudging off a wall if needed.</summary>
    public bool Rotate()
    {
        if (Over) return false;
        int r = (Rot + 1) % 4;
        foreach (int kick in new[] { 0, -1, 1, -2, 2 })
            if (Fits(Kind, r, PX + kick, PY)) { Rot = r; PX += kick; return true; }
        return false;
    }

    /// <summary>One row down; when it lands: lock, clear rows, next piece. Returns the rows cleared (-1 = only moved).</summary>
    public int Fall()
    {
        if (Over) return 0;
        if (Fits(Kind, Rot, PX, PY + 1)) { PY++; return -1; }
        foreach (var (x, y) in Current())
        {
            if (y < 0) { Over = true; return 0; }
            Cells[y * Cols + x] = Kind + 1;
        }
        int cleared = 0;
        for (int y = Rows - 1; y >= 0; y--)
        {
            bool full = true;
            for (int x = 0; x < Cols; x++) if (Cells[y * Cols + x] == 0) { full = false; break; }
            if (!full) continue;
            cleared++;
            Array.Copy(Cells, 0, Cells, Cols, y * Cols);
            Array.Clear(Cells, 0, Cols);
            y++; // look at the same row again (it now holds the one above)
        }
        if (cleared > 0)
        {
            Lines += cleared;
            Score += new[] { 0, 100, 300, 500, 800 }[cleared] * Level;
        }
        Spawn();
        return cleared;
    }

    /// <summary>Straight down; the rows cleared.</summary>
    public int Drop()
    {
        int r;
        int rows = 0;
        while ((r = Fall()) == -1) rows++;
        Score += rows * 2;
        return r;
    }

    /// <summary>Where the piece would land (the ghost).</summary>
    public int GhostY()
    {
        int y = PY;
        while (Fits(Kind, Rot, PX, y + 1)) y++;
        return y;
    }
}

public sealed class BalokView : GameView
{
    public Pit Pit { get; private set; }
    readonly HighScores _best;
    readonly Particles _fx = new Particles();
    readonly Rng _rng = new Rng(31);
    readonly ulong _seed;
    double _fall, _repeat;
    public event Action? Changed;
    public static readonly int[] Colors = { 0, 0x4FA3E0, 0xF2D14B, 0x8E6BD8, 0x5FB85A, 0xE0563B, 0x3A6FD8, 0xE9A64C };
    const int Cell = 18;

    public BalokView(ulong seed, HighScores best) : base(Pit.Cols * Cell + 110, Pit.Rows * Cell, 60) { _seed = seed; _best = best; Pit = new Pit(seed); }

    protected override void Update()
    {
        if (Pit.Over)
        {
            if (Input.AnyPressed(RawKey.Enter, RawKey.Space)) { Pit = new Pit(_seed + (ulong)Frames); Changed?.Invoke(); }
            _fx.Update(Dt);
            return;
        }
        int before = Pit.Lines;
        if (Input.AnyPressed(RawKey.Up, RawKey.X, RawKey.W)) { if (Pit.Rotate()) Sfx.Blip(); }
        int dir = Input.Axis(RawKey.Left, RawKey.Right, RawKey.A, RawKey.D);
        if (Input.AnyPressed(RawKey.Left, RawKey.A, RawKey.Right, RawKey.D)) { Pit.Move(dir); _repeat = -0.18; }
        else if (dir != 0) { _repeat += Dt; if (_repeat >= 0.05) { _repeat = 0; Pit.Move(dir); } }
        if (Input.WasPressed(RawKey.Space)) { Land(Pit.Drop()); }
        else
        {
            _fall += Dt * (Input.AnyDown(RawKey.Down, RawKey.S) ? 10 : 1);
            while (_fall >= Pit.Gravity && !Pit.Over) { _fall -= Pit.Gravity; Land(Pit.Fall()); }
        }
        if (Pit.Lines != before || Pit.Over) Changed?.Invoke();
        _fx.Update(Dt);
    }

    void Land(int cleared)
    {
        if (cleared > 0) { Sfx.Score(); _fx.Burst(_rng, Pit.Cols * Cell / 2.0, Pit.Rows * Cell * 0.7, 12 * cleared, 0x9BE07A, 160, 0.6); }
        else if (cleared == 0) Sfx.Hit();
        if (Pit.Over) { Sfx.Lose(); _best.Add(Pit.Score, save: GameEnv.Interactive); }
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillRect(f, 0x0E1A14);
        dc.FillRect(SRect(0, 0, Pit.Cols * Cell, Pit.Rows * Cell), 0x13261B);
        for (int y = 0; y < Pit.Rows; y++)
            for (int x = 0; x < Pit.Cols; x++)
            {
                int v = Pit.Cells[y * Pit.Cols + x];
                if (v != 0) Block(dc, x, y, Colors[v]);
            }
        if (!Pit.Over)
        {
            int gy = Pit.GhostY() - Pit.PY;
            foreach (var (x, y) in Pit.Current())
                if (y + gy >= 0) dc.StrokeRoundRect(SRect(x * Cell + 1, (y + gy) * Cell + 1, Cell - 2, Cell - 2), Math.Max(2, Sl(3)), 0x3E5C48, 1);
            foreach (var (x, y) in Pit.Current()) if (y >= 0) Block(dc, x, y, Colors[Pit.Kind + 1]);
        }
        // the side panel: next piece, score
        int px = Pit.Cols * Cell + 12;
        dc.Text(Sx(px), Sy(6), "Berikut", 0xA9C7B5, 0);
        foreach (var (x, y) in Pit.CellsOf(Pit.NextKind, 0))
            dc.FillRoundRect(SRect(px + x * 14, 30 + y * 14, 13, 13), Math.Max(2, Sl(3)), Colors[Pit.NextKind + 1]);
        dc.Text(Sx(px), Sy(100), "Skor", 0xA9C7B5, 0);
        dc.Text(Sx(px), Sy(118), Pit.Score.ToString(), 0xEAF6EE, 1);
        dc.Text(Sx(px), Sy(150), "Baris " + Pit.Lines, 0xA9C7B5, 0);
        dc.Text(Sx(px), Sy(170), "Level " + Pit.Level, 0xA9C7B5, 0);
        dc.Text(Sx(px), Sy(200), "Terbaik", 0xA9C7B5, 0);
        dc.Text(Sx(px), Sy(218), Math.Max(_best.Best, Pit.Score).ToString(), 0xF2B544, 1);
        _fx.Draw(dc, f.X, f.Y);
        if (Pit.Over) Banner(dc, SRect(0, 0, Pit.Cols * Cell, Pit.Rows * Cell), "Gudang penuh", "Enter untuk main lagi");
    }

    void Block(DrawingContext dc, int x, int y, int color) =>
        dc.FillGradient(SRect(x * Cell + 1, y * Cell + 1, Cell - 2, Cell - 2), Math.Max(2, Sl(3)), Color.Lighten(color, 28), color);
}

public static class BalokApp
{
    public static (Window, BalokView) Build(bool headless = false, ulong seed = 0)
    {
        var best = new HighScores("BALOK", load: !headless);
        var view = new BalokView(seed == 0 ? (ulong)Clock.Now | 1 : seed, best);
        var status = new TextBlock("Panah: geser, Atas/X: putar, Bawah: cepat, Spasi: jatuhkan, P: jeda") { Margin = new Thickness(4, 0, 4, 8) };
        var w = GameHost.Build("Balok", view, 400, 520, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Balok");
        // every shape and rotation has four distinct cells inside its 4x4 box
        bool shapes = true;
        for (int k = 0; k < 7; k++)
            for (int r = 0; r < 4; r++)
            {
                var cells = Pit.CellsOf(k, r).ToList();
                shapes &= cells.Distinct().Count() == 4 && cells.All(p => p.X >= 0 && p.X < 4 && p.Y >= 0 && p.Y < 4);
            }
        c.That(shapes, "7 shapes x 4 rotations: four cells each, inside the box");
        var pit = new Pit(1);
        int x0 = pit.PX;
        pit.Move(-1);
        c.That(pit.PX == x0 - 1, "a piece moves sideways");
        int moves = 0;
        while (pit.Move(-1)) moves++;
        c.That(!pit.Move(-1), "the wall stops it (" + moves + " more moves)");
        // a full row is harvested
        var p2 = new Pit(2);
        for (int x = 0; x < Pit.Cols; x++) p2.Cells[(Pit.Rows - 1) * Pit.Cols + x] = 1;
        p2.Cells[(Pit.Rows - 1) * Pit.Cols + 0] = 0;
        p2.Cells[(Pit.Rows - 2) * Pit.Cols + 5] = 3;
        // fill the gap by hand and lock through a fall
        p2.Cells[(Pit.Rows - 1) * Pit.Cols + 0] = 2;
        int pre = p2.Lines;
        p2.Drop();
        c.That(p2.Lines == pre + 1 && p2.Cells[(Pit.Rows - 1) * Pit.Cols + 5] == 3, "a full row is cleared and the row above falls into it");
        // stacking to the top ends the game
        var p3 = new Pit(3);
        int guard = 0;
        while (!p3.Over && guard++ < 200) p3.Drop();
        c.That(p3.Over && guard < 200, "a pit stacked to the top ends the game (" + guard + " pieces)");

        var (w, view) = Build(headless: true, seed: 4);
        int px = view.Pit.PX;
        w.DispatchKeyEvent(RawKey.Right, true); view.Step(); w.DispatchKeyEvent(RawKey.Right, false); view.Step();
        c.That(view.Pit.PX == px + 1, "Right (KEY_EVENT) moves the piece one column");
        int py = view.Pit.PY;
        view.Step(60);
        c.That(view.Pit.PY > py || view.Pit.PY == 0, "gravity pulls it down over a second");
        int lockedBefore = view.Pit.Cells.Count(v => v != 0);
        w.SimulateKeyEvents(" ");
        view.Step();
        c.That(view.Pit.Cells.Count(v => v != 0) == lockedBefore + 4, "Space drops it to the floor (4 cells locked)");
        w.RenderFrame();
        c.That(w.LastFrameOps > 20, "the pit drew " + w.LastFrameOps + " ops");
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
            var c = BalokApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = BalokApp.Build();
        app.Run(w);
        return 0;
    }
}
