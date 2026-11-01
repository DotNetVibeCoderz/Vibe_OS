//! Multi-core on RISC-V: one hart for now (SMP via SBI HSM is a later
//! increment). The shared scheduler and the `cpu` shell command call these.

/// Ping idle CPUs when a task becomes runnable: there are none to ping yet.
pub fn wake_others() {}

/// Harts the kernel runs on.
pub fn cpus_online() -> usize {
    1
}

/// The `cpu` shell command's report.
pub fn report() -> alloc::vec::Vec<alloc::string::String> {
    let n = cpus_online();
    let mut out = alloc::vec![alloc::format!(
        "{} hart (RV64, S-mode di bawah SBI), scheduler simetris + Big Kernel Lock; timebase {} MHz",
        n,
        super::cpu::counter_hz() / 1_000_000
    )];
    for c in 0..n {
        out.push(alloc::format!(
            "  CPU {}: {} interrupt, {} syscall, {} task-switch kernel",
            c,
            super::trap::irqs_on(c),
            crate::task::syscalls_on(c),
            crate::task::kernel_runs_on(c)
        ));
    }
    out
}
