//! Kernel tasks: preemptive round-robin scheduler + context switch
//! (v0.2 "Akar" milestone: two tasks running alternately).
//!
//! Design notes:
//! - Single core. Preemption happens in the timer IRQ (after EOI) via
//!   [`preempt`]; voluntary switches via [`yield_now`].
//! - The scheduler lock is only ever taken with interrupts disabled, so the
//!   timer handler can never observe it held.
//! - Each task owns a heap-allocated kernel stack. Task 0 is the boot flow
//!   (`kernel_main`) on the bootloader-provided stack; it never exits.
//! - Finished tasks are never rescheduled; their stacks are freed lazily the
//!   next time [`spawn`] runs (single-core, so nothing references them then).

use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::{gdt, usermode};

const STACK_SIZE: usize = 32 * 1024;
/// Per-thread SYSCALL kernel stack for cooperative ring-3 user threads. Kept
/// separate from the TSS interrupt stack so a timer taken during one thread's
/// ring-3 never lands on another thread's suspended syscall frame.
const SYS_STACK_SIZE: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Runnable,
    /// Blocked on a futex address ([`Task::wait_key`]); skipped by the scheduler
    /// until [`futex_wake`] makes it runnable again.
    Blocked,
    Finished,
}

/// Ring-3 user-thread context (v0.15 "Matang" increment 2). Present only for
/// tasks that back a user thread; `None` for kernel tasks and the boot task
/// while it runs a single-threaded app (so those paths are unchanged).
struct UserThread {
    /// This thread's private SYSCALL kernel stack top (loaded into the global
    /// on switch-in).
    sys_kstack_top: u64,
    /// Saved longjmp context + user-rsp scratch while switched out.
    kctx: u64,
    user_rsp: u64,
    /// Ring-3 entry to run on first schedule: (rip, arg, user_stack_top).
    params: Option<(u64, u64, u64)>,
    /// Owned SYSCALL kernel stack.
    _sys_stack: Option<Box<[u8]>>,
}

struct Task {
    id: u64,
    name: &'static str,
    state: State,
    /// Saved stack pointer while the task is switched out.
    rsp: u64,
    entry: fn(),
    /// Owned kernel stack; `None` for the boot task.
    stack: Option<Box<[u8]>>,
    /// Accumulated CPU time in timer ticks.
    cpu_ticks: u64,
    /// Ring-3 user-thread context, if this task backs a user thread.
    user: Option<UserThread>,
    /// Futex address this task is blocked on (valid only in `Blocked` state).
    wait_key: u64,
}

struct Scheduler {
    tasks: Vec<Task>,
    current: usize,
    next_id: u64,
}

/// Snapshot of a task for the process listing (v0.9 "Serbuk").
#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub id: u64,
    pub name: &'static str,
    pub running: bool,
    pub finished: bool,
    pub cpu_ticks: u64,
}

/// Charge one timer tick to the currently running task (called from the timer
/// IRQ). Cheap; skips if the scheduler lock is momentarily held.
pub fn account_tick() {
    if let Some(mut guard) = SCHEDULER.try_lock() {
        if let Some(sched) = guard.as_mut() {
            let cur = sched.current;
            sched.tasks[cur].cpu_ticks += 1;
        }
    }
}

/// List all tasks (kernel-side). The user app is added separately by `process`.
pub fn list() -> Vec<TaskInfo> {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        match guard.as_ref() {
            Some(sched) => sched
                .tasks
                .iter()
                .map(|t| TaskInfo {
                    id: t.id,
                    name: t.name,
                    running: t.state == State::Runnable && t.id == sched.tasks[sched.current].id,
                    finished: t.state == State::Finished,
                    cpu_ticks: t.cpu_ticks,
                })
                .collect(),
            None => Vec::new(),
        }
    })
}

/// Mark a task finished by id (for PROC_KILL). The boot task (id 0) is
/// protected. Returns true if a live task was terminated.
pub fn kill(id: u64) -> bool {
    if id == 0 {
        return false; // never kill the boot task
    }
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            if let Some(t) = sched.tasks.iter_mut().find(|t| t.id == id) {
                if t.state == State::Runnable {
                    t.state = State::Finished;
                    return true;
                }
            }
        }
        false
    })
}

impl Scheduler {
    /// Pick the next runnable task after `current` (round-robin).
    fn pick_next(&self) -> usize {
        let n = self.tasks.len();
        for off in 1..=n {
            let idx = (self.current + off) % n;
            if self.tasks[idx].state == State::Runnable {
                return idx;
            }
        }
        // Fall through returns current below.
        self.current
    }
}

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);
/// Timer-driven preemption. Only the v0.2 scheduler demo needs it; the rest of
/// the kernel runs cooperatively (yield_now), which avoids switching tasks in
/// the middle of the boot task's heavy heap/framebuffer work.
static PREEMPT_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable timer-driven preemption. Cooperative `yield_now` always
/// works regardless.
pub fn set_preemption(enabled: bool) {
    PREEMPT_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Register the currently running boot flow as task 0 and enable preemption.
pub fn init() {
    interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        *sched = Some(Scheduler {
            tasks: alloc::vec![Task {
                id: 0,
                name: "kernel",
                state: State::Runnable,
                rsp: 0, // filled in on the first switch away
                entry: || {},
                stack: None,
                cpu_ticks: 0,
                user: None,
                wait_key: 0,
            }],
            current: 0,
            next_id: 1,
        });
    });
    STARTED.store(true, Ordering::SeqCst);
}

