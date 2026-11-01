// Buitenzorg OS — SMP fine-grained locking test #2: window-system syscalls.
//
// Like SMPSYS, the kernel boot test runs one copy per CPU at once, twice (all
// syscalls behind the Big Kernel Lock, then fine-grained). Each copy opens a
// small window and loops for ~1.5 s:
//   fill rect + blit a 96x48 frame into its window -> SYS_STAT -> PROC_LIST
//   (+ a full compose every 64 rounds),
// counting rounds, then presents once more. Any failed call is an error.
// Exit code: 100 + rounds = correct; 1 = wrong data; 2 = a syscall failed.
// Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

class SmpGui
{
    [DllImport("*")] static extern unsafe uint bz_win_create(byte* title, ulong len, ulong dims);
    [DllImport("*")] static extern unsafe ulong bz_win_cmd(uint window, DrawCmd* cmd);
    [DllImport("*")] static extern void bz_win_present(uint window);
    [DllImport("*")] static extern unsafe ulong bz_sys_stat(byte* outp);
    [DllImport("*")] static extern unsafe ulong bz_proc_list(byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_ticks();

    struct DrawCmd { public ulong Op; public int X, Y, W, H; public uint Color, Pad; public ulong TextPtr, TextLen; }

    const ulong OP_FILL = 0, OP_BLIT = 7;
    const ulong RUN_TICKS = 27; // ~1.5 s at the PIT's 18.2 Hz
    const int BW = 96, BH = 48;
    const int MAX_PROCS = 64, PROC_SIZE = 64;

    static unsafe int Main()
    {
        byte* title = stackalloc byte[6];
        title[0] = (byte)'s'; title[1] = (byte)'m'; title[2] = (byte)'p'; title[3] = (byte)'g';
        title[4] = (byte)'u'; title[5] = (byte)'i';
        uint win = bz_win_create(title, 6, ((ulong)200 << 32) | 120);
        if (win == 0)
            return 2;
        uint[] frame = new uint[BW * BH];
        byte[] procs = new byte[MAX_PROCS * PROC_SIZE];
        byte* stat = stackalloc byte[48];
        long rounds = 0;
        ulong start = bz_ticks();
        while (bz_ticks() - start < RUN_TICKS)
        {
            uint shade = (uint)(rounds * 7) & 0xFF;
            for (int i = 0; i < frame.Length; i++)
                frame[i] = 0xFF000000u | (shade << 16) | ((uint)i & 0xFF);
            DrawCmd fill = new DrawCmd { Op = OP_FILL, X = 0, Y = 0, W = 160, H = 70, Color = 0x203020 };
            if (bz_win_cmd(win, &fill) != 0)
                return 2;
            fixed (uint* fp = frame)
            {
                DrawCmd blit = new DrawCmd { Op = OP_BLIT, X = 4, Y = 4, W = BW, H = BH, TextPtr = (ulong)fp, TextLen = (ulong)(BW * BH * 4) };
                if (bz_win_cmd(win, &blit) != 0)
                    return 2;
            }
            if (bz_sys_stat(stat) != 0)
                return 2;
            if (*(ulong*)stat == 0) // uptime_ticks
                return 1;
            ulong n;
            fixed (byte* pp = procs)
                n = bz_proc_list(pp, MAX_PROCS);
            if (n == 0 || n > MAX_PROCS)
                return 1;
            // A full compose now and then: with the split WM lock the other
            // copies keep drawing into their windows while it runs.
            if ((rounds & 63) == 63)
                bz_win_present(win);
            rounds++;
        }
        // One full compose at the end (outside the timed loop: a compose of the
        // whole desktop takes far longer than a round and would dominate it).
        bz_win_present(win);
        return 100 + (int)rounds;
    }
}
