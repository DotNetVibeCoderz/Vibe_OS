// Ranjau -- clear the garden plot without digging up a buried mine. A number
// says how many mines touch that square. Click / Space opens a square, a long
// press / F plants a flag; arrow keys move the cursor. The first square is
// always safe. Buitenzorg.Game.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace MRanjau;

public sealed class Field
{
    public int Cols { get; }
    public int Rows { get; }
    public int MineCount { get; }
    public readonly bool[] Mine;
    public readonly bool[] Open;
    public readonly bool[] Flag;
    public bool Started { get; private set; }
    public bool Lost { get; private set; }
    public bool Won { get; private set; }
    public int Opened { get; private set; }
    readonly Rng _rng;

    public Field(int cols, int rows, int mines, ulong seed)
    {
        Cols = cols; Rows = rows; MineCount = mines;
        Mine = new bool[cols * rows]; Open = new bool[cols * rows]; Flag = new bool[cols * rows];
        _rng = new Rng(seed);
    }

    public int Index(int x, int y) => y * Cols + x;
    bool In(int x, int y) => x >= 0 && y >= 0 && x < Cols && y < Rows;
    public int Flags { get { int n = 0; foreach (var f in Flag) if (f) n++; return n; } }

    /// <summary>Mines around (x, y).</summary>
    public int Around(int x, int y)
    {
        int n = 0;
        for (int dy = -1; dy <= 1; dy++)
            for (int dx = -1; dx <= 1; dx++)
                if ((dx != 0 || dy != 0) && In(x + dx, y + dy) && Mine[Index(x + dx, y + dy)]) n++;
        return n;
    }

    /// <summary>Lay the mines, keeping (sx, sy) and its neighbours clear.</summary>
    public void Lay(int sx, int sy)
    {
        int placed = 0;
        while (placed < MineCount)
        {
            int x = _rng.Next(Cols), y = _rng.Next(Rows);
            if (Math.Abs(x - sx) <= 1 && Math.Abs(y - sy) <= 1) continue;
            int i = Index(x, y);
            if (Mine[i]) continue;
            Mine[i] = true;
            placed++;
        }
        Started = true;
    }

    /// <summary>Open a square (and, if it has no mines around, its neighbours).
    /// Returns the squares opened, -1 when it was a mine.</summary>
    public int Reveal(int x, int y)
    {
        if (Lost || Won || !In(x, y)) return 0;
        if (!Started) Lay(x, y);
        int i = Index(x, y);
        if (Open[i] || Flag[i]) return 0;
        if (Mine[i]) { Open[i] = true; Lost = true; return -1; }
        int n = 0;
        var stack = new Stack<int>();
        stack.Push(i);
        while (stack.Count > 0)
        {
            int k = stack.Pop();
            if (Open[k] || Flag[k]) continue;
            Open[k] = true; n++;
            int kx = k % Cols, ky = k / Cols;
            if (Around(kx, ky) != 0) continue;
            for (int dy = -1; dy <= 1; dy++)
                for (int dx = -1; dx <= 1; dx++)
                    if (In(kx + dx, ky + dy) && !Open[Index(kx + dx, ky + dy)]) stack.Push(Index(kx + dx, ky + dy));
        }
        Opened += n;
        if (Opened == Cols * Rows - MineCount) Won = true;
        return n;
    }

    public void ToggleFlag(int x, int y)
    {
        if (!In(x, y) || Lost || Won) return;
        int i = Index(x, y);
        if (!Open[i]) Flag[i] = !Flag[i];
    }
}

public sealed class RanjauView : GameView
{
    public Field Field { get; private set; }
    public int CursorX { get; private set; }
    public int CursorY { get; private set; }
    public double Seconds { get; private set; }
    readonly HighScores _best;
    readonly Particles _fx = new Particles();
    readonly Rng _rng = new Rng(11);
    readonly ulong _seed;
    public event Action? Changed;

    const int Cell = 24;

    public RanjauView(ulong seed, HighScores best) : base(16 * Cell, 12 * Cell, 30)
    {
        _seed = seed;
        _best = best;
        Field = new Field(16, 12, 30, seed);
    }

