// __APPNAME__ -- a 2D game made with bz new game-csharp: catch the falling
// leaves with the basket (arrow keys or A/D), miss three and the game is over.
//
// Buitenzorg.Game gives the loop: GameView calls Update() 60 times a second
// with the keys held (Input, from raw KEY_EVENTs) and Draw() when the window
// redraws; Sfx plays tones, Rng is a seeded random generator, HighScores keeps
// the best scores on /disk. The rules live in Catch (no UI), so --selftest
// can drive them without a window: `dotnet bin/.../__NAME83__.dll --selftest`.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace __NAME83__;

/// <summary>The rules: a basket at the bottom, leaves falling from the top.</summary>
public sealed class Catch
{
    public const int Width = 320, Height = 240, BasketW = 48;
    public double BasketX = Width / 2 - BasketW / 2;
    public readonly List<(double X, double Y)> Leaves = new();
    public int Score, Missed;
    public bool Over => Missed >= 3;
    readonly Rng _rng;
    double _spawn;

    public Catch(ulong seed) { _rng = new Rng(seed); }

    /// <summary>One step of `dt` seconds with the basket moving `dir` (-1, 0, 1).</summary>
    public int Step(double dt, int dir)
    {
        if (Over) return 0;
        BasketX = Math.Clamp(BasketX + dir * 220 * dt, 0, Width - BasketW);
        _spawn -= dt;
        if (_spawn <= 0)
        {
            Leaves.Add((_rng.Range(8, Width - 8), -8));
            _spawn = 0.9;
        }
        int caught = 0;
        for (int i = Leaves.Count - 1; i >= 0; i--)
        {
            var (x, y) = Leaves[i];
            y += (60 + Score * 4) * dt;
            if (y >= Height - 20 && y <= Height - 8 && x >= BasketX && x <= BasketX + BasketW)
            {
                Leaves.RemoveAt(i);
                Score++;
                caught++;
            }
            else if (y > Height)
            {
                Leaves.RemoveAt(i);
                Missed++;
            }
            else Leaves[i] = (x, y);
        }
        return caught;
    }
}

public sealed class CatchView : GameView
{
    public readonly Catch Game;
    public event Action? Changed;

    public CatchView(Catch game) : base(Catch.Width, Catch.Height) { Game = game; }

    protected override void Update()
    {
        int dir = Input.Axis(RawKey.Left, RawKey.Right, RawKey.A, RawKey.D);
        if (Game.Over)
        {
            if (Input.AnyPressed(RawKey.Enter, RawKey.Space)) { Game.Leaves.Clear(); Game.Score = 0; Game.Missed = 0; Changed?.Invoke(); }
            return;
        }
        int missed = Game.Missed;
        if (Game.Step(Dt, dir) > 0) { Sfx.Score(); Changed?.Invoke(); }
        if (Game.Missed != missed) { Sfx.Lose(); Changed?.Invoke(); }
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillRect(f, 0x12301F);
        foreach (var (x, y) in Game.Leaves)
            dc.Ellipse(SRect(x - 6, y - 6, 12, 12), 0x7CC4A4, true);
        dc.FillRoundRect(SRect(Game.BasketX, Catch.Height - 16, Catch.BasketW, 10), Math.Max(2, Sl(4)), 0xC98B4A);
        if (Game.Over) Banner(dc, f, "Selesai", "Skor " + Game.Score + "  -  Enter untuk main lagi");
    }
}

public static class Program
{
    public static (Window, CatchView) Build(bool headless = false)
    {
        var view = new CatchView(new Catch(headless ? 7UL : (ulong)Clock.Now | 1));
        var status = new TextBlock("Skor 0") { Margin = new Thickness(4, 0, 4, 8) };
        view.Changed += () => status.Text = "Skor " + view.Game.Score + "    Terlewat " + view.Game.Missed + " / 3    (panah atau A/D)";
        var w = GameHost.Build("__APPNAME__", view, 520, 440, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("__APPNAME__");
        var g = new Catch(1);
        g.Step(1.0, 0); // the first leaf appears
        c.That(g.Leaves.Count == 1, "a leaf falls");
        // put the basket under it and let it land
        g.BasketX = g.Leaves[0].X - Catch.BasketW / 2;
        int caught = 0;
        for (int i = 0; i < 400 && caught == 0; i++) caught += g.Step(1.0 / 60, 0);
        c.That(caught == 1 && g.Score == 1, "the basket catches it");
        for (int i = 0; i < 4000 && !g.Over; i++) { g.BasketX = 0; g.Step(1.0 / 60, 0); }
        c.That(g.Over, "three missed leaves end the game");
        var (w, view) = Build(headless: true);
        double x0 = view.Game.BasketX;
        w.DispatchKeyEvent(RawKey.Right, true);
        view.Step(20);
        w.DispatchKeyEvent(RawKey.Right, false);
        c.That(view.Game.BasketX > x0, "the Right key moves the basket");
        w.RenderFrame();
        c.That(w.LastFrameOps > 3, "a frame drew " + w.LastFrameOps + " ops");
        return c;
    }

    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = Build();
        app.Run(w);
        return 0;
    }
}
