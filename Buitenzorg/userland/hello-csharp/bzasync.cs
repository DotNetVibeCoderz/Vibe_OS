// Buitenzorg.Bcl part 4 — async/await for native (bflat --stdlib:zero) C#.
//
// zerolib has no Task, no builders, no awaiters, so `async` did not compile.
// The C# compiler finds those types by name in any source it compiles, so this
// file supplies them: an `async` method is lowered to a state machine that
// calls AsyncTaskMethodBuilder, and `await x` needs x.GetAwaiter() with
// IsCompleted / OnCompleted / GetResult. Continuations run on a single-thread
// **event loop** (`BzAsync.Run`), the JavaScript model: `Task.Delay` puts a
// timer on it, `Task.Yield` re-queues the caller, `Task.WhenAll` /
// `Task.WhenAny` wait for several tasks, and `await` on a task that is not
// done yet parks the method until the task completes.
//
// Needs bzbcl3.cs (Action / Func) and the shim's RhpByRefAssignRef (copying a
// state machine struct that holds references). Two zerolib limits shape it:
// a delegate cannot be bound to an *interface* method, so continuations go
// through the small `MoveNextRunner` class; and there are no exceptions
// (nothing can throw), so a task never faults -- `SetException` only records
// that it was called.
//
// Not here: tasks on other threads (`Task.Run` queues work on the loop, like
// a browser's setTimeout(0)), cancellation tokens, ValueTask, IAsyncEnumerable.

using System.Runtime.InteropServices;
using System.Threading.Tasks;
using Buitenzorg;

namespace System
{
    /// <summary>zerolib has no exception types; the async builders name this one.</summary>
    /// <summary>zerolib lacks it too; `params` needs it.</summary>
    public sealed class ParamArrayAttribute : Attribute { public ParamArrayAttribute() { } }

    public class Exception
    {
        public string Message;
        public Exception() { Message = ""; }
        public Exception(string message) { Message = message; }
    }
}

namespace System.Runtime.CompilerServices
{
    public interface IAsyncStateMachine
    {
        void MoveNext();
        void SetStateMachine(IAsyncStateMachine stateMachine);
    }

    public interface INotifyCompletion { void OnCompleted(Action continuation); }
    public interface ICriticalNotifyCompletion : INotifyCompletion { void UnsafeOnCompleted(Action continuation); }

    public sealed class AsyncStateMachineAttribute : Attribute { public AsyncStateMachineAttribute(Type stateMachineType) { } }
    public sealed class IteratorStateMachineAttribute : Attribute { public IteratorStateMachineAttribute(Type stateMachineType) { } }

    /// <summary>A boxed state machine and its MoveNext as an Action (a delegate
    /// cannot point at an interface method under zerolib).</summary>
    sealed class MoveNextRunner
    {
        public IAsyncStateMachine Machine;
        public void Run() { Machine.MoveNext(); }

        public static Action For<TSM>(ref TSM sm) where TSM : IAsyncStateMachine
        {
            var r = new MoveNextRunner();
            r.Machine = sm; // boxes the current state; later awaits box again
            return r.Run;
        }
    }

    public struct AsyncTaskMethodBuilder
    {
        Task _task;
        public static AsyncTaskMethodBuilder Create() { var b = default(AsyncTaskMethodBuilder); b._task = new Task(); return b; }
        public void Start<TSM>(ref TSM sm) where TSM : IAsyncStateMachine { sm.MoveNext(); }
        public void SetStateMachine(IAsyncStateMachine sm) { }
        public void SetResult() { _task.Finish(); }
        public void SetException(Exception e) { _task.Fault(e); }
        public void AwaitOnCompleted<TA, TSM>(ref TA awaiter, ref TSM sm) where TA : INotifyCompletion where TSM : IAsyncStateMachine
            => awaiter.OnCompleted(MoveNextRunner.For(ref sm));
        public void AwaitUnsafeOnCompleted<TA, TSM>(ref TA awaiter, ref TSM sm) where TA : ICriticalNotifyCompletion where TSM : IAsyncStateMachine
            => awaiter.UnsafeOnCompleted(MoveNextRunner.For(ref sm));
        public Task Task => _task;
    }

    public struct AsyncTaskMethodBuilder<T>
    {
        Task<T> _task;
        public static AsyncTaskMethodBuilder<T> Create() { var b = default(AsyncTaskMethodBuilder<T>); b._task = new Task<T>(); return b; }
        public void Start<TSM>(ref TSM sm) where TSM : IAsyncStateMachine { sm.MoveNext(); }
        public void SetStateMachine(IAsyncStateMachine sm) { }
        public void SetResult(T result) { _task.SetResult(result); }
        public void SetException(Exception e) { _task.Fault(e); }
        public void AwaitOnCompleted<TA, TSM>(ref TA awaiter, ref TSM sm) where TA : INotifyCompletion where TSM : IAsyncStateMachine
            => awaiter.OnCompleted(MoveNextRunner.For(ref sm));
        public void AwaitUnsafeOnCompleted<TA, TSM>(ref TA awaiter, ref TSM sm) where TA : ICriticalNotifyCompletion where TSM : IAsyncStateMachine
            => awaiter.UnsafeOnCompleted(MoveNextRunner.For(ref sm));
        public Task<T> Task => _task;
    }

