// Buitenzorg OS — v0.15 "Matang" increment 3: thread sync + TLS + clock PAL.
//
// Two worker threads repeatedly enter a critical section guarded by a
// futex-backed mutex; inside the CS each stamps its own thread id and yields,
// then checks the stamp is still its own (proving no other thread was inside
// at the same time — real mutual exclusion, not just cooperative luck). Also
// exercises the thread-self id (pthread_self foundation) and the monotonic
// clock. These are PAL pieces the .NET runtime (Monitor/lock, ThreadPool,
// Stopwatch, TLS) needs. Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

class SyncDemo
{
    [DllImport("*")] public static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] public static extern ulong bz_spawn_mutex_worker(ulong ctx);
    [DllImport("*")] public static extern ulong bz_thread_join(ulong tid);
    [DllImport("*")] public static extern ulong bz_thread_self();
    [DllImport("*")] public static extern ulong bz_clock_mono();

    const ulong PROT_RW = 1 | 2;
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FFFEUL;
    const ulong ITERS = 500; // per worker; two workers => counter should be 1000

    static unsafe void Main()
    {
        Console.WriteLine("Sync: menguji mutex (futex), thread-self, clock...");

        ulong mem = bz_mmap(4096, PROT_RW);
        if (mem >= ERR_LOW || mem == 0)
        {
            Console.WriteLine("Sync: MMAP GAGAL");
            return;
        }

        // Shared data: mutex(i32)@+0, counter(i64)@+8, token(i64)@+16,
        // error(i32)@+24, slot(i32)@+28, ids[2](i64)@+32,+40.
        // MutexCtx struct @+64: {mutex,counter,token,error,iters,ids,slot}.
        ulong mutexA = mem + 0, counterA = mem + 8, tokenA = mem + 16, errorA = mem + 24;
        ulong slotA = mem + 28, idsA = mem + 32;
        ulong ctxA = mem + 64;
        *(int*)mutexA = 0;
        *(long*)counterA = 0;
        *(long*)tokenA = 0;
        *(int*)errorA = 0;
        *(int*)slotA = 0;
        *(long*)idsA = 0;
        *(long*)(idsA + 8) = 0;
        ulong* ctx = (ulong*)ctxA;
        ctx[0] = mutexA;
        ctx[1] = counterA;
        ctx[2] = tokenA;
        ctx[3] = errorA;
        ctx[4] = ITERS;
        ctx[5] = idsA;
        ctx[6] = slotA;

        ulong t0 = bz_clock_mono();

        // Two workers contending on the same mutex/counter.
        ulong w1 = bz_spawn_mutex_worker(ctxA);
        ulong w2 = bz_spawn_mutex_worker(ctxA);
        if (w1 == 0 || w2 == 0 || w1 == w2)
        {
            Console.WriteLine("Sync: SPAWN GAGAL");
            return;
        }
        bz_thread_join(w1);
        bz_thread_join(w2);

        ulong t1 = bz_clock_mono();

        long counter = *(long*)counterA;
        int error = *(int*)errorA;
        long id0 = *(long*)idsA;
        long id1 = *(long*)(idsA + 8);

        bool countOk = counter == (long)(2 * ITERS);
        bool mutexOk = error == 0;
        bool clockOk = t1 > t0;
        // THREAD_SELF inside each worker must equal the kernel-assigned tid, and
        // the two must be distinct and non-zero.
        bool idsOk = id0 != 0 && id1 != 0 && id0 != id1
                     && ((ulong)id0 == w1 || (ulong)id0 == w2)
                     && ((ulong)id1 == w1 || (ulong)id1 == w2);

        if (countOk && mutexOk && clockOk && idsOk)
            Console.WriteLine("MILESTONE: SYNC OK");
        else if (!mutexOk)
            Console.WriteLine("Sync: mutual exclusion DILANGGAR");
        else
            Console.WriteLine("Sync: verifikasi gagal (count/ids/clock)");
    }
}
