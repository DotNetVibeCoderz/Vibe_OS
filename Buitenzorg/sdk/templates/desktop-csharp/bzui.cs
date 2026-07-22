// Buitenzorg UI helper (v0.8): thin wrappers over the window syscalls, for
// desktop apps compiled freestanding with bflat (--stdlib:zero) + the bzstart
// shim. Copy this file alongside your app, or reference the SDK once packaged.
//
// No heap allocation: apps are freestanding (no GC yet), so keep board/state
// on the stack (stackalloc) and avoid `new T[]`.

using System.Runtime.InteropServices;

public static unsafe class BzUi
{
    [DllImport("*")] static extern unsafe uint bz_win_create(byte* title, ulong len, ulong dims);
    [DllImport("*")] static extern unsafe ulong bz_win_cmd(uint window, DrawCmd* cmd);
    [DllImport("*")] static extern void bz_win_present(uint window);
    [DllImport("*")] static extern uint bz_key_read();
    [DllImport("*")] static extern ulong bz_ticks();

    // Mirror of bz_abi::DrawCmd (48 bytes).
    struct DrawCmd
    {
        public ulong Op;
        public int X, Y, W, H;
        public uint Color, Pad;
        public ulong TextPtr, TextLen;
    }

    const ulong OpFill = 0, OpText = 1, OpClear = 2;

    public static uint CreateWindow(string title, int w, int h)
    {
        byte* buf = stackalloc byte[64];
        int n = 0;
        fixed (char* tc = title)
            for (int i = 0; i < title.Length && n < 63; i++) buf[n++] = (byte)tc[i];
        return bz_win_create(buf, (ulong)n, ((ulong)(uint)w << 32) | (uint)h);
    }

    public static void Fill(uint win, int x, int y, int w, int h, uint color)
    {
        var cmd = new DrawCmd { Op = OpFill, X = x, Y = y, W = w, H = h, Color = color };
        bz_win_cmd(win, &cmd);
    }

    public static void Clear(uint win, uint color)
    {
        var cmd = new DrawCmd { Op = OpClear, Color = color };
        bz_win_cmd(win, &cmd);
    }

    public static void Text(uint win, int x, int y, string s, uint color)
    {
        byte* buf = stackalloc byte[256];
        int n = 0;
        fixed (char* sc = s)
            for (int i = 0; i < s.Length && n < 255; i++) buf[n++] = (byte)sc[i];
        var cmd = new DrawCmd { Op = OpText, X = x, Y = y, Color = color, TextPtr = (ulong)buf, TextLen = (ulong)n };
        bz_win_cmd(win, &cmd);
    }

    public static void Present(uint win) => bz_win_present(win);

    /// <summary>Read one key (0 when none).</summary>
    public static char ReadKey() => (char)bz_key_read();

    /// <summary>Timer ticks since boot (~18.2/second).</summary>
    public static ulong Ticks() => bz_ticks();

    public static void Sleep(ulong ticks)
    {
        ulong until = bz_ticks() + ticks;
        while (bz_ticks() < until) { }
    }
}
