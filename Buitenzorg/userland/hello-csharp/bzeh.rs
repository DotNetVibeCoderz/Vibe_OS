//! Exception handling for native C# programs (`throw`, `try` / `catch` /
//! `finally`, `catch ... when`, `throw;`), the part of the NativeAOT runtime
//! zerolib has none of. Programs are compiled with the upstream ILC (see
//! scripts/build-hello-csharp.*), which emits what the dispatcher needs:
//!
//! * `.eh_frame` (DWARF CFI) for every method and funclet: how to unwind a
//!   frame to its caller;
//! * an LSDA per FDE (the NativeAOT format: a flags byte -- kind ROOT /
//!   HANDLER / FILTER, has EH info, has associated data -- then for a funclet
//!   a relative pointer to its method's LSDA and its start offset, then a
//!   relative pointer to the EH info);
//! * the EH info: a VarInt clause count, then per clause try start,
//!   try length << 2 | kind (0 typed, 1 fault/finally, 2 filter), handler
//!   offset, and a relative pointer to the caught MethodTable (typed) or the
//!   filter offset (filter).
//!
//! `RhpThrowEx(obj)` runs the two passes .NET does: pass 1 walks up the stack
//! to the first clause that takes the exception (typed: the object is an
//! instance of the clause's type; filter: its funclet says so); pass 2 walks
//! the same frames again calling every finally / fault funclet on the way,
//! then calls the catch funclet and resumes its method at the address the
//! funclet returns, with the callee-saved registers and stack pointer the
//! method had at that frame. No handler: `Unhandled exception: <Message>`
//! and the program ends (finally blocks do not run then, as in .NET) through
//! CRASH_REPORT: a crash report from the throw, and a debugger stop there.
//!
//! Funclets run on the frame pointer of their method: x86-64 -- rbp = the
//! method's rbp, rsi = the exception, returns rax (catch: resume address,
//! filter: 0 / 1); ARM64 -- fp = the method's fp, x1 = the exception, returns
//! x0. They are called through `bz_call_funclet`, which keeps a record of the
//! running funclet just above its return address: a frame whose return
//! address is that call site is a funclet the dispatcher started, and the
//! walk continues in the method that owns it (skipping the clauses up to the
//! one being handled) -- that is how an exception thrown inside a catch or
//! finally block, and `throw;`, find their way out.

use crate::{bz_write, MethodTable, SystemNative_Abort};

extern "C" {
    static __bz_eh_frame_start: u8;
    static __bz_eh_frame_end: u8;
}

#[cfg(target_arch = "x86_64")]
mod arch {
    /// DWARF number of the stack pointer.
    pub const SP: usize = 7; // (the context keeps rbx rbp r12-r15: 3, 6, 12-15)
}

#[cfg(target_arch = "aarch64")]
mod arch {
    pub const SP: usize = 31; // (the context keeps x19-x30)
}

const NREG: usize = 33;

/// A frame's machine state: the pc (a return address, except where noted),
/// the stack pointer and the callee-saved registers, by DWARF number.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ctx {
    pub pc: usize,
    pub regs: [usize; NREG],
}

/// What `bz_call_funclet` leaves above the funclet's return address.
#[repr(C)]
struct Running {
    /// The method frame the funclet belongs to.
    parent: Ctx,
    /// The clause being handled (the walk continues after it).
    clause: usize,
    obj: *const u8,
    /// 2 = filter (pass 1): an exception escaping it is not supported.
    kind: usize,
}

// ---- DWARF ------------------------------------------------------------------

unsafe fn uleb(p: &mut *const u8) -> usize {
    let (mut r, mut s) = (0usize, 0);
    loop {
        let b = **p;
        *p = p.add(1);
        r |= ((b & 0x7f) as usize) << s;
        s += 7;
        if b & 0x80 == 0 {
            return r;
        }
    }
}

