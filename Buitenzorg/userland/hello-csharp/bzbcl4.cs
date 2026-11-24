// Buitenzorg.Bcl, part 4: threading and tuples for native (zerolib) C#.
//
//   System.Tuple            Tuple<T1,T2>, Tuple<T1,T2,T3>, Tuple.Create
//                           (ValueTuple -- `(a, b)` syntax -- is in zerolib already)
//   BzMonitor               Monitor: reentrant Enter/TryEnter/Exit, Wait (with a
//                           timeout), Pulse, PulseAll -- a condition variable
//   BzManualResetEvent      ManualResetEvent(Slim): Set/Reset/Wait(timeout)
//   BzAutoResetEvent        AutoResetEvent: Set releases one waiter
//   BzSemaphore             SemaphoreSlim: Wait/Release(n)/CurrentCount
//   BzCountdownEvent        CountdownEvent: Signal/AddCount/Wait
//   BzThreadPool            ThreadPool: QueueUserWorkItem on a fixed set of worker
//                           threads, Run<T> -> BzFuture<T>, WaitIdle
//   BzSynchronizationContext SynchronizationContext: Post/Send, per-thread Current;
//                           BzLoopContext runs posted work on its own thread
//   BzFinalizer             GC.SuppressFinalize / ReRegisterForFinalize /
//                           WaitForPendingFinalizers (the shim's collector runs
//                           `~T()` finalizers on a finalizer thread)
//
// Every blocking call waits in the kernel (FUTEX_WAIT), never spinning, and is a
// GC safepoint. The futex words are fields of heap objects: the collector does
// not move objects, so their addresses stay valid while the object lives.
// Needs bzbcl3.cs (Action / Func).

using System;
using System.Runtime.InteropServices;

namespace System
{
    public class Tuple<T1, T2>
    {
        public T1 Item1 { get; }
        public T2 Item2 { get; }
        public Tuple(T1 item1, T2 item2) { Item1 = item1; Item2 = item2; }
        public void Deconstruct(out T1 item1, out T2 item2) { item1 = Item1; item2 = Item2; }
    }

    public class Tuple<T1, T2, T3>
    {
        public T1 Item1 { get; }
        public T2 Item2 { get; }
        public T3 Item3 { get; }
        public Tuple(T1 item1, T2 item2, T3 item3) { Item1 = item1; Item2 = item2; Item3 = item3; }
        public void Deconstruct(out T1 item1, out T2 item2, out T3 item3) { item1 = Item1; item2 = Item2; item3 = Item3; }
    }

    public static class Tuple
    {
        public static Tuple<T1, T2> Create<T1, T2>(T1 item1, T2 item2) => new Tuple<T1, T2>(item1, item2);
        public static Tuple<T1, T2, T3> Create<T1, T2, T3>(T1 item1, T2 item2, T3 item3) => new Tuple<T1, T2, T3>(item1, item2, item3);
    }
}

namespace System.Runtime.CompilerServices
{
    /// <summary>Element names of a tuple in a signature (<c>(int Sum, long Product)</c>);
    /// the compiler needs the type to exist.</summary>
    public sealed class TupleElementNamesAttribute : Attribute
    {
        readonly string[] _names;
        public TupleElementNamesAttribute(string[] transformNames) { _names = transformNames; }
        public string[] TransformNames => _names;
    }
}

namespace Buitenzorg
{
    /// <summary>The shim's futex and thread calls shared by the types below.</summary>
    internal static unsafe class BzSync
    {
        [DllImport("*")] internal static extern ulong bz_futex_wait(int* addr, ulong expected);
        [DllImport("*")] internal static extern ulong bz_futex_wait_timeout(int* addr, ulong expected, ulong ns);
        [DllImport("*")] internal static extern ulong bz_futex_wake(int* addr, ulong count);
        [DllImport("*")] internal static extern ulong bz_thread_self();
        [DllImport("*")] internal static extern ulong bz_thread_create(ulong entry, ulong arg);
        [DllImport("*")] internal static extern ulong bz_thread_join(ulong tid);
        [DllImport("*")] internal static extern ulong bz_clock_nanos();
        [DllImport("*")] internal static extern int bz_atomic_add32(int* p, int v);
        [DllImport("*")] internal static extern int bz_atomic_cas32(int* p, int expected, int value);
        [DllImport("*")] internal static extern int bz_atomic_xchg32(int* p, int value);

