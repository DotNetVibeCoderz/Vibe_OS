// Jaringan -- the network: is the machine connected, how (cable / Wi-Fi), its
// addresses; ask DHCP again; scan for Wi-Fi networks and join one with a
// password. Opened by the tray's network icon; the same NetworkPanel is the
// "Jaringan" page of Pengaturan. Buitenzorg.UI + Buitenzorg.Net.NetworkConfig.

using Buitenzorg.Net;
using Buitenzorg.UI;

namespace MJaring;

/// <summary>The network panel (status + Wi-Fi), for this app and Pengaturan.</summary>
public sealed class NetworkPanel
{
    public Control Root = null!;
    public TextBlock Link = null!, Address = null!, Details = null!, Status = null!, WifiInfo = null!;
    public Button Renew = null!, Scan = null!, Connect = null!, Forget = null!;
    public ListBox Networks = null!;
    /// <summary>Scan, list, name/password, join: shown only with an adapter whose driver can join.</summary>
    public StackPanel WifiBox = null!;
    public TextBox Ssid = null!, Password = null!;
    readonly ObservableCollection<WifiNetwork> _found = new();
    readonly bool _headless;
    bool _busy;

    public NetworkStatus Last = new();

    public NetworkPanel(bool headless) { _headless = headless; Build(); }

    static Control Row(params Control[] items)
    {
        var s = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        foreach (var c in items) s.Children.Add(c);
        return s;
    }

    void Build()
    {
        Link = new TextBlock("").WithClass("h2");
        Address = new TextBlock("").WithClass("strong");
        Details = new TextBlock("") { TextWrapping = TextWrapping.Wrap }.WithClass("faint");
        Renew = new Button(new TextBlock("Sambungkan ulang")).WithClass("subtle");
        Renew.Click += _ => DoRenew();

        WifiInfo = new TextBlock("") { TextWrapping = TextWrapping.Wrap }.WithClass("faint");
        Scan = new Button(new TextBlock("Cari jaringan"));
        Scan.Click += _ => DoScan();
        Networks = new ListBox { ItemsSource = _found, Height = 112 };
        Networks.ItemTemplate = o =>
        {
            var n = (WifiNetwork)o;
            var r = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            r.Children.Add(new SignalBars(n.Bars) { VerticalAlignment = VerticalAlignment.Center });
            r.Children.Add(new TextBlock(n.Ssid.Length == 0 ? "(tersembunyi)" : n.Ssid).WithClass("strong"));
            r.Children.Add(new TextBlock((n.Secured ? "aman" : "terbuka") + ", kanal " + n.Channel).WithClass("faint"));
            return r;
        };
        Networks.SelectionChanged += l =>
        {
            if (l.SelectedIndex >= 0 && l.SelectedIndex < _found.Count)
            {
                Ssid.Text = _found[l.SelectedIndex].Ssid;
                Password.Text = "";
            }
        };
        Ssid = new TextBox { Watermark = "nama jaringan (SSID)", Width = 190 };
        Password = new TextBox { Watermark = "kata sandi", PasswordChar = '*', Width = 150 };
        Connect = new Button(new TextBlock("Sambungkan")).WithClass("accent");
        Connect.Click += _ => DoConnect();
        Forget = new Button(new TextBlock("Lupakan")).WithClass("subtle");
        Forget.Click += _ => { NetworkConfig.Forget(); Refresh(); Status.Text = "Jaringan tersimpan dilupakan."; };
        Status = new TextBlock("") { TextWrapping = TextWrapping.Wrap }.WithClass("faint");

        var s = new StackPanel { Spacing = 8 };
        s.Children.Add(Row(new NetGlyph(this) { VerticalAlignment = VerticalAlignment.Center }, Link));
        s.Children.Add(Address);
        s.Children.Add(Details);
        s.Children.Add(Renew);
        s.Children.Add(new TextBlock("Wi-Fi").WithClass("h2"));
        s.Children.Add(WifiInfo);
        WifiBox = new StackPanel { Spacing = 8 };
        WifiBox.Children.Add(Scan);
        WifiBox.Children.Add(Networks);
        WifiBox.Children.Add(Row(Ssid, Password));
        WifiBox.Children.Add(Row(Connect, Forget));
        s.Children.Add(WifiBox);
        s.Children.Add(Status);
        Root = s;
        Refresh();
    }

