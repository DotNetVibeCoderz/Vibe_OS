// Perekam Suara -- the sound recorder: records from the microphone
// (Buitenzorg.Media.Recorder over AUDIO_READ, 48 kHz stereo), shows the time
// and a level meter, saves REKAMnnn.WAV in /disk/REKAMAN, lists the
// recordings and plays them back (MediaPlayer). A recording streams to a
// temporary file while it runs (FS_APPEND) and becomes a WAV with its real
// sizes when it stops; at most 60 s. Buitenzorg.UI.

using Buitenzorg.App;
using Buitenzorg.Media;
using Buitenzorg.UI;
using Window = Buitenzorg.UI.Window;

namespace MRekam;

public sealed class Recording
{
    public string Path = "", Name = "";
    public int Seconds;
    public override string ToString() => Name + "   " + Seconds / 60 + ":" + (Seconds % 60 < 10 ? "0" : "") + Seconds % 60;
}

/// <summary>Bars that follow the input level.</summary>
public sealed class LevelBars : Control
{
    readonly int[] _bars = new int[32];
    int _k;
    public void Push(int level) { _bars[_k++ % _bars.Length] = level; InvalidateVisual(); }
    public void Clear() { Array.Clear(_bars, 0, _bars.Length); InvalidateVisual(); }
    protected override Size MeasureOverride(Size a) => new Size(Math.Min(a.Width, 460), 64);
    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        int w = Math.Max(2, b.Width / _bars.Length - 3);
        for (int i = 0; i < _bars.Length; i++)
        {
            int v = _bars[(_k + i) % _bars.Length];
            int h = Math.Max(3, v * b.Height / 100);
            dc.FillRoundRect(new Rect(b.X + i * (w + 3), b.Y + b.Height - h, w, h), 2, v > 70 ? 0xE5735F : 0x7CC4A4);
        }
    }
}

public sealed class RekamApp
{
    public const int MaxSeconds = 60;
    const int BytesPerSecond = Recorder.Rate * Recorder.Channels * 2;
    public Window W { get; }
    public readonly ObservableCollection<Recording> Recordings = new ObservableCollection<Recording>();
    public string Folder { get; }
    public readonly Recorder Rec = new Recorder();
    public readonly MediaPlayer Player = new MediaPlayer();
    readonly TextBlock _time = new TextBlock("0:00");
    readonly TextBlock _status = new TextBlock("");
    readonly TextBlock _recText = new TextBlock("Rekam");
    readonly LevelBars _level = new LevelBars();
    readonly ListBox _list;
    readonly DispatcherTimer _timer = new DispatcherTimer(100);
    readonly byte[] _chunk = new byte[64 * 1024];
    string _temp = "";
    long _bytes;
    public string Status => _status.Text;

