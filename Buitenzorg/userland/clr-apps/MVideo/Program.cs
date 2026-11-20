// Video -- plays AVI files with Motion-JPEG pictures and PCM sound (frames
// JPEG-decoded and scaled by the host through Buitenzorg.Media.Picture), and
// Matroska / WebM and MP4 / QuickTime files through Buitenzorg.Media.Movie
// (VP8 or Motion JPEG pictures; Opus, Vorbis, MP3 or PCM sound, all decoded by
// the host). Sound goes to this program's mixer stream (AUDIO_WRITE) and the
// picture follows the sound's position (A/V sync). Original clips on every
// image: /disk/FAJAR.AVI (synthesized at build time), /disk/KEBUN.WEB (VP8 +
// Opus) and /disk/SENJA.MP4 (Motion JPEG + MP3).

using Buitenzorg.App;
using Buitenzorg.Media;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;
using Window = Buitenzorg.UI.Window;

namespace MVideo;

/// <summary>The parts of an AVI file a player needs.</summary>
public sealed class Avi
{
    public int Width, Height, MicrosPerFrame;
    public string Title = "";
    public byte[] Data = Array.Empty<byte>();
    public readonly List<(int Offset, int Size)> Frames = new List<(int, int)>();
    public readonly List<(int Offset, int Size)> Sound = new List<(int, int)>();
    public int AudioRate, AudioChannels, AudioBits;
    public string Error = "";
    /// <summary>A Matroska / WebM / MP4 file: the host decodes its frames.</summary>
    public Movie? Native;
    public string Codecs = "";

    /// <summary>A host-read movie as a clip: the sound decoded whole, in quarter-second chunks.</summary>
    public static Avi FromMovie(Movie m)
    {
        var a = new Avi { Native = m, Width = m.Width, Height = m.Height, Title = m.Title };
        a.MicrosPerFrame = m.Fps > 0 ? (int)(1_000_000 / m.Fps) : 100_000;
        a.Codecs = m.VideoCodec + (m.AudioCodec.Length > 0 ? " + " + m.AudioCodec : "");
        if (m.HasVideo) for (int i = 0; i < m.FrameCount; i++) a.Frames.Add((i, 0));
        if (m.AudioBytes > 0 && m.AudioRate > 0)
        {
            a.Data = m.Sound();
            a.AudioRate = m.AudioRate;
            a.AudioChannels = m.AudioChannels;
            a.AudioBits = 16;
            int chunk = m.AudioRate / 4 * m.AudioChannels * 2;
            for (int o = 0; o < a.Data.Length; o += chunk) a.Sound.Add((o, Math.Min(chunk, a.Data.Length - o)));
        }
        if (!m.HasVideo) a.Error = m.Unsupported.Length > 0 ? "tidak bisa diputar: " + m.Unsupported : "tidak ada gambar di berkas ini";
        return a;
    }

    public double Fps => MicrosPerFrame > 0 ? 1_000_000.0 / MicrosPerFrame : 0;
    public double Seconds => Frames.Count / Math.Max(0.001, Fps);
    public bool HasSound => Sound.Count > 0 && AudioRate > 0;

    static int U16(byte[] d, int o) => d[o] | d[o + 1] << 8;
    static int U32(byte[] d, int o) => d[o] | d[o + 1] << 8 | d[o + 2] << 16 | d[o + 3] << 24;
    static bool Is(byte[] d, int o, string id) => o + 4 <= d.Length && d[o] == id[0] && d[o + 1] == id[1] && d[o + 2] == id[2] && d[o + 3] == id[3];

    public static Avi Parse(byte[] d)
    {
        var a = new Avi { Data = d };
        if (d.Length < 12 || !Is(d, 0, "RIFF") || !Is(d, 8, "AVI ")) { a.Error = "bukan berkas AVI"; return a; }
        a.Walk(12, d.Length, "");
        if (a.Frames.Count == 0) a.Error = "tidak ada gambar MJPEG di berkas ini";
        return a;
    }

