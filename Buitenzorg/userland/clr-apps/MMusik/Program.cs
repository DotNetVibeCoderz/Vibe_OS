// Musik -- a music player with a library: every song on /disk, in its
// folders (two levels, e.g. /disk/MUSIK/ALBUM) and on plugged-in USB sticks
// (/usb0..3), sorted by title and searchable; play / pause / previous / next, a seekable progress bar, this
// program's volume in the kernel mixer, and a level meter. It plays through
// Buitenzorg.Media.MediaPlayer, which keeps the program's mixer stream fed
// from a UI timer -- other programs' sounds mix in alongside. Buitenzorg.UI.

using Buitenzorg.App;
using Buitenzorg.Media;
using Buitenzorg.UI;
using Window = Buitenzorg.UI.Window;

namespace MMusik;

public sealed class Track
{
    public string Path { get; }
    public string Title { get; set; }
    public double Seconds { get; set; }
    /// <summary>A track held in memory (tests, generated sounds) instead of a file.</summary>
    public byte[]? Data { get; init; }
    public Track(string path, string title) { Path = path; Title = title; }
    public override string ToString() => Title + (Seconds > 0 ? "   " + Clock(Seconds) : "");
    public static string Clock(double s) => ((int)s / 60) + ":" + ((int)s % 60).ToString("00");
}

/// <summary>Bars that follow the player's level (a peak meter, not a spectrum).</summary>
public sealed class Meter : Control
{
    readonly int[] _bars = new int[24];
    int _k;
    public void Push(int level)
    {
        _bars[_k++ % _bars.Length] = level;
        InvalidateVisual();
    }
    protected override Size MeasureOverride(Size a) => new Size(Math.Min(a.Width, 420), 56);
    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        int w = Math.Max(2, b.Width / _bars.Length - 3);
        for (int i = 0; i < _bars.Length; i++)
        {
            int v = _bars[(_k + i) % _bars.Length];
            int h = Math.Max(3, v * b.Height / 100);
            dc.FillRoundRect(new Rect(b.X + i * (w + 3), b.Y + b.Height - h, w, h), 2, v > 70 ? 0xF2B544 : 0x7CC4A4);
        }
    }
}

public sealed class MusikApp
{
    public readonly MediaPlayer Player = new MediaPlayer();
    /// <summary>The songs shown (the library, filtered by the search box).</summary>
    public readonly ObservableCollection<Track> Tracks = new ObservableCollection<Track>();
    /// <summary>Every song found by <see cref="Scan"/>.</summary>
    public readonly List<Track> Library = new List<Track>();
    readonly TextBox _search = new TextBox { Watermark = "Cari lagu..." };
    readonly TextBlock _count = new TextBlock("");
    public int Current { get; private set; } = -1;
    public Window W { get; }
    readonly ListBox _list;
    readonly TextBlock _title = new TextBlock("Pilih lagu") { };
    readonly TextBlock _time = new TextBlock("0:00 / 0:00");
    readonly ProgressBar _bar = new ProgressBar { Maximum = 1000, Height = 10 };
    readonly Button _play;
    readonly TextBlock _playText = new TextBlock("Putar");
    readonly Meter _meter = new Meter();
    readonly Slider _volume = new Slider { Maximum = 100, Value = 100, Width = 160 };
    readonly DispatcherTimer _timer = new DispatcherTimer(100);
    bool _syncing;

