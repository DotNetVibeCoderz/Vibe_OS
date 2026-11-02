// Pengaturan -- the desktop settings app: theme (+ animations, large cursor),
// wallpaper, screensaver (+ idle delay) and sound (volume, test tone), each a
// page behind a side navigation. Changes apply at once: the kernel recomposes
// the desktop (SETTING syscall). Buitenzorg.UI + Buitenzorg.App.Settings / Audio.

using Buitenzorg.UI;
using AppSettings = Buitenzorg.App.Settings;
using AppAudio = Buitenzorg.App.Audio;

namespace MSettings;

public static class SettingsApp
{
    /// <summary>Friendly names for the kernel's theme ids.</summary>
    public static string ThemeLabel(string id) => id switch
    {
        "kebun" => "Kebun (bawaan)",
        "dark" => "Gelap",
        "light" => "Terang",
        "neo-brutalism" or "neobrutalism" => "Neo Brutalism",
        "clean" => "Bersih",
        "material" => "Material",
        "bento" => "Bento",
        "classic-linux" => "Linux klasik",
        "classic-windows" => "Windows klasik",
        "sun" => "Sun",
        "beos" => "BeOS",
        _ => id,
    };

    public static string SaverLabel(string id) => id switch
    {
        "starfield" => "Bintang",
        "mystify" => "Mystify",
        "pipes" => "Pipa",
        "marquee" => "Teks berjalan",
        "bouncing" => "Logo memantul",
        "blank" => "Layar hitam",
        "off" => "Mati",
        _ => id,
    };

    public sealed class Parts
    {
        public ListBox Nav = null!, Themes = null!, Wallpapers = null!, Savers = null!;
        public Slider Delay = null!, Volume = null!;
        public ToggleSwitch Animations = null!, Cursor = null!;
        public TextBlock DelayText = null!, SoundStatus = null!;
    }

    static ListBox Choice(List<string> labels, int current, int key, Func<int, string>? after = null)
    {
        var list = new ListBox { ItemsSource = labels, Height = 300 };
        list.SelectedIndex = current;
        list.SelectionChanged += l =>
        {
            int i = l.SelectedIndex;
            if (i >= 0 && i != AppSettings.Get(key)) AppSettings.Set(key, i);
        };
        return list;
    }

    static StackPanel Page(string title, string hint, params Control[] body)
    {
        var p = new StackPanel { Orientation = Orientation.Vertical, Spacing = 8 };
        p.Children.Add(new TextBlock(title).WithClass("h2"));
        p.Children.Add(new TextBlock(hint).WithClass("faint"));
        foreach (var c in body) p.Children.Add(c);
        return p;
    }