    /// <summary>Chunks and LISTs from p to end; `stream` = the stream type of the last strh.</summary>
    void Walk(int p, int end, string stream)
    {
        var d = Data;
        while (p + 8 <= end)
        {
            int size = U32(d, p + 4);
            if (size < 0 || p + 8 + size > d.Length) break;
            if (Is(d, p, "LIST"))
            {
                Walk(p + 12, p + 8 + size, stream);
                if (Is(d, p + 8, "strl")) stream = "";
            }
            else if (Is(d, p, "avih") && size >= 40)
            {
                MicrosPerFrame = U32(d, p + 8);
                Width = U32(d, p + 8 + 32);
                Height = U32(d, p + 8 + 36);
            }
            else if (Is(d, p, "strh") && size >= 8)
            {
                _lastType = Is(d, p + 8, "vids") ? "v" : Is(d, p + 8, "auds") ? "a" : "";
            }
            else if (Is(d, p, "strf") && _lastType == "a" && size >= 16)
            {
                if (U16(d, p + 8) == 1) { AudioChannels = U16(d, p + 10); AudioRate = U32(d, p + 12); AudioBits = U16(d, p + 22); }
            }
            else if (Is(d, p, "INAM"))
            {
                int n = 0;
                while (n < size && d[p + 8 + n] != 0) n++;
                Title = System.Text.Encoding.UTF8.GetString(d, p + 8, n);
            }
            else if (d[p + 2] == 'd' && (d[p + 3] == 'c' || d[p + 3] == 'b') && d[p] == '0') Frames.Add((p + 8, size));
            else if (d[p + 2] == 'w' && d[p + 3] == 'b' && d[p] == '0' && AudioRate > 0 && (AudioBits == 8 || AudioBits == 16)) Sound.Add((p + 8, size));
            p += 8 + size + (size & 1);
        }
    }

    string _lastType = "";

    public byte[] Frame(int i)
    {
        var (o, n) = Frames[i];
        var b = new byte[n];
        Array.Copy(Data, o, b, 0, n);
        return b;
    }
}

/// <summary>The picture: the current frame scaled into the control.</summary>
public sealed class VideoView : Control
{
    int[] _px = Array.Empty<int>();
    int _w, _h;
    public int Shown = -1;
    public string Message = "";

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(160, a.Width), Math.Max(90, a.Height));

    /// <summary>Decode frame `i` of `avi` into the view's buffer (the host decodes and scales).</summary>
    public bool Show(Avi avi, int i)
    {
        if (i == Shown || i < 0 || i >= avi.Frames.Count) return false;
        if (avi.Native != null)
        {
            var bn = Bounds;
            double sc = Math.Min((double)Math.Max(16, bn.Width) / Math.Max(1, avi.Width), (double)Math.Max(16, bn.Height) / Math.Max(1, avi.Height));
            int nw = Math.Max(1, (int)(avi.Width * sc)), nh = Math.Max(1, (int)(avi.Height * sc));
            if (_px.Length != nw * nh) _px = new int[nw * nh];
            if (!avi.Native.Frame(i, _px, nw, nh)) { Message = "bingkai " + i + " tidak terbaca"; return false; }
            _w = nw;
            _h = nh;
            Shown = i;
            InvalidateVisual();
            return true;
        }
        var pic = Picture.FromBytes(avi.Frame(i));
        if (pic == null) { Message = "bingkai " + i + " tidak terbaca"; return false; }
        var b = Bounds;
        var (w, h) = pic.FitSize(Math.Max(16, b.Width), Math.Max(16, b.Height), allowUp: true);
        if (_px.Length != w * h) _px = new int[w * h];
        _w = w; _h = h;
        pic.Render(0, _px, w, h, 0, 0, pic.Width, pic.Height);
        Shown = i;
        InvalidateVisual();
        return true;
    }

    /// <summary>A sum of the shown picture (tests: frames differ).</summary>
    public int Checksum
    {
        get
        {
            int sum = 0;
            for (int i = 0; i < _w * _h; i += 7) sum = sum * 31 + _px[i];
            return sum;
        }
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRect(b, 0x0B140F);
        if (_w > 0 && _w <= b.Width && _h <= b.Height) dc.Blit(b.X + (b.Width - _w) / 2, b.Y + (b.Height - _h) / 2, _w, _h, _px);
        if (Message.Length > 0) dc.Text(b.X + 12, b.Y + 12, Message, 0xE0E0E0);
    }
}

public sealed class VideoApp
{
    public Window W { get; }
    public readonly VideoView View = new VideoView();
    public readonly List<string> Paths = new List<string>();
    public readonly List<string> Names = new List<string>();
    public Avi? Clip { get; private set; }
    readonly ProgressBar _bar = new ProgressBar { Maximum = 1000, Height = 10 };
    readonly TextBlock _time = new TextBlock("0:00 / 0:00");
    readonly TextBlock _playText = new TextBlock("Putar");
    readonly DispatcherTimer _timer = new DispatcherTimer(40);
    readonly ListBox _list;
    public bool Playing { get; private set; }
    int _soundChunk;          // next sound chunk to queue
    long _soundBytes;         // bytes handed to the mixer
    double _base;             // seconds played before the last (re)start
    long _startMs;
    bool _clock;              // no streaming sound: the clock sets the position
    public int FramesShown;

