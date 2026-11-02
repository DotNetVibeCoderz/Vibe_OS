// Gambar -- the managed image viewer: lists the BMP pictures on /disk (the
// official wallpapers, PHOTO.BMP, anything saved there), decodes the chosen
// one (24- / 32-bit uncompressed BMP) and shows it fitted to the window over a
// checkerboard, with its name and size. Buitenzorg.UI + Buitenzorg.App.Files.

using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;

namespace MImage;

/// <summary>A decoded picture: 0x00RRGGBB pixels, row-major, top row first.</summary>
public sealed class Picture
{
    public int Width, Height;
    public int[] Pixels = Array.Empty<int>();

    static int U16(byte[] b, int o) => b[o] | (b[o + 1] << 8);
    static int I32(byte[] b, int o) => b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24);

    /// <summary>Decode an uncompressed 24- or 32-bit BMP (null for anything else).</summary>
    public static Picture? DecodeBmp(byte[] b)
    {
        if (b.Length < 54 || b[0] != (byte)'B' || b[1] != (byte)'M') return null;
        int offset = I32(b, 10), w = I32(b, 18), h = I32(b, 22), bpp = U16(b, 28), compression = I32(b, 30);
        if (w <= 0 || w > 8192 || h == 0 || (bpp != 24 && bpp != 32) || (compression != 0 && compression != 3)) return null;
        bool topDown = h < 0;
        if (topDown) h = -h;
        int bytesPer = bpp / 8, stride = (w * bytesPer + 3) & ~3;
        if (offset + stride * (h - 1) + w * bytesPer > b.Length) return null;
        var p = new Picture { Width = w, Height = h, Pixels = new int[w * h] };
        for (int y = 0; y < h; y++)
        {
            int row = offset + (topDown ? y : h - 1 - y) * stride;
            int o = y * w;
            for (int x = 0; x < w; x++)
            {
                int s = row + x * bytesPer;
                p.Pixels[o + x] = (b[s + 2] << 16) | (b[s + 1] << 8) | b[s];
            }
        }
        return p;
    }

    /// <summary>A copy scaled (nearest neighbour) to fit inside w x h, aspect kept.</summary>
    public Picture Fit(int w, int h)
    {
        if (Width <= w && Height <= h) return this;
        double s = Math.Min((double)w / Width, (double)h / Height);
        int nw = Math.Max(1, (int)(Width * s)), nh = Math.Max(1, (int)(Height * s));
        var r = new Picture { Width = nw, Height = nh, Pixels = new int[nw * nh] };
        for (int y = 0; y < nh; y++)
        {
            int sy = y * Height / nh, o = y * nw, so = sy * Width;
            for (int x = 0; x < nw; x++) r.Pixels[o + x] = Pixels[so + x * Width / nw];
        }
        return r;
    }
}

/// <summary>Shows a picture centred over a checkerboard.</summary>
public sealed class PictureView : Control
{
    Picture? _src, _fitted;
    int _fitW, _fitH;
    public string Caption = "";

    public PictureView() { IsHitTestVisible = false; }

    public Picture? Source
    {
        get => _src;
        set { _src = value; _fitted = null; InvalidateVisual(); }
    }

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(200, a.Width), Math.Max(150, a.Height));

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(b, 10, Theme.SurfaceLo);
        int cell = 16;
        for (int y = 0; y < b.Height - 24; y += cell)
            for (int x = (y / cell) % 2 * cell; x < b.Width; x += 2 * cell)
                dc.FillRect(new Rect(b.X + x, b.Y + y, Math.Min(cell, b.Width - x), Math.Min(cell, b.Height - 24 - y)), Theme.Stroke);
        if (_src != null)
        {
            int aw = b.Width - 16, ah = b.Height - 40;
            if (_fitted == null || _fitW != aw || _fitH != ah) { _fitted = _src.Fit(aw, ah); _fitW = aw; _fitH = ah; }
            int x = b.X + (b.Width - _fitted.Width) / 2, y = b.Y + 8 + (ah - _fitted.Height) / 2;
            dc.Blit(x, y, _fitted.Width, _fitted.Height, _fitted.Pixels);
        }
        dc.Text(b.X + 10, b.Y + b.Height - 20, Caption, Theme.TextFaint);
    }
}

public static class ImageApp
{
    /// <summary>The BMP files on /disk (or the scratch folder).</summary>
    public static List<string> Pictures()
    {
        var list = new List<string>();
        foreach (var f in Files.List("/disk"))
            if (!f.IsDirectory && f.Name.EndsWith(".BMP", StringComparison.OrdinalIgnoreCase)) list.Add(f.Path);
        return list;
    }

    public static string NameOf(string path) { int i = path.LastIndexOf('/'); return i >= 0 ? path.Substring(i + 1) : path; }

    public static (Window, ListBox, PictureView) Build(bool headless = false)
    {
        var w = new Window("Gambar", 760, 500, headless);
        var paths = Pictures();
        var names = paths.ConvertAll(NameOf);
        var view = new PictureView { Caption = paths.Count == 0 ? "Tidak ada gambar BMP di /disk." : "Pilih gambar di kiri." };
        var list = new ListBox { ItemsSource = names, Width = 170 };
        list.SelectionChanged += l =>
        {
            int i = l.SelectedIndex;
            if (i < 0 || i >= paths.Count) return;
            var bytes = Files.ReadAllBytes(paths[i]);
            var pic = bytes == null ? null : Picture.DecodeBmp(bytes);
            view.Source = pic;
            view.Caption = pic == null ? names[i] + ": tidak dapat dibaca (BMP 24/32-bit saja)" : names[i] + "  -  " + pic.Width + " x " + pic.Height;
        };
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Gambar", IconKind.Sun, 0xC792EA).DockTo(Dock.Top));
        var body = new DockPanel();
        body.Children.Add(list.DockTo(Dock.Left));
        view.Margin = new Thickness(12, 0, 0, 0);
        body.Children.Add(view);
        root.Children.Add(body);
        w.Content = root;
        Application.Current?.Apply(w);
        if (paths.Count > 0) list.SelectedIndex = 0;
        w.UpdateLayout();
        return (w, list, view);
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
        var pic = Picture.DecodeBmp(SampleBmp(8, 4));
        c.That(pic != null && pic.Width == 8 && pic.Height == 4, "24-bit BMP decoded");
        c.That(pic != null && (pic.Pixels[0] >> 16) == 255 && (pic.Pixels[8 * 3] >> 16) == 0, "rows in the right order (top row red)");
        c.That(pic != null && (pic.Pixels[7] & 0xFF) == 255 && (pic.Pixels[0] & 0xFF) == 0, "blue ramp left to right");
        var fit = pic!.Fit(4, 4);
        c.That(fit.Width == 4 && fit.Height == 2, "fitted with the aspect kept (" + fit.Width + "x" + fit.Height + ")");
        c.That(Picture.DecodeBmp(new byte[10]) == null, "garbage refused");
        var (w, list, view) = Build(headless: true);
        if (list.ItemCount > 0)
            c.That(view.Source != null, "first picture on /disk shown: " + view.Caption);
        else
            c.That(view.Caption.Length > 0, "no pictures: " + view.Caption);
        w.RenderFrame();
        c.That(w.LastFrameOps > 10, "frame drew " + w.LastFrameOps + " ops");
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
            var c = ImageApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = ImageApp.Build();
        app.Run(w);
        return 0;
    }
}