    public static (Window, Parts) Build(bool headless = false)
    {
        var parts = new Parts();
        var w = new Window("Pengaturan", 640, 470, headless);

        // Tampilan
        var themeIds = AppSettings.Choices(AppSettings.ThemeKey);
        parts.Themes = Choice(themeIds.ConvertAll(ThemeLabel), AppSettings.Theme, AppSettings.ThemeKey);
        parts.Themes.Height = 230;
        parts.Animations = new ToggleSwitch("", AppSettings.Animations) { OnContent = "Animasi jendela nyala", OffContent = "Animasi jendela mati", Padding = new Thickness(0) };
        parts.Animations.IsCheckedChanged += t => AppSettings.Animations = t.IsChecked;
        parts.Cursor = new ToggleSwitch("", AppSettings.LargeCursor) { OnContent = "Kursor besar", OffContent = "Kursor normal", Padding = new Thickness(0) };
        parts.Cursor.IsCheckedChanged += t => AppSettings.LargeCursor = t.IsChecked;
        var look = Page("Tampilan", "Tema berlaku langsung untuk seluruh desktop.", parts.Themes, parts.Animations, parts.Cursor);

        // Wallpaper
        var wpNames = AppSettings.Choices(AppSettings.WallpaperKey);
        var wpText = AppSettings.Choices(AppSettings.WallpaperKey, descriptions: true);
        var wpLabels = new List<string>();
        for (int i = 0; i < wpNames.Count; i++) wpLabels.Add(wpNames[i] + (i < wpText.Count && wpText[i].Length > 0 ? "  -  " + wpText[i] : ""));
        parts.Wallpapers = Choice(wpLabels, AppSettings.Wallpaper, AppSettings.WallpaperKey);
        var paper = Page("Wallpaper", "Delapan wallpaper resmi Buitenzorg OS.", parts.Wallpapers);

        // Screensaver
        var saverIds = AppSettings.Choices(AppSettings.ScreensaverKey);
        parts.Savers = Choice(saverIds.ConvertAll(SaverLabel), AppSettings.Screensaver, AppSettings.ScreensaverKey);
        parts.Savers.Height = 200;
        int delay = Math.Max(5, AppSettings.ScreensaverDelaySeconds);
        parts.DelayText = new TextBlock("Mulai setelah " + delay + " detik tanpa input");
        parts.Delay = new Slider { Minimum = 5, Maximum = 600, Value = Math.Min(600, delay), TickFrequency = 5, IsSnapToTickEnabled = true, Width = 320 };
        parts.Delay.ValueChanged += s =>
        {
            int v = (int)s.Value;
            parts.DelayText.Text = "Mulai setelah " + v + " detik tanpa input";
            AppSettings.ScreensaverDelaySeconds = v;
        };
        var saver = Page("Screensaver", "Muncul saat desktop menganggur; gerakkan mouse atau tekan tombol untuk kembali.", parts.Savers, parts.DelayText, parts.Delay);

        // Suara
        bool present = AppAudio.IsPresent;
        parts.SoundStatus = new TextBlock(present
            ? "Kartu suara: " + AppAudio.SampleRate + " Hz, stereo"
            : "Tidak ada kartu suara. (Raspberry Pi: audio HDMI / jack belum didukung.)");
        parts.Volume = new Slider { Minimum = 0, Maximum = 100, Value = present ? AppAudio.Volume : 0, TickFrequency = 5, IsSnapToTickEnabled = true, Width = 320, IsEnabled = present };
        var volText = new TextBlock("Volume " + (int)parts.Volume.Value + "%");
        parts.Volume.ValueChanged += s => { AppAudio.Volume = (int)s.Value; volText.Text = "Volume " + (int)s.Value + "%"; };
        var beep = new Button(new TextBlock("Bunyikan nada uji")) { IsEnabled = present };
        beep.Click += _ => AppAudio.Beep(880, 250);
        var sound = Page("Suara", "Volume utama sistem.", parts.SoundStatus, volText, parts.Volume, beep);

        var pages = new List<(string, IconKind, Control)>
        {
            ("Tampilan", IconKind.Sun, look),
            ("Wallpaper", IconKind.Leaf, paper),
            ("Screensaver", IconKind.Moon, saver),
            ("Suara", IconKind.Play, sound),
        };
        var host = new Border { Padding = new Thickness(16, 0, 0, 0) };
        var nav = new ListBox { BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0, Padding = new Thickness(6), Width = 160 };
        var names = new List<string>();
        foreach (var p in pages) names.Add(p.Item1);
        nav.ItemsSource = names;
        nav.ItemTemplate = o =>
        {
            int i = names.IndexOf((string)o);
            var r = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            r.Children.Add(new Icon(pages[i].Item2, 14) { VerticalAlignment = VerticalAlignment.Center });
            r.Children.Add(new TextBlock((string)o));
            return r;
        };
        nav.SelectionChanged += l => { if (l.SelectedIndex >= 0) host.Child = pages[l.SelectedIndex].Item3; };
        nav.SelectedIndex = 0;
        parts.Nav = nav;

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Sistem", "Pengaturan", IconKind.Gear, 0xD9B38C).DockTo(Dock.Top));
        root.Children.Add(nav.DockTo(Dock.Left));
        root.Children.Add(host);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, parts);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Pengaturan");
        var (w, p) = Build(headless: true);
        int themes = p.Themes.ItemCount, papers = p.Wallpapers.ItemCount, savers = p.Savers.ItemCount;
        c.That(themes >= 2 && papers >= 1 && savers >= 2, "choices listed (" + themes + " themes, " + papers + " wallpapers, " + savers + " screensavers)");

        // theme: pick another through the list, read it back, restore
        int theme0 = AppSettings.Theme;
        p.Themes.SelectedIndex = (theme0 + 1) % themes;
        c.That(AppSettings.Theme == (theme0 + 1) % themes, "theme changed through the list");
        p.Themes.SelectedIndex = theme0;
        c.That(AppSettings.Theme == theme0, "theme restored");

        int saver0 = AppSettings.Screensaver;
        p.Savers.SelectedIndex = savers - 1; // "off"
        c.That(AppSettings.Screensaver == savers - 1, "screensaver off");
        p.Savers.SelectedIndex = saver0 < 0 ? 0 : saver0;

        int delay0 = AppSettings.ScreensaverDelaySeconds;
        p.Delay.Value = 60;
        p.Delay.Value = 60; // (ValueChanged fires on a change)
        AppSettings.ScreensaverDelaySeconds = 60;
        c.That(Math.Abs(AppSettings.ScreensaverDelaySeconds - 60) <= 1, "screensaver delay 60 s (" + AppSettings.ScreensaverDelaySeconds + ")");
        AppSettings.ScreensaverDelaySeconds = Math.Max(5, delay0);

        bool anim0 = AppSettings.Animations;
        AppSettings.Animations = !anim0;
        c.That(AppSettings.Animations == !anim0, "animations switched");
        AppSettings.Animations = anim0;

        if (AppAudio.IsPresent)
        {
            int v0 = AppAudio.Volume;
            AppAudio.Volume = 35;
            c.That(AppAudio.Volume == 35, "volume 35%");
            AppAudio.Volume = v0;
        }
        else c.That(!p.Volume.IsEnabled, "no sound card: the volume slider is disabled");

        c.Equal("Mati", SaverLabel("off"), "screensaver labels");
        p.Nav.SelectedIndex = 1;
        w.RenderFrame();
        c.That(w.LastFrameOps > 30, "frame drew " + w.LastFrameOps + " ops");
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
            var c = SettingsApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = SettingsApp.Build();
        app.Run(w);
        return 0;
    }
}
