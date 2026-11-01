//! Kernel tasks: a preemptive, **symmetric multiprocessor** scheduler
//! (v0.2 "Akar" round-robin, extended to preemptive multi-process ring 3 and
//! then to SMP).
//!
//! Design notes:
//! - **One scheduler for every CPU.** A single task list; each CPU has its own
//!   `current` task and picks the next runnable one itself (round-robin).
//!   Kernel tasks and ring-3 processes run on any CPU, except tasks *pinned* to
//!   one (the boot flow / desktop on CPU 0, and each CPU's idle task).
//! - **Big Kernel Lock (BKL).** The kernel was written for one core, so all
//!   kernel code (everything except interrupt handlers and the scheduler
//!   itself) runs holding one ticket lock: syscalls, kernel tasks, fault
//!   handling. A CPU drops it when it enters ring 3, halts while idle, or
//!   switches away from a task; "holds the BKL" is per-task state carried
//!   across switches (like Linux's old `lock_depth`). Kernel code therefore
//!   still never runs on two CPUs at once, while ring-3 code runs on all of
//!   them and a syscall is served on the CPU that made it. Interrupt handlers
//!   run without the BKL; what they share with kernel code is protected by its
//!   own spin lock or atomics (keyboard queue, mouse, ticks, this scheduler).
//!   Waiting for the BKL is done with interrupts *enabled*, so a CPU queued on
//!   it still answers TLB-shootdown IPIs.
//! - **Only ring-3 code is preempted** (plus kernel code during the v0.2
//!   scheduler demo). Kernel code stays cooperative: preempting the boot task
//!   mid heap/framebuffer work corrupted memory in the past, while a timer that
//!   interrupts *user* code holds no kernel lock by construction.
//! - **Safe hand-over between CPUs.** A task is marked `running` from the
//!   moment a CPU picks it until its context is completely saved: the CPU that
//!   switches away records it in a per-CPU slot and the context it switched
//!   *to* clears the flag (`finish_switch`). No other CPU can pick a running
//!   task, so none can resume a half-saved one.
//! - **Threads of one process never run at the same time** (`busy_spaces`):
//!   the ring-3 runtime (e.g. the bump-heap malloc in bzstart.rs) is not
//!   SMP-safe. Separate processes run in parallel.
//! - Per task: SYSCALL kernel stack (also TSS rsp0 of the CPU running it),
//!   longjmp context for leaving ring 3, FXSAVE area, address space.
//! - The scheduler lock is only taken with interrupts disabled, and never
//!   while waiting for the BKL (the BKL is dropped before it is taken).
//! - Each task owns a heap-allocated kernel stack. Task 0 is the boot flow
//!   (`kernel_main`) on the bootloader-provided stack; it never exits.
//!   Finished tasks are reaped lazily by `spawn` once no CPU runs them.

use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::arch::context::{self, FpuState};
use crate::arch::cpu as interrupts;
use crate::{aspace, gdt, usermode};

const STACK_SIZE: usize = 32 * 1024;
/// Per-thread SYSCALL + ring-0 interrupt stack for ring-3 processes/threads.
/// Sized like the boot task's privilege stack (gdt.rs): a WIN_PRESENT syscall
/// composites every window on it, which needs real headroom.
const SYS_STACK_SIZE: usize = 64 * 1024;
const MAX_CPUS: usize = crate::arch::cpu::MAX_CPUS;
const NO_TASK: usize = usize::MAX;

fn cpu() -> usize {
    crate::arch::cpu::id()
}

// --- Big Kernel Lock -----------------------------------------------------------

static BKL_NEXT: AtomicU32 = AtomicU32::new(0);
static BKL_SERVING: AtomicU32 = AtomicU32::new(0);
/// Times a CPU had to wait for the BKL.
static BKL_WAITS: AtomicU64 = AtomicU64::new(0);
/// Whether the task running on each CPU holds the BKL.
static CPU_BKL: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
/// Whether the task running on each CPU is servicing a ring-3 syscall
/// (mirrors `Task::in_user_syscall`, saved/restored on every switch).
static CPU_USER_SYSCALL: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];

/// Mark the current task as servicing (or not) a ring-3 syscall; returns the
/// previous value.
pub fn set_in_user_syscall(v: bool) -> bool {
    CPU_USER_SYSCALL[cpu()].swap(v, Ordering::Relaxed)
}