    public VideoApp(bool headless)
    {
        W = new Window("Video", 760, 520, headless);
        Scan();
        _list = new ListBox { ItemsSource = Names, Width = 150 };
        _list.SelectionChanged += l => { if (l.SelectedIndex >= 0) Open(l.SelectedIndex); };
        var play = new Button(_playText, _ => Toggle()).WithClass("accent");
        var restart = new Button("Dari awal", _ => { Seek(0); if (!Playing) Toggle(); }).WithClass("subtle");
        _bar.PointerPressed += (_, e) => { var b = _bar.Bounds; if (b.Width > 0 && Clip != null) Seek(Clip.Seconds * (e.X - b.X) / b.Width); };
        var controls = new DockPanel { Margin = new Thickness(0, 8, 0, 0) };
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        buttons.Children.Add(play);
        buttons.Children.Add(restart);
        controls.Children.Add(buttons.DockTo(Dock.Left));
        _time.Margin = new Thickness(12, 4, 0, 0);
        controls.Children.Add(_time);
        var main = new DockPanel();
        main.Children.Add(controls.DockTo(Dock.Bottom));
        main.Children.Add(_bar.DockTo(Dock.Bottom));
        main.Children.Add(View);
        var root = new DockPanel { Margin = new Thickness(12) };
        root.Children.Add(_list.DockTo(Dock.Left));
        main.Margin = new Thickness(10, 0, 0, 0);
        root.Children.Add(main);
        W.Content = root;
        W.UnhandledKey += e => { if (e.Key == ' ') { Toggle(); e.Handled = true; } };
        _timer.Tick += Tick;
        Application.Current?.Apply(W);
        W.UpdateLayout();
        if (Paths.Count > 0) _list.SelectedIndex = 0; else View.Message = "Tidak ada berkas video (.AVI, .MP4, .WEBM, .MKV, .MOV) di /disk";
    }

    public void Scan()
    {
        Paths.Clear(); Names.Clear();
        var dirs = Files.ScratchDirectory == "/disk" ? new[] { "/disk" } : new[] { "/disk", Files.ScratchDirectory };
        foreach (var d in dirs)
        {
            List<FileEntry> files;
            try { files = Files.List(d); } catch (Exception) { continue; }
            foreach (var f in files)
                if (!f.IsDirectory && (f.Name.ToUpperInvariant().EndsWith(".AVI") || Movie.IsMovieName(f.Name))) { Paths.Add(f.Path); Names.Add(f.Name); }
        }
    }

    public bool Open(int i)
    {
        var b = Files.ReadAllBytes(Paths[i]);
        if (b == null) return Load(new Avi { Error = "berkas tidak terbaca" });
        if (Movie.IsMovieName(Paths[i]))
        {
            Clip?.Native?.Close();
            var m = Movie.Open(b);
            return Load(m == null ? new Avi { Error = Movie.LastError } : Avi.FromMovie(m));
        }
        return Load(Avi.Parse(b));
    }

    public bool Load(Avi avi)
    {
        Stop();
        Clip = avi;
        if (avi.Error.Length > 0) { View.Message = avi.Error; return false; }
        View.Message = "";
        View.Shown = -1;
        View.Show(avi, 0);
        Refresh();
        return true;
    }

    /// <summary>Seconds into the clip: the sound actually played, else the clock.</summary>
    public double Position
    {
        get
        {
            if (Clip == null) return 0;
            if (!Playing) return _base;
            if (_clock) return Math.Min(Clip.Seconds, _base + (Environment.TickCount64 - _startMs) / 1000.0);
            int bytesPerSec = Clip.AudioRate * Clip.AudioChannels * Clip.AudioBits / 8;
            double queued = Audio.QueuedMs / 1000.0;
            return Math.Max(0, Math.Min(Clip.Seconds, _base + _soundBytes / (double)Math.Max(1, bytesPerSec) - queued));
        }
    }

    public void Toggle()
    {
        if (Clip == null || Clip.Error.Length > 0) return;
        if (Playing) { _base = Position; Playing = false; _timer.Stop(); Session.KeepAwake(false); Refresh(); return; }
        if (_base >= Clip.Seconds - 0.05) Seek(0);
        Playing = true;
        _clock = !Clip.HasSound || !Audio.CanStream;
        _startMs = Environment.TickCount64;
        _soundBytes = 0;
        _soundChunk = ChunkAt(_base);
        Session.KeepAwake(true);
        _timer.Start();
        Tick();
    }

