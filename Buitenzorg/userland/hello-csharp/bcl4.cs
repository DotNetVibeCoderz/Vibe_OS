// Buitenzorg.Bcl part 4 demo (bzbcl4.cs): tuples, Monitor (Wait/Pulse), reset
// events, semaphore, countdown, thread pool, SynchronizationContext and
// finalizers run by the shim's collector. Prints MILESTONE: BCL4 OK only if
// every check passes.

using System;
using Buitenzorg;

/// A finalizable object: its finalizer counts, and resurrects every 50th.
sealed class Tracked
{
    public static long Finalized, Resurrected;
    public static Tracked Saved;
    public int Id;
    public int[] Payload;
    public Tracked(int id) { Id = id; Payload = new int[8]; Payload[7] = id; }
    ~Tracked()
    {
        BzInterlocked.Increment(ref Finalized);
        if (Id == 4242) { Saved = this; BzInterlocked.Increment(ref Resurrected); }
    }
}

/// Finalizable too, but every instance is suppressed: its finalizer must not run.
sealed class Quiet
{
    public static long Finalized;
    ~Quiet() { BzInterlocked.Increment(ref Finalized); }
}

static unsafe class Bcl4
{
    static int Fails;
    static long Counter;

    static void Check(bool ok, string what)
    {
        BzDebug.Write(ok ? "  ok   " : "  GAGAL ");
        Console.WriteLine(what);
        if (!ok) Fails++;
    }

    static void Num(string label, long v)
    {
        var b = new char[24];
        int n = 0;
        bool neg = v < 0;
        ulong u = neg ? (ulong)(-v) : (ulong)v;
        do { b[n++] = (char)('0' + (int)(u % 10)); u /= 10; } while (u != 0);
        if (neg) b[n++] = '-';
        var r = new char[n];
        for (int i = 0; i < n; i++) r[i] = b[n - 1 - i];
        BzDebug.Write("    ");
        BzDebug.Write(label);
        BzDebug.Write(r, n);
        BzDebug.Write("\n");
    }

    static (int Sum, long Product) SumProduct(int a, int b) => (a + b, (long)a * b);

    // ---- Monitor: a bounded buffer with Wait/Pulse ------------------------
    static readonly BzMonitor BufLock = new BzMonitor();
    static readonly int[] Buf = new int[4];
    static int BufCount, BufHead;
    static long ConsumedSum;

    static void Producer(ulong n)
    {
        for (int i = 1; i <= (int)n; i++)
        {
            BufLock.Enter();
            while (BufCount == Buf.Length) BufLock.Wait();
            Buf[(BufHead + BufCount) % Buf.Length] = i;
            BufCount++;
            BufLock.PulseAll();
            BufLock.Exit();
        }
    }

    static void Consumer(ulong n)
    {
        for (int i = 0; i < (int)n; i++)
        {
            BufLock.Enter();
            while (BufCount == 0) BufLock.Wait();
            ConsumedSum += Buf[BufHead];
            BufHead = (BufHead + 1) % Buf.Length;
            BufCount--;
            BufLock.PulseAll();
            BufLock.Exit();
        }
    }

    // ---- events -------------------------------------------------------------
    static readonly BzManualResetEvent Gate = new BzManualResetEvent();
    static readonly BzAutoResetEvent Turnstile = new BzAutoResetEvent();
    static long Through, GateSeen;
    static readonly BzMonitor Held = new BzMonitor();
    static long TryEnterResult = -1;

    static void GateWaiter(ulong _) { Gate.Wait(); BzInterlocked.Increment(ref GateSeen); }
    static void TurnstileWaiter(ulong _) { Turnstile.Wait(); BzInterlocked.Increment(ref Through); }
    static void TryHeld(ulong _) { TryEnterResult = Held.TryEnter(30) ? 1 : 0; if (TryEnterResult == 1) Held.Exit(); }

    static unsafe ulong Spawn(delegate*<ulong, void> f, ulong arg) => BzSync.bz_thread_create((ulong)f, arg);
    static void Join(ulong t) { if (t != 0) BzSync.bz_thread_join(t); }

    static void MakeGarbage(int n)
    {
        for (int i = 0; i < n; i++) new Tracked(i);
        new Tracked(4242);
    }