/// True while the current task services a ring-3 syscall.
pub fn in_user_syscall() -> bool {
    CPU_USER_SYSCALL[cpu()].load(Ordering::Relaxed)
}

/// Take the BKL (FIFO ticket lock). Interrupts are enabled while waiting so
/// IPIs (TLB shootdown acknowledgements) keep being served, and restored after.
fn bkl_lock() {
    let ticket = BKL_NEXT.fetch_add(1, Ordering::Relaxed);
    if BKL_SERVING.load(Ordering::Acquire) != ticket {
        BKL_WAITS.fetch_add(1, Ordering::Relaxed);
        let was_on = interrupts::interrupts_enabled();
        interrupts::enable_interrupts();
        while BKL_SERVING.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop();
        }
        if !was_on {
            interrupts::disable_interrupts();
        }
    }
}

fn bkl_unlock() {
    BKL_SERVING.fetch_add(1, Ordering::Release);
}

/// Entering kernel code from ring 3 (syscall, fault): take the BKL.
pub fn kernel_enter() {
    if !STARTED.load(Ordering::Relaxed) {
        return;
    }
    let c = cpu();
    if !CPU_BKL[c].swap(true, Ordering::Relaxed) {
        bkl_lock();
    }
}

/// About to return to ring 3: drop the BKL. A task killed while it was in the
/// kernel does not go back to ring 3 — it finishes here.
pub fn kernel_exit() {
    if !STARTED.load(Ordering::Relaxed) {
        return;
    }
    if !current_runnable() {
        exit_current();
    }
    let c = cpu();
    if CPU_BKL[c].swap(false, Ordering::Relaxed) {
        bkl_unlock();
    }
}

/// Halt until the next interrupt, without holding the BKL meanwhile (for
/// kernel tasks that idle, like the desktop loop).
pub fn idle_halt() {
    let c = cpu();
    let held = CPU_BKL[c].swap(false, Ordering::Relaxed);
    if held {
        bkl_unlock();
    }
    interrupts::halt();
    if held {
        CPU_BKL[c].store(true, Ordering::Relaxed);
        bkl_lock();
    }
}

/// After a switch: take the BKL if the task now running holds it.
fn resume_bkl() {
    if CPU_BKL[cpu()].load(Ordering::Relaxed) {
        bkl_lock();
    }
}

// --- Tasks ---------------------------------------------------------------------

/// Per-task SYSCALL-path state.
#[derive(Clone, Copy)]
struct CpuCtx {
    /// SYSCALL kernel stack top; also loaded into the TSS rsp0 of the CPU
    /// running the task.
    sys_kstack_top: u64,
    /// Kernel stack pointer saved on entry to ring 3, for the longjmp back
    /// when the program exits (EXIT / THREAD_EXIT / a fatal fault).
    kctx: u64,
    /// True while the task's ring-3 program is alive (routes EXIT).
    user_active: bool,
}

impl CpuCtx {
    fn kernel() -> CpuCtx {
        CpuCtx { sys_kstack_top: gdt::privilege_stack_top().as_u64(), kctx: 0, user_active: false }
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

/// Ring-3 thread record. Present for tasks that back a user thread or a
/// spawned process's main thread; `None` for kernel tasks.
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
    /// Owned kernel stack; `None` for the boot task and the AP idle tasks.
    stack: Option<Box<[u8]>>,
    /// Accumulated CPU time in timer ticks.
    cpu_ticks: u64,
    user: Option<UserThread>,
    /// Futex address this task is blocked on (valid only in `Blocked` state).
    wait_key: u64,
    ctx: CpuCtx,
    /// Saved x87/SSE registers while switched out.
    fpu: Box<FpuState>,
    /// User address space (`aspace::kernel()` for kernel tasks); its id is the
    /// owning process id.
    space: aspace::Space,
    /// On a CPU, or switched away from one but not yet completely saved.
    running: bool,
    /// Only this CPU may run the task.
    pinned: Option<usize>,
    /// A CPU's idle task (picked only when nothing else can run there).
    idle: bool,
    /// Holds the BKL while running (kernel tasks, and user tasks while inside
    /// the kernel); carried across switches.
    bkl: bool,
    /// CPU that last ran the task.
    last_cpu: usize,
    /// Servicing a syscall from ring 3 (pointer arguments are untrusted).
    in_user_syscall: bool,
}

impl Task {
    fn new(name: &'static str, entry: fn(), rsp: u64, stack: Option<Box<[u8]>>) -> Task {
        Task {
            id: 0,
            name,
            state: State::Runnable,
            rsp,
            entry,
            stack,
            cpu_ticks: 0,
            user: None,
            wait_key: 0,
            ctx: CpuCtx::kernel(),
            fpu: FpuState::clean(),
            space: aspace::kernel(),
            running: false,
            pinned: None,
            idle: false,
            bkl: true,
            last_cpu: 0,
            in_user_syscall: false,
        }
    }
}

struct Scheduler {
    /// Boxed so a task's address never changes when the vector grows: another
    /// CPU may be saving into `rsp`/`fpu` of a task while one is spawned.
    tasks: Vec<Box<Task>>,
    current: [usize; MAX_CPUS],
    idle_of: [usize; MAX_CPUS],
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

/// Address spaces with a thread running on another CPU.
struct BusySpaces {
    ids: [u64; MAX_CPUS],
    n: usize,
}

impl BusySpaces {
    fn contains(&self, id: u64) -> bool {
        id != 0 && self.ids[..self.n].contains(&id)
    }
}

impl Scheduler {
    fn busy_spaces(&self, c: usize) -> BusySpaces {
        let mut b = BusySpaces { ids: [0; MAX_CPUS], n: 0 };
        for (cpu, &idx) in self.current.iter().enumerate() {
            if cpu == c || idx == NO_TASK {
                continue;
            }
            let id = self.tasks[idx].space.id;
            if id != 0 && !b.contains(id) && b.n < b.ids.len() {
                b.ids[b.n] = id;
                b.n += 1;
            }
        }
        b
    }

