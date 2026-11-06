//! Debugger for ring-3 programs: breakpoints, single-step, registers, memory
//! and a symbolized backtrace, driven by the shell's `dbg` command (and so
//! from a PC through `bz run "dbg ..."`).
//!
//! **Model.** A debugged program stops *in its own task*: a breakpoint
//! (x86 `int3`, ARM64 `brk #0`, RISC-V `c.ebreak`) or a finished single step
//! traps into the kernel on the program's kernel stack, with its address
//! space installed, and [`on_trap`] parks that task there. Each `dbg`
//! command is a request the parked task executes itself -- so reading and
//! writing the program's memory and registers needs no cross-address-space
//! access -- before it either waits for the next request or returns to user
//! mode (continue / step). The requester (the shell, on the desktop task)
//! waits for the reply on a kernel futex key.
//!
//! **Stepping.** x86: the trap flag (#DB after one instruction); ARM64: the
//! software-step state machine (MDSCR_EL1.SS + SPSR.SS, EC 0x32); RISC-V has
//! no single-step from S-mode, so a step puts temporary breakpoints on every
//! possible next instruction (fall-through, branch / jump targets, `jalr`'s
//! register target). Continuing from a breakpoint steps over the original
//! instruction first and then puts the breakpoint back.
//!
//! Breakpoints are written into the code page by making it writable for a
//! moment (`paging::protect`), then `sync_code` (I-cache on ARM64/RISC-V).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::paging::{self, Flags};

/// The trapped program's registers, viewed as a slice of the trap frame.
pub struct Regs<'a> {
    pub r: &'a mut [u64],
    pub names: &'static [&'static str],
    pub pc: usize,
    pub sp: usize,
    pub link: Option<usize>,
    /// x86 rflags / ARM64 spsr (stepping bits)
    pub flags: Option<usize>,
}

