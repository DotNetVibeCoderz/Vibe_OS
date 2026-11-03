// Buitenzorg.UI -- the drawing context: every primitive is one kernel draw op
// on the window's canvas (rounded rects, gradients, styled text, blends), so
// the interpreted managed code never touches pixels. Clipping is done here,
// by trimming or skipping primitives against a clip stack.

namespace Buitenzorg.UI;

public sealed class DrawingContext
{
    // bz-abi draw_op
    const int FillRectOp = 0, TextOp = 1, ClearOp = 2, LineOp = 3, EllipseOp = 4, FillEllipseOp = 5, RectOp = 6;
    const int FillRoundOp = 8, RoundOp = 9, GradientOp = 10, RoundGradientOp = 11, TextStyledOp = 12, BlendRoundOp = 13;

    readonly int _win;
    readonly List<Rect> _clips = new List<Rect>();
    Rect _clip;

    /// <summary>Draw ops issued (diagnostics / tests).</summary>
    public int Ops { get; private set; }

    internal DrawingContext(int window, int width, int height)
    {
        _win = window;
        _clip = new Rect(0, 0, width, height);
    }

    public Rect Clip => _clip;

    public void PushClip(Rect r)
    {
        _clips.Add(_clip);
        _clip = _clip.Intersect(r);
    }

    public void PopClip()
    {
        _clip = _clips[_clips.Count - 1];
        _clips.RemoveAt(_clips.Count - 1);
    }

    /// <summary>Draw ops the kernel refused (0 on a healthy system).</summary>
    public static int Refused { get; private set; }

    void Op(int op, int x, int y, int w, int h, int color, int p = 0, int p2 = 0, string text = "")
    {
        Ops++;
        if (_win == 0) return;
        int r = op < 8 ? Native.WinDraw(_win, op, x, y, w, h, color, text) : Native.WinDrawEx(_win, op, x, y, w, h, color, p, p2, text);
        if (r != 0) Refused++;
    }

    public void Clear(int color) { if (color >= 0) Op(ClearOp, 0, 0, 0, 0, color); }

    public void FillRect(Rect r, int color)
    {
        if (color < 0) return;
        r = r.Intersect(_clip);
        if (!r.IsEmpty) Op(FillRectOp, r.X, r.Y, r.Width, r.Height, color);
    }

    public void FillRoundRect(Rect r, int radius, int color)
    {
        if (color < 0 || r.IsEmpty) return;
        if (radius <= 0 || !_clip.Covers(r)) { FillRect(r, color); return; }
        Op(FillRoundOp, r.X, r.Y, r.Width, r.Height, color, Math.Min(radius, Math.Min(r.Width, r.Height) / 2));
    }

    public void StrokeRoundRect(Rect r, int radius, int color, int thickness = 1)
    {
        if (color < 0 || r.IsEmpty || thickness <= 0) return;
        if (!_clip.Covers(r))
        {
            // Clipped: draw the visible edges as thin rectangles.
            FillRect(new Rect(r.X, r.Y, r.Width, thickness), color);
            FillRect(new Rect(r.X, r.Bottom - thickness, r.Width, thickness), color);
            FillRect(new Rect(r.X, r.Y, thickness, r.Height), color);
            FillRect(new Rect(r.Right - thickness, r.Y, thickness, r.Height), color);
            return;
        }
        int rad = Math.Min(radius, Math.Min(r.Width, r.Height) / 2);
        for (int i = 0; i < thickness; i++)
        {
            var ri = r.Inflate(-i);
            if (rad <= 0) Op(RectOp, ri.X, ri.Y, ri.Width, ri.Height, color);
            else Op(RoundOp, ri.X, ri.Y, ri.Width, ri.Height, color, Math.Max(0, rad - i));
        }
    }