unsafe fn sleb(p: &mut *const u8) -> isize {
    let (mut r, mut s) = (0isize, 0);
    loop {
        let b = **p;
        *p = p.add(1);
        r |= ((b & 0x7f) as isize) << s;
        s += 7;
        if b & 0x80 == 0 {
            if s < 64 && b & 0x40 != 0 {
                r |= -1isize << s;
            }
            return r;
        }
    }
}

unsafe fn rd32(p: *const u8) -> i32 {
    core::ptr::read_unaligned(p as *const i32)
}

/// A pointer in an eh_frame encoding (absolute or pc-relative, 4 or 8 bytes).
unsafe fn enc_ptr(p: &mut *const u8, enc: u8) -> usize {
    let at = *p as usize;
    let v = match enc & 0x0f {
        0x00 => {
            let v = core::ptr::read_unaligned(*p as *const usize);
            *p = p.add(8);
            v
        }
        0x03 => {
            let v = core::ptr::read_unaligned(*p as *const u32) as usize;
            *p = p.add(4);
            v
        }
        0x0b => {
            let v = rd32(*p) as isize as usize;
            *p = p.add(4);
            v
        }
        0x04 | 0x0c => {
            let v = core::ptr::read_unaligned(*p as *const usize);
            *p = p.add(8);
            v
        }
        _ => return 0,
    };
    if enc & 0x70 == 0x10 {
        at.wrapping_add(v)
    } else {
        v
    }
}

#[derive(Clone, Copy)]
enum Rule {
    Same,
    Offset(isize),
}

struct Fde {
    start: usize,
    lsda: usize,
    cie_insns: *const u8,
    cie_end: *const u8,
    insns: *const u8,
    end_insns: *const u8,
    code_align: usize,
    data_align: isize,
    ra: usize,
}

/// The FDE covering `pc`.
unsafe fn find_fde(pc: usize) -> Option<Fde> {
    let mut p = core::ptr::addr_of!(__bz_eh_frame_start);
    let end = core::ptr::addr_of!(__bz_eh_frame_end);
    while p < end {
        let len = core::ptr::read_unaligned(p as *const u32) as usize;
        if len == 0 || len == 0xffff_ffff {
            return None;
        }
        let rec = p.add(4);
        let next = rec.add(len);
        let id = core::ptr::read_unaligned(rec as *const u32) as usize;
        if id != 0 {
            // an FDE: its CIE first (the pointer is the distance from this
            // field back to the CIE's length field)
            let cie = rec.sub(id);
            let cie_len = core::ptr::read_unaligned(cie as *const u32) as usize;
            let cie_end = cie.add(4 + cie_len);
            let mut c = cie.add(8); // past the length and the CIE id
            let _version = *c;
            c = c.add(1);
            let aug = c;
            while *c != 0 {
                c = c.add(1);
            }
            c = c.add(1);
            let code_align = uleb(&mut c);
            let data_align = sleb(&mut c);
            let ra = uleb(&mut c);
            let (mut lsda_enc, mut fde_enc) = (0xffu8, 0u8);
            let mut has_z = false;
            if *aug == b'z' {
                has_z = true;
                let n = uleb(&mut c);
                let after = c.add(n);
                let mut a = aug.add(1);
                while *a != 0 {
                    match *a {
                        b'L' => {
                            lsda_enc = *c;
                            c = c.add(1);
                        }
                        b'R' => {
                            fde_enc = *c;
                            c = c.add(1);
                        }
                        b'P' => {
                            let e = *c;
                            c = c.add(1);
                            enc_ptr(&mut c, e);
                        }
                        _ => {}
                    }
                    a = a.add(1);
                }
                c = after;
            }
            let mut f = rec.add(4);
            let start = enc_ptr(&mut f, fde_enc);
            let range = enc_ptr(&mut f, fde_enc & 0x0f);
            let mut lsda = 0;
            if has_z {
                let n = uleb(&mut f);
                let after = f.add(n);
                if lsda_enc != 0xff && n > 0 {
                    lsda = enc_ptr(&mut f, lsda_enc);
                }
                f = after;
            }
            if pc >= start && pc < start + range {
                return Some(Fde { start, lsda, cie_insns: c, cie_end, insns: f, end_insns: next, code_align, data_align, ra });
            }
        }
        p = next;
    }
    None
}

