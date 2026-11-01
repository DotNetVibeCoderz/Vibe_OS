// Buitenzorg OS — parallel threads within one process (SMP).
//
// Threads of one process now run on different CPUs at the same time. This
// program starts 3 worker threads (plus the main thread waiting) that, for
// ~1.5 s each:
//   - allocate small managed arrays (`new int[16]`) — the heap allocator in the
//     runtime shim must hand every thread distinct memory even when they call
//     it at the same instant on different CPUs;
//   - stamp each array with the worker's number and keep its address;
//   - add their work to a shared total with an atomic add,
// then verify every array they allocated still holds their own stamp. Two
// threads ever receiving overlapping memory, or a lost atomic update, fails it.
// Exit code: 0 = correct; 1 = heap corruption; 2 = setup failed; 3 = lost update.
// Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

unsafe class ParallelThreads
{
    [DllImport("*")] static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] static extern ulong bz_thread_create(ulong entry, ulong arg);
    [DllImport("*")] static extern ulong bz_thread_join(ulong tid);
    [DllImport("*")] static extern ulong bz_ticks();
    [DllImport("*")] static extern long bz_atomic_add(long* p, long v);

    const int WORKERS = 3;
    const int KEEP = 480;          // array addresses each worker keeps to verify
    const ulong RUN_TICKS = 27;    // ~1.5 s at the PIT's 18.2 Hz
    const ulong PROT_RW = 1 | 2;

    // Shared page layout (longs): [0] total work, [1] errors,
    // [8 + w] = rounds of worker w; per-worker pages hold kept addresses.
    static void Worker(ulong arg)
    {
        long* shared = (long*)(arg & ~0xFFFUL);
        long w = (long)(arg & 0xFFF);
        long* kept = (long*)shared[16 + w];
        int n = 0;
        long rounds = 0;
        ulong start = bz_ticks();
        while (bz_ticks() - start < RUN_TICKS)
        {
            int[] a = new int[16];
            int stamp = (int)(w * 1000003 + rounds);
            for (int i = 0; i < a.Length; i++)
                a[i] = stamp;
            if (n < KEEP)
            {
                fixed (int* p = a)
                    kept[2 * n] = (long)p;
                kept[2 * n + 1] = stamp;
                n++;
            }
            long sum = 0;
            for (int i = 0; i < 2000; i++)
                sum += i ^ w;
            bz_atomic_add(&shared[0], 1);
            rounds++;
        }
        // Every kept array must still hold exactly this worker's stamp.
        for (int k = 0; k < n; k++)
        {
            int* p = (int*)kept[2 * k];
            int stamp = (int)kept[2 * k + 1];
            for (int i = 0; i < 16; i++)
            {
                if (p[i] != stamp)
                {
                    bz_atomic_add(&shared[1], 1);
                    break;
                }
            }
        }
        shared[8 + w] = rounds;
    }

    static int Main()
    {
        long* shared = (long*)bz_mmap(4096, PROT_RW);
        if ((ulong)shared == 0 || (ulong)shared >= 0xFFFF_FFFF_FFFF_FF00UL)
            return 2;
        for (int w = 0; w < WORKERS; w++)
        {
            ulong page = bz_mmap((ulong)(KEEP * 16), PROT_RW);
            if (page == 0 || page >= 0xFFFF_FFFF_FFFF_FF00UL)
                return 2;
            shared[16 + w] = (long)page;
        }
        ulong* tids = stackalloc ulong[WORKERS];
        delegate*<ulong, void> entry = &Worker;
        for (int w = 0; w < WORKERS; w++)
        {
            tids[w] = bz_thread_create((ulong)entry, (ulong)shared | (ulong)w);
            if (tids[w] == 0)
                return 2;
        }
        for (int w = 0; w < WORKERS; w++)
            bz_thread_join(tids[w]);
        long rounds = 0;
        for (int w = 0; w < WORKERS; w++)
            rounds += shared[8 + w];
        if (shared[1] != 0)
        {
            Console.WriteLine("PTHREAD: HEAP RUSAK -- dua thread mendapat memori yang sama");
            return 1;
        }
        if (shared[0] != rounds)
        {
            Console.WriteLine("PTHREAD: UPDATE HILANG -- penambahan atomik tidak utuh");
            return 3;
        }
        Console.WriteLine("PTHREAD: 3 thread paralel selesai -- heap & atomik utuh");
        return 0;
    }
}