    /// <summary>Vertical gradient (top to bottom), optionally rounded.</summary>
    public void FillGradient(Rect r, int radius, int top, int bottom)
    {
        if (top < 0 || r.IsEmpty) return;
        if (bottom < 0 || bottom == top) { FillRoundRect(r, radius, top); return; }
        if (!_clip.Covers(r))
        {
            var c = r.Intersect(_clip);
            if (!c.IsEmpty) Op(GradientOp, c.X, c.Y, c.Width, c.Height, top, bottom);
            return;
        }
        if (radius <= 0) Op(GradientOp, r.X, r.Y, r.Width, r.Height, top, bottom);
        else Op(RoundGradientOp, r.X, r.Y, r.Width, r.Height, top, bottom, Math.Min(radius, Math.Min(r.Width, r.Height) / 2));
    }

    /// <summary>A translucent rounded rectangle (alpha 0-255): overlays, shadows, focus glows.</summary>
    public void BlendRoundRect(Rect r, int radius, int color, int alpha)
    {
        if (color < 0 || alpha <= 0 || r.IsEmpty) return;
        if (!_clip.Covers(r)) { r = r.Intersect(_clip); radius = 0; if (r.IsEmpty) return; }
        int rad = Math.Min(radius, Math.Min(r.Width, r.Height) / 2);
        Op(BlendRoundOp, r.X, r.Y, r.Width, r.Height, color, (Math.Min(alpha, 255) << 16) | (rad & 0xFFFF));
    }

    /// <summary>A soft drop shadow under a rounded rectangle.</summary>
    public void Shadow(Rect r, int radius, BoxShadow s)
    {
        if (s.IsEmpty) return;
        var b = new Rect(r.X + s.OffsetX, r.Y + s.OffsetY, r.Width, r.Height);
        int steps = Math.Max(1, Math.Min(4, s.Blur / 2));
        for (int i = steps; i >= 1; i--)
            BlendRoundRect(b.Inflate(i * s.Blur / steps), radius + i * s.Blur / steps, s.Color, s.Alpha / (steps + 1));
        BlendRoundRect(b, radius, s.Color, s.Alpha / 2);
    }

    public void Line(int x0, int y0, int x1, int y1, int color)
    {
        if (color < 0) return;
        if (!_clip.Contains(x0, y0) || !_clip.Contains(x1, y1))
        {
            // Axis-aligned lines are trimmed; others are dropped when they leave the clip.
            if (y0 == y1) { FillRect(new Rect(Math.Min(x0, x1), y0, Math.Abs(x1 - x0) + 1, 1), color); }
            else if (x0 == x1) { FillRect(new Rect(x0, Math.Min(y0, y1), 1, Math.Abs(y1 - y0) + 1), color); }
            return;
        }
        Op(LineOp, x0, y0, x1 - x0, y1 - y0, color);
    }

    public void Ellipse(Rect r, int color, bool fill)
    {
        if (color < 0 || r.IsEmpty || !_clip.Covers(r)) return;
        Op(fill ? FillEllipseOp : EllipseOp, r.X, r.Y, r.Width, r.Height, color);
    }

    /// <summary>One line of text with its top-left at (x, y); trimmed to the clip.</summary>
    public void Text(int x, int y, string text, int color, int style = 0)
    {
        if (color < 0 || text.Length == 0) return;
        int gw = Fonts.GlyphWidth(style), lh = Fonts.LineHeight(style);
        if (y < _clip.Y || y + lh > _clip.Bottom) return;
        int skip = 0;
        if (x < _clip.X) { skip = (_clip.X - x + gw - 1) / gw; x += skip * gw; }
        int fit = (_clip.Right - x) / gw;
        if (fit <= 0 || skip >= text.Length) return;
        int n = Math.Min(fit, text.Length - skip);
        string s = skip == 0 && n == text.Length ? text : text.Substring(skip, n);
        if (style == 0) Op(TextOp, x, y, 0, 0, color, 0, 0, s);
        else Op(TextStyledOp, x, y, 0, 0, color, style, 0, s);
    }