struct Row {
    cfa_reg: usize,
    cfa_off: isize,
    rules: [Rule; NREG],
}

/// Run CFA instructions up to `pc` (exclusive of rows after it).
unsafe fn run_cfi(row: &mut Row, init: &[Rule; NREG], mut p: *const u8, end: *const u8, f: &Fde, pc: usize, mut loc: usize) -> bool {
    let mut stack: [(usize, isize, [Rule; NREG]); 4] = [(0, 0, [Rule::Same; NREG]); 4];
    let mut depth = 0;
    while p < end {
        let op = *p;
        p = p.add(1);
        let (hi, lo) = (op & 0xc0, (op & 0x3f) as usize);
        match hi {
            0x40 => {
                loc += lo * f.code_align;
                if loc > pc {
                    return true;
                }
                continue;
            }
            0x80 => {
                let off = uleb(&mut p) as isize * f.data_align;
                if lo < NREG {
                    (*row.rules.get_unchecked_mut(lo)) = Rule::Offset(off);
                }
                continue;
            }
            0xc0 => {
                if lo < NREG {
                    (*row.rules.get_unchecked_mut(lo)) = (*init.get_unchecked(lo));
                }
                continue;
            }
            _ => {}
        }
        match op {
            0x00 => {}
            0x01 => {
                loc = enc_ptr(&mut p, 0x00);
                if loc > pc {
                    return true;
                }
            }
            0x02 | 0x03 | 0x04 => {
                let d = match op {
                    0x02 => {
                        let v = *p as usize;
                        p = p.add(1);
                        v
                    }
                    0x03 => {
                        let v = core::ptr::read_unaligned(p as *const u16) as usize;
                        p = p.add(2);
                        v
                    }
                    _ => {
                        let v = core::ptr::read_unaligned(p as *const u32) as usize;
                        p = p.add(4);
                        v
                    }
                };
                loc += d * f.code_align;
                if loc > pc {
                    return true;
                }
            }
            0x05 => {
                let r = uleb(&mut p);
                let off = uleb(&mut p) as isize * f.data_align;
                if r < NREG {
                    (*row.rules.get_unchecked_mut(r)) = Rule::Offset(off);
                }
            }
            0x06 | 0x07 | 0x08 => {
                let r = uleb(&mut p);
                if r < NREG {
                    (*row.rules.get_unchecked_mut(r)) = if op == 0x06 { (*init.get_unchecked(r)) } else { Rule::Same };
                }
            }
            0x0a => {
                if depth < 4 {
                    (*stack.get_unchecked_mut(depth)) = (row.cfa_reg, row.cfa_off, row.rules);
                    depth += 1;
                }
            }
            0x0b => {
                if depth > 0 {
                    depth -= 1;
                    row.cfa_reg = (*stack.get_unchecked_mut(depth)).0;
                    row.cfa_off = (*stack.get_unchecked_mut(depth)).1;
                    row.rules = (*stack.get_unchecked_mut(depth)).2;
                }
            }
            0x0c => {
                row.cfa_reg = uleb(&mut p);
                row.cfa_off = uleb(&mut p) as isize;
            }
            0x0d => row.cfa_reg = uleb(&mut p),
            0x0e => row.cfa_off = uleb(&mut p) as isize,
            0x11 => {
                let r = uleb(&mut p);
                let off = sleb(&mut p) * f.data_align;
                if r < NREG {
                    (*row.rules.get_unchecked_mut(r)) = Rule::Offset(off);
                }
            }
            0x12 => {
                row.cfa_reg = uleb(&mut p);
                row.cfa_off = sleb(&mut p) * f.data_align;
            }
            0x13 => row.cfa_off = sleb(&mut p) * f.data_align,
            0x2e => {
                uleb(&mut p);
            }
            _ => return false, // expressions etc.: not emitted by the JIT
        }
    }
    true
}

