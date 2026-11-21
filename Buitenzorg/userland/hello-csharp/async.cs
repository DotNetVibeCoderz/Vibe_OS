// async/await in native C# (bflat zerolib) on the bzasync.cs event loop.
// Build: async.cs + bzbcl.cs + bzbcl2.cs + bzbcl3.cs + bzasync.cs -> ASYNC.ELF
// Prints MILESTONE: ASYNC OK when every check passes.

using System;
using System.Threading.Tasks;
using Buitenzorg;

static class Program
{
    [System.Runtime.InteropServices.DllImport("*")] static extern ulong bz_clock_nanos();
    static int _fails, _checks;
    static Task<int> _later;
    static int _voids;
    // the interleaving test writes letters here (no string concat in zerolib)
    static char[] _trace;
    static int _traceLen;

    static void Check(bool ok, string what)
    {
        _checks++;
        if (!ok) _fails++;
        BzDebug.Write(ok ? "  ok   " : "  FAIL ");
        BzDebug.WriteLine(what);
    }

    static void Trace(char c) { if (_traceLen < _trace.Length) _trace[_traceLen++] = c; }

    static Task<int> Value(int n, bool later)
    {
        if (later) { _later = new Task<int>(); return _later; }
        return Task.FromResult(n * 2);
    }

    static async Task<int> Chain(int n)
    {
        int a = await Value(n, false);     // completed: no suspension
        int b = await Value(a, true);      // pending: suspends until _later completes
        return a + b;
    }

    static async Task Ping(char c, int rounds)
    {
        for (int i = 0; i < rounds; i++)
        {
            Trace(c);
            await Task.Yield();
        }
    }

    static async Task Sleeper(char c, int ms)
    {
        await Task.Delay(ms);
        Trace(c);
    }

    static async Task<int> Sum(int n)
    {
        int s = 0;
        for (int i = 1; i <= n; i++)
        {
            s += i;
            if (i % 10 == 0) await Task.Yield();
        }
        return s;
    }

    static async void Handler() { await Task.Yield(); _voids++; }

    static async Task<int> Failing(int code)
    {
        await Task.Yield();
        if (code > 0) throw new InvalidOperationException("async failure");
        return 0;
    }

    static async Task<int> Main2()
    {
        // 1. a chain that suspends on a pending task and resumes when it completes
        var t = Chain(1);
        Check(!t.IsCompleted, "await on a pending task suspends the method");
        // complete it from queued work (all sources are one assembly: internal is visible)
        Task.Run(() => { _later.SetResult(8); });
        int r = await t;
        Check(r == 10, "the method resumes with the result when the task completes (2 + 8)");

        // 2. Task.Yield interleaves two coroutines
        _traceLen = 0;
        await Task.WhenAll(Ping('a', 3), Ping('b', 3));
        Check(_traceLen == 6 && _trace[0] == 'a' && _trace[1] == 'b' && _trace[2] == 'a' && _trace[5] == 'b', "Task.Yield interleaves: a b a b a b");

        // 3. timers fire in due order, not start order
        _traceLen = 0;
        ulong t0 = bz_clock_nanos();
        await Task.WhenAll(Sleeper('x', 60), Sleeper('y', 10), Sleeper('z', 30));
        ulong ms = (bz_clock_nanos() - t0) / 1_000_000UL;
        Check(_traceLen == 3 && _trace[0] == 'y' && _trace[1] == 'z' && _trace[2] == 'x', "Task.Delay timers complete in due order (10, 30, 60 ms)");
        Check(ms >= 55 && ms < 2000, "WhenAll waited for the longest delay (~60 ms)");

        // 4. WhenAny completes with the first task
        var slow = Task.Delay(200);
        var fast = Task.Delay(5);
        var first = await Task.WhenAny(slow, fast);
        Check((object)first == (object)fast && !slow.IsCompleted, "WhenAny completes with the first task");

        // 5. Task.Run<T> on the loop, async Task<T> with many suspensions
        int v = await Task.Run(() => 6 * 7);
        Check(v == 42, "Task.Run<T> returns its value");
        int s = await Sum(1000);
        Check(s == 500500, "an async loop with 100 suspensions computes 1..1000 = 500500");

        // 6. async void and blocking Result (the loop is pumped while waiting)
        Handler();
        Handler();
        await Task.Yield();
        await Task.Yield();
        Check(_voids == 2, "async void handlers ran to completion");
        var pending = Sum(50);
        Check(pending.Result == 1275, "Task.Result on a pending task pumps the loop until it is done");

        // 7. an exception escaping an async method faults its task; await rethrows it
        var failing = Failing(3);
        bool caught = false;
        try { await failing; }
        catch (InvalidOperationException) { caught = true; }
        Check(caught && failing.IsFaulted, "await on a faulted task throws its exception");
        return 1;
    }

    static int Main()
    {
        _trace = new char[16];
        BzDebug.WriteLine("async/await on native C# (bzasync.cs event loop):");
        bool ran = BzAsync.Run(async () => { await Main2(); });
        Check(ran, "BzAsync.Run pumped the program to completion");
        if (_fails == 0)
        {
            BzDebug.WriteLine("MILESTONE: ASYNC OK (native C#: async Task / Task<T> / void, await, Yield, Delay, WhenAll, WhenAny, Task.Run, Result, exceptions through await)");
            return 0;
        }
        BzDebug.WriteLine("async: checks FAILED");
        return 1;
    }
}
