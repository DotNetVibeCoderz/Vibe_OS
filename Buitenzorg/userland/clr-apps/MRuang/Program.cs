// Ruang 3D -- the G3 demo: a small garden scene drawn by the kernel's 3D
// rasterizer through Buitenzorg.Three (scene graph) and Buitenzorg.Graphics3D.
// A textured cube, a glossy sphere, a gold torus, a cone, a glass block, a
// lantern (point light) circling them, a sun, a grid floor and fog. Drag with
// the mouse or use the arrow keys to orbit, W/S to zoom, Space to pause,
// R for wireframe, K for fog.

using Buitenzorg.UI;
using Buitenzorg.Three;
using Vector3 = Buitenzorg.Graphics3D.Vector3;

namespace MRuang;

/// <summary>The garden: every object the demo animates.</summary>
public sealed class Garden
{
    public readonly Scene Scene = new Scene();
    public readonly Node Camera, Cube, Sphere, Torus, Cone, Glass, Lantern, Floor;
    public readonly List<Material> Solid = new List<Material>();
    public float Azimuth = 0.6f, Elevation = 0.42f, Distance = 7.5f, Time;
    public bool Fog = true;
    const int Forest = 0x0E1A14;

    public Garden()
    {
        var s = Scene;
        s.Background = Forest;
        s.AmbientColor = 0xCFE8DA;
        s.AmbientIntensity = 0.28f;
        s.FogColor = Forest;
        SetFog(true);

        var grid = Texture.Grid(64, 0x1F3A2B, 0x3C6B50, 8);
        var floorMat = Material.Lambert(0xFFFFFF);
        floorMat.Map = grid;
        Floor = s.AddMesh(Geometry.Plane(18, 18, 6, 9), floorMat, null, "lantai");
        Floor.EulerAngles = new Vector3(-MathF.PI / 2, 0, 0);
        Floor.Position = new Vector3(0, -1, 0);

        var crate = Material.Lambert(0xFFFFFF);
        crate.Map = Texture.Checker(32, 0xF2E6C9, 0x7CC4A4, 4);
        Cube = s.AddMesh(Geometry.Box(1.3f, 1.3f, 1.3f), crate, null, "kubus");
        Cube.Position = new Vector3(-2.4f, -0.2f, 0);

        Sphere = s.AddMesh(Geometry.Sphere(0.85f, 20, 14), Material.Phong(0xE3615A, 48, 0.8f), null, "bola");
        Sphere.Position = new Vector3(0, -0.12f, 0);

        Torus = s.AddMesh(Geometry.Torus(0.62f, 0.22f, 10, 28), Material.Phong(0xE9B44C, 32, 0.7f), null, "torus");
        Torus.Position = new Vector3(2.4f, 0, 0);

        Cone = s.AddMesh(Geometry.Cone(0.6f, 1.5f, 20), Material.Lambert(0x7FB2D9), null, "kerucut");
        Cone.Position = new Vector3(0, -0.25f, -2.6f);

        var glass = Material.Phong(0x9FD8F0, 64, 0.9f);
        glass.Transparent = true;
        glass.Opacity = 0.45f;
        Glass = s.AddMesh(Geometry.Box(1.1f, 1.6f, 0.25f), glass, null, "kaca");
        Glass.Position = new Vector3(0, -0.2f, 2.3f);

        foreach (var n in new[] { Cube, Sphere, Torus, Cone }) Solid.Add(n.Material!);
        Solid.Add(floorMat);

        // a lantern: a warm point light with a small glowing ball to show where it is
        Lantern = s.AddLight(Light.Point(0xFFD9A0, 1.3f, 9f), null, "lentera");
        s.AddMesh(Geometry.Sphere(0.12f, 8, 6), Material.Basic(0xFFE7B8), Lantern, "nyala");

        var sun = s.AddLight(Light.Directional(0xFFFFFF, 0.65f), null, "matahari");
        sun.Position = new Vector3(4, 7, 5);
        sun.LookAt(Vector3.Zero);

        Camera = s.AddCamera(Buitenzorg.Three.Camera.Perspective(0.9f, 0.1f, 60f), new Vector3(0, 2, 7));
        Step(0);
    }