    public MusikApp(bool headless)
    {
        W = new Window("Musik", 520, 520, headless);
        _list = new ListBox { ItemsSource = Tracks, Height = 190 };
        _list.SelectionChanged += l => { if (!_syncing && l.SelectedIndex >= 0) PlayTrack(l.SelectedIndex); };
        _play = new Button(_playText, _ => Toggle()).WithClass("accent");
        var prev = new Button("Sebelumnya", _ => Step(-1)).WithClass("subtle");
        var next = new Button("Berikutnya", _ => Step(1)).WithClass("subtle");
        _volume.ValueChanged += s => Audio.AppVolume(0, (int)s.Value);
        _bar.PointerPressed += (_, e) => SeekTo(e.X);

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(0, 10, 0, 6) };
        buttons.Children.Add(prev);
        buttons.Children.Add(_play);
        buttons.Children.Add(next);
        var vol = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        vol.Children.Add(new TextBlock("Volume app ini").WithClass("faint"));
        vol.Children.Add(_volume);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Musik", IconKind.Play, 0xB99AE8).DockTo(Dock.Top));
        root.Children.Add(_title.WithClass("strong").DockTo(Dock.Top));
        root.Children.Add(_bar.DockTo(Dock.Top));
        root.Children.Add(_time.WithClass("faint").DockTo(Dock.Top));
        root.Children.Add(buttons.DockTo(Dock.Top));
        root.Children.Add(_meter.DockTo(Dock.Top));
        root.Children.Add(vol.DockTo(Dock.Bottom));
        var lib = new DockPanel { Margin = new Thickness(0, 4, 0, 4) };
        lib.Children.Add(_count.WithClass("faint").DockTo(Dock.Right));
        lib.Children.Add(_search);
        _search.TextChanged += t => Filter(t.Text);
        root.Children.Add(lib.DockTo(Dock.Top));
        root.Children.Add(_list);
        W.Content = root;
        W.UnhandledKey += e => { if (e.Key == ' ') { Toggle(); e.Handled = true; } };

        Player.Ended += () => Step(1);
        _timer.Tick += Tick;
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    public static bool IsSong(string name)
    {
        string up = name.ToUpperInvariant();
        return up.EndsWith(".WAV") || up.EndsWith(".FLA") || up.EndsWith(".FLAC") || up.EndsWith(".MP3") || up.EndsWith(".MID") || up.EndsWith(".OGG")
            || up.EndsWith(".OGA") || up.EndsWith(".OPU") || up.EndsWith(".OPUS");
    }

    // system folders on /disk that never hold music
    static readonly string[] Skip = { "PKGHIST", "CRASH", "APPDATA", "LAYAR", "PROYEK", "UNDUHAN" };

    void ScanDir(string dir, int depth, HashSet<string> seen)
    {
        if (Library.Count >= 400) return;
        List<FileEntry> files;
        try { files = Files.List(dir); } catch (Exception) { return; }
        foreach (var f in files)
        {
            if (f.IsDirectory)
            {
                if (depth < 2 && Array.IndexOf(Skip, f.Name.ToUpperInvariant()) < 0) ScanDir(f.Path, depth + 1, seen);
                continue;
            }
            if (!IsSong(f.Name) || !seen.Add(f.Path.ToUpperInvariant())) continue;
            int dot = f.Name.LastIndexOf('.');
            Library.Add(new Track(f.Path, dot > 0 ? f.Name.Substring(0, dot) : f.Name));
        }
    }

    /// <summary>The library: songs on /disk (two folder levels deep), the scratch
    /// folder and USB sticks, sorted by title; then the search filter.</summary>
    public void Scan()
    {
        Library.Clear();
        var seen = new HashSet<string>();
        var roots = new List<string> { "/disk" };
        if (Files.ScratchDirectory != "/disk") roots.Add(Files.ScratchDirectory);
        for (int u = 0; u < 4; u++) roots.Add("/usb" + u);
        foreach (var r in roots) ScanDir(r, 0, seen);
        Library.Sort((a, b) => string.Compare(a.Title, b.Title, StringComparison.OrdinalIgnoreCase));
        Filter(_search.Text);
    }

    /// <summary>Show the songs whose title or path contains `q` (case-insensitive).</summary>
    public void Filter(string q)
    {
        _syncing = true;
        Tracks.Clear();
        string lq = (q ?? "").Trim().ToLowerInvariant();
        foreach (var t in Library)
            if (lq.Length == 0 || t.Title.ToLowerInvariant().Contains(lq) || t.Path.ToLowerInvariant().Contains(lq)) Tracks.Add(t);
        _syncing = false;
        Current = -1;
        _count.Text = Tracks.Count == Library.Count ? Library.Count + " lagu" : Tracks.Count + " dari " + Library.Count + " lagu";
    }

    public void Add(Track t) { Library.Add(t); Tracks.Add(t); }

