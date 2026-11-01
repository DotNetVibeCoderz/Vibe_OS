//! Hardware virtualization detection on RISC-V (the x86 `vmx` counterpart for
//! the hypervisor code). RISC-V's extension is "H" (hypervisor mode, HS); a
//! kernel running in plain S-mode under OpenSBI -- QEMU `virt` with
//! `-cpu rv64` -- has no use of it here (`misa` is M-mode only, and nothing
//! sets the kernel up as a hypervisor), so the executor is the software VMM
//! (`crate::vmm`), as on x86 under TCG and on ARM64 at EL1.

/// Which virtualization backend the hypervisor will use.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Backend {
    HardwareVtx,
    HardwareSvm,
    Software,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::HardwareVtx => "hardware VT-x (VMX)",
            Backend::HardwareSvm => "hardware AMD-V (SVM)",
            Backend::Software => "software VMM (virtual CPU)",
        }
    }
}

pub fn has_vtx() -> bool {
    false
}

pub fn has_svm() -> bool {
    false
}

/// The SBI implementation, ASCII (the closest thing to a CPU vendor string
/// S-mode can read; `mvendorid` is M-mode only).
pub fn vendor() -> [u8; 12] {
    match super::sbi::version().2 {
        1 => *b"OpenSBI     ",
        _ => *b"RISC-V      ",
    }
}

pub fn backend() -> Backend {
    Backend::Software
}

pub fn summary() -> alloc::vec::Vec<alloc::string::String> {
    let (maj, min, imp, ver) = super::sbi::version();
    alloc::vec![
        alloc::format!("CPU           : RV64 (S-mode), SBI {}.{} impl {} versi {:#x}", maj, min, imp, ver),
        alloc::format!("Paging        : {}", super::paging::mode_name()),
        alloc::format!("Ekstensi H    : tidak dipakai (kernel di S-mode di bawah OpenSBI)"),
        alloc::format!("Eksekusi      : {}", Backend::Software.name()),
    ]
}
