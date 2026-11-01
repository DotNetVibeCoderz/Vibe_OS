//! Hardware virtualization detection (v0.13 "Lapis"). A type-2 hypervisor uses
//! the CPU's virtualization extensions (Intel VT-x / AMD-V). Here we *detect*
//! those extensions honestly via CPUID (and the VMX capability MSRs when
//! present) and choose a backend. Nested VT-x/AMD-V is not exposed by QEMU/TCG
//! on the dev machines, so the real execution path is the software VMM in
//! [`crate::vmm`] — mirroring the compute layer's GPU→CPU fallback.

use core::arch::x86_64::__cpuid;

/// Which virtualization backend the hypervisor will use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Intel VT-x (VMX) available on this CPU.
    HardwareVtx,
    /// AMD-V (SVM) available on this CPU.
    HardwareSvm,
    /// No hardware extension exposed — use the software VMM (always available).
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

/// Intel VT-x: CPUID.1:ECX.VMX[bit 5].
pub fn has_vtx() -> bool {
    let r = __cpuid(1);
    (r.ecx >> 5) & 1 == 1
}

/// AMD-V: CPUID.80000001:ECX.SVM[bit 2] (guard on the leaf being present).
pub fn has_svm() -> bool {
    let max_ext = __cpuid(0x8000_0000).eax;
    if max_ext < 0x8000_0001 {
        return false;
    }
    let r = __cpuid(0x8000_0001);
    (r.ecx >> 2) & 1 == 1
}

/// CPU vendor string from CPUID leaf 0 (EBX,EDX,ECX).
pub fn vendor() -> [u8; 12] {
    let r = __cpuid(0);
    let mut v = [0u8; 12];
    v[0..4].copy_from_slice(&r.ebx.to_le_bytes());
    v[4..8].copy_from_slice(&r.edx.to_le_bytes());
    v[8..12].copy_from_slice(&r.ecx.to_le_bytes());
    v
}

/// Pick the best available backend. Reserved for the future native VMX/SVM
/// driver; v0.13 always executes on [`Backend::Software`].
#[allow(dead_code)]
pub fn backend() -> Backend {
    if has_vtx() {
        Backend::HardwareVtx
    } else if has_svm() {
        Backend::HardwareSvm
    } else {
        Backend::Software
    }
}

/// IA32_VMX_BASIC (0x480): revision id + capability bits. Only valid when VMX
/// is present; reading it otherwise would #GP, so callers must gate on
/// [`has_vtx`]. Returns the raw MSR value.
fn vmx_basic() -> u64 {
    let msr = x86_64::registers::model_specific::Msr::new(0x480);
    unsafe { msr.read() }
}

/// A short, honest description of the detected virtualization capabilities and
/// the chosen backend (for the shell and boot log).
pub fn summary() -> alloc::vec::Vec<alloc::string::String> {
    use alloc::string::String;
    use alloc::vec;

    let v = vendor();
    let vendor_str = core::str::from_utf8(&v).unwrap_or("unknown");
    let hw = has_vtx() || has_svm();

    let mut out = vec![
        alloc::format!("CPU vendor    : {}", vendor_str),
        alloc::format!("Intel VT-x    : {}", if has_vtx() { "ya" } else { "tidak" }),
        alloc::format!("AMD-V (SVM)   : {}", if has_svm() { "ya" } else { "tidak" }),
        // The executor in v0.13 is always the software VMM: a native VMX/SVM
        // driver (VMXON/EPT, VMCB) is backlog, and nested HW virtualization is
        // not exposed under QEMU/TCG regardless.
        alloc::format!("Eksekusi      : {}", Backend::Software.name()),
    ];

    if has_vtx() {
        let basic = vmx_basic();
        out.push(alloc::format!(
            "VMX revision  : {:#x} (IA32_VMX_BASIC)",
            basic & 0x7fff_ffff
        ));
    }
    out.push(String::from(if hw {
        "catatan       : ekstensi HW terdeteksi; driver native VMX/SVM = backlog"
    } else {
        "catatan       : ekstensi HW tak tersedia; VMM software (portabel) dipakai"
    }));
    out
}
