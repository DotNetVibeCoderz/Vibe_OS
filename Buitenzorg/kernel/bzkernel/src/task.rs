//! Kernel tasks: preemptive round-robin scheduler + context switch
//! (v0.2 "Akar" milestone: two tasks running alternately), extended to
//! preemptive multi-process ring 3 (multi-process increment 1).
//!
//! Design notes:
//! - Kernel code runs on the bootstrap CPU only. Preemption happens in the
//!   timer IRQ (after EOI) via [`preempt`]; voluntary switches via
//!   [`yield_now`]. Ring-3 code may also run on the application processors
//!   (see "SMP" below).
//! - **Only ring-3 code is preempted** (plus kernel code during the v0.2
//!   scheduler demo). Kernel code stays cooperative: preempting the boot task
//!   mid heap/framebuffer work corrupted memory in the past, while a timer that
//!   interrupts *user* code holds no kernel lock by construction.
//! - The per-CPU state the SYSCALL path uses (kernel stack, longjmp context,
//!   user-rsp scratch, user-active flag), the TSS ring-0 stack, the SSE/x87
//!   registers (FXSAVE) and the installed user address space are all saved and
//!   restored per task on every switch, so each process/thread runs on its own
//!   kernel stack in its own address space.
//! - The scheduler lock is only ever taken with interrupts disabled, so the
//!   timer handler can never observe it held.
//! - Each task owns a heap-allocated kernel stack. Task 0 is the boot flow
//!   (`kernel_main`) on the bootloader-provided stack; it never exits.
//! - Finished tasks are never rescheduled; their stacks are freed lazily the
//!   next time [`spawn`] runs (single-core, so nothing references them then).

use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;
use crate::arch::context::{self, FpuState};
use crate::arch::cpu as interrupts;

use crate::{aspace, gdt, usermode};

const STACK_SIZE: usize = 32 * 1024;
/// Per-thread SYSCALL + ring-0 interrupt stack for ring-3 processes/threads.
/// Sized like the boot task's privilege stack (gdt.rs): a WIN_PRESENT syscall
/// composites every window on it, which needs real headroom.
const SYS_STACK_SIZE: usize = 64 * 1024;

/// Per-task copy of the SYSCALL-path globals in `usermode.rs`.
#[derive(Clone, Copy)]
struct CpuCtx {
    /// SYSCALL kernel stack top; also loaded into TSS rsp0.
    sys_kstack_top: u64,
    kctx: u64,
    user_rsp: u64,
    user_active: bool,
}

