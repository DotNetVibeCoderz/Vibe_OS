// Asisten AI -- the OS's built-in AI in one app (Buitenzorg.AI):
// - Cerita: a local language model (/disk/STORIES.GGU, stories260K) continues
//   the prompt; the answer appears token by token (one Generation.Next per
//   UI tick), with the speed and where it ran (GPU compute backend or CPU);
// - Penglihatan: edge detection (Sobel), grayscale and a brightness
//   histogram on a photo from /disk, done by the host.
// Everything runs on the machine -- no network. Buitenzorg.UI.

using Buitenzorg.AI;
using Buitenzorg.App;
using Buitenzorg.Media;
using Buitenzorg.UI;
using Window = Buitenzorg.UI.Window;

namespace MAsisten;

/// <summary>Brightness histogram as bars.</summary>
public sealed class HistogramView : Control
{
    int[] _bins = new int[0];
    public void Show(int[] bins) { _bins = bins; InvalidateVisual(); }
    protected override Size MeasureOverride(Size a) => new Size(Math.Min(a.Width, 520), 56);
    public override void Render(DrawingContext dc)
    {
        if (_bins.Length == 0) return;
        var b = Bounds;
        int max = 1;
        foreach (int v in _bins) max = Math.Max(max, v);
        int w = Math.Max(2, b.Width / _bins.Length - 2);
        for (int i = 0; i < _bins.Length; i++)
        {
            int h = Math.Max(2, (int)((long)_bins[i] * b.Height / max));
            int shade = 0x30 + i * 0xC0 / Math.Max(1, _bins.Length - 1);
            dc.FillRoundRect(new Rect(b.X + i * (w + 2), b.Y + b.Height - h, w, h), 2, shade << 16 | shade << 8 | shade);
        }
    }
}

public sealed class AsistenApp
{
    public Window W { get; }
    public LanguageModel? Model { get; private set; }
    public readonly TextBox Prompt = new TextBox { Watermark = "Awal cerita (bahasa Inggris), mis. Once upon a time" };
    public readonly TextBlock Answer = new TextBlock("") { TextWrapping = TextWrapping.Wrap };
    readonly TextBlock _modelInfo = new TextBlock("");
    readonly TextBlock _status = new TextBlock("");
    readonly CheckBox _varied = new CheckBox("Variasi (suhu 0.8)");
    readonly Slider _length = new Slider { Minimum = 16, Maximum = 160, Value = 64, Width = 160 };
    readonly TextBlock _lengthText = new TextBlock("64 token");
    readonly Image _photo = new Image { Height = 170 };
    readonly Image _result = new Image { Height = 170 };
    readonly HistogramView _hist = new HistogramView();
    readonly TextBlock _visionInfo = new TextBlock("");
    readonly DispatcherTimer _timer = new DispatcherTimer(30);
    Generation? _gen;
    int[] _pixels = new int[0];
    int _pw, _ph;
    public string Status => _status.Text;
    public bool Busy => _gen != null && !_gen.IsDone;

    public AsistenApp(bool headless)
    {
        W = new Window("Asisten AI", 640, 520, headless);
        var tabs = new TabControl();
        tabs.Add("Cerita", StoryPage(), IconKind.File);
        tabs.Add("Penglihatan", VisionPage(), IconKind.Sun);
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("AI lokal", "Asisten AI", IconKind.Leaf, 0x9BE0B8).DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(tabs);
        W.Content = root;
        _timer.Tick += Tick;
        LoadModel();
        LoadPhoto();
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    Control StoryPage()
    {
        var write = new Button("Tulis", _ => Write()).WithClass("accent");
        var stop = new Button("Berhenti", _ => Stop()).WithClass("subtle");
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(0, 8, 0, 8) };
        row.Children.Add(write);
        row.Children.Add(stop);
        row.Children.Add(_varied);
        _length.ValueChanged += s => _lengthText.Text = (int)s.Value + " token";
        var len = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        len.Children.Add(new TextBlock("Panjang"));
        len.Children.Add(_length);
        len.Children.Add(_lengthText);
        var page = new DockPanel { Margin = new Thickness(0, 10, 0, 0) };
        page.Children.Add(_modelInfo.WithClass("faint").DockTo(Dock.Top));
        page.Children.Add(Prompt.DockTo(Dock.Top));
        page.Children.Add(row.DockTo(Dock.Top));
        page.Children.Add(len.DockTo(Dock.Top));
        page.Children.Add(new ScrollViewer { Content = Answer, Margin = new Thickness(0, 10, 0, 0) });
        Prompt.KeyDown += (_, e) => { if (e.Key == '\r' || e.Key == '\n') { Write(); e.Handled = true; } };
        return page;
    }

