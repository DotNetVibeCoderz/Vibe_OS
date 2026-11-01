//! The x86 GDT/TSS have no RISC-V counterpart: a trap from U-mode lands on the
//! kernel stack whose top `sscratch` holds -- the interrupted task's own
//! kernel stack, just below its `enter_user_thread` frame. These are the
//! no-op equivalents the shared scheduler (`task.rs`) calls.

/// An address with the x86_64 crate's accessor, for `task.rs`.
pub struct StackTop(u64);

impl StackTop {
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// The boot task's "privilege stack": none on RISC-V (traps from U-mode use
/// the task's kernel stack through `sscratch`).
pub fn privilege_stack_top() -> StackTop {
    StackTop(0)
}

/// Per-CPU ring-0 stack for interrupts from user mode: `sscratch` serves.
pub fn set_rsp0_on(_cpu: usize, _top: u64) {}