    /// <summary>`async void` (event handlers): fire and forget.</summary>
    public struct AsyncVoidMethodBuilder
    {
        public static AsyncVoidMethodBuilder Create() => default(AsyncVoidMethodBuilder);
        public void Start<TSM>(ref TSM sm) where TSM : IAsyncStateMachine { sm.MoveNext(); }
        public void SetStateMachine(IAsyncStateMachine sm) { }
        public void SetResult() { }
        public void SetException(Exception e) { }
        public void AwaitOnCompleted<TA, TSM>(ref TA awaiter, ref TSM sm) where TA : INotifyCompletion where TSM : IAsyncStateMachine
            => awaiter.OnCompleted(MoveNextRunner.For(ref sm));
        public void AwaitUnsafeOnCompleted<TA, TSM>(ref TA awaiter, ref TSM sm) where TA : ICriticalNotifyCompletion where TSM : IAsyncStateMachine
            => awaiter.UnsafeOnCompleted(MoveNextRunner.For(ref sm));
    }
}

namespace System.Threading.Tasks
{
    using System.Runtime.CompilerServices;

    /// <summary>A continuation waiting for a task (a linked list).</summary>
    sealed class Continuation
    {
        public Action Run;
        public Continuation Next;
    }

    public class Task
    {
        Continuation _first;
        bool _done;
        Exception _error;

        public bool IsCompleted => _done;
        public bool IsFaulted => _error != null;
        public Exception Exception => _error;

        internal void Finish()
        {
            if (_done) return;
            _done = true;
            // continuations run on the loop, never inside the completing call
            for (var c = _first; c != null; c = c.Next) BzAsync.Post(c.Run);
            _first = null;
        }

        internal void Fault(Exception e) { _error = e; Finish(); }

        internal void OnDone(Action a)
        {
            if (_done) { BzAsync.Post(a); return; }
            var c = new Continuation();
            c.Run = a;
            // keep the order continuations were registered in
            if (_first == null) _first = c;
            else { var t = _first; while (t.Next != null) t = t.Next; t.Next = c; }
        }

        public TaskAwaiter GetAwaiter() => new TaskAwaiter(this);

        /// <summary>Block (pumping the event loop) until the task is done.</summary>
        public void Wait() { BzAsync.RunUntil(this); }

        static Task _completed;
        public static Task CompletedTask
        {
            get
            {
                if (_completed == null) { _completed = new Task(); _completed._done = true; }
                return _completed;
            }
        }

        public static Task<T> FromResult<T>(T value)
        {
            var t = new Task<T>();
            t.SetResult(value);
            return t;
        }

        /// <summary>A task that completes after `milliseconds` (a timer on the loop).</summary>
        public static Task Delay(int milliseconds)
        {
            var t = new Task();
            BzAsync.AddTimer(milliseconds, t);
            return t;
        }

        /// <summary>`await Task.Yield()`: let the other queued work run first.</summary>
        public static YieldAwaitable Yield() => default(YieldAwaitable);

        /// <summary>Run `work` on the loop (queued, like setTimeout(0)).</summary>
        public static Task Run(Action work)
        {
            var job = new RunJob();
            job.Work = work;
            job.Done = new Task();
            BzAsync.Post(job.Execute);
            return job.Done;
        }

        public static Task<T> Run<T>(Func<T> work)
        {
            var job = new RunJob<T>();
            job.Work = work;
            job.Done = new Task<T>();
            BzAsync.Post(job.Execute);
            return job.Done;
        }

        /// <summary>Done when every task is done.</summary>
        public static Task WhenAll(params Task[] tasks)
        {
            var all = new Task();
            var counter = new WhenAllCounter();
            counter.Left = tasks.Length;
            counter.All = all;
            if (tasks.Length == 0) all.Finish();
            for (int i = 0; i < tasks.Length; i++) tasks[i].OnDone(counter.One);
            return all;
        }

        /// <summary>Done when the first task is done; its result is that task.</summary>
        public static Task<Task> WhenAny(params Task[] tasks)
        {
            var any = new Task<Task>();
            for (int i = 0; i < tasks.Length; i++)
            {
                var w = new WhenAnyWatcher();
                w.Any = any;
                w.Task = tasks[i];
                tasks[i].OnDone(w.Fire);
            }
            return any;
        }
    }