        /// <summary>Wait while *p == expected; false if <paramref name="deadline"/>
        /// (CLOCK_NANOS, 0 = none) passed.</summary>
        internal static bool WaitWhile(int* p, int expected, ulong deadline)
        {
            if (deadline == 0)
            {
                bz_futex_wait(p, (ulong)(uint)expected);
                return true;
            }
            ulong now = bz_clock_nanos();
            if (now >= deadline) return false;
            bz_futex_wait_timeout(p, (ulong)(uint)expected, deadline - now);
            return bz_clock_nanos() < deadline || *p != expected;
        }

        internal static ulong Deadline(int timeoutMs) =>
            timeoutMs < 0 ? 0 : bz_clock_nanos() + (ulong)timeoutMs * 1000000UL + 1;

        internal static long ThreadId => (long)bz_thread_self();

        /// <summary>A fresh read of a word other threads change.</summary>
        internal static int Load(int* p) => bz_atomic_add32(p, 0);
    }

    /// <summary>Monitor (System.Threading.Monitor) as an object of its own: a
    /// reentrant lock plus a condition variable. <c>Wait</c> releases the lock
    /// (all recursion levels), sleeps until a <c>Pulse</c> / <c>PulseAll</c> or the
    /// timeout, and takes it back. As with any condition variable, check the
    /// condition in a loop around <c>Wait</c>.</summary>
    public sealed unsafe class BzMonitor
    {
        int _lock;      // 0 free, 1 held, 2 held with waiters
        int _seq;       // bumped by every pulse
        long _owner = -1;
        int _depth;

        public bool IsEntered => _owner == BzSync.ThreadId;

        public void Enter() => TryEnter(-1);

        /// <summary>Take the lock within <paramref name="timeoutMs"/> (-1 = wait for ever).</summary>
        public bool TryEnter(int timeoutMs)
        {
            long me = BzSync.ThreadId;
            if (_owner == me) { _depth++; return true; }
            ulong deadline = BzSync.Deadline(timeoutMs);
            fixed (int* p = &_lock)
            {
                if (BzSync.bz_atomic_cas32(p, 0, 1) != 0)
                {
                    // contended: mark "held with waiters", sleep until released
                    while (BzSync.bz_atomic_xchg32(p, 2) != 0)
                        if (!BzSync.WaitWhile(p, 2, deadline) && BzSync.bz_atomic_cas32(p, 0, 2) != 0)
                            return false;
                }
            }
            _owner = me;
            _depth = 1;
            return true;
        }

        public void Exit()
        {
            if (_owner != BzSync.ThreadId) throw new InvalidOperationException("Object synchronization method was called from an unsynchronized block of code.");
            if (--_depth > 0) return;
            _owner = -1;
            fixed (int* p = &_lock)
                if (BzSync.bz_atomic_xchg32(p, 0) == 2) BzSync.bz_futex_wake(p, 1);
        }

        /// <summary>Release, wait for a pulse (or <paramref name="timeoutMs"/>), re-enter.
        /// False if it timed out.</summary>
        public bool Wait(int timeoutMs = -1)
        {
            if (_owner != BzSync.ThreadId) throw new InvalidOperationException("Object synchronization method was called from an unsynchronized block of code.");
            int depth = _depth;
            int seq = _seq;
            _depth = 1;
            Exit();
            bool pulsed;
            fixed (int* s = &_seq)
                pulsed = BzSync.WaitWhile(s, seq, BzSync.Deadline(timeoutMs)) || _seq != seq;
            Enter();
            _depth = depth;
            return pulsed;
        }

        public void Pulse() => Signal(1);
        public void PulseAll() => Signal(0x7fffffff);

        void Signal(int n)
        {
            if (_owner != BzSync.ThreadId) throw new InvalidOperationException("Object synchronization method was called from an unsynchronized block of code.");
            fixed (int* s = &_seq)
            {
                BzSync.bz_atomic_add32(s, 1);
                BzSync.bz_futex_wake(s, (ulong)(uint)n);
            }
        }
    }

    /// <summary>ManualResetEvent(Slim): once set, every Wait returns until Reset.</summary>
    public sealed unsafe class BzManualResetEvent
    {
        int _state;
        public BzManualResetEvent(bool initialState = false) { _state = initialState ? 1 : 0; }
        public bool IsSet => _state != 0;

