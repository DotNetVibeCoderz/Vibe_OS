// Unduhan -- the download manager: paste an http:// or https:// address, the
// file is saved to /disk/UNDUHAN with an 8.3 name. Downloads run one at a time
// on a background thread, stream straight to the file (HttpClient.Download over
// FS_APPEND: nothing is held in memory), can be paused and continued later --
// also after a restart, with an HTTP Range request -- and the list is kept in
// /disk/UNDUHAN/DAFTAR.TXT. Buitenzorg.UI.

using System.Text;
using System.Threading;
using Buitenzorg.App;
using Buitenzorg.Net;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;
using HttpClient = Buitenzorg.Net.HttpClient;
using Window = Buitenzorg.UI.Window;

namespace MUnduh;

public enum JobState { Waiting, Running, Paused, Done, Failed }

public sealed class Job
{
    public string Url = "", Path = "", Name = "";
    public JobState State;
    public long Saved, Total = -1;
    public string Error = "";
    /// <summary>Set by the UI: the worker stops after the current piece.</summary>
    public bool StopAsked;
    /// <summary>Tests: pause by itself once this many bytes are saved (0 = never).</summary>
    public long PauseAt;
    public int BytesPerSecond;
    public bool Removed;
}

public sealed class UnduhApp
{
    public Window W { get; }
    public readonly List<Job> Jobs = new List<Job>();
    public string Folder { get; }
    /// <summary>Tests: run downloads on the calling thread (RunPending).</summary>
    public bool Synchronous;
    readonly TextBox _url = new TextBox { Watermark = "Tempel alamat, mis. https://example.com/berkas.zip" };
    readonly StackPanel _rows = new StackPanel { Spacing = 10 };
    readonly TextBlock _status = new TextBlock("");
    readonly DispatcherTimer _timer = new DispatcherTimer(300);
    readonly List<Row> _rowViews = new List<Row>();
    readonly object _lock = new object();
    bool _workerRunning;
    string _shape = "";

    sealed class Row
    {
        public Job Job = null!;
        public ProgressBar Bar = null!;
        public TextBlock Info = null!;
        public long LastSaved;
        public long LastMs;
    }

