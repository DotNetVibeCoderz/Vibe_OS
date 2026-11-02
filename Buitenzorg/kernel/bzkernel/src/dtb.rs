//! Flattened Device Tree (FDT / DTB) parser — platform discovery for boards
//! without ACPI (multi-arch M1). ARM64 and RISC-V machines, QEMU `virt`
//! included, describe their RAM, UART, interrupt controller, timer, CPUs and
//! virtio-mmio slots in a DTB the firmware/loader hands over (Limine and UEFI
//! pass it on; on PCs ACPI plays this role, see `power.rs`).
//!
//! Format (devicetree-specification v0.4, ch. 5): a 40-byte big-endian header,
//! a memory-reservation block, a structure block of tokens (BEGIN_NODE name,
//! PROP len/nameoff/value, END_NODE, NOP, END) and a strings block holding
//! property names. A node's properties precede its children, so the
//! `#address-cells` / `#size-cells` that shape the children's `reg` are known
//! before the children appear. Every read is bounds-checked: a malformed or
//! truncated blob yields `Err`, never a panic or an out-of-range read.

use alloc::vec::Vec;

const MAGIC: u32 = 0xD00D_FEED;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;
const END: u32 = 9;

#[derive(Clone, Copy)]
pub struct Prop<'a> {
    pub name: &'a str,
    pub value: &'a [u8],
}

pub struct Node<'a> {
    /// Unit name, e.g. `pl011@9000000` (empty for the root).
    pub name: &'a str,
    pub parent: Option<usize>,
    pub props: Vec<Prop<'a>>,
    /// Cells this node's *children* use in `reg` (defaults 2 / 1).
    pub addr_cells: u32,
    pub size_cells: u32,
}

impl<'a> Node<'a> {
    pub fn prop(&self, name: &str) -> Option<&'a [u8]> {
        self.props.iter().find(|p| p.name == name).map(|p| p.value)
    }

    /// Strings of a string-list property (`compatible`).
    pub fn strings(&self, name: &str) -> impl Iterator<Item = &'a str> {
        self.prop(name)
            .unwrap_or(&[])
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .filter_map(|s| core::str::from_utf8(s).ok())
    }

    pub fn is_compatible(&self, c: &str) -> bool {
        self.strings("compatible").any(|s| s == c)
    }

    pub fn u32_prop(&self, name: &str) -> Option<u32> {
        let v = self.prop(name)?;
        (v.len() >= 4).then(|| be32(v, 0))
    }
}

pub struct Fdt<'a> {
    pub nodes: Vec<Node<'a>>,
    pub boot_cpu: u32,
}

fn be32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn read32(b: &[u8], off: usize) -> Result<u32, &'static str> {
    if off.checked_add(4).map_or(true, |e| e > b.len()) {
        return Err("dtb: read past the end");
    }
    Ok(be32(b, off))
}

fn cstr(b: &[u8], off: usize) -> Result<&str, &'static str> {
    let rest = b.get(off..).ok_or("dtb: string offset out of range")?;
    let len = rest.iter().position(|&c| c == 0).ok_or("dtb: unterminated string")?;
    core::str::from_utf8(&rest[..len]).map_err(|_| "dtb: string not UTF-8")
}