impl CpuCtx {
    fn kernel() -> CpuCtx {
        CpuCtx {
            sys_kstack_top: gdt::privilege_stack_top().as_u64(),
            kctx: 0,
            user_rsp: 0,
            user_active: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Runnable,
    /// Blocked on a futex address ([`Task::wait_key`]); skipped by the scheduler
    /// until [`futex_wake`] makes it runnable again.
    Blocked,
    Finished,
}

/// Ring-3 thread record (v0.15 "Matang" increment 2). Present for tasks that
/// back a user thread or a spawned process's main thread; `None` for kernel
/// tasks and for the boot task while it runs a single-threaded app.
struct UserThread {
    /// Ring-3 entry to run on first schedule: (rip, arg, user_stack_top).
    params: Option<(u64, u64, u64)>,
    /// Owned SYSCALL kernel stack (its top is in the task's `ctx`).
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
    /// Saved SYSCALL-path state while switched out.
    ctx: CpuCtx,
    /// Saved x87/SSE registers while switched out.
    fpu: Box<FpuState>,
    /// User address space this task runs in (`aspace::kernel()` for kernel
    /// tasks); its id is the owning process id.
    space: aspace::Space,
    /// Queued for, or running on, an application processor: the BSP must not
    /// schedule it, reap its stacks, or destroy its space.
    away: bool,
}

struct Scheduler {
    /// Boxed so a task's address never changes when the vector grows: another
    /// CPU may be saving into `rsp`/`fpu` of a task while the BSP spawns one.
    tasks: Vec<Box<Task>>,
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
    /// Owning process (address-space id); 0 for kernel tasks.
    pub space_id: u64,
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
                    space_id: t.space.id,
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

/// Address spaces with a thread away on an AP (fixed capacity: at most one
/// running per AP plus the AP queue).
struct BusySpaces {
    ids: [u64; 96],
    n: usize,
}

impl BusySpaces {
    fn contains(&self, id: u64) -> bool {
        id != 0 && self.ids[..self.n].contains(&id)
    }
}

impl Scheduler {
    /// Processes that currently have a thread on an application processor.
    /// Threads of one process never run at the same time (the ring-3 runtime
    /// - e.g. the bump-heap malloc in bzstart.rs - is not SMP-safe, and the
    /// thread ABI is documented as cooperative); only separate processes run
    /// in parallel.
    fn busy_spaces(&self) -> BusySpaces {
        let mut b = BusySpaces { ids: [0; 96], n: 0 };
        for t in self.tasks.iter() {
            if t.away && t.space.id != 0 && !b.contains(t.space.id) && b.n < b.ids.len() {
                b.ids[b.n] = t.space.id;
                b.n += 1;
            }
        }
        b
    }

    /// Pick the next runnable task after `current` (round-robin).
    fn pick_next(&self) -> usize {
        let n = self.tasks.len();
        let busy = self.busy_spaces();
        for off in 1..=n {
            let idx = (self.current + off) % n;
            let t = &self.tasks[idx];
            if t.state == State::Runnable && !t.away && !busy.contains(t.space.id) {
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
            tasks: alloc::vec![Box::new(Task {
                id: 0,
                name: "kernel",
                state: State::Runnable,
                rsp: 0, // filled in on the first switch away
                entry: || {},
                stack: None,
                cpu_ticks: 0,
                user: None,
                wait_key: 0,
                ctx: CpuCtx::kernel(),
                fpu: FpuState::clean(),
                space: aspace::kernel(),
                away: false,
            })],
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
    let fpu = FpuState::clean();
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        // Lazily reap stacks of finished tasks (safe: they can't be running).
        for t in sched.tasks.iter_mut() {
            if t.state == State::Finished && !t.away {
                t.stack = None;
                t.user = None;
            }
        }
        let rsp = context::prepare_stack(&stack, task_trampoline);
        let id = sched.next_id;
        sched.next_id += 1;
        sched.tasks.push(Box::new(Task {
            id,
            name,
            state: State::Runnable,
            rsp,
            entry,
            stack: Some(stack),
            cpu_ticks: 0,
            user: None,
            wait_key: 0,
            ctx: CpuCtx::kernel(),
            fpu,
            space: aspace::kernel(),
            away: false,
        }));
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
                params: None,
                _sys_stack: Some(stack),
            });
            // Effective on the main thread's NEXT syscall / ring-3 interrupt
            // (this syscall is still on the privilege stack and returns normally).
            sched.tasks[cur].ctx.sys_kstack_top = top;
            usermode::set_syscall_kstack(top);
            gdt::set_rsp0(top);
        }
    });
}

/// Spawn a cooperative ring-3 user thread that enters `rip(arg)` on
/// `user_stack_top`. Returns its thread id.
pub fn spawn_user_thread(rip: u64, arg: u64, user_stack_top: u64) -> u64 {
    // Threads run in the creating task's address space (same process).
    let space = interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        let s = guard.as_ref().expect("scheduler not initialized");
        s.tasks[s.current].space
    });
    spawn_ring3("uthread", user_thread_body, (rip, arg, user_stack_top), space)
}