    Control VisionPage()
    {
        var pics = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12, Margin = new Thickness(0, 10, 0, 10) };
        pics.Children.Add(_photo);
        pics.Children.Add(_result);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        row.Children.Add(new Button("Tepi (Sobel)", _ => Edges()).WithClass("accent"));
        row.Children.Add(new Button("Abu-abu", _ => Gray()).WithClass("subtle"));
        row.Children.Add(new Button("Foto berikutnya", _ => LoadPhoto(true)).WithClass("subtle"));
        var page = new StackPanel { Margin = new Thickness(0, 10, 0, 0), Spacing = 6 };
        page.Children.Add(row);
        page.Children.Add(pics);
        page.Children.Add(new TextBlock("Histogram kecerahan").WithClass("strong"));
        page.Children.Add(_hist);
        page.Children.Add(_visionInfo.WithClass("faint"));
        return page;
    }

    void LoadModel()
    {
        Model = LanguageModel.Load();
        if (Model == null) { _modelInfo.Text = "Model tidak bisa dimuat: " + LanguageModel.LastError; return; }
        _modelInfo.Text = "Model " + Model.Path + ": " + Model.Parameters / 1000 + " ribu parameter, " + Model.Layers + " lapisan, konteks "
            + Model.ContextLength + " token -- berjalan di " + Model.Backend + (Model.IsSimulated ? " (di luar perangkat)" : "");
        _status.Text = "Semua berjalan di mesin ini, tanpa internet. Perangkat komputasi: " + (ComputeDevice.Name.Length == 0 ? "CPU" : ComputeDevice.Name);
    }

    /// <summary>Start writing a continuation of the prompt.</summary>
    public bool Write()
    {
        if (Model == null) return false;
        string p = Prompt.Text.Trim();
        if (p.Length == 0) p = "Once upon a time";
        _gen = Model.Begin(p, (int)_length.Value, _varied.IsChecked == true ? 0.8 : 0);
        Answer.Text = p;
        _status.Text = "Menulis ...";
        _timer.Start();
        return true;
    }

    public void Stop()
    {
        if (_gen == null) return;
        _timer.Stop();
        Report();
        _gen = null;
    }

    /// <summary>Take the next pieces of the answer (the timer; tests call it).</summary>
    public void Tick()
    {
        if (_gen == null) return;
        var t0 = Environment.TickCount64;
        // a few tokens per tick, but give the UI back within ~25 ms
        do
        {
            var piece = _gen.Next();
            if (piece == null) { _timer.Stop(); Report(); _gen = null; return; }
            Answer.Text += piece;
        } while (Environment.TickCount64 - t0 < 25);
    }

    void Report()
    {
        if (Model == null) return;
        _status.Text = Model.LastTokens + " token dalam " + Model.LastMilliseconds + " ms ("
            + (int)Model.LastTokensPerSecond + " token/detik, " + Model.Backend + ")";
    }

    /// <summary>Run a whole generation now (tests, and the PC).</summary>
    public string Finish()
    {
        while (_gen != null) Tick();
        return Answer.Text;
    }

    // ---- vision ----------------------------------------------------------------------

    static readonly string[] Photos = { "/disk/KEBUN.JPG", "/disk/ANGGREK.PNG", "/disk/PHOTO.BMP", "/disk/WPNATURE.BMP" };
    int _photoIndex = -1;

    public bool LoadPhoto(bool next = false)
    {
        for (int tries = 0; tries < Photos.Length; tries++)
        {
            _photoIndex = (_photoIndex + 1) % Photos.Length;
            var pic = Files.GetInfo(Photos[_photoIndex]) == null ? null : Picture.Load(Photos[_photoIndex]);
            if (pic == null) continue;
            var (w, h) = pic.FitSize(280, 170, true);
            var px = new int[w * h];
            pic.Render(0, px, w, h, 0, 0, pic.Width, pic.Height);
            SetPhoto(px, w, h, Photos[_photoIndex]);
            return true;
        }
        // no photo on this machine (off the device): a drawn one
        int sw = 240, sh = 150;
        var b = Bitmap.Create(sw, sh, (x, y) =>
        {
            int dx = x - 120, dy = y - 75;
            if (dx * dx + dy * dy < 45 * 45) return 0xE9B44C;
            return y > 110 ? 0x3F7A58 : 0x7FB2D9 + (y / 3);
        });
        SetPhoto(b.Pixels, sw, sh, "gambar contoh");
        return false;
    }

    void SetPhoto(int[] px, int w, int h, string name)
    {
        _pixels = px; _pw = w; _ph = h;
        _photo.Source = Wrap(px, w, h);
        _result.Source = null;
        _hist.Show(Vision.Histogram(px, 24));
        _visionInfo.Text = name + ": " + w + " x " + h + " piksel, ketajaman " + Vision.Sharpness(px, w, h) + "/255";
    }

    static Bitmap Wrap(int[] px, int w, int h)
    {
        var b = new Bitmap(w, h);
        Array.Copy(px, b.Pixels, Math.Min(px.Length, b.Pixels.Length));
        return b;
    }

    public int[] Edges()
    {
        var e = Vision.Edges(_pixels, _pw, _ph);
        _result.Source = Wrap(e, _pw, _ph);
        return e;
    }

    public int[] Gray()
    {
        var g = Vision.Grayscale(_pixels, _pw, _ph);
        _result.Source = Wrap(g, _pw, _ph);
        return g;
    }

    public int PhotoWidth => _pw;
    public int PhotoHeight => _ph;
    public int[] PhotoPixels => _pixels;

    // ---- self-test -------------------------------------------------------------------

    public static Checks SelfTest()
    {
        var c = new Checks("Asisten AI");
        var app = new AsistenApp(true);
        var m = app.Model;
        c.That(m != null && m.Parameters > 0 && m.Layers > 0 && m.Vocabulary > 0,
            "model loaded: " + (m == null ? LanguageModel.LastError : m.Parameters + " parameters, " + m.Layers + " layers, " + m.Backend));
        if (m != null)
        {
            // the greedy continuation is fixed: the kernel's LLM test checks the same tokens
            string direct = m.Generate("Once upon a time", 16);
            c.That(direct.Length > 10, "Generate: \"" + direct.Trim() + "\"");
            c.That(m.IsSimulated || direct.StartsWith(", there was a little girl"), "the real model writes its known greedy text");
            app.Prompt.Text = "Once upon a time";
            app._length.Value = 16;
            bool started = app.Write();
            app.Tick();
            c.That(started && (app.Busy || app.Answer.Text.Length > "Once upon a time".Length), "Write starts a streamed answer");
            string text = app.Finish();
            c.That(text == "Once upon a time" + direct, "the streamed answer equals Generate (" + m.LastTokens + " tokens, " + (int)m.LastTokensPerSecond + " tok/s)");
            int n = 0;
            foreach (var _ in m.Stream("The cat", 5)) n++;
            c.That(n >= 1 && n <= 5 && m.LastTokens == n, "Stream yields at most the asked tokens (" + n + ")");
            c.That(m.CountTokens("Once upon a time") >= 3, "CountTokens (" + m.CountTokens("Once upon a time") + ")");
            var again = LanguageModel.Load();
            c.That(again != null && again.Parameters == m.Parameters, "loading the model again reuses it");
            c.That(LanguageModel.Load("/disk/TIDAKADA.GGU") == null || m.IsSimulated, "a missing model file is reported: " + LanguageModel.LastError);
        }
        // vision: a square on black has edges on its border, none inside or outside
        int w = 32, h = 32;
        var sq = new int[w * h];
        for (int y = 8; y < 24; y++) for (int x = 8; x < 24; x++) sq[y * w + x] = 0xFFFFFF;
        var e = Vision.Edges(sq, w, h);
        c.That((e[8 * w + 16] & 0xFF) > 100 && (e[16 * w + 16] & 0xFF) == 0 && (e[2 * w + 2] & 0xFF) == 0, "Sobel edges: border bright, flat areas dark");
        var hist = Vision.Histogram(sq, 4);
        c.That(hist[0] == w * h - 256 && hist[3] == 256, "histogram: " + hist[0] + " dark, " + hist[3] + " bright");
        var g = Vision.Grayscale(new[] { 0xFF0000, 0x00FF00 }, 2, 1);
        c.That((g[0] & 0xFF) == ((g[0] >> 16) & 0xFF) && (g[1] & 0xFF) > (g[0] & 0xFF), "grayscale (green brighter than red)");
        var pe = app.Edges();
        c.That(pe.Length == app.PhotoWidth * app.PhotoHeight && app.PhotoWidth > 0, "edges of the photo (" + app.PhotoWidth + " x " + app.PhotoHeight + ")");
        return c;
    }

    public static (Window, AsistenApp) Build(bool headless = false)
    {
        var a = new AsistenApp(headless);
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
            var c = AsistenApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = AsistenApp.Build();
        app.Run(w);
        return 0;
    }
}
