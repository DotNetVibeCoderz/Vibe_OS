// Buitenzorg OS — multi-process increment 1: preemptive, isolated processes.
//
// The kernel boot test starts several copies of this program as separate
// processes at once. Each one:
//   - maps one page with MMAP. Every process gets its own address space, so all
//     copies receive the SAME virtual address (0x2000_0000) yet must keep their
//     own contents — it stamps its thread id there and checks it repeatedly;
//   - spins for ~2 s doing double-precision work WITHOUT ever yielding, so the
//     only way the copies can run side by side is timer preemption; the running
//     sum lives in an SSE register, so a context switch that failed to save and
//     restore SSE state (FXSAVE) would corrupt it and the exact-result check
//     below would fail.
// Exit code 0 = isolated + exact, 1 = corrupted, 2 = setup failed.
// Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

class MultiProc
{
    [DllImport("*")] static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] static extern ulong bz_thread_self();
    [DllImport("*")] static extern ulong bz_ticks();

    const ulong PROT_RW = 1 | 2;
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FF00UL;
    const ulong MMAP_BASE = 0x2000_0000UL; // first address every fresh arena hands out
    const ulong RUN_TICKS = 36;            // ~2 s at the PIT's 18.2 Hz
    const long FLOAT_CAP = 40_000_000;     // keeps every partial sum exact in a double

    static unsafe int Main()
    {
        long id = (long)bz_thread_self();
        ulong mem = bz_mmap(4096, PROT_RW);
        if (mem == 0 || mem >= ERR_LOW)
        {
            Console.WriteLine("MPROC: MMAP GAGAL");
            return 2;
        }
        bool ok = mem == MMAP_BASE; // same virtual address in every process
        long* slot = (long*)mem;
        *slot = id;

        double acc = 0;
        long n = 0;
        ulong start = bz_ticks();
        while (bz_ticks() - start < RUN_TICKS)
        {
            if (n < FLOAT_CAP)
            {
                for (int k = 0; k < 100000; k++)
                {
                    acc += n * 0.5 + id;
                    n++;
                }
            }
            else
            {
                for (int k = 0; k < 100000; k++)
                    *slot = *slot ^ 0; // keep busy without changing the stamp
            }
            if (*slot != id)
                ok = false;
        }

        // sum_{i<n} (i/2 + id) = n(n-1)/4 + id*n, exact in a double at this size.
        double expect = (double)(n * (n - 1)) / 4.0 + (double)(id * n);
        if (acc != expect)
            ok = false;

        if (ok)
        {
            Console.WriteLine("MPROC: proses selesai -- memori terisolasi, register SSE utuh");
            return 0;
        }
        Console.WriteLine("MPROC: KORUPSI -- isolasi memori atau state SSE rusak");
        return 1;
    }
}
