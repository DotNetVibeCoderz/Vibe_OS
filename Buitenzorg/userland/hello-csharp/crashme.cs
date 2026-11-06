// Buitenzorg OS — native C# (zerolib): crashes on purpose, three calls deep.
//
// Level3 indexes a null array: the length read at address 0x8 is a ring-3
// page fault. The kernel ends the program (exit 0xDEAD) and writes a crash
// report whose backtrace must name Level3 <- Level2 <- Level1 from the ELF's
// symbol table (crash.rs) -- the boot checks it: MILESTONE: CRASH REPORT OK.
// Built with bflat --stdlib:zero.

using System.Runtime.CompilerServices;

static class Crash
{
    [MethodImpl(MethodImplOptions.NoInlining)]
    static int Level3(int[] values, int i) => values[i] + 1;

    [MethodImpl(MethodImplOptions.NoInlining)]
    static int Level2(int[] values, int i) => Level3(values, i) * 2 + i;

    [MethodImpl(MethodImplOptions.NoInlining)]
    static int Level1(int[] values, int i) => Level2(values, i + 1) - 3;

    static int Main()
    {
        int[] none = Pick(0);
        return Level1(none, 0);
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    static int[] Pick(int n) => n == 0 ? null : new int[n];
}
