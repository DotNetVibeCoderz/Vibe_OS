//! Virtualization Manager + software virtual machine (v0.13 "Lapis").
//!
//! requirements.md §9 calls for a type-2 hypervisor, a Virtualization Manager,
//! paravirtualized (virtio) devices, virtual disks, and snapshots. Real
//! VT-x/AMD-V isn't exposed under nested QEMU/TCG (see [`crate::vmx`]), so the
//! executing core here is a compact **software virtual CPU** ("BZVM") — a
//! genuine fetch/decode/execute interpreter with its own registers, linear
//! guest memory, a stack, virtio-style I/O ports, a RAM-backed virtual disk,
//! and full state snapshot/restore. A tiny guest ("NanoOS") is assembled and
//! runs on it, so Buitenzorg literally executes another OS as a VM.
//!
//! This mirrors the compute layer's GPU→CPU fallback: when hardware
//! virtualization lands (a real VMX/EPT driver), the manager gains a hardware
//! backend and the software VMM remains the portable fallback.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

// --- BZVM instruction set ----------------------------------------------------
// Fixed 8-byte encoding: [op, rd, rs, pad, imm:u32-le].

const NOP: u8 = 0x00;
const MOVI: u8 = 0x01; // rd = imm
const MOV: u8 = 0x02; // rd = r[rs]
const ADD: u8 = 0x03; // rd += r[rs]
const SUB: u8 = 0x04; // rd -= r[rs]
const MUL: u8 = 0x05; // rd *= r[rs]
const ADDI: u8 = 0x06; // rd += imm
const AND: u8 = 0x07;
const OR: u8 = 0x08;
const XOR: u8 = 0x09;
const CMP: u8 = 0x0A; // zf = (r[rd] == r[rs])
const CMPI: u8 = 0x0B; // zf = (r[rd] == imm)
const JMP: u8 = 0x0C;
const JZ: u8 = 0x0D;
const JNZ: u8 = 0x0E;
const LOADB: u8 = 0x0F; // rd = mem[r[rs] + imm]
const STOREB: u8 = 0x10; // mem[r[rd] + imm] = r[rs]
const OUT: u8 = 0x11; // port(imm) <- r[rs]
const IN: u8 = 0x12; // rd <- port(imm)
const PUSH: u8 = 0x13;
const POP: u8 = 0x14;
const CALL: u8 = 0x15;
const RET: u8 = 0x16;
const HLT: u8 = 0xFF;

// virtio-style paravirtual device ports.
const PORT_CONSOLE_CHAR: u32 = 0; // append a byte as a character
const PORT_CONSOLE_NUM: u32 = 1; // append a u32 formatted as decimal
const PORT_DISK_WRITE: u32 = 2; // append a byte to the virtual disk
const PORT_HOST_TICK: u32 = 3; // read the host timer (guest-tools integration)

const DATA_BASE: u32 = 0x600; // guest strings live here (code stays below)

// --- Tiny two-pass assembler for the guest image -----------------------------

