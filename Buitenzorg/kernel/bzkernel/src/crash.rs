//! Crash reports for ring-3 programs: when a program faults, the kernel ends
//! it (as before) and now also writes a report -- the fault, the registers
//! that matter, and a **symbolized backtrace** -- to the log and to
//! `/ram/CRASH/P<pid>.TXT` (`/disk/CRASH/` where there is no `/ram`).
//!
//! * **Symbols** come from the program's own ELF: [`on_load`] keeps the
//!   function symbols of `.symtab` (and the executable segments) per pid when
//!   the image is loaded, so a report needs no file access in the fault path.
//!   A stripped program (CLRHOST.ELF) gets addresses only.
//! * **Backtrace**: native C# (NativeAOT) and Rust code is not guaranteed to
//!   keep frame pointers, so the walk scans the user stack for words that
//!   point into an executable segment *right after a call instruction*
//!   (x86 `call`, ARM64 `bl`/`blr`, RISC-V `jal`/`jalr` that link `ra`) --
//!   the return addresses of the active calls, plus the link register on
//!   ARM64 / RISC-V. A stale return address left in a dead stack slot can
//!   show up too; the report says the frames are found by a scan.
//!
//! Generic code: each architecture's fault path calls [`record`] with the
//! fault's pc, sp and (where there is one) link register, while the faulting
//! address space is still installed.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;
use spin::Mutex;

struct Syms {
    /// (address, size, name), sorted by address.
    funcs: Vec<(u64, u64, String)>,
    /// Executable segments [start, end).
    exec: Vec<(u64, u64)>,
}

static SYMS: Mutex<BTreeMap<u64, Arc<Syms>>> = Mutex::new(BTreeMap::new());
/// Reports not yet written to a file: (pid, text).
static PENDING: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());
/// The newest report (tests, `crash` in the shell before a flush).
static LAST: Mutex<Option<(u64, String)>> = Mutex::new(None);

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

fn parse(image: &[u8]) -> Option<Syms> {
    if image.get(0..4)? != b"\x7fELF" || *image.get(4)? != 2 {
        return None; // ELF64 only
    }
    let (phoff, phentsize, phnum) = (u64_at(image, 0x20)? as usize, u16_at(image, 0x36)? as usize, u16_at(image, 0x38)? as usize);
    let mut exec = Vec::new();
    for i in 0..phnum.min(64) {
        let p = phoff + i * phentsize;
        if u32_at(image, p)? == 1 && u32_at(image, p + 4)? & 1 != 0 {
            let (va, memsz) = (u64_at(image, p + 0x10)?, u64_at(image, p + 0x28)?);
            exec.push((va, va + memsz));
        }
    }
    let (shoff, shentsize, shnum) = (u64_at(image, 0x28)? as usize, u16_at(image, 0x3A)? as usize, u16_at(image, 0x3C)? as usize);
    let mut funcs = Vec::new();
    for i in 0..shnum.min(256) {
        let s = shoff + i * shentsize;
        if u32_at(image, s + 4)? != 2 {
            continue; // SHT_SYMTAB
        }
        let (off, size, link, entsize) =
            (u64_at(image, s + 0x18)? as usize, u64_at(image, s + 0x20)? as usize, u32_at(image, s + 0x28)? as usize, u64_at(image, s + 0x38)? as usize);
        let st = shoff + link * shentsize;
        let (stroff, strsize) = (u64_at(image, st + 0x18)? as usize, u64_at(image, st + 0x20)? as usize);
        let strtab = image.get(stroff..stroff + strsize)?;
        let entsize = if entsize == 0 { 24 } else { entsize };
        for k in 0..(size / entsize).min(200_000) {
            let e = off + k * entsize;
            let (name, info, shndx, value, sz) =
                (u32_at(image, e)? as usize, *image.get(e + 4)?, u16_at(image, e + 6)?, u64_at(image, e + 8)?, u64_at(image, e + 16)?);
            let kind = info & 0xF;
            if shndx == 0 || (kind != 2 && kind != 0) || value == 0 || !exec.iter().any(|&(a, b)| value >= a && value < b) {
                continue;
            }
            let end = strtab.get(name..).and_then(|r| r.iter().position(|&c| c == 0)).map(|n| name + n);
            let Some(raw) = end.and_then(|e| core::str::from_utf8(&strtab[name..e]).ok()) else { continue };
            if raw.is_empty() || raw.starts_with('$') || raw.starts_with(".L") {
                continue; // ARM mapping symbols, local labels
            }
            funcs.push((value, sz, demangle(raw)));
        }
    }
    funcs.sort_by_key(|f| f.0);
    funcs.dedup_by_key(|f| f.0);
    Some(Syms { funcs, exec })
}

