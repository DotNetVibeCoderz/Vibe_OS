// Bata -- knock the bricks out of the garden wall with a bouncing seed. The
// paddle follows the arrow keys / A-D (or the pointer); Space launches the
// ball. Three lives; every cleared wall comes back one row taller and faster.
// Buitenzorg.Game.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace MBata;

public sealed class Wall
{
    public const int W = 480, H = 360, Cols = 10, BrickW = 44, BrickH = 16, Top = 40, PaddleY = 336, PaddleH = 10, BallR = 6;
    public int Rows { get; private set; }
    public bool[] Bricks { get; private set; } = Array.Empty<bool>();
    public double PaddleX { get; set; } = W / 2.0;
    public double PaddleW { get; } = 72;
    public Vec2 Ball;
    public Vec2 Vel;
    public bool Launched { get; private set; }
    public int Lives { get; private set; }
    public int Score { get; private set; }
    public int Level { get; private set; }
    public bool Over => Lives <= 0;
    public int Left { get { int n = 0; foreach (var b in Bricks) if (b) n++; return n; } }

    public Wall() { NewGame(); }

    public void NewGame() { Lives = 3; Score = 0; Level = 1; Build(); }

    void Build()
    {
        Rows = Math.Min(8, 3 + Level);
        Bricks = new bool[Rows * Cols];
        Array.Fill(Bricks, true);
        Serve();
    }

    void Serve() { Launched = false; Ball = new Vec2(PaddleX, PaddleY - BallR - 1); Vel = new Vec2(0, 0); }

    public double BrickX(int c) => 20 + c * BrickW;
    public double BrickY(int r) => Top + r * (BrickH + 4);

    public void Launch()
    {
        if (Launched || Over) return;
        Launched = true;
        double speed = 220 + Level * 30;
        Vel = new Vec2(speed * 0.45, -speed);
    }

    /// <summary>One step of `dt` seconds. Returns: 0 nothing, 1 brick, 2 paddle, 3 lost a ball, 4 wall cleared.</summary>
    public int Step(double dt, int dir)
    {
        PaddleX = Math.Clamp(PaddleX + dir * 320 * dt, PaddleW / 2, W - PaddleW / 2);
        if (!Launched) { Ball = new Vec2(PaddleX, PaddleY - BallR - 1); return 0; }
        Ball = Ball + Vel * dt;
        int result = 0;
        if (Ball.X < BallR) { Ball.X = BallR; Vel.X = Math.Abs(Vel.X); }
        if (Ball.X > W - BallR) { Ball.X = W - BallR; Vel.X = -Math.Abs(Vel.X); }
        if (Ball.Y < BallR) { Ball.Y = BallR; Vel.Y = Math.Abs(Vel.Y); }
        // the paddle: the further from its middle, the steeper the bounce
        if (Vel.Y > 0 && Collide.CircleRect(Ball, BallR, PaddleX - PaddleW / 2, PaddleY, PaddleW, PaddleH) != 0)
        {
            double off = (Ball.X - PaddleX) / (PaddleW / 2);
            double speed = Vel.Length;
            var d = new Vec2(off * 0.8, -1).Normalized();
            Vel = d * speed;
            Ball.Y = PaddleY - BallR - 0.5;
            result = 2;
        }
        for (int r = 0; r < Rows; r++)
            for (int c = 0; c < Cols; c++)
            {
                int i = r * Cols + c;
                if (!Bricks[i]) continue;
                int side = Collide.CircleRect(Ball, BallR, BrickX(c) + 2, BrickY(r), BrickW - 4, BrickH);
                if (side == 0) continue;
                Bricks[i] = false;
                if (side == 1) Vel.Y = -Vel.Y; else Vel.X = -Vel.X;
                Score += 10 * (Rows - r);
                result = 1;
                if (Left == 0) { Level++; Build(); return 4; }
                return result;
            }
        if (Ball.Y > H + BallR)
        {
            Lives--;
            if (!Over) Serve();
            return 3;
        }
        return result;
    }
}