    void Stop() { Playing = false; _timer.Stop(); _base = 0; Session.KeepAwake(false); }

    public void Seek(double seconds)
    {
        if (Clip == null) return;
        bool was = Playing;
        if (was) { Playing = false; _timer.Stop(); }
        _base = Math.Clamp(seconds, 0, Clip.Seconds);
        View.Show(Clip, Math.Min(Clip.Frames.Count - 1, (int)(_base * Clip.Fps)));
        if (was) Toggle(); else Refresh();
    }

    int ChunkAt(double seconds)
    {
        if (Clip == null || !Clip.HasSound) return 0;
        // chunks are equal-sized in practice: walk until the byte position is reached
        long want = (long)(seconds * Clip.AudioRate) * Clip.AudioChannels * Clip.AudioBits / 8, at = 0;
        for (int i = 0; i < Clip.Sound.Count; i++) { if (at + Clip.Sound[i].Size > want) return i; at += Clip.Sound[i].Size; }
        return Clip.Sound.Count;
    }

    /// <summary>Every 40 ms: keep ~0.5 s of sound queued, show the frame for the position.</summary>
    public void Tick()
    {
        if (Clip == null || !Playing) return;
        if (!_clock)
        {
            int fmt = Audio.Format(Clip.AudioRate, Clip.AudioChannels, Clip.AudioBits);
            int guard = 0;
            while (_soundChunk < Clip.Sound.Count && Audio.QueuedMs < 500 && guard++ < 8)
            {
                var (o, n) = Clip.Sound[_soundChunk];
                int took = Audio.Write(Clip.Data, o, n, fmt);
                if (took <= 0) break;
                _soundBytes += took;
                if (took < n) break; // the rest next time (rare: chunks are small)
                _soundChunk++;
            }
        }
        double pos = Position;
        int frame = Math.Min(Clip.Frames.Count - 1, (int)(pos * Clip.Fps));
        if (View.Show(Clip, frame)) FramesShown++;
        bool ended = _clock ? pos >= Clip.Seconds - 0.001 : (_soundChunk >= Clip.Sound.Count && Audio.QueuedMs == 0);
        if (ended) { _base = Clip.Seconds; Playing = false; _timer.Stop(); Session.KeepAwake(false); }
        Refresh();
    }

    static string Clock(double s) { int t = (int)s; return t / 60 + ":" + (t % 60 < 10 ? "0" : "") + t % 60; }

    void Refresh()
    {
        if (Clip == null) return;
        double pos = Position;
        _time.Text = Clock(pos) + " / " + Clock(Clip.Seconds) + "   " + (Clip.Title.Length > 0 ? Clip.Title + "   " : "") + Clip.Width + "x" + Clip.Height + ", " + (int)Math.Round(Clip.Fps) + " fps" + (Clip.HasSound ? ", suara " + Clip.AudioRate / 1000 + " kHz" : ", tanpa suara") + (Clip.Codecs.Length > 0 ? "   (" + Clip.Codecs + ")" : "");
        _bar.Value = Clip.Seconds > 0 ? pos * 1000 / Clip.Seconds : 0;
        _playText.Text = Playing ? "Jeda" : "Putar";
    }

    // ---- tests ----------------------------------------------------------------

    /// <summary>A tiny AVI built here: `frames` BMP pictures as '00dc' chunks (the
    /// player hands any picture format to Picture), plus a PCM track.</summary>
    public static byte[] SampleAvi(int frames)
    {
        var o = new List<byte>();
        void Id(string s) { foreach (char ch in s) o.Add((byte)ch); }
        void U32(int v) { o.Add((byte)v); o.Add((byte)(v >> 8)); o.Add((byte)(v >> 16)); o.Add((byte)(v >> 24)); }
        void U16(int v) { o.Add((byte)v); o.Add((byte)(v >> 8)); }
        Id("RIFF"); U32(0); Id("AVI ");
        Id("avih"); U32(56); U32(100_000); for (int i = 0; i < 7; i++) U32(0); U32(8); U32(4); for (int i = 0; i < 4; i++) U32(0);
        Id("strh"); U32(8); Id("auds"); U32(0);
        Id("strf"); U32(16); U16(1); U16(1); U32(8000); U32(16000); U16(2); U16(16);
        Id("INAM"); U32(4); Id("Uji"); o.Add(0);
        for (int f = 0; f < frames; f++)
        {
            var bmp = Bmp(f);
            Id("00dc"); U32(bmp.Length); o.AddRange(bmp);
            Id("01wb"); U32(1600); for (int i = 0; i < 1600; i++) o.Add((byte)(i % 7 == 0 ? 40 : 0));
        }
        var a = o.ToArray();
        int size = a.Length - 8;
        a[4] = (byte)size; a[5] = (byte)(size >> 8); a[6] = (byte)(size >> 16); a[7] = (byte)(size >> 24);
        return a;
    }