    public bool PlayTrack(int i)
    {
        if (i < 0 || i >= Tracks.Count) return false;
        var t = Tracks[i];
        if (!(t.Data != null ? Player.Load(t.Data, t.Title) : Player.Load(t.Path))) { _title.Text = t.Title + ": bukan WAV / FLAC / MP3 / MIDI yang bisa diputar"; return false; }
        t.Title = Player.Title;
        t.Seconds = Player.Duration;
        Tracks[i] = t; // the list shows the title and the length now
        Current = i;
        _syncing = true;
        _list.SelectedIndex = i;
        _syncing = false;
        Player.Play();
        _timer.Start();
        Refresh();
        return true;
    }

    public void Toggle()
    {
        if (Player.State == PlayState.Playing) Player.Pause();
        else if (Current < 0) PlayTrack(0);
        else { Player.Play(); _timer.Start(); }
        Refresh();
    }

    public void Step(int d)
    {
        if (Tracks.Count == 0) return;
        PlayTrack(((Current < 0 ? 0 : Current + d) % Tracks.Count + Tracks.Count) % Tracks.Count);
    }

    void SeekTo(int x)
    {
        var b = _bar.Bounds;
        if (b.Width <= 0) return;
        Player.Seek(Player.Duration * (x - b.X) / b.Width);
        Refresh();
    }

    public void Tick()
    {
        Player.Pump();
        _meter.Push(Player.State == PlayState.Playing ? Player.Level : 0);
        Refresh();
        if (Player.State != PlayState.Playing) _timer.Stop();
    }

