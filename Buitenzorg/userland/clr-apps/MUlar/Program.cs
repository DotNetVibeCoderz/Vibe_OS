// Ular -- a snake in the botanical garden: eat the fruit, grow, do not bite
// yourself or the hedge. Arrow keys / WASD, P pause, Enter / Space restart.
// Buitenzorg.Game.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace MUlar;

/// <summary>The rules on a grid (no drawing): tests drive it directly.</summary>
public sealed class Snake
{
    public const int Cols = 24, Rows = 18;
    public readonly List<(int X, int Y)> Body = new List<(int, int)>(); // head first
    public (int X, int Y) Dir { get; private set; } = (1, 0);
    (int X, int Y) _next = (1, 0);
    public (int X, int Y) Food { get; set; }
    public int Score { get; private set; }
    public bool Over { get; private set; }
    /// <summary>A new snake waits for the first direction key.</summary>
    public bool Waiting { get; private set; } = true;
    public int Eaten { get; private set; }
    readonly Rng _rng;

    public Snake(ulong seed) { _rng = new Rng(seed); Reset(); }

    public void Reset()
    {
        Body.Clear();
        for (int i = 0; i < 4; i++) Body.Add((8 - i, Rows / 2));
        Dir = (1, 0);
        _next = Dir;
        Score = 0;
        Eaten = 0;
        Over = false;
        Waiting = true;
        PlaceFood();
    }

    /// <summary>Turn (ignored when it would reverse onto the neck).</summary>
    public void Turn(int dx, int dy)
    {
        Waiting = false;
        if (dx == -Dir.X && dy == -Dir.Y) return;
        _next = (dx, dy);
    }

    public void PlaceFood()
    {
        for (int tries = 0; tries < 1000; tries++)
        {
            var f = (_rng.Next(Cols), _rng.Next(Rows));
            if (!Body.Contains(f)) { Food = f; return; }
        }
    }

    /// <summary>Moves per second: faster as it grows.</summary>
    public double Speed => Math.Min(14, 6 + Eaten * 0.4);

    /// <summary>One move. Returns 0 moved, 1 ate, 2 died.</summary>
    public int Advance()
    {
        if (Over) return 2;
        Waiting = false;
        Dir = _next;
        var h = (X: Body[0].X + Dir.X, Y: Body[0].Y + Dir.Y);
        bool ate = h == Food;
        // the tail moves away this turn unless the snake grows
        int len = ate ? Body.Count : Body.Count - 1;
        bool self = false;
        for (int i = 0; i < len; i++) if (Body[i] == h) { self = true; break; }
        if (h.X < 0 || h.Y < 0 || h.X >= Cols || h.Y >= Rows || self) { Over = true; return 2; }
        Body.Insert(0, h);
        if (ate)
        {
            Eaten++;
            Score += 10 + Eaten;
            PlaceFood();
            return 1;
        }
        Body.RemoveAt(Body.Count - 1);
        return 0;
    }
}

public sealed class UlarView : GameView
{
    public Snake Game { get; }
    public HighScores Best { get; }
    readonly Particles _fx = new Particles();
    readonly Rng _rng = new Rng(7);
    double _moveAcc;
    public event Action? Changed;

    public UlarView(Snake game, HighScores best) : base(Snake.Cols * 20, Snake.Rows * 20, 60) { Game = game; Best = best; }

    protected override void Update()
    {
        if (Input.AnyPressed(RawKey.Up, RawKey.W)) Game.Turn(0, -1);
        else if (Input.AnyPressed(RawKey.Down, RawKey.S)) Game.Turn(0, 1);
        else if (Input.AnyPressed(RawKey.Left, RawKey.A)) Game.Turn(-1, 0);
        else if (Input.AnyPressed(RawKey.Right, RawKey.D)) Game.Turn(1, 0);
        if (Game.Over)
        {
            if (Input.AnyPressed(RawKey.Enter, RawKey.Space)) { Game.Reset(); Changed?.Invoke(); }
            _fx.Update(Dt);
            return;
        }
        if (Game.Waiting) { _fx.Update(Dt); return; }
        _moveAcc += Dt * Game.Speed;
        while (_moveAcc >= 1)
        {
            _moveAcc -= 1;
            var food = Game.Food;
            int r = Game.Advance();
            if (r == 1) { Sfx.Score(); _fx.Burst(_rng, food.X * 20 + 10, food.Y * 20 + 10, 14, 0xF2B544, 90, 0.5); Changed?.Invoke(); }
            if (r == 2) { Sfx.Lose(); Best.Add(Game.Score, save: GameEnv.Interactive); Changed?.Invoke(); break; }
        }
        _fx.Update(Dt);
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillRect(f, 0x13261B);
        // a checkered lawn
        for (int y = 0; y < Snake.Rows; y++)
            for (int x = (y & 1); x < Snake.Cols; x += 2)
                dc.FillRect(SRect(x * 20, y * 20, 20, 20), 0x173021);
        var food = Game.Food;
        dc.Ellipse(SRect(food.X * 20 + 3, food.Y * 20 + 3, 14, 14), 0xE0563B, true);
        dc.FillRect(SRect(food.X * 20 + 9, food.Y * 20 + 1, 3, 5), 0x6FBF4A);
        for (int i = Game.Body.Count - 1; i >= 0; i--)
        {
            var (x, y) = Game.Body[i];
            int shade = i == 0 ? 0x9BE07A : (i % 2 == 0 ? 0x5FB85A : 0x55A852);
            dc.FillRoundRect(SRect(x * 20 + 1, y * 20 + 1, 18, 18), Math.Max(2, Sl(6)), Game.Over ? 0x6F7A70 : shade);
        }
        var head = Game.Body[0];
        var d = Game.Dir;
        int ex = head.X * 20 + 10 + d.X * 4, ey = head.Y * 20 + 10 + d.Y * 4;
        dc.FillRect(SRect(ex - 4 + d.Y * 0, ey - 4, 3, 3), 0x10241A);
        dc.FillRect(SRect(ex + 2, ey + 2 - d.Y * 0, 3, 3), 0x10241A);
        _fx.Draw(dc, f.X, f.Y);
        if (Game.Over) Banner(dc, f, "Ular tersandung", "Skor " + Game.Score + "  -  Enter untuk main lagi");
        else if (Game.Waiting) Banner(dc, f, "Ular", "Tekan panah (atau WASD) untuk mulai");
    }
}