/// Rust's v0 mangling, the plain paths of it: `_RNvCs<hash>_7crashme6level3`
/// -> `crashme::level3`. Anything with generics or impls stays as it is.
fn demangle_v0(s: &str) -> Option<String> {
    fn disambiguator(b: &[u8], i: &mut usize) {
        if b.get(*i) == Some(&b's') {
            while *i < b.len() && b[*i] != b'_' {
                *i += 1;
            }
            *i += 1;
        }
    }
    fn ident<'a>(b: &'a [u8], i: &mut usize) -> Option<&'a str> {
        disambiguator(b, i);
        if b.get(*i) == Some(&b'u') {
            return None; // punycode
        }
        let start = *i;
        while b.get(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        let n: usize = core::str::from_utf8(&b[start..*i]).ok()?.parse().ok()?;
        if b.get(*i) == Some(&b'_') {
            *i += 1;
        }
        let r = core::str::from_utf8(b.get(*i..*i + n)?).ok()?;
        *i += n;
        Some(r)
    }
    fn path(b: &[u8], i: &mut usize, out: &mut Vec<String>) -> Option<()> {
        match *b.get(*i)? {
            b'C' => {
                *i += 1;
                out.push(String::from(ident(b, i)?));
            }
            b'N' => {
                *i += 2; // 'N' + namespace letter
                path(b, i, out)?;
                let id = ident(b, i)?;
                if !id.is_empty() {
                    out.push(String::from(id));
                }
            }
            _ => return None,
        }
        Some(())
    }
    let b = s.strip_prefix("_R")?.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    path(b, &mut i, &mut out)?;
    Some(out.join("::"))
}

/// Rust's legacy mangling `_ZN3foo3barE` / `_ZN...17h<hash>E` -> `foo::bar`.
fn demangle(s: &str) -> String {
    if s.starts_with("_R") {
        return demangle_v0(s).unwrap_or_else(|| String::from(s));
    }
    let Some(mut rest) = s.strip_prefix("_ZN") else { return String::from(s) };
    let mut parts: Vec<&str> = Vec::new();
    while let Some(c) = rest.chars().next() {
        if c == 'E' {
            break;
        }
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        let Ok(n) = rest[..digits].parse::<usize>() else { return String::from(s) };
        let Some(part) = rest.get(digits..digits + n) else { return String::from(s) };
        parts.push(part);
        rest = &rest[digits + n..];
    }
    if parts.last().is_some_and(|p| p.len() == 17 && p.starts_with('h') && p[1..].chars().all(|c| c.is_ascii_hexdigit())) {
        parts.pop();
    }
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str("::");
        }
        out.push_str(&p.replace("$LT$", "<").replace("$GT$", ">").replace("$u20$", " ").replace("..", "::"));
    }
    out
}

/// Keep the function symbols of the program `pid` was just loaded from.
pub fn on_load(pid: u64, image: &[u8]) {
    if let Some(s) = parse(image) {
        SYMS.lock().insert(pid, Arc::new(s));
    }
}

/// The program ended: drop its symbols.
pub fn on_exit(pid: u64) {
    crate::appprof::on_exit(pid); // a profile's report needs the symbols
    SYMS.lock().remove(&pid);
}

/// `name+0xoff` for `addr`, if a symbol covers it.
fn symbolize(s: &Syms, addr: u64) -> Option<String> {
    let i = s.funcs.partition_point(|f| f.0 <= addr);
    let (start, size, name) = s.funcs.get(i.checked_sub(1)?)?;
    let next = s.funcs.get(i).map_or(u64::MAX, |f| f.0);
    if *size != 0 && addr >= start + size && addr >= next {
        return None;
    }
    Some(if addr == *start { name.clone() } else { alloc::format!("{}+{:#x}", name, addr - start) })
}

