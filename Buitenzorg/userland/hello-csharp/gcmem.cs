// Buitenzorg OS — v0.15 "Matang" increment 5: GC memory model (reserve/commit).
//
// The .NET GC reserves a large managed heap up front (mmap PROT_NONE, no
// frames) and commits sub-ranges on demand (mprotect READ|WRITE). This app
// proves that model works: it reserves 256 MiB lazily — which would run the
// machine out of physical RAM (~512 MiB, mostly used) if the reservation
// eagerly committed frames — then commits two individual pages and uses them.
// Success means reservation is lazy and commit-on-demand works. Built with
// bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

class GcMem
{
    [DllImport("*")] public static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] public static extern ulong bz_mprotect(ulong addr, ulong size, ulong prot);

    const ulong PROT_NONE = 0;
    const ulong PROT_RW = 1 | 2;
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FFFEUL;

    static bool Ok(ulong r) => r < ERR_LOW && r != 0;

    static unsafe void Main()
    {
        Console.WriteLine("GcMem: menguji reserve (lazy) + commit-on-demand...");

        // Reserve 256 MiB with no access — must NOT allocate frames.
        const ulong RESERVE = 256UL * 1024 * 1024;
        ulong region = bz_mmap(RESERVE, PROT_NONE);
        if (!Ok(region))
        {
            // A failure here means the reservation tried to commit 256 MiB and
            // ran out of frames — i.e., reservation was not lazy.
            Console.WriteLine("GcMem: RESERVE GAGAL (tidak lazy)");
            return;
        }

        // Commit the first page and use it.
        bool ok0 = bz_mprotect(region, 4096, PROT_RW) == 0;
        if (ok0)
        {
            *(long*)region = 0x1111_2222_3333_4444;
            ok0 = *(long*)region == 0x1111_2222_3333_4444;
        }

        // Commit a page 1 MiB into the reservation and use it.
        ulong page2 = region + (1024 * 1024);
        bool ok1 = bz_mprotect(page2, 4096, PROT_RW) == 0;
        if (ok1)
        {
            *(int*)page2 = 0x5678;
            ok1 = *(int*)page2 == 0x5678;
        }

        if (ok0 && ok1)
            Console.WriteLine("MILESTONE: GCMEM OK");
        else
            Console.WriteLine("GcMem: commit-on-demand GAGAL");
    }
}
