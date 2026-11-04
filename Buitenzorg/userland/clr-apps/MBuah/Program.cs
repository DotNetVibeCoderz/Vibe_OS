// Panen Buah 3D -- the G3 game: catch the fruit falling into the garden with
// a basket, dodge the rocks. Arrow keys / A-D move the basket, or click the
// lawn (a raycast from the camera finds the spot); R starts again.
// Buitenzorg.Three draws it (on the GPU through virgl when there is one,
// otherwise the kernel's rasterizer); the rules live in `Harvest`, separate
// from the window, so the self-test can play a deterministic game.

using Buitenzorg.UI;
using Buitenzorg.Three;
using Vector3 = Buitenzorg.Graphics3D.Vector3;

namespace MBuah;

/// <summary>The game: a basket on a lawn, things falling into it.</summary>
public sealed class Harvest
{
    public const float Half = 4.2f;      // the basket moves in -Half..Half
    public const float CatchRadius = 0.85f;
    public const int StartLives = 3;

    public readonly Scene Scene = new Scene();
    public readonly Node Camera, Basket, Lawn;
    readonly Node _drops;
    readonly Geometry _fruitGeo, _rockGeo;
    readonly Material[] _fruitMats;
    readonly Material _rockMat;
    uint _seed;
    float _spawnIn;

    public int Score, Lives, Caught, Missed;
    public bool Over;
    public float Speed = 2.2f;           // fall speed, grows with the score
    public float BasketVelocity;         // set by the keys (units / s)
    /// <summary>Drop things on a timer (off in the deterministic self-test).</summary>
    public bool AutoSpawn = true;
    public int Drops => _drops.Children.Count;

    sealed class Drop { public bool Rock; public float Fall; }

    public Harvest(uint seed = 12345)
    {
        _seed = seed;
        var s = Scene;
        s.Background = 0x9FD3E8;
        s.FogColor = 0x9FD3E8;
        s.FogStart = 14; s.FogEnd = 30;
        s.AmbientColor = 0xFFFFFF; s.AmbientIntensity = 0.45f;
        var sun = s.AddLight(Light.Directional(0xFFF4DD, 0.85f), null, "matahari");
        sun.Position = new Vector3(-3, 8, 6);
        sun.LookAt(Vector3.Zero);

        var grass = Material.Lambert(0xFFFFFF);
        grass.Map = Texture.Checker(32, 0x5FA05A, 0x6DB066, 8);
        Lawn = s.AddMesh(Geometry.Plane(14, 14, 4, 6), grass, null, "rumput");
        Lawn.EulerAngles = new Vector3(-MathF.PI / 2, 0, 0);

        // a row of trees at the back
        var trunk = Material.Lambert(0x8B5A2B);
        var leaves = Material.Lambert(0x2E7D4F);
        for (int i = -3; i <= 3; i++)
        {
            var tree = s.CreateNode(null, "pohon");
            tree.Position = new Vector3(i * 2.1f, 0, -5.5f + (i & 1) * 0.6f);
            s.AddMesh(Geometry.Cylinder(0.15f, 0.2f, 1.2f, 8), trunk, tree).Position = new Vector3(0, 0.6f, 0);
            s.AddMesh(Geometry.Cone(0.9f, 2.2f, 10), leaves, tree).Position = new Vector3(0, 2.2f, 0);
            foreach (var c in tree.Children) c.Pickable = false;
        }

        Basket = s.AddMesh(Geometry.Cylinder(0.75f, 0.55f, 0.6f, 16), Material.Phong(0xC8873E, 16, 0.3f), null, "keranjang");
        Basket.Position = new Vector3(0, 0.3f, 2.2f);

        _drops = s.CreateNode(null, "jatuhan");
        _fruitGeo = Geometry.Sphere(0.35f, 12, 8);
        _rockGeo = Geometry.Box(0.55f, 0.45f, 0.5f);
        _fruitMats = new[] { Material.Phong(0xE0352B, 40, 0.7f), Material.Phong(0xF29A1E, 40, 0.6f), Material.Phong(0xF2D22E, 40, 0.6f) };
        _rockMat = Material.Lambert(0x6B6F73);

        Camera = s.AddCamera(Buitenzorg.Three.Camera.Perspective(0.95f, 0.1f, 60f), new Vector3(0, 5.2f, 9.5f));
        Camera.LookAt(new Vector3(0, 0.8f, 0));
        Restart();
    }

    float Rand() { _seed = _seed * 1664525 + 1013904223; return (_seed >> 8) / 16777216f; }