    public void NewGame() { Field = new Field(16, 12, 30, _seed + (ulong)Frames); Seconds = 0; _fx.Clear(); Changed?.Invoke(); }

    public void Open(int x, int y)
    {
        int r = Field.Reveal(x, y);
        if (r < 0) { Sfx.Lose(); _fx.Burst(_rng, x * Cell + 12, y * Cell + 12, 30, 0xE0563B, 160, 0.9, 4); }
        else if (r > 0) Sfx.Blip();
        if (Field.Won)
        {
            Sfx.Score();
            _best.Add(Math.Max(1, 1000 - (int)Seconds), save: GameEnv.Interactive);
            _fx.Burst(_rng, Field.Cols * Cell / 2, Field.Rows * Cell / 2, 60, 0xF2B544, 200, 1.2, 4);
        }
        Changed?.Invoke();
    }

    protected override void Update()
    {
        if (Input.AnyPressed(RawKey.Left, RawKey.A)) CursorX = Math.Max(0, CursorX - 1);
        if (Input.AnyPressed(RawKey.Right, RawKey.D)) CursorX = Math.Min(Field.Cols - 1, CursorX + 1);
        if (Input.AnyPressed(RawKey.Up, RawKey.W)) CursorY = Math.Max(0, CursorY - 1);
        if (Input.AnyPressed(RawKey.Down, RawKey.S)) CursorY = Math.Min(Field.Rows - 1, CursorY + 1);
        if (Input.WasPressed(RawKey.Space) || Input.WasPressed(RawKey.Enter))
        {
            if (Field.Lost || Field.Won) NewGame(); else Open(CursorX, CursorY);
        }
        if (Input.WasPressed(RawKey.F)) { Field.ToggleFlag(CursorX, CursorY); Changed?.Invoke(); }
        if (Input.WasPressed(RawKey.N)) NewGame();
        if (Field.Started && !Field.Lost && !Field.Won) Seconds += Dt;
        _fx.Update(Dt);
    }

    (int, int)? CellAt(int px, int py)
    {
        var f = FieldRect;
        if (!f.Contains(px, py)) return null;
        return ((px - f.X) * Field.Cols / f.Width, (py - f.Y) * Field.Rows / f.Height);
    }

    protected override void OnPointerPressed(PointerEventArgs e) { Focus(); e.Handled = true; }

    protected override void OnPointerReleased(PointerEventArgs e)
    {
        if (CellAt(e.X, e.Y) is (int x, int y))
        {
            CursorX = x; CursorY = y;
            if (Field.Lost || Field.Won) NewGame(); else Open(x, y);
            InvalidateVisual();
        }
        e.Handled = true;
    }

    protected override void OnHold(PointerEventArgs e)
    {
        if (CellAt(e.X, e.Y) is (int x, int y)) { Field.ToggleFlag(x, y); Changed?.Invoke(); InvalidateVisual(); }
        e.Handled = true;
    }

    static readonly int[] NumberInk = { 0, 0x4FA3E0, 0x5FB85A, 0xE0563B, 0x8E6BD8, 0xC0703A, 0x3AA5A0, 0x222222, 0x777777 };

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillRect(f, 0x1A2E22);
        for (int y = 0; y < Field.Rows; y++)
            for (int x = 0; x < Field.Cols; x++)
            {
                int i = Field.Index(x, y);
                var r = SRect(x * Cell + 1, y * Cell + 1, Cell - 2, Cell - 2);
                if (!Field.Open[i])
                {
                    bool show = Field.Lost && Field.Mine[i];
                    dc.FillGradient(r, Math.Max(2, Sl(4)), show ? 0x7A3328 : 0x4E8A55, show ? 0x5A2219 : 0x3C6E44);
                    if (Field.Flag[i])
                    {
                        dc.FillRect(SRect(x * Cell + 9, y * Cell + 5, 2, 14), 0xEAF6EE);
                        dc.FillRect(SRect(x * Cell + 11, y * Cell + 5, 7, 6), 0xE0563B);
                    }
                    else if (show) dc.Ellipse(SRect(x * Cell + 6, y * Cell + 6, 12, 12), 0x10241A, true);
                    continue;
                }
                dc.FillRoundRect(r, Math.Max(2, Sl(3)), Field.Mine[i] ? 0xE0563B : 0xD9CBA8);
                if (Field.Mine[i]) { dc.Ellipse(SRect(x * Cell + 6, y * Cell + 6, 12, 12), 0x10241A, true); continue; }
                int n = Field.Around(x, y);
                if (n > 0)
                {
                    string s = n.ToString();
                    int tw = Fonts.Measure(s, 1), th = Fonts.LineHeight(1);
                    dc.Text(r.X + (r.Width - tw) / 2, r.Y + (r.Height - th) / 2, s, NumberInk[n], 1);
                }
            }
        dc.StrokeRoundRect(SRect(CursorX * Cell, CursorY * Cell, Cell, Cell), Math.Max(2, Sl(4)), 0xF2B544, 2);
        _fx.Draw(dc, f.X, f.Y);
        if (Field.Lost) Banner(dc, f, "Ranjau meledak", "Klik atau Spasi untuk kebun baru");
        else if (Field.Won) Banner(dc, f, "Kebun aman!", (int)Seconds + " detik  -  klik untuk lagi");
    }
}