public sealed class BataView : GameView
{
    public Wall Wall { get; } = new Wall();
    readonly Particles _fx = new Particles();
    readonly Rng _rng = new Rng(21);
    readonly HighScores _best;
    int _pointerX = -1;
    public event Action? Changed;
    static readonly int[] RowColors = { 0xE0563B, 0xE9A64C, 0xF2D14B, 0x9BE07A, 0x5FB85A, 0x4FA3E0, 0x8E6BD8, 0xD86BA8 };

    public BataView(HighScores best) : base(Wall.W, Wall.H, 60) { _best = best; }

    protected override void Update()
    {
        if (Wall.Over)
        {
            if (Input.AnyPressed(RawKey.Enter, RawKey.Space)) { Wall.NewGame(); Changed?.Invoke(); }
            _fx.Update(Dt);
            return;
        }
        int dir = Input.Axis(RawKey.Left, RawKey.Right, RawKey.A, RawKey.D);
        if (dir == 0 && _pointerX >= 0)
        {
            double target = (_pointerX - FieldRect.X) * (double)Wall.W / Math.Max(1, FieldRect.Width);
            if (Math.Abs(target - Wall.PaddleX) > 4) dir = target > Wall.PaddleX ? 1 : -1;
        }
        if (Input.AnyPressed(RawKey.Space, RawKey.Up, RawKey.W)) Wall.Launch();
        var ball = Wall.Ball;
        int r = Wall.Step(Dt, dir);
        switch (r)
        {
            case 1: Sfx.Hit(); _fx.Burst(_rng, ball.X, ball.Y, 10, 0xF2D14B, 120, 0.4); Changed?.Invoke(); break;
            case 2: Sfx.Blip(); break;
            case 3:
                Sfx.Lose();
                if (Wall.Over) _best.Add(Wall.Score, save: GameEnv.Interactive);
                Changed?.Invoke();
                break;
            case 4: Sfx.Score(); _fx.Burst(_rng, Wall.W / 2.0, Wall.H / 2.0, 50, 0x9BE07A, 220, 1.0, 4); Changed?.Invoke(); break;
        }
        _fx.Update(Dt);
    }

    protected override void OnPointerMoved(PointerEventArgs e) { _pointerX = e.X; base.OnPointerMoved(e); }
    protected override void OnPointerPressed(PointerEventArgs e) { Focus(); Wall.Launch(); e.Handled = true; }
    protected override void OnPointerExited() { _pointerX = -1; base.OnPointerExited(); }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillGradient(f, 0, 0x16291E, 0x0E1A14);
        for (int r = 0; r < Wall.Rows; r++)
            for (int c = 0; c < Wall.Cols; c++)
            {
                if (!Wall.Bricks[r * Wall.Cols + c]) continue;
                int col = RowColors[r % RowColors.Length];
                dc.FillGradient(SRect(Wall.BrickX(c) + 2, Wall.BrickY(r), Wall.BrickW - 4, Wall.BrickH), Math.Max(2, Sl(4)), Color.Lighten(col, 30), col);
            }
        dc.FillRoundRect(SRect(Wall.PaddleX - Wall.PaddleW / 2, Wall.PaddleY, Wall.PaddleW, Wall.PaddleH), Math.Max(2, Sl(5)), 0xEAF6EE);
        dc.Ellipse(SRect(Wall.Ball.X - Wall.BallR, Wall.Ball.Y - Wall.BallR, Wall.BallR * 2, Wall.BallR * 2), 0xF2B544, true);
        _fx.Draw(dc, f.X, f.Y);
        if (!Wall.Launched && !Wall.Over)
        {
            string hint = "Spasi untuk meluncurkan";
            int tw = Fonts.Measure(hint, 0);
            dc.Text(f.X + (f.Width - tw) / 2, Sy(260), hint, 0xA9C7B5, 0);
        }
        if (Wall.Over) Banner(dc, f, "Tembok bertahan", "Skor " + Wall.Score + "  -  Enter untuk main lagi");
    }
}