    static void MakeQuiet(int n)
    {
        for (int i = 0; i < n; i++) BzFinalizer.SuppressFinalize(new Quiet());
    }

    static int Main()
    {
        Console.WriteLine("Bcl4: Tuple, Monitor, event, semaphore, thread pool, SynchronizationContext, finalizer...");

        // ---- tuples ----
        var t = Tuple.Create(7, "tujuh");
        var (num, word) = t;
        var t3 = Tuple.Create(1, 2L, 'c');
        var sp = SumProduct(6, 7);
        (int x, int y) swap = (1, 2);
        swap = (swap.y, swap.x);
        Check(num == 7 && (object)word == (object)"tujuh" && t3.Item3 == 'c' && sp.Sum == 13 && sp.Product == 42
              && swap.x == 2 && swap.y == 1, "Tuple.Create / dekonstruksi / ValueTuple bernama / tukar");

        // ---- Monitor ----
        var m = new BzMonitor();
        m.Enter(); m.Enter();
        bool reentrant = m.IsEntered;
        m.Exit();
        bool stillHeld = m.IsEntered;
        m.Exit();
        ulong t0 = BzSync.bz_clock_nanos();
        m.Enter();
        bool timedOut = !m.Wait(40);
        ulong waited = (BzSync.bz_clock_nanos() - t0) / 1000000;
        m.Exit();
        Check(reentrant && stillHeld && !m.IsEntered && timedOut && waited >= 30 && waited < 2000,
              "Monitor: reentran, Wait(40 ms) habis waktu");
        Num("Wait(40) menunggu ms: ", (long)waited);

        Held.Enter();
        ulong th = Spawn(&TryHeld, 0);
        Join(th);
        Held.Exit();
        Check(TryEnterResult == 0, "Monitor.TryEnter(30) dari thread lain gagal selama dipegang");

        ulong p1 = Spawn(&Producer, 500), c1 = Spawn(&Consumer, 250), c2 = Spawn(&Consumer, 250);
        Join(p1); Join(c1); Join(c2);
        Check(ConsumedSum == 500L * 501 / 2 && BufCount == 0, "Monitor Wait/PulseAll: buffer 4 slot, 1 produsen + 2 konsumen, 500 item");

        // ---- events ----
        ulong g1 = Spawn(&GateWaiter, 0), g2 = Spawn(&GateWaiter, 0), g3 = Spawn(&GateWaiter, 0);
        bool notYet = !Gate.Wait(20) && GateSeen == 0;
        Gate.Set();
        Join(g1); Join(g2); Join(g3);
        Check(notYet && GateSeen == 3 && Gate.IsSet && Gate.Wait(0), "ManualResetEvent: Wait habis waktu, Set melepas 3 thread");

        ulong a1 = Spawn(&TurnstileWaiter, 0), a2 = Spawn(&TurnstileWaiter, 0), a3 = Spawn(&TurnstileWaiter, 0);
        int rounds = 0;
        while (BzInterlocked.Read(ref Through) < 3 && rounds < 300)
        {
            long before = BzInterlocked.Read(ref Through);
            Turnstile.Set();
            for (int w = 0; w < 50 && BzInterlocked.Read(ref Through) == before; w++) new BzManualResetEvent().Wait(2);
            rounds++;
        }
        Join(a1); Join(a2); Join(a3);
        Check(Through == 3 && !Turnstile.Wait(10), "AutoResetEvent: tiap Set melepas satu thread, lalu tertutup lagi");

        var sem = new BzSemaphore(2);
        bool s1 = sem.Wait(0), s2 = sem.Wait(0), s3 = sem.Wait(20);
        sem.Release(2);
        Check(s1 && s2 && !s3 && sem.CurrentCount == 2, "SemaphoreSlim: 2 izin, yang ketiga habis waktu, Release(2)");

        // ---- thread pool ----
        BzThreadPool.SetWorkers(3);
        var cd = new BzCountdownEvent(200);
        for (int i = 0; i < 200; i++)
            BzThreadPool.QueueUserWorkItem(() => { BzInterlocked.Increment(ref Counter); cd.Signal(); });
        bool counted = cd.Wait(10000);
        var f = BzThreadPool.Run(() => 6 * 7);
        var bad = BzThreadPool.Run<int>(() => throw new InvalidOperationException("sengaja"));
        bool rethrown = false;
        try { int unused = bad.Result; } catch (InvalidOperationException) { rethrown = true; }
        bool idle = BzThreadPool.WaitIdle(10000);
        Check(counted && Counter == 200 && f.Result == 42 && rethrown && bad.IsFaulted && idle
              && BzThreadPool.WorkerCount == 3 && BzThreadPool.Pending == 0,
              "ThreadPool: 200 item di 3 worker, Run<T> -> 42, pengecualian diteruskan, WaitIdle");
        int faultsBefore = (int)BzThreadPool.Faults;
        BzThreadPool.QueueUserWorkItem(() => throw new InvalidOperationException("lepas"));
        BzThreadPool.WaitIdle(10000);
        var after = BzThreadPool.Run(() => 1);
        Check(BzThreadPool.Faults == faultsBefore + 1 && after.Result == 1, "ThreadPool: pengecualian yang lepas dihitung, worker tetap jalan");

        // ---- SynchronizationContext ----
        var loop = new BzLoopContext();
        BzSynchronizationContext.SetSynchronizationContext(loop);
        long mainThread = BzSync.ThreadId;
        long onOwner = 0, posted = 0, sentBack = 0;
        var poolSaw = BzThreadPool.Run(() => BzSynchronizationContext.Current == null ? 1 : 0);
        for (int i = 0; i < 10; i++)
            BzThreadPool.QueueUserWorkItem(() =>
                loop.Post(() => { posted++; if (BzSync.ThreadId == mainThread) onOwner++; }));
        var sender = BzThreadPool.Run(() => { loop.Send(() => sentBack = BzSync.ThreadId); return 1; });
        bool pumped = loop.RunUntil(() => posted == 10 && sender.IsCompleted, 10000);
        Check(pumped && onOwner == 10 && sentBack == mainThread && sender.Result == 1
              && BzSynchronizationContext.Current == loop && poolSaw.Result == 1,
              "SynchronizationContext: Post dari worker jalan di thread pemilik, Send menunggu, Current per thread");
        BzSynchronizationContext.SetSynchronizationContext(null);
        Check(BzSynchronizationContext.Current == null, "SetSynchronizationContext(null)");

        // ---- finalizers ----
        var keep = new Tracked(-1);
        MakeGarbage(1000);
        MakeQuiet(100);
        BzGC.Collect();
        long ran = BzFinalizer.WaitForPendingFinalizers();
        BzGC.Collect();
        BzFinalizer.WaitForPendingFinalizers();
        Num("finalizer jalan: ", Tracked.Finalized);
        Num("terdaftar: ", BzFinalizer.Registered);
        Check(Tracked.Finalized >= 900 && Tracked.Finalized <= 1001 && ran >= 900, "finalizer ~T() dari 1000 objek mati dijalankan thread finalizer");
        Check(Quiet.Finalized == 0, "GC.SuppressFinalize: 100 objek, tidak satu pun difinalisasi");
        Check(keep.Id == -1 && keep.Payload[7] == -1 && BzFinalizer.Registered >= 1, "objek yang masih dipakai tidak difinalisasi");
        Tracked back = Tracked.Saved;
        Check(Tracked.Resurrected == 1 && back != null && back.Id == 4242 && back.Payload[7] == 4242,
              "kebangkitan: finalizer menyimpan 'this', isinya (array) tetap utuh");
        BzFinalizer.ReRegisterForFinalize(back);
        Tracked.Saved = null;
        back = null;
        long beforeRe = Tracked.Finalized;
        BzGC.Collect();
        BzFinalizer.WaitForPendingFinalizers();
        Check(Tracked.Finalized >= beforeRe && BzFinalizer.Pending == 0, "ReRegisterForFinalize + antrean kosong setelah WaitForPendingFinalizers");
        GC_KeepAlive(keep);

        Console.WriteLine(Fails == 0 ? "MILESTONE: BCL4 OK" : "MILESTONE: BCL4 FAIL");
        return Fails == 0 ? 0 : 1;
    }

    static void GC_KeepAlive(object o) { if (o == null) Console.WriteLine(""); }
}