    /// Next task for CPU `c`: round-robin over runnable tasks this CPU may run
    /// (not running elsewhere, pin respected, no sibling thread running on
    /// another CPU), the current task last; else this CPU's idle task.
    fn pick_next(&self, c: usize) -> usize {
        let cur = self.current[c];
        let busy = self.busy_spaces(c);
        let aps_allowed = AP_ENABLED.load(Ordering::Relaxed);
        let n = self.tasks.len();
        let start = if cur == NO_TASK { 0 } else { cur };
        for off in 1..=n {
            let idx = (start + off) % n;
            let t = &self.tasks[idx];
            let allowed = match t.pinned {
                Some(p) => p == c,
                None => c == 0 || aps_allowed,
            };
            if t.state == State::Runnable
                && !t.idle
                && allowed
                && (!t.running || idx == cur)
                && !busy.contains(t.space.id)
            {
                return idx;
            }
        }
        if self.idle_of[c] != NO_TASK {
            return self.idle_of[c];
        }
        cur
    }
}

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);
/// Timer-driven preemption of *kernel* code. Only the v0.2 scheduler demo
/// turns it on.
static PREEMPT_ENABLED: AtomicBool = AtomicBool::new(false);
/// Let tasks run on the application processors (`cpu on|off`).
static AP_ENABLED: AtomicBool = AtomicBool::new(true);
/// Task each CPU switched away from, until the next context saves it.
static PREV: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(NO_TASK) }; MAX_CPUS];
/// Each CPU is running its idle task.
static IDLE: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
/// Each CPU is running a ring-3 task (user thread / process).
static RUNNING_USER: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];

// Per-CPU statistics.
static RING3_TICKS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static SYSCALLS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static KERNEL_RUNS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static MIGRATIONS: AtomicU64 = AtomicU64::new(0);