enum Tok {
    Label(&'static str),
    Ins { op: u8, rd: u8, rs: u8, imm: u32, tgt: Option<&'static str> },
}

fn i(op: u8, rd: u8, rs: u8, imm: u32) -> Tok {
    Tok::Ins { op, rd, rs, imm, tgt: None }
}
fn j(op: u8, tgt: &'static str) -> Tok {
    Tok::Ins { op, rd: 0, rs: 0, imm: 0, tgt: Some(tgt) }
}
fn lbl(n: &'static str) -> Tok {
    Tok::Label(n)
}

fn assemble(toks: &[Tok]) -> Vec<u8> {
    let mut labels: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut addr = 0u32;
    for t in toks {
        match t {
            Tok::Label(n) => {
                labels.insert(n, addr);
            }
            Tok::Ins { .. } => addr += 8,
        }
    }
    let mut out = Vec::new();
    for t in toks {
        if let Tok::Ins { op, rd, rs, imm, tgt } = t {
            let imm = match tgt {
                Some(n) => *labels.get(n).unwrap_or(&0),
                None => *imm,
            };
            out.push(*op);
            out.push(*rd);
            out.push(*rs);
            out.push(0);
            out.extend_from_slice(&imm.to_le_bytes());
        }
    }
    out
}

/// Build the guest's data blob (NUL-terminated strings) and return the blob
/// plus each string's absolute guest address.
fn guest_data() -> (Vec<u8>, [u32; 4]) {
    let strings: [&str; 4] = [
        "NanoOS v0.1 (guest) boot di atas Buitenzorg VMM\n",
        "  hitung 1..10 = ",
        "  host tick (guest tools) = ",
        "NanoOS halted. sampai jumpa.\n",
    ];
    let mut blob = Vec::new();
    let mut addrs = [0u32; 4];
    for (k, s) in strings.iter().enumerate() {
        addrs[k] = DATA_BASE + blob.len() as u32;
        blob.extend_from_slice(s.as_bytes());
        blob.push(0);
    }
    (blob, addrs)
}

/// Assemble the NanoOS guest: a real program that prints via the virtio
/// console, computes sum(1..=10), reads the host tick through guest tools, then
/// halts. Returns (code, data_blob).
fn build_nano_guest() -> (Vec<u8>, Vec<u8>) {
    let (blob, a) = guest_data();
    let (a_banner, a_sum, a_tick, a_halt) = (a[0], a[1], a[2], a[3]);

    // Registers: r0 scratch, r1 string ptr, r2 char, r5 accumulator/value,
    // r6 loop counter.
    let prog = [
        // print banner
        i(MOVI, 1, 0, a_banner),
        j(CALL, "print_str"),
        // "  hitung 1..10 = "
        i(MOVI, 1, 0, a_sum),
        j(CALL, "print_str"),
        i(MOVI, 5, 0, 0), // acc = 0
        i(MOVI, 6, 0, 1), // i = 1
        lbl("loop_sum"),
        i(CMPI, 6, 0, 11), // i == 11?
        j(JZ, "done_sum"),
        i(ADD, 5, 6, 0), // acc += i
        i(ADDI, 6, 0, 1), // i += 1
        j(JMP, "loop_sum"),
        lbl("done_sum"),
        i(OUT, 0, 5, PORT_CONSOLE_NUM), // print acc (=55)
        i(MOVI, 0, 0, 10),
        i(OUT, 0, 0, PORT_CONSOLE_CHAR), // '\n'
        // host tick via guest tools
        i(MOVI, 1, 0, a_tick),
        j(CALL, "print_str"),
        i(IN, 5, 0, PORT_HOST_TICK), // r5 = host tick
        i(OUT, 0, 5, PORT_CONSOLE_NUM),
        i(MOVI, 0, 0, 10),
        i(OUT, 0, 0, PORT_CONSOLE_CHAR),
        // farewell + halt
        i(MOVI, 1, 0, a_halt),
        j(CALL, "print_str"),
        i(HLT, 0, 0, 0),
        // subroutine: print NUL-terminated string at r1 (clobbers r2)
        lbl("print_str"),
        lbl("ps_loop"),
        i(LOADB, 2, 1, 0), // r2 = mem[r1]
        i(CMPI, 2, 0, 0),
        j(JZ, "ps_end"),
        i(OUT, 0, 2, PORT_CONSOLE_CHAR),
        i(ADDI, 1, 0, 1),
        j(JMP, "ps_loop"),
        lbl("ps_end"),
        i(RET, 0, 0, 0),
    ];
    (assemble(&prog), blob)
}

// --- Virtual machine ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Created,
    Halted,
    Faulted,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Created => "created",
            State::Halted => "halted",
            State::Faulted => "faulted",
        }
    }
}

struct Snapshot {
    mem: Vec<u8>,
    regs: [u32; 8],
    sp: u32,
    pc: u32,
    zf: bool,
    disk: Vec<u8>,
    console: String,
    steps: u64,
    state: State,
}

pub struct Vm {
    pub id: u32,
    pub name: String,
    pub mem_kib: u32,
    pub vcpus: u32,
    pub disk_kib: u32,
    pub state: State,
    mem: Vec<u8>,
    regs: [u32; 8],
    sp: u32,
    pc: u32,
    zf: bool,
    disk: Vec<u8>,
    console: String,
    steps: u64,
    snap: Option<Box<Snapshot>>,
}

const MAX_STEPS: u64 = 100_000; // guest can't hang the boot

impl Vm {
    fn reset_and_load(&mut self) {
        let (code, data) = build_nano_guest();
        for b in self.mem.iter_mut() {
            *b = 0;
        }
        self.mem[..code.len()].copy_from_slice(&code);
        let dstart = DATA_BASE as usize;
        self.mem[dstart..dstart + data.len()].copy_from_slice(&data);
        self.regs = [0; 8];
        self.sp = self.mem.len() as u32; // stack grows down from top
        self.pc = 0;
        self.zf = false;
        self.console.clear();
        self.steps = 0;
        self.state = State::Created;
    }

    fn rd_u32(&self, addr: u32) -> Option<u32> {
        let a = addr as usize;
        if a + 4 <= self.mem.len() {
            Some(u32::from_le_bytes(self.mem[a..a + 4].try_into().unwrap()))
        } else {
            None
        }
    }
    fn wr_u32(&mut self, addr: u32, v: u32) -> bool {
        let a = addr as usize;
        if a + 4 <= self.mem.len() {
            self.mem[a..a + 4].copy_from_slice(&v.to_le_bytes());
            true
        } else {
            false
        }
    }

