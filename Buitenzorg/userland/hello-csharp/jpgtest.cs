// Buitenzorg OS - v0.16 "Panen": JPEG decoder test.
//
// Loads /disk/GRAD.JPG (a 64x64 baseline red->blue gradient, 4:2:0) through the
// FS_READ syscall, decodes it with Buitenzorg.Drawing's Jpeg.Load, and checks
// the result against the reference decode (ffmpeg): dimensions 64x64, a reddish
// left column, a bluish right column, and a purple middle. Prints
// MILESTONE: JPEG OK on success. Built with bflat --stdlib:zero + bzgfx.

using System;
using System.Runtime.InteropServices;
using Buitenzorg.Drawing;

class JpgTest
{
    [DllImport("*")] static extern unsafe uint bz_fs_read(byte* path, byte* buf, ulong max);

    static unsafe byte[] ReadFile(string path, int maxBytes, out int count)
    {
        byte* pb = stackalloc byte[128];
        int pl = 0; for (int i = 0; i < path.Length && pl < 126; i++) pb[pl++] = (byte)path[i]; pb[pl] = 0;
        byte[] buf = new byte[maxBytes];
        uint n; fixed (byte* dst = buf) { n = bz_fs_read(pb, dst, (ulong)maxBytes); }
        count = (int)n;
        return n == 0 ? null : buf;
    }

    static int Near(uint px, int r, int g, int b, int tol)
    {
        int pr = (int)((px >> 16) & 255), pg = (int)((px >> 8) & 255), pb = (int)(px & 255);
        int ok = (Abs(pr - r) <= tol && Abs(pg - g) <= tol && Abs(pb - b) <= tol) ? 1 : 0;
        return ok;
    }
    static int Abs(int v) => v < 0 ? -v : v;

    static void Main()
    {
        Console.WriteLine("JpgTest: baseline JPEG decode (Buitenzorg.Drawing)...");
        int n;
        byte[] raw = ReadFile("/disk/GRAD.JPG", 64 * 1024, out n);
        if (raw == null || n < 4 || raw[0] != 0xFF || raw[1] != 0xD8)
        {
            Console.WriteLine("JpgTest: GRAD.JPG missing / not a JPEG");
            return;
        }

        Bitmap img = Jpeg.Load(raw);
        if (img == null) { Console.WriteLine("JpgTest: decode returned null"); return; }

        bool dimOk = img.Width == 64 && img.Height == 64;
        // Reference (ffmpeg): (8,32)=R221 B32, (32,32)=R125 B130, (56,32)=R28 B225.
        // Tolerance is generous: integer IDCT + chroma upsampling differ slightly.
        int left = Near(img.Pixels[32 * 64 + 8], 221, 0, 32, 40);
        int mid = Near(img.Pixels[32 * 64 + 32], 125, 0, 130, 45);
        int right = Near(img.Pixels[32 * 64 + 56], 28, 0, 225, 40);
        // Also assert it is a real gradient (left redder than right, right bluer).
        uint pl = img.Pixels[32 * 64 + 8], pr = img.Pixels[32 * 64 + 56];
        bool grad = ((pl >> 16) & 255) > ((pr >> 16) & 255) && (pr & 255) > (pl & 255);

        if (dimOk && left == 1 && mid == 1 && right == 1 && grad)
            Console.WriteLine("MILESTONE: JPEG OK");
        else
            Console.WriteLine("JpgTest: verifikasi gagal (dim/warna/gradien)");
    }
}
