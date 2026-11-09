// Gambar -- the photo viewer: lists the pictures on /disk and the scratch
// folder (BMP, PNG, JPEG, GIF), shows the chosen one fitted to the window,
// zooms (buttons, + / -, the wheel) and pans by dragging, plays animated GIFs,
// and runs a slide show. Decoding and scaling happen in the host
// (Buitenzorg.Media.Picture), so the interpreter only blits finished pixels.

using Buitenzorg.UI;
using Buitenzorg.Media;
using Files = Buitenzorg.App.Files;

namespace MImage;

/// <summary>Shows one picture: fitted or zoomed, panned by dragging, animated.</summary>
public sealed class PhotoView : Control
{
    Picture? _pic;
    int[] _buf = Array.Empty<int>();
    string _key = "";
    int _dragX, _dragY;
    double _dragCx, _dragCy;
    bool _dragging;
    readonly DispatcherTimer _anim = new DispatcherTimer(100);

    /// <summary>0 = fit to the window, else the scale (1 = 100 %).</summary>
    public double Zoom { get; private set; }
    public double CenterX { get; private set; }
    public double CenterY { get; private set; }
    public int Frame { get; private set; }
    public string Name = "";
    public string Message = "Pilih gambar di kiri.";
    /// <summary>Size of the last picture drawn (for tests).</summary>
    public int ShownW, ShownH;
    public int Renders;

    public PhotoView()
    {
        Focusable = true;
        _anim.Tick += NextFrame;
    }

    public Picture? Source
    {
        get => _pic;
        set
        {
            _pic = value; Frame = 0; Zoom = 0; _key = "";
            if (value != null) { CenterX = value.Width / 2.0; CenterY = value.Height / 2.0; }
            _anim.Stop();
            if (value != null && value.FrameCount > 1) _anim.Start();
            InvalidateVisual();
        }
    }

    public void NextFrame()
    {
        if (_pic == null || _pic.FrameCount < 2) { _anim.Stop(); return; }
        Frame = (Frame + 1) % _pic.FrameCount;
        InvalidateVisual();
    }

    /// <summary>The scale "fit" works out to, for the zoom buttons.</summary>
    double FitScale()
    {
        if (_pic == null) return 1;
        var (aw, ah) = Area();
        return Math.Min(1.0, Math.Min((double)aw / _pic.Width, (double)ah / _pic.Height));
    }

    (int, int) Area() => (Math.Max(16, Bounds.Width - 16), Math.Max(16, Bounds.Height - 36));

    public void SetZoom(double z)
    {
        if (_pic == null) return;
        Zoom = z <= 0 ? 0 : Math.Clamp(z, 0.05, 16);
        if (Zoom > 0 && Math.Abs(Zoom - FitScale()) < 0.001) Zoom = 0;
        InvalidateVisual();
    }

    public void ZoomBy(double f) => SetZoom((Zoom == 0 ? FitScale() : Zoom) * f);