impl<'a> Fdt<'a> {
    /// Parse a DTB held in `blob` (which may be longer than the tree).
    pub fn parse(blob: &'a [u8]) -> Result<Fdt<'a>, &'static str> {
        if read32(blob, 0)? != MAGIC {
            return Err("dtb: bad magic");
        }
        let total = read32(blob, 4)? as usize;
        if total < 40 || total > blob.len() {
            return Err("dtb: totalsize exceeds the blob");
        }
        let b = &blob[..total];
        let off_struct = read32(b, 8)? as usize;
        let off_strings = read32(b, 12)? as usize;
        let version = read32(b, 20)?;
        let last_comp = read32(b, 24)?;
        let boot_cpu = read32(b, 28)?;
        let size_strings = read32(b, 32)? as usize;
        let size_struct = read32(b, 36)? as usize;
        if version < 16 || last_comp > 17 {
            return Err("dtb: unsupported version");
        }
        let strings = b
            .get(off_strings..off_strings.checked_add(size_strings).ok_or("dtb: overflow")?)
            .ok_or("dtb: strings block out of range")?;
        let st = b
            .get(off_struct..off_struct.checked_add(size_struct).ok_or("dtb: overflow")?)
            .ok_or("dtb: structure block out of range")?;

        let mut nodes: Vec<Node<'a>> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut p = 0usize;
        loop {
            let tok = read32(st, p)?;
            p += 4;
            match tok {
                BEGIN_NODE => {
                    let name = cstr(st, p)?;
                    p = (p + name.len() + 1 + 3) & !3;
                    let parent = stack.last().copied();
                    nodes.push(Node {
                        name,
                        parent,
                        props: Vec::new(),
                        addr_cells: 2,
                        size_cells: 1,
                    });
                    stack.push(nodes.len() - 1);
                    if stack.len() > 64 {
                        return Err("dtb: nesting too deep");
                    }
                }
                END_NODE => {
                    stack.pop().ok_or("dtb: unbalanced END_NODE")?;
                }
                PROP => {
                    let len = read32(st, p)? as usize;
                    let nameoff = read32(st, p + 4)? as usize;
                    p += 8;
                    let value = st.get(p..p.checked_add(len).ok_or("dtb: overflow")?).ok_or("dtb: property past the end")?;
                    p = (p + len + 3) & !3;
                    let name = cstr(strings, nameoff)?;
                    let &cur = stack.last().ok_or("dtb: property outside a node")?;
                    let node = &mut nodes[cur];
                    if name == "#address-cells" && value.len() >= 4 {
                        node.addr_cells = be32(value, 0);
                    } else if name == "#size-cells" && value.len() >= 4 {
                        node.size_cells = be32(value, 0);
                    }
                    node.props.push(Prop { name, value });
                }
                NOP => {}
                END => break,
                _ => return Err("dtb: bad token"),
            }
        }
        if !stack.is_empty() {
            return Err("dtb: unterminated node");
        }
        if nodes.is_empty() {
            return Err("dtb: no root node");
        }
        Ok(Fdt { nodes, boot_cpu })
    }

    /// Parse the DTB at physical address `phys` (as handed over by a loader),
    /// read through the physical-memory window.
    ///
    /// # Safety
    /// `phys` must point at a DTB in mapped, unchanging memory.
    pub unsafe fn from_phys(phys: u64) -> Result<Fdt<'static>, &'static str> {
        let base = (crate::boot::info().phys_offset + phys) as *const u8;
        let head = unsafe { core::slice::from_raw_parts(base, 8) };
        if be32(head, 0) != MAGIC {
            return Err("dtb: bad magic");
        }
        let total = be32(head, 4) as usize;
        if total > 16 * 1024 * 1024 {
            return Err("dtb: implausible size");
        }
        Fdt::parse(unsafe { core::slice::from_raw_parts(base, total) })
    }

