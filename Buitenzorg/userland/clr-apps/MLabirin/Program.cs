// Labirin Kebun -- a first-person maze in the botanical garden: find the five
// golden flowers between the hedges, then the gate in the old stone wall
// opens. Arrow keys (or W / S) walk, Left / Right turn, A / D step sideways,
// P pauses, Enter starts a new maze. The picture is ray-cast in the host
// (RayRender native, clrhost raycast.rs): one call per frame, one Blit.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace MLabirin;

/// <summary>The maze and the rules (no drawing).</summary>
public sealed class Maze
{
    public const int Cells = 8;                 // 8x8 cells -> a 17x17 grid
    public const int Size = Cells * 2 + 1;
    public const int Flowers = 5;
    public readonly byte[] Map = new byte[Size * Size];   // 0 open, 1 hedge, 2 stone, 3 gate
    public double X, Y, Angle;                  // player: cell units, radians
    public readonly List<(double X, double Y, bool Taken)> Items = new List<(double, double, bool)>();
    public int Taken => Items.Count(i => i.Taken);
    public bool GateOpen { get; private set; }
    public bool Won { get; private set; }
    public double Time;
    public (int X, int Y) Gate;
    readonly Rng _rng;

    public Maze(ulong seed) { _rng = new Rng(seed); Generate(); }

    public byte At(int x, int y) => x < 0 || y < 0 || x >= Size || y >= Size ? (byte)2 : Map[y * Size + x];

    public void Generate()
    {
        for (int i = 0; i < Map.Length; i++) Map[i] = 1;
        for (int i = 0; i < Size; i++) { Map[i] = 2; Map[(Size - 1) * Size + i] = 2; Map[i * Size] = 2; Map[i * Size + Size - 1] = 2; }
        // depth-first carving from cell (0, 0)
        var seen = new bool[Cells * Cells];
        var stack = new List<int> { 0 };
        seen[0] = true;
        Map[1 * Size + 1] = 0;
        while (stack.Count > 0)
        {
            int c = stack[stack.Count - 1];
            int cx = c % Cells, cy = c / Cells;
            var options = new List<int>();
            if (cx > 0 && !seen[c - 1]) options.Add(c - 1);
            if (cx < Cells - 1 && !seen[c + 1]) options.Add(c + 1);
            if (cy > 0 && !seen[c - Cells]) options.Add(c - Cells);
            if (cy < Cells - 1 && !seen[c + Cells]) options.Add(c + Cells);
            if (options.Count == 0) { stack.RemoveAt(stack.Count - 1); continue; }
            int n = options[_rng.Next(options.Count)];
            int nx = n % Cells, ny = n / Cells;
            Map[(cy * 2 + 1 + (ny - cy)) * Size + cx * 2 + 1 + (nx - cx)] = 0;
            Map[(ny * 2 + 1) * Size + nx * 2 + 1] = 0;
            seen[n] = true;
            stack.Add(n);
        }
        // the gate: in the east wall beside the far corner
        Gate = (Size - 1, Size - 2);
        Map[Gate.Y * Size + Gate.X] = 3;
        // flowers on cells far from the start
        Items.Clear();
        var used = new HashSet<int> { 0 };
        while (Items.Count < Flowers)
        {
            int c = _rng.Next(Cells * Cells);
            if (used.Contains(c) || (c % Cells) + (c / Cells) < 3) continue;
            used.Add(c);
            Items.Add(((c % Cells) * 2 + 1.5, (c / Cells) * 2 + 1.5, false));
        }
        // start facing the open corridor, not the hedge
        X = 1.5; Y = 1.5; Time = 0; GateOpen = false; Won = false;
        Angle = Map[1 * Size + 2] == 0 ? 0 : Math.PI / 2;
    }

    /// <summary>Every flower and the gate reachable from the start (the generator's promise).</summary>
    public bool AllReachable()
    {
        var seen = new bool[Size * Size];
        var queue = new Queue<int>();
        queue.Enqueue(1 * Size + 1);
        seen[1 * Size + 1] = true;
        bool gate = false;
        while (queue.Count > 0)
        {
            int p = queue.Dequeue();
            int x = p % Size, y = p / Size;
            foreach (var (dx, dy) in new[] { (1, 0), (-1, 0), (0, 1), (0, -1) })
            {
                int q = (y + dy) * Size + x + dx;
                if (x + dx < 0 || y + dy < 0 || x + dx >= Size || y + dy >= Size || seen[q]) continue;
                if (Map[q] == 3) gate = true;
                if (Map[q] != 0) continue;
                seen[q] = true;
                queue.Enqueue(q);
            }
        }
        foreach (var it in Items) if (!seen[(int)it.Y * Size + (int)it.X]) return false;
        return gate;
    }

