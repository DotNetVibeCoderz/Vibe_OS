//! Multi-core on ARM64 comes with a later increment (PSCI CPU_ON + per-CPU
//! GIC redistributors). Until then one CPU runs everything; these are the
//! single-core answers the shared scheduler asks for.

/// Wake idle CPUs because a task became runnable: there are none to wake.
pub fn wake_others() {}

/// CPUs running the kernel.
#[allow(dead_code)]
pub fn cpus_online() -> usize {
    1
}