    public void Restart()
    {
        foreach (var d in _drops.Children.ToArray()) d.Remove();
        Score = 0; Caught = 0; Missed = 0; Lives = StartLives; Over = false; Speed = 2.2f;
        Basket.Position = new Vector3(0, 0.3f, 2.2f);
        BasketVelocity = 0;
        _spawnIn = 0.3f;
    }

    /// <summary>Drop a fruit (or a rock) at x.</summary>
    public Node Spawn(float x, bool rock)
    {
        var n = Scene.AddMesh(rock ? _rockGeo : _fruitGeo, rock ? _rockMat : _fruitMats[(int)(Rand() * 3) % 3], _drops, rock ? "batu" : "buah");
        n.Position = new Vector3(x, 6.5f, Basket.Position.Z);
        n.Tag = new Drop { Rock = rock };
        n.Spin(rock ? 1.5f : 2.5f);
        return n;
    }

    public void MoveBasketTo(float x) =>
        Basket.Position = new Vector3(Math.Clamp(x, -Half, Half), Basket.Position.Y, Basket.Position.Z);

    /// <summary>Advance the game by dt seconds.</summary>
    public void Step(float dt)
    {
        if (Over) return;
        if (BasketVelocity != 0) MoveBasketTo(Basket.Position.X + BasketVelocity * dt);
        _spawnIn -= dt;
        if (AutoSpawn && _spawnIn <= 0)
        {
            Spawn((Rand() * 2 - 1) * Half, Rand() < 0.25f);
            _spawnIn = 1.1f - Math.Min(0.6f, Score * 0.03f);
        }
        Scene.Update(dt); // spinning
        foreach (var n in _drops.Children.ToArray())
        {
            var d = (Drop)n.Tag!;
            d.Fall += dt;
            var p = n.Position;
            n.Position = new Vector3(p.X, p.Y - (Speed + d.Fall * 1.5f) * dt, p.Z);
            if (n.Position.Y <= 0.75f && n.Position.Y > 0.1f && MathF.Abs(n.Position.X - Basket.Position.X) < CatchRadius)
            {
                n.Remove();
                if (d.Rock) LoseLife(); else { Score++; Caught++; Speed = 2.2f + Score * 0.08f; }
            }
            else if (n.Position.Y <= -0.4f)
            {
                n.Remove();
                if (!d.Rock) { Missed++; LoseLife(); }
            }
        }
    }

    void LoseLife()
    {
        Lives--;
        if (Lives <= 0) { Lives = 0; Over = true; }
    }

    public string Status => Over
        ? "Panen selesai! Skor " + Score + "  ·  tekan R untuk main lagi"
        : "Skor " + Score + "   Nyawa " + new string('*', Lives) + "   ·  panah / A D / klik rumput: geser keranjang";
}

/// <summary>Shows the game: renders the scene at the control size, keys and clicks steer the basket.</summary>
public sealed class GameView : Control
{
    public readonly Harvest Game;
    Renderer? _r;
    public RenderStats Stats;

    public GameView(Harvest g) { Game = g; Focusable = true; }
    public Renderer? Renderer => _r;

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(240, a.Width), Math.Max(180, a.Height));

    public RenderStats RenderScene()
    {
        var b = Bounds;
        int w = Math.Max(16, Math.Min(1024, b.Width)), h = Math.Max(16, Math.Min(1024, b.Height));
        if (_r == null) _r = new Renderer(w, h); else _r.Resize(w, h);
        Stats = _r.Render(Game.Scene, Game.Camera);
        return Stats;
    }

    public override void Render(DrawingContext dc)
    {
        RenderScene();
        dc.Present3D(_r!.Device, Bounds.X, Bounds.Y);
    }

    /// <summary>Click on the lawn: move the basket there (picking with a ray from the camera).</summary>
    public bool ClickAt(int x, int y)
    {
        if (_r == null) RenderScene();
        var hit = _r!.Pick(Game.Scene, Game.Camera, x, y);
        if (hit == null) return false;
        Game.MoveBasketTo(hit.Point.X);
        return true;
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        ClickAt(e.X - Bounds.X, e.Y - Bounds.Y);
        e.Handled = true;
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        char k = char.ToLower(e.Key);
        if (k == Keys.Left || k == 'a') { Game.BasketVelocity = -7; e.Handled = true; }
        else if (k == Keys.Right || k == 'd') { Game.BasketVelocity = 7; e.Handled = true; }
        else if (k == 'r') { Game.Restart(); e.Handled = true; }
        else if (k == ' ' || k == Keys.Down) { Game.BasketVelocity = 0; e.Handled = true; }
        base.OnKeyDown(e);
    }
}