    /// <summary>Read the state again and show it.</summary>
    public void Refresh()
    {
        var st = NetworkConfig.Status();
        Last = st;
        Link.Text = st.Connected ? "Tersambung: " + st.LinkText : "Tidak tersambung";
        Address.Text = st.Connected ? "IPv4 " + st.Address + "/" + st.Prefix : "Tidak ada alamat IPv4";
        var d = new List<string>();
        if (st.Gateway.Length > 0) d.Add("gateway " + st.Gateway);
        if (st.Dns.Length > 0) d.Add("DNS " + st.Dns);
        if (st.Mac.Length > 0) d.Add("MAC " + st.Mac);
        if (st.Ipv6.Count > 0) d.Add("IPv6 " + st.Ipv6[0]);
        if (!st.Connected && st.Message.Length > 0) d.Add("status: " + st.Message);
        Details.Text = string.Join("  |  ", d);
        bool canWifi = st.WifiAdapter.Length > 0 && st.WifiCanScan;
        WifiInfo.Text = st.WifiAdapter.Length == 0
            ? "Komputer ini tidak punya adaptor Wi-Fi yang dikenali."
            : !st.WifiCanScan ? "Adaptor: " + st.WifiAdapter + ". Belum bisa dipakai untuk menyambung."
            : "Adaptor: " + st.WifiAdapter + (st.SavedNetwork.Length > 0 ? ". Tersimpan: " + st.SavedNetwork + " (disambung saat mulai)." : ".");
        WifiBox.IsVisible = canWifi;
        Scan.IsEnabled = canWifi && !_busy;
        Connect.IsEnabled = canWifi && !_busy;
        Networks.IsEnabled = canWifi;
        Ssid.IsEnabled = canWifi;
        Password.IsEnabled = canWifi;
        Forget.IsEnabled = st.SavedNetwork.Length > 0;
    }

    /// <summary>Slow work (a scan, joining) runs on a thread in the live app and
    /// inline in tests; `done` always runs on the UI thread.</summary>
    void Work(string message, Func<Action> job)
    {
        if (_busy) return;
        _busy = true;
        Status.Text = message;
        Refresh();
        if (_headless)
        {
            var done = job();
            _busy = false;
            done();
            Refresh();
            return;
        }
        var t = new Thread(() =>
        {
            var done = job();
            Dispatcher.Post(() => { _busy = false; done(); Refresh(); });
        });
        t.Start();
    }

    public void DoRenew()
    {
        bool ok = NetworkConfig.Renew();
        Status.Text = ok ? "Meminta alamat lagi (DHCP)... tunggu 10-15 detik." : "Platform ini tidak bisa menyambung ulang.";
        if (!_headless && ok)
        {
            var again = new DispatcherTimer(5000);
            int n = 0;
            again.Tick += () => { Refresh(); if (++n >= 4) again.Stop(); };
            again.Start();
        }
    }

    public void DoScan() => Work("Mencari jaringan Wi-Fi (beberapa detik)...", () =>
    {
        var list = NetworkConfig.Scan(out string err);
        return () =>
        {
            if (list == null) { Status.Text = "Tidak bisa mencari: " + err; return; }
            _found.ResetWith(list);
            Status.Text = list.Count + " jaringan ditemukan. Pilih satu, isi kata sandi, lalu Sambungkan.";
        };
    });

    public void DoConnect()
    {
        string ssid = Ssid.Text.Trim(), pw = Password.Text;
        Work("Menyambung ke " + ssid + "...", () =>
        {
            var err = NetworkConfig.Connect(ssid, pw);
            return () =>
            {
                Status.Text = err == null ? "Tersambung ke " + ssid + ". Disimpan untuk disambung saat mulai." : "Gagal: " + err;
                if (err == null) Password.Text = "";
            };
        });
    }
}

/// <summary>Four signal bars, `bars` of them lit.</summary>
sealed class SignalBars : Control
{
    readonly int _bars;
    public SignalBars(int bars) { _bars = bars; Width = 18; Height = 14; }
    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        for (int i = 0; i < 4; i++)
        {
            int h = 4 + i * 3;
            dc.FillRect(new Rect(b.X + i * 4, b.Y + 14 - h, 3, h), i < _bars ? Theme.Accent : Theme.Stroke);
        }
    }
}