    public void PanBy(double dx, double dy)
    {
        if (_pic == null || Zoom == 0) return;
        CenterX = Math.Clamp(CenterX + dx, 0, _pic.Width);
        CenterY = Math.Clamp(CenterY + dy, 0, _pic.Height);
        InvalidateVisual();
    }

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(200, a.Width), Math.Max(150, a.Height));

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(b, 10, Theme.SurfaceLo);
        string caption = Message;
        if (_pic != null)
        {
            var (aw, ah) = Area();
            int dw, dh, sx, sy, sw, sh;
            double scale = Zoom == 0 ? FitScale() : Zoom;
            dw = Math.Max(1, Math.Min(aw, (int)(_pic.Width * scale)));
            dh = Math.Max(1, Math.Min(ah, (int)(_pic.Height * scale)));
            sw = Math.Max(1, (int)(dw / scale));
            sh = Math.Max(1, (int)(dh / scale));
            sx = Math.Clamp((int)(CenterX - sw / 2.0), 0, Math.Max(0, _pic.Width - sw));
            sy = Math.Clamp((int)(CenterY - sh / 2.0), 0, Math.Max(0, _pic.Height - sh));
            string key = dw + "," + dh + "," + sx + "," + sy + "," + sw + "," + sh + "," + Frame;
            int delay = 0;
            if (key != _key)
            {
                if (_buf.Length < dw * dh) _buf = new int[dw * dh];
                delay = _pic.Render(Frame, _buf, dw, dh, sx, sy, sw, sh);
                _key = delay < 0 ? "" : key;
                Renders++;
                if (delay > 0) _anim.IntervalMs = Math.Max(20, delay);
            }
            int x = b.X + (b.Width - dw) / 2, y = b.Y + 8 + (ah - dh) / 2;
            if (_key.Length > 0)
            {
                if (_buf.Length != dw * dh) { var exact = new int[dw * dh]; Array.Copy(_buf, exact, dw * dh); dc.Blit(x, y, dw, dh, exact); }
                else dc.Blit(x, y, dw, dh, _buf);
            }
            ShownW = dw; ShownH = dh;
            caption = Name + "   " + _pic.Format + " " + _pic.Width + " x " + _pic.Height
                    + "   " + (Zoom == 0 ? "pas (" + (int)Math.Round(scale * 100) + "%)" : (int)Math.Round(Zoom * 100) + "%")
                    + (_pic.FrameCount > 1 ? "   bingkai " + (Frame + 1) + "/" + _pic.FrameCount : "");
        }
        dc.Text(b.X + 10, b.Y + b.Height - 20, caption, Theme.TextFaint);
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        _dragging = Zoom > 0; _dragX = e.X; _dragY = e.Y; _dragCx = CenterX; _dragCy = CenterY;
        e.Handled = true;
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        if (!_dragging || !e.LeftButton || _pic == null || Zoom <= 0) return;
        CenterX = Math.Clamp(_dragCx - (e.X - _dragX) / Zoom, 0, _pic.Width);
        CenterY = Math.Clamp(_dragCy - (e.Y - _dragY) / Zoom, 0, _pic.Height);
        InvalidateVisual();
        e.Handled = true;
    }

    protected override void OnPointerReleased(PointerEventArgs e) { _dragging = false; }

    protected override void OnPointerWheelChanged(PointerEventArgs e)
    {
        if (e.WheelDelta == 0) return;
        ZoomBy(e.WheelDelta > 0 ? 1.25 : 0.8);
        e.Handled = true;
    }
}

public sealed class PhotoApp
{
    public Window W { get; }
    public readonly List<string> Paths = new List<string>();
    public readonly List<string> Names = new List<string>();
    public readonly PhotoView View = new PhotoView();
    readonly ListBox _list;
    readonly TextBlock _showText = new TextBlock("Slide show");
    readonly DispatcherTimer _slides = new DispatcherTimer(3000);
    public int Current { get; private set; } = -1;
    public bool SlideShow => _slides.IsEnabled;