    public class Task<T> : Task
    {
        T _result;
        public T Result
        {
            get
            {
                if (!IsCompleted) Wait();
                return _result;
            }
        }
        internal void SetResult(T value) { _result = value; Finish(); }
        public new TaskAwaiter<T> GetAwaiter() => new TaskAwaiter<T>(this);
    }

    sealed class RunJob
    {
        public Action Work;
        public Task Done;
        public void Execute() { Work(); Done.Finish(); }
    }

    sealed class RunJob<T>
    {
        public Func<T> Work;
        public Task<T> Done;
        public void Execute() { Done.SetResult(Work()); }
    }

    sealed class WhenAllCounter
    {
        public int Left;
        public Task All;
        public void One() { if (--Left == 0) All.Finish(); }
    }

    sealed class WhenAnyWatcher
    {
        public Task<Task> Any;
        public Task Task;
        public void Fire() { if (!Any.IsCompleted) Any.SetResult(Task); }
    }

    public struct TaskAwaiter : ICriticalNotifyCompletion
    {
        readonly Task _task;
        public TaskAwaiter(Task task) { _task = task; }
        public bool IsCompleted => _task.IsCompleted;
        public void GetResult() { }
        public void OnCompleted(Action continuation) => _task.OnDone(continuation);
        public void UnsafeOnCompleted(Action continuation) => _task.OnDone(continuation);
    }

    public struct TaskAwaiter<T> : ICriticalNotifyCompletion
    {
        readonly Task<T> _task;
        public TaskAwaiter(Task<T> task) { _task = task; }
        public bool IsCompleted => _task.IsCompleted;
        public T GetResult() => _task.Result;
        public void OnCompleted(Action continuation) => _task.OnDone(continuation);
        public void UnsafeOnCompleted(Action continuation) => _task.OnDone(continuation);
    }

    public struct YieldAwaitable
    {
        public YieldAwaiter GetAwaiter() => default(YieldAwaiter);
    }

    public struct YieldAwaiter : ICriticalNotifyCompletion
    {
        public bool IsCompleted => false;
        public void GetResult() { }
        public void OnCompleted(Action continuation) => BzAsync.Post(continuation);
        public void UnsafeOnCompleted(Action continuation) => BzAsync.Post(continuation);
    }
}

namespace Buitenzorg
{
    /// <summary>
    /// The event loop async code runs on: a queue of ready continuations and a
    /// list of timers. `BzAsync.Run(main)` runs `main` and pumps until its task
    /// is done; `task.Wait()` / `.Result` pump the same loop.
    /// </summary>
    public static class BzAsync
    {
        [DllImport("*")] static extern ulong bz_clock_nanos();
        [DllImport("*")] static extern void bz_yield();

        sealed class Item { public System.Action Run; public Item Next; }
        sealed class Timer { public ulong Due; public Task Task; public Timer Next; }

        static Item _head, _tail;
        static Timer _timers;
        static int _posted, _ran;

        /// <summary>Continuations run so far (diagnostics).</summary>
        public static int Ran => _ran;
        public static int Posted => _posted;

        internal static void Post(System.Action a)
        {
            var it = new Item();
            it.Run = a;
            if (_tail == null) { _head = _tail = it; } else { _tail.Next = it; _tail = it; }
            _posted++;
        }

        internal static void AddTimer(int ms, Task t)
        {
            var tm = new Timer();
            tm.Due = bz_clock_nanos() + (ulong)(ms < 0 ? 0 : ms) * 1_000_000UL;
            tm.Task = t;
            // sorted by due time
            if (_timers == null || tm.Due < _timers.Due) { tm.Next = _timers; _timers = tm; return; }
            var p = _timers;
            while (p.Next != null && p.Next.Due <= tm.Due) p = p.Next;
            tm.Next = p.Next;
            p.Next = tm;
        }

        /// <summary>Run ready work and due timers once; false = nothing at all is pending.</summary>
        public static bool Step()
        {
            ulong now = bz_clock_nanos();
            while (_timers != null && _timers.Due <= now)
            {
                var t = _timers;
                _timers = t.Next;
                t.Task.Finish();
            }
            if (_head != null)
            {
                var it = _head;
                _head = it.Next;
                if (_head == null) _tail = null;
                _ran++;
                it.Run();
                return true;
            }
            if (_timers != null) { bz_yield(); return true; }
            return false;
        }

        /// <summary>Pump until `task` is done (or nothing is left that could finish it).</summary>
        public static bool RunUntil(Task task)
        {
            while (!task.IsCompleted)
                if (!Step()) return false;
            return true;
        }

        /// <summary>The entry point of an async program: run `main`, pump the loop until it is done.</summary>
        public static bool Run(System.Func<Task> main)
        {
            var t = main();
            return RunUntil(t);
        }
    }
}