    pub fn root(&self) -> &Node<'a> {
        &self.nodes[0]
    }

    /// Full path of node `i`, e.g. `/soc/serial@10000000`.
    pub fn path(&self, i: usize) -> alloc::string::String {
        let mut parts = Vec::new();
        let mut cur = Some(i);
        while let Some(n) = cur {
            if self.nodes[n].parent.is_some() {
                parts.push(self.nodes[n].name);
            }
            cur = self.nodes[n].parent;
        }
        let mut s = alloc::string::String::new();
        for p in parts.iter().rev() {
            s.push('/');
            s.push_str(p);
        }
        if s.is_empty() {
            s.push('/');
        }
        s
    }

    /// Node at an absolute path (`/a/b`); a path naming a unit without its
    /// `@address` also matches (`/chosen`).
    pub fn find_path(&self, path: &str) -> Option<usize> {
        let mut cur = 0usize;
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            cur = (0..self.nodes.len()).find(|&i| {
                let n = &self.nodes[i];
                n.parent == Some(cur)
                    && (n.name == comp || (!comp.contains('@') && n.name.split('@').next() == Some(comp)))
            })?;
        }
        Some(cur)
    }

    /// Indices of nodes listing `compat` in their `compatible`.
    pub fn compatible<'s>(&'s self, compat: &'s str) -> impl Iterator<Item = usize> + 's {
        (0..self.nodes.len()).filter(move |&i| self.nodes[i].is_compatible(compat))
    }

    /// Map a bus address on the bus `bus` to its parent address space, up to
    /// the root, through each bus's `ranges` (a Raspberry Pi's peripherals sit
    /// at 0x7e00_0000 on /soc and 0xfe00_0000 physically). An empty `ranges` is
    /// the identity; a node with none (/cpus: `reg` is an id) is left alone.
    fn translate(&self, bus: usize, mut addr: u64) -> u64 {
        let mut cur = bus;
        while let Some(up) = self.nodes[cur].parent {
            let n = &self.nodes[cur];
            let Some(r) = n.prop("ranges") else { break };
            if !r.is_empty() {
                let (ca, pa, sc) = (n.addr_cells as usize, self.nodes[up].addr_cells as usize, n.size_cells as usize);
                if ca == 0 || ca > 2 || pa == 0 || pa > 2 || sc > 2 {
                    break;
                }
                let cell = |off: usize, cells: usize| (0..cells).fold(0u64, |acc, k| (acc << 32) | be32(r, off + k * 4) as u64);
                let hit = (0..r.len() / ((ca + pa + sc) * 4)).find_map(|e| {
                    let off = e * (ca + pa + sc) * 4;
                    let (child, parent, size) = (cell(off, ca), cell(off + ca * 4, pa), cell(off + (ca + pa) * 4, sc));
                    (addr >= child && addr - child < size).then(|| parent + (addr - child))
                });
                let Some(mapped) = hit else { break };
                addr = mapped;
            }
            cur = up;
        }
        addr
    }

    /// `reg` of node `i` as (physical address, size) pairs, using its parent's
    /// cells and translated through the buses above it.
    pub fn reg(&self, i: usize) -> Vec<(u64, u64)> {
        let n = &self.nodes[i];
        let (ac, sc) = match n.parent {
            Some(p) => (self.nodes[p].addr_cells as usize, self.nodes[p].size_cells as usize),
            None => (2, 1),
        };
        let mut out = Vec::new();
        let Some(v) = n.prop("reg") else { return out };
        if ac == 0 || ac > 2 || sc > 2 {
            return out;
        }
        let step = (ac + sc) * 4;
        let cell = |off: usize, cells: usize| -> u64 {
            (0..cells).fold(0u64, |acc, k| (acc << 32) | be32(v, off + k * 4) as u64)
        };
        let mut off = 0;
        while off + step <= v.len() {
            let addr = match n.parent {
                Some(p) => self.translate(p, cell(off, ac)),
                None => cell(off, ac),
            };
            out.push((addr, cell(off + ac * 4, sc)));
            off += step;
        }
        out
    }

    /// RAM ranges: every `device_type = "memory"` node's `reg`.
    pub fn memory(&self) -> Vec<(u64, u64)> {
        (0..self.nodes.len())
            .filter(|&i| self.nodes[i].strings("device_type").any(|s| s == "memory"))
            .flat_map(|i| self.reg(i))
            .collect()
    }

    /// The console device: `/chosen` `stdout-path` (options after ':' and
    /// `/aliases` names resolved).
    pub fn stdout(&self) -> Option<usize> {
        let chosen = self.find_path("/chosen")?;
        let raw = self.nodes[chosen].strings("stdout-path").next()?;
        let path = raw.split(':').next()?;
        if path.starts_with('/') {
            return self.find_path(path);
        }
        let aliases = self.find_path("/aliases")?;
        let target = self.nodes[aliases].strings(path).next()?;
        self.find_path(target)
    }

    /// The UART to talk to: `stdout-path`, unless that is missing or names the
    /// Raspberry Pi's mini-UART (no driver; its baud rate follows the core
    /// clock), then the `uart0` alias -- the Pi's PL011, which our config.txt
    /// routes to the header pins.
    pub fn console(&self) -> Option<usize> {
        self.stdout().filter(|&i| !self.nodes[i].is_compatible("brcm,bcm2835-aux-uart")).or_else(|| {
            let aliases = self.find_path("/aliases")?;
            let target = self.nodes[aliases].strings("uart0").next()?;
            self.find_path(target)
        })
    }

    /// CPUs: `/cpus` children with `device_type = "cpu"`.
    pub fn cpu_count(&self) -> usize {
        let Some(cpus) = self.find_path("/cpus") else { return 0 };
        self.nodes
            .iter()
            .filter(|n| n.parent == Some(cpus) && n.strings("device_type").any(|s| s == "cpu"))
            .count()
    }
}

