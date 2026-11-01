//! Hardware virtualization detection on ARM64 (the x86 `vmx` counterpart for
//! the hypervisor code). ARM's extension is EL2; a kernel entered at EL1 (as
//! here, QEMU `virt` without virtualization=on) cannot use it, so the
//! executor is the software VMM (`crate::vmm`), as on x86 under TCG.

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

/// The CPU's implementer, ASCII (from MIDR_EL1).
pub fn vendor() -> [u8; 12] {
    if super::cpu::midr() >> 24 == 0x41 {
        *b"ARM         "
    } else {
        *b"unknown     "
    }
}

pub fn backend() -> Backend {
    Backend::Software
}

pub fn summary() -> alloc::vec::Vec<alloc::string::String> {
    let midr = super::cpu::midr();
    alloc::vec![
        alloc::format!("CPU           : AArch64, implementer {:#x}, part {:#x}", midr >> 24, (midr >> 4) & 0xFFF),
        alloc::format!("Kernel di     : EL{} (EL2 = ekstensi virtualisasi ARM)", super::cpu::current_el()),
        alloc::format!(
            "EL2 tersedia  : {}",
            if super::cpu::current_el() >= 2 { "ya" } else { "tidak (kernel di EL1)" }
        ),
        alloc::format!("Eksekusi      : {}", Backend::Software.name()),
    ]
}