/// The caller's state of the frame `c` (a pc inside `f`). `lookup` = the pc
/// used to pick the CFI row (the return address minus one for calls).
unsafe fn unwind(c: &Ctx, f: &Fde, lookup: usize) -> Option<Ctx> {
    let mut row = Row { cfa_reg: arch::SP, cfa_off: 0, rules: [Rule::Same; NREG] };
    if !run_cfi(&mut row, &[Rule::Same; NREG], f.cie_insns, f.cie_end, f, usize::MAX, 0) {
        return None;
    }
    let init = row.rules;
    if !run_cfi(&mut row, &init, f.insns, f.end_insns, f, lookup, f.start) {
        return None;
    }
    // (checked here so no indexing below can panic: the shim links no libcore)
    if row.cfa_reg >= NREG || f.ra >= NREG {
        return None;
    }
    let cfa = (c.regs[row.cfa_reg] as isize + row.cfa_off) as usize;
    let mut n = *c;
    for r in 0..NREG {
        if let Rule::Offset(o) = row.rules[r] {
            n.regs[r] = core::ptr::read_unaligned((cfa as isize + o) as *const usize);
        }
    }
    n.pc = match row.rules[f.ra] {
        Rule::Offset(o) => core::ptr::read_unaligned((cfa as isize + o) as *const usize),
        Rule::Same => c.regs[f.ra], // ARM64 leaf: the link register
    };
    n.regs[arch::SP] = cfa;
    Some(n)
}

// ---- NativeAOT LSDA / EH info ---------------------------------------------------

const KIND_MASK: u8 = 3;
const KIND_ROOT: u8 = 0;
const KIND_FILTER: u8 = 2;
const HAS_EHINFO: u8 = 4;
const HAS_ASSOCIATED: u8 = 0x10;

/// NativeAOT's VarInt: the low bits of the first byte give the length.
unsafe fn varint(p: &mut *const u8) -> usize {
    let b = **p;
    let (len, shift) = if b & 1 == 0 {
        (1, 1)
    } else if b & 2 == 0 {
        (2, 2)
    } else if b & 4 == 0 {
        (3, 3)
    } else if b & 8 == 0 {
        (4, 4)
    } else {
        let v = core::ptr::read_unaligned(p.add(1) as *const u32) as usize;
        *p = p.add(5);
        return v;
    };
    let mut v = 0usize;
    for i in 0..len {
        v |= (*p.add(i) as usize) << (8 * i);
    }
    *p = p.add(len);
    v >> shift
}

struct Method {
    start: usize,
    ehinfo: *const u8,
    funclet_kind: u8,
}

unsafe fn method_of(f: &Fde) -> Method {
    let mut m = Method { start: f.start, ehinfo: core::ptr::null(), funclet_kind: KIND_ROOT };
    if f.lsda == 0 {
        return m;
    }
    let lsda = f.lsda as *const u8;
    let flags = *lsda;
    let mut main = lsda;
    if flags & KIND_MASK != KIND_ROOT {
        let p = lsda.add(1);
        main = p.offset(rd32(p) as isize);
        m.start = f.start.wrapping_sub(rd32(p.add(4)) as isize as usize);
        m.funclet_kind = flags & KIND_MASK;
    }
    let mflags = *main;
    let mut q = main.add(1);
    if mflags & HAS_ASSOCIATED != 0 {
        q = q.add(4);
    }
    if mflags & HAS_EHINFO != 0 {
        m.ehinfo = q.offset(rd32(q) as isize);
    }
    m
}

#[derive(Clone, Copy)]
struct Clause {
    try_start: usize,
    try_end: usize,
    kind: usize,
    handler: usize,
    target: *const MethodTable,
    filter: usize,
}