        public void Set()
        {
            fixed (int* p = &_state)
                if (BzSync.bz_atomic_xchg32(p, 1) == 0) BzSync.bz_futex_wake(p, 0xffffffffUL);
        }

        public void Reset() { fixed (int* p = &_state) BzSync.bz_atomic_xchg32(p, 0); }

        public bool Wait(int timeoutMs = -1)
        {
            ulong deadline = BzSync.Deadline(timeoutMs);
            fixed (int* p = &_state)
                while (BzSync.Load(p) == 0)
                    if (!BzSync.WaitWhile(p, 0, deadline)) return BzSync.Load(p) != 0;
            return true;
        }
    }

    /// <summary>AutoResetEvent: Set lets exactly one waiter through, then the event
    /// is reset again.</summary>
    public sealed unsafe class BzAutoResetEvent
    {
        int _state;
        public BzAutoResetEvent(bool initialState = false) { _state = initialState ? 1 : 0; }

        public void Set()
        {
            fixed (int* p = &_state)
                if (BzSync.bz_atomic_xchg32(p, 1) == 0) BzSync.bz_futex_wake(p, 1);
        }

        public void Reset() { fixed (int* p = &_state) BzSync.bz_atomic_xchg32(p, 0); }

        public bool Wait(int timeoutMs = -1)
        {
            ulong deadline = BzSync.Deadline(timeoutMs);
            fixed (int* p = &_state)
                while (BzSync.bz_atomic_cas32(p, 1, 0) != 1)
                    if (!BzSync.WaitWhile(p, 0, deadline)) return BzSync.bz_atomic_cas32(p, 1, 0) == 1;
            return true;
        }
    }

    /// <summary>SemaphoreSlim: a count; Wait takes one (sleeping while it is 0),
    /// Release adds some.</summary>
    public sealed unsafe class BzSemaphore
    {
        int _count;
        public BzSemaphore(int initialCount) { _count = initialCount; }
        public int CurrentCount => _count;

        public bool Wait(int timeoutMs = -1)
        {
            ulong deadline = BzSync.Deadline(timeoutMs);
            fixed (int* p = &_count)
            {
                while (true)
                {
                    int c = BzSync.Load(p);
                    if (c > 0)
                    {
                        if (BzSync.bz_atomic_cas32(p, c, c - 1) == c) return true;
                        continue;
                    }
                    if (!BzSync.WaitWhile(p, 0, deadline) && BzSync.Load(p) <= 0) return false;
                }
            }
        }

        public int Release(int n = 1)
        {
            fixed (int* p = &_count)
            {
                int before = BzSync.bz_atomic_add32(p, n);
                BzSync.bz_futex_wake(p, (ulong)(uint)n);
                return before;
            }
        }
    }

    /// <summary>CountdownEvent: set once Signal was called as often as the count.</summary>
    public sealed unsafe class BzCountdownEvent
    {
        int _left;
        public BzCountdownEvent(int count) { _left = count; }
        public int CurrentCount => _left;
        public bool IsSet => _left <= 0;

        public void AddCount(int n = 1) { fixed (int* p = &_left) BzSync.bz_atomic_add32(p, n); }

        /// <summary>True when this signal brought the count to zero.</summary>
        public bool Signal(int n = 1)
        {
            fixed (int* p = &_left)
            {
                if (BzSync.bz_atomic_add32(p, -n) - n > 0) return false;
                BzSync.bz_futex_wake(p, 0xffffffffUL);
                return true;
            }
        }

        public bool Wait(int timeoutMs = -1)
        {
            ulong deadline = BzSync.Deadline(timeoutMs);
            fixed (int* p = &_left)
                while (true)
                {
                    int c = BzSync.Load(p);
                    if (c <= 0) return true;
                    if (!BzSync.WaitWhile(p, c, deadline)) return BzSync.Load(p) <= 0;
                }
        }
    }

    /// <summary>The result of <see cref="BzThreadPool.Run{T}"/>.</summary>
    public sealed class BzFuture<T>
    {
        internal readonly BzManualResetEvent Done = new BzManualResetEvent();
        internal T Value;
        internal Exception Error;
        public bool IsCompleted => Done.IsSet;
        public bool IsFaulted => Done.IsSet && Error != null;
        public Exception Exception => Error;
        public bool Wait(int timeoutMs = -1) => Done.Wait(timeoutMs);

        /// <summary>The value; waits for it, rethrows the work item's exception.</summary>
        public T Result
        {
            get
            {
                Done.Wait();
                if (Error != null) throw Error;
                return Value;
            }
        }
    }