public static class BuahApp
{
    public static (Window, GameView, TextBlock) Build(bool headless = false)
    {
        var w = new Window("Panen Buah 3D", 720, 540, headless);
        var game = new Harvest((uint)(headless ? 12345 : Environment.TickCount64));
        var view = new GameView(game);
        var status = new TextBlock(game.Status) { Margin = new Thickness(0, 8, 0, 0) }.WithClass("faint");
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Permainan", "Panen Buah 3D", IconKind.Leaf, 0xE0352B).DockTo(Dock.Top));
        root.Children.Add(status.DockTo(Dock.Bottom));
        root.Children.Add(view);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        view.Focus();
        if (!headless)
        {
            long last = Clock.Now;
            var timer = new DispatcherTimer(40);
            timer.Tick += () =>
            {
                long now = Clock.Now;
                float dt = Math.Min(150, now - last) / 1000f;
                last = now;
                // the keys give a short push: the basket glides and stops
                game.Step(dt);
                game.BasketVelocity *= 0.6f;
                if (MathF.Abs(game.BasketVelocity) < 0.3f) game.BasketVelocity = 0;
                status.Text = game.Status + "   (" + view.Stats.Milliseconds + " ms/frame)";
                view.InvalidateVisual();
            };
            timer.Start();
        }
        return (w, view, status);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Panen Buah");
        var (w, view, _) = Build(headless: true);
        var g = view.Game;
        g.AutoSpawn = false;

        // a fruit straight above the basket is caught
        g.Spawn(g.Basket.Position.X, false);
        for (int i = 0; i < 200 && g.Drops > 0; i++) g.Step(0.02f);
        c.That(g.Score == 1 && g.Lives == Harvest.StartLives, "a fruit above the basket is caught (score " + g.Score + ")");

        // a rock caught costs a life; a fruit far away is missed and costs one too
        g.Restart();
        g.Spawn(g.Basket.Position.X, true);
        for (int i = 0; i < 200 && g.Drops > 0; i++) g.Step(0.02f);
        c.That(g.Lives == Harvest.StartLives - 1 && g.Score == 0, "catching a rock costs a life (" + g.Lives + ")");
        g.Spawn(g.Basket.Position.X + 3, false);
        for (int i = 0; i < 300 && g.Drops > 0; i++) g.Step(0.02f);
        c.That(g.Missed == 1 && g.Lives == Harvest.StartLives - 2, "a missed fruit costs a life (" + g.Lives + ")");

        // the keys move the basket
        g.Restart();
        float x0 = g.Basket.Position.X;
        view.Focus();
        w.DispatchKey(Keys.Right);
        g.Step(0.1f);
        c.That(g.Basket.Position.X > x0 + 0.3f, "the right arrow moves the basket right");

        // picking: a click on the lawn left of the basket moves it there
        g.Restart();
        var st = view.RenderScene();
        var r = view.Renderer!;
        c.That(st.DrawCalls >= 16 && st.Triangles > 100, "the garden is drawn (" + st + ", " + r.Device.Backend + ")");
        var (px, py, vis) = r.Project(g.Scene, new Vector3(-2.5f, 0, 2.2f), g.Camera);
        c.That(vis && view.ClickAt(px, py) && MathF.Abs(g.Basket.Position.X + 2.5f) < 0.3f, "a click on the lawn puts the basket there (x " + g.Basket.Position.X + ")");
        var (bx, by, _) = r.Project(g.Scene, g.Basket.Position, g.Camera);
        var hit = r.Pick(g.Scene, g.Camera, bx, by);
        c.That(hit != null && hit.Node == g.Basket, "picking the basket's pixel hits the basket (" + (hit?.Node.Name ?? "nothing") + ")");

        // the game ends after the last life, R starts again
        g.Restart();
        for (int i = 0; i < Harvest.StartLives; i++)
        {
            g.Spawn(g.Basket.Position.X, true);
            for (int k = 0; k < 200 && g.Drops > 0; k++) g.Step(0.02f);
        }
        c.That(g.Over, "three rocks end the game");
        w.DispatchKey('r');
        c.That(!g.Over && g.Lives == Harvest.StartLives, "R starts a new game");

        // the spawner keeps things coming
        g.Restart();
        g.AutoSpawn = true;
        for (int i = 0; i < 100; i++) g.Step(0.05f);
        c.That(g.Caught + g.Missed + g.Drops + (Harvest.StartLives - g.Lives) > 2, "things keep falling");
        r.Dispose();
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
            var c = BuahApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = BuahApp.Build();
        app.Run(w);
        return 0;
    }
}