    public UnduhApp(bool headless, string? folder = null)
    {
        Folder = folder ?? PickFolder();
        W = new Window("Unduhan", 600, 480, headless);
        _url.KeyDown += (_, e) => { if (e.Key == Keys.Enter) { AddFromBox(); e.Handled = true; } };
        var bar = new DockPanel { Margin = new Thickness(0, 0, 0, 12) };
        bar.Children.Add(new Button("Unduh", _ => AddFromBox()).WithClass("accent").DockTo(Dock.Right));
        _url.Margin = new Thickness(0, 0, 8, 0);
        bar.Children.Add(_url);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Utilitas", "Unduhan", IconKind.Save, 0x5FA8D3).DockTo(Dock.Top));
        root.Children.Add(bar.DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(new ScrollViewer { Content = _rows });
        W.Content = root;
        _timer.Tick += Refresh;
        Load();
        Rebuild();
        Application.Current?.Apply(W);
        W.UpdateLayout();
        _timer.Start();
    }

    static string PickFolder()
    {
        const string d = "/disk/UNDUHAN";
        if (Files.GetInfo(d) != null || Files.CreateDirectory(d)) return d;
        return Files.ScratchDirectory;
    }

    string ListFile => Folder + "/DAFTAR.TXT";

    // ---- the list ---------------------------------------------------------

    void AddFromBox()
    {
        string u = _url.Text.Trim();
        if (u.Length == 0) return;
        if (!u.Contains("://")) u = "http://" + u;
        var j = Add(u);
        if (j == null) { _status.Text = "Alamat tidak sah: " + u; return; }
        _url.Text = "";
        _status.Text = "Ditambahkan: " + j.Name;
    }

    /// <summary>Queue a download; null when the address is not http(s).</summary>
    public Job? Add(string url)
    {
        if (Url.Parse(url) == null) return null;
        Job j;
        lock (_lock)
        {
            string name = FileNameFor(url, n => Files.GetInfo(Folder + "/" + n) != null || Jobs.Exists(x => !x.Removed && x.Name == n));
            j = new Job { Url = url, Name = name, Path = Folder + "/" + name, State = JobState.Waiting };
            Jobs.Add(j);
        }
        Save();
        Rebuild();
        Kick();
        return j;
    }

    public void Pause(Job j)
    {
        if (j.State == JobState.Running) j.StopAsked = true;
        else if (j.State == JobState.Waiting) { j.State = JobState.Paused; Save(); }
        Rebuild();
    }

    public void Resume(Job j)
    {
        if (j.State != JobState.Paused && j.State != JobState.Failed) return;
        j.State = JobState.Waiting;
        j.Error = "";
        Save();
        Rebuild();
        Kick();
    }

    /// <summary>Take it off the list (the file stays unless it is incomplete).</summary>
    public void Remove(Job j)
    {
        if (j.State == JobState.Running) { j.StopAsked = true; j.Removed = true; return; }   // the worker drops it
        j.Removed = true;
        if (j.State != JobState.Done) Files.Delete(j.Path);
        lock (_lock) Jobs.Remove(j);
        Save();
        Rebuild();
    }

    // ---- the worker -------------------------------------------------------

    void Kick()
    {
        if (Synchronous) return;
        lock (_lock)
        {
            if (_workerRunning) return;
            _workerRunning = true;
        }
        new Thread(() =>
        {
            while (RunOne()) { }
            lock (_lock) _workerRunning = false;
            // a job queued between the last RunOne and the flag: start again
            bool more;
            lock (_lock) more = Jobs.Exists(x => x.State == JobState.Waiting);
            if (more) Kick();
        }).Start();
    }

    /// <summary>Tests: run every waiting download now, on this thread.</summary>
    public void RunPending() { while (RunOne()) { } }

    bool RunOne()
    {
        Job? j;
        lock (_lock)
        {
            j = Jobs.Find(x => x.State == JobState.Waiting && !x.Removed);
            if (j == null) return false;
            j.State = JobState.Running;
            j.StopAsked = false;
        }
        try
        {
            var http = new HttpClient { TimeoutMs = 20000, UserAgent = "Buitenzorg/1.0 (Unduhan)" };
            var r = http.Download(j.Url, j.Path,
                (saved, total) => { j.Saved = saved; j.Total = total; if (j.PauseAt > 0 && saved >= j.PauseAt) j.StopAsked = true; },
                () => j.StopAsked);
            j.Saved = r.Saved;
            if (r.Cancelled) j.State = JobState.Paused;
            else if (r.Ok || r.Status == 416) { j.State = JobState.Done; j.Total = j.Saved; }
            else { j.State = JobState.Failed; j.Error = "server menjawab " + r.Status + (r.Reason.Length > 0 ? " " + r.Reason : ""); }
        }
        catch (Exception e)
        {
            j.State = JobState.Failed;
            j.Error = e.Message;
        }
        if (j.Removed)
        {
            if (j.State != JobState.Done) Files.Delete(j.Path);
            lock (_lock) Jobs.Remove(j);
        }
        Save();
        return true;
    }

    // ---- persistence: one job per line, state <TAB> url <TAB> name -------

    void Save()
    {
        var sb = new StringBuilder();
        lock (_lock)
            foreach (var j in Jobs)
            {
                var st = j.State == JobState.Running || j.State == JobState.Waiting ? JobState.Paused : j.State;
                if (j.State == JobState.Waiting) st = JobState.Waiting;
                sb.Append((int)st).Append('\t').Append(j.Url).Append('\t').Append(j.Name).Append('\n');
            }
        Files.WriteAllText(ListFile, sb.ToString());
    }

    void Load()
    {
        string text = Files.ReadAllText(ListFile);
        foreach (var line in text.Split('\n'))
        {
            var f = line.Split('\t');
            if (f.Length < 3 || !int.TryParse(f[0], out int st) || f[2].Length == 0) continue;
            var j = new Job { Url = f[1], Name = f[2], Path = Folder + "/" + f[2], State = (JobState)st };
            if (j.State == JobState.Running) j.State = JobState.Paused;   // interrupted by a restart
            j.Saved = Math.Max(0L, Files.Size(j.Path));
            if (j.State == JobState.Done) j.Total = j.Saved;
            Jobs.Add(j);
        }
        if (Jobs.Exists(x => x.State == JobState.Waiting)) Kick();
    }

    // ---- the view ---------------------------------------------------------

    void Rebuild()
    {
        _rows.Children.Clear();
        _rowViews.Clear();
        List<Job> snapshot;
        lock (_lock) snapshot = new List<Job>(Jobs);
        var sb = new StringBuilder();
        foreach (var j in snapshot)
        {
            sb.Append((int)j.State);
            var row = new Row { Job = j, Bar = new ProgressBar { Maximum = 1000, Height = 8 }, Info = new TextBlock("") };
            var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Margin = new Thickness(10, 0, 0, 0) };
            if (j.State == JobState.Running || j.State == JobState.Waiting)
                buttons.Children.Add(new Button("Jeda", _ => Pause(j)).WithClass("subtle"));
            else if (j.State == JobState.Paused || j.State == JobState.Failed)
                buttons.Children.Add(new Button("Lanjutkan", _ => Resume(j)).WithClass("accent"));
            buttons.Children.Add(new Button("Hapus", _ => Remove(j)).WithClass("subtle"));
            var text = new StackPanel { Spacing = 4 };
            text.Children.Add(new TextBlock(j.Name).WithClass("strong"));
            text.Children.Add(row.Bar);
            text.Children.Add(row.Info.WithClass("faint"));
            var dock = new DockPanel();
            dock.Children.Add(buttons.DockTo(Dock.Right));
            dock.Children.Add(text);
            _rows.Children.Add(dock);
            _rowViews.Add(row);
            UpdateRow(row, 0);
        }
        if (snapshot.Count == 0) _rows.Children.Add(new TextBlock("Belum ada unduhan. Tempel alamat di atas lalu tekan Enter.") { TextWrapping = TextWrapping.Wrap }.WithClass("faint"));
        _shape = sb.ToString();
        _status.Text = "Disimpan ke " + Folder;
    }