/// Clause `index` of a method (None past the last).
unsafe fn clause(m: &Method, index: usize) -> Option<Clause> {
    if m.ehinfo.is_null() {
        return None;
    }
    let mut p = m.ehinfo;
    let n = varint(&mut p);
    if index >= n {
        return None;
    }
    let mut c = Clause { try_start: 0, try_end: 0, kind: 0, handler: 0, target: core::ptr::null(), filter: 0 };
    for i in 0..=index {
        let ts = varint(&mut p);
        let tl = varint(&mut p);
        c.try_start = ts;
        c.try_end = ts + (tl >> 2);
        c.kind = tl & 3;
        c.handler = m.start + varint(&mut p);
        c.target = core::ptr::null();
        match c.kind {
            0 => {
                c.target = p.offset(rd32(p) as isize) as *const MethodTable;
                p = p.add(4);
            }
            2 => c.filter = m.start + varint(&mut p),
            _ => {}
        }
        let _ = i;
    }
    Some(c)
}

// ---- dispatch ---------------------------------------------------------------------

extern "C" {
    fn bz_call_funclet(handler: usize, ctx: *const Ctx, obj: *const u8, running: *const Running) -> usize;
    fn bz_eh_resume(ctx: *const Ctx, pc: usize) -> !;
    static bz_funclet_return: u8;
}

/// One step up the stack: (caller state, first clause to consider there).
/// A funclet started by the dispatcher returns into `bz_call_funclet`: the
/// walk goes on in its method, after the clause it was handling.
unsafe fn step(c: &Ctx, f: &Fde) -> Option<(Ctx, usize)> {
    let n = unwind(c, f, c.pc - 1)?;
    if n.pc == core::ptr::addr_of!(bz_funclet_return) as usize {
        let r = &*(n.regs[arch::SP] as *const *const Running).read();
        if r.kind == KIND_FILTER as usize {
            fatal_msg(b"exception thrown inside an exception filter (not supported)");
        }
        return Some((r.parent, r.clause + 1));
    }
    Some((n, 0))
}

/// Is the call at return address `pc` inside the clause's try range? The
/// call instruction is (pc - 1): on ARM64 a call can be the last instruction
/// of a try block, so its return address is the range's end.
unsafe fn in_try(c: &Clause, m: &Method, pc: usize) -> bool {
    let off = pc.wrapping_sub(1).wrapping_sub(m.start);
    off >= c.try_start && off < c.try_end
}

/// Throw `obj` from the frame `ctx` (the throwing method, pc = the return
/// address of its call to the throw helper).
#[no_mangle]
pub unsafe extern "C" fn bz_eh_throw(obj: *const u8, ctx: *const Ctx) -> ! {
    if obj.is_null() {
        fatal_msg(b"System.NullReferenceException (throw null)");
    }
    let first = *ctx;
    // pass 1: find the clause that catches
    let mut c = first;
    let mut from = 0usize;
    let mut depth = 0usize;
    let found: (usize, usize, usize);
    'search: loop {
        let Some(f) = find_fde(c.pc - 1) else { unhandled(obj, &first) };
        let m = method_of(&f);
        let mut i = from;
        while let Some(cl) = clause(&m, i) {
            if in_try(&cl, &m, c.pc) {
                let take = match cl.kind {
                    0 => crate::assignable(cl.target, crate::type_of(obj)),
                    2 => {
                        let r = Running { parent: c, clause: i, obj, kind: KIND_FILTER as usize };
                        bz_call_funclet(cl.filter, &c, obj, &r) & 0xffff_ffff != 0
                    }
                    _ => false,
                };
                if take {
                    found = (depth, i, cl.handler);
                    break 'search;
                }
            }
            i += 1;
        }
        let Some((n, next)) = step(&c, &f) else { unhandled(obj, &first) };
        c = n;
        from = next;
        depth += 1;
        if depth > 100_000 {
            unhandled(obj, &first);
        }
    }
    // pass 2: run the finally / fault blocks between, then the catch
    let (target_depth, target_clause, handler) = found;
    let mut c = first;
    let mut from = 0usize;
    let mut depth = 0usize;
    loop {
        let f = find_fde(c.pc - 1).unwrap_or_else(|| unhandled(obj, &first));
        let m = method_of(&f);
        let last = if depth == target_depth { target_clause } else { usize::MAX };
        let mut i = from;
        while i < last {
            let Some(cl) = clause(&m, i) else { break };
            if cl.kind == 1 && in_try(&cl, &m, c.pc) {
                let r = Running { parent: c, clause: i, obj, kind: 1 };
                bz_call_funclet(cl.handler, &c, obj, &r);
            }
            i += 1;
        }
        if depth == target_depth {
            let r = Running { parent: c, clause: target_clause, obj, kind: 0 };
            let resume = bz_call_funclet(handler, &c, obj, &r);
            bz_eh_resume(&c, resume);
        }
        let (n, next) = step(&c, &f).unwrap_or_else(|| unhandled(obj, &first));
        c = n;
        from = next;
        depth += 1;
    }
}