/// What the kernel needs to know to bring a board up, from its Device Tree.
#[derive(Debug, Default)]
pub struct Platform {
    pub model: alloc::string::String,
    pub memory: Vec<(u64, u64)>,
    pub cpus: usize,
    /// (compatible, base) of the console UART.
    pub uart: Option<(alloc::string::String, u64)>,
    /// (compatible, base) of the interrupt controller.
    pub irq: Option<(alloc::string::String, u64)>,
    pub timer: Option<alloc::string::String>,
    pub virtio_mmio: Vec<u64>,
    /// PSCI (ARM) or an SBI-era poweroff device present.
    pub power: Option<alloc::string::String>,
}

const IRQ_CONTROLLERS: &[&str] = &["arm,gic-v3", "arm,cortex-a15-gic", "arm,gic-400", "sifive,plic-1.0.0", "riscv,plic0"];

impl Platform {
    pub fn discover(fdt: &Fdt) -> Platform {
        let first = |i: usize| fdt.reg(i).first().map(|r| r.0).unwrap_or(0);
        let compat0 = |i: usize| alloc::string::String::from(fdt.nodes[i].strings("compatible").next().unwrap_or(""));
        let root = fdt.root();
        let model = root.strings("compatible").next().unwrap_or("?");
        let mut virtio: Vec<u64> = fdt.compatible("virtio,mmio").map(first).collect();
        virtio.sort_unstable();
        Platform {
            model: alloc::string::String::from(model),
            memory: fdt.memory(),
            cpus: fdt.cpu_count(),
            uart: fdt.console().map(|i| (compat0(i), first(i))),
            irq: IRQ_CONTROLLERS.iter().find_map(|c| fdt.compatible(c).next()).map(|i| (compat0(i), first(i))),
            timer: ["arm,armv8-timer", "riscv,clint0", "sifive,clint0"]
                .iter()
                .find_map(|c| fdt.compatible(c).next())
                .map(compat0)
                .or_else(|| {
                    // RISC-V: the timer is SBI's; /cpus carries timebase-frequency.
                    let cpus = fdt.find_path("/cpus")?;
                    let hz = fdt.nodes[cpus].u32_prop("timebase-frequency")?;
                    Some(alloc::format!("sbi timer ({} Hz timebase)", hz))
                }),
            virtio_mmio: virtio,
            power: ["arm,psci-1.0", "arm,psci-0.2", "arm,psci", "syscon-poweroff"]
                .iter()
                .find_map(|c| fdt.compatible(c).next())
                .map(compat0),
        }
    }

    pub fn summary(&self) -> alloc::string::String {
        let ram: u64 = self.memory.iter().map(|m| m.1).sum();
        // UEFI firmware removes /memory nodes (its memory map is authoritative).
        let ram = if self.memory.is_empty() {
            alloc::string::String::from("RAM per firmware memory map")
        } else {
            alloc::format!("{} MiB RAM at {:#x}", ram / (1024 * 1024), self.memory[0].0)
        };
        alloc::format!(
            "{}: {}, {} CPU(s), console {} @ {:#x}, irq {} @ {:#x}, timer {}, {} virtio-mmio slot(s), power {}",
            self.model,
            ram,
            self.cpus,
            self.uart.as_ref().map(|u| u.0.as_str()).unwrap_or("-"),
            self.uart.as_ref().map(|u| u.1).unwrap_or(0),
            self.irq.as_ref().map(|u| u.0.as_str()).unwrap_or("-"),
            self.irq.as_ref().map(|u| u.1).unwrap_or(0),
            self.timer.as_deref().unwrap_or("-"),
            self.virtio_mmio.len(),
            self.power.as_deref().unwrap_or("-"),
        )
    }
}