    void Refresh()
    {
        var sb = new StringBuilder();
        lock (_lock) foreach (var j in Jobs) sb.Append((int)j.State);
        if (sb.ToString() != _shape) { Rebuild(); return; }   // a job changed state or the list changed
        long now = Environment.TickCount64;
        foreach (var r in _rowViews) UpdateRow(r, now);
    }

    void UpdateRow(Row r, long now)
    {
        var j = r.Job;
        if (now > 0 && j.State == JobState.Running && r.LastMs > 0 && now - r.LastMs >= 1000)
        {
            j.BytesPerSecond = (int)((j.Saved - r.LastSaved) * 1000 / (now - r.LastMs));
            r.LastSaved = j.Saved;
            r.LastMs = now;
        }
        else if (r.LastMs == 0) { r.LastSaved = j.Saved; r.LastMs = now; }
        r.Bar.Value = j.Total > 0 ? j.Saved * 1000 / j.Total : j.State == JobState.Done ? 1000 : 0;
        r.Info.Text = Describe(j);
    }

    public static string Describe(Job j)
    {
        string amount = j.Total > 0 ? Size(j.Saved) + " dari " + Size(j.Total) : Size(j.Saved);
        switch (j.State)
        {
            case JobState.Waiting: return "Menunggu  -  " + amount;
            case JobState.Running: return "Mengunduh  -  " + amount + (j.BytesPerSecond > 0 ? "  -  " + Size(j.BytesPerSecond) + "/s" : "");
            case JobState.Paused: return "Dijeda  -  " + amount;
            case JobState.Done: return "Selesai  -  " + Size(j.Saved);
            default: return "Gagal: " + j.Error;
        }
    }

