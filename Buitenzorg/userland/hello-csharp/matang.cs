// Buitenzorg OS — v0.15 "Matang" milestone program (increment 1).
//
// Exercises the new managed-runtime PAL memory syscalls (MMAP/MPROTECT/MUNMAP)
// end-to-end from ring-3 C#: it maps anonymous pages, writes and reads back a
// pattern through raw pointers (no heap needed), re-protects and unmaps them,
// and only prints the MILESTONE markers when everything checks out. This is the
// foundation the .NET GC will allocate its managed heap on in later increments.
//
// Built with bflat --stdlib:zero (no GC yet) — this step delivers the OS memory
// PAL, not the managed heap. Build: scripts/build-hello-csharp.

using System;
using System.Runtime.InteropServices;

class Matang
{
    [DllImport("*")] public static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] public static extern ulong bz_mprotect(ulong addr, ulong size, ulong prot);
    [DllImport("*")] public static extern ulong bz_munmap(ulong addr, ulong size);

    const ulong PROT_READ = 1;
    const ulong PROT_WRITE = 2;
    // syserr range: NOSYS = 0xFFFF...FF, INVAL = 0xFFFF...FE. A result >= this
    // is a failure. (zerolib has no ulong.MaxValue, so spell it out.)
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FFFEUL;

    static bool Ok(ulong r) => r < ERR_LOW && r != 0;

    static unsafe void Main()
    {
        Console.WriteLine("Matang: menguji PAL memori (mmap/mprotect/munmap)...");

        // 1) Map 64 KiB read/write and prove a write/read roundtrip over every
        //    page (so the whole multi-page range is really backed by frames).
        const ulong size = 64 * 1024;
        ulong addr = bz_mmap(size, PROT_READ | PROT_WRITE);
        if (!Ok(addr))
        {
            Console.WriteLine("Matang: MMAP GAGAL");
            return;
        }
        byte* p = (byte*)addr;
        bool roundtrip = true;
        for (ulong i = 0; i < size; i++)
            p[i] = (byte)(i * 31 + 7);
        for (ulong i = 0; i < size; i++)
            if (p[i] != (byte)(i * 31 + 7)) { roundtrip = false; break; }

        // 2) Re-protect read-only, then unmap (both return 0 on success).
        bool reprotect = bz_mprotect(addr, size, PROT_READ) == 0;
        bool unmapped = bz_munmap(addr, size) == 0;

        // 3) A second, independent mapping must get a distinct address and work.
        ulong addr2 = bz_mmap(4096, PROT_READ | PROT_WRITE);
        bool second = Ok(addr2) && addr2 != addr;
        if (second)
        {
            *(int*)addr2 = 0x1234_5678;
            second = *(int*)addr2 == 0x1234_5678;
        }

        if (roundtrip && second)
            Console.WriteLine("MILESTONE: MMAP OK");
        else
            Console.WriteLine("Matang: PAL memori GAGAL");

        if (reprotect && unmapped)
            Console.WriteLine("Matang: mprotect+munmap ok");

        if (roundtrip && reprotect && unmapped && second)
            Console.WriteLine("MILESTONE: MATANG OK");
    }
}