    fn device_out(&mut self, port: u32, val: u32) {
        match port {
            PORT_CONSOLE_CHAR => self.console.push((val as u8) as char),
            PORT_CONSOLE_NUM => self.console.push_str(&val.to_string()),
            PORT_DISK_WRITE => self.disk.push(val as u8),
            _ => {}
        }
    }
    fn device_in(&self, port: u32) -> u32 {
        match port {
            PORT_HOST_TICK => crate::interrupts::ticks() as u32,
            _ => 0,
        }
    }

    /// Run the virtual CPU until HLT, a fault, or the step budget.
    fn run(&mut self) {
        loop {
            if self.steps >= MAX_STEPS {
                self.state = State::Faulted;
                return;
            }
            let pc = self.pc as usize;
            if pc + 8 > self.mem.len() {
                self.state = State::Faulted;
                return;
            }
            let op = self.mem[pc];
            let rd = self.mem[pc + 1] as usize & 7;
            let rs = self.mem[pc + 2] as usize & 7;
            let imm = u32::from_le_bytes(self.mem[pc + 4..pc + 8].try_into().unwrap());
            self.pc += 8;
            self.steps += 1;

            match op {
                NOP => {}
                MOVI => self.regs[rd] = imm,
                MOV => self.regs[rd] = self.regs[rs],
                ADD => self.regs[rd] = self.regs[rd].wrapping_add(self.regs[rs]),
                SUB => self.regs[rd] = self.regs[rd].wrapping_sub(self.regs[rs]),
                MUL => self.regs[rd] = self.regs[rd].wrapping_mul(self.regs[rs]),
                ADDI => self.regs[rd] = self.regs[rd].wrapping_add(imm),
                AND => self.regs[rd] &= self.regs[rs],
                OR => self.regs[rd] |= self.regs[rs],
                XOR => self.regs[rd] ^= self.regs[rs],
                CMP => self.zf = self.regs[rd] == self.regs[rs],
                CMPI => self.zf = self.regs[rd] == imm,
                JMP => self.pc = imm,
                JZ => {
                    if self.zf {
                        self.pc = imm;
                    }
                }
                JNZ => {
                    if !self.zf {
                        self.pc = imm;
                    }
                }
                LOADB => {
                    let a = self.regs[rs].wrapping_add(imm) as usize;
                    if a < self.mem.len() {
                        self.regs[rd] = self.mem[a] as u32;
                    } else {
                        self.state = State::Faulted;
                        return;
                    }
                }
                STOREB => {
                    let a = self.regs[rd].wrapping_add(imm) as usize;
                    if a < self.mem.len() {
                        self.mem[a] = self.regs[rs] as u8;
                    } else {
                        self.state = State::Faulted;
                        return;
                    }
                }
                OUT => {
                    let v = self.regs[rs];
                    self.device_out(imm, v);
                }
                IN => self.regs[rd] = self.device_in(imm),
                PUSH => {
                    self.sp = self.sp.wrapping_sub(4);
                    if !self.wr_u32(self.sp, self.regs[rd]) {
                        self.state = State::Faulted;
                        return;
                    }
                }
                POP => {
                    match self.rd_u32(self.sp) {
                        Some(v) => {
                            self.regs[rd] = v;
                            self.sp = self.sp.wrapping_add(4);
                        }
                        None => {
                            self.state = State::Faulted;
                            return;
                        }
                    }
                }
                CALL => {
                    self.sp = self.sp.wrapping_sub(4);
                    if !self.wr_u32(self.sp, self.pc) {
                        self.state = State::Faulted;
                        return;
                    }
                    self.pc = imm;
                }
                RET => match self.rd_u32(self.sp) {
                    Some(v) => {
                        self.pc = v;
                        self.sp = self.sp.wrapping_add(4);
                    }
                    None => {
                        self.state = State::Faulted;
                        return;
                    }
                },
                HLT => {
                    self.state = State::Halted;
                    return;
                }
                _ => {
                    self.state = State::Faulted;
                    return;
                }
            }
        }
    }

    fn take_snapshot(&mut self) {
        self.snap = Some(Box::new(Snapshot {
            mem: self.mem.clone(),
            regs: self.regs,
            sp: self.sp,
            pc: self.pc,
            zf: self.zf,
            disk: self.disk.clone(),
            console: self.console.clone(),
            steps: self.steps,
            state: self.state,
        }));
    }
    fn restore_snapshot(&mut self) -> bool {
        if let Some(s) = &self.snap {
            self.mem.clone_from(&s.mem);
            self.regs = s.regs;
            self.sp = s.sp;
            self.pc = s.pc;
            self.zf = s.zf;
            self.disk.clone_from(&s.disk);
            self.console.clone_from(&s.console);
            self.steps = s.steps;
            self.state = s.state;
            true
        } else {
            false
        }
    }
}