    bool Free(double x, double y)
    {
        const double r = 0.22;
        for (int k = 0; k < 4; k++)
        {
            double cx = x + (k % 2 == 0 ? -r : r), cy = y + (k < 2 ? -r : r);
            byte cell = At((int)Math.Floor(cx), (int)Math.Floor(cy));
            if (cell != 0 && !(cell == 3 && GateOpen)) return false;
        }
        return true;
    }

    /// <summary>One step of the rules: walk (forward, side), turn; pick flowers up; open the gate; win.</summary>
    public int Step(double dt, double forward, double side, double turn)
    {
        if (Won) return 0;
        Time += dt;
        Angle += turn * 2.4 * dt;
        double speed = 2.6 * dt;
        double dx = (Math.Cos(Angle) * forward - Math.Sin(Angle) * side) * speed;
        double dy = (Math.Sin(Angle) * forward + Math.Cos(Angle) * side) * speed;
        if (Free(X + dx, Y)) X += dx;
        if (Free(X, Y + dy)) Y += dy;
        int picked = 0;
        for (int i = 0; i < Items.Count; i++)
        {
            var it = Items[i];
            if (!it.Taken && (it.X - X) * (it.X - X) + (it.Y - Y) * (it.Y - Y) < 0.36) { Items[i] = (it.X, it.Y, true); picked++; }
        }
        if (!GateOpen && Taken == Flowers) { GateOpen = true; return 2; }
        if (GateOpen && X > Size - 1.2) { Won = true; return 3; }
        return picked > 0 ? 1 : 0;
    }
}

/// <summary>The first-person view.</summary>
public sealed class LabirinView : GameView
{
    public Maze Game;
    public event Action? Changed;
    int[] _frame = Array.Empty<int>();
    int _fw, _fh;
    public int LastRender = -2;   // the native's result (-2 = not drawn yet, 1 = managed fallback)
    static bool _managed;

    [System.Runtime.InteropServices.DllImport("buitenzorg")]
    static extern int RayRender(int[] pixels, int w, int h, byte[] map, int mapW, int mapH, int px, int py, int angle, int[] sprites);

    public LabirinView(Maze m) : base(640, 400, 30) { Game = m; }

    protected override void Update()
    {
        if (Input.WasPressed(RawKey.Enter) && Game.Won) { Game.Generate(); Changed?.Invoke(); return; }
        double fwd = Input.Axis(RawKey.Down, RawKey.Up, RawKey.S, RawKey.W);
        double side = Input.Axis(RawKey.A, RawKey.D);
        double turn = Input.Axis(RawKey.Left, RawKey.Right);
        int r = Game.Step(Dt, fwd, side, turn);
        if (r == 1) Sfx.Score();
        else if (r == 2) Sfx.Tone(660, 200);
        else if (r == 3) Sfx.Tone(880, 400);
        if (r != 0 || (int)(Game.Time * 30) % 15 == 0) Changed?.Invoke();
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        int w = Math.Min(f.Width, 560), h = Math.Min(f.Height, 350);
        if (w < 16 || h < 16) return;
        if (_frame.Length != w * h) { _frame = new int[w * h]; _fw = w; _fh = h; }
        var sprites = new List<int>();
        foreach (var it in Game.Items) if (!it.Taken) { sprites.Add((int)(it.X * 1024)); sprites.Add((int)(it.Y * 1024)); sprites.Add(1); }
        var map = Game.Map;
        if (Game.GateOpen)
        {
            map = new byte[Game.Map.Length];
            Array.Copy(Game.Map, map, map.Length);
            map[Game.Gate.Y * Maze.Size + Game.Gate.X] = 0; // the gate stands open
        }
        int angle = (int)(Game.Angle / (2 * Math.PI) * 4096);
        if (!_managed)
        {
            try { LastRender = RayRender(_frame, w, h, map, Maze.Size, Maze.Size, (int)(Game.X * 1024), (int)(Game.Y * 1024), angle, sprites.ToArray()); }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _managed = true; }
        }
        if (_managed) { ManagedFrame(map, w, h); LastRender = 1; }
        int x = f.X + (f.Width - w) / 2, y = f.Y + (f.Height - h) / 2;
        dc.Blit(x, y, w, h, _frame);
        if (Game.Won) Banner(dc, f, "Keluar dari labirin!", Game.Taken + " bunga dalam " + (int)Game.Time + " detik  -  Enter untuk labirin baru");
    }

    /// <summary>Off the device: flat-colored columns (tests and `dotnet` runs).</summary>
    void ManagedFrame(byte[] map, int w, int h)
    {
        for (int i = 0; i < _frame.Length; i++) _frame[i] = i < w * h / 2 ? 0x8CB4DC : 0x2E5A2A;
        double dirX = Math.Cos(Game.Angle), dirY = Math.Sin(Game.Angle);
        double plX = -dirY * 0.66, plY = dirX * 0.66;
        for (int x = 0; x < w; x += 4)
        {
            double cam = 2.0 * x / w - 1, rx = dirX + plX * cam, ry = dirY + plY * cam;
            double d = 0;
            byte cell = 0;
            while (d < 20) { d += 0.05; cell = Game.At((int)(Game.X + rx * d), (int)(Game.Y + ry * d)); if (cell != 0 && !(cell == 3 && Game.GateOpen)) break; }
            int line = (int)(h / Math.Max(0.1, d * 0.9));
            int c = cell == 1 ? 0x2F6B2F : cell == 2 ? 0x9A9078 : 0xC08040;
            for (int y = Math.Max(0, h / 2 - line / 2); y < Math.Min(h, h / 2 + line / 2); y++)
                for (int k = 0; k < 4 && x + k < w; k++) _frame[y * w + x + k] = c;
        }
    }

    /// <summary>Lit (non-sky, non-lawn) pixels of the last frame, for tests.</summary>
    public int WallPixels()
    {
        int n = 0;
        for (int i = 0; i < _frame.Length; i += 7) { int c = _frame[i] & 0xFFFFFF; if (c != 0) n++; }
        return n;
    }

    public int[] Frame => _frame;
}