    /// <summary>ThreadPool: work items queued in order and run by a fixed set of
    /// worker threads (started with the first item; <see cref="SetWorkers"/> before
    /// that picks how many, default 2). An exception escaping a work item is
    /// counted in <see cref="Faults"/> and does not stop the worker.</summary>
    public static unsafe class BzThreadPool
    {
        sealed class Item
        {
            public Action Work;
            public Item Next;
        }

        static readonly BzMonitor Gate = new BzMonitor();
        static Item _head, _tail;
        static int _workers = 2, _started, _busy, _queued;
        static long _completed, _faults;
        static readonly BzManualResetEvent Idle = new BzManualResetEvent(true);
        static int _idleVersion;

        public static int WorkerCount => _started;
        public static long Completed => _completed;
        public static long Faults => _faults;
        public static int Pending { get { Gate.Enter(); int n = _queued + _busy; Gate.Exit(); return n; } }

        /// <summary>Worker threads to start (1..16); only before the first work item.</summary>
        public static bool SetWorkers(int n)
        {
            if (_started != 0 || n < 1 || n > 16) return false;
            _workers = n;
            return true;
        }

        public static bool QueueUserWorkItem(Action work)
        {
            if (work == null) return false;
            var item = new Item { Work = work };
            Gate.Enter();
            if (_started == 0)
            {
                for (int i = 0; i < _workers; i++)
                {
                    delegate*<ulong, void> entry = &Worker;
                    if (BzSync.bz_thread_create((ulong)entry, (ulong)i) != 0) _started++;
                }
                if (_started == 0) { Gate.Exit(); return false; }
            }
            if (_tail == null) _head = item; else _tail.Next = item;
            _tail = item;
            _queued++;
            Idle.Reset();
            Gate.Pulse();
            Gate.Exit();
            return true;
        }

        public static bool QueueUserWorkItem<T>(Action<T> work, T state) => QueueUserWorkItem(() => work(state));

        /// <summary>Run <paramref name="work"/> on the pool; the future carries its value.</summary>
        public static BzFuture<T> Run<T>(Func<T> work)
        {
            var f = new BzFuture<T>();
            if (!QueueUserWorkItem(() =>
            {
                try { f.Value = work(); }
                catch (Exception e) { f.Error = e; }
                f.Done.Set();
            }))
            {
                f.Error = new InvalidOperationException("thread pool unavailable");
                f.Done.Set();
            }
            return f;
        }

        /// <summary>Wait until no work item is queued or running.</summary>
        public static bool WaitIdle(int timeoutMs = -1) => Idle.Wait(timeoutMs);

        static void Worker(ulong index)
        {
            while (true)
            {
                Gate.Enter();
                while (_head == null) Gate.Wait();
                Item item = _head;
                _head = item.Next;
                if (_head == null) _tail = null;
                _queued--;
                _busy++;
                Gate.Exit();
                try { item.Work(); }
                catch (Exception) { BzInterlocked.Increment(ref _faults); }
                BzInterlocked.Increment(ref _completed);
                Gate.Enter();
                _busy--;
                if (_busy == 0 && _queued == 0) Idle.Set();
                Gate.Exit();
            }
        }
    }

    /// <summary>SynchronizationContext: where a piece of work should run. The base
    /// context posts to the thread pool and sends inline. <see cref="Current"/> is
    /// per thread (set with <see cref="SetSynchronizationContext"/>).</summary>
    public class BzSynchronizationContext
    {
        sealed class Entry
        {
            public long Thread;
            public BzSynchronizationContext Context;
            public Entry Next;
        }

        static readonly BzMonitor TableLock = new BzMonitor();
        static Entry _table;

        public virtual void Post(Action work) => BzThreadPool.QueueUserWorkItem(work);
        public virtual void Send(Action work) => work();

        public static BzSynchronizationContext Current
        {
            get
            {
                long me = BzSync.ThreadId;
                TableLock.Enter();
                BzSynchronizationContext r = null;
                for (var e = _table; e != null; e = e.Next) if (e.Thread == me) { r = e.Context; break; }
                TableLock.Exit();
                return r;
            }
        }