/// <summary>The link glyph beside the status line (same shapes as the tray icon).</summary>
sealed class NetGlyph : Control
{
    readonly NetworkPanel _p;
    public NetGlyph(NetworkPanel p) { _p = p; Width = 24; Height = 20; }
    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        var st = _p.Last;
        if (st.Link == LinkKind.Wireless && st.Connected)
        {
            for (int i = 0; i < 4; i++) { int h = 5 + i * 4; dc.FillRect(new Rect(b.X + 2 + i * 5, b.Y + 19 - h, 4, h), Theme.Accent); }
            return;
        }
        var c = st.Connected ? Theme.Accent : Theme.Stroke;
        dc.StrokeRoundRect(new Rect(b.X + 2, b.Y + 1, 20, 13), 2, c, 2);
        dc.FillRect(new Rect(b.X + 11, b.Y + 14, 2, 3), c);
        dc.FillRect(new Rect(b.X + 7, b.Y + 17, 10, 2), c);
        if (!st.Connected) dc.FillRect(new Rect(b.X + 15, b.Y + 9, 8, 8), 0xE0483B);
    }
}

public static class JaringApp
{
    public static (Window, NetworkPanel) Build(bool headless = false)
    {
        var w = new Window("Jaringan", 560, 520, headless);
        var panel = new NetworkPanel(headless);
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Sistem", "Jaringan", IconKind.Home, 0x8FB8E8).DockTo(Dock.Top));
        root.Children.Add(new ScrollViewer { Content = panel.Root });
        w.Content = root;
        if (!headless)
        {
            // keep the status current (a cable plugged in, DHCP finishing)
            var t = new DispatcherTimer(5000);
            t.Tick += panel.Refresh;
            t.Start();
        }
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, panel);
    }

    /// <summary>Checks the panel against the device (or the simulated machine off it).
    /// It never joins or forgets a real network.</summary>
    public static Checks SelfTest()
    {
        var c = new Checks("Jaringan");
        var parsed = NetworkConfig.Parse(new[] { "link=wifi", "ip=192.168.1.9", "prefix=24", "gateway=192.168.1.1", "ipv6=fe80::1/64", "wifi=X", "wifi_scan=1", "wifi_saved=Kebun" });
        c.That(parsed.Link == LinkKind.Wireless && parsed.Connected && parsed.Prefix == 24 && parsed.Ipv6.Count == 1 && parsed.WifiCanScan && parsed.SavedNetwork == "Kebun", "netinfo lines parsed");
        c.That(!NetworkConfig.Parse(new[] { "link=none", "ip=10.0.0.1" }).Connected, "no link = not connected");
        c.That(NetworkConfig.Connect("x", "short") != null && NetworkConfig.Connect("", "") != null, "a bad password / empty name is refused before trying");

        var (w, p) = Build(headless: true);
        var st = p.Last;
        c.That(p.Link.Text.Length > 0 && p.WifiInfo.Text.Length > 0, "status: " + p.Link.Text + " / " + p.Address.Text);
        if (Buitenzorg.App.Shell.IsAvailable)
        {
            // on the device: netinfo answers and agrees with the panel
            c.That(Buitenzorg.App.Shell.Run("netinfo").Output.Contains("link="), "netinfo answers");
            c.That(st.Connected == (st.Address.Length > 0), "address shown when connected (" + st.LinkText + ")");
            c.That(p.WifiBox.IsVisible == (st.WifiAdapter.Length > 0 && st.WifiCanScan), "Wi-Fi controls shown only with a working adapter (" + (st.WifiAdapter.Length == 0 ? "none" : st.WifiAdapter) + ")");
        }
        else
        {
            // the simulated machine: scan, a wrong password, join, forget
            c.That(st.Link == LinkKind.Wired && st.Connected, "cable connected");
            w.SimulateClick(p.Scan);
            c.That(p.Networks.ItemCount == 3, "scan listed " + p.Networks.ItemCount + " networks");
            p.Networks.SelectedIndex = 0;
            c.Equal("KebunRaya", p.Ssid.Text, "picking a network fills its name");
            p.Password.Text = "salahsalah";
            w.SimulateClick(p.Connect);
            c.That(p.Status.Text.StartsWith("Gagal"), "wrong password: " + p.Status.Text);
            p.Password.Text = "rahasia123";
            w.SimulateClick(p.Connect);
            c.That(p.Last.Link == LinkKind.Wireless && p.Link.Text.Contains("Wi-Fi"), "joined: " + p.Link.Text);
            c.That(p.Forget.IsEnabled && p.WifiInfo.Text.Contains("KebunRaya"), "saved network shown");
            w.SimulateClick(p.Forget);
            c.That(!p.Forget.IsEnabled, "forgotten");
        }
        w.SimulateClick(p.Renew);
        c.That(p.Status.Text.Length > 0, "renew: " + p.Status.Text);
        w.RenderFrame();
        c.That(w.LastFrameOps > 20 && DrawingContext.Refused == 0, "frame drew " + w.LastFrameOps + " ops");
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
            var c = JaringApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = JaringApp.Build();
        app.Run(w);
        return 0;
    }
}
