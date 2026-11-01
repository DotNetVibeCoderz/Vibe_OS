//! Multi-core on ARM64 comes with a later increment (PSCI CPU_ON + per-CPU
//! GIC redistributors). Until then one CPU runs everything; these are the
//! single-core answers the shared scheduler asks for.

/// Wake idle CPUs because a task became runnable: there are none to wake.
pub fn wake_others() {}

/// The `cpu` shell command's report.
pub fn report() -> alloc::vec::Vec<alloc::string::String> {
    alloc::vec![alloc::format!(
        "1 CPU (AArch64; core lain menyusul lewat PSCI CPU_ON), scheduler simetris + Big Kernel Lock; timer generik {} MHz",
        super::cpu::counter_hz() / 1_000_000
    )]
}

/// CPUs running the kernel.
#[allow(dead_code)]
pub fn cpus_online() -> usize {
    1
}