// Real blobs from `qemu-system-{aarch64,riscv64} -M virt,dumpdtb=…` (QEMU 10.2,
// 2 CPUs, 512 MiB; the aarch64 one with GICv3, repacked from QEMU's 1 MiB
// padding). They are what the ARM64/RISC-V ports will receive at boot.
static VIRT_AARCH64: &[u8] = include_bytes!("../testdata/virt-aarch64.dtb");
static VIRT_RISCV64: &[u8] = include_bytes!("../testdata/virt-riscv64.dtb");
// The tree the Raspberry Pi firmware ships for a Pi 4 (RPi4 UEFI firmware v1.53).
#[cfg(target_arch = "aarch64")]
static RPI4: &[u8] = include_bytes!("../testdata/bcm2711-rpi-4-b.dtb");

/// The size of a flattened tree at `blob`, trusting nothing but its header.
///
/// # Safety
/// `blob` must point to at least 8 readable bytes.
#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
pub unsafe fn total_size(blob: *const u8) -> usize {
    let word = unsafe { core::ptr::read_unaligned(blob.add(4) as *const u32) };
    u32::from_be(word) as usize
}

/// RAM ranges of the tree at `blob`, without a heap: the direct boot needs
/// them to build the memory map the heap itself comes from.
///
/// # Safety
/// `blob` must point to a whole flattened tree (the entry code checked the magic).
#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
pub unsafe fn scan_memory(blob: *const u8, found: &mut dyn FnMut(u64, u64)) {
    let b = unsafe { core::slice::from_raw_parts(blob, total_size(blob)) };
    let (st, strings) = (be32(b, 8) as usize, be32(b, 12) as usize);
    let (mut p, mut depth, mut in_memory) = (st, 0usize, false);
    let (mut ac, mut sc) = (2usize, 1usize);
    while p + 4 <= b.len() {
        let token = be32(b, p);
        p += 4;
        match token {
            BEGIN_NODE => {
                let len = b[p..].iter().position(|&c| c == 0).unwrap_or(0);
                depth += 1;
                if depth == 2 {
                    in_memory = b[p..p + len].starts_with(b"memory");
                }
                p = (p + len + 4) & !3;
            }
            END_NODE => {
                depth = depth.saturating_sub(1);
                if depth < 2 {
                    in_memory = false;
                }
            }
            PROP => {
                let (len, name) = (be32(b, p) as usize, be32(b, p + 4) as usize);
                let (v, name) = (&b[p + 8..p + 8 + len], &b[strings + name..]);
                let is = |s: &[u8]| name.starts_with(s) && name.get(s.len()) == Some(&0);
                if depth == 1 && is(b"#address-cells") {
                    ac = be32(v, 0) as usize;
                } else if depth == 1 && is(b"#size-cells") {
                    sc = be32(v, 0) as usize;
                } else if depth == 2 && in_memory && is(b"reg") {
                    let cell = |off: usize, n: usize| (0..n).fold(0u64, |a, k| (a << 32) | be32(v, off + k * 4) as u64);
                    for e in 0..len / ((ac + sc) * 4) {
                        let off = e * (ac + sc) * 4;
                        found(cell(off, ac), cell(off + ac * 4, sc));
                    }
                }
                p = (p + 8 + len + 3) & !3;
            }
            NOP => {}
            _ => return,
        }
    }
}

