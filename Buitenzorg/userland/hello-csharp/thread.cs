// Buitenzorg OS — v0.15 "Matang" increment 2: cooperative ring-3 threads.
//
// The main thread and a spawned worker thread share this process's address
// space (the counter lives in mmap'd memory), each has its own kernel syscall
// stack, and they are scheduled cooperatively by the kernel. Both bump the
// shared counter 1000 times, yielding between bumps so they interleave; the
// main thread joins the worker and checks the total (2000). This is the
// threading foundation the .NET PAL (pthread_create/join, ThreadPool, Tasks)
// will sit on.
//
// The worker body is provided by the shim (bz_spawn_worker) — a valid native
// entry — so the demo doesn't depend on C# UnmanagedCallersOnly marshaling,
// which zerolib doesn't emit correctly yet. Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

class ThreadDemo
{
    [DllImport("*")] public static extern ulong bz_mmap(ulong size, ulong prot);
    [DllImport("*")] public static extern ulong bz_spawn_worker(ulong counter);
    [DllImport("*")] public static extern ulong bz_thread_join(ulong tid);
    [DllImport("*")] public static extern void bz_yield();

    const ulong PROT_RW = 1 | 2;
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FFFEUL;
    const int K = 1000; // must match the shim worker's iteration count

    static unsafe void Main()
    {
        Console.WriteLine("Thread: menguji thread ring-3 kooperatif...");

        // Shared counter in mmap'd memory (visible to both threads).
        ulong mem = bz_mmap(4096, PROT_RW);
        if (mem >= ERR_LOW || mem == 0)
        {
            Console.WriteLine("Thread: MMAP GAGAL");
            return;
        }
        long* counter = (long*)mem;
        *counter = 0;

        // Spawn the worker (it bumps *counter K times, yielding between bumps).
        ulong tid = bz_spawn_worker(mem);
        if (tid == 0)
        {
            Console.WriteLine("Thread: THREAD_CREATE GAGAL");
            return;
        }

        // Main thread bumps the same counter, yielding so the two interleave.
        for (int i = 0; i < K; i++)
        {
            (*counter)++;
            bz_yield();
        }

        // Wait for the worker to finish, then check the total.
        bz_thread_join(tid);

        if (*counter == 2 * K)
            Console.WriteLine("MILESTONE: THREAD OK");
        else
            Console.WriteLine("Thread: hitungan salah (thread tak berjalan benar)");
    }
}