public static class RanjauApp
{
    public static (Window, RanjauView) Build(bool headless = false, ulong seed = 0)
    {
        var best = new HighScores("RANJAU", load: !headless);
        var view = new RanjauView(seed == 0 ? (ulong)Clock.Now | 1 : seed, best);
        var status = new TextBlock() { Margin = new Thickness(4, 0, 4, 8) };
        void Refresh() => status.Text = "Ranjau " + view.Field.MineCount + "    Bendera " + view.Field.Flags + "    Waktu " + (int)view.Seconds + " dtk    (klik buka, tahan/F bendera, N baru)";
        view.Changed += Refresh;
        Refresh();
        var w = GameHost.Build("Ranjau", view, 430, 380, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Ranjau");
        var f = new Field(9, 9, 10, 3);
        int n = f.Reveal(4, 4);
        int mines = 0; foreach (var m in f.Mine) if (m) mines++;
        c.That(mines == 10 && !f.Mine[f.Index(4, 4)] && n > 1, "the first square is safe and opens an area (" + n + " squares)");
        c.That(f.Around(4, 4) == 0, "around the first square: no mines (so it cascades)");
        // open every safe square: won
        for (int i = 0; i < 81; i++) if (!f.Mine[i]) f.Reveal(i % 9, i / 9);
        c.That(f.Won && !f.Lost, "opening every safe square wins");
        var g = new Field(9, 9, 10, 4);
        g.Reveal(0, 0);
        int mi = Array.IndexOf(g.Mine, true);
        g.ToggleFlag(mi % 9, mi / 9);
        c.That(g.Reveal(mi % 9, mi / 9) == 0 && !g.Lost, "a flagged mine cannot be opened");
        g.ToggleFlag(mi % 9, mi / 9);
        c.That(g.Reveal(mi % 9, mi / 9) == -1 && g.Lost, "opening a mine loses");

        var (w, view) = Build(headless: true, seed: 77);
        w.DispatchKeyEvent(RawKey.Right, true); view.Step(); w.DispatchKeyEvent(RawKey.Right, false);
        w.DispatchKeyEvent(RawKey.Down, true); view.Step(); w.DispatchKeyEvent(RawKey.Down, false);
        c.That(view.CursorX == 1 && view.CursorY == 1, "arrow keys move the cursor");
        w.SimulateKeyEvents(" ");
        view.Step();
        c.That(view.Field.Opened > 0, "Space opened squares (" + view.Field.Opened + ")");
        var fr = view.FieldRect;
        w.SimulatePointer(1, fr.X + fr.Width - 5, fr.Y + fr.Height - 5);
        w.SimulatePointer(2, fr.X + fr.Width - 5, fr.Y + fr.Height - 5);
        w.SimulatePointer(3, fr.X + fr.Width - 5, fr.Y + fr.Height - 5);
        c.That(view.CursorX == 15 && view.CursorY == 11, "a click picks the square under the pointer");
        w.RenderFrame();
        c.That(w.LastFrameOps > 150, "the field drew " + w.LastFrameOps + " ops");
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
            var c = RanjauApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = RanjauApp.Build();
        app.Run(w);
        return 0;
    }
}
