// Aplikasi Managed -- the folder of managed C# apps (RustCLR + Buitenzorg.UI),
// opened from the desktop. Tiles in a WrapPanel through an ItemsControl
// template, a search box that filters them, launching through PROC_SPAWN.
// `--selftest` is the boot check: a real window rendered with the toolkit's
// draw ops, plus every app's own self-test in this one process.

using Buitenzorg.UI;
using Programs = Buitenzorg.App.Programs;

namespace Managed;

public sealed class AppInfo
{
    public string Launch { get; }
    public string Name { get; }
    public string Blurb { get; }
    public IconKind Glyph { get; }
    public int Tint { get; }
    public AppInfo(string launch, string name, string blurb, IconKind glyph, int tint) { Launch = launch; Name = name; Blurb = blurb; Glyph = glyph; Tint = tint; }
    public override string ToString() => Name;
}

public static class Folder
{
    public static readonly AppInfo[] Apps =
    {
        new AppInfo("mcalc", "Kalkulator", "Hitung cepat, dengan riwayat", IconKind.Grid, 0xE9B44C),
        new AppInfo("m2048", "2048", "Gabungkan ubin sampai berbunga", IconKind.Grid, 0xF0844B),
        new AppInfo("mnotes", "Catatan", "Tulis dan simpan ke /disk", IconKind.File, 0x7CC4A4),
        new AppInfo("mstudio", "Studio", "IDE: proyek, editor kode, jalankan (F5)", IconKind.File, 0x9CC9D9),
        new AppInfo("mfiles", "Berkas", "Jelajahi media dan program", IconKind.Folder, 0x7FB2D9),
        new AppInfo("mtasks", "Pemantau", "Proses, memori, akhiri tugas", IconKind.Cpu, 0x8FA8E8),
        new AppInfo("mxox", "XOX", "Lawan komputer yang tak terkalahkan", IconKind.Close, 0x62AD7C),
        new AppInfo("mclock", "Jam", "Jam, stopwatch, timer, kalender", IconKind.Clock, 0xD3E07A),
        new AppInfo("mimage", "Gambar", "Wallpaper dan foto di /disk", IconKind.Sun, 0xC792EA),
        new AppInfo("mpaint", "Paint", "Menggambar dengan mouse", IconKind.Leaf, 0xE3A6C9),
        new AppInfo("mbuah", "Panen Buah 3D", "Tangkap buah yang jatuh, hindari batu", IconKind.Leaf, 0xE0352B),
        new AppInfo("mular", "Ular", "Makan buah, tumbuh panjang, jangan menabrak", IconKind.Leaf, 0x5FB85A),
        new AppInfo("mranjau", "Ranjau", "Buka kebun tanpa menggali ranjau", IconKind.Grid, 0xE0563B),
        new AppInfo("mbata", "Bata", "Pantulkan biji, runtuhkan tembok bata", IconKind.Grid, 0xE9A64C),
        new AppInfo("mbalok", "Balok", "Susun peti jatuh, panen baris penuh", IconKind.Grid, 0x4FA3E0),
        new AppInfo("mkartu", "Kartu", "Solitaire: pulangkan semua kartu", IconKind.File, 0xC8352B),
        new AppInfo("mruang", "Ruang 3D", "Taman 3D: orbit kamera, lampu, kabut", IconKind.Grid, 0x7CC4A4),
        new AppInfo("mpiano", "Piano", "Satu oktaf, mainkan dengan keyboard", IconKind.Play, 0xB99AE8),
        new AppInfo("mgallery", "Galeri Kontrol", "Semua kontrol Buitenzorg.UI", IconKind.Info, 0xC7A6E0),
        new AppInfo("msetting", "Pengaturan", "Tema, wallpaper, screensaver, suara", IconKind.Gear, 0xD9B38C),
        new AppInfo("mwaktu", "Tanggal & Waktu", "Atur waktu, zona, bahasa dan format", IconKind.Clock, 0xD3E07A),
        new AppInfo("mhwlab", "Lab Perangkat", "Sensor, LED, GPIO, I2C, SPI (Raspberry Pi)", IconKind.Cpu, 0x9BD08F),
        new AppInfo("hellogui", "Jam Analog", "Demo GUI pertama RustCLR", IconKind.Sun, 0xE3615A),
    };

