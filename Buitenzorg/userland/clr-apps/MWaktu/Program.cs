// Tanggal & Waktu -- date, time and region: a live clock with the long date,
// a month calendar, setting the date and time, the time zone, the interface
// language and the 12/24-hour clock. Opened by clicking the tray clock. The
// values live in the kernel (SETTING keys TIMEZONE / LANGUAGE / CLOCK_24H /
// CLOCK, saved to /disk/REGION.CFG); the tray clock follows them at once.
// Buitenzorg.UI + Buitenzorg.App.Region.

using Buitenzorg.UI;
using Region = Buitenzorg.App.Region;

namespace MWaktu;

public static class WaktuApp
{
    public sealed class Parts
    {
        public TextBlock Time = null!, DateText = null!, Status = null!;
        public Calendar Calendar = null!;
        public NumericUpDown Hour = null!, Minute = null!;
        public ComboBox Zone = null!, Language = null!;
        public ToggleSwitch H24 = null!;
        public Button Apply = null!;
    }

    static Control Field(string label, Control input)
    {
        var s = new StackPanel { Spacing = 4 };
        s.Children.Add(new TextBlock(label).WithClass("faint"));
        s.Children.Add(input);
        return s;
    }

    public static (Window, Parts) Build(bool headless = false)
    {
        var p = new Parts();
        var w = new Window("Tanggal & Waktu", 660, 470, headless);
        var now = Region.LocalTime;

        // left: the live clock, the long date and the calendar
        p.Time = new TextBlock(Region.FormatTime(now, seconds: true)).WithClass("h1");
        p.Time.FontSize = 32;
        p.DateText = new TextBlock(Region.FormatDate(now)).WithClass("muted");
        p.Calendar = new Calendar { SelectedDate = new Date(now.Year, now.Month, now.Day) };
        var left = new StackPanel { Spacing = 10, Width = 290 };
        left.Children.Add(p.Time);
        left.Children.Add(p.DateText);
        left.Children.Add(p.Calendar);

        // right: set date + time, region
        p.Hour = new NumericUpDown { Minimum = 0, Maximum = 23, Value = now.Hour, Width = 110 };
        p.Minute = new NumericUpDown { Minimum = 0, Maximum = 59, Value = now.Minute, Width = 110 };
        var hm = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        hm.Children.Add(Field("Jam", p.Hour));
        hm.Children.Add(Field("Menit", p.Minute));
        p.Status = new TextBlock("Pilih tanggal di kalender, atur jam, lalu terapkan.") { TextWrapping = TextWrapping.Wrap }.WithClass("faint");
        p.Apply = new Button(new TextBlock("Terapkan tanggal & waktu")).WithClass("accent");
        p.Apply.Click += _ => ApplyTime(p);

        var zones = Region.TimeZones;
        var zoneText = Region.TimeZoneDescriptions;
        var zoneLabels = new List<string>();
        for (int i = 0; i < zones.Count; i++) zoneLabels.Add(zones[i] + "  -  " + (i < zoneText.Count ? zoneText[i] : ""));
        p.Zone = new ComboBox(zoneLabels, Region.TimeZone) { Width = 320 };
        p.Zone.SelectionChanged += c =>
        {
            if (c.SelectedIndex >= 0 && c.SelectedIndex != Region.TimeZone)
            {
                Region.TimeZone = c.SelectedIndex;
                Refresh(p, syncFields: true);
                p.Status.Text = "Zona waktu: " + zones[c.SelectedIndex];
            }
        };
        p.Language = new ComboBox(Region.Languages, Region.Language) { Width = 320 };
        p.Language.SelectionChanged += c =>
        {
            if (c.SelectedIndex >= 0 && c.SelectedIndex != Region.Language)
            {
                Region.Language = c.SelectedIndex;
                Refresh(p);
            }
        };
        p.H24 = new ToggleSwitch("", Region.Use24Hour) { OnContent = "Format 24 jam", OffContent = "Format 12 jam (AM/PM)", Padding = new Thickness(0) };
        p.H24.IsCheckedChanged += t => { Region.Use24Hour = t.IsChecked; Refresh(p); };

        var right = new StackPanel { Spacing = 12 };
        right.Children.Add(new TextBlock("Atur tanggal & waktu").WithClass("h2"));
        right.Children.Add(hm);
        right.Children.Add(p.Apply);
        right.Children.Add(p.Status);
        right.Children.Add(new TextBlock("Wilayah & bahasa").WithClass("h2"));
        right.Children.Add(Field("Zona waktu", p.Zone));
        right.Children.Add(Field("Bahasa antarmuka (tanggal di taskbar)", p.Language));
        right.Children.Add(p.H24);

        var body = new DockPanel();
        body.Children.Add(left.DockTo(Dock.Left));
        body.Children.Add(new Border { Padding = new Thickness(24, 0, 0, 0), Child = right });

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Sistem", "Tanggal & Waktu", IconKind.Clock, 0xD3E07A).DockTo(Dock.Top));
        root.Children.Add(body);
        w.Content = root;

        if (!headless)
        {
            // the clock ticks once a second (only the time text changes)
            var tick = new DispatcherTimer(1000);
            tick.Tick += () => Refresh(p);
            tick.Start();
        }
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, p);
    }

    static void Refresh(Parts p, bool syncFields = false)
    {
        var t = Region.LocalTime;
        p.Time.Text = Region.FormatTime(t, seconds: true);
        p.DateText.Text = Region.FormatDate(t);
        if (syncFields)
        {
            p.Hour.Value = t.Hour;
            p.Minute.Value = t.Minute;
            p.Calendar.SelectedDate = new Date(t.Year, t.Month, t.Day);
        }
    }

    static void ApplyTime(Parts p)
    {
        var d = p.Calendar.SelectedDate ?? Date.Today;
        var t = new DateTime(d.Year, d.Month, d.Day, (int)p.Hour.Value, (int)p.Minute.Value, 0);
        Region.LocalTime = t;
        var now = Region.LocalTime;
        bool ok = Math.Abs((now - t).TotalSeconds) < 5;
        p.Status.Text = ok ? "Waktu diatur: " + Region.FormatDate(now) + ", " + Region.FormatTime(now)
                           : "Waktu tidak bisa diatur.";
        Refresh(p);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Tanggal & Waktu");
        var (w, p) = Build(headless: true);
        int zones = p.Zone.Items.Count;
        c.That(zones >= 4 && p.Language.Items.Count >= 2, "zones (" + zones + ") and languages listed");
        c.That(p.Time.Text.Contains(':') && p.DateText.Text.Length > 8, "clock shows " + p.Time.Text + ", " + p.DateText.Text);

        // the time zone moves local time by the zones' offset difference
        int zone0 = Region.TimeZone;
        var t0 = Region.LocalTime;
        p.Zone.SelectedIndex = 1; // WITA = WIB + 1 h
        double shift = (Region.LocalTime - t0).TotalMinutes;
        c.That(Region.TimeZone == 1 && Math.Abs(shift - (zone0 == 0 ? 60 : shift)) < 2, "zone WITA shifts the clock (" + Math.Round(shift) + " min)");
        p.Zone.SelectedIndex = zone0;
        c.That(Region.TimeZone == zone0, "zone restored");

        // 12 / 24 hours
        bool h0 = Region.Use24Hour;
        p.H24.IsChecked = false;
        c.That(!Region.Use24Hour && p.Time.Text.Contains('M'), "12-hour clock: " + p.Time.Text);
        p.H24.IsChecked = true;
        c.That(Region.Use24Hour && !p.Time.Text.Contains('M'), "24-hour clock: " + p.Time.Text);
        Region.Use24Hour = h0;

        // set the date and time through the form, then put the real time back
        var real = Region.LocalTime;
        p.Calendar.SelectedDate = new Date(2030, 1, 15);
        p.Hour.Value = 9;
        p.Minute.Value = 30;
        w.SimulateClick(p.Apply);
        var set = Region.LocalTime;
        c.That(set.Year == 2030 && set.Month == 1 && set.Day == 15 && set.Hour == 9 && set.Minute == 30, "form set the clock to " + set.ToString("yyyy-MM-dd HH:mm"));
        Region.LocalTime = real.AddSeconds(1);
        c.That(Math.Abs((Region.LocalTime - real).TotalSeconds) < 10, "clock restored");

        // language: English date names
        int lang0 = Region.Language;
        p.Language.SelectedIndex = 1;
        c.That(Region.Language == 1 && p.DateText.Text.Contains(','), "English date: " + p.DateText.Text);
        p.Language.SelectedIndex = lang0;
        Region.Language = lang0;

        w.RenderFrame();
        c.That(w.LastFrameOps > 40, "frame drew " + w.LastFrameOps + " ops");
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
            var c = WaktuApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = WaktuApp.Build();
        app.Run(w);
        return 0;
    }
}