// --- Virtualization Manager (registry) ---------------------------------------

static VMS: Mutex<Vec<Vm>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Public, lock-free-to-read summary of a VM.
pub struct VmInfo {
    pub id: u32,
    pub name: String,
    pub mem_kib: u32,
    pub vcpus: u32,
    pub disk_kib: u32,
    pub state: State,
    pub steps: u64,
    pub has_snapshot: bool,
}

/// Create a VM preloaded with the NanoOS guest. Returns its id.
pub fn create(name: &str, mem_kib: u32, vcpus: u32) -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let mem_kib = mem_kib.clamp(4, 1024);
    let disk_kib = 64;
    let mut vm = Vm {
        id,
        name: name.to_string(),
        mem_kib,
        vcpus: vcpus.clamp(1, 4),
        disk_kib,
        state: State::Created,
        mem: vec![0u8; (mem_kib as usize) * 1024],
        regs: [0; 8],
        sp: 0,
        pc: 0,
        zf: false,
        disk: Vec::new(),
        console: String::new(),
        steps: 0,
        snap: None,
    };
    vm.reset_and_load();
    VMS.lock().push(vm);
    id
}

fn find_index(sel: &str) -> Option<usize> {
    let vms = VMS.lock();
    if let Ok(id) = sel.parse::<u32>() {
        if let Some(p) = vms.iter().position(|v| v.id == id) {
            return Some(p);
        }
    }
    vms.iter().position(|v| v.name == sel)
}

pub fn list() -> Vec<VmInfo> {
    VMS.lock()
        .iter()
        .map(|v| VmInfo {
            id: v.id,
            name: v.name.clone(),
            mem_kib: v.mem_kib,
            vcpus: v.vcpus,
            disk_kib: v.disk_kib,
            state: v.state,
            steps: v.steps,
            has_snapshot: v.snap.is_some(),
        })
        .collect()
}

/// Result of running a VM's guest.
pub struct RunResult {
    pub name: String,
    pub state: State,
    pub steps: u64,
    pub console: String,
}

/// Start (run) a VM's guest CPU to completion (HLT/fault/budget).
pub fn start(sel: &str) -> Result<RunResult, String> {
    let idx = find_index(sel).ok_or_else(|| alloc::format!("VM '{}' tidak ditemukan", sel))?;
    let mut vms = VMS.lock();
    let vm = &mut vms[idx];
    vm.reset_and_load(); // fresh boot each start
    vm.run();
    Ok(RunResult {
        name: vm.name.clone(),
        state: vm.state,
        steps: vm.steps,
        console: vm.console.clone(),
    })
}

pub fn snapshot(sel: &str) -> Result<(), String> {
    let idx = find_index(sel).ok_or_else(|| alloc::format!("VM '{}' tidak ditemukan", sel))?;
    let mut vms = VMS.lock();
    vms[idx].take_snapshot();
    Ok(())
}

pub fn restore(sel: &str) -> Result<(), String> {
    let idx = find_index(sel).ok_or_else(|| alloc::format!("VM '{}' tidak ditemukan", sel))?;
    let mut vms = VMS.lock();
    if vms[idx].restore_snapshot() {
        Ok(())
    } else {
        Err(String::from("belum ada snapshot"))
    }
}

pub fn remove(sel: &str) -> Result<(), String> {
    let idx = find_index(sel).ok_or_else(|| alloc::format!("VM '{}' tidak ditemukan", sel))?;
    VMS.lock().remove(idx);
    Ok(())
}

/// A self-test used by the boot demo: create a VM, run the guest, snapshot,
/// mutate, restore, and verify. Returns (console_output, snapshot_ok).
pub fn selftest() -> (String, bool) {
    let _ = create("nanovm", 64, 1);
    let run = match start("nanovm") {
        Ok(r) => r,
        Err(_) => return (String::new(), false),
    };
    // Snapshot the (freshly booted) machine, corrupt it, then restore.
    let _ = snapshot("nanovm");
    let snapshot_ok = {
        let mut vms = VMS.lock();
        if let Some(vm) = vms.iter_mut().find(|v| v.name == "nanovm") {
            let before = vm.console.clone();
            vm.console.push_str("XXCORRUPTXX");
            vm.regs[3] = 0xDEAD_BEEF;
            vm.disk.push(0xFF);
            let restored = vm.restore_snapshot();
            restored && vm.console == before && vm.regs[3] != 0xDEAD_BEEF
        } else {
            false
        }
    };
    (run.console, snapshot_ok)
}
