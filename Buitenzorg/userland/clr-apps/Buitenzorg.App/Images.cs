// Buitenzorg.Media.Picture -- pictures in the common formats.
//
// The decoding runs in the host (clrhost imgdecode.rs: BMP, PNG, baseline and
// progressive JPEG, GIF with every animation frame), never per pixel in the
// interpreter: Render() asks the host for a source rectangle sampled into the
// size the program wants, alpha already composited over a checkerboard, ready
// for one Blit. Off the device (a PC running `dotnet X.dll`) only BMP decodes,
// in C#, so self-tests still work there.

namespace Buitenzorg.Media;

using System.Runtime.InteropServices;

internal static class ImageNative
{
    [DllImport("buitenzorg")] internal static extern long ImageInfo(byte[] data);
    [DllImport("buitenzorg")] internal static extern int ImageDecodeScaled(byte[] data, int frame, int[] pixels, int dw, int dh, int sx, int sy, int sw, int sh, int bg1, int bg2);
}

public sealed class Picture
{
    readonly byte[] _data;
    readonly int[]? _bmp;      // off the device: the BMP decoded in C# (0xAARRGGBB)

    public int Width { get; }
    public int Height { get; }
    /// <summary>1 for a still picture, more for an animated GIF.</summary>
    public int FrameCount { get; }
    /// <summary>"BMP", "PNG", "JPEG", "GIF" (from the file's first bytes).</summary>
    public string Format { get; }

    Picture(byte[] data, int w, int h, int frames, string format, int[]? bmp)
    {
        _data = data; Width = w; Height = h; FrameCount = frames; Format = format; _bmp = bmp;
    }

    /// <summary>The format named by the first bytes, "" when unknown.</summary>
    public static string Sniff(byte[] b)
    {
        if (b.Length >= 2 && b[0] == 'B' && b[1] == 'M') return "BMP";
        if (b.Length >= 8 && b[0] == 0x89 && b[1] == 'P' && b[2] == 'N' && b[3] == 'G') return "PNG";
        if (b.Length >= 3 && b[0] == 0xFF && b[1] == 0xD8 && b[2] == 0xFF) return "JPEG";
        if (b.Length >= 6 && b[0] == 'G' && b[1] == 'I' && b[2] == 'F') return "GIF";
        return "";
    }

    /// <summary>True for the file names this class can open.</summary>
    public static bool IsPictureName(string name)
    {
        string n = name.ToUpperInvariant();
        return n.EndsWith(".BMP") || n.EndsWith(".PNG") || n.EndsWith(".JPG") || n.EndsWith(".JPEG") || n.EndsWith(".GIF");
    }

    public static Picture? Load(string path)
    {
        var b = Buitenzorg.App.Files.ReadAllBytes(path);
        return b == null ? null : FromBytes(b);
    }

    public static Picture? FromBytes(byte[] data)
    {
        string format = Sniff(data);
        if (format.Length == 0) return null;
        try
        {
            long info = ImageNative.ImageInfo(data);
            if (info < 0) return null;
            return new Picture(data, (int)(info & 0xFFFF), (int)((info >> 16) & 0xFFFF), (int)(info >> 32), format, null);
        }
        catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException)
        {
            if (format != "BMP") return null;
            var px = DecodeBmp(data, out int w, out int h);
            return px == null ? null : new Picture(data, w, h, 1, format, px);
        }
    }

    /// <summary>Draw the source rectangle (sx, sy, sw, sh) of a frame into
    /// dst (dw x dh pixels, nearest sampling, transparent parts over a
    /// checkerboard of bg1 / bg2). Returns the frame's delay in ms (0 = still),
    /// -1 on failure.</summary>
    public int Render(int frame, int[] dst, int dw, int dh, int sx, int sy, int sw, int sh, int bg1 = 0x2A3A33, int bg2 = 0x22302A)
    {
        if (dst.Length < dw * dh || dw <= 0 || dh <= 0) return -1;
        if (_bmp == null) return ImageNative.ImageDecodeScaled(_data, frame, dst, dw, dh, sx, sy, Math.Max(1, sw), Math.Max(1, sh), bg1, bg2);
        for (int y = 0; y < dh; y++)
        {
            int ty = sy + y * sh / dh;
            for (int x = 0; x < dw; x++)
            {
                int tx = sx + x * sw / dw;
                int bg = (((x >> 3) ^ (y >> 3)) & 1) == 0 ? bg1 : bg2;
                dst[y * dw + x] = tx < 0 || ty < 0 || tx >= Width || ty >= Height ? bg : _bmp[ty * Width + tx] & 0xFFFFFF;
            }
        }
        return 0;
    }

    /// <summary>The whole frame at its own size.</summary>
    public int[] Pixels(int frame = 0)
    {
        var px = new int[Width * Height];
        Render(frame, px, Width, Height, 0, 0, Width, Height);
        return px;
    }

    /// <summary>The size that fits inside w x h with the aspect kept (never enlarged unless allowUp).</summary>
    public (int W, int H) FitSize(int w, int h, bool allowUp = false)
    {
        double s = Math.Min((double)w / Width, (double)h / Height);
        if (!allowUp && s > 1) s = 1;
        return (Math.Max(1, (int)(Width * s)), Math.Max(1, (int)(Height * s)));
    }

    static int U16(byte[] b, int o) => b[o] | (b[o + 1] << 8);
    static int I32(byte[] b, int o) => b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24);

    /// <summary>An uncompressed 24- or 32-bit BMP (C#; null for anything else).</summary>
    public static int[]? DecodeBmp(byte[] b, out int w, out int h)
    {
        w = h = 0;
        if (b.Length < 54 || b[0] != 'B' || b[1] != 'M') return null;
        int offset = I32(b, 10), bpp = U16(b, 28), compression = I32(b, 30);
        w = I32(b, 18); h = I32(b, 22);
        if (w <= 0 || w > 8192 || h == 0 || (bpp != 24 && bpp != 32) || (compression != 0 && compression != 3)) return null;
        bool topDown = h < 0;
        if (topDown) h = -h;
        int bytesPer = bpp / 8, stride = (w * bytesPer + 3) & ~3;
        if (offset + stride * (h - 1) + w * bytesPer > b.Length) return null;
        var px = new int[w * h];
        for (int y = 0; y < h; y++)
        {
            int row = offset + (topDown ? y : h - 1 - y) * stride, o = y * w;
            for (int x = 0; x < w; x++)
            {
                int s = row + x * bytesPer;
                px[o + x] = unchecked((int)0xFF000000) | (b[s + 2] << 16) | (b[s + 1] << 8) | b[s];
            }
        }
        return px;
    }
}