/// Spawn a new kernel task that starts at `entry` and exits when it returns.
pub fn spawn(entry: fn()) {
    spawn_named("task", entry);
}

/// Spawn a named kernel task (name appears in the process listing).
pub fn spawn_named(name: &'static str, entry: fn()) {
    let stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        // Lazily reap stacks of finished tasks (safe: they can't be running).
        for t in sched.tasks.iter_mut() {
            if t.state == State::Finished {
                t.stack = None;
                t.user = None;
            }
        }
        let rsp = prepare_stack(&stack);
        let id = sched.next_id;
        sched.next_id += 1;
        sched.tasks.push(Task {
            id,
            name,
            state: State::Runnable,
            rsp,
            entry,
            stack: Some(stack),
            cpu_ticks: 0,
            user: None,
            wait_key: 0,
        });
    });
}

// --- Cooperative ring-3 user threads (v0.15 "Matang" increment 2) -------------

/// Body of a user-thread kernel task: read the ring-3 entry params this thread
/// was created with, then run in ring 3 until it exits (THREAD_EXIT / EXIT).
fn user_thread_body() {
    let params = interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        let cur = sched.current;
        sched.tasks[cur].user.as_mut().and_then(|u| u.params.take())
    });
    let Some((rip, arg, user_stack_top)) = params else {
        return;
    };
    // The switch-in hook already loaded our private SYSCALL stack; enter ring 3
    // at rip(arg) on our user stack. Returns after the thread exits.
    let _code = usermode::enter_user_thread(rip, user_stack_top, arg);
}

/// Promote the current task (the app's main thread) to a user thread with its
/// own private SYSCALL kernel stack, snapshotting the live context. Idempotent.
/// Called by THREAD_CREATE before spawning the first worker so the main thread
/// stops sharing the TSS interrupt stack for its syscalls.
pub fn ensure_main_user_thread() {
    let stack = alloc::vec![0u8; SYS_STACK_SIZE].into_boxed_slice();
    let top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        let cur = sched.current;
        if sched.tasks[cur].user.is_none() {
            sched.tasks[cur].user = Some(UserThread {
                sys_kstack_top: top,
                kctx: usermode::get_kctx(),
                user_rsp: usermode::get_user_rsp(),
                params: None,
                _sys_stack: Some(stack),
            });
            // Effective on the main thread's NEXT syscall (this one is still on
            // the TSS stack and returns normally).
            usermode::set_syscall_kstack(top);
        }
    });
}

/// Spawn a cooperative ring-3 user thread that enters `rip(arg)` on
/// `user_stack_top`. Returns its thread id.
pub fn spawn_user_thread(rip: u64, arg: u64, user_stack_top: u64) -> u64 {
    let kstack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let sys_stack = alloc::vec![0u8; SYS_STACK_SIZE].into_boxed_slice();
    let sys_top = (sys_stack.as_ptr() as u64 + sys_stack.len() as u64) & !0xF;
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        for t in sched.tasks.iter_mut() {
            if t.state == State::Finished {
                t.stack = None;
                t.user = None;
            }
        }
        let rsp = prepare_stack(&kstack);
        let id = sched.next_id;
        sched.next_id += 1;
        sched.tasks.push(Task {
            id,
            name: "uthread",
            state: State::Runnable,
            rsp,
            entry: user_thread_body,
            stack: Some(kstack),
            cpu_ticks: 0,
            user: Some(UserThread {
                sys_kstack_top: sys_top,
                kctx: 0,
                user_rsp: 0,
                params: Some((rip, arg, user_stack_top)),
                _sys_stack: Some(sys_stack),
            }),
            wait_key: 0,
        });
        id
    })
}

/// Current thread's id (backs THREAD_SELF; a TLS/pthread_self foundation).
pub fn current_id() -> u64 {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        guard.as_ref().map(|s| s.tasks[s.current].id).unwrap_or(0)
    })
}

/// Futex wait: block the current thread on `key` (if another task is runnable
/// to switch to), then yield. Returns after being woken or after a plain yield.
pub fn futex_wait_block(key: u64) {
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        if let Some(s) = guard.as_mut() {
            let cur = s.current;
            let has_other = s
                .tasks
                .iter()
                .enumerate()
                .any(|(i, t)| i != cur && t.state == State::Runnable);
            if has_other {
                s.tasks[cur].state = State::Blocked;
                s.tasks[cur].wait_key = key;
            }
        }
    });
    yield_now();
}