    public void SetFog(bool on)
    {
        Fog = on;
        Scene.FogStart = on ? 6 : 0;
        Scene.FogEnd = on ? 17 : 0;
    }

    public void SetWireframe(bool on)
    {
        foreach (var m in Solid) m.Wireframe = on;
    }

    public bool Wireframe => Solid[0].Wireframe;

    /// <summary>Advance the animation by dt seconds and place the camera.</summary>
    public void Step(float dt)
    {
        Time += dt;
        float t = Time;
        Cube.EulerAngles = new Vector3(t * 0.5f, t * 0.8f, 0);
        Torus.EulerAngles = new Vector3(t * 0.9f, t * 0.4f, 0);
        Sphere.Position = new Vector3(0, -0.12f + 0.25f * MathF.Sin(t * 1.6f), 0);
        Lantern.Position = new Vector3(3.2f * MathF.Cos(t * 0.7f), 1.2f, 3.2f * MathF.Sin(t * 0.7f));
        PlaceCamera();
    }

    public void PlaceCamera()
    {
        if (Elevation < -0.1f) Elevation = -0.1f;
        if (Elevation > 1.35f) Elevation = 1.35f;
        if (Distance < 3) Distance = 3;
        if (Distance > 16) Distance = 16;
        float ce = MathF.Cos(Elevation);
        Camera.Position = new Vector3(Distance * ce * MathF.Sin(Azimuth), Distance * MathF.Sin(Elevation), Distance * ce * MathF.Cos(Azimuth));
        Camera.LookAt(new Vector3(0, -0.2f, 0));
    }
}

/// <summary>A control that shows a scene: renders it into a kernel render
/// context the size of the control and copies the picture into the window.</summary>
public sealed class Viewport3D : Control
{
    public readonly Garden Garden;
    Renderer? _r;
    int _lastX, _lastY;
    bool _drag;
    public RenderStats Stats;
    public int Frames;
    /// <summary>Name of the object last clicked (picking with a ray from the camera).</summary>
    public string Picked = "";

    public Viewport3D(Garden g) { Garden = g; Focusable = true; }

    public Renderer? Renderer => _r;

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(240, a.Width), Math.Max(180, a.Height));

    /// <summary>Render the scene at the control's size (also used headless by the self-test).</summary>
    public RenderStats RenderScene()
    {
        var b = Bounds;
        int w = Math.Max(16, Math.Min(1024, b.Width)), h = Math.Max(16, Math.Min(1024, b.Height));
        if (_r == null) _r = new Renderer(w, h);
        else _r.Resize(w, h);
        Stats = _r.Render(Garden.Scene, Garden.Camera);
        Frames++;
        return Stats;
    }

    public override void Render(DrawingContext dc)
    {
        RenderScene();
        dc.Present3D(_r!.Device, Bounds.X, Bounds.Y);
    }

    /// <summary>The object under view pixel (x, y), remembered in Picked.</summary>
    public string PickAt(int x, int y)
    {
        if (_r == null) RenderScene();
        var hit = _r!.Pick(Garden.Scene, Garden.Camera, x, y);
        Picked = hit == null ? "" : hit.Node.Name;
        return Picked;
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        PickAt(e.X - Bounds.X, e.Y - Bounds.Y);
        _drag = true; _lastX = e.X; _lastY = e.Y;
        e.Handled = true;
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        if (_drag && e.LeftButton)
        {
            Garden.Azimuth -= (e.X - _lastX) * 0.01f;
            Garden.Elevation += (e.Y - _lastY) * 0.01f;
            _lastX = e.X; _lastY = e.Y;
            Garden.PlaceCamera();
            InvalidateVisual();
            e.Handled = true;
        }
        base.OnPointerMoved(e);
    }

    protected override void OnPointerReleased(PointerEventArgs e) { _drag = false; base.OnPointerReleased(e); }

    protected override void OnPointerWheelChanged(PointerEventArgs e)
    {
        Garden.Distance -= e.WheelDelta * 0.6f;
        Garden.PlaceCamera();
        InvalidateVisual();
        e.Handled = true;
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        var g = Garden;
        bool used = true;
        switch (char.ToLower(e.Key))
        {
            case Keys.Left: g.Azimuth -= 0.15f; break;
            case Keys.Right: g.Azimuth += 0.15f; break;
            case Keys.Up: g.Elevation += 0.1f; break;
            case Keys.Down: g.Elevation -= 0.1f; break;
            case 'w': g.Distance -= 0.6f; break;
            case 's': g.Distance += 0.6f; break;
            default: used = false; break;
        }
        if (used)
        {
            g.PlaceCamera();
            InvalidateVisual();
            e.Handled = true;
        }
        base.OnKeyDown(e);
    }
}

