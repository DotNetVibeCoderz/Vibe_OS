using System;

namespace MImage;

/// <summary>EXIF orientation: where a shown pixel comes from in the stored picture,
/// and turning a pixel buffer (the host's PixOrient on the device; a managed loop
/// elsewhere -- slow in the interpreter, so only the fallback).</summary>
public static class Orient
{
    [System.Runtime.InteropServices.DllImport("buitenzorg")] static extern int PixOrient(int[] dst, int[] src, int sw, int sh, int orientation);

    static int _native; // 0 unknown, 1 host, 2 managed
    public static bool NativeUsed => _native == 1;

    /// <summary>The stored pixel (in an sw x sh picture) behind shown pixel (lx, ly).</summary>
    public static (int, int) Source(int o, int lx, int ly, int sw, int sh) => o switch
    {
        2 => (sw - 1 - lx, ly),
        3 => (sw - 1 - lx, sh - 1 - ly),
        4 => (lx, sh - 1 - ly),
        5 => (ly, lx),
        6 => (ly, sh - 1 - lx),
        7 => (sw - 1 - ly, sh - 1 - lx),
        8 => (sw - 1 - ly, lx),
        _ => (lx, ly),
    };

    /// <summary>dst (shown: sh x sw for 5..8, else sw x sh) from src (stored, sw x sh).</summary>
    public static void Apply(int[] dst, int[] src, int sw, int sh, int o)
    {
        if (_native != 2)
        {
            try { if (PixOrient(dst, src, sw, sh, o) == 0) { _native = 1; return; } }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _native = 2; }
        }
        int lw = o >= 5 ? sh : sw, lh = o >= 5 ? sw : sh;
        for (int ly = 0; ly < lh; ly++)
            for (int lx = 0; lx < lw; lx++)
            {
                var (x, y) = Source(o, lx, ly, sw, sh);
                dst[ly * lw + lx] = src[y * sw + x];
            }
    }
}