/// Enable or disable timer-driven preemption of kernel code. Cooperative
/// `yield_now` always works regardless.
pub fn set_preemption(enabled: bool) {
    PREEMPT_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Allow/forbid tasks on the application processors (they keep running their
/// idle task). Returns the previous setting.
pub fn set_ap_scheduling(on: bool) -> bool {
    AP_ENABLED.swap(on, Ordering::SeqCst)
}

/// Charge one timer tick to the task running on CPU 0 (PIT IRQ). Cheap; skips
/// if the scheduler lock is momentarily held.
pub fn account_tick() {
    if let Some(mut guard) = SCHEDULER.try_lock() {
        if let Some(sched) = guard.as_mut() {
            let cur = sched.current[0];
            if cur != NO_TASK {
                sched.tasks[cur].cpu_ticks += 1;
            }
        }
    }
}

/// Register the boot flow as task 0 (pinned to CPU 0, holding the BKL) and
/// create CPU 0's idle task.
pub fn init() {
    let mut boot = Task::new("kernel", || {}, 0, None);
    boot.running = true;
    boot.pinned = Some(0);
    interrupts::without_interrupts(|| {
        let mut current = [NO_TASK; MAX_CPUS];
        current[0] = 0;
        *SCHEDULER.lock() = Some(Scheduler {
            tasks: alloc::vec![Box::new(boot)],
            current,
            idle_of: [NO_TASK; MAX_CPUS],
            next_id: 1,
        });
    });
    bkl_lock();
    CPU_BKL[0].store(true, Ordering::SeqCst);
    STARTED.store(true, Ordering::SeqCst);
    let idle = spawn_task("idle", idle_body, false, true, Some(0));
    interrupts::without_interrupts(|| {
        if let Some(s) = SCHEDULER.lock().as_mut() {
            s.idle_of[0] = idle;
        }
    });
}

/// Body of an idle task: run whatever becomes runnable, otherwise sleep until
/// the next interrupt (timer tick or wake-up IPI).
fn idle_body() {
    loop {
        interrupts::disable_interrupts();
        if !switch_with(|_| ()) {
            interrupts::enable_and_halt();
        }
    }
}

/// An application processor joins the scheduler: its boot context becomes its
/// idle task. Never returns.
pub fn ap_start(me: usize) -> ! {
    interrupts::disable_interrupts();
    let mut t = Task::new("idle", || {}, 0, None);
    t.running = true;
    t.pinned = Some(me);
    t.idle = true;
    t.bkl = false;
    t.last_cpu = me;
    let joined = {
        let mut guard = SCHEDULER.lock();
        match guard.as_mut() {
            Some(s) => {
                t.id = s.next_id;
                s.next_id += 1;
                s.tasks.push(Box::new(t));
                let idx = s.tasks.len() - 1;
                s.current[me] = idx;
                s.idle_of[me] = idx;
                true
            }
            None => false,
        }
    };
    IDLE[me].store(true, Ordering::Release);
    loop {
        if joined {
            idle_body();
        }
        interrupts::enable_and_halt();
    }
}

/// Spawn a new kernel task that starts at `entry` and exits when it returns.
pub fn spawn(entry: fn()) {
    spawn_named("task", entry);
}

/// Spawn a named kernel task (name appears in the process listing).
pub fn spawn_named(name: &'static str, entry: fn()) {
    spawn_task(name, entry, true, false, None);
}

/// Create a task; returns its index.
fn spawn_task(name: &'static str, entry: fn(), bkl: bool, idle: bool, pinned: Option<usize>) -> usize {
    let stack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let rsp = context::prepare_stack(&stack, task_trampoline);
    let mut t = Task::new(name, entry, rsp, Some(stack));
    t.bkl = bkl;
    t.idle = idle;
    t.pinned = pinned;
    let idx = push_task(t);
    crate::smp::wake_others();
    idx
}

fn push_task(mut t: Task) -> usize {
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        // Lazily reap stacks of finished tasks no CPU is running.
        for task in sched.tasks.iter_mut() {
            if task.state == State::Finished && !task.running {
                task.stack = None;
                task.user = None;
            }
        }
        t.id = sched.next_id;
        sched.next_id += 1;
        sched.tasks.push(Box::new(t));
        sched.tasks.len() - 1
    })
}

// --- Ring-3 threads and processes --------------------------------------------

/// Run `f` on the current task (scheduler lock held, interrupts off).
fn with_current<R>(f: impl FnOnce(&mut Task) -> R) -> R {
    interrupts::without_interrupts(|| {
        let c = cpu();
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");
        let cur = sched.current[c];
        f(&mut sched.tasks[cur])
    })
}

fn current_runnable() -> bool {
    with_current(|t| t.state == State::Runnable)
}

/// Body of a user-thread task: enter ring 3 at its entry until it exits.
fn user_thread_body() {
    let params = with_current(|t| t.user.as_mut().and_then(|u| u.params.take()));
    let Some((rip, arg, user_stack_top)) = params else {
        return;
    };
    let _code = usermode::enter_user_thread(rip, user_stack_top, arg);
}

/// Promote the current task (the app's main thread) to a user thread with its
/// own private SYSCALL kernel stack. Idempotent.
pub fn ensure_main_user_thread() {
    let stack = alloc::vec![0u8; SYS_STACK_SIZE].into_boxed_slice();
    let top = (stack.as_ptr() as u64 + stack.len() as u64) & !0xF;
    let c = cpu();
    with_current(|t| {
        if t.user.is_none() {
            t.user = Some(UserThread { params: None, _sys_stack: Some(stack) });
            // Effective on the main thread's NEXT syscall / ring-3 interrupt.
            t.ctx.sys_kstack_top = top;
            usermode::set_syscall_kstack(c, top);
            gdt::set_rsp0_on(c, top);
        }
    });
}