public static class UlarApp
{
    public static (Window, UlarView) Build(bool headless = false, ulong seed = 0)
    {
        var game = new Snake(seed == 0 ? (ulong)Clock.Now | 1 : seed);
        var best = new HighScores("ULAR", load: !headless);
        var view = new UlarView(game, best);
        var status = new TextBlock("Skor 0") { Margin = new Thickness(4, 0, 4, 8) };
        void Refresh() => status.Text = "Skor " + game.Score + "    Panjang " + game.Body.Count + "    Terbaik " + Math.Max(best.Best, game.Score) + "    (panah/WASD, P jeda)";
        view.Changed += Refresh;
        Refresh();
        var w = GameHost.Build("Ular", view, 520, 460, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Ular");
        var s = new Snake(42);
        c.That(s.Body.Count == 4 && !s.Body.Contains(s.Food), "a new snake: 4 long, the fruit not on it");
        // put the fruit right in front: eating grows it and scores
        s.Food = (s.Body[0].X + 1, s.Body[0].Y);
        int r = s.Advance();
        c.That(r == 1 && s.Body.Count == 5 && s.Score > 0, "eating the fruit grows the snake (score " + s.Score + ")");
        s.Turn(-1, 0);
        c.That(s.Advance() == 0 && s.Dir == (1, 0), "it cannot reverse onto its neck");
        // run into the right hedge
        int guard = 0;
        while (!s.Over && guard++ < 40) { s.Food = (0, 0); s.Advance(); }
        c.That(s.Over, "the hedge ends the game");
        // biting itself: a tight loop of turns
        var t = new Snake(9);
        for (int i = 0; i < 4; i++) { t.Food = (t.Body[0].X + t.Dir.X, t.Body[0].Y + t.Dir.Y); t.Advance(); }
        t.Turn(0, 1); t.Food = (0, 0); t.Advance();
        t.Turn(-1, 0); t.Advance();
        t.Turn(0, -1); t.Advance();
        c.That(t.Over, "biting its own body ends the game (length " + t.Body.Count + ")");

        // through the window: raw key events turn it, the loop moves it
        var (w, view) = Build(headless: true, seed: 5);
        var g = view.Game;
        g.Food = (0, 0);
        var still = g.Body[0];
        view.Step(60);
        c.That(g.Waiting && g.Body[0] == still, "a new snake waits for the first key");
        int y0 = g.Body[0].Y;
        w.DispatchKeyEvent(RawKey.Down, true);
        view.Step(12); // 60 Hz, 6 moves/s: one move per 10 updates
        w.DispatchKeyEvent(RawKey.Down, false);
        c.That(g.Dir == (0, 1) && g.Body[0].Y > y0, "the Down key (KEY_EVENT) turned it and it moved");
        w.SimulateKeyEvents("p");
        long f = view.Frames;
        view.Step(30);
        c.That(view.Paused && view.Frames == f, "P pauses the loop");
        w.SimulateKeyEvents("p");
        w.RenderFrame();
        c.That(w.LastFrameOps > 200, "the lawn, the snake and the fruit drew " + w.LastFrameOps + " ops");
        var hs = new HighScores("ULARTEST", load: false);
        hs.Add(30, save: false); hs.Add(90, save: false); hs.Add(60, save: false);
        c.That(hs.Best == 90 && hs.Scores[1] == 60, "high scores keep the best first");
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
            var c = UlarApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = UlarApp.Build();
        app.Run(w);
        return 0;
    }
}