    static byte[] Bmp(int f)
    {
        int w = 8, h = 4, stride = 24, size = 54 + stride * h;
        var b = new byte[size];
        b[0] = (byte)'B'; b[1] = (byte)'M'; b[2] = (byte)size; b[10] = 54; b[14] = 40; b[18] = (byte)w; b[22] = (byte)h; b[26] = 1; b[28] = 24;
        for (int i = 54; i < size; i++) b[i] = (byte)(f * 20);
        return b;
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Video");
        var avi = Avi.Parse(SampleAvi(10));
        c.That(avi.Error.Length == 0 && avi.Frames.Count == 10 && avi.Sound.Count == 10 && avi.Width == 8 && avi.Height == 4, "AVI parsed: 10 frames, 10 sound chunks, 8x4");
        c.That(Math.Abs(avi.Fps - 10) < 0.01 && avi.AudioRate == 8000 && avi.AudioBits == 16 && avi.Title == "Uji", "10 fps, 8 kHz 16-bit sound, title");
        c.That(Avi.Parse(new byte[] { 1, 2, 3 }).Error.Length > 0, "not an AVI -> an error");
        var app = new VideoApp(headless: true);
        c.That(app.Load(avi) && app.View.Shown == 0, "loaded; frame 1 shown");
        app.Seek(0.55);
        c.That(app.View.Shown == 5, "seek to 0.55 s shows frame 6");
        app.Toggle();
        c.That(app.Playing, "playing");
        app.Toggle();
        c.That(!app.Playing, "paused");
        app.W.RenderFrame();
        c.That(app.W.LastFrameOps > 5, "drew " + app.W.LastFrameOps + " ops");
        int fajar = app.Names.FindIndex(n => n.ToUpperInvariant() == "FAJAR.AVI");
        if (fajar >= 0)
        {
            c.That(app.Open(fajar) && app.Clip!.Frames.Count == 120 && app.Clip.HasSound, "FAJAR.AVI: " + app.Clip?.Frames.Count + " MJPEG frames, sound " + app.Clip?.AudioRate + " Hz");
            int decoded = 0;
            for (int i = 0; i < 120; i += 30) { app.View.Shown = -1; if (app.View.Show(app.Clip!, i)) decoded++; }
            c.That(decoded == 4, "MJPEG frames decoded (" + decoded + " of 4 sampled)");
        }
        // host-read movies (on the device): VP8 + Opus in WebM, Motion JPEG + MP3 in MP4
        string[] mfiles = { "KEBUN.WEB", "SENJA.MP4" };
        string[] mcodecs = { "V_VP8", "mp4v" };
        int[] mrates = { 48000, 44100 };
        for (int mi = 0; mi < mfiles.Length; mi++)
        {
            string file = mfiles[mi], codec = mcodecs[mi];
            int rate = mrates[mi];
            int k = -1;
            for (int n = 0; n < app.Names.Count; n++) if (app.Names[n].ToUpperInvariant() == file) k = n;
            if (k < 0) continue;
            bool opened = app.Open(k);
            var clip = app.Clip!;
            c.That(opened && clip.Native != null && clip.Frames.Count >= 40 && clip.HasSound && clip.AudioRate == rate && clip.Native.VideoCodec == codec,
                file + ": " + clip.Frames.Count + " frames " + (clip.Native == null ? "?" : clip.Native.VideoCodec) + ", sound " + clip.AudioRate + " Hz " + clip.Error);
            int ok = 0, distinct = 0, last = 0;
            for (int i = 0; i < 31; i += 15)
            {
                app.View.Shown = -1;
                if (!app.View.Show(clip, i)) continue;
                ok++;
                int sum = app.View.Checksum;
                if (sum != last) distinct++;
                last = sum;
            }
            c.That(ok == 3 && distinct == 3, file + ": frames 1, 16, 31 decoded and differ (" + ok + " / " + distinct + ")");
            if (file == "KEBUN.WEB") c.That(clip.Title == "Kebun Fraktal", "WebM title: " + clip.Title);
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
            var c = VideoApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var v = new VideoApp(headless: false);
        app.Run(v.W);
        return 0;
    }
}
