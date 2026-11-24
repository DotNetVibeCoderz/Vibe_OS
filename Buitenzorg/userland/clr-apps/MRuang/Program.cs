// Ruang 3D -- the G3 demo: a small garden scene drawn by the kernel's 3D
// rasterizer through Buitenzorg.Three (scene graph) and Buitenzorg.Graphics3D.
// A textured cube, a glossy sphere, a gold torus, a cone, a glass block, a
// lantern (point light) circling them, a sun, a grid floor and fog. Drag with
// the mouse or use the arrow keys to orbit, W/S to zoom, Space to pause,
// R for wireframe, K for fog, J to drop a fruit (rigid-body physics, with a
// thud placed in 3D space where it lands).

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
    /// <summary>Fruit dropped with J: physics against the floor and the crate, sound where they land.</summary>
    public readonly PhysicsWorld Physics = new PhysicsWorld();
    public readonly Audio3D Sound = new Audio3D();
    public readonly List<RigidBody> Fruit = new List<RigidBody>();
    public int Thuds;
    static readonly short[] Thud = Audio3D.Tone(180, 120);
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
        Physics.Planes.Add(Plane.Ground(-1));
        Physics.Add(RigidBody.Box(Cube, new Vector3(0.65f, 0.65f, 0.65f), 0, isStatic: true));
        Physics.Collided += (a, b, speed) =>
        {
            Thuds++;
            Sound.Play(Thud, 22050, a.Position, volume: Math.Min(1f, speed / 4));
        };
        Sound.Listener = Camera;
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

    /// <summary>Drop a fruit from above (x, z); at most 8 (the oldest is removed).</summary>
    public RigidBody DropFruit(float x, float z)
    {
        if (Fruit.Count >= 8)
        {
            var old = Fruit[0];
            Fruit.RemoveAt(0);
            Physics.Bodies.Remove(old);
            old.Node.Remove();
        }
        int[] colors = { 0xF2A03D, 0xE3615A, 0xF2D84B, 0x8BC34A };
        var n = Scene.AddMesh(Geometry.Sphere(0.3f, 12, 8), Material.Phong(colors[Fruit.Count % 4], 40, 0.6f), null, "buah");
        n.Position = new Vector3(x, 3.5f, z);
        var b = Physics.Add(RigidBody.Sphere(n, 0.3f));
        b.Restitution = 0.5f;
        Fruit.Add(b);
        return b;
    }

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
        if (dt > 0 && Fruit.Count > 0)
        {
            Physics.Step(Math.Min(dt, 0.05f));
            Sound.Update();
        }
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

    protected override bool OnRelativeMotion(int dx, int dy)
    {
        Garden.Azimuth -= dx * 0.005f;
        Garden.Elevation = Math.Clamp(Garden.Elevation + dy * 0.005f, -1.2f, 1.4f);
        Garden.PlaceCamera();
        InvalidateVisual();
        return true;
    }

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
            case 'm':
                // mouse look: the cursor is held in the window, motion turns the camera
                if (Window is Window win) win.SetPointerLock(!win.PointerLocked);
                break;
            case (char)27:
                if (Window is Window w2 && w2.PointerLocked) w2.SetPointerLock(false); else used = false;
                break;
            case 'j': g.DropFruit(-2.4f + 0.3f * (g.Fruit.Count % 3 - 1), 0.2f * (g.Fruit.Count % 2)); break;
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

    /// <summary>Off the device: the model from the source tree (the simulated VFS has no /disk files).</summary>
    static byte[]? RepoModel()
    {
        try
        {
            string? d = AppContext.BaseDirectory;
            for (int i = 0; i < 8 && d != null; i++, d = System.IO.Path.GetDirectoryName(d))
            {
                string f = System.IO.Path.Combine(d, "assets", "models", "pohon.glb");
                if (System.IO.File.Exists(f)) return System.IO.File.ReadAllBytes(f);
            }
        }
        catch (Exception) { }
        return null;
    }

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

        // mouse look: M locks the pointer, relative motion turns the camera, Esc releases
        w.DispatchKey('m');
        float az = g.Azimuth;
        w.DispatchRelative(40, 0);
        bool turned = MathF.Abs(g.Azimuth - az) > 0.1f;
        w.DispatchKey((char)27);
        c.That(turned && !w.PointerLocked, "M = mouse look: relative motion turns the camera, Esc releases it");

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

        // physics: a fruit dropped onto the crate bounces off it and comes to rest on the floor
        var ga = new Garden();
        ga.Cube.EulerAngles = Vector3.Zero;
        var f1 = ga.DropFruit(-2.4f, 0.4f);
        var f2 = ga.DropFruit(1.5f, 0);
        float lowest = 99;
        bool hitCrate = false;
        ga.Physics.SubSteps = 3;
        for (int i = 0; i < 100; i++) // 3.3 s of simulated time, cheap enough for slow emulators
        {
            ga.Physics.Step(1 / 30f);
            lowest = MathF.Min(lowest, f2.Position.Y);
            if (f1.Contacts > 0 && f1.Position.Y > 0) hitCrate = true;
        }
        c.That(hitCrate && MathF.Abs(f2.Position.Y - (-0.7f)) < 0.02f && lowest > -0.75f && f2.Sleeping,
            "physics: fruit lands on the crate, another rests on the floor (y " + f2.Position.Y.ToString("0.00") + ", asleep " + f2.Sleeping + ")");
        c.That(ga.Thuds >= 2 && ga.Sound.Playing >= 1, "physics: landings make positioned sounds (" + ga.Thuds + " thuds)");
        // 3D audio: a sound to the camera's right is louder on the right, a far one quieter
        ga.PlaceCamera();
        ga.Scene.UpdateWorldTransforms();
        var right = ga.Camera.WorldPosition + ga.Camera.WorldMatrix.TransformDirection(new Vector3(3, 0, 0));
        var (l1, r1) = ga.Sound.Gains(right);
        var (l2, r2) = ga.Sound.Gains(ga.Camera.WorldPosition + ga.Camera.WorldForward * 30);
        c.That(r1 > l1 * 2 && MathF.Abs(l2 - r2) < 0.05f && l2 + r2 < (l1 + r1) / 3, "3D audio: panned to the right, quieter far away (" + l1.ToString("0.00") + "/" + r1.ToString("0.00") + ")");

        // a glTF model (/disk/POHON.GLB): node tree, quaternion, vertex colors, materials
        var glb = Buitenzorg.App.Files.ReadAllBytes("/disk/POHON.GLB") ?? RepoModel();
        var tree = glb == null ? null : Gltf.Load(glb);
        var pohon = tree != null && tree.Children.Count == 1 ? tree.Children[0] : null;
        Node? batang = null, tajuk = null, kolam = null;
        if (pohon != null) foreach (var ch in pohon.Children) { if (ch.Name == "batang") batang = ch; else if (ch.Name == "tajuk") tajuk = ch; else if (ch.Name == "kolam") kolam = ch; }
        c.That(pohon != null && batang?.Geometry != null && batang.Geometry.TriangleCount == 12 && batang.Geometry.Colors != null
            && tajuk != null && tajuk.Children.Count == 2 && tajuk.Children[0].Geometry?.TriangleCount == 16,
            "glTF: nodes, meshes, two primitives, vertex colors (" + (glb == null ? "no file" : Gltf.LastError) + ")");
        c.That(tajuk != null && MathF.Abs(tajuk.EulerAngles.Y - 20 * MathF.PI / 180) < 0.01f && kolam?.Material != null && kolam.Material.Transparent
            && MathF.Abs(kolam.Material.Opacity - 0.6f) < 0.01f && kolam.Material.DoubleSided, "glTF: rotation quaternion, translucent double-sided material");
        if (pohon != null)
        {
            pohon.Position = new Vector3(2.5f, -0.6f, -1.5f);
            g.Scene.Root.Add(pohon);
            int withTree = view.RenderScene().DrawCalls;
            var tw = tajuk!.WorldPosition;
            c.That(withTree == 11 && MathF.Abs(tw.Y - (-0.6f + 1.08f)) < 0.01f, "glTF: the model draws (" + withTree + " objects), world transform (y " + tw.Y.ToString("0.00") + ")");
            pohon.Remove();
        }
        SkeletalChecks(c);
        r.Dispose();
        return c;
    }

    /// <summary>Skeletal animation: a two-joint stem bent by hand, a sampled clip, a skinned glTF with an animation.</summary>
    static void SkeletalChecks(Checks c)
    {
        try
        {
            var sc = new Scene();
            var a = sc.CreateNode(null, "pangkal");
            a.Position = new Vector3(0, -1, 0);
            var bj = sc.CreateNode(a, "ruas");
            bj.Position = new Vector3(0, 1, 0);
            var stem = sc.AddMesh(Geometry.Cylinder(0.1f, 0.1f, 2, 8, false), Material.Lambert(0x6FAF5F), null, "batang");
            var skin = Skin.BindPose(sc, new[] { a, bj });
            int nv = stem.Geometry!.VertexCount;
            var joints = new int[nv * 4];
            var weights = new float[nv * 4];
            for (int i = 0; i < nv; i++)
            {
                joints[i * 4] = stem.Geometry.Vertices[i * 8 + 1] > 0 ? 1 : 0;
                weights[i * 4] = 1;
            }
            var sm = new SkinnedMesh(stem, skin, joints, weights);
            bj.EulerAngles = new Vector3(0, 0, MathF.PI / 2);
            sc.UpdateWorldTransforms();
            sc.ApplySkins();
            var tip = sm.VertexPosition(0); // (0, 1, 0.1) in the bind pose
            var foot = sm.VertexPosition(1); // (0, -1, 0.1)
            c.That(Near(tip, new Vector3(-1, 0, 0.1f)) && Near(foot, new Vector3(0, -1, 0.1f)) && sm.Updates == 1,
                "skinning: the upper joint turned 90 degrees bends the tip to (-1, 0) (" + V(tip) + "), the foot stays");

            var clip = new AnimationClip("goyang");
            clip.Channels.Add(AnimationChannel.RotationEuler(bj, new float[] { 0, 1 }, new[] { Vector3.Zero, new Vector3(0, 0, MathF.PI / 2) }));
            var anim = new Animator(clip) { Loop = false }.AttachTo(sc.Root);
            sc.Update(0.5f);
            bool half = MathF.Abs(bj.EulerAngles.Z - MathF.PI / 4) < 0.01f;
            sc.Update(0.75f);
            c.That(half && !anim.Playing && MathF.Abs(bj.EulerAngles.Z - MathF.PI / 2) < 0.01f,
                "animation: slerp halfway = 45 degrees, a clip without Loop stops at its end");

            var model = Gltf.Load(SkinnedGltf());
            Node? mesh = null;
            model?.Traverse(n => { if (n.Skinned != null) mesh = n; });
            var clips = Gltf.LastAnimations;
            if (mesh != null && clips.Count == 1)
            {
                var gs = new Scene();
                gs.Root.Add(model!);
                new Animator(clips[0]) { Loop = false }.AttachTo(gs.Root);
                gs.Update(1);
                gs.UpdateWorldTransforms();
                gs.ApplySkins();
            }
            var moved = mesh?.Skinned?.VertexPosition(2) ?? new Vector3(99, 99, 99);
            c.That(mesh != null && clips.Count == 1 && clips[0].Channels.Count == 1 && Near(moved, new Vector3(-1, 1, 0)) && Near(mesh.Skinned!.VertexPosition(1), new Vector3(1, 0, 0)),
                "glTF: skin (JOINTS_0, WEIGHTS_0, inverse bind matrices) + rotation animation (" + (mesh == null ? Gltf.LastError : V(moved)) + ")");
        }
        catch (Exception e) { c.That(false, "skeletal: " + e.Message); }
    }

    static bool Near(Vector3 p, Vector3 q) => MathF.Abs(p.X - q.X) < 0.01f && MathF.Abs(p.Y - q.Y) < 0.01f && MathF.Abs(p.Z - q.Z) < 0.01f;
    static string V(Vector3 p) => p.X.ToString("0.00") + "," + p.Y.ToString("0.00") + "," + p.Z.ToString("0.00");

    /// <summary>A three-vertex skinned glTF: joint 1 at (0, 1, 0) owns vertex (0, 2, 0) and turns 90 degrees about Z in one second.</summary>
    static byte[] SkinnedGltf()
    {
        var buf = new List<byte>();
        void F(float f) { int b = Bits(f); buf.Add((byte)b); buf.Add((byte)(b >> 8)); buf.Add((byte)(b >> 16)); buf.Add((byte)(b >> 24)); }
        foreach (var f in new float[] { 0, 0, 0, 1, 0, 0, 0, 2, 0 }) F(f);              // 0: positions, 36 bytes
        foreach (var j in new byte[] { 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0 }) buf.Add(j); // 36: joints, 12
        for (int v = 0; v < 3; v++) { F(1); F(0); F(0); F(0); }                          // 48: weights, 48
        foreach (var f in new float[] { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1 }) F(f);  // 96: inverse bind 0
        foreach (var f in new float[] { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, -1, 0, 1 }) F(f); // 160: inverse bind 1 (column-major)
        F(0); F(1);                                                                      // 224: key times
        float h = 0.70710678f;
        foreach (var f in new float[] { 0, 0, 0, 1, 0, 0, h, h }) F(f);                  // 232: rotations, 32 bytes
        string data = "data:application/octet-stream;base64," + Base64(buf.ToArray());
        string json = "{\"asset\":{\"version\":\"2.0\"},\"scene\":0,\"scenes\":[{\"nodes\":[0,1]}]," +
            "\"nodes\":[{\"name\":\"kulit\",\"mesh\":0,\"skin\":0},{\"name\":\"akar\",\"children\":[2]},{\"name\":\"ujung\",\"translation\":[0,1,0]}]," +
            "\"meshes\":[{\"primitives\":[{\"attributes\":{\"POSITION\":0,\"JOINTS_0\":1,\"WEIGHTS_0\":2}}]}]," +
            "\"skins\":[{\"joints\":[1,2],\"inverseBindMatrices\":3}]," +
            "\"animations\":[{\"name\":\"tekuk\",\"samplers\":[{\"input\":4,\"output\":5}],\"channels\":[{\"sampler\":0,\"target\":{\"node\":2,\"path\":\"rotation\"}}]}]," +
            "\"buffers\":[{\"byteLength\":264,\"uri\":\"" + data + "\"}]," +
            "\"bufferViews\":[{\"buffer\":0,\"byteOffset\":0,\"byteLength\":36},{\"buffer\":0,\"byteOffset\":36,\"byteLength\":12},{\"buffer\":0,\"byteOffset\":48,\"byteLength\":48}," +
            "{\"buffer\":0,\"byteOffset\":96,\"byteLength\":128},{\"buffer\":0,\"byteOffset\":224,\"byteLength\":8},{\"buffer\":0,\"byteOffset\":232,\"byteLength\":32}]," +
            "\"accessors\":[{\"bufferView\":0,\"componentType\":5126,\"count\":3,\"type\":\"VEC3\"},{\"bufferView\":1,\"componentType\":5121,\"count\":3,\"type\":\"VEC4\"}," +
            "{\"bufferView\":2,\"componentType\":5126,\"count\":3,\"type\":\"VEC4\"},{\"bufferView\":3,\"componentType\":5126,\"count\":2,\"type\":\"MAT4\"}," +
            "{\"bufferView\":4,\"componentType\":5126,\"count\":2,\"type\":\"SCALAR\"},{\"bufferView\":5,\"componentType\":5126,\"count\":2,\"type\":\"VEC4\"}]}";
        var bytes = new byte[json.Length];
        for (int i = 0; i < json.Length; i++) bytes[i] = (byte)json[i];
        return bytes;
    }

    /// <summary>IEEE 754 single bits of a normal float or zero (no BitConverter on every BCL).</summary>
    static int Bits(float f)
    {
        if (f == 0) return 0;
        int sign = f < 0 ? 1 : 0;
        double v = Math.Abs((double)f);
        int e = 0;
        while (v >= 2) { v /= 2; e++; }
        while (v < 1) { v *= 2; e--; }
        int mant = (int)Math.Round((v - 1) * 8388608);
        if (mant == 8388608) { mant = 0; e++; }
        return sign << 31 | (e + 127) << 23 | mant;
    }

    static string Base64(byte[] b)
    {
        const string t = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        var o = new char[(b.Length + 2) / 3 * 4];
        int k = 0;
        for (int i = 0; i < b.Length; i += 3)
        {
            int n = b[i] << 16 | (i + 1 < b.Length ? b[i + 1] << 8 : 0) | (i + 2 < b.Length ? b[i + 2] : 0);
            o[k++] = t[n >> 18 & 63]; o[k++] = t[n >> 12 & 63];
            o[k++] = i + 1 < b.Length ? t[n >> 6 & 63] : '=';
            o[k++] = i + 2 < b.Length ? t[n & 63] : '=';
        }
        return new string(o);
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
