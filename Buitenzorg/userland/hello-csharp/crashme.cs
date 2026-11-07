// Buitenzorg OS — native C# (zerolib): crashes on purpose, three calls deep.
//
// Level3 indexes a null array: the length read at address 0x8 is a ring-3
// page fault. The kernel ends the program (exit 0xDEAD) and writes a crash
// report whose backtrace must name Level3 <- Level2 <- Level1 from the ELF's
// symbol table (crash.rs) -- the boot checks it: MILESTONE: CRASH REPORT OK.
// With the argument `spin` it does not crash: it spends ~1.2 s in Hot and
// ~0.3 s in Cold, for the per-app profiler's boot check (appprof.rs,
// MILESTONE: APP PROFILER OK). Built with bflat --stdlib:zero.

using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

static class Crash
{
    [MethodImpl(MethodImplOptions.NoInlining)]
    static int Level3(int[] values, int i) => values[i] + 1;

    [MethodImpl(MethodImplOptions.NoInlining)]
    static int Level2(int[] values, int i) => Level3(values, i) * 2 + i;

    [MethodImpl(MethodImplOptions.NoInlining)]
    static int Level1(int[] values, int i) => Level2(values, i + 1) - 3;

    [DllImport("*")] static extern unsafe ulong bz_proc_args(byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_clock_nanos();

    // busy work until the clock passes `until` (kept in a non-inlined method
    // so its samples land on its own symbol)
    [MethodImpl(MethodImplOptions.NoInlining)]
    static ulong Hot(ulong until)
    {
        ulong x = 1;
        while (bz_clock_nanos() < until)
            for (int i = 0; i < 200000; i++) x = x * 6364136223846793005UL + 1442695040888963407UL;
        return x;
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    static ulong Cold(ulong until)
    {
        ulong x = 7;
        while (bz_clock_nanos() < until)
            for (int i = 0; i < 200000; i++) x = x * 2862933555777941757UL + 3037000493UL;
        return x;
    }

    static unsafe bool Spin()
    {
        byte* a = stackalloc byte[16];
        ulong n = bz_proc_args(a, 16);
        return n >= 4 && a[0] == (byte)'s' && a[1] == (byte)'p' && a[2] == (byte)'i' && a[3] == (byte)'n';
    }

    static int Main()
    {
        if (Spin())
        {
            ulong t = bz_clock_nanos();
            ulong r = Hot(t + 1_200_000_000UL) ^ Cold(t + 1_500_000_000UL);
            return r == 42 ? 1 : 0;
        }
        int[] none = Pick(0);
        return Level1(none, 0);
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    static int[] Pick(int n) => n == 0 ? null : new int[n];
}
