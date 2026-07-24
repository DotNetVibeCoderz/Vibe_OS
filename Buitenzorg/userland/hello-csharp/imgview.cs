// Buitenzorg OS - v0.16 "Panen": Image Viewer.
//
// Loads an image from the VFS through Buitenzorg.Bcl's `BzFile` (System.IO,
// over the FS_READ syscall), decodes it with Buitenzorg.Drawing, and shows it in a
// Buitenzorg.UI window: an ImageView custom UIElement scales the picture to
// fit its box (preserving aspect ratio) over a checkerboard "transparency"
// backdrop, with a filename/size caption. The demo reads /disk/PHOTO.BMP,
// verifies the decode (dimensions + a non-trivial pixel), renders, and prints
// MILESTONE: IMGVIEW OK. Built with bflat --stdlib:zero + bzui/bzgfx/bzbcl.

using System;
using Buitenzorg;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

sealed unsafe class ImageView : UIElement
{
    Bitmap _img;
    Font _font;
    public readonly char[] Caption = new char[64];
    public int CaptionN;

    public ImageView(Font f) { _font = f; }
    public void SetImage(Bitmap b) { _img = b; }

    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 360; DesiredH = Height >= 0 ? Height : 260; }

    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF15181F), X, Y, W, H, 6);
        g.DrawRoundedRectangle(new Color(0xFF3A4050), X, Y, W, H, 6);

        int capH = 20;
        int ix = X + 8, iy = Y + 8, iw = W - 16, ih = H - 16 - capH;

        // Checkerboard backdrop (the classic "no image" / transparency look).
        int cell = 12;
        for (int yy = 0; yy < ih; yy += cell)
            for (int xx = 0; xx < iw; xx += cell)
            {
                bool dark = (((xx / cell) + (yy / cell)) & 1) == 0;
                Color c = dark ? new Color(0xFF23272F) : new Color(0xFF2C313B);
                int cw = iw - xx; if (cw > cell) cw = cell;
                int ch = ih - yy; if (ch > cell) ch = cell;
                g.FillRectangle(c, ix + xx, iy + yy, cw, ch);
            }

        if (_img != null)
        {
            // Fit-to-box, preserve aspect ratio.
            int dw = iw, dh = _img.Height * iw / _img.Width;
            if (dh > ih) { dh = ih; dw = _img.Width * ih / _img.Height; }
            int dx = ix + (iw - dw) / 2, dy = iy + (ih - dh) / 2;
            g.DrawImageScaled(_img, dx, dy, dw, dh);
            g.DrawRectangle(new Color(0xFF4A5162), dx - 1, dy - 1, dw + 2, dh + 2);
        }

        // Caption bar.
        g.FillRectangle(new Color(0xFF1B1F27), X + 1, Y + H - capH - 1, W - 2, capH);
        g.DrawChars(_font, Caption, CaptionN, new Color(0xFF9FD0FF), X + 10, Y + H - capH + (capH - _font.CharH) / 2);
    }
}

class ImgViewer
{
    // Load an image file, dispatching on its magic bytes: "BM" -> 24-bit BMP,
    // 0xFF 0xD8 -> baseline JPEG. Returns null if unreadable/unsupported.
    // The read itself is System.IO (BzFile), so the FS_READ call lives in one
    // place; Bmp/Jpeg only read what their headers describe, so a buffer larger
    // than the file is harmless and saves a second big copy.
    static Bitmap Decode(string path)
    {
        byte[] raw;
        int n = BzFile.ReadAllBytes(path, 400 * 1024, out raw);
        if (raw == null || n < 4) return null;
        if (raw[0] == (byte)'B' && raw[1] == (byte)'M') return Bmp.Load(raw);
        if (raw[0] == 0xFF && raw[1] == 0xD8) return Jpeg.Load(raw);
        return null;
    }

    // "name WxH", with the numbers formatted by System.Globalization.
    static void SetCaption(ImageView iv, char[] name, int nameLen, int w, int h)
    {
        int p = 0;
        for (int i = 0; i < nameLen && p < 60; i++) iv.Caption[p++] = name[i];
        iv.Caption[p++] = ' ';
        p = BzCulture.FormatIntAt(w, iv.Caption, p, false, ',');
        iv.Caption[p++] = 'x';
        p = BzCulture.FormatIntAt(h, iv.Caption, p, false, ',');
        iv.CaptionN = p;
    }

    static void Main()
    {
        Console.WriteLine("ImgView: Image Viewer (Buitenzorg.UI + FS_READ)...");
        Font font = Font.Default();
        const int W = 360, H = 290;
        UIHost host = new UIHost("Image Viewer", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 10; root.Spacing = 6;
        root.Background = new Color(0xFF1C2028);
        TextBlock title = new TextBlock("IMAGE VIEWER", font);
        title.Foreground = Color.White;

        ImageView iv = new ImageView(font);
        iv.Width = 340; iv.Height = 228;

        root.Add(title);
        root.Add(iv);
        host.Root = root;
        host.Layout();

        // Load an image through the FS_READ syscall and decode it by format.
        // Supports both 24-bit BMP ("BM") and baseline JPEG (0xFF 0xD8); the
        // demo shows PHOTO.BMP, but the same path opens a .jpg.
        bool ok = false;
        const string Path = "/disk/PHOTO.BMP";

        // The caption name is the file name, derived with System.IO's BzPath
        // instead of being hard-coded a second time next to the path.
        char[] full = new char[Path.Length];
        for (int i = 0; i < Path.Length; i++) full[i] = Path[i];
        char[] name = new char[32];
        int nameLen = BzPath.GetFileName(full, Path.Length, name);

        Bitmap img = Decode(Path);
        if (img != null)
        {
            iv.SetImage(img);
            SetCaption(iv, name, nameLen, img.Width, img.Height);
            bool litPixel = false;
            for (int i = 0; i < img.Pixels.Length; i += 97)
                if ((img.Pixels[i] & 0x00FFFFFF) != 0) { litPixel = true; break; }
            ok = img.Width == 320 && img.Height == 200 && litPixel;
        }

        host.Render(new Color(0xFF141820));
        host.Present();

        if (ok)
            Console.WriteLine("MILESTONE: IMGVIEW OK");
        else
            Console.WriteLine("ImgView: gagal memuat/dekode gambar");
    }
}