/// `throw;` inside a catch block: the exception of the catch funclet that is
/// running, thrown again from where `throw;` is.
#[no_mangle]
pub unsafe extern "C" fn bz_eh_rethrow(ctx: *const Ctx) -> ! {
    let mut c = *ctx;
    for _ in 0..100_000 {
        let Some(f) = find_fde(c.pc - 1) else { break };
        let Some(n) = unwind(&c, &f, c.pc - 1) else { break };
        if n.pc == core::ptr::addr_of!(bz_funclet_return) as usize {
            let r = &*(n.regs[arch::SP] as *const *const Running).read();
            bz_eh_throw(r.obj, ctx);
        }
        c = n;
    }
    fatal_msg(b"throw; outside a catch block")
}

#[inline(never)]
unsafe fn fatal_msg(m: &[u8]) -> ! {
    let here = 0u8;
    let sp = core::hint::black_box(&here) as *const u8 as usize;
    fatal_at(m, fatal_msg as *const () as usize, sp)
}

/// End the program with `Unhandled exception: <m>`, reported at `pc` / `sp`.
unsafe fn fatal_at(m: &[u8], pc: usize, sp: usize) -> ! {
    let mut text = [0u8; 256];
    let out = text.as_mut_ptr();
    let p = b"Unhandled exception: ";
    core::ptr::copy_nonoverlapping(p.as_ptr(), out, p.len());
    let n = m.len().min(200);
    core::ptr::copy_nonoverlapping(m.as_ptr(), out.add(p.len()), n);
    bz_write(out, (p.len() + n) as u64);
    bz_write(b"\n".as_ptr(), 1);
    crate::crash_report(out, pc, sp);
    SystemNative_Abort() // (only if the kernel refused the report)
}

extern "C" {
    fn bz_throw_null_reference() -> !;
    fn bz_throw_divide_by_zero() -> !;
    fn bz_throw_index_out_of_range() -> !;
    fn bz_throw_invalid_cast() -> !;
    fn bz_throw_array_type_mismatch() -> !;
}

/// Where the program went wrong: the throwing frame, or its caller when that
/// frame is one of the runtime's throwers (a redirected fault "called" it, so
/// the stack scan of a report could not find the faulting method by itself).
unsafe fn origin(first: &Ctx) -> (usize, usize) {
    let mut c = *first;
    if let Some(f) = find_fde(c.pc - 1) {
        let s = f.start;
        if s == bz_throw_null_reference as *const () as usize
            || s == bz_throw_divide_by_zero as *const () as usize
            || s == bz_throw_index_out_of_range as *const () as usize
            || s == bz_throw_invalid_cast as *const () as usize
            || s == bz_throw_array_type_mismatch as *const () as usize
        {
            if let Some(n) = unwind(&c, &f, c.pc - 1) {
                c = n;
            }
        }
    }
    // back from the return address into the call (or onto the faulting load)
    #[cfg(target_arch = "x86_64")]
    let pc = c.pc - 1;
    #[cfg(target_arch = "aarch64")]
    let pc = c.pc - 4;
    (pc, *c.regs.as_ptr().add(arch::SP))
}