/// Spawn a task that enters ring 3 at `params` = (rip, arg, user_stack_top) in
/// `space`, with its own kernel task stack and SYSCALL/interrupt stack.
fn spawn_ring3(
    name: &'static str,
    body: fn(),
    params: (u64, u64, u64),
    space: aspace::Space,
) -> u64 {
    let kstack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let sys_stack = alloc::vec![0u8; SYS_STACK_SIZE].into_boxed_slice();
    let sys_top = (sys_stack.as_ptr() as u64 + sys_stack.len() as u64) & !0xF;
    let fpu = FpuState::clean();
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        for t in sched.tasks.iter_mut() {
            if t.state == State::Finished && !t.away {
                t.stack = None;
                t.user = None;
            }
        }
        let rsp = context::prepare_stack(&kstack, task_trampoline);
        let id = sched.next_id;
        sched.next_id += 1;
        sched.tasks.push(Box::new(Task {
            id,
            name,
            state: State::Runnable,
            rsp,
            entry: body,
            stack: Some(kstack),
            cpu_ticks: 0,
            user: Some(UserThread {
                params: Some(params),
                _sys_stack: Some(sys_stack),
            }),
            wait_key: 0,
            ctx: CpuCtx {
                sys_kstack_top: sys_top,
                kctx: 0,
                user_rsp: 0,
                user_active: false,
            },
            fpu,
            space,
            away: false,
        }));
        id
    })
}

// --- Ring-3 processes (multi-process increment 1) ------------------------------

/// Body of a process's main task: enter ring 3 at the program entry, and when
/// the program exits (EXIT syscall, or a fault that killed it) hand the exit
/// code to `app::process_exited` for teardown.
fn process_body() {
    let (params, space_id) = interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        let cur = sched.current;
        let t = &mut sched.tasks[cur];
        (t.user.as_mut().and_then(|u| u.params.take()), t.space.id)
    });
    let Some((rip, arg, user_stack_top)) = params else {
        return;
    };
    let code = usermode::enter_user_thread(rip, user_stack_top, arg);
    usermode::set_user_active(false);
    crate::app::process_exited(space_id, code);
}

/// Start a ring-3 process: a new task whose main thread enters `entry` on
/// `user_stack_top` inside `space` (already loaded by the caller). Returns the
/// task id of its main thread.
pub fn spawn_process(entry: u64, user_stack_top: u64, space: aspace::Space) -> u64 {
    spawn_ring3("app", process_body, (entry, 0, user_stack_top), space)
}

/// Move the current task back to the kernel address space (so its own space
/// can be destroyed) and finish every other task of `space_id`.
pub fn detach_current_from_space(space_id: u64) {
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            let cur = sched.current;
            for (i, t) in sched.tasks.iter_mut().enumerate() {
                if i != cur && t.space.id == space_id && t.state != State::Finished {
                    t.state = State::Finished;
                }
            }
            sched.tasks[cur].space = aspace::kernel();
        }
        aspace::install(aspace::kernel());
    });
}

/// Finish every task of process `space_id` (PROC_KILL). Refuses to kill the
/// caller's own process (it should EXIT). Returns the victim's space if any
/// task was live (for the caller to destroy).
pub fn kill_space(space_id: u64) -> Option<aspace::Space> {
    if space_id == 0 {
        return None;
    }
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut()?;
        if sched.tasks[sched.current].space.id == space_id {
            return None;
        }
        let mut victim = None;
        for t in sched.tasks.iter_mut() {
            if t.space.id == space_id && t.state != State::Finished {
                t.state = State::Finished;
                victim = Some(t.space);
            }
        }
        victim
    })
}

/// True if a task other than the current one belonging to a user process is
/// runnable — the desktop loop yields to it instead of halting.
pub fn user_work_pending() -> bool {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        let Some(s) = guard.as_ref() else { return false };
        s.tasks
            .iter()
            .enumerate()
            .any(|(i, t)| i != s.current && t.state == State::Runnable && !t.away && t.space.id != 0)
    })
}

/// Process (address-space id) of the running task, for the timer IRQ's CPU
/// accounting; `None` if the scheduler lock is momentarily held.
pub fn current_space_id_irq() -> Option<u64> {
    let guard = SCHEDULER.try_lock()?;
    guard.as_ref().map(|s| s.tasks[s.current].space.id)
}