/// Spawn a ring-3 user thread that enters `rip(arg)` on `user_stack_top` in
/// the current process. Returns its thread id.
pub fn spawn_user_thread(rip: u64, arg: u64, user_stack_top: u64) -> u64 {
    let space = with_current(|t| t.space);
    spawn_ring3("uthread", user_thread_body, (rip, arg, user_stack_top), space)
}

/// Spawn a task that enters ring 3 at `params` = (rip, arg, user_stack_top) in
/// `space`, with its own kernel task stack and SYSCALL/interrupt stack.
fn spawn_ring3(name: &'static str, body: fn(), params: (u64, u64, u64), space: aspace::Space) -> u64 {
    let kstack = alloc::vec![0u8; STACK_SIZE].into_boxed_slice();
    let sys_stack = alloc::vec![0u8; SYS_STACK_SIZE].into_boxed_slice();
    let sys_top = (sys_stack.as_ptr() as u64 + sys_stack.len() as u64) & !0xF;
    let rsp = context::prepare_stack(&kstack, task_trampoline);
    let mut t = Task::new(name, body, rsp, Some(kstack));
    t.user = Some(UserThread { params: Some(params), _sys_stack: Some(sys_stack) });
    t.ctx = CpuCtx { sys_kstack_top: sys_top, kctx: 0, user_active: false };
    t.space = space;
    let idx = push_task(t);
    crate::smp::wake_others();
    interrupts::without_interrupts(|| SCHEDULER.lock().as_ref().map(|s| s.tasks[idx].id).unwrap_or(0))
}

/// Body of a process's main task: run the program in ring 3, then tear down.
fn process_body() {
    let (params, space_id) = with_current(|t| (t.user.as_mut().and_then(|u| u.params.take()), t.space.id));
    let Some((rip, arg, user_stack_top)) = params else {
        return;
    };
    let code = usermode::enter_user_thread(rip, user_stack_top, arg);
    set_user_active(false);
    crate::app::process_exited(space_id, code);
}

/// Start a ring-3 process (its space already loaded). Returns the task id.
pub fn spawn_process(entry: u64, user_stack_top: u64, space: aspace::Space) -> u64 {
    spawn_ring3("app", process_body, (entry, 0, user_stack_top), space)
}