public static class RuangApp
{
    public static (Window, Viewport3D, TextBlock, ToggleButton) Build(bool headless = false)
    {
        var w = new Window("Ruang 3D", 720, 540, headless);
        var garden = new Garden();
        var view = new Viewport3D(garden);
        var status = new TextBlock("") { Margin = new Thickness(0, 8, 0, 0) }.WithClass("faint");
        var play = new ToggleButton("Jeda") { IsChecked = true };
        var wire = new ToggleButton("Rangka");
        var fog = new ToggleButton("Kabut") { IsChecked = true };
        play.IsCheckedChanged += b => b.Content = b.IsChecked ? "Jeda" : "Putar";
        wire.IsCheckedChanged += b => { garden.SetWireframe(b.IsChecked); view.InvalidateVisual(); };
        fog.IsCheckedChanged += b => { garden.SetFog(b.IsChecked); view.InvalidateVisual(); };

        var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        bar.Children.Add(play); bar.Children.Add(wire); bar.Children.Add(fog);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Ruang 3D", IconKind.Grid, 0x7CC4A4, bar).DockTo(Dock.Top));
        root.Children.Add(status.DockTo(Dock.Bottom));
        root.Children.Add(view);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        view.Focus();

        void ShowStats() =>
            status.Text = view.Stats + (view.Picked.Length > 0 ? "  ·  dipilih: " + view.Picked : "") + "  ·  klik: pilih objek, seret / panah: putar kamera, W/S: dekat-jauh, Spasi: jeda, R: rangka, K: kabut";
        w.KeyDown += (_, e) =>
        {
            char k = char.ToLower(e.Key);
            if (k == ' ') { play.IsChecked = !play.IsChecked; e.Handled = true; }
            else if (k == 'r') { wire.IsChecked = !wire.IsChecked; e.Handled = true; }
            else if (k == 'k') { fog.IsChecked = !fog.IsChecked; e.Handled = true; }
        };
        if (!headless)
        {
            long last = Clock.Now;
            var timer = new DispatcherTimer(40);
            timer.Tick += () =>
            {
                long now = Clock.Now;
                float dt = Math.Min(200, now - last) / 1000f;
                last = now;
                if (play.IsChecked) { garden.Step(dt); view.InvalidateVisual(); }
                ShowStats();
            };
            timer.Start();
        }
        return (w, view, status, wire);
    }

    static int R(int c) => (c >> 16) & 0xFF;
    static int G(int c) => (c >> 8) & 0xFF;
    static int B(int c) => c & 0xFF;