    /// <summary>One tile: glyph, name, one line about it.</summary>
    static Control Tile(AppInfo a, Action<AppInfo> open)
    {
        var s = new StackPanel { Spacing = 8 };
        s.Children.Add(new GlyphTile(a.Glyph, a.Tint, 44) { HorizontalAlignment = HorizontalAlignment.Left });
        s.Children.Add(new TextBlock(a.Name).WithClass("strong"));
        s.Children.Add(new TextBlock(a.Blurb) { TextWrapping = TextWrapping.Wrap }.WithClass("faint"));
        var b = new Button(s, _ => open(a)) { Width = 162, Height = 128, Padding = new Thickness(12), HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top, Tag = a };
        b.Classes.Add("tile");
        return b.WithTip("Buka " + a.Name + " (" + a.Launch + ")");
    }

    public static (Window, ItemsControl, TextBox, List<string>) Build(bool headless = false)
    {
        var app = Application.Current;
        if (app != null)
        {
            // tiles: flat cards that lift on hover
            app.Styles.Add(new Style("Button.tile") { { Control.BackgroundProperty, Theme.Surface }, { Control.BackgroundBottomProperty, Theme.Surface },
                { Control.BorderBrushProperty, Theme.StrokeSoft }, { Control.CornerRadiusProperty, 14 } });
            app.Styles.Add(new Style("Button.tile:pointerover") { { Control.BackgroundProperty, Theme.SurfaceHi }, { Control.BorderBrushProperty, Theme.Accent },
                { Control.BoxShadowProperty, new BoxShadow(0, 4, 8, 0x000000, 90) } });
        }
        var w = new Window("Managed Apps", 560, 560, headless);
        var launched = new List<string>();
        var notes = headless ? null : new WindowNotificationManager(w);
        var shown = new ObservableCollection<AppInfo>(Folder.Apps);

        void Open(AppInfo a)
        {
            launched.Add(a.Launch);
            if (headless) return;
            try
            {
                Programs.Start(a.Launch);
                notes?.Show(new Notification("Membuka " + a.Name, "Dijalankan oleh RustCLR.", NotificationType.Information, 2500));
            }
            catch (Exception e) { notes?.Show(new Notification("Tidak bisa dibuka", e.Message, NotificationType.Error)); }
        }

        var tiles = new ItemsControl
        {
            ItemsPanel = () => new WrapPanel { ItemSpacing = 10, LineSpacing = 10 },
            ItemTemplate = o => Tile((AppInfo)o, Open),
        };
        tiles.ItemsSource = shown;

        var search = new TextBox { Watermark = "Cari aplikasi...", Width = 200 };
        search.TextChanged += t =>
        {
            string q = t.Text.Trim().ToLowerInvariant();
            var list = new List<AppInfo>();
            foreach (var a in Folder.Apps)
                if (q.Length == 0 || a.Name.ToLowerInvariant().Contains(q) || a.Blurb.ToLowerInvariant().Contains(q)) list.Add(a);
            shown.ResetWith(list);
        };

        var footer = new TextBlock("C# di atas RustCLR  ·  Buitenzorg.UI  ·  Tab + Enter untuk membuka") { Margin = new Thickness(0, 10, 0, 0) }.WithClass("faint");
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Folder", "Managed Apps", IconKind.Folder, 0x7CC4A4, search).DockTo(Dock.Top));
        root.Children.Add(footer.DockTo(Dock.Bottom));
        root.Children.Add(new ScrollViewer { Content = tiles });
        w.Content = root;
        app?.Apply(w);
        w.UpdateLayout();
        return (w, tiles, search, launched);
    }

    static Checks SelfTestFolder()
    {
        var c = new Checks("Folder");
        var (w, tiles, search, launched) = Build(headless: true);
        c.That(tiles.ItemCount == Apps.Length, "all " + Apps.Length + " apps shown");
        search.Focus();
        w.SimulateKeys("jam");
        c.That(tiles.ItemCount == 2, "search 'jam' leaves 2 tiles (" + tiles.ItemCount + ")");
        foreach (var ch in "jam") w.DispatchKey(Keys.Backspace);
        w.UpdateLayout();
        c.That(tiles.ItemCount == Apps.Length, "clearing the search brings all back");
        Control? first = null;
        foreach (var d in w.Descendants()) if (d is Button b && b.Classes.Contains("tile")) { first = b; break; }
        if (first != null) w.SimulateClick(first);
        c.That(launched.Count == 1 && launched[0] == "mcalc", "clicking a tile launches it");
        return c;
    }