impl Regs<'_> {
    fn pc(&self) -> u64 {
        self.r[self.pc]
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Trap {
    Break,
    Step,
}

#[cfg(target_arch = "x86_64")]
const BP: &[u8] = &[0xCC];
#[cfg(target_arch = "aarch64")]
const BP: &[u8] = &[0x00, 0x00, 0x20, 0xD4]; // brk #0
#[cfg(target_arch = "riscv64")]
const BP: &[u8] = &[0x02, 0x90]; // c.ebreak

enum Req {
    Regs,
    Set(String, u64),
    Mem(u64, usize),
    Break(u64),
    Delete(u64),
    Bt,
    Cont,
    Step,
    Kill,
    Detach,
}

struct Session {
    pid: u64,
    /// (address, original bytes)
    bps: Vec<(u64, Vec<u8>)>,
    /// RISC-V step: temporary breakpoints on the possible next instructions
    temps: Vec<(u64, Vec<u8>)>,
    /// the breakpoint at the program's entry (removed when hit)
    entry: Option<u64>,
    stopped: bool,
    at: u64,
    reason: &'static str,
    req: Option<Req>,
    reply: Option<Vec<String>>,
    /// a breakpoint removed to step over its instruction, to put back
    reinsert: Option<u64>,
    /// the user asked for a step (stop after it) rather than a continue
    user_step: bool,
    /// stopped at a fault: continuing ends the program
    faulted: bool,
}

static SESSIONS: Mutex<Vec<Session>> = Mutex::new(Vec::new());
/// `dbg run`: the next program loaded is debugged (stop at its entry).
static NEXT_LOAD: Mutex<bool> = Mutex::new(false);
/// The session `dbg` commands act on by default.
static CURRENT: Mutex<u64> = Mutex::new(0);

fn key(pid: u64, which: u64) -> u64 {
    0xDB_0000_0000_0000 | (pid << 1) | which
}

fn with<R>(pid: u64, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSIONS.lock().iter_mut().find(|s| s.pid == pid).map(f)
}

// ---- code patching (runs in the target's address space) -------------------

fn read_user(addr: u64, n: usize) -> Option<Vec<u8>> {
    if !crate::memory::validate_user_range(addr, n as u64, false) {
        return None;
    }
    Some((0..n).map(|i| unsafe { core::ptr::read_volatile((addr + i as u64) as *const u8) }).collect())
}

/// Write `bytes` at `addr` in the current (target) address space, through a
/// read-only code page if need be.
fn poke(addr: u64, bytes: &[u8]) -> bool {
    if !crate::memory::validate_user_range(addr, bytes.len() as u64, false) {
        return false;
    }
    let first = addr & !0xFFF;
    let last = (addr + bytes.len() as u64 - 1) & !0xFFF;
    let mut pages = Vec::new();
    let mut p = first;
    while p <= last {
        if let Some((_, writable)) = paging::user_access(p) {
            if !writable {
                pages.push(p);
            }
        }
        p += 0x1000;
    }
    let rw = Flags { write: true, user: true, exec: true };
    let rx = Flags { write: false, user: true, exec: true };
    crate::memory::with_ctx(|ctx| {
        for &pg in &pages {
            paging::protect(&mut ctx.mapper, pg, rw);
        }
    });
    crate::arch::cpu::flush_tlb();
    for (i, b) in bytes.iter().enumerate() {
        unsafe { core::ptr::write_volatile((addr + i as u64) as *mut u8, *b) };
    }
    crate::memory::with_ctx(|ctx| {
        for &pg in &pages {
            paging::protect(&mut ctx.mapper, pg, rx);
        }
    });
    crate::arch::cpu::flush_tlb();
    crate::arch::cpu::sync_code(addr, bytes.len());
    true
}

/// Put a breakpoint instruction at `addr`; the original bytes, or None.
fn insert(addr: u64) -> Option<Vec<u8>> {
    let orig = read_user(addr, BP.len())?;
    if orig == BP {
        return None; // already one there
    }
    poke(addr, BP).then_some(orig)
}

// ---- stepping -----------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
fn hw_step(r: &mut Regs, on: bool) -> bool {
    let f = r.flags.unwrap();
    if on {
        r.r[f] |= 1 << 8; // TF
    } else {
        r.r[f] &= !(1 << 8);
    }
    true
}

#[cfg(target_arch = "aarch64")]
fn hw_step(r: &mut Regs, on: bool) -> bool {
    let f = r.flags.unwrap();
    unsafe {
        let mut mdscr: u64;
        core::arch::asm!("mrs {}, mdscr_el1", out(reg) mdscr, options(nomem, nostack));
        if on {
            mdscr |= 1; // SS
            core::arch::asm!("msr oslar_el1, xzr", options(nomem, nostack)); // OS lock off
            r.r[f] |= 1 << 21; // SPSR.SS: step one instruction, then trap
        } else {
            mdscr &= !1;
            r.r[f] &= !(1 << 21);
        }
        core::arch::asm!("msr mdscr_el1, {}", "isb", in(reg) mdscr, options(nomem, nostack));
    }
    true
}

#[cfg(target_arch = "riscv64")]
fn hw_step(_r: &mut Regs, _on: bool) -> bool {
    false
}

/// RISC-V: every address the instruction at `pc` can continue at.
#[cfg(target_arch = "riscv64")]
fn successors(pc: u64, r: &Regs) -> Vec<u64> {
    let x = |i: u32| if i == 0 { 0 } else { r.r[i as usize] };
    let Some(b) = read_user(pc, 4).or_else(|| read_user(pc, 2)) else { return Vec::new() };
    let lo = u16::from_le_bytes([b[0], b[1]]);
    if lo & 3 != 3 {
        // compressed (2 bytes)
        let next = pc + 2;
        let (op, f3) = (lo & 3, lo >> 13);
        let rs1 = ((lo >> 7) & 0x1F) as u32;
        let rs2 = ((lo >> 2) & 0x1F) as u32;
        if op == 1 && f3 == 5 {
            // c.j
            let i = lo as u32;
            let imm = ((i >> 1) & 0x800) | ((i >> 7) & 0x10) | ((i >> 1) & 0x300) | ((i << 2) & 0x400) | ((i >> 1) & 0x40) | ((i << 1) & 0x80) | ((i >> 2) & 0xE) | ((i << 3) & 0x20);
            let imm = ((imm as i32) << 20 >> 20) as i64;
            return alloc::vec![(pc as i64 + imm) as u64];
        }
        if op == 1 && (f3 == 6 || f3 == 7) {
            // c.beqz / c.bnez
            let i = lo as u32;
            let imm = ((i >> 4) & 0x100) | ((i >> 7) & 0x18) | ((i << 1) & 0xC0) | ((i >> 2) & 0x6) | ((i << 3) & 0x20);
            let imm = ((imm as i32) << 23 >> 23) as i64;
            return alloc::vec![next, (pc as i64 + imm) as u64];
        }
        if op == 2 && f3 == 4 && rs2 == 0 && rs1 != 0 {
            // c.jr / c.jalr
            return alloc::vec![x(rs1) & !1];
        }
        return alloc::vec![next];
    }
    let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let next = pc + 4;
    let rs1 = (w >> 15) & 0x1F;
    match w & 0x7F {
        0x6F => {
            // jal
            let imm = ((w >> 11) & 0x10_0000) | (w & 0xF_F000) | ((w >> 9) & 0x800) | ((w >> 20) & 0x7FE);
            let imm = ((imm as i32) << 11 >> 11) as i64;
            alloc::vec![(pc as i64 + imm) as u64]
        }
        0x67 => {
            // jalr
            let imm = (w as i32 >> 20) as i64;
            alloc::vec![((x(rs1) as i64 + imm) as u64) & !1]
        }
        0x63 => {
            // branches
            let imm = ((w >> 19) & 0x1000) | ((w << 4) & 0x800) | ((w >> 20) & 0x7E0) | ((w >> 7) & 0x1E);
            let imm = ((imm as i32) << 19 >> 19) as i64;
            alloc::vec![next, (pc as i64 + imm) as u64]
        }
        _ => alloc::vec![next],
    }
}

/// Arrange for the program to stop after one instruction.
fn begin_step(s: &mut Session, r: &mut Regs) {
    if hw_step(r, true) {
        return;
    }
    #[cfg(target_arch = "riscv64")]
    for a in successors(r.pc(), r) {
        if s.temps.iter().any(|t| t.0 == a) || s.bps.iter().any(|b| b.0 == a) {
            continue; // a real breakpoint stops there anyway
        }
        if let Some(orig) = insert(a) {
            s.temps.push((a, orig));
        }
    }
    let _ = s;
}

fn clear_temps(s: &mut Session) {
    for (a, orig) in core::mem::take(&mut s.temps) {
        poke(a, &orig);
    }
}

// ---- the trapped side ------------------------------------------------------

/// The next program loaded is debugged: called by the loader (its address
/// space installed) with the program's entry point.
pub fn at_load(pid: u64, entry: u64) {
    if !core::mem::take(&mut *NEXT_LOAD.lock()) {
        return;
    }
    let orig = insert(entry);
    let mut s = Session {
        pid,
        bps: Vec::new(),
        temps: Vec::new(),
        entry: None,
        stopped: false,
        at: 0,
        reason: "",
        req: None,
        reply: None,
        reinsert: None,
        user_step: false,
        faulted: false,
    };
    if let Some(o) = orig {
        s.bps.push((entry, o));
        s.entry = Some(entry);
    }
    SESSIONS.lock().push(s);
    *CURRENT.lock() = pid;
}

/// A breakpoint / step trap from ring 3 (the arch glue has put the pc on the
/// breakpoint's address). False: not a debugger trap (the caller treats it as
/// a fault).
pub fn on_trap(kind: Trap, r: &mut Regs) -> bool {
    let pid = crate::task::current_space().id;
    let pc = r.pc();
    enum Next {
        Stop(&'static str),
        Resume,
        NotOurs,
    }
    let next = with(pid, |s| {
        if kind == Trap::Step {
            hw_step(r, false);
        }
        let temp_hit = kind == Trap::Break && s.temps.iter().any(|t| t.0 == pc);
        if kind == Trap::Step || temp_hit {
            clear_temps(s);
            if let Some(a) = s.reinsert.take() {
                if let Some(i) = s.bps.iter().position(|b| b.0 == a) {
                    if let Some(orig) = insert(a) {
                        s.bps[i].1 = orig;
                    }
                }
            }
            // stepping onto a real breakpoint stops there as a breakpoint
            if s.user_step || s.bps.iter().any(|b| b.0 == pc) {
                return Next::Stop("step");
            }
            return Next::Resume;
        }
        if s.bps.iter().any(|b| b.0 == pc) {
            if s.entry == Some(pc) {
                // the entry breakpoint is one-shot
                s.entry = None;
                if let Some(i) = s.bps.iter().position(|b| b.0 == pc) {
                    let (a, orig) = s.bps.remove(i);
                    poke(a, &orig);
                }
                return Next::Stop("entry");
            }
            return Next::Stop("breakpoint");
        }
        Next::NotOurs
    })
    .unwrap_or(Next::NotOurs);
    match next {
        Next::NotOurs => false,
        Next::Resume => true,
        Next::Stop(why) => {
            serve(pid, why, r);
            true
        }
    }
}

fn describe(pid: u64, addr: u64) -> String {
    format!("{:#x}  {}", addr, crate::crash::symbol_of(pid, addr).unwrap_or_else(|| String::from("?")))
}

/// Parked: answer requests until one resumes the program.
fn serve(pid: u64, why: &'static str, r: &mut Regs) {
    let pc = r.pc();
    with(pid, |s| {
        s.stopped = true;
        s.at = pc;
        s.reason = why;
    });
    crate::println!("[dbg] pid {} stopped ({}) at {}", pid, why, describe(pid, pc));
    crate::task::futex_wake(key(pid, 1), u64::MAX); // `dbg run` / `dbg cont` wait for this
    loop {
        while crate::task::block_while(key(pid, 0), || with(pid, |s| s.req.is_none()).unwrap_or(false)) {}
        let Some(req) = with(pid, |s| s.req.take()).flatten() else { return };
        let (out, resume) = execute(pid, req, r);
        with(pid, |s| {
            s.reply = Some(out);
            if resume != Resume::No {
                s.stopped = false;
            }
        });
        crate::task::futex_wake(key(pid, 1), u64::MAX);
        match resume {
            Resume::No => {}
            Resume::Yes => return,
            Resume::Kill => {
                crate::usermode::exit_user(crate::app::KILLED_EXIT_CODE);
            }
        }
    }
}

#[derive(PartialEq)]
enum Resume {
    No,
    Yes,
    Kill,
}

/// Execute one request in the stopped program's context.
fn execute(pid: u64, req: Req, r: &mut Regs) -> (Vec<String>, Resume) {
    let pc = r.pc();
    match req {
        Req::Regs => {
            let mut out = Vec::new();
            let mut line = String::new();
            for (i, n) in r.names.iter().enumerate() {
                if n.is_empty() {
                    continue;
                }
                line.push_str(&format!("{:>6} {:#018x}  ", n, r.r[i]));
                if line.len() > 80 {
                    out.push(core::mem::take(&mut line));
                }
            }
            if !line.is_empty() {
                out.push(line);
            }
            (out, Resume::No)
        }
        Req::Set(name, v) => match r.names.iter().position(|n| *n == name) {
            Some(i) => {
                r.r[i] = v;
                (alloc::vec![format!("{} = {:#x}", name, v)], Resume::No)
            }
            None => (alloc::vec![format!("dbg: no register '{}'", name)], Resume::No),
        },
        Req::Mem(addr, n) => match read_user(addr, n) {
            Some(mut b) => {
                // show the program's own bytes, not the breakpoints
                with(pid, |s| {
                    for (a, orig) in s.bps.iter().chain(s.temps.iter()) {
                        for (k, o) in orig.iter().enumerate() {
                            let at = a + k as u64;
                            if at >= addr && at < addr + n as u64 && s.reinsert != Some(*a) {
                                b[(at - addr) as usize] = *o;
                            }
                        }
                    }
                });
                let mut out = Vec::new();
                for (k, chunk) in b.chunks(16).enumerate() {
                    let hex: String = chunk.iter().map(|x| format!("{:02x} ", x)).collect();
                    let asc: String = chunk.iter().map(|&x| if (32..127).contains(&x) { x as char } else { '.' }).collect();
                    out.push(format!("{:#010x}  {:<48} {}", addr + k as u64 * 16, hex, asc));
                }
                (out, Resume::No)
            }
            None => (alloc::vec![format!("dbg: {:#x}..+{} is not mapped", addr, n)], Resume::No),
        },
        Req::Break(addr) => {
            let msg = with(pid, |s| {
                if s.bps.iter().any(|b| b.0 == addr) {
                    return format!("breakpoint already at {}", describe(pid, addr));
                }
                match insert(addr) {
                    Some(orig) => {
                        s.bps.push((addr, orig));
                        format!("breakpoint {} at {}", s.bps.len(), describe(pid, addr))
                    }
                    None => format!("dbg: cannot write a breakpoint at {:#x}", addr),
                }
            })
            .unwrap_or_default();
            (alloc::vec![msg], Resume::No)
        }
        Req::Delete(addr) => {
            let msg = with(pid, |s| match s.bps.iter().position(|b| b.0 == addr) {
                Some(i) => {
                    let (a, orig) = s.bps.remove(i);
                    if s.reinsert != Some(a) {
                        poke(a, &orig);
                    }
                    format!("breakpoint at {} deleted", describe(pid, a))
                }
                None => format!("dbg: no breakpoint at {:#x}", addr),
            })
            .unwrap_or_default();
            (alloc::vec![msg], Resume::No)
        }
        Req::Bt => {
            let link = r.link.map(|i| r.r[i]);
            (crate::crash::backtrace(pid, pc, r.r[r.sp], link), Resume::No)
        }
        Req::Cont | Req::Step if with(pid, |s| s.faulted).unwrap_or(false) => {
            (alloc::vec![String::from("continuing (the program faulted: it ends now)")], Resume::Yes)
        }
        Req::Cont | Req::Step => {
            let step = matches!(req, Req::Step);
            with(pid, |s| {
                s.user_step = step;
                if let Some(i) = s.bps.iter().position(|b| b.0 == pc) {
                    // step over the original instruction, then put the breakpoint back
                    let orig = s.bps[i].1.clone();
                    poke(pc, &orig);
                    s.reinsert = Some(pc);
                    begin_step(s, r);
                } else if step {
                    begin_step(s, r);
                }
            });
            (alloc::vec![String::from(if step { "stepping" } else { "continuing" })], Resume::Yes)
        }
        Req::Kill => (alloc::vec![format!("pid {} killed", pid)], Resume::Kill),
        Req::Detach => {
            with(pid, |s| {
                clear_temps(s);
                for (a, orig) in core::mem::take(&mut s.bps) {
                    if s.reinsert != Some(a) {
                        poke(a, &orig);
                    }
                }
                s.reinsert = None;
            });
            SESSIONS.lock().retain(|s| s.pid != pid);
            (alloc::vec![format!("detached from pid {}", pid)], Resume::Yes)
        }
    }
}

/// A debugged program faulted: stop there (`dbg bt`, `regs`, `mem` show the
/// state) before the caller ends it. Only pc / sp / link are known here.
pub fn on_fault(what: &str, pc: u64, sp: u64, link: Option<u64>) {
    let pid = crate::task::current_space().id;
    if with(pid, |s| s.faulted = true).is_none() {
        return;
    }
    crate::println!("[dbg] pid {}: {}", pid, what);
    let mut r = [pc, sp, link.unwrap_or(0)];
    const NAMES: [&str; 3] = ["pc", "sp", "lr"];
    let names: &'static [&'static str] = if link.is_some() { &NAMES } else { &NAMES[..2] };
    let mut regs = Regs { r: &mut r, names, pc: 0, sp: 1, link: link.map(|_| 2), flags: None };
    serve(pid, "fault", &mut regs);
}

/// The program ended (exit, fault, kill): drop its session, release waiters.
pub fn on_exit(pid: u64) {
    let had = {
        let mut ss = SESSIONS.lock();
        let n = ss.len();
        ss.retain(|s| s.pid != pid);
        n != ss.len()
    };
    if had {
        crate::task::futex_wake(key(pid, 1), u64::MAX);
    }
}

// ---- the requesting side (the shell) ------------------------------------------

/// Send a request to a stopped program and wait for its reply.
fn request(pid: u64, req: Req) -> Vec<String> {
    let ok = with(pid, |s| {
        if !s.stopped {
            return Err(String::from("dbg: the program is running (it stops at a breakpoint)"));
        }
        s.req = Some(req);
        s.reply = None;
        Ok(())
    });
    match ok {
        None => return alloc::vec![format!("dbg: no debugged process {}", pid)],
        Some(Err(e)) => return alloc::vec![e],
        Some(Ok(())) => {}
    }
    crate::task::futex_wake(key(pid, 0), u64::MAX);
    while crate::task::block_while(key(pid, 1), || with(pid, |s| s.reply.is_none()).unwrap_or(false)) {}
    with(pid, |s| s.reply.take()).flatten().unwrap_or_else(|| match crate::process::exit_code(pid) {
        Some(c) => alloc::vec![format!("pid {} exited with code {:#x}", pid, c)],
        None => alloc::vec![format!("pid {} ended", pid)],
    })
}

/// Wait (up to `ms`) until `pid` is stopped or gone; the status line.
fn wait_stop(pid: u64, ms: u64) -> String {
    let t0 = crate::arch::cpu::nanos();
    loop {
        match with(pid, |s| (s.stopped, s.at, s.reason)) {
            None => {
                return match crate::process::exit_code(pid) {
                    Some(c) => format!("pid {} exited with code {:#x}", pid, c),
                    None => format!("pid {} is not being debugged", pid),
                }
            }
            Some((true, at, why)) => return format!("pid {} stopped ({}) at {}", pid, why, describe(pid, at)),
            Some((false, _, _)) => {}
        }
        if crate::arch::cpu::nanos() - t0 > ms * 1_000_000 {
            return format!("pid {} is running (not stopped within {} ms; 'dbg' shows its state)", pid, ms);
        }
        crate::task::yield_now();
    }
}

/// An address: `0x...`, a decimal number, or a symbol of the program.
fn address(pid: u64, s: &str) -> Result<u64, String> {
    if let Some(h) = s.strip_prefix("0x") {
        return u64::from_str_radix(h, 16).map_err(|_| format!("dbg: bad address '{}'", s));
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(n);
    }
    crate::crash::find_symbol(pid, s)
}

/// The `dbg` shell command.
pub fn command(args: &[&str]) -> Vec<String> {
    let cur = *CURRENT.lock();
    let one = |v: String| alloc::vec![v];
    match args {
        [] | ["list"] => {
            let ss = SESSIONS.lock();
            if ss.is_empty() {
                return one(String::from("dbg: no debugged programs ('dbg run APP [args]' starts one)"));
            }
            ss.iter()
                .map(|s| {
                    format!(
                        "{} pid {} {} {}, {} breakpoint(s)",
                        if s.pid == cur { "*" } else { " " },
                        s.pid,
                        crate::process::name_of(s.pid).unwrap_or_default(),
                        if s.stopped { format!("stopped ({}) at {:#x}", s.reason, s.at) } else { String::from("running") },
                        s.bps.len()
                    )
                })
                .collect()
        }
        ["run", app, rest @ ..] => {
            *NEXT_LOAD.lock() = true;
            let r = crate::app::spawn_with_args(app, &rest.join(" "));
            let armed = core::mem::take(&mut *NEXT_LOAD.lock()); // still set: no ELF was loaded
            match r {
                Ok(pid) if !armed => one(wait_stop(pid, 5000)),
                Ok(pid) => one(format!("dbg: pid {} is not a native ELF program (managed apps and scripts run in CLRHOST.ELF)", pid)),
                Err(e) => one(format!("dbg: {}", e)),
            }
        }
        ["pid", p] => match p.parse::<u64>() {
            Ok(p) if with(p, |_| ()).is_some() => {
                *CURRENT.lock() = p;
                one(format!("dbg: now on pid {}", p))
            }
            _ => one(format!("dbg: no debugged process {}", p)),
        },
        ["break" | "b", a] => match address(cur, a) {
            Ok(addr) => request(cur, Req::Break(addr)),
            Err(e) => one(e),
        },
        ["delete" | "d", a] => match address(cur, a) {
            Ok(addr) => request(cur, Req::Delete(addr)),
            Err(e) => one(e),
        },
        ["breaks"] => with(cur, |s| s.bps.iter().map(|b| describe(cur, b.0)).collect()).unwrap_or_else(|| one(String::from("dbg: no session"))),
        ["cont" | "c"] | ["step" | "s"] => {
            let step = matches!(args, ["step" | "s"]);
            let mut out = request(cur, if step { Req::Step } else { Req::Cont });
            if out.first().is_some_and(|l| l.starts_with("continuing") || l == "stepping") {
                out.clear();
                out.push(wait_stop(cur, 3000));
            }
            out
        }
        ["regs" | "r"] => request(cur, Req::Regs),
        ["set", reg, v] => match address(cur, v) {
            Ok(v) => request(cur, Req::Set(String::from(*reg), v)),
            Err(e) => one(e),
        },
        ["mem" | "x", a, rest @ ..] => {
            let n = rest.first().and_then(|n| n.parse::<usize>().ok()).unwrap_or(64).min(1024);
            match address(cur, a) {
                Ok(addr) => request(cur, Req::Mem(addr, n)),
                Err(e) => one(e),
            }
        }
        ["bt"] => request(cur, Req::Bt),
        ["kill"] => {
            let out = request(cur, Req::Kill);
            let _ = wait_stop(cur, 2000);
            out
        }
        ["detach"] => request(cur, Req::Detach),
        _ => alloc::vec![
            String::from("dbg run APP [args]   start a program under the debugger (stops at its entry)"),
            String::from("dbg break SYM|ADDR   breakpoint (a symbol name or part of one, or 0x...)"),
            String::from("dbg cont | step      continue / one instruction"),
            String::from("dbg regs | bt        registers / backtrace"),
            String::from("dbg mem ADDR [N]     memory (hex + text)"),
            String::from("dbg set REG VALUE    change a register"),
            String::from("dbg delete SYM|ADDR | breaks | list | pid N | kill | detach"),
        ],
    }
}

/// Self-test (boot): debug `app`, break on `func`, continue to it, step,
/// read registers, memory and the backtrace (which must name `caller`), kill.
pub fn self_test(app: &str, func: &str, caller: &str) -> bool {
    let started = command(&["run", app]);
    let s0 = started.join(" ");
    let Some(pid) = s0.split_whitespace().nth(1).and_then(|p| p.parse::<u64>().ok()) else {
        crate::println!("[dbg] self-test: {}", s0);
        return false;
    };
    let at_entry = s0.contains("stopped (entry)");
    let b = command(&["break", func]).join(" ");
    let hit = command(&["cont"]).join(" ");
    let at_func = hit.contains("stopped (breakpoint)") && hit.contains(func);
    let bt = command(&["bt"]).join("\n");
    let regs = command(&["regs"]);
    let sp_name = if cfg!(target_arch = "x86_64") { "rsp" } else { "sp" };
    let addr = crate::crash::find_symbol(pid, func).unwrap_or(0);
    let mem = command(&["mem", &format!("{:#x}", addr), "16"]).join(" ");
    let mem_ok = addr != 0 && mem.starts_with(&format!("{:#010x}", addr));
    let step = command(&["step"]).join(" ");
    let stepped = step.contains("stopped (step)");
    let killed = command(&["kill"]).join(" ");
    let exit = crate::app::wait(pid);
    let ok = at_entry
        && at_func
        && bt.contains(caller)
        && regs.iter().any(|l| l.contains(sp_name))
        && mem_ok
        && stepped
        && exit == crate::app::KILLED_EXIT_CODE;
    for l in [&s0, &b, &hit, &step, &killed] {
        crate::println!("[dbg] self-test: {}", l);
    }
    for l in bt.lines() {
        crate::println!("[dbg] self-test bt: {}", l);
    }
    if ok {
        crate::println!(
            "[kernel] MILESTONE: DEBUGGER OK (ring-3 program stopped at its entry, breakpoint on {}, backtrace names {}, registers + memory read, single step, killed)",
            func, caller
        );
    } else {
        crate::println!(
            "[dbg] self-test failed: entry {} func {} bt {} regs {} mem {} step {} exit {:#x}",
            at_entry,
            at_func,
            bt.contains(caller),
            regs.len(),
            mem_ok,
            stepped,
            exit
        );
    }
    ok
}