    public RekamApp(bool headless, string? folder = null)
    {
        Folder = folder ?? PickFolder();
        W = new Window("Perekam Suara", 520, 470, headless);
        _list = new ListBox { ItemsSource = Recordings, Height = 160 };
        var rec = new Button(_recText, _ => Toggle()).WithClass("accent");
        var play = new Button("Putar", _ => Play()).WithClass("subtle");
        var del = new Button("Hapus", _ => DeleteSelected()).WithClass("subtle");
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(0, 10, 0, 10) };
        buttons.Children.Add(rec);
        buttons.Children.Add(play);
        buttons.Children.Add(del);
        _time.FontSize = 24;
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Perekam Suara", IconKind.Play, 0xE5735F).DockTo(Dock.Top));
        root.Children.Add(_time.WithClass("strong").DockTo(Dock.Top));
        root.Children.Add(_level.DockTo(Dock.Top));
        root.Children.Add(buttons.DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(_list);
        W.Content = root;
        W.UnhandledKey += e => { if (e.Key == ' ') { Toggle(); e.Handled = true; } };
        _timer.Tick += Tick;
        Scan();
        _status.Text = "Disimpan ke " + Folder + " (maks " + MaxSeconds + " detik per rekaman)";
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    static string PickFolder()
    {
        const string d = "/disk/REKAMAN";
        if (Files.GetInfo(d) != null || Files.CreateDirectory(d)) return d;
        return Files.ScratchDirectory;
    }

    public void Scan()
    {
        Recordings.Clear();
        List<FileEntry> files;
        try { files = Files.List(Folder); } catch (Exception) { return; }
        foreach (var f in files)
        {
            string up = f.Name.ToUpperInvariant();
            if (f.IsDirectory || !up.StartsWith("REKAM") || !up.EndsWith(".WAV")) continue;
            Recordings.Add(new Recording { Path = f.Path, Name = up.Substring(0, up.Length - 4), Seconds = (int)Math.Max(0, (Files.Size(f.Path) - 44) / BytesPerSecond) });
        }
    }

    public bool IsRecording => Rec.Recording;

    public void Toggle()
    {
        if (Rec.Recording) Stop(); else Start();
    }

    public bool Start()
    {
        Player.Stop();
        if (!Rec.Start()) { _status.Text = "Tidak bisa merekam: " + Rec.Error; return false; }
        _temp = Folder + "/REKAMTMP.PCM";
        Files.Delete(_temp);
        _bytes = 0;
        _level.Clear();
        _recText.Text = "Berhenti";
        _status.Text = "Merekam ... (Spasi atau Berhenti untuk selesai)";
        _timer.Start();
        return true;
    }

    /// <summary>Move what was recorded to the temporary file (the timer; tests call it).</summary>
    public void Tick()
    {
        if (!Rec.Recording) return;
        int n, reads = 0;
        // bounded: a device that delivers faster than real time must not keep the UI here
        while (reads++ < 16 && (n = Rec.Read(_chunk)) > 0)
        {
            Files.AppendAllBytes(_temp, _chunk, 0, n);
            _bytes += n;
            _level.Push(Recorder.Level(_chunk, n));
        }
        int s = (int)(_bytes / BytesPerSecond);
        _time.Text = s / 60 + ":" + (s % 60 < 10 ? "0" : "") + s % 60;
        if (s >= MaxSeconds) Stop();
    }

    /// <summary>Stop and save: the WAV header with the real sizes, then the samples.</summary>
    public Recording? Stop()
    {
        if (!Rec.Recording) return null;
        Tick();
        Rec.Stop();
        _timer.Stop();
        _recText.Text = "Rekam";
        var pcm = Files.ReadAllBytes(_temp) ?? new byte[0];
        Files.Delete(_temp);
        if (pcm.Length < 4) { _status.Text = "Tidak ada suara yang terekam."; return null; }
        string name = NextName();
        string path = Folder + "/" + name + ".WAV";
        int data = pcm.Length - pcm.Length % 4;
        if (!Files.WriteAllBytes(path, Wav.Header(data, Recorder.Rate, Recorder.Channels)) || !Files.AppendAllBytes(path, pcm, 0, data))
        {
            _status.Text = "Menyimpan " + path + " gagal (disk penuh?)";
            return null;
        }
        var r = new Recording { Path = path, Name = name, Seconds = data / BytesPerSecond };
        Recordings.Add(r);
        _status.Text = "Disimpan: " + path + " (" + data / 1024 + " KB)";
        return r;
    }

    string NextName()
    {
        for (int n = 1; n < 1000; n++)
        {
            string name = "REKAM" + (n < 10 ? "00" : n < 100 ? "0" : "") + n;
            if (Files.GetInfo(Folder + "/" + name + ".WAV") == null) return name;
        }
        return "REKAM999";
    }

    public bool Play()
    {
        int i = _list.SelectedIndex >= 0 ? _list.SelectedIndex : Recordings.Count - 1;
        if (i < 0) return false;
        if (!Player.Load(Recordings[i].Path)) { _status.Text = "Tidak bisa memutar " + Recordings[i].Name; return false; }
        Player.Play();
        _status.Text = "Memutar " + Recordings[i].Name;
        return true;
    }

    public void DeleteSelected()
    {
        int i = _list.SelectedIndex;
        if (i < 0 || i >= Recordings.Count) return;
        Player.Stop();
        Files.Delete(Recordings[i].Path);
        _status.Text = "Dihapus: " + Recordings[i].Name;
        Recordings.RemoveAt(i);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Perekam");
        string dir = Files.ScratchDirectory + "/REKAMTES";
        foreach (var e in Files.List(dir)) Files.Delete(e.Path);
        Files.CreateDirectory(dir);
        var app = new RekamApp(true, dir);
        bool started = app.Start();
        if (!started && app.Rec.Error.Contains("tidak ada perangkat"))
        {
            // a board without a recording device: the app says so and does nothing else
            c.That(app.Status.Contains("tidak ada perangkat"), "no recording device: reported (" + app.Status + ")");
        }
        else
        {
            c.That(started && app.IsRecording, "recording started");
            long t0 = Environment.TickCount64;
            while (Environment.TickCount64 - t0 < 700)
            {
                System.Threading.Thread.Sleep(50);
                app.Tick();
            }
            var r = app.Stop();
            var bytes = r == null ? null : Files.ReadAllBytes(r.Path);
            var info = bytes == null ? null : Wav.Parse(bytes);
            int frames = info == null ? 0 : info.DataLength / 4;
            c.That(r != null && info != null && info.Rate == 48000 && info.Channels == 2 && frames > 48000 / 5,
                "saved a 48 kHz stereo WAV: " + (r == null ? "none" : r.Name) + ", " + frames + " frames");
            c.That(app.Recordings.Count == 1 && !app.IsRecording && Files.GetInfo(dir + "/REKAMTMP.PCM") == null, "listed, temporary file removed");
            c.That(app.Play() && app.Player.Duration > 0.15, "plays back (" + (int)(app.Player.Duration * 1000) + " ms)");
            app.Player.Stop();
            var again = new RekamApp(true, dir);
            c.That(again.Recordings.Count == 1 && again.Recordings[0].Name == "REKAM001", "the list is read back from the folder");
        }
        foreach (var e in Files.List(dir)) Files.Delete(e.Path);
        Files.Delete(dir);
        return c;
    }

    public static (Window, RekamApp) Build(bool headless = false)
    {
        var a = new RekamApp(headless);
        return (a.W, a);
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = RekamApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = RekamApp.Build();
        app.Run(w);
        return 0;
    }
}