    /// <summary>The boot check: a real window drawn with the new draw ops, then every app's self-test.</summary>
    public static int SelfTest(bool all)
    {
        int failed = 0;
        void Report(Checks c) { Console.WriteLine(c.ToString()); if (!c.Ok) failed++; }

        // 1. the toolkit on a real window: rounded rects, gradients, styled text, blends, a popup
        // a real window on the device; headless off it (no desktop to draw on)
        Window w;
        try { (w, _, _, _) = Build(); }
        catch (Exception) { (w, _, _, _) = Build(headless: true); }
        w.RenderFrame();
        var ui = new Checks("Buitenzorg.UI");
        Console.WriteLine("Buitenzorg.UI: window " + w.Id + (w.Id == 0 ? " (headless)" : ""));
        ui.That(w.LastFrameOps > 60, "frame drew " + w.LastFrameOps + " ops");
        Control? tile = null;
        foreach (var d in w.Descendants()) if (d is Button b && b.Classes.Contains("tile")) { tile = b; break; }
        if (tile != null) { w.SimulatePointer(1, tile.Bounds.Center.X, tile.Bounds.Center.Y); }
        ui.That(tile != null && tile.IsPointerOver, "hover styles a tile");
        Dialog.Show(w, "Uji", "Dialog di atas jendela nyata.", "OK", null, null);
        w.RenderFrame();
        ui.That(w.Popups.Count == 1, "modal dialog drawn over the window");
        ui.That(DrawingContext.Refused == 0, "kernel accepted every draw op (" + DrawingContext.Refused + " refused)");
        Report(ui);
        if (ui.Ok) Console.WriteLine("MILESTONE: MANAGED UI OK");

        // 2. the apps, headless, in this process. At boot only the ones that reach
        //    the device (VFS, process list) -- an interpreter under emulation
        //    is slow; `--selftest all` runs every app (and CI runs each on .NET).
        Report(SelfTestFolder());
        Report(MFiles.FilesApp.SelfTest());
        Report(MTasks.TasksApp.SelfTest());
        Report(MNotes.NotesApp.SelfTest());
        Report(MStudio.Studio.SelfTest()); // the IDE: templates, editor, Run through SHELL_RUN, Problems
        Report(MHwLab.LabApp.SelfTest()); // board I/O: a quick "no board" check off a Raspberry Pi
        Report(MSettings.SettingsApp.SelfTest()); // the SETTING syscall, end to end (restores every value)
        Report(MWaktu.WaktuApp.SelfTest()); // time zone / language / 12-24 h / setting the clock (restores them)
        Report(MRuang.RuangApp.SelfTest()); // G3: the scene graph through the kernel's 3D rasterizer (GFX3D)
        Report(MUlar.UlarApp.SelfTest()); // Buitenzorg.Game: the loop, raw key events, drawing
        if (all)
        {
            Report(MCalc.CalcApp.SelfTest());
            Report(M2048.GameApp.SelfTest());
            Report(MXox.XoxApp.SelfTest());
            Report(MClock.ClockApp.SelfTest());
            Report(MGallery.GalleryApp.SelfTest());
            Report(MImage.ImageApp.SelfTest());
            Report(MPaint.PaintApp.SelfTest());
            Report(MPiano.PianoApp.SelfTest());
            Report(MBuah.BuahApp.SelfTest());
            Report(MRanjau.RanjauApp.SelfTest());
            Report(MBata.BataApp.SelfTest());
            Report(MBalok.BalokApp.SelfTest());
            Report(MKartu.KartuApp.SelfTest());
        }
        if (failed == 0) Console.WriteLine("MILESTONE: MANAGED APPS OK");
        return failed;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0) return Folder.SelfTest(Array.IndexOf(args, "all") >= 0);
        var (w, _, _, _) = Folder.Build();
        app.Run(w);
        return 0;
    }
}