    void Refresh()
    {
        _title.Text = Current >= 0 ? Player.Title : "Pilih lagu";
        _time.Text = Track.Clock(Player.Position) + " / " + Track.Clock(Player.Duration) + (Audio.CanStream ? "" : "   (tanpa kartu suara yang bisa streaming)");
        _bar.Value = Player.Duration > 0 ? Player.Position * 1000 / Player.Duration : 0;
        _playText.Text = Player.State == PlayState.Playing ? "Jeda" : "Putar";
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Musik");
        // a WAV with a title, parsed back
        var tone = new short[8000];
        for (int i = 0; i < tone.Length; i++) tone[i] = (short)(Math.Sin(i * 2 * Math.PI * 440 / 8000) * 12000);
        var wav = Wav.Build(tone, 8000, "Nada Uji");
        var info = Wav.Parse(wav);
        c.That(info != null && info.Rate == 8000 && info.Channels == 1 && info.Bits == 16 && info.Title == "Nada Uji" && Math.Abs(info.Seconds - 1.0) < 0.01,
               "WAV: 8 kHz mono 16-bit, 1 s, title from INAM");
        c.That(Wav.Parse(new byte[] { 1, 2, 3 }) == null, "not a WAV -> null");
        var p = new MediaPlayer();
        c.That(p.Load(wav, "x") && p.Title == "Nada Uji" && p.State == PlayState.Stopped, "the player loads it");
        p.Play();
        p.Pump();
        c.That(p.State == PlayState.Playing && p.Level > 20, "playing: the first chunk went to the mixer (level " + p.Level + ")");
        p.Pause();
        c.That(p.State == PlayState.Paused, "pause");
        p.Seek(0.5);
        c.That(Math.Abs(p.Position - 0.5) < 0.05, "seek to 0.5 s");
        p.Stop();
        c.That(p.Position == 0, "stop rewinds");

        // the library: a song in an album folder is found, the search narrows the list
        string album = Files.ScratchDirectory + "/MUSIK";
        Files.CreateDirectory(album);
        Files.CreateDirectory(album + "/KEBUN");
        Files.WriteAllBytes(album + "/KEBUN/UJIALBUM.WAV", wav);
        var lib = new MusikApp(headless: true);
        lib.Scan();
        bool inAlbum = lib.Library.Exists(t => t.Path.ToUpperInvariant().EndsWith("/MUSIK/KEBUN/UJIALBUM.WAV"));
        bool sorted = true;
        for (int i = 1; i < lib.Library.Count; i++) if (string.Compare(lib.Library[i - 1].Title, lib.Library[i].Title, StringComparison.OrdinalIgnoreCase) > 0) sorted = false;
        lib.Filter("ujialbum");
        c.That(inAlbum && sorted && lib.Tracks.Count == 1 && lib.Library.Count >= 1, "library: album folder scanned, sorted, search finds 1 of " + lib.Library.Count);
        lib.Filter("");
        Files.Delete(album + "/KEBUN/UJIALBUM.WAV");
        Files.Delete(album + "/KEBUN");
        Files.Delete(album);

        // the app around it (an in-memory track off the device)
        var app = new MusikApp(headless: true);
        app.Scan();
        int scanned = app.Tracks.Count;
        app.Add(new Track("(memori)", "uji") { Data = wav });
        int k = app.Tracks.Count - 1;
        c.That(app.Tracks.Count == scanned + 1, "the list: " + scanned + " WAV file(s) on the media + one in memory");
        c.That(app.PlayTrack(k) && app.Player.State == PlayState.Playing && app.Tracks[k].Title == "Nada Uji", "selecting it plays it");
        app.Toggle();
        c.That(app.Player.State == PlayState.Paused, "the play button pauses");
        app.W.RenderFrame();
        c.That(app.W.LastFrameOps > 20, "the player drew " + app.W.LastFrameOps + " ops");
        // on the device: the image's FLAC, decoded by the host (off the device there is no decoder)
        int fla = -1;
        for (int i = 0; i < app.Tracks.Count; i++) if (app.Tracks[i].Path.ToUpperInvariant().EndsWith("LAGU3.FLA")) fla = i;
        if (fla >= 0)
        {
            var fp = new MediaPlayer();
            bool ok = fp.Load(app.Tracks[fla].Path);
            c.That(ok && fp.Title == "Tetes Embun" && Math.Abs(fp.Duration - 10) < 0.2 && fp.Info!.Rate == 22050,
                "FLAC decoded: '" + fp.Title + "', " + (int)(fp.Duration * 10) / 10 + "." + (int)(fp.Duration * 10) % 10 + " s");
        }
        int mid = -1;
        for (int i = 0; i < app.Tracks.Count; i++) if (app.Tracks[i].Path.ToUpperInvariant().EndsWith("LAGU5.MID")) mid = i;
        if (mid >= 0)
        {
            var mm = new MediaPlayer();
            bool ok = mm.Load(app.Tracks[mid].Path);
            c.That(ok && mm.Title == "Kupu-kupu di Taman" && mm.Duration > 37 && mm.Duration < 41 && mm.Info!.Rate == 22050,
                "MIDI synthesized: '" + mm.Title + "', " + (int)mm.Duration + " s");
        }
        int mp3 = -1;
        for (int i = 0; i < app.Tracks.Count; i++) if (app.Tracks[i].Path.ToUpperInvariant().EndsWith("LAGU4.MP3")) mp3 = i;
        if (mp3 >= 0)
        {
            var mp = new MediaPlayer();
            bool ok = mp.Load(app.Tracks[mp3].Path);
            c.That(ok && mp.Title == "Gerimis" && Math.Abs(mp.Duration - 10) < 0.3 && mp.Info!.Rate == 22050,
                "MP3 decoded: '" + mp.Title + "', " + (int)(mp.Duration * 10) / 10 + "." + (int)(mp.Duration * 10) % 10 + " s");
        }
        // Ogg Vorbis and Ogg Opus, decoded in the host
        string[] oggFiles = { "LAGU6.OGG", "LAGU7.OPU" }, oggTitles = { "Angin Sore", "Embun Pagi" };
        int[] oggRates = { 22050, 48000 };
        int[] oggSecs = { 22, 20 };
        for (int f = 0; f < oggFiles.Length; f++)
        {
            int slot = -1;
            for (int i = 0; i < app.Tracks.Count; i++) if (app.Tracks[i].Path.ToUpperInvariant().EndsWith(oggFiles[f])) slot = i;
            if (slot < 0) continue;
            var og = new MediaPlayer();
            bool ok = og.Load(app.Tracks[slot].Path);
            c.That(ok && og.Title == oggTitles[f] && Math.Abs(og.Duration - oggSecs[f]) < 0.3 && og.Info!.Rate == oggRates[f],
                oggFiles[f] + " decoded: '" + og.Title + "', " + (int)og.Duration + " s at " + (og.Info == null ? 0 : og.Info.Rate) + " Hz");
        }
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
            var c = MusikApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var m = new MusikApp(headless: false);
        m.Scan();
        app.Run(m.W);
        return 0;
    }
}