/// No clause takes `obj`: report it (its Message when it has one) and end
/// the program.
unsafe fn unhandled(obj: *const u8, first: &Ctx) -> ! {
    let mut buf = [0u8; 200];
    let mut n = 0;
    // System.Exception's first field is the Message string (bzexc.cs)
    let msg = *(obj.add(8) as *const *const u8);
    if !msg.is_null() && is_string(msg) {
        let len = (*(msg.add(8) as *const u32) as usize).min(180);
        for i in 0..len {
            let ch = *(msg.add(12 + 2 * i) as *const u16);
            *buf.as_mut_ptr().add(n) = if (0x20..0x7f).contains(&ch) { ch as u8 } else { b'?' };
            n += 1;
        }
    }
    if n == 0 {
        let s = b"(an exception without a message)";
        core::ptr::copy_nonoverlapping(s.as_ptr(), buf.as_mut_ptr(), s.len());
        n = s.len();
    }
    let (pc, sp) = origin(first);
    fatal_at(core::slice::from_raw_parts(buf.as_ptr(), n), pc, sp)
}

/// Does `p` look like a System.String (component size 2, no references)?
unsafe fn is_string(p: *const u8) -> bool {
    let mt = *(p as *const *const MethodTable);
    if mt.is_null() || (mt as usize) < 0x40_0000 {
        return false;
    }
    (*mt).flags & crate::bzgc::MT_HAS_COMPONENT_SIZE != 0 && (*mt).component_size == 2 && (*mt).flags & crate::MT_HAS_POINTERS == 0
}

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    // Ctx = pc, then regs[0..33] by DWARF number (8 bytes each): rbx = regs[3]
    // at +32, rbp = regs[6] at +56, rsp = regs[7] at +64, r12..r15 = regs[12..15]
    // at +104..+128. 272 bytes.
    ".global RhpThrowEx",
    "RhpThrowEx:",
    "    mov rax, rsp",
    "    sub rsp, 280",              // entry rsp = 8 mod 16 -> 0 mod 16
    "    mov rcx, [rax]",
    "    mov [rsp], rcx",            // pc = return address into the thrower
    "    lea rcx, [rax + 8]",
    "    mov [rsp + 64], rcx",       // its stack pointer after the call
    "    mov [rsp + 32], rbx",
    "    mov [rsp + 56], rbp",
    "    mov [rsp + 104], r12",
    "    mov [rsp + 112], r13",
    "    mov [rsp + 120], r14",
    "    mov [rsp + 128], r15",
    "    mov rsi, rsp",
    "    call {throw}",
    "    int3",
    ".global RhpRethrow",
    "RhpRethrow:",
    "    mov rax, rsp",
    "    sub rsp, 280",
    "    mov rcx, [rax]",
    "    mov [rsp], rcx",
    "    lea rcx, [rax + 8]",
    "    mov [rsp + 64], rcx",
    "    mov [rsp + 32], rbx",
    "    mov [rsp + 56], rbp",
    "    mov [rsp + 104], r12",
    "    mov [rsp + 112], r13",
    "    mov [rsp + 120], r14",
    "    mov [rsp + 128], r15",
    "    mov rdi, rsp",
    "    call {rethrow}",
    "    int3",
    // bz_call_funclet(handler, ctx, obj, running) -> rax
    ".global bz_call_funclet",
    ".global bz_funclet_return",
    "bz_call_funclet:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    push rcx",                  // the Running record, just above the return address
    "    mov rax, rdi",
    "    mov rbx, [rsi + 32]",
    "    mov rbp, [rsi + 56]",
    "    mov r12, [rsi + 104]",
    "    mov r13, [rsi + 112]",
    "    mov r14, [rsi + 120]",
    "    mov r15, [rsi + 128]",
    "    mov rsi, rdx",
    "    mov rdi, rdx",
    "    call rax",
    "bz_funclet_return:",
    "    pop rcx",
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    ret",
    // bz_eh_resume(ctx, pc): the method's registers and stack, then jump
    ".global bz_eh_resume",
    "bz_eh_resume:",
    "    mov rbx, [rdi + 32]",
    "    mov rbp, [rdi + 56]",
    "    mov r12, [rdi + 104]",
    "    mov r13, [rdi + 112]",
    "    mov r14, [rdi + 120]",
    "    mov r15, [rdi + 128]",
    "    mov rsp, [rdi + 64]",
    "    jmp rsi",
    throw = sym bz_eh_throw,
    rethrow = sym bz_eh_rethrow,
);

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    // Ctx: pc at 0, regs[n] at 8 + 8n: x19 at 160, ..., x29 at 240, x30 at
    // 248, sp at 256. 272 bytes.
    ".global RhpThrowEx",
    "RhpThrowEx:",
    "    sub sp, sp, #288",
    "    str x30, [sp]",             // pc = return address into the thrower
    "    add x9, sp, #288",
    "    str x9, [sp, #256]",        // its stack pointer
    "    stp x19, x20, [sp, #160]",
    "    stp x21, x22, [sp, #176]",
    "    stp x23, x24, [sp, #192]",
    "    stp x25, x26, [sp, #208]",
    "    stp x27, x28, [sp, #224]",
    "    stp x29, x30, [sp, #240]",
    "    mov x1, sp",
    "    bl {throw}",
    "    brk #0",
    ".global RhpRethrow",
    "RhpRethrow:",
    "    sub sp, sp, #288",
    "    str x30, [sp]",
    "    add x9, sp, #288",
    "    str x9, [sp, #256]",
    "    stp x19, x20, [sp, #160]",
    "    stp x21, x22, [sp, #176]",
    "    stp x23, x24, [sp, #192]",
    "    stp x25, x26, [sp, #208]",
    "    stp x27, x28, [sp, #224]",
    "    stp x29, x30, [sp, #240]",
    "    mov x0, sp",
    "    bl {rethrow}",
    "    brk #0",
    ".global bz_call_funclet",
    ".global bz_funclet_return",
    "bz_call_funclet:",
    "    stp x29, x30, [sp, #-112]!",
    "    stp x19, x20, [sp, #16]",
    "    stp x21, x22, [sp, #32]",
    "    stp x23, x24, [sp, #48]",
    "    stp x25, x26, [sp, #64]",
    "    stp x27, x28, [sp, #80]",
    "    str x3, [sp, #96]",
    "    mov x9, x0",
    "    ldp x19, x20, [x1, #160]",
    "    ldp x21, x22, [x1, #176]",
    "    ldp x23, x24, [x1, #192]",
    "    ldp x25, x26, [x1, #208]",
    "    ldp x27, x28, [x1, #224]",
    "    ldr x29, [x1, #240]",
    "    mov x0, x2",
    "    mov x1, x2",
    // the record must sit at the funclet's CFA: put it just below sp
    "    sub sp, sp, #16",
    "    str x3, [sp]",
    "    blr x9",
    "bz_funclet_return:",
    "    add sp, sp, #16",
    "    ldp x19, x20, [sp, #16]",
    "    ldp x21, x22, [sp, #32]",
    "    ldp x23, x24, [sp, #48]",
    "    ldp x25, x26, [sp, #64]",
    "    ldp x27, x28, [sp, #80]",
    "    ldp x29, x30, [sp], #112",
    "    ret",
    ".global bz_eh_resume",
    "bz_eh_resume:",
    "    ldp x19, x20, [x0, #160]",
    "    ldp x21, x22, [x0, #176]",
    "    ldp x23, x24, [x0, #192]",
    "    ldp x25, x26, [x0, #208]",
    "    ldp x27, x28, [x0, #224]",
    "    ldp x29, x30, [x0, #240]",
    "    ldr x9, [x0, #256]",
    "    mov sp, x9",
    "    br x1",
    throw = sym bz_eh_throw,
    rethrow = sym bz_eh_rethrow,
);

// the asm above hard-codes this layout
const _: () = assert!(core::mem::size_of::<Ctx>() == 272);