/// Read `N` bytes of user memory, if every byte is mapped for the user.
fn user_bytes<const N: usize>(addr: u64) -> Option<[u8; N]> {
    if !crate::memory::validate_user_range(addr, N as u64, false) {
        return None;
    }
    let mut b = [0u8; N];
    for (i, x) in b.iter_mut().enumerate() {
        *x = unsafe { core::ptr::read_volatile((addr + i as u64) as *const u8) };
    }
    Some(b)
}

/// Whether the instruction before return address `ra` is a call.
fn after_call(ra: u64) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let Some(b) = ra.checked_sub(6).and_then(user_bytes::<6>) else { return false };
        // call rel32 (E8 xx xx xx xx); call r/m: FF /2 with a 2, 3 or 6 byte encoding
        let call_rm = |op: u8, modrm: u8| op == 0xFF && (modrm >> 3) & 7 == 2;
        b[1] == 0xE8 || call_rm(b[4], b[5]) || call_rm(b[3], b[4]) || call_rm(b[0], b[1])
    }
    #[cfg(target_arch = "aarch64")]
    {
        let Some(b) = ra.checked_sub(4).and_then(user_bytes::<4>) else { return false };
        let w = u32::from_le_bytes(b);
        w & 0xFC00_0000 == 0x9400_0000 || w & 0xFFFF_FC1F == 0xD63F_0000 // bl / blr
    }
    #[cfg(target_arch = "riscv64")]
    {
        let links_ra = |w: u32| (w >> 7) & 0x1F == 1;
        if let Some(b) = ra.checked_sub(4).and_then(user_bytes::<4>) {
            let w = u32::from_le_bytes(b);
            if (w & 0x7F == 0x6F || w & 0x7F == 0x67) && links_ra(w) {
                return true; // jal ra / jalr ra
            }
        }
        if let Some(b) = ra.checked_sub(2).and_then(user_bytes::<2>) {
            let h = u16::from_le_bytes(b);
            return h & 0xF07F == 0x9002 && (h >> 7) & 0x1F != 0; // c.jalr
        }
        false
    }
}

/// `name+0xoff` for `addr` in process `pid`'s program.
pub fn symbol_of(pid: u64, addr: u64) -> Option<String> {
    let syms = SYMS.lock().get(&pid).cloned()?;
    symbolize(&syms, addr)
}

