//! The x86 GDT/TSS have no ARM64 counterpart: an exception from EL0 runs on
//! SP_EL1, which already is the interrupted task's kernel stack. These are
//! the no-op equivalents the shared scheduler (`task.rs`) calls.

/// An address with the x86_64 crate's accessor, for `task.rs`.
pub struct StackTop(u64);

impl StackTop {
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// The boot task's "privilege stack": none on ARM64 (it runs on Limine's
/// stack, which SP_EL1 already points into).
pub fn privilege_stack_top() -> StackTop {
    StackTop(0)
}

/// Per-CPU ring-0 stack for interrupts from user mode: SP_EL1 serves.
pub fn set_rsp0_on(_cpu: usize, _top: u64) {}