/// Address space of the current task.
pub fn current_space() -> aspace::Space {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        guard
            .as_ref()
            .map(|s| s.tasks[s.current].space)
            .unwrap_or_else(aspace::kernel)
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
                .any(|(i, t)| i != cur && t.state == State::Runnable && !t.away);
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

extern "C" fn task_trampoline() -> ! {
    finish_switch();
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
    interrupts::disable_interrupts();
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
/// Ring-3 code is always preemptible; kernel code only while the v0.2 demo
/// has kernel preemption switched on.
pub fn preempt(from_user: bool) {
    if !STARTED.load(Ordering::SeqCst) {
        return;
    }
    if from_user && maybe_offload() {
        USER_PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if from_user || PREEMPT_ENABLED.load(Ordering::SeqCst) {
        if switch_with(|_| ()) && from_user {
            USER_PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Number of times the timer took the CPU away from ring-3 code and gave it to
/// another task — direct evidence of preemptive multitasking (the multiproc
/// self-test requires it to grow; cooperative yields do not count).
static USER_PREEMPTIONS: AtomicU64 = AtomicU64::new(0);

pub fn user_preemptions() -> u64 {
    USER_PREEMPTIONS.load(Ordering::Relaxed)
}

/// Run `before` on the scheduler, then context-switch to the next runnable
/// task if it differs from the current one. Interrupts must be disabled.
/// Returns true if a switch happened (once this task has been resumed).
fn switch_with(before: impl FnOnce(&mut Scheduler)) -> bool {
    switch_with_offload(false, before)
}

/// [`switch_with`], optionally handing the outgoing task to the AP queue
/// (`offload`): it is marked away and queued only once its context is saved.
fn switch_with_offload(offload: bool, before: impl FnOnce(&mut Scheduler)) -> bool {
    let (prev_rsp_ptr, next_rsp, prev_fx, next_fx) = {
        let mut guard = SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else { return false };
        before(sched);
        let cur = sched.current;
        if offload {
            let t = &sched.tasks[cur];
            if cur == 0 || t.user.is_none() || t.space.id == 0 || t.state != State::Runnable {
                return false; // only ring-3 tasks run on the APs
            }
            if sched.busy_spaces().contains(t.space.id) {
                return false; // a sibling thread is already on an AP
            }
            sched.tasks[cur].away = true;
        }
        let next = sched.pick_next();
        if next == sched.current {
            if offload {
                sched.tasks[cur].away = false;
            }
            return false; // nothing else runnable (or still just the boot task)
        }
        // Save the outgoing task's SYSCALL-path state...
        let c = &mut sched.tasks[cur].ctx;
        c.kctx = usermode::get_kctx();
        c.user_rsp = usermode::get_user_rsp();
        c.user_active = usermode::user_active();
        let prev: *mut u64 = &mut sched.tasks[cur].rsp;
        let prev_fx: *mut FpuState = &mut *sched.tasks[cur].fpu;
        sched.current = next;
        // ...and load the incoming one's: its own SYSCALL + ring-0 interrupt
        // stack (so two ring-3 tasks never share one), its longjmp context, and
        // its address space.
        let n = sched.tasks[next].ctx;
        usermode::set_syscall_kstack(n.sys_kstack_top);
        gdt::set_rsp0(n.sys_kstack_top);
        usermode::set_kctx(n.kctx);
        usermode::set_user_rsp(n.user_rsp);
        usermode::set_user_active(n.user_active);
        aspace::install(sched.tasks[next].space);
        let next_fx: *const FpuState = &*sched.tasks[next].fpu;
        if offload {
            HANDOFF[cpu()].store(HANDOFF_TO_AP << 56 | cur as u64, Ordering::Release);
        }
        (prev, sched.tasks[next].rsp, prev_fx, next_fx)
        // Lock is dropped here. Tasks are boxed, so the pointers stay valid even
        // if another task is spawned before the switch.
    };
    unsafe { context::switch(prev_rsp_ptr, next_rsp, prev_fx, next_fx) };
    finish_switch();
    true
}

// --- SMP: ring-3 tasks on the application processors (SMP increment 3) -------
//
// Asymmetric multiprocessing. All kernel code keeps running on the bootstrap
// CPU (the kernel was written for one core); the application processors run
// *ring-3 code only*. A user task is handed to an AP at the points where it is
// about to return to ring 3 on the BSP (end of a syscall, a timer preemption);
// on the AP it runs until it makes a syscall, faults, gets preempted for
// another AP-queued task, or is killed — then it goes back (to the BSP, or to
// the AP queue). So user programs compute in parallel on every core while the
// kernel stays single-threaded.
//
// A task "away" from the BSP (queued for or running on an AP) is invisible to
// the BSP scheduler, its stack is never reaped, and its address space is never
// destroyed (`wait_home`). Hand-offs between CPUs happen only after the moving
// task's context is fully saved: the CPU that switched away records the
// hand-off in its per-CPU slot, and the context it switched *to* completes it
// (`finish_switch`) — so no CPU can resume a half-saved task.

const MAX_CPUS: usize = crate::arch::cpu::MAX_CPUS;
const NONE: u64 = u64::MAX;
const NO_TASK: usize = usize::MAX;

const HANDOFF_TO_AP: u64 = 1;
const HANDOFF_TO_BSP: u64 = 2;
const HANDOFF_REQUEUE: u64 = 3;

/// Per-CPU pending hand-off: kind << 56 | task index (NONE = nothing).
static HANDOFF: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(NONE) }; MAX_CPUS];
/// Task index each AP is running (NO_TASK = idle).
static AP_CURRENT: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(NO_TASK) }; MAX_CPUS];
/// Saved stack pointer of each AP's idle context while it runs a task.
static mut AP_IDLE_RSP: [u64; MAX_CPUS] = [0; MAX_CPUS];
/// Each AP's idle-context FPU area (leaked Box).
static AP_IDLE_FPU: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
/// AP timer ticks that interrupted ring-3 code, per CPU (evidence of parallel
/// user execution).
static AP_USER_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
/// Hand-offs BSP -> AP (for the self-test and `cpu`).
static OFFLOADS: AtomicU64 = AtomicU64::new(0);
/// The AP scheduler is running (at least one AP entered its idle loop).
static AP_SCHED: AtomicBool = AtomicBool::new(false);
/// Master switch (the shell's `smp off` / tests can force BSP-only).
static AP_ENABLED: AtomicBool = AtomicBool::new(true);

/// Queue of task indices waiting for an AP (FIFO ring).
struct ApQueue {
    buf: [usize; 64],
    head: usize,
    len: usize,
}

static AP_QUEUE: Mutex<ApQueue> = Mutex::new(ApQueue { buf: [0; 64], head: 0, len: 0 });

fn queue_push(idx: usize) -> bool {
    let mut q = AP_QUEUE.lock();
    if q.len == q.buf.len() {
        return false;
    }
    let tail = (q.head + q.len) % q.buf.len();
    q.buf[tail] = idx;
    q.len += 1;
    true
}

fn queue_pop() -> Option<usize> {
    let mut q = AP_QUEUE.lock();
    if q.len == 0 {
        return None;
    }
    let idx = q.buf[q.head];
    q.head = (q.head + 1) % q.buf.len();
    q.len -= 1;
    Some(idx)
}

fn queue_len() -> usize {
    AP_QUEUE.lock().len
}

fn cpu() -> usize {
    crate::arch::cpu::id()
}

/// Enable/disable handing ring-3 tasks to the APs (tasks already away finish
/// their slice). Returns the previous setting.
pub fn set_ap_scheduling(on: bool) -> bool {
    AP_ENABLED.swap(on, Ordering::SeqCst)
}

/// Ring-3 timer ticks AP `cpu` has run (0 for the BSP).
pub fn ap_user_ticks(cpu: usize) -> u64 {
    AP_USER_TICKS.get(cpu).map(|t| t.load(Ordering::Relaxed)).unwrap_or(0)
}

/// Total ring-3 ticks across all APs.
pub fn ap_user_ticks_total() -> u64 {
    AP_USER_TICKS.iter().map(|t| t.load(Ordering::Relaxed)).sum()
}

/// Number of BSP -> AP hand-offs since boot.
pub fn offloads() -> u64 {
    OFFLOADS.load(Ordering::Relaxed)
}

/// True while AP `cpu` runs a task.
pub fn ap_running(cpu: usize) -> bool {
    AP_CURRENT.get(cpu).map(|c| c.load(Ordering::Relaxed) != NO_TASK).unwrap_or(false)
}

/// Complete the hand-off the previous context on this CPU left behind. Must
/// run right after every context switch, in the context switched to.
fn finish_switch() {
    let me = cpu();
    let h = HANDOFF[me].swap(NONE, Ordering::AcqRel);
    if h == NONE {
        return;
    }
    let (kind, idx) = (h >> 56, (h & 0x00FF_FFFF_FFFF_FFFF) as usize);
    match kind {
        HANDOFF_TO_AP | HANDOFF_REQUEUE => {
            if queue_push(idx) {
                crate::smp::wake_aps();
            } else {
                set_home(idx); // queue full: back to the BSP
            }
        }
        HANDOFF_TO_BSP => {
            set_home(idx);
            crate::smp::wake_bsp(); // it may be idle in hlt
        }
        _ => {}
    }
}

fn set_home(idx: usize) {
    let mut guard = SCHEDULER.lock();
    if let Some(s) = guard.as_mut() {
        if let Some(t) = s.tasks.get_mut(idx) {
            t.away = false;
        }
    }
}

/// BSP, interrupts off, about to return to ring 3 (end of a syscall or a
/// timer preemption of user code): hand the current user task to an idle AP.
/// Returns true if it was handed over (once it runs again, on some AP).
pub fn maybe_offload() -> bool {
    if !AP_SCHED.load(Ordering::Relaxed) || !AP_ENABLED.load(Ordering::Relaxed) || cpu() != 0 {
        return false;
    }
    let aps = crate::smp::aps_online();
    if aps == 0 || queue_len() >= aps {
        return false; // every AP already has work waiting
    }
    let moved = switch_with_offload(true, |_| ());
    if moved {
        OFFLOADS.fetch_add(1, Ordering::Relaxed);
    }
    moved
}

/// AP, interrupts off, on the current task's kernel stack: give the CPU back
/// to this AP's idle context and leave the task with hand-off `kind`. Returns
/// when the task is resumed — on the BSP for HANDOFF_TO_BSP.
fn ap_leave(kind: u64) {
    let me = cpu();
    let idx = AP_CURRENT[me].load(Ordering::Relaxed);
    if me == 0 || idx == NO_TASK {
        return;
    }
    let (prev_rsp, prev_fx) = {
        let mut guard = SCHEDULER.lock();
        let s = guard.as_mut().expect("scheduler not initialized");
        let t = &mut s.tasks[idx];
        (&mut t.rsp as *mut u64, &mut *t.fpu as *mut FpuState)
    };
    HANDOFF[me].store(kind << 56 | idx as u64, Ordering::Release);
    let idle_rsp = unsafe { (core::ptr::addr_of!(AP_IDLE_RSP) as *const u64).add(me).read_volatile() };
    let idle_fx = AP_IDLE_FPU[me].load(Ordering::Relaxed) as *const FpuState;
    unsafe { context::switch(prev_rsp, idle_rsp, prev_fx, idle_fx) };
    finish_switch();
}

/// Syscall from ring 3 on an AP: move to the BSP, where kernel code runs.
pub fn ap_leave_to_bsp() {
    if cpu() != 0 {
        ap_leave(HANDOFF_TO_BSP);
    }
}

/// A ring-3 fault: handle it on the BSP (it ends the program, kernel work).
pub fn ensure_on_bsp() {
    ap_leave_to_bsp();
}

/// LAPIC timer on an AP (after EOI, interrupts off). Ring-3 time is charged to
/// the task; a killed task is sent home; with others waiting, round-robin.
pub fn ap_tick(from_user: bool) {
    let me = cpu();
    if me == 0 || !from_user {
        return;
    }
    let idx = AP_CURRENT[me].load(Ordering::Relaxed);
    if idx == NO_TASK {
        return;
    }
    let n = AP_USER_TICKS[me].fetch_add(1, Ordering::Relaxed) + 1;
    let (alive, space_id) = {
        let mut guard = SCHEDULER.lock();
        let Some(s) = guard.as_mut() else { return };
        let t = &mut s.tasks[idx];
        // The AP timer runs at ~100 Hz, the PIT at ~18.2 Hz: charge one
        // CPU-accounting tick per 5 AP ticks so `ps` / Task Manager units match.
        if n % 5 == 0 {
            t.cpu_ticks += 1;
        }
        (t.state == State::Runnable, t.space.id)
    };
    if n % 5 == 0 {
        crate::process::account_space_tick(space_id);
    }
    if !alive {
        ap_leave(HANDOFF_TO_BSP); // killed: let the BSP reap it
    } else if queue_len() > 0 {
        USER_PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        ap_leave(HANDOFF_REQUEUE);
    }
}

/// Run task `idx` on this AP until it leaves. Returns false if it was not
/// runnable (killed while queued): it goes straight home.
fn ap_run(me: usize, idx: usize) -> bool {
    let (next_rsp, next_fx) = {
        let mut guard = SCHEDULER.lock();
        let Some(s) = guard.as_mut() else { return false };
        let Some(t) = s.tasks.get_mut(idx) else { return false };
        if t.state != State::Runnable || !t.away {
            t.away = false;
            return false;
        }
        AP_CURRENT[me].store(idx, Ordering::Relaxed);
        usermode::set_ap_syscall_kstack(me, t.ctx.sys_kstack_top);
        gdt::set_rsp0_on(me, t.ctx.sys_kstack_top);
        aspace::install_on_ap(me, t.space);
        (t.rsp, &*t.fpu as *const FpuState)
    };
    let idle_rsp = unsafe { (core::ptr::addr_of_mut!(AP_IDLE_RSP) as *mut u64).add(me) };
    let idle_fx = AP_IDLE_FPU[me].load(Ordering::Relaxed) as *mut FpuState;
    unsafe { context::switch(idle_rsp, next_rsp, idle_fx, next_fx) };
    true
}

/// Main loop of an application processor (never returns): run queued ring-3
/// tasks, otherwise sleep until a timer tick or a wake-up IPI.
pub fn ap_idle_loop(me: usize) -> ! {
    AP_IDLE_FPU[me].store(Box::leak(FpuState::clean()) as *mut FpuState as u64, Ordering::Release);
    AP_SCHED.store(true, Ordering::Release);
    loop {
        interrupts::disable_interrupts();
        // A task just came back to this idle context: complete its hand-off,
        // and stop using its address space (it may be destroyed next).
        if AP_CURRENT[me].swap(NO_TASK, Ordering::Relaxed) != NO_TASK {
            aspace::install_on_ap(me, aspace::kernel());
        }
        finish_switch();
        match queue_pop() {
            Some(idx) => {
                ap_run(me, idx);
            }
            // Atomically enable interrupts and halt: no wake-up can slip by.
            None => interrupts::enable_and_halt(),
        }
    }
}

/// Wait (bounded) until no task of `space_id` is away on an AP — required
/// before its address space or stacks may be freed. Its tasks must already be
/// marked finished; an AP sends a running one home at its next tick (~10 ms)
/// and drops a queued one when it pops it. Returns false on timeout (the
/// caller then leaks the space instead of freeing memory an AP may still use).
pub fn wait_home(space_id: u64) -> bool {
    let away = || {
        interrupts::without_interrupts(|| {
            let guard = SCHEDULER.lock();
            guard
                .as_ref()
                .map(|s| s.tasks.iter().any(|t| t.away && t.space.id == space_id))
                .unwrap_or(false)
        })
    };
    if !away() {
        return true;
    }
    crate::smp::wake_aps();
    for _ in 0..400_000u64 {
        for _ in 0..500 {
            core::hint::spin_loop();
        }
        if !away() {
            return true;
        }
    }
    false
}