    /// <summary>"512 B", "48 KB", "3.2 MB" (integer math: RustBCL has no "F1").</summary>
    public static string Size(long b)
    {
        if (b < 1024) return b + " B";
        if (b < 1024 * 1024) return (b / 1024) + " KB";
        long tenths = b * 10 / (1024 * 1024);
        return (tenths / 10) + "." + (tenths % 10) + " MB";
    }

    /// <summary>An 8.3 name for the address's last path segment: letters and
    /// digits only, upper case, "UNDUHAN.BIN" when nothing is left; a name in
    /// use gets a number in its last characters.</summary>
    public static string FileNameFor(string url, Func<string, bool> taken)
    {
        string path = Url.Parse(url)?.Path ?? "/";
        int q = path.IndexOf('?');
        if (q >= 0) path = path.Substring(0, q);
        string seg = path.Substring(path.LastIndexOf('/') + 1);
        int dot = seg.LastIndexOf('.');
        string stem = Clean(dot > 0 ? seg.Substring(0, dot) : seg, 8);
        string ext = Clean(dot > 0 ? seg.Substring(dot + 1) : "", 3);
        if (stem.Length == 0) stem = "UNDUHAN";
        if (ext.Length == 0) ext = "BIN";
        string name = stem + "." + ext;
        for (int n = 1; taken(name) && n < 100; n++)
        {
            string num = n.ToString();
            name = stem.Substring(0, Math.Min(stem.Length, 8 - num.Length)) + num + "." + ext;
        }
        return name;
    }