        public static void SetSynchronizationContext(BzSynchronizationContext context)
        {
            long me = BzSync.ThreadId;
            TableLock.Enter();
            Entry prev = null;
            for (var e = _table; e != null; prev = e, e = e.Next)
            {
                if (e.Thread != me) continue;
                if (context != null) e.Context = context;
                else if (prev == null) _table = e.Next;
                else prev.Next = e.Next;
                TableLock.Exit();
                return;
            }
            if (context != null) _table = new Entry { Thread = me, Context = context, Next = _table };
            TableLock.Exit();
        }
    }

    /// <summary>A context with a queue, owned by the thread that runs it (a UI or
    /// event-loop thread): <c>Post</c> from any thread queues work, the owner runs
    /// it in <see cref="Pump"/> / <see cref="RunUntil"/>; <c>Send</c> from another
    /// thread waits until the owner ran it.</summary>
    public sealed class BzLoopContext : BzSynchronizationContext
    {
        sealed class Item
        {
            public Action Work;
            public BzManualResetEvent Sent;
            public Item Next;
        }

        readonly BzMonitor _gate = new BzMonitor();
        Item _head, _tail;
        public long OwnerThread { get; private set; }
        public long Ran { get; private set; }

        public BzLoopContext() { OwnerThread = BzSync.ThreadId; }

        public override void Post(Action work) => Enqueue(work, null);

        public override void Send(Action work)
        {
            if (BzSync.ThreadId == OwnerThread) { work(); return; }
            var done = new BzManualResetEvent();
            Enqueue(work, done);
            done.Wait();
        }

        void Enqueue(Action work, BzManualResetEvent sent)
        {
            var item = new Item { Work = work, Sent = sent };
            _gate.Enter();
            if (_tail == null) _head = item; else _tail.Next = item;
            _tail = item;
            _gate.PulseAll();
            _gate.Exit();
        }

        /// <summary>Run every queued item (owner thread). Returns how many ran.</summary>
        public int Pump()
        {
            int n = 0;
            while (true)
            {
                _gate.Enter();
                Item item = _head;
                if (item != null) { _head = item.Next; if (_head == null) _tail = null; }
                _gate.Exit();
                if (item == null) return n;
                try { item.Work(); }
                finally { item.Sent?.Set(); }
                Ran++;
                n++;
            }
        }

        /// <summary>Pump until <paramref name="done"/> returns true, sleeping while the
        /// queue is empty; false after <paramref name="timeoutMs"/>.</summary>
        public bool RunUntil(Func<bool> done, int timeoutMs = -1)
        {
            ulong deadline = BzSync.Deadline(timeoutMs);
            while (true)
            {
                Pump();
                if (done()) return true;
                if (deadline != 0 && BzSync.bz_clock_nanos() >= deadline) return false;
                _gate.Enter();
                if (_head == null) _gate.Wait(20);
                _gate.Exit();
            }
        }
    }

    /// <summary>The finalizer side of System.GC: finalizers (<c>~T()</c>) run on the
    /// collector's finalizer thread after the object was found unreachable.</summary>
    public static unsafe class BzFinalizer
    {
        [DllImport("*")] static extern ulong bz_gc_suppress_finalize(void* obj);
        [DllImport("*")] static extern ulong bz_gc_reregister_finalize(void* obj);
        [DllImport("*")] static extern ulong bz_gc_wait_finalizers();
        [DllImport("*")] static extern void bz_gc_finalizer_stats(ulong* outp);

#pragma warning disable CS8500 // the address of a reference: the object it points to
        static void* Address(object o) => *(void**)&o;
#pragma warning restore CS8500

        /// <summary>GC.SuppressFinalize: the object's finalizer will not run.</summary>
        public static bool SuppressFinalize(object o) => o != null && bz_gc_suppress_finalize(Address(o)) != 0;

        /// <summary>GC.ReRegisterForFinalize: run its finalizer (again) once it is unreachable.</summary>
        public static bool ReRegisterForFinalize(object o) => o != null && bz_gc_reregister_finalize(Address(o)) != 0;

        /// <summary>GC.WaitForPendingFinalizers: until every queued finalizer ran.
        /// Returns the number of finalizers run so far.</summary>
        public static long WaitForPendingFinalizers() => (long)bz_gc_wait_finalizers();

        static ulong Stat(int i) { ulong* s = stackalloc ulong[3]; bz_gc_finalizer_stats(s); return s[i]; }
        public static long Ran => (long)Stat(0);
        public static long Registered => (long)Stat(1);
        public static long Pending => (long)Stat(2);
    }
}