    public PhotoApp(bool headless)
    {
        W = new Window("Gambar", 820, 560, headless);
        Scan();
        _list = new ListBox { ItemsSource = Names, Width = 180 };
        _list.SelectionChanged += l => { if (l.SelectedIndex >= 0 && l.SelectedIndex != Current) Show(l.SelectedIndex); };
        _slides.Tick += () => Step(1);
        View.Message = Paths.Count == 0 ? "Tidak ada gambar di /disk." : "Pilih gambar di kiri.";

        var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Margin = new Thickness(0, 0, 0, 8) };
        bar.Children.Add(new Button("<", _ => Step(-1)).WithClass("subtle"));
        bar.Children.Add(new Button(">", _ => Step(1)).WithClass("subtle"));
        bar.Children.Add(new Button("Perkecil", _ => View.ZoomBy(0.8)).WithClass("subtle"));
        bar.Children.Add(new Button("Pas", _ => View.SetZoom(0)).WithClass("subtle"));
        bar.Children.Add(new Button("1:1", _ => View.SetZoom(1)).WithClass("subtle"));
        bar.Children.Add(new Button("Perbesar", _ => View.ZoomBy(1.25)).WithClass("subtle"));
        bar.Children.Add(new Button(_showText, _ => ToggleSlideShow()).WithClass("accent"));

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Gambar", IconKind.Sun, 0xC792EA).DockTo(Dock.Top));
        root.Children.Add(bar.DockTo(Dock.Top));
        var body = new DockPanel();
        body.Children.Add(_list.DockTo(Dock.Left));
        View.Margin = new Thickness(12, 0, 0, 0);
        body.Children.Add(View);
        root.Children.Add(body);
        W.Content = root;
        W.UnhandledKey += Key;
        Application.Current?.Apply(W);
        if (Paths.Count > 0) Show(0);
        W.UpdateLayout();
    }

    void Key(KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Keys.Left: case Keys.PageUp: Step(-1); break;
            case Keys.Right: case Keys.PageDown: Step(1); break;
            case '+': case '=': View.ZoomBy(1.25); break;
            case '-': View.ZoomBy(0.8); break;
            case '0': View.SetZoom(0); break;
            case '1': View.SetZoom(1); break;
            case ' ': ToggleSlideShow(); break;
            case 'a': case 'A': View.PanBy(-40, 0); break;
            case 'd': case 'D': View.PanBy(40, 0); break;
            case 'w': case 'W': View.PanBy(0, -40); break;
            case 's': case 'S': View.PanBy(0, 40); break;
            default: return;
        }
        e.Handled = true;
    }

    public static string NameOf(string path) { int i = path.LastIndexOf('/'); return i >= 0 ? path.Substring(i + 1) : path; }

    /// <summary>The pictures of /disk and the scratch folder.</summary>
    public void Scan()
    {
        Paths.Clear(); Names.Clear();
        var dirs = Files.ScratchDirectory == "/disk" ? new[] { "/disk" } : new[] { "/disk", Files.ScratchDirectory };
        foreach (var dir in dirs)
        {
            List<Buitenzorg.App.FileEntry> files;
            try { files = Files.List(dir); } catch (Exception) { continue; }
            foreach (var f in files)
                if (!f.IsDirectory && Picture.IsPictureName(f.Name)) { Paths.Add(f.Path); Names.Add(f.Name); }
        }
    }

    public bool Show(int i)
    {
        if (i < 0 || i >= Paths.Count) return false;
        Current = i;
        _list.SelectedIndex = i;
        var pic = Picture.Load(Paths[i]);
        View.Name = Names[i];
        View.Message = pic == null ? Names[i] + ": formatnya tidak dapat dibaca" : "";
        View.Source = pic;
        return pic != null;
    }

    public void Step(int d)
    {
        if (Paths.Count == 0) return;
        Show(((Current < 0 ? 0 : Current) + d + Paths.Count) % Paths.Count);
    }

    public void ToggleSlideShow()
    {
        if (_slides.IsEnabled) _slides.Stop(); else _slides.Start();
        _showText.Text = _slides.IsEnabled ? "Henti" : "Slide show";
    }

    /// <summary>A tiny bottom-up 24-bit BMP, for the decoder test.</summary>
    public static byte[] SampleBmp(int w, int h)
    {
        int stride = (w * 3 + 3) & ~3, size = 54 + stride * h;
        var b = new byte[size];
        b[0] = (byte)'B'; b[1] = (byte)'M';
        void Put(int o, int v) { b[o] = (byte)v; b[o + 1] = (byte)(v >> 8); b[o + 2] = (byte)(v >> 16); b[o + 3] = (byte)(v >> 24); }
        Put(2, size); Put(10, 54); Put(14, 40); Put(18, w); Put(22, h);
        b[26] = 1; b[28] = 24;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int o = 54 + (h - 1 - y) * stride + x * 3; // top row stored last
                b[o] = (byte)(x * 255 / Math.Max(1, w - 1)); // blue ramps left to right
                b[o + 2] = (byte)(y == 0 ? 255 : 0);         // the top row is red
            }
        return b;
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Gambar");
        var pic = Picture.FromBytes(SampleBmp(8, 4));
        c.That(pic != null && pic.Width == 8 && pic.Height == 4 && pic.Format == "BMP" && pic.FrameCount == 1, "24-bit BMP decoded");
        var px = pic!.Pixels();
        c.That(((px[0] >> 16) & 255) == 255 && ((px[8 * 3] >> 16) & 255) == 0, "rows in the right order (top row red)");
        c.That((px[7] & 0xFF) == 255 && (px[0] & 0xFF) == 0, "blue ramp left to right");
        var half = new int[4 * 2];
        pic.Render(0, half, 4, 2, 0, 0, 8, 4);
        c.That((half[3] & 0xFF) > 150 && ((half[0] >> 16) & 255) == 255, "scaled to 4x2");
        c.That(pic.FitSize(4, 4) == (4, 2), "fit keeps the aspect");
        c.That(Picture.FromBytes(new byte[10]) == null && Picture.Sniff(new byte[] { 0x89, (byte)'P', (byte)'N', (byte)'G', 13, 10, 26, 10 }) == "PNG", "garbage refused, PNG sniffed");

        // every non-BMP picture on the media must decode (on the device: the host's PNG/JPEG/GIF decoders)
        var app = new PhotoApp(headless: true);
        int other = 0, ok = 0, animated = 0;
        for (int i = 0; i < app.Paths.Count; i++)
        {
            if (app.Names[i].ToUpperInvariant().EndsWith(".BMP")) continue;
            other++;
            var p = Picture.Load(app.Paths[i]);
            if (p != null && p.Width > 0) ok++;
            if (p != null && p.FrameCount > 1) animated++;
        }
        c.That(ok == other, "PNG/JPEG/GIF on the media decoded: " + ok + " of " + other + (animated > 0 ? " (" + animated + " animated)" : ""));
        if (app.Paths.Count > 0)
        {
            c.That(app.View.Source != null, "first picture shown: " + app.Names[0]);
            app.W.RenderFrame();
            int fitW = app.View.ShownW;
            app.View.SetZoom(4);
            app.W.RenderFrame();
            c.That(app.View.Zoom == 4 && app.View.ShownW >= fitW, "400% zoom (" + fitW + " -> " + app.View.ShownW + " px wide)");
            double cx = app.View.CenterX;
            app.View.PanBy(-30, 0);
            c.That(app.View.CenterX <= cx, "pan");
            app.Step(1);
            c.That(app.Current == (app.Paths.Count > 1 ? 1 : 0) && app.View.Zoom == 0, "next picture, fitted again");
            int gif = app.Names.FindIndex(n => n.ToUpperInvariant().EndsWith(".GIF"));
            if (gif >= 0 && app.Show(gif) && app.View.Source!.FrameCount > 1)
            {
                app.W.RenderFrame();
                int r = app.View.Renders;
                app.View.NextFrame();
                app.W.RenderFrame();
                c.That(app.View.Frame == 1 && app.View.Renders == r + 1, "GIF animation: frame 2 of " + app.View.Source.FrameCount);
            }
        }
        else c.That(app.View.Message.Length > 0, "no pictures: " + app.View.Message);
        app.ToggleSlideShow();
        c.That(app.SlideShow, "slide show on");
        app.ToggleSlideShow();
        app.W.RenderFrame();
        c.That(app.W.LastFrameOps > 10, "frame drew " + app.W.LastFrameOps + " ops");
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
            var c = PhotoApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var p = new PhotoApp(headless: false);
        app.Run(p.W);
        return 0;
    }
}
