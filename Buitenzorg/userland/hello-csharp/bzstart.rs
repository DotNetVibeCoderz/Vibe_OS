//! Freestanding Buitenzorg startup + PAL shim for bflat/zerolib C# programs
//! (v0.4 "Tunas"). Compiled to an object for `x86_64-unknown-none` and linked
//! with the bflat `-c` output to produce a static ELF that runs in ring 3.
//!
//! It replaces glibc + libSystem.Native with three raw-syscall functions and
//! an `_start` that calls the NativeAOT managed entry. The ring-3 syscall ABI
//! matches the kernel dispatcher (kernel/bzkernel/src/syscall.rs):
//!   rax = syscall number, rdi/rsi/rdx = args, result in rax.
#![no_std]
#![no_main]
#![allow(internal_features)]

use core::panic::PanicInfo;

// Buitenzorg syscall numbers (mirror of bz_abi::sysno).
const SYS_DEBUG_WRITE: u64 = 1;
const SYS_EXIT: u64 = 2;
const SYS_TICKS: u64 = 4;
const SYS_WIN_CREATE: u64 = 6;
const SYS_WIN_CMD: u64 = 7;
const SYS_WIN_PRESENT: u64 = 8;
const SYS_KEY_READ: u64 = 9;
const SYS_PROC_LIST: u64 = 10;
const SYS_PROC_KILL: u64 = 11;
const SYS_SYS_STAT: u64 = 12;

#[inline(always)]
unsafe fn syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") nr => ret,
        in("rdi") a0,
        in("rsi") a1,
        in("rdx") a2,
        lateout("rcx") _, // clobbered by syscall
        lateout("r11") _, // clobbered by syscall
        options(nostack),
    );
    ret
}

#[inline(always)]
unsafe fn syscall2(nr: u64, a0: u64, a1: u64) -> u64 {
    syscall3(nr, a0, a1, 0)
}

// NativeAOT managed entry emitted by bflat: `int __managed__Main(int, char**)`.
extern "C" {
    fn __managed__Main(argc: i32, argv: *const *const u8) -> i32;
}

// Freestanding C runtime intrinsics the compiler emits for struct/buffer
// zeroing and copies. Needed once apps use larger stackalloc/structs.
#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    let byte = c as u8;
    let mut i = 0;
    while i < n {
        *dest.add(i) = byte;
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dest.add(i) = *src.add(i);
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dest as usize) < (src as usize) {
        memcpy(dest, src, n)
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *dest.add(i) = *src.add(i);
        }
        dest
    }
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

/// zerolib Console.Write → SystemNative_Log(buffer, length).
#[no_mangle]
pub unsafe extern "C" fn SystemNative_Log(buffer: *const u8, length: i32) {
    if !buffer.is_null() && length > 0 {
        syscall2(SYS_DEBUG_WRITE, buffer as u64, length as u64);
    }
}

/// A bump allocator over a fixed .bss arena. zerolib never frees, so this is
/// enough to run object-allocating programs; a real heap arrives with the GC
/// integration later in v0.4.
const HEAP_SIZE: usize = 256 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static mut HEAP_OFFSET: usize = 0;

#[no_mangle]
pub unsafe extern "C" fn SystemNative_Malloc(size: usize) -> *mut u8 {
    let align = 16;
    let start = (HEAP_OFFSET + align - 1) & !(align - 1);
    if start + size > HEAP_SIZE {
        return core::ptr::null_mut();
    }
    HEAP_OFFSET = start + size;
    // Raw pointer arithmetic to avoid a bounds-check panic path (no panic
    // machinery is linked into a freestanding user program).
    (core::ptr::addr_of_mut!(HEAP) as *mut u8).add(start)
}

#[no_mangle]
pub unsafe extern "C" fn SystemNative_Abort() -> ! {
    syscall2(SYS_EXIT, 134, 0);
    loop {}
}

// --- Buitenzorg UI/app ABI (v0.8), called from C# apps via DllImport --------

/// Create an app window; returns its id. `dims = (w << 32) | h`.
#[no_mangle]
pub unsafe extern "C" fn bz_win_create(title_ptr: *const u8, title_len: u64, dims: u64) -> u32 {
    syscall3(SYS_WIN_CREATE, title_ptr as u64, title_len, dims) as u32
}

/// Submit a draw command (`cmd` points to a DrawCmd) to a window.
#[no_mangle]
pub unsafe extern "C" fn bz_win_cmd(window: u32, cmd: *const u8) -> u64 {
    syscall2(SYS_WIN_CMD, window as u64, cmd as u64)
}

/// Recompose the desktop so the window's canvas is shown.
#[no_mangle]
pub unsafe extern "C" fn bz_win_present(window: u32) {
    syscall2(SYS_WIN_PRESENT, window as u64, 0);
}

/// Read one keyboard character (0 when none).
#[no_mangle]
pub unsafe extern "C" fn bz_key_read() -> u32 {
    syscall2(SYS_KEY_READ, 0, 0) as u32
}

/// Monotonic timer ticks since boot (for simple app timing).
#[no_mangle]
pub unsafe extern "C" fn bz_ticks() -> u64 {
    syscall2(SYS_TICKS, 0, 0)
}

/// Fill a ProcInfo array; returns the count written.
#[no_mangle]
pub unsafe extern "C" fn bz_proc_list(buf: *mut u8, max: u64) -> u64 {
    syscall2(SYS_PROC_LIST, buf as u64, max)
}

/// Terminate a process by pid; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_proc_kill(pid: u64) -> u64 {
    syscall2(SYS_PROC_KILL, pid, 0)
}

/// Fill a SysStat pointed to by `out`; 0 on success.
#[no_mangle]
pub unsafe extern "C" fn bz_sys_stat(out: *mut u8) -> u64 {
    syscall2(SYS_SYS_STAT, out as u64, 0)
}

/// ELF entry point. zerolib's Main needs no OS-level init, so we call the
/// managed entry directly with a minimal argv, then exit with its return code.
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "xor ebp, ebp",
        "lea rdi, [rip + PROG_NAME]",  // argv[0]
        "push 0",                       // argv[1] = null
        "push rdi",                     // argv[0]
        "mov rsi, rsp",                 // argv
        "mov edi, 1",                   // argc = 1
        "and rsp, -16",                 // 16-byte align for the call
        "call {main}",
        "mov edi, eax",                 // exit code
        "mov rax, {exit}",
        "mov esi, 0",
        "syscall",
        "2: hlt",
        "jmp 2b",
        main = sym __managed__Main,
        exit = const SYS_EXIT,
    )
}

#[no_mangle]
static PROG_NAME: [u8; 6] = *b"hello\0";

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { SystemNative_Abort() }
}