    /// <summary>Copy w*h 0xRRGGBB pixels to (x, y); only when wholly inside the clip.</summary>
    public void Blit(int x, int y, int w, int h, int[] pixels)
    {
        if (w <= 0 || h <= 0 || !_clip.Covers(new Rect(x, y, w, h))) return;
        Ops++;
        if (_win != 0) Native.WinBlit(_win, x, y, w, h, pixels);
    }

    /// <summary>Copy a 3D picture (Buitenzorg.Graphics3D) to (x, y) -- one kernel
    /// copy from the render context; only when wholly inside the clip. Off the
    /// device (the simulated rasterizer) the pixels are blitted instead.</summary>
    public void Present3D(Buitenzorg.Graphics3D.Device3D device, int x, int y)
    {
        if (!_clip.Covers(new Rect(x, y, device.Width, device.Height))) return;
        Ops++;
        if (_win == 0) return;
        if (device.IsSimulated) Native.WinBlit(_win, x, y, device.Width, device.Height, device.ReadPixels());
        else device.PresentTo(_win, x, y);
    }
}

/// <summary>A drop shadow (Avalonia BoxShadow): offset, blur radius, color and opacity.</summary>
public struct BoxShadow
{
    public int OffsetX, OffsetY, Blur, Color, Alpha;
    public BoxShadow(int offsetX, int offsetY, int blur, int color, int alpha)
    {
        OffsetX = offsetX; OffsetY = offsetY; Blur = blur; Color = color; Alpha = alpha;
    }
    public bool IsEmpty => Alpha <= 0;
    public static BoxShadow None => new BoxShadow(0, 0, 0, 0, 0);
}

/// <summary>A small 0xRRGGBB pixel image for <see cref="Image"/>.</summary>
public sealed class Bitmap
{
    public int Width { get; }
    public int Height { get; }
    public int[] Pixels { get; }

    public Bitmap(int width, int height)
    {
        Width = width; Height = height; Pixels = new int[width * height];
    }

    public static Bitmap Create(int width, int height, Func<int, int, int> pixel)
    {
        var b = new Bitmap(width, height);
        for (int y = 0; y < height; y++)
            for (int x = 0; x < width; x++)
                b.Pixels[y * width + x] = pixel(x, y);
        return b;
    }

    /// <summary>Decode an uncompressed 24- or 32-bit BMP (bottom-up or top-down); null if it is not one.</summary>
    public static Bitmap? FromBmp(byte[]? b)
    {
        if (b == null || b.Length < 54 || b[0] != 'B' || b[1] != 'M') return null;
        int off = BitConverter.ToInt32(b, 10), w = BitConverter.ToInt32(b, 18), hs = BitConverter.ToInt32(b, 22);
        int bpp = BitConverter.ToInt16(b, 28), comp = BitConverter.ToInt32(b, 30);
        int h = Math.Abs(hs);
        if (w <= 0 || h <= 0 || w > 4096 || h > 4096 || (bpp != 24 && bpp != 32) || (comp != 0 && comp != 3)) return null;
        int bytes = bpp / 8, stride = (w * bytes + 3) & ~3;
        if (off + stride * (h - 1) + w * bytes > b.Length) return null;
        var bmp = new Bitmap(w, h);
        for (int y = 0; y < h; y++)
        {
            int row = off + (hs > 0 ? h - 1 - y : y) * stride;
            for (int x = 0; x < w; x++)
            {
                int p = row + x * bytes;
                bmp.Pixels[y * w + x] = (b[p + 2] << 16) | (b[p + 1] << 8) | b[p];
            }
        }
        return bmp;
    }

    /// <summary>Nearest-neighbour scale (cached by the Image control).</summary>
    public Bitmap Scale(int w, int h)
    {
        var b = new Bitmap(w, h);
        for (int y = 0; y < h; y++)
        {
            int sy = y * Height / h;
            for (int x = 0; x < w; x++) b.Pixels[y * w + x] = Pixels[sy * Width + x * Width / w];
        }
        return b;
    }
}