/// The address of function `name` in `pid`'s program: an exact name, else
/// the one symbol whose name contains it (`Level2` -> `crashme_Crash__Level2`).
pub fn find_symbol(pid: u64, name: &str) -> Result<u64, String> {
    let Some(syms) = SYMS.lock().get(&pid).cloned() else {
        return Err(alloc::format!("dbg: no symbols for pid {}", pid));
    };
    if let Some(f) = syms.funcs.iter().find(|f| f.2 == name) {
        return Ok(f.0);
    }
    let hits: Vec<&(u64, u64, String)> = syms.funcs.iter().filter(|f| f.2.contains(name)).collect();
    match hits.as_slice() {
        [one] => Ok(one.0),
        [] => Err(alloc::format!("dbg: no symbol '{}'", name)),
        many => Err(alloc::format!(
            "dbg: '{}' matches {} symbols: {}",
            name,
            many.len(),
            many.iter().take(4).map(|f| f.2.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// The backtrace lines of a program stopped at `pc` / `sp` (the current,
/// i.e. its own, address space must be installed).
pub fn backtrace(pid: u64, pc: u64, sp: u64, link: Option<u64>) -> Vec<String> {
    let syms = SYMS.lock().get(&pid).cloned();
    let at = |a: u64| syms.as_ref().and_then(|s| symbolize(s, a)).unwrap_or_else(|| String::from("?"));
    let Some(s) = &syms else {
        return alloc::vec![String::from("(no symbols: the program's ELF has no symbol table)")];
    };
    let in_code = |a: u64| s.exec.iter().any(|&(lo, hi)| a >= lo && a < hi);
    let mut frames: Vec<u64> = alloc::vec![pc];
    if let Some(l) = link.filter(|&l| in_code(l) && l != pc && after_call(l)) {
        frames.push(l);
    }
    // scan up to 4 KiB of stack for return addresses
    let mut p = sp & !7;
    for _ in 0..512 {
        if frames.len() >= 16 {
            break;
        }
        let Some(b) = user_bytes::<8>(p) else { break };
        let w = u64::from_le_bytes(b);
        if in_code(w) && frames.last() != Some(&w) && after_call(w) {
            frames.push(w);
        }
        p += 8;
    }
    let mut out = alloc::vec![String::from("backtrace (return addresses found on the stack, innermost first):")];
    for (i, f) in frames.iter().enumerate() {
        out.push(alloc::format!("  #{:<2} {:#x}  {}", i, f, at(*f)));
    }
    out
}

/// A ring-3 fault in process `pid`: build the report (log + pending file).
/// `what` names the fault, `addr` is the faulting data address where the
/// architecture reports one, `link` the link register (ARM64 x30 / RISC-V ra).
pub fn record(pid: u64, what: &str, pc: u64, sp: u64, addr: Option<u64>, link: Option<u64>) {
    let syms = SYMS.lock().get(&pid).cloned();
    let name = crate::process::name_of(pid).unwrap_or_default();
    let mut r = String::new();
    let _ = writeln!(r, "=== crash report: pid {} '{}' ===", pid, name);
    let _ = write!(r, "{}", what);
    if let Some(a) = addr {
        let _ = write!(r, ", address {:#x}", a);
    }
    let _ = writeln!(r);
    let at = |a: u64| syms.as_ref().and_then(|s| symbolize(s, a)).unwrap_or_else(|| String::from("?"));
    let _ = writeln!(r, "pc {:#x}  {}", pc, at(pc));
    let _ = writeln!(r, "sp {:#x}", sp);
    if let Some(l) = link {
        let _ = writeln!(r, "lr {:#x}  {}", l, at(l));
    }
    for line in backtrace(pid, pc, sp, link) {
        let _ = writeln!(r, "{}", line);
    }
    for line in r.lines() {
        crate::println!("[crash] {}", line);
    }
    *LAST.lock() = Some((pid, r.clone()));
    PENDING.lock().push((pid, r));
}

/// The boot's check of a deliberate crash (CRASHME.ELF): the program ended
/// with the fault exit code, the report names `funcs` (innermost first, all of
/// them) and was saved. Prints `MILESTONE: CRASH REPORT OK`.
pub fn boot_check(exit: Option<u64>, funcs: &[&str]) -> bool {
    flush();
    let report = last().map(|(_, t)| t).unwrap_or_default();
    let named = funcs.iter().all(|f| report.contains(f));
    let saved = crate::vfs::list(dir()).map_or(false, |l| l.iter().any(|n| n.starts_with('P')));
    let ok = exit == Some(0xDEAD) && named && saved;
    if ok {
        crate::println!(
            "[kernel] MILESTONE: CRASH REPORT OK (a ring-3 fault three calls deep: program ended, backtrace {} from the ELF's symbols, report saved in {})",
            funcs.join(" <- "),
            dir()
        );
    } else {
        crate::println!("[crash] boot check failed: exit {:?}, functions named {}, saved {}", exit, named, saved);
    }
    ok
}

/// The newest report (pid, text).
pub fn last() -> Option<(u64, String)> {
    LAST.lock().clone()
}

/// Where reports are kept: `/ram/CRASH`, else `/disk/CRASH`.
pub fn dir() -> &'static str {
    if crate::vfs::list("/ram").is_ok() {
        "/ram/CRASH"
    } else {
        "/disk/CRASH"
    }
}

/// Write pending reports to files (the desktop loop; never from the fault path).
pub fn flush() {
    let pending = core::mem::take(&mut *PENDING.lock());
    if pending.is_empty() {
        return;
    }
    let dir = dir();
    let _ = crate::vfs::mkdir(dir);
    for (pid, text) in pending {
        let path = alloc::format!("{}/P{}.TXT", dir, pid % 10_000_000);
        match crate::vfs::write(&path, text.as_bytes()) {
            Ok(()) => crate::println!("[crash] report saved: {}", path),
            Err(e) => crate::println!("[crash] could not save {}: {}", path, e),
        }
    }
}
