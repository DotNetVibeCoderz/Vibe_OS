// Buitenzorg OS — SMP fine-grained locking test: a syscall-heavy program.
//
// The kernel boot test runs one copy per CPU at once, twice: once with every
// syscall behind the Big Kernel Lock and once with the fine-grained path, where
// memory-management, scheduler and console syscalls take only their own
// subsystem locks. Each copy loops for ~1.5 s:
//   mmap two pages (RW) -> stamp them with this process's id -> mprotect them
//   read-only -> verify every stamp -> munmap -> check THREAD_SELF,
// and counts completed rounds. A page that came back with another process's
// stamp, or a failed call, means the kernel mixed up processes or pages.
// Exit code: 100 + rounds = correct; 1 = corruption; 2 = a syscall failed.
// Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

class SmpSys
{
    [DllImport("*")] static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] static extern ulong bz_mprotect(ulong addr, ulong size, ulong prot);
    [DllImport("*")] static extern ulong bz_munmap(ulong addr, ulong size);
    [DllImport("*")] static extern ulong bz_thread_self();
    [DllImport("*")] static extern ulong bz_ticks();

    const ulong PROT_R = 1;
    const ulong PROT_RW = 1 | 2;
    const ulong SIZE = 8192;
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FF00UL;
    const ulong RUN_TICKS = 27; // ~1.5 s at the PIT's 18.2 Hz

    static unsafe int Main()
    {
        ulong id = bz_thread_self();
        long rounds = 0;
        ulong start = bz_ticks();
        while (bz_ticks() - start < RUN_TICKS)
        {
            ulong mem = bz_mmap(SIZE, PROT_RW);
            if (mem == 0 || mem >= ERR_LOW)
                return 2;
            ulong* p = (ulong*)mem;
            int words = (int)(SIZE / 8);
            for (int i = 0; i < words; i += 64)
                p[i] = id * 1000003UL + (ulong)i + (ulong)rounds;
            if (bz_mprotect(mem, SIZE, PROT_R) != 0)
                return 2;
            for (int i = 0; i < words; i += 64)
            {
                if (p[i] != id * 1000003UL + (ulong)i + (ulong)rounds)
                    return 1;
            }
            if (bz_munmap(mem, SIZE) != 0)
                return 2;
            if (bz_thread_self() != id)
                return 1;
            rounds++;
        }
        return 100 + (int)rounds;
    }
}