public static class LabirinApp
{
    public static (Window, LabirinView) Build(bool headless = false, ulong seed = 0)
    {
        var maze = new Maze(seed == 0 ? (ulong)Clock.Now | 1 : seed);
        var view = new LabirinView(maze);
        var best = new HighScores("LABIRIN", load: !headless);
        var status = new TextBlock("") { Margin = new Thickness(4, 0, 4, 8) };
        void Refresh() => status.Text = "Bunga " + view.Game.Taken + "/" + Maze.Flowers + (view.Game.GateOpen ? "   - gerbang terbuka di dinding timur!" : "") + "    Waktu " + (int)view.Game.Time + " dtk    (panah/WASD, A/D geser, P jeda)";
        view.Changed += Refresh;
        Refresh();
        var w = GameHost.Build("Labirin Kebun", view, 620, 470, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Labirin");
        bool allOk = true;
        for (ulong s = 1; s <= 12; s++) { var m = new Maze(s); if (!m.AllReachable() || m.Items.Count != Maze.Flowers) allOk = false; }
        c.That(allOk, "12 generated mazes: every flower and the gate reachable");
        var g = new Maze(3);
        // walk into the north wall: the hedge stops the player
        g.Angle = -Math.PI / 2;
        for (int i = 0; i < 60; i++) g.Step(1.0 / 30, 1, 0, 0);
        c.That(g.Y > 1.2 && g.Y < 1.5, "the wall stops the player (y*100 = " + (int)(g.Y * 100) + ")");
        // stand on each flower: picked up; the gate opens after the fifth
        int r = 0;
        for (int i = 0; i < g.Items.Count; i++) { g.X = g.Items[i].X; g.Y = g.Items[i].Y; r = g.Step(1.0 / 30, 0, 0, 0); }
        c.That(g.Taken == Maze.Flowers && g.GateOpen && r == 2, "five flowers picked up, the gate opened");
        g.X = Maze.Size - 1.5; g.Y = Maze.Size - 1.5; g.Angle = 0;
        for (int i = 0; i < 40 && !g.Won; i++) g.Step(1.0 / 30, 1, 0, 0);
        c.That(g.Won, "walking out through the gate wins");

        var (w, view) = Build(headless: true, seed: 4);
        w.RenderFrame();
        c.That(view.LastRender == 0 || view.LastRender == 1, "rendered (" + (view.LastRender == 0 ? "host ray caster" : "managed fallback") + ")");
        c.That(view.WallPixels() > 100, "the frame has a picture (" + view.WallPixels() + " sampled pixels)");
        double a0 = view.Game.Angle;
        w.DispatchKeyEvent(RawKey.Right, true);
        view.Step(15);
        w.DispatchKeyEvent(RawKey.Right, false);
        c.That(view.Game.Angle > a0 + 0.5, "the Right key turned the view");
        c.That(w.LastFrameOps > 0, "one blit per frame (" + w.LastFrameOps + " ops)");
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
            var c = LabirinApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = LabirinApp.Build();
        app.Run(w);
        return 0;
    }
}
