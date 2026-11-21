// Buitenzorg OS -- native C# (zerolib): exceptions.
//
// zerolib has no exception machinery; bflat dropped the clause tables too.
// Programs are now compiled with the upstream ILC (unwind + clause tables)
// and the startup shim has a dispatcher (bzeh.rs). This app checks the
// semantics .NET programs rely on and prints MILESTONE: EXCEPTIONS OK.
// Built with bzexc.cs (the exception types).

using System;
using System.IO;
using System.Runtime.InteropServices;

class CodeException : Exception
{
    public int Code;
    public CodeException(int code) : base("code exception") { Code = code; }
}

class Resource : IDisposable
{
    public static int Disposed;
    public void Dispose() { Disposed++; }
}

delegate int IntFunc(int x);

struct Pair<T>
{
    public T A;
    public T B;
}

static class Exc
{
    static int s_log;
    static int s_failures;
    static int s_threadResult;

    [DllImport("*")] static extern ulong bz_thread_create(ulong entry, ulong arg);
    [DllImport("*")] static extern ulong bz_thread_join(ulong tid);

    static void Check(bool ok, string what)
    {
        if (!ok)
        {
            s_failures++;
            Console.WriteLine(what);
        }
    }

    static void Log(int digit) { s_log = s_log * 10 + digit; }

    static int Deep(int n, int code)
    {
        if (n == 0) throw new CodeException(code);
        return Deep(n - 1, code) + 1;
    }

    static int ThroughFinally()
    {
        try
        {
            try { Deep(3, 5); }
            finally { Log(1); }
        }
        finally { Log(2); }
        return 0;
    }

    static int Filter(int code)
    {
        try { Deep(1, code); }
        catch (CodeException e) when (e.Code == 7) { return 70; }
        catch (CodeException e) { return e.Code; }
        return -1;
    }

    static Exception Rethrow(Exception ex)
    {
        try
        {
            try { throw ex; }
            catch (Exception) { Log(3); throw; }
        }
        catch (Exception e) { return e; }
    }

    static int ThrowInCatch()
    {
        try
        {
            try { Deep(2, 1); }
            catch (CodeException) { Log(4); throw new ArgumentException("second"); }
        }
        catch (ArgumentException e) { return e.Message == null ? 0 : 2; }
        catch (CodeException) { return 1; }
        return 0;
    }

    static int ThrowInFinally()
    {
        try
        {
            try { Deep(1, 9); }
            finally { Log(5); throw new InvalidOperationException("from finally"); }
        }
        catch (InvalidOperationException) { return 2; }
        catch (CodeException) { return 1; }
    }

    static T Generic<T>(T value, bool fail)
    {
        Pair<T> p;
        p.A = value;
        p.B = value;
        if (fail) throw new NotSupportedException("generic");
        return p.B;
    }

    static int ThroughDelegate(IntFunc f, int v)
    {
        try { return f(v); }
        catch (FormatException) { return -5; }
    }

    static void Worker(ulong arg)
    {
        int r = 0;
        try { Deep(4, 11); }
        catch (CodeException e) { r = e.Code; }
        finally { r += 100; }
        s_threadResult = r;
    }

    static int Main()
    {
        Console.WriteLine("Exceptions: menguji throw / catch / finally...");

        // 1. across several frames, typed catch, the object arrives intact
        int got = 0;
        try { Deep(5, 42); }
        catch (CodeException e) { got = e.Code; }
        Check(got == 42, "FAIL 1: catch across frames");

        // 2. a catch of the base class
        bool baseCaught = false;
        try { throw new ArgumentNullException("x"); }
        catch (Exception e) { baseCaught = e is ArgumentException; }
        Check(baseCaught, "FAIL 2: catch of a base class");

        // 3. a non-matching inner catch passes it on
        int which = 0;
        try
        {
            try { throw new ArgumentException("arg"); }
            catch (IOException) { which = 1; }
        }
        catch (ArgumentException) { which = 2; }
        Check(which == 2, "FAIL 3: non-matching catch skipped");

        // 4. finally blocks run inner-first on the way out, before the catch
        s_log = 0;
        int code = 0;
        try { ThroughFinally(); }
        catch (CodeException e) { code = e.Code; Log(9); }
        Check(s_log == 129 && code == 5, "FAIL 4: finally order");

        // 5. finally on the normal path
        s_log = 0;
        try { Log(6); }
        finally { Log(7); }
        Check(s_log == 67, "FAIL 5: finally without an exception");

        // 6. exception filters
        Check(Filter(7) == 70 && Filter(5) == 5, "FAIL 6: catch ... when");

        // 7. throw; keeps the same object
        s_log = 0;
        var original = new CodeException(3);
        Check(Rethrow(original) == original && s_log == 3, "FAIL 7: throw; rethrow");

        // 8. a new exception thrown inside a catch block
        s_log = 0;
        Check(ThrowInCatch() == 2 && s_log == 4, "FAIL 8: throw inside catch");

        // 9. an exception thrown inside a finally block replaces the first
        s_log = 0;
        Check(ThrowInFinally() == 2 && s_log == 5, "FAIL 9: throw inside finally");

        // 10. catch without a type, using / Dispose on the way out
        bool any = false;
        Resource.Disposed = 0;
        try
        {
            using (var r = new Resource()) { throw new TimeoutException(); }
        }
        catch { any = true; }
        Check(any && Resource.Disposed == 1, "FAIL 10: catch-all + using");

        // 11. generic code and delegates
        bool genericCaught = false;
        try { Generic(1.5, true); }
        catch (NotSupportedException) { genericCaught = true; }
        Check(genericCaught && Generic(7, false) == 7, "FAIL 11a: generic method");
        IntFunc parse = x => { if (x < 0) throw new FormatException(); return x * 2; };
        Check(ThroughDelegate(parse, 4) == 8 && ThroughDelegate(parse, -1) == -5, "FAIL 11b: through a delegate");

        // 12. many exceptions in a loop (each one is a new object)
        int caught = 0;
        for (int i = 0; i < 2000; i++)
        {
            try { if ((i & 1) == 0) throw new CodeException(i); caught += 0; }
            catch (CodeException e) { if (e.Code == i) caught++; }
        }
        Check(caught == 1000, "FAIL 12: exceptions in a loop");

        // 13. on another thread (its own stack)
        unsafe
        {
            delegate*<ulong, void> entry = &Worker;
            ulong tid = bz_thread_create((ulong)entry, 0);
            if (tid != 0) bz_thread_join(tid);
        }
        Check(s_threadResult == 111, "FAIL 13: exception on a worker thread");

        if (s_failures == 0)
        {
            Console.WriteLine("MILESTONE: EXCEPTIONS OK (throw/catch across frames, base-class catch, finally order, filters, throw;, throw inside catch and finally, catch-all + using, generics, delegates, 2000 throws, worker thread)");
            return 0;
        }
        return 1;
    }
}