/// Parse both QEMU `virt` trees and check what discovery finds against the
/// known machine layout; also feed corrupted blobs and require clean errors.
/// On a boot that received a DTB, parse that one too. → `MILESTONE: DTB OK`.
pub fn self_test() -> bool {
    let mut ok = true;
    let mut check = |what: &str, cond: bool| {
        if !cond {
            crate::println!("[dtb] check failed: {}", what);
            ok = false;
        }
    };

    match Fdt::parse(VIRT_AARCH64) {
        Ok(fdt) => {
            let p = Platform::discover(&fdt);
            crate::println!("[dtb] aarch64 {}", p.summary());
            check("arm boot cpu", fdt.boot_cpu == 0);
            check("arm memory", p.memory == [(0x4000_0000, 0x2000_0000)]);
            check("arm cpus", p.cpus == 2);
            check("arm uart", p.uart == Some(("arm,pl011".into(), 0x900_0000)));
            check("arm gic", p.irq == Some(("arm,gic-v3".into(), 0x800_0000)));
            check("arm timer", p.timer.as_deref() == Some("arm,armv8-timer"));
            check("arm virtio", p.virtio_mmio.len() == 32 && p.virtio_mmio[0] == 0xa00_0000 && p.virtio_mmio[31] == 0xa00_3e00);
            check("arm psci", p.power.as_deref() == Some("arm,psci-1.0"));
            check("arm pcie ecam", fdt.compatible("pci-host-ecam-generic").next().map(|i| fdt.reg(i)[0]) == Some((0x40_1000_0000, 0x1000_0000)));
        }
        Err(e) => check(e, false),
    }
    match Fdt::parse(VIRT_RISCV64) {
        Ok(fdt) => {
            let p = Platform::discover(&fdt);
            crate::println!("[dtb] riscv64 {}", p.summary());
            check("rv memory", p.memory == [(0x8000_0000, 0x2000_0000)]);
            check("rv cpus", p.cpus == 2);
            check("rv uart", p.uart == Some(("ns16550a".into(), 0x1000_0000)));
            check("rv plic", p.irq.as_ref().map(|i| i.1) == Some(0xc00_0000));
            check("rv virtio", p.virtio_mmio.len() == 8 && p.virtio_mmio[0] == 0x1000_1000);
            check("rv timer", p.timer.is_some());
            check("rv path", fdt.stdout().map(|i| fdt.path(i)).as_deref() == Some("/soc/serial@10000000"));
        }
        Err(e) => check(e, false),
    }

    // A Raspberry Pi 4: everything behind /soc `ranges`, no stdout-path, a GICv2.
    #[cfg(target_arch = "aarch64")]
    match Fdt::parse(RPI4) {
        Ok(fdt) => {
            let p = Platform::discover(&fdt);
            crate::println!("[dtb] rpi4 {}", p.summary());
            check("rpi cpus", p.cpus == 4);
            check("rpi console = PL011 through ranges", p.uart == Some(("arm,pl011".into(), 0xfe20_1000)));
            check("rpi gic-400", p.irq == Some(("arm,gic-400".into(), 0xff84_1000)));
            let gic = fdt.compatible("arm,gic-400").next().map(|i| fdt.reg(i));
            check("rpi gicc", gic.as_ref().and_then(|r| r.get(1)).map(|r| r.0) == Some(0xff84_2000));
            check("rpi cpu ids untranslated", fdt.find_path("/cpus/cpu@3").map(|i| fdt.reg(i)[0].0) == Some(3));
            check("rpi timer", p.timer.as_deref() == Some("arm,armv8-timer"));
        }
        Err(e) => check(e, false),
    }

    // Malformed input must be rejected, never read out of bounds.
    let mut bad = alloc::vec::Vec::from(VIRT_RISCV64);
    bad[0] = 0;
    check("bad magic rejected", Fdt::parse(&bad).is_err());
    check("truncated rejected", Fdt::parse(&VIRT_RISCV64[..VIRT_RISCV64.len() / 2]).is_err());
    let mut wild = alloc::vec::Vec::from(VIRT_RISCV64);
    wild[8..12].copy_from_slice(&0x7fff_fff0u32.to_be_bytes()); // struct offset
    check("wild offset rejected", Fdt::parse(&wild).is_err());
    let mut tok = alloc::vec::Vec::from(VIRT_RISCV64);
    let st = be32(&tok, 8) as usize;
    tok[st..st + 4].copy_from_slice(&7u32.to_be_bytes());
    check("bad token rejected", Fdt::parse(&tok).is_err());
    check("empty rejected", Fdt::parse(&[]).is_err());

    // A DTB handed over by the loader (ARM64/RISC-V boots; none on PCs).
    let handed = crate::boot::info().firmware.dtb;
    let mut live = "none (ACPI platform)";
    if let Some(phys) = handed {
        match unsafe { Fdt::from_phys(phys) } {
            Ok(fdt) => {
                crate::println!("[dtb] boot {}", Platform::discover(&fdt).summary());
                live = "parsed";
            }
            Err(e) => {
                check(e, false);
                live = "failed";
            }
        }
    }
    if ok {
        crate::println!(
            "[kernel] MILESTONE: DTB OK (Device Tree parser: QEMU virt aarch64 + riscv64 discovered -- RAM, CPUs, UART, GIC/PLIC, timer, virtio-mmio, PSCI; malformed blobs rejected; loader DTB: {})",
            live
        );
    }
    ok
}