    static string Clean(string s, int max)
    {
        var sb = new StringBuilder();
        foreach (char ch in s)
        {
            char c = char.ToUpperInvariant(ch);
            if ((c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')) sb.Append(c);
            if (sb.Length == max) break;
        }
        return sb.ToString();
    }

    // ---- self-test --------------------------------------------------------

    const int TestPort = 8089;

    static byte[] TestData()
    {
        var b = new byte[600000];
        for (int i = 0; i < b.Length; i++) b[i] = (byte)(i * 31 + (i >> 10));
        return b;
    }

    /// <summary>A tiny HTTP server: /data.bin (with Range), everything else 404.</summary>
    static void Serve(TcpServer server, Func<bool> stop)
    {
        var data = TestData();
        while (!stop())
        {
            var c = server.Accept(50);
            if (c == null) continue;
            try
            {
                c.TimeoutMs = 2000;
                var buf = new byte[4096];
                var req = new StringBuilder();
                while (!req.ToString().Contains("\r\n\r\n"))
                {
                    int n = c.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    req.Append(Encoding.ASCII.GetString(buf, 0, n));
                }
                string line = req.ToString();
                string head;
                long from = 0;
                if (line.StartsWith("GET /data.bin"))
                {
                    int ri = line.IndexOf("Range: bytes=");
                    if (ri >= 0) long.TryParse(line.Substring(ri + 13, line.IndexOf('-', ri) - ri - 13), out from);
                    head = (from > 0 ? "HTTP/1.1 206 Partial Content\r\n" : "HTTP/1.1 200 OK\r\n") + "Content-Length: " + (data.Length - from) + "\r\nConnection: close\r\n\r\n";
                }
                else head = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                var h = Encoding.ASCII.GetBytes(head);
                c.Write(h, 0, h.Length);
                if (head.Contains(" 20"))
                    for (long o = from; o < data.Length; o += 16384) c.Write(data, (int)o, (int)Math.Min(16384, data.Length - o));
            }
            catch (Exception) { }
            c.Close();
        }
        server.Close();
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Unduhan");
        c.Equal("LAPORANA.PDF", FileNameFor("http://h/a/laporan-akhir.pdf?x=1", _ => false), "8.3 name from the address");
        c.Equal("UNDUHAN.BIN", FileNameFor("https://h/", _ => false), "a name for an address without a file");
        c.Equal("LAPORAN1.PDF", FileNameFor("http://h/laporan.pdf", n => n == "LAPORAN.PDF"), "a taken name gets a number");
        c.Equal("3.2 MB", Size(3 * 1024 * 1024 + 220 * 1024), "sizes without format strings");

        string dir = Files.ScratchDirectory + "/UNDUHTES";
        foreach (var e in Files.List(dir)) Files.Delete(e.Path);
        Files.CreateDirectory(dir);
        bool stop = false;
        TcpServer? server = null;
        try { server = new TcpServer(TestPort); } catch (Exception e) { c.That(false, "listen on " + TestPort + ": " + e.Message); }
        if (server != null)
        {
            var th = new Thread(() => Serve(server, () => stop));
            th.Start();
            try
            {
                var want = TestData();
                var app = new UnduhApp(true, dir) { Synchronous = true };
                var a = app.Add("http://localhost:" + TestPort + "/data.bin")!;
                app.RunPending();
                var got = Files.ReadAllBytes(a.Path);
                bool same = got != null && got.Length == want.Length && got[0] == want[0] && got[123457] == want[123457] && got[599999] == want[599999];
                c.That(a.State == JobState.Done && same, "downloaded to " + a.Name + ": " + Describe(a));

                var b = app.Add("http://localhost:" + TestPort + "/data.bin")!;
                b.PauseAt = 1;   // stop after the first piece written
                app.RunPending();
                c.That(b.State == JobState.Paused && b.Saved > 0 && b.Saved < want.Length && b.Name != a.Name, "paused at " + b.Saved + " bytes (" + b.Name + ")");
                b.PauseAt = 0;
                app.Resume(b);
                app.RunPending();
                got = Files.ReadAllBytes(b.Path);
                c.That(b.State == JobState.Done && got != null && got.Length == want.Length && got[150001] == want[150001], "resumed and complete: " + Describe(b));

                var d = app.Add("http://localhost:" + TestPort + "/hilang.bin")!;
                app.RunPending();
                c.That(d.State == JobState.Failed && d.Error.Contains("404"), "a missing file fails: " + Describe(d));

                var again = new UnduhApp(true, dir) { Synchronous = true };
                c.That(again.Jobs.Count == 3 && again.Jobs[0].State == JobState.Done && again.Jobs[0].Saved == want.Length && again.Jobs[2].State == JobState.Failed,
                    "the list survives a restart (" + again.Jobs.Count + " jobs)");
                again.W.RenderFrame();
                int texts = 0;
                foreach (var x in again.W.Descendants()) if (x is TextBlock t && t.Text.StartsWith("Selesai")) texts++;
                c.That(texts == 2, "the list shows two finished downloads (" + texts + ")");
                again.Remove(again.Jobs[2]);
                c.That(again.Jobs.Count == 2, "a failed download is taken off the list");
            }
            catch (Exception e) { c.That(false, "download: " + e.Message); }
            stop = true;
            th.Join();
        }
        foreach (var e in Files.List(dir)) Files.Delete(e.Path);
        Files.Delete(dir);
        return c;
    }

    public static (Window, UnduhApp) Build(bool headless = false)
    {
        var a = new UnduhApp(headless);
        return (a.W, a);
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        // `munduh --fetch URL PATH`: download without a window and exit 0 on
        // success -- the kernel's package repository uses it for https.
        int fi = Array.IndexOf(args, "--fetch");
        if (fi >= 0 && fi + 2 < args.Length) return Fetch(args[fi + 1], args[fi + 2]);
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = UnduhApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, a) = UnduhApp.Build();
        if (args.Length > 0 && args[0].Contains("://")) a.Add(args[0]);
        app.Run(w);
        return 0;
    }

    static int Fetch(string url, string path)
    {
        try
        {
            Files.Delete(path);
            var r = new HttpClient().Download(url, path, null, null, false);
            Console.WriteLine("munduh: " + url + " -> " + path + ": " + r.Status + " " + r.Reason + ", " + r.Saved + " bytes" + (r.Tls.Length > 0 ? ", " + r.Tls : ""));
            return r.Status == 200 && !r.Cancelled ? 0 : 1;
        }
        catch (Exception e)
        {
            Console.WriteLine("munduh: " + url + ": " + e.Message);
            return 2;
        }
    }
}