public static class BataApp
{
    public static (Window, BataView) Build(bool headless = false)
    {
        var best = new HighScores("BATA", load: !headless);
        var view = new BataView(best);
        var status = new TextBlock() { Margin = new Thickness(4, 0, 4, 8) };
        void Refresh() => status.Text = "Skor " + view.Wall.Score + "    Nyawa " + view.Wall.Lives + "    Level " + view.Wall.Level + "    Terbaik " + Math.Max(best.Best, view.Wall.Score);
        view.Changed += Refresh;
        Refresh();
        var w = GameHost.Build("Bata", view, 520, 450, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Bata");
        var wall = new Wall();
        c.That(wall.Left == 40 && wall.Lives == 3 && !wall.Launched, "level 1: 4 rows of 10 bricks, 3 lives, the ball on the paddle");
        wall.Step(0.5, 1);
        c.That(wall.PaddleX > Wall.W / 2.0 && Math.Abs(wall.Ball.X - wall.PaddleX) < 0.01, "the paddle moves and carries the ball");
        wall.Launch();
        int hit = 0;
        for (int i = 0; i < 600 && hit == 0; i++) if (wall.Step(1.0 / 60, 0) == 1) hit = i;
        c.That(hit > 0 && wall.Left == 39 && wall.Score > 0, "the launched ball breaks a brick (after " + hit + " steps)");
        // a ball past the paddle costs a life
        var w2 = new Wall();
        w2.Launch();
        w2.Ball = new Vec2(10, Wall.H - 2);
        w2.Vel = new Vec2(0, 300);
        int lost = 0;
        for (int i = 0; i < 30; i++) { if (w2.Step(1.0 / 60, 1) == 3) { lost = 1; break; } }
        c.That(lost == 1 && w2.Lives == 2 && !w2.Launched, "a missed ball costs a life and is served again");
        // the paddle bounces it upward
        var w3 = new Wall();
        w3.Launch();
        w3.Ball = new Vec2(w3.PaddleX + 20, Wall.PaddleY - 8);
        w3.Vel = new Vec2(0, 200);
        int r = 0;
        for (int i = 0; i < 10 && r != 2; i++) r = w3.Step(1.0 / 60, 0);
        c.That(r == 2 && w3.Vel.Y < 0 && w3.Vel.X > 0, "the paddle bounces it up, to the side it hit");
        // clearing the wall: the next level is taller
        var w4 = new Wall();
        w4.Launch();
        Array.Fill(w4.Bricks, false);
        w4.Bricks[0] = true;
        w4.Ball = new Vec2(w4.BrickX(0) + 20, w4.BrickY(0) + 30);
        w4.Vel = new Vec2(0, -300);
        int cleared = 0;
        for (int i = 0; i < 30; i++) if (w4.Step(1.0 / 60, 0) == 4) { cleared = 1; break; }
        c.That(cleared == 1 && w4.Level == 2 && w4.Left == 50, "a cleared wall comes back one row taller (level " + w4.Level + ")");

        var (win, view) = Build(headless: true);
        double x0 = view.Wall.PaddleX;
        win.DispatchKeyEvent(RawKey.Left, true);
        view.Step(20);
        win.DispatchKeyEvent(RawKey.Left, false);
        c.That(view.Wall.PaddleX < x0, "holding Left (KEY_EVENT) moves the paddle left");
        win.SimulateKeyEvents(" ");
        view.Step();
        c.That(view.Wall.Launched, "Space launches");
        win.RenderFrame();
        c.That(win.LastFrameOps > 40, "the wall drew " + win.LastFrameOps + " ops");
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
            var c = BataApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = BataApp.Build();
        app.Run(w);
        return 0;
    }
}