/// Move the current task back to the kernel address space (so its own space
/// can be destroyed) and finish every other task of `space_id`.
pub fn detach_current_from_space(space_id: u64) {
    interrupts::without_interrupts(|| {
        let c = cpu();
        let mut guard = SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            let cur = sched.current[c];
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

/// Finish every task of process `space_id` (PROC_KILL). Refuses the caller's
/// own process. Returns the victim's space if any task was live.
pub fn kill_space(space_id: u64) -> Option<aspace::Space> {
    if space_id == 0 {
        return None;
    }
    interrupts::without_interrupts(|| {
        let c = cpu();
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut()?;
        if sched.tasks[sched.current[c]].space.id == space_id {
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

/// Wait (bounded) until no other CPU runs a task of `space_id` — required
/// before its address space may be freed. Its tasks must already be finished:
/// a CPU running one in ring 3 drops it at its next timer tick, one inside a
/// syscall drops it when the syscall ends. Yields meanwhile, which also lets
/// a CPU queued on the BKL in. Returns false on timeout (the caller then leaks
/// the space instead of freeing memory a CPU may still use).
pub fn wait_space_stopped(space_id: u64) -> bool {
    let running = || {
        interrupts::without_interrupts(|| {
            let c = cpu();
            let guard = SCHEDULER.lock();
            guard
                .as_ref()
                .map(|s| {
                    s.tasks
                        .iter()
                        .enumerate()
                        .any(|(i, t)| t.running && i != s.current[c] && t.space.id == space_id)
                })
                .unwrap_or(false)
        })
    };
    for _ in 0..200_000u64 {
        if !running() {
            return true;
        }
        yield_now();
        for _ in 0..200 {
            core::hint::spin_loop();
        }
    }
    false
}

/// True if a runnable user task is waiting for a CPU — the desktop loop yields
/// to it instead of halting.
pub fn user_work_pending() -> bool {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        let Some(s) = guard.as_ref() else { return false };
        s.tasks
            .iter()
            .any(|t| t.state == State::Runnable && !t.running && !t.idle && t.space.id != 0)
    })
}

/// Process (address-space id) of the task on this CPU, for the timer IRQ's CPU
/// accounting; `None` if the scheduler lock is momentarily held.
pub fn current_space_id_irq() -> Option<u64> {
    let c = cpu();
    let guard = SCHEDULER.try_lock()?;
    guard.as_ref().and_then(|s| {
        let cur = s.current[c];
        (cur != NO_TASK).then(|| s.tasks[cur].space.id)
    })
}

/// Address space of the current task.
pub fn current_space() -> aspace::Space {
    if !STARTED.load(Ordering::Relaxed) {
        return aspace::kernel();
    }
    with_current(|t| t.space)
}

/// Current thread's id (backs THREAD_SELF).
pub fn current_id() -> u64 {
    if !STARTED.load(Ordering::Relaxed) {
        return 0;
    }
    with_current(|t| t.id)
}

/// Before the scheduler exists (early boot ring-3 runs), the longjmp context
/// and user-active flag live here.
static BOOT_KCTX: AtomicU64 = AtomicU64::new(0);
static BOOT_USER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Where the current task's ring-3 entry saves its kernel stack pointer.
pub fn current_kctx_ptr() -> *mut u64 {
    if !STARTED.load(Ordering::Relaxed) {
        return BOOT_KCTX.as_ptr();
    }
    // Tasks are boxed, so the field's address is stable while it runs.
    with_current(|t| &mut t.ctx.kctx as *mut u64)
}

pub fn user_active() -> bool {
    if !STARTED.load(Ordering::Relaxed) {
        return BOOT_USER_ACTIVE.load(Ordering::Relaxed);
    }
    with_current(|t| t.ctx.user_active)
}

pub fn set_user_active(v: bool) {
    if !STARTED.load(Ordering::Relaxed) {
        return BOOT_USER_ACTIVE.store(v, Ordering::Relaxed);
    }
    with_current(|t| t.ctx.user_active = v);
}

/// Futex wait: block the current thread on `key`, then yield. Every CPU has an
/// idle task to switch to, so the block always takes effect.
pub fn futex_wait_block(key: u64) {
    with_current(|t| {
        t.state = State::Blocked;
        t.wait_key = key;
    });
    yield_now();
}

/// FUTEX_WAIT core: if the user word at `addr` (validated by the caller) still
/// holds `expected`, block on it and yield. The comparison and the block are
/// one step under the scheduler lock, and `futex_wake` takes the same lock, so
/// a wake-up between them cannot be lost. Returns false if the value differed.
pub fn futex_wait_if(addr: u64, expected: u32) -> bool {
    let blocked = with_current(|t| {
        let val = unsafe { core::ptr::read_volatile(addr as *const u32) };
        if val != expected {
            return false;
        }
        t.state = State::Blocked;
        t.wait_key = addr;
        true
    });
    if blocked {
        yield_now();
    }
    blocked
}

/// Futex wake: make up to `count` threads blocked on `key` runnable. Returns
/// the number woken.
pub fn futex_wake(key: u64, count: u64) -> u64 {
    let n = interrupts::without_interrupts(|| {
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
    });
    if n > 0 {
        crate::smp::wake_others();
    }
    n
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

/// List all tasks (kernel-side). The user app is added separately by `process`.
pub fn list() -> Vec<TaskInfo> {
    interrupts::without_interrupts(|| {
        let guard = SCHEDULER.lock();
        match guard.as_ref() {
            Some(sched) => sched
                .tasks
                .iter()
                .filter(|t| !t.idle)
                .map(|t| TaskInfo {
                    id: t.id,
                    name: t.name,
                    running: t.running,
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
        return false;
    }
    interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        if let Some(sched) = guard.as_mut() {
            if let Some(t) = sched.tasks.iter_mut().find(|t| t.id == id && !t.idle) {
                if t.state == State::Runnable {
                    t.state = State::Finished;
                    return true;
                }
            }
        }
        false
    })
}

extern "C" fn task_trampoline() -> ! {
    finish_switch();
    resume_bkl();
    let entry = with_current(|t| t.entry);
    entry();
    exit_current()
}

/// Mark the current task finished and switch away for good.
pub fn exit_current() -> ! {
    interrupts::disable_interrupts();
    switch_with(|sched| {
        let cur = sched.current[cpu()];
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

/// Called from a timer interrupt (interrupts disabled, EOI already sent) on
/// any CPU. Ring-3 code is always preemptible; kernel code only while the v0.2
/// demo has kernel preemption switched on.
pub fn preempt(from_user: bool) {
    if !STARTED.load(Ordering::SeqCst) {
        return;
    }
    if from_user || PREEMPT_ENABLED.load(Ordering::SeqCst) {
        if switch_with(|_| ()) && from_user {
            USER_PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// LAPIC timer tick on an application processor (after EOI, interrupts off):
/// account ring-3 time, then preempt like the PIT does on CPU 0.
pub fn ap_timer(from_user: bool) {
    let c = cpu();
    if c == 0 || !STARTED.load(Ordering::Relaxed) {
        return;
    }
    if from_user {
        let n = RING3_TICKS[c].fetch_add(1, Ordering::Relaxed) + 1;
        // The AP timer runs at ~100 Hz, the PIT at ~18.2 Hz: one accounting
        // tick per 5 AP ticks keeps `ps` / Task Manager units comparable.
        if n % 5 == 0 {
            let mut space = None;
            if let Some(mut guard) = SCHEDULER.try_lock() {
                if let Some(s) = guard.as_mut() {
                    let cur = s.current[c];
                    s.tasks[cur].cpu_ticks += 1;
                    space = Some(s.tasks[cur].space.id);
                }
            }
            if let Some(space) = space {
                crate::process::account_space_tick(space);
            }
        }
    }
    preempt(from_user);
}

/// Count a syscall served on this CPU (`fine` = without the BKL), and track how
/// many BKL-free syscalls are in flight on different CPUs at once.
pub fn note_syscall(fine: bool) {
    let c = cpu();
    SYSCALLS[c].fetch_add(1, Ordering::Relaxed);
    if fine {
        FINE_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Syscalls served without the BKL since boot.
static FINE_CALLS: AtomicU64 = AtomicU64::new(0);
/// BKL-free syscalls currently executing / the most ever executing at once.
static FINE_IN_FLIGHT: AtomicU64 = AtomicU64::new(0);
static FINE_MAX_IN_FLIGHT: AtomicU64 = AtomicU64::new(0);

/// Enter / leave a BKL-free syscall (for the concurrency statistic).
pub fn fine_enter() {
    let now = FINE_IN_FLIGHT.fetch_add(1, Ordering::AcqRel) + 1;
    FINE_MAX_IN_FLIGHT.fetch_max(now, Ordering::AcqRel);
}
pub fn fine_leave() {
    FINE_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
}

/// (BKL-free syscalls served, most executing at once); `reset` clears the peak.
pub fn fine_stats(reset: bool) -> (u64, u64) {
    let r = (FINE_CALLS.load(Ordering::Relaxed), FINE_MAX_IN_FLIGHT.load(Ordering::Relaxed));
    if reset {
        FINE_MAX_IN_FLIGHT.store(0, Ordering::Relaxed);
    }
    r
}

/// Number of times the timer took the CPU away from ring-3 code and gave it to
/// another task — direct evidence of preemptive multitasking.
static USER_PREEMPTIONS: AtomicU64 = AtomicU64::new(0);

pub fn user_preemptions() -> u64 {
    USER_PREEMPTIONS.load(Ordering::Relaxed)
}

/// Ring-3 timer ticks CPU `cpu` has run (APs; 0 for CPU 0).
pub fn ap_user_ticks(cpu: usize) -> u64 {
    RING3_TICKS.get(cpu).map(|t| t.load(Ordering::Relaxed)).unwrap_or(0)
}

/// Total ring-3 ticks across all APs.
pub fn ap_user_ticks_total() -> u64 {
    RING3_TICKS.iter().map(|t| t.load(Ordering::Relaxed)).sum()
}

/// Syscalls served on CPU `cpu`.
pub fn syscalls_on(cpu: usize) -> u64 {
    SYSCALLS.get(cpu).map(|t| t.load(Ordering::Relaxed)).unwrap_or(0)
}

/// Kernel tasks switched in on CPU `cpu`.
pub fn kernel_runs_on(cpu: usize) -> u64 {
    KERNEL_RUNS.get(cpu).map(|t| t.load(Ordering::Relaxed)).unwrap_or(0)
}

/// Tasks resumed on a different CPU than the one that last ran them.
pub fn migrations() -> u64 {
    MIGRATIONS.load(Ordering::Relaxed)
}

/// Times a CPU had to wait for the Big Kernel Lock.
pub fn bkl_waits() -> u64 {
    BKL_WAITS.load(Ordering::Relaxed)
}

/// True while CPU `cpu` runs something other than its idle task.
pub fn ap_running(cpu: usize) -> bool {
    IDLE.get(cpu).map(|i| !i.load(Ordering::Relaxed)).unwrap_or(false)
}

/// True while CPU `cpu` runs a ring-3 task.
pub fn cpu_runs_user(cpu: usize) -> bool {
    RUNNING_USER.get(cpu).map(|i| i.load(Ordering::Relaxed)).unwrap_or(false)
}

/// After a switch, in the context switched to: the task this CPU switched away
/// from is now completely saved — another CPU may run it.
fn finish_switch() {
    let c = cpu();
    let prev = PREV[c].swap(NO_TASK, Ordering::AcqRel);
    if prev == NO_TASK {
        return;
    }
    let wake = {
        let mut guard = SCHEDULER.lock();
        match guard.as_mut() {
            Some(s) => {
                let t = &mut s.tasks[prev];
                t.running = false;
                t.state == State::Runnable && !t.idle && t.pinned.is_none()
            }
            None => false,
        }
    };
    // It is still runnable: an idle CPU may take it.
    if wake && IDLE.iter().enumerate().any(|(i, b)| i != c && b.load(Ordering::Relaxed)) {
        crate::smp::wake_others();
    }
}

/// Run `before` on the scheduler, then switch this CPU to the next task if it
/// differs from the current one. Interrupts must be disabled. Returns true if
/// a switch happened (once this task has been resumed, on whichever CPU).
fn switch_with(before: impl FnOnce(&mut Scheduler)) -> bool {
    let c = cpu();
    // Never wait for the BKL while holding the scheduler lock: drop it first.
    let held = CPU_BKL[c].load(Ordering::Relaxed);
    if held {
        bkl_unlock();
    }
    let switch = {
        let mut guard = SCHEDULER.lock();
        match guard.as_mut() {
            None => None,
            Some(sched) => {
                before(sched);
                let cur = sched.current[c];
                let next = sched.pick_next(c);
                if next == cur || next == NO_TASK {
                    None
                } else {
                    sched.tasks[cur].bkl = held;
                    sched.tasks[cur].in_user_syscall = CPU_USER_SYSCALL[c].load(Ordering::Relaxed);
                    let n = &mut sched.tasks[next];
                    n.running = true;
                    if n.last_cpu != c && !n.idle {
                        MIGRATIONS.fetch_add(1, Ordering::Relaxed);
                    }
                    n.last_cpu = c;
                    sched.current[c] = next;
                    PREV[c].store(cur, Ordering::Release);
                    let n = &sched.tasks[next];
                    // The incoming task's own SYSCALL + ring-0 interrupt stack
                    // on this CPU, its address space, and its BKL state.
                    usermode::set_syscall_kstack(c, n.ctx.sys_kstack_top);
                    gdt::set_rsp0_on(c, n.ctx.sys_kstack_top);
                    aspace::install(n.space);
                    CPU_BKL[c].store(n.bkl, Ordering::Relaxed);
                    CPU_USER_SYSCALL[c].store(n.in_user_syscall, Ordering::Relaxed);
                    IDLE[c].store(n.idle, Ordering::Relaxed);
                    RUNNING_USER[c].store(n.user.is_some(), Ordering::Relaxed);
                    if !n.idle && n.user.is_none() {
                        KERNEL_RUNS[c].fetch_add(1, Ordering::Relaxed);
                    }
                    let prev: *mut u64 = &mut sched.tasks[cur].rsp;
                    let prev_fx: *mut FpuState = &mut *sched.tasks[cur].fpu;
                    let next_fx: *const FpuState = &*sched.tasks[next].fpu;
                    Some((prev, sched.tasks[next].rsp, prev_fx, next_fx))
                }
            }
        }
    };
    match switch {
        None => {
            if held {
                bkl_lock();
            }
            false
        }
        Some((prev, next_rsp, prev_fx, next_fx)) => {
            unsafe { context::switch(prev, next_rsp, prev_fx, next_fx) };
            finish_switch();
            resume_bkl();
            true
        }
    }
}
