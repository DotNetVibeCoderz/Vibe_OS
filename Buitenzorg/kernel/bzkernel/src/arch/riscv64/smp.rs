//! Multi-core on RISC-V: one hart for now (SMP via SBI HSM is a later
//! increment). The shared scheduler calls these.

/// Ping idle CPUs when a task becomes runnable: there are none to ping yet.
pub fn wake_others() {}

/// Harts the kernel runs on.
pub fn cpus_online() -> usize {
    1
}