/// Futex wake: make up to `count` threads blocked on `key` runnable. Returns
/// the number woken.
pub fn futex_wake(key: u64, count: u64) -> u64 {
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let Some(s) = guard.as_mut() else { return 0 };
        let mut n = 0u64;
        for t in s.tasks.iter_mut() {
            if n >= count {
                break;
            }
            if t.state == State::Blocked && t.wait_key == key {
                t.state = State::Runnable;
                t.wait_key = 0;
                n += 1;
            }
        }
        n
    })
}

/// True if the thread with `id` has finished (or no longer exists).
pub fn is_finished(id: u64) -> bool {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|s| s.tasks.iter().find(|t| t.id == id))
            .map(|t| t.state == State::Finished)
            .unwrap_or(true)
    })
}

/// Tear down any lingering user threads and restore the main thread to using
/// the TSS interrupt stack for syscalls. Called when an app finishes so the
/// next (possibly single-threaded) app starts from the clean default.
pub fn terminate_user_threads() {
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            let cur = sched.current;
            for (i, t) in sched.tasks.iter_mut().enumerate() {
                if i != cur && t.user.is_some() {
                    t.state = State::Finished;
                }
            }
            sched.tasks[cur].user = None;
        }
    });
    usermode::set_syscall_kstack(gdt::privilege_stack_top().as_u64());
}

/// Build the initial stack frame consumed by [`context_switch`]'s restore
/// path: saved rflags + 6 callee-saved registers, then the trampoline as
/// return address, then a 0 sentinel so `rsp % 16 == 8` at trampoline entry
/// (SysV ABI) and backtraces terminate.
fn prepare_stack(stack: &[u8]) -> u64 {
    let top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
    unsafe {
        let mut sp = top as *mut u64;
        let mut push = |val: u64| {
            sp = sp.sub(1);
            sp.write(val);
        };
        push(0); // sentinel return address
        push(task_trampoline as *const () as u64); // `ret` target
        push(0); // rbp
        push(0); // rbx
        push(0); // r12
        push(0); // r13
        push(0); // r14
        push(0); // r15
        push(0x202); // rflags: IF=1 so fresh tasks run with interrupts enabled
        sp as u64
    }
}

extern "C" fn task_trampoline() -> ! {
    let entry = interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().expect("scheduler not initialized");
        sched.tasks[sched.current].entry
    });
    entry();
    exit_current()
}

/// Mark the current task finished and switch away for good.
pub fn exit_current() -> ! {
    interrupts::disable();
    switch_with(|sched| {
        let cur = sched.current;
        sched.tasks[cur].state = State::Finished;
    });
    unreachable!("finished task was rescheduled");
}

/// Voluntarily give up the CPU (also backs the YIELD syscall).
pub fn yield_now() {
    if !STARTED.load(Ordering::SeqCst) {
        return;
    }
    interrupts::without_interrupts(|| switch_with(|_| ()));
}

/// Called from the timer interrupt (interrupts disabled, EOI already sent).
pub fn preempt() {
    if !STARTED.load(Ordering::SeqCst) || !PREEMPT_ENABLED.load(Ordering::SeqCst) {
        return;
    }
    switch_with(|_| ());
}

/// Run `before` on the scheduler, then context-switch to the next runnable
/// task if it differs from the current one. Interrupts must be disabled.
fn switch_with(before: impl FnOnce(&mut Scheduler)) {
    let (prev_rsp_ptr, next_rsp) = {
        let mut guard = SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else { return };
        before(sched);
        let next = sched.pick_next();
        if next == sched.current {
            return; // nothing else runnable (or still just the boot task)
        }
        let cur = sched.current;
        // Save the outgoing user thread's live longjmp/user-rsp context. (No-op
        // for kernel tasks, so single-threaded/kernel paths are unaffected.)
        if let Some(u) = sched.tasks[cur].user.as_mut() {
            u.kctx = usermode::get_kctx();
            u.user_rsp = usermode::get_user_rsp();
        }
        let prev: *mut u64 = &mut sched.tasks[cur].rsp;
        sched.current = next;
        // Load the incoming user thread's private SYSCALL stack + context, so
        // two ring-3 threads never share a syscall kernel stack.
        if let Some(u) = sched.tasks[next].user.as_ref() {
            usermode::set_syscall_kstack(u.sys_kstack_top);
            usermode::set_kctx(u.kctx);
            usermode::set_user_rsp(u.user_rsp);
        }
        (prev, sched.tasks[next].rsp)
        // Lock is dropped here; no allocation happens before the switch, so
        // `prev` stays valid (single core, interrupts off).
    };
    unsafe { context_switch(prev_rsp_ptr, next_rsp) };
}

/// Save callee-saved state + rflags on the current stack, store rsp through
/// `prev_rsp`, load `next_rsp`, restore, and return on the new stack.
#[unsafe(naked)]
unsafe extern "C" fn context_switch(prev_rsp: *mut u64, next_rsp: u64) {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "pushfq",
        "mov [rdi], rsp",
        "mov rsp, rsi",
        "popfq",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    )
}