    public static Checks SelfTest()
    {
        var c = new Checks("Ruang 3D");
        var (w, view, _, wire) = Build(headless: true);
        var g = view.Garden;
        var st = view.RenderScene();
        var r = view.Renderer!;
        bool sim = r.Device.IsSimulated;
        Console.WriteLine("Ruang 3D: " + r.Width + "x" + r.Height + " (" + r.Device.Backend + "): " + st);
        c.That(st.DrawCalls == 7, "7 objects drawn (" + st.DrawCalls + ")");
        c.That(st.Triangles > 300 && st.Pixels > r.Width * r.Height / 3, "triangles and pixels drawn (" + st + ")");
        c.That(sim || st.Lights == 2, "a sun and a lantern lit the scene (" + st.Lights + ")");

        // the sphere is red where it projects; the sky above the scene is the background
        var (sx, sy, vis) = r.Project(g.Scene, g.Sphere.WorldPosition, g.Camera);
        int sp = r.ReadPixel(sx, sy);
        c.That(vis && R(sp) > G(sp) + 30 && R(sp) > B(sp) + 30, "the sphere shows red at its centre (" + sx + "," + sy + " = " + sp.ToString("X6") + ")");
        c.That(view.PickAt(sx, sy) == "bola", "clicking the sphere picks it (" + view.Picked + ")");
        int sky = r.ReadPixel(r.Width / 2, 2);
        c.That(sky == g.Scene.Background, "the sky is the background (" + sky.ToString("X6") + ")");

        // depth: the cube in front of the torus? orbit so the cube hides the sphere
        g.Azimuth = -MathF.PI / 2; g.Elevation = 0.05f; g.Distance = 7; g.PlaceCamera();
        view.RenderScene();
        var (cx, cy, cv) = r.Project(g.Scene, g.Cube.WorldPosition, g.Camera);
        int cp = r.ReadPixel(cx, cy);
        c.That(cv && !(R(cp) > G(cp) + 60 && R(cp) > B(cp) + 60), "from the side the near cube hides the red sphere (" + cp.ToString("X6") + ")");

        // lighting: the sphere's lit side is brighter than its shadow side (kernel only: the stand-in is unlit)
        g.Azimuth = 0.6f; g.Elevation = 0.42f; g.Distance = 7.5f; g.PlaceCamera();
        view.RenderScene();
        if (!sim)
        {
            var top = r.Project(g.Scene, g.Sphere.WorldPosition + new Vector3(0, 0.6f, 0.3f), g.Camera);
            var bottom = r.Project(g.Scene, g.Sphere.WorldPosition + new Vector3(0, -0.6f, 0.3f), g.Camera);
            int lt = r.ReadPixel(top.x, top.y), lb = r.ReadPixel(bottom.x, bottom.y);
            c.That(R(lt) > R(lb) + 20, "sunlit top of the sphere is brighter than its underside (" + lt.ToString("X6") + " / " + lb.ToString("X6") + ")");
        }

        // wireframe draws fewer pixels than solid
        int solid = view.RenderScene().Pixels;
        wire.IsChecked = true;
        int lines = view.RenderScene().Pixels;
        wire.IsChecked = false;
        c.That(lines < solid, "wireframe draws edges only (" + lines + " < " + solid + ")");

        // keys orbit the camera; the picture changes
        var before = g.Camera.Position;
        view.Focus();
        w.DispatchKey(Keys.Left);
        w.DispatchKey('w');
        c.That(Vector3.Distance(before, g.Camera.Position) > 0.3f, "arrow and W keys move the camera");

        // the animation moves the lantern
        var l0 = g.Lantern.Position;
        g.Step(0.5f);
        c.That(Vector3.Distance(l0, g.Lantern.Position) > 0.5f, "the lantern circles the garden");

        // an OBJ model loads (a tetrahedron) and draws
        var tet = Geometry.FromObj("v 0 1 0\nv -1 -1 1\nv 1 -1 1\nv 0 -1 -1\nf 1 2 3\nf 1 3 4\nf 1 4 2\nf 2 4 3\n");
        c.That(tet.TriangleCount == 4 && tet.VertexCount == 4, "OBJ loader: 4 faces (" + tet.TriangleCount + ")");
        var obj = g.Scene.AddMesh(tet, Material.Basic(0x00FF00), null, "obj");
        obj.Position = new Vector3(0, 0.3f, 0);
        int before3 = view.RenderScene().DrawCalls;
        obj.Remove();
        c.That(before3 == 8, "the OBJ model is drawn as an eighth object (" + before3 + ")");
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
            var c = RuangApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _, _) = RuangApp.Build();
        app.Run(w);
        return 0;
    }
}
