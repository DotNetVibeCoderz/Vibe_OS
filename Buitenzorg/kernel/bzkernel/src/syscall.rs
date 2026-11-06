//! Syscall ABI v1 dispatcher (requirements.md §4, §10.1).
//!
//! The numbered contract lives in the shared `bz-abi` crate and is mirrored
//! by `Buitenzorg.Runtime` (C#). Today the dispatcher is exercised from
//! kernel context; wiring it to a ring-3 `syscall` instruction entry point is
//! the remaining v0.2/v0.4 work tracked in requirements.md §17.

use bz_abi::{pixel_format, syserr, sysno, FramebufferInfo, ABI_VERSION};

use crate::{framebuffer, interrupts, task};

/// Copy `len` bytes from a user pointer into a kernel `Vec` using volatile
/// reads. Never build a `&[u8]`/`&[u32]` slice directly over a user-space
/// buffer: the optimizer then assumes the whole region is dereferenceable and
/// immutable, which miscompiles into a boot-corrupting Heisenbug (scrambled
/// serial/rodata). Reading per element with `read_volatile` and working from
/// the kernel copy avoids that entirely.
fn copy_user_bytes(ptr: u64, len: usize) -> alloc::vec::Vec<u8> {
    // Callers must have validated the range already (see `user_read` /
    // `user_write`); this is the last line of defence if one forgets.
    if from_user() && !crate::memory::validate_user_range(ptr, len as u64, false) {
        return alloc::vec::Vec::new();
    }
    let mut v = alloc::vec::Vec::with_capacity(len);
    let p = ptr as *const u8;
    for i in 0..len {
        v.push(unsafe { core::ptr::read_volatile(p.add(i)) });
    }
    v
}

/// Entry point for ring-3 syscalls: marks the call as untrusted, so the
/// pointer-validating helpers below actually check.
///
/// "Servicing a ring-3 syscall" is per-*task* state (task.rs carries it across
/// context switches): a syscall can yield and resume on another CPU, and with
/// fine-grained locking several CPUs serve syscalls at the same time — a global
/// flag would let one CPU's kernel-internal call disable another's pointer
/// validation (or the reverse).
pub fn dispatch_from_user(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    // Profiler zone: total time spent servicing ring-3 syscalls. Inert unless
    // profiling is enabled.
    let _z = crate::profile::Guard::new("syscall");
    let prev = task::set_in_user_syscall(true);
    let r = dispatch(nr, a0, a1, a2);
    task::set_in_user_syscall(prev);
    r
}

fn from_user() -> bool {
    task::in_user_syscall()
}

// --- Fine-grained locking (SMP increment 5) -----------------------------------
//
// These syscalls touch only state behind its own lock, so they run WITHOUT the
// Big Kernel Lock and may execute on several CPUs at the same time:
//   - memory management (MMAP/MPROTECT/MUNMAP): the paging context `memory::CTX`
//     (page tables + frame allocator) and `memory::ARENAS`, always taken in that
//     order; TLB shootdowns happen with neither held;
//   - scheduler (YIELD, THREAD_SELF, FUTEX_WAIT/WAKE): the scheduler lock; the
//     futex value check and the block are one step under it (`futex_wait_if`),
//     so no wake-up is lost;
//   - console (DEBUG_WRITE): the serial and framebuffer-console locks;
//   - window system (WIN_CREATE/WIN_CMD/WIN_PRESENT, FB_INFO): the `wm::WM`
//     lock, then theme / wallpaper / RTC / process-registry locks inside it
//     (never the other way round); KEY_READ: keyboard queue + `wm::WM`;
//   - process registry (PROC_LIST, PROC_ARGS, SYS_STAT): `process::APPS`,
//     the scheduler lock, the heap allocator's lock;
//   - CLOCK_RTC: the CMOS lock (rtc.rs);
//   - files (FS_LIST/FS_READ/FS_WRITE): `vfs::MOUNTS`, held for the whole
//     operation — it also serializes the FAT code and the block devices
//     (ATA port I/O), which are only reachable through a mount;
//   - processes (PROC_SPAWN, PROC_WAIT): spawning takes the VFS lock to read
//     the image, the paging context to build the new space (installed only on
//     the calling CPU while loading), the process registry and the scheduler;
//     PROC_WAIT's "still alive?" check and its block are one step under the
//     scheduler lock (`task::block_while`), and the exiting child unregisters
//     before its wake-up takes that lock, so no wake-up is lost;
//   - network (NET_*): `net::STACK` (the loopback device, sockets and
//     counters live behind it; `poll` takes it per frame);
//   - packages (PKG_LIST/PKG_SET): `pkg::INSTALLED` (installing checks the
//     payload through the VFS first, without holding it);
//   - audio (AUDIO_*): the AC'97 `DEVICE` lock, held across the port I/O and
//     the DMA buffer fill;
//   - constants/atomics (ABI_VERSION, IS_INTERACTIVE).
// Still under the BKL: EXIT / THREAD_EXIT / PROC_KILL (they tear processes
// down) and THREAD_CREATE / THREAD_JOIN.
// Pointer validation walks the calling CPU's own page tables. Everything else
// still takes the BKL.

static FINE_LOCKING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

/// Turn the fine-grained path on/off (off = every syscall takes the BKL, the
/// increment-4 behavior). Returns the previous setting.
pub fn set_fine_locking(on: bool) -> bool {
    FINE_LOCKING.swap(on, core::sync::atomic::Ordering::SeqCst)
}

/// True if syscall `nr` runs without the Big Kernel Lock.
pub fn bkl_free(nr: u64) -> bool {
    if !FINE_LOCKING.load(core::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    matches!(
        nr,
        sysno::ABI_VERSION
            | sysno::DEBUG_WRITE
            | sysno::YIELD
            | sysno::MMAP
            | sysno::MPROTECT
            | sysno::MUNMAP
            | sysno::FUTEX_WAIT
            | sysno::FUTEX_WAIT_TIMEOUT
            | sysno::CLOCK_NANOS
            | sysno::WIN_EVENT
            | sysno::TEXT_METRICS
            | sysno::FUTEX_WAKE
            | sysno::THREAD_SELF
            | sysno::IS_INTERACTIVE
            | sysno::FB_INFO
            | sysno::WIN_CREATE
            | sysno::WIN_CMD
            | sysno::WIN_PRESENT
            | sysno::KEY_READ
            | sysno::PROC_LIST
            | sysno::PROC_ARGS
            | sysno::SYS_STAT
            | sysno::CLOCK_RTC
            | sysno::FS_LIST
            | sysno::FS_READ
            | sysno::FS_WRITE
            | sysno::FS_OP
            | sysno::USER_OP
            | sysno::GFX3D
            | sysno::RANDOM
            | sysno::PROC_SPAWN
            | sysno::PROC_WAIT
            | sysno::NET_SOCKET
            | sysno::NET_BIND
            | sysno::NET_SEND
            | sysno::NET_RECV
            | sysno::NET_CLOSE
            | sysno::NET_INFO
            | sysno::PKG_LIST
            | sysno::PKG_SET
            | sysno::AUDIO_STAT
            | sysno::AUDIO_SET_VOLUME
            | sysno::AUDIO_TONE
            | sysno::AUDIO_PLAY
    )
}

/// A user buffer the kernel will only read. Returns false when the app passed
/// something it does not own — a kernel address, an unmapped page, or a range
/// that wraps. See [`crate::memory::validate_user_range`] for why this matters.
fn user_read(ptr: u64, len: u64) -> bool {
    if !from_user() {
        return ptr != 0; // kernel-origin call: trusted, but still not null
    }
    crate::memory::validate_user_range(ptr, len, false)
}

/// A user buffer the kernel will write results into. Stricter than
/// [`user_read`]: the pages must also be writable, so a syscall can never turn
/// a read-only mapping (or kernel memory) into an output buffer.
fn user_write(ptr: u64, len: u64) -> bool {
    if !from_user() {
        return ptr != 0;
    }
    crate::memory::validate_user_range(ptr, len, true)
}

/// Read a NUL-terminated path from user space, bounded and validated.
/// Returns `None` if the pointer is not user memory or has no terminator.
fn read_user_path_checked(path_ptr: u64) -> Option<alloc::string::String> {
    if path_ptr == 0 {
        return None;
    }
    let len = if from_user() {
        crate::memory::validate_user_cstr(path_ptr, 256)?
    } else {
        // Kernel-origin call: bounded scan of a trusted pointer.
        let p = path_ptr as *const u8;
        let mut n = 0u64;
        while n < 256 && unsafe { core::ptr::read_volatile(p.add(n as usize)) } != 0 {
            n += 1;
        }
        n
    };
    let mut path = alloc::string::String::new();
    let p = path_ptr as *const u8;
    for i in 0..len as usize {
        path.push(unsafe { core::ptr::read_volatile(p.add(i)) } as char);
    }
    Some(path)
}

/// Security self-test (v1.0 hardening): confirm that syscalls reject user
/// pointers the caller does not own. Runs from kernel context, so it passes
/// hostile addresses directly to [`dispatch`] the way a malicious ring-3 app
/// would. Returns true when every attack is refused.
///
/// Before this hardening each of these succeeded: the kernel would happily read
/// its own memory back out through `DEBUG_WRITE` (information leak) or write
/// syscall results *into* kernel memory through `PROC_LIST`/`FS_READ`/`SYS_STAT`
/// (arbitrary kernel write — full privilege escalation from an unprivileged
/// app), and an unmapped pointer took the kernel down with a page fault.
pub fn security_self_test() -> bool {
    let mut ok = true;

    // A kernel address (the heap) must never be accepted as an output buffer.
    let kernel_addr = crate::allocator::HEAP_START + 4096;
    // Every probe below must be seen as a ring-3 call, or the trusted
    // kernel path would (correctly) let it through.
    let mut check = |name: &str, res: u64| {
        if res != syserr::INVAL {
            crate::println!("[sec] FAIL: {} accepted a bad pointer (returned {})", name, res);
            ok = false;
        }
    };
    check("SYS_STAT(kernel)", dispatch_from_user(sysno::SYS_STAT, kernel_addr, 0, 0));
    check("PROC_LIST(kernel)", dispatch_from_user(sysno::PROC_LIST, kernel_addr, 8, 0));
    check("PKG_LIST(kernel)", dispatch_from_user(sysno::PKG_LIST, kernel_addr, 8, 0));
    check("FS_READ(kernel)", dispatch_from_user(sysno::FS_READ, kernel_addr, kernel_addr, 64));
    check("AUDIO_STAT(kernel)", dispatch_from_user(sysno::AUDIO_STAT, kernel_addr, 0, 0));
    check("FB_INFO(kernel)", dispatch_from_user(sysno::FB_INFO, kernel_addr, 0, 0));
    check("CLOCK_RTC(kernel)", dispatch_from_user(sysno::CLOCK_RTC, kernel_addr, 0, 0));
    check("NET_INFO(kernel)", dispatch_from_user(sysno::NET_INFO, kernel_addr, 0, 0));
    check("DEBUG_WRITE(kernel)", dispatch_from_user(sysno::DEBUG_WRITE, kernel_addr, 32, 0));

    // An unmapped user address must be refused, not page-fault the kernel.
    let unmapped = 0x1000_0000u64; // below USER_ADDR_MAX, never mapped
    check("SYS_STAT(unmapped)", dispatch_from_user(sysno::SYS_STAT, unmapped, 0, 0));
    check("DEBUG_WRITE(unmapped)", dispatch_from_user(sysno::DEBUG_WRITE, unmapped, 32, 0));

    // A range that starts in user space but runs past the limit must be refused.
    check(
        "FS_READ(overflow)",
        dispatch_from_user(sysno::FS_READ, unmapped, crate::memory::USER_ADDR_MAX - 16, 4096),
    );
    // And an outright wrapping length.
    check("PROC_LIST(wrap)", dispatch_from_user(sysno::PROC_LIST, u64::MAX - 8, 256, 0));

    // Null must still be rejected everywhere.
    check("SYS_STAT(null)", dispatch_from_user(sysno::SYS_STAT, 0, 0, 0));

    // Sanity: the validator must not reject legitimate kernel-side reads of
    // its own low-half mappings — there are none, so a known-bad case only.
    if crate::memory::validate_user_range(kernel_addr, 8, false) {
        crate::println!("[sec] FAIL: validate_user_range accepted a kernel address");
        ok = false;
    }
    ok
}

/// Dispatch a syscall by number. Arguments and result follow the C ABI:
/// unused arguments are ignored, results are returned as `u64`.
pub fn dispatch(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    match nr {
        sysno::ABI_VERSION => ABI_VERSION,
        sysno::DEBUG_WRITE => sys_debug_write(a0, a1),
        sysno::EXIT => sys_exit(a0),
        sysno::YIELD => {
            task::yield_now();
            0
        }
        sysno::TICKS => interrupts::ticks(),
        sysno::FB_INFO => sys_fb_info(a0),
        sysno::WIN_CREATE => sys_win_create(a0, a1, a2),
        sysno::WIN_CMD => sys_win_cmd(a0, a1),
        sysno::WIN_PRESENT => {
            crate::wm::flip_owned(crate::aspace::current_id());
            crate::wm::present_now();
            0
        }
        sysno::KEY_READ => sys_key_read(),
        sysno::PROC_SPAWN => sys_proc_spawn(a0, a1),
        sysno::PROC_WAIT => sys_proc_wait(a0),
        sysno::PROC_ARGS => sys_proc_args(a0, a1),
        sysno::PROC_LIST => sys_proc_list(a0, a1),
        sysno::PROC_KILL => {
            if crate::process::kill(a0) {
                0
            } else {
                syserr::INVAL
            }
        }
        sysno::SYS_STAT => sys_stat(a0),
        sysno::MMAP => crate::memory::user_mmap(a0, a1),
        sysno::MPROTECT => crate::memory::user_mprotect(a0, a1, a2),
        sysno::MUNMAP => crate::memory::user_munmap(a0, a1),
        sysno::THREAD_CREATE => sys_thread_create(a0, a1, a2),
        sysno::THREAD_JOIN => sys_thread_join(a0),
        sysno::THREAD_EXIT => crate::usermode::exit_user(a0),
        sysno::FUTEX_WAIT => sys_futex_wait(a0, a1),
        sysno::FUTEX_WAIT_TIMEOUT => sys_futex_wait_timeout(a0, a1, a2),
        sysno::CLOCK_NANOS => crate::arch::cpu::nanos(),
        sysno::WIN_EVENT => crate::wm::take_event(a0 as u32, crate::aspace::current_id()).unwrap_or(syserr::INVAL),
        sysno::TEXT_METRICS => {
            let style = match a0 {
                1 => crate::gfx::TextStyle::Strong,
                2 => crate::gfx::TextStyle::Heading,
                _ => crate::gfx::TextStyle::Body,
            };
            ((crate::gfx::text_width("M", style) as u64) << 32) | crate::gfx::line_height(style) as u64
        }
        sysno::SETTING => sys_setting(a0, a1, a2),
        sysno::HW_BOARD => crate::hwio::board(a0, a1),
        sysno::HW_GPIO => crate::hwio::gpio(a0, a1, a2),
        sysno::HW_LED => crate::hwio::led(a0, a1 != 0),
        sysno::HW_I2C => sys_hw_transfer(sysno::HW_I2C, 0, a0),
        sysno::HW_SPI => sys_hw_transfer(sysno::HW_SPI, 0, a0),
        sysno::HW_UART => sys_hw_transfer(sysno::HW_UART, a0, a1),
        sysno::FUTEX_WAKE => task::futex_wake_user(a0, a1),
        sysno::THREAD_SELF => task::current_id(),
        sysno::CLOCK_MONO => crate::arch::cpu::timestamp(),
        sysno::AUDIO_STAT => sys_audio_stat(a0),
        sysno::AUDIO_SET_VOLUME => {
            if crate::audio::set_volume(a0 as u32) {
                0
            } else {
                syserr::INVAL
            }
        }
        sysno::AUDIO_TONE => {
            if crate::audio::play_tone(a0 as u32, a1 as u32) {
                0
            } else {
                syserr::INVAL
            }
        }
        sysno::AUDIO_PLAY => {
            if a1 == 0 || !user_read(a0, a1) {
                syserr::INVAL
            } else if crate::audio::play_pcm(a0, a1) {
                0
            } else {
                syserr::INVAL
            }
        }
        sysno::PKG_LIST => sys_pkg_list(a0, a1),
        sysno::PKG_SET => sys_pkg_set(a0, a1, a2),
        sysno::FS_LIST => sys_fs_list(a0, a1, a2),
        sysno::FS_READ => sys_fs_read(a0, a1, a2),
        sysno::IS_INTERACTIVE => crate::interactive::is_active() as u64,
        sysno::FS_WRITE => sys_fs_write(a0, a1, a2),
        sysno::FS_OP => sys_fs_op(a0, a1, a2),
        sysno::USER_OP => sys_user_op(a0, a1, a2),
        sysno::GFX3D => sys_gfx3d(a0),
        sysno::RANDOM => sys_random(a0, a1),
        sysno::CLOCK_RTC => sys_clock_rtc(a0),
        sysno::NET_SOCKET => sys_net_socket(a0),
        sysno::NET_BIND => sys_net_bind(a0, a1),
        sysno::NET_SEND => sys_net_send(a0, a1, a2),
        sysno::NET_RECV => sys_net_recv(a0, a1, a2),
        sysno::NET_CLOSE => sys_net_close(a0),
        sysno::NET_INFO => sys_net_info(a0),
        _ => syserr::NOSYS,
    }
}

fn sys_fs_list(path_ptr: u64, buf_ptr: u64, max: u64) -> u64 {
    if max == 0 || max > 128 {
        return syserr::INVAL;
    }
    if !user_write(buf_ptr, max * core::mem::size_of::<bz_abi::FsEntry>() as u64) {
        return syserr::INVAL;
    }
    // An empty/NULL path lists the mounts; anything else must be a valid
    // NUL-terminated string in the caller's own memory.
    let path = if path_ptr == 0 {
        alloc::string::String::new()
    } else {
        match read_user_path_checked(path_ptr) {
            Some(p) => p,
            None => return syserr::INVAL,
        }
    };
    // Empty path => list the mounts (as directories); else list the folder.
    let names: alloc::vec::Vec<(alloc::string::String, bool)> = if path.is_empty() {
        crate::vfs::mounts().into_iter().map(|m| (m, true)).collect()
    } else {
        match crate::vfs::list_entries(&path) {
            Ok(v) => v.into_iter().map(|e| (e.name, e.is_dir)).collect(),
            Err(_) => return 0,
        }
    };
    let dst = buf_ptr as *mut bz_abi::FsEntry;
    let mut n = 0usize;
    for (name, is_dir) in names.iter() {
        if n >= max as usize {
            break;
        }
        // Mount names come back as "/disk"; strip the leading slash for display.
        let disp = name.strip_prefix('/').unwrap_or(name);
        let mut e = bz_abi::FsEntry { name: [0; 24], is_dir: if *is_dir { 1 } else { 0 } };
        let b = disp.as_bytes();
        let m = b.len().min(23);
        e.name[..m].copy_from_slice(&b[..m]);
        unsafe { core::ptr::write_volatile(dst.add(n), e) };
        n += 1;
    }
    n as u64
}

/// GFX3D: the 3D API (`bz_abi::gfx3d_op`, `gfx3d.rs`). Every data block is
/// validated and copied into the kernel (never a slice over user memory);
/// the f32s are converted to fixed point by `gfx3d` without floating point.
fn sys_gfx3d(cmd_ptr: u64) -> u64 {
    use bz_abi::{gfx3d_op as op, Gfx3dCmd};
    let size = core::mem::size_of::<Gfx3dCmd>() as u64;
    if !user_read(cmd_ptr, size) {
        return syserr::INVAL;
    }
    let raw = copy_user_bytes(cmd_ptr, size as usize);
    if raw.len() != size as usize {
        return syserr::INVAL;
    }
    let u32_at = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().unwrap());
    let c = Gfx3dCmd {
        op: u32_at(0),
        target: u32_at(4),
        a: u32_at(8),
        b: u32_at(12),
        c: u32_at(16),
        d: u32_at(20),
        ptr: u64_at(24),
        len: u64_at(32),
        ptr2: u64_at(40),
        len2: u64_at(48),
    };
    let owner = task::current_space().id;
    // a block of `n` 32-bit words at `ptr`, validated and copied
    let words = |ptr: u64, n: usize| -> Option<alloc::vec::Vec<u32>> {
        let bytes = (n as u64).checked_mul(4)?;
        if n == 0 || !user_read(ptr, bytes) {
            return None;
        }
        let b = copy_user_bytes(ptr, bytes as usize);
        (b.len() == bytes as usize).then(|| b.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect())
    };
    let r: Result<u64, &'static str> = match c.op {
        op::CREATE => crate::gfx3d::create(owner, c.a, c.b).map(|id| id as u64),
        op::FREE => {
            if crate::gfx3d::free(owner, c.target) { Ok(0) } else { Err("no such resource") }
        }
        op::CLEAR => crate::gfx3d::clear(owner, c.target, c.a, c.b).map(|_| 0),
        op::MESH => {
            let (nv, ni) = (c.a as usize, c.b as usize);
            if nv > crate::gfx3d::MAX_VERTICES || ni > crate::gfx3d::MAX_INDICES {
                return syserr::INVAL;
            }
            let vb = nv as u64 * core::mem::size_of::<bz_abi::Vertex3D>() as u64;
            if nv == 0 || !user_read(c.ptr, vb) {
                return syserr::INVAL;
            }
            let raw = copy_user_bytes(c.ptr, vb as usize);
            if raw.len() != vb as usize {
                return syserr::INVAL;
            }
            let v = raw.chunks_exact(40).map(crate::gfx3d::Vtx::from_bytes).collect();
            let idx = if ni == 0 { alloc::vec::Vec::new() } else { match words(c.ptr2, ni) { Some(w) => w, None => return syserr::INVAL } };
            crate::gfx3d::mesh(owner, v, idx).map(|id| id as u64)
        }
        op::TEXTURE => {
            if c.a == 0 || c.b == 0 || c.a > crate::gfx3d::MAX_DIM || c.b > crate::gfx3d::MAX_DIM {
                return syserr::INVAL;
            }
            match words(c.ptr, (c.a * c.b) as usize) {
                Some(px) => crate::gfx3d::texture(owner, c.a, c.b, px).map(|id| id as u64),
                None => return syserr::INVAL,
            }
        }
        op::LIGHTS => {
            let n = (c.len / 4).min((bz_abi::gfx3d_light::LIGHTS + bz_abi::gfx3d_light::STRIDE * bz_abi::gfx3d_light::MAX_LIGHTS) as u64) as usize;
            match words(c.ptr, n) {
                Some(b) => crate::gfx3d::set_lighting(owner, c.target, crate::gfx3d::Lighting::from_f32(&b)).map(|_| 0),
                None => return syserr::INVAL,
            }
        }
        op::DRAW => {
            let Some(m) = words(c.ptr, 32) else { return syserr::INVAL };
            let mut model = [0i64; 16];
            let mut vp = [0i64; 16];
            for i in 0..16 {
                model[i] = crate::gfx3d::fx(m[i]);
                vp[i] = crate::gfx3d::fx(m[16 + i]);
            }
            crate::gfx3d::draw(owner, c.target, c.a, c.b, c.c, c.d, model, vp).map(|(t, p)| (p as u64) << 32 | t as u64)
        }
        op::INFO => {
            if crate::gfx3d::exists(owner, c.target) { Ok(crate::gfx3d::on_gpu(owner, c.target) as u64) } else { Err("no such context") }
        }
        op::PRESENT => crate::gfx3d::present(owner, c.target, c.a, c.b as i32, c.c as i32).map(|_| 0),
        op::READ_PIXEL => crate::gfx3d::read_pixel(owner, c.target, c.a, c.b).map(|p| p as u64),
        op::READ => {
            let r = crate::gfx3d::with_color(owner, c.target, |w, h, px| {
                let need = w as u64 * h as u64 * 4;
                if c.len < need || !user_write(c.ptr, need) {
                    return false;
                }
                let out = c.ptr as *mut u32;
                for (i, p) in px.iter().enumerate() {
                    unsafe { out.add(i).write_volatile(*p) };
                }
                true
            });
            match r {
                Ok(true) => Ok(0),
                _ => Err("read refused"),
            }
        }
        _ => Err("unknown op"),
    };
    r.unwrap_or(syserr::INVAL)
}

/// USER_OP: accounts (see `bz_abi::user_op`, `users.rs`).
fn sys_user_op(op: u64, req_ptr: u64, out_ptr: u64) -> u64 {
    use bz_abi::user_op;
    if op == user_op::LIST {
        const CAP: u64 = 2048;
        if !user_write(out_ptr, CAP) {
            return syserr::INVAL;
        }
        let mut text = alloc::string::String::new();
        for (name, admin, avatar) in crate::users::list() {
            text.push_str(&alloc::format!("{}\t{}\t{}\n", name, admin as u8, avatar));
        }
        let bytes = &text.as_bytes()[..text.len().min(CAP as usize)];
        let out = out_ptr as *mut u8;
        for (i, &b) in bytes.iter().enumerate() {
            unsafe { out.add(i).write_volatile(b) };
        }
        return bytes.len() as u64;
    }
    let Some(req) = read_user_path_checked(req_ptr) else { return syserr::INVAL };
    let f: alloc::vec::Vec<&str> = req.split('\n').collect();
    let arg = |i: usize| f.get(i).copied().unwrap_or("");
    let r = match op {
        user_op::VERIFY => {
            if crate::users::verify(arg(0), arg(1)) { Ok(()) } else { Err("") }
        }
        user_op::ADD => crate::users::add(arg(0), arg(1), arg(2), arg(3), arg(4) == "1"),
        user_op::REMOVE => crate::users::remove(arg(0), arg(1), arg(2)),
        user_op::PASSWD => crate::users::change_password(arg(0), arg(1), arg(2)),
        user_op::RESET => crate::users::reset_password(arg(0), arg(1), arg(2), arg(3)),
        user_op::AVATAR => crate::users::set_avatar(arg(0), arg(1), arg(2), arg(3)),
        _ => Err(""),
    };
    if r.is_ok() { 0 } else { syserr::INVAL }
}

/// FS_OP: mkdir / remove / rename-move / stat (see `bz_abi::fs_op`).
fn sys_fs_op(op: u64, path_ptr: u64, a2: u64) -> u64 {
    use bz_abi::fs_op;
    let Some(path) = read_user_path_checked(path_ptr) else { return syserr::INVAL };
    let ok = |r: Result<(), &'static str>| match r {
        Ok(()) => 0,
        Err(_) => syserr::INVAL,
    };
    match op {
        fs_op::MKDIR => ok(crate::vfs::mkdir(&path)),
        fs_op::REMOVE => ok(crate::vfs::remove(&path)),
        fs_op::RENAME => {
            let Some(to) = read_user_path_checked(a2) else { return syserr::INVAL };
            ok(crate::vfs::rename(&path, &to))
        }
        fs_op::STAT => {
            if !user_write(a2, 32) {
                return syserr::INVAL;
            }
            let Ok(e) = crate::vfs::stat(&path) else { return syserr::INVAL };
            let out = a2 as *mut u64;
            unsafe {
                out.write_volatile(e.size as u64);
                out.add(1).write_volatile(e.is_dir as u64);
                out.add(2).write_volatile((e.date as u64) << 16 | e.time as u64);
                out.add(3).write_volatile(0);
            }
            0
        }
        _ => syserr::INVAL,
    }
}

fn sys_fs_read(path_ptr: u64, buf_ptr: u64, max: u64) -> u64 {
    if max == 0 {
        return syserr::INVAL;
    }
    if !user_write(buf_ptr, max) {
        return syserr::INVAL;
    }
    let path = match read_user_path_checked(path_ptr) {
        Some(p) => p,
        None => return syserr::INVAL,
    };
    let data = match crate::vfs::read(&path) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let n = data.len().min(max as usize);
    // Copy into user space per byte with write_volatile (never build a slice
    // over user memory - see the from_raw_parts Heisenbug note in CLAUDE.md).
    let dst = buf_ptr as *mut u8;
    for i in 0..n {
        unsafe { core::ptr::write_volatile(dst.add(i), data[i]) };
    }
    n as u64
}

/// RANDOM: fill the caller's buffer from the kernel DRBG.
fn sys_random(buf_ptr: u64, len: u64) -> u64 {
    if len == 0 || len > 4096 || !user_write(buf_ptr, len) {
        return syserr::INVAL;
    }
    let mut tmp = alloc::vec![0u8; len as usize];
    crate::rng::fill(&mut tmp);
    let dst = buf_ptr as *mut u8;
    for (i, b) in tmp.iter().enumerate() {
        unsafe { core::ptr::write_volatile(dst.add(i), *b) };
    }
    len
}

/// FS_WRITE: create/truncate a VFS file with the caller's bytes.
fn sys_fs_write(path_ptr: u64, buf_ptr: u64, len: u64) -> u64 {
    if len > 1024 * 1024 {
        return syserr::INVAL;
    }
    if !user_read(buf_ptr, len) {
        return syserr::INVAL;
    }
    let path = match read_user_path_checked(path_ptr) {
        Some(p) => p,
        None => return syserr::INVAL,
    };
    let data = copy_user_bytes(buf_ptr, len as usize);
    match crate::vfs::write(&path, &data) {
        Ok(()) => len,
        Err(_) => 0,
    }
}

/// CLOCK_RTC: read the CMOS real-time clock into an `abi::RtcTime`.
fn sys_clock_rtc(out_ptr: u64) -> u64 {
    if !user_write(out_ptr, core::mem::size_of::<bz_abi::RtcTime>() as u64) {
        return syserr::INVAL;
    }
    // local time: the RTC (UTC) + time zone + the user's correction
    let (year, month, day, hour, minute, second) = crate::clock::now();
    let out = out_ptr as *mut u64;
    unsafe {
        out.write_volatile(year as u64);
        out.add(1).write_volatile(month as u64);
        out.add(2).write_volatile(day as u64);
        out.add(3).write_volatile(hour as u64);
        out.add(4).write_volatile(minute as u64);
        out.add(5).write_volatile(second as u64);
    }
    0
}

// ---- networking (System.Net.Sockets) ------------------------------------

fn sys_net_socket(kind: u64) -> u64 {
    if kind != bz_abi::sock_kind::DGRAM {
        return syserr::INVAL; // TCP is not implemented yet
    }
    crate::net::udp_socket()
}

fn sys_net_bind(handle: u64, port: u64) -> u64 {
    if handle == 0 || port == 0 || port > 65535 {
        return syserr::INVAL;
    }
    match crate::net::udp_bind(handle, port as u16) {
        Ok(()) => 0,
        Err(_) => syserr::INVAL,
    }
}

fn sys_net_close(handle: u64) -> u64 {
    match crate::net::udp_close(handle) {
        Ok(()) => 0,
        Err(_) => syserr::INVAL,
    }
}

/// NET_SEND: the buffer is an `abi::NetDatagram` header followed by the payload.
fn sys_net_send(handle: u64, buf_ptr: u64, len: u64) -> u64 {
    if handle == 0 {
        return syserr::INVAL;
    }
    if len as usize > crate::net::UDP_MAX_PAYLOAD {
        return syserr::INVAL;
    }
    // Header + payload live in one caller-supplied buffer.
    if !user_read(buf_ptr, 16 + len) {
        return syserr::INVAL;
    }
    let header = copy_user_bytes(buf_ptr, 16);
    let dest = [header[0], header[1], header[2], header[3]];
    let port = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if port == 0 || port > 65535 {
        return syserr::INVAL;
    }
    let payload = copy_user_bytes(buf_ptr + 16, len as usize);
    match crate::net::udp_send(handle, dest, port as u16, &payload) {
        Ok(n) => {
            // Loopback: run the stack so the datagram lands in the peer's queue
            // before the caller polls for it.
            crate::net::poll();
            n as u64
        }
        Err(_) => 0,
    }
}

/// NET_RECV: fills the `abi::NetDatagram` header and the payload after it.
/// Returns the payload length, or 0 when nothing is queued (non-blocking).
/// SETTING: desktop settings (see `settings.rs`).
fn sys_setting(op: u64, k: u64, v: u64) -> u64 {
    use bz_abi::setting_op as o;
    match op {
        o::GET => crate::settings::get(k),
        o::SET => crate::settings::set(k, v),
        o::COUNT => crate::settings::count(k),
        o::NAME | o::DESCRIPTION => {
            if !user_write(v, 64) {
                return syserr::INVAL;
            }
            let Some(text) = crate::settings::label(k & 0xFFFF, (k >> 16) as usize, op == o::DESCRIPTION) else {
                return syserr::INVAL;
            };
            let b = text.as_bytes();
            let n = b.len().min(63);
            let out = v as *mut u8;
            for (i, c) in b.iter().take(n).enumerate() {
                unsafe { out.add(i).write_volatile(*c) };
            }
            unsafe { out.add(n).write_volatile(0) };
            n as u64
        }
        _ => syserr::INVAL,
    }
}

/// HW_I2C / HW_SPI / HW_UART: read the `HwTransfer`, copy `tx` in, run the
/// bus operation on kernel buffers, copy `rx` out (validated, volatile).
fn sys_hw_transfer(nr: u64, op: u64, t_ptr: u64) -> u64 {
    use bz_abi::{hw_uart, HwTransfer};
    const MAX: u64 = 4096;
    let size = core::mem::size_of::<HwTransfer>() as u64;
    if !user_read(t_ptr, size) {
        return syserr::INVAL;
    }
    let raw = copy_user_bytes(t_ptr, size as usize);
    if raw.len() != size as usize {
        return syserr::INVAL;
    }
    let u32_at = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]);
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().unwrap());
    let t = HwTransfer {
        bus: u32_at(0),
        addr: u32_at(4),
        speed_hz: u32_at(8),
        mode: u32_at(12),
        tx_ptr: u64_at(16),
        tx_len: u64_at(24),
        rx_ptr: u64_at(32),
        rx_len: u64_at(40),
    };
    if t.tx_len > MAX || t.rx_len > MAX {
        return syserr::INVAL;
    }
    let tx = if t.tx_len > 0 {
        if !user_read(t.tx_ptr, t.tx_len) {
            return syserr::INVAL;
        }
        copy_user_bytes(t.tx_ptr, t.tx_len as usize)
    } else {
        alloc::vec::Vec::new()
    };
    if t.rx_len > 0 && !user_write(t.rx_ptr, t.rx_len) {
        return syserr::INVAL;
    }
    let mut rx = alloc::vec![0u8; t.rx_len as usize];
    let r = match nr {
        sysno::HW_I2C => crate::hwio::i2c(t.bus, t.addr, t.speed_hz, &tx, &mut rx),
        sysno::HW_SPI => crate::hwio::spi(t.bus, t.addr, t.speed_hz, t.mode, &tx, &mut rx),
        _ => match op {
            hw_uart::OPEN => crate::hwio::uart_open(t.bus, t.speed_hz),
            hw_uart::CLOSE => crate::hwio::uart_close(t.bus),
            hw_uart::WRITE => crate::hwio::uart_write(t.bus, &tx),
            hw_uart::READ => crate::hwio::uart_read(t.bus, &mut rx),
            _ => syserr::INVAL,
        },
    };
    // Copy out what was read (all of rx for I2C/SPI, the count for UART READ).
    let n = if nr == sysno::HW_UART { if op == hw_uart::READ && r <= t.rx_len { r as usize } else { 0 } } else { rx.len() };
    let out = t.rx_ptr as *mut u8;
    for (i, b) in rx.iter().take(n).enumerate() {
        unsafe { out.add(i).write_volatile(*b) };
    }
    r
}

fn sys_net_recv(handle: u64, buf_ptr: u64, max: u64) -> u64 {
    if handle == 0 || max as usize > crate::net::UDP_MAX_PAYLOAD {
        return syserr::INVAL;
    }
    if !user_write(buf_ptr, 16 + max) {
        return syserr::INVAL;
    }
    crate::net::poll();
    let Some((addr, port, payload)) = crate::net::udp_recv(handle) else {
        return 0;
    };
    let n = payload.len().min(max as usize);
    let hdr = buf_ptr as *mut u8;
    unsafe {
        for i in 0..4 {
            hdr.add(i).write_volatile(addr[i]);
        }
        let p = (port as u32).to_le_bytes();
        for i in 0..4 {
            hdr.add(4 + i).write_volatile(p[i]);
        }
        let l = (n as u64).to_le_bytes();
        for i in 0..8 {
            hdr.add(8 + i).write_volatile(l[i]);
        }
        for i in 0..n {
            hdr.add(16 + i).write_volatile(payload[i]);
        }
    }
    n as u64
}

/// NET_INFO: address, link state and counters (`abi::NetInfo`).
fn sys_net_info(out_ptr: u64) -> u64 {
    if !user_write(out_ptr, core::mem::size_of::<bz_abi::NetInfo>() as u64) {
        return syserr::INVAL;
    }
    let ip = crate::net::local_ip();
    let (icmp, arp) = crate::net::counters();
    let (tx, rx) = crate::net::udp_counters();
    let bytes = out_ptr as *mut u8;
    unsafe {
        let a = ip.unwrap_or([0, 0, 0, 0]);
        for i in 0..4 {
            bytes.add(i).write_volatile(a[i]);
        }
        for i in 4..8 {
            bytes.add(i).write_volatile(0);
        }
        let out = out_ptr as *mut u64;
        out.add(1).write_volatile(ip.is_some() as u64);
        out.add(2).write_volatile(tx);
        out.add(3).write_volatile(rx);
        out.add(4).write_volatile(icmp);
        out.add(5).write_volatile(arp);
    }
    0
}

fn sys_pkg_list(buf_ptr: u64, max: u64) -> u64 {
    if max == 0 || max > 64 {
        return syserr::INVAL;
    }
    if !user_write(buf_ptr, max * core::mem::size_of::<bz_abi::PkgInfo>() as u64) {
        return syserr::INVAL;
    }
    let max = max as usize;
    // Fill a kernel buffer, then copy to user via volatile writes.
    let mut tmp: alloc::vec::Vec<bz_abi::PkgInfo> = alloc::vec::Vec::with_capacity(max);
    tmp.resize_with(max, || unsafe { core::mem::zeroed() });
    let n = crate::pkg::list(&mut tmp);
    let dst = buf_ptr as *mut bz_abi::PkgInfo;
    for i in 0..n {
        unsafe { core::ptr::write_volatile(dst.add(i), tmp[i]) };
    }
    n as u64
}

fn sys_pkg_set(name_ptr: u64, name_len: u64, action: u64) -> u64 {
    if name_len == 0 || name_len > 64 {
        return syserr::INVAL;
    }
    if !user_read(name_ptr, name_len) {
        return syserr::INVAL;
    }
    let bytes = copy_user_bytes(name_ptr, name_len as usize);
    let Ok(name) = core::str::from_utf8(&bytes) else {
        return syserr::INVAL;
    };
    let res = if action == 1 {
        crate::pkg::install(name).map(|_| ())
    } else {
        crate::pkg::remove(name)
    };
    match res {
        Ok(()) => 0,
        Err(_) => syserr::INVAL,
    }
}

fn sys_audio_stat(out_ptr: u64) -> u64 {
    if !user_write(out_ptr, core::mem::size_of::<bz_abi::AudioInfo>() as u64) {
        return syserr::INVAL;
    }
    let info = bz_abi::AudioInfo {
        present: if crate::audio::is_present() { 1 } else { 0 },
        sample_rate: crate::audio::SAMPLE_RATE as u64,
        channels: 2,
        bits: 16,
        volume: crate::audio::volume() as u64,
        muted: if crate::audio::is_muted() { 1 } else { 0 },
    };
    unsafe { core::ptr::write(out_ptr as *mut bz_abi::AudioInfo, info) };
    0
}

/// FUTEX_WAIT_TIMEOUT: block while the word at `addr` holds `expected`, at
/// most `timeout_ns` (0 = no limit). One wait, standard futex semantics: 0 on
/// a wake-up or a changed word (the caller re-checks its condition), 1 when
/// the time ran out.
fn sys_futex_wait_timeout(addr: u64, expected: u64, timeout_ns: u64) -> u64 {
    if addr % 4 != 0 || !user_read(addr, 4) {
        return syserr::INVAL;
    }
    let deadline = if timeout_ns == 0 { 0 } else { crate::arch::cpu::nanos().saturating_add(timeout_ns).max(1) };
    if !task::futex_wait_timeout_if(addr, expected as u32, deadline) {
        return 0;
    }
    if deadline != 0 && crate::arch::cpu::nanos() >= deadline {
        1
    } else {
        0
    }
}

fn sys_futex_wait(addr: u64, expected: u64) -> u64 {
    if addr % 4 != 0 || !user_read(addr, 4) {
        return syserr::INVAL;
    }
    // While the word still holds the expected value, block. The check and the
    // block are one step under the scheduler lock (no lost wake-up even
    // without the BKL).
    let mut spins: u64 = 0;
    loop {
        if !task::futex_wait_if(addr, expected as u32) {
            return 0;
        }
        spins += 1;
        if spins > 500_000_000 {
            return syserr::INVAL; // safety valve
        }
    }
}

fn sys_thread_create(rip: u64, arg: u64, user_stack_top: u64) -> u64 {
    if rip == 0 || user_stack_top == 0 {
        return syserr::INVAL;
    }
    // Give the main thread its own SYSCALL stack before a second thread exists.
    task::ensure_main_user_thread();
    task::spawn_user_thread(rip, arg, user_stack_top)
}

/// KEY_READ: with several programs alive, keystrokes belong to whoever has
/// focus — only the owner of the topmost window reads keys (when the terminal
/// has focus they stay with the terminal). A background process polling
/// KEY_READ gets 0 instead of stealing input.
fn sys_key_read() -> u64 {
    let me = crate::aspace::current_id();
    let key = if me != 0 && crate::wm::focus_owner() == me {
        crate::keyboard::pop().map(|c| c as u64).unwrap_or(0)
    } else {
        0
    };
    if key == 0 && me != 0 {
        // Nothing to read: give the CPU away rather than letting an app's
        // polling loop burn its whole time slice (every program runs on its
        // own kernel stack, so yielding inside the syscall is safe).
        task::yield_now();
    }
    key
}

/// PROC_SPAWN: start `path` (app name or VFS path) with `args` as a new
/// process. Strings are NUL-terminated user pointers (<= 255 bytes).
fn sys_proc_spawn(path_ptr: u64, args_ptr: u64) -> u64 {
    let Some(path) = read_user_path_checked(path_ptr) else {
        return syserr::INVAL;
    };
    let args = if args_ptr == 0 {
        alloc::string::String::new()
    } else {
        match read_user_path_checked(args_ptr) {
            Some(a) => a,
            None => return syserr::INVAL,
        }
    };
    match crate::app::spawn_with_args(&path, &args) {
        Ok(pid) => pid,
        Err(_) => syserr::INVAL,
    }
}

/// PROC_WAIT: sleep (Blocked state, woken by the exit in `process::app_exit`)
/// until `pid` exits, then return its exit code.
fn sys_proc_wait(pid: u64) -> u64 {
    if pid == 0 || pid == crate::aspace::current_id() {
        return syserr::INVAL; // waiting on yourself would never return
    }
    crate::app::wait_exit(pid).unwrap_or(syserr::INVAL)
}

/// PROC_ARGS: copy the caller's argument string (NUL-terminated if it fits).
fn sys_proc_args(buf_ptr: u64, max: u64) -> u64 {
    let args = crate::process::args(crate::aspace::current_id());
    if max > 0 {
        if !user_write(buf_ptr, max) {
            return syserr::INVAL;
        }
        let bytes = args.as_bytes();
        let n = bytes.len().min(max as usize - 1);
        let dst = buf_ptr as *mut u8;
        for (i, b) in bytes[..n].iter().enumerate() {
            unsafe { core::ptr::write_volatile(dst.add(i), *b) };
        }
        unsafe { core::ptr::write_volatile(dst.add(n), 0) };
    }
    args.len() as u64
}

fn sys_thread_join(tid: u64) -> u64 {
    // Cooperative wait: yield until the target thread has finished.
    let mut spins: u64 = 0;
    while !task::is_finished(tid) {
        task::yield_now();
        spins += 1;
        if spins > 500_000_000 {
            return syserr::INVAL; // safety valve against a wedged thread
        }
    }
    0
}

fn sys_proc_list(buf_ptr: u64, max: u64) -> u64 {
    if max == 0 || max > 256 {
        return syserr::INVAL;
    }
    if !user_write(buf_ptr, max * core::mem::size_of::<bz_abi::ProcInfo>() as u64) {
        return syserr::INVAL;
    }
    // Fill a kernel buffer, then copy to user via volatile writes (never hand a
    // &mut slice over user memory to safe code — same Heisenbug class as reads).
    let max = max as usize;
    let mut tmp: alloc::vec::Vec<bz_abi::ProcInfo> = alloc::vec::Vec::with_capacity(max);
    tmp.resize_with(max, || unsafe { core::mem::zeroed() });
    let n = crate::process::list(&mut tmp);
    let dst = buf_ptr as *mut bz_abi::ProcInfo;
    for i in 0..n {
        unsafe { core::ptr::write_volatile(dst.add(i), tmp[i]) };
    }
    n as u64
}

fn sys_stat(out_ptr: u64) -> u64 {
    if !user_write(out_ptr, core::mem::size_of::<bz_abi::SysStat>() as u64) {
        return syserr::INVAL;
    }
    let stat = crate::process::stat();
    unsafe { core::ptr::write(out_ptr as *mut bz_abi::SysStat, stat) };
    0
}

fn sys_win_create(title_ptr: u64, title_len: u64, dims: u64) -> u64 {
    if title_len == 0 || title_len > 128 {
        return 0;
    }
    if !user_read(title_ptr, title_len) {
        return 0;
    }
    let bytes = copy_user_bytes(title_ptr, title_len as usize);
    let Ok(title) = core::str::from_utf8(&bytes) else {
        return 0;
    };
    let client = dims & bz_abi::win_flags::CLIENT_SIZE != 0;
    let (mut w, mut h) = (((dims >> 32) & 0x7FFF_FFFF) as i32, (dims & 0xFFFF_FFFF) as i32);
    if client {
        (w, h) = crate::wm::outer_size(w, h);
    }
    let w = w.clamp(80, 2000);
    let h = h.clamp(60, 2000);
    crate::wm::create_app_window(title, w, h) as u64
}

fn sys_win_cmd(win_id: u64, cmd_ptr: u64) -> u64 {
    if !user_read(cmd_ptr, core::mem::size_of::<bz_abi::DrawCmd>() as u64) {
        return syserr::INVAL;
    }
    let cmd = unsafe { core::ptr::read(cmd_ptr as *const bz_abi::DrawCmd) };
    if cmd.op == bz_abi::draw_op::BLIT {
        // text_ptr = client ARGB buffer (w*h u32), placed at (x, y).
        if cmd.text_ptr == 0 || cmd.w <= 0 || cmd.h <= 0 {
            return syserr::INVAL;
        }
        let count = (cmd.w as u64) * (cmd.h as u64);
        if count == 0 || count > 8_000_000 || cmd.text_len < count * 4 {
            return syserr::INVAL;
        }
        // The compositor reads count*4 bytes straight out of this pointer.
        if !user_read(cmd.text_ptr, count * 4) {
            return syserr::INVAL;
        }
        return match crate::wm::blit_on_window(
            win_id as u32,
            cmd.x,
            cmd.y,
            cmd.w,
            cmd.h,
            cmd.text_ptr,
        ) {
            Ok(()) => 0,
            Err(_) => syserr::INVAL,
        };
    }
    let has_text = cmd.op == bz_abi::draw_op::DRAW_TEXT || cmd.op == bz_abi::draw_op::DRAW_TEXT_STYLED;
    let text_buf = if has_text {
        if cmd.text_len == 0 || cmd.text_len > 4096 {
            return syserr::INVAL;
        }
        if !user_read(cmd.text_ptr, cmd.text_len) {
            return syserr::INVAL;
        }
        copy_user_bytes(cmd.text_ptr, cmd.text_len as usize)
    } else {
        alloc::vec::Vec::new()
    };
    let text = if has_text {
        match core::str::from_utf8(&text_buf) {
            Ok(s) => Some(s),
            Err(_) => return syserr::INVAL,
        }
    } else {
        None
    };
    match crate::wm::draw_on_window(win_id as u32, &cmd, text) {
        Ok(()) => 0,
        Err(_) => syserr::INVAL,
    }
}

fn sys_debug_write(ptr: u64, len: u64) -> u64 {
    if len == 0 || len > 64 * 1024 {
        return syserr::INVAL;
    }
    // Without this an app could point DEBUG_WRITE at kernel memory and have the
    // kernel dump it to the serial log — a straight information leak.
    if !user_read(ptr, len) {
        return syserr::INVAL;
    }
    let bytes = copy_user_bytes(ptr, len as usize);
    match core::str::from_utf8(&bytes) {
        Ok(s) => {
            print!("{}", s);
            len
        }
        Err(_) => syserr::INVAL,
    }
}

fn sys_exit(code: u64) -> u64 {
    if crate::usermode::user_active() {
        crate::usermode::exit_user(code);
    }
    crate::println!("[kernel] task exited with code {}", code);
    task::exit_current()
}

fn sys_fb_info(out_ptr: u64) -> u64 {
    if !user_write(out_ptr, core::mem::size_of::<FramebufferInfo>() as u64) {
        return syserr::INVAL;
    }
    let Some(console) = framebuffer::CONSOLE.get() else {
        return syserr::INVAL;
    };
    let mut console = console.lock();
    let info = console.info();
    let (addr, size) = console.buffer_addr();
    let fb = FramebufferInfo {
        address: addr,
        size,
        width: info.width as u64,
        height: info.height as u64,
        stride: info.stride as u64,
        bytes_per_pixel: info.bytes_per_pixel as u64,
        pixel_format: match info.format {
            crate::boot::PixelFormat::Rgb => pixel_format::RGB,
            crate::boot::PixelFormat::Bgr => pixel_format::BGR,
            crate::boot::PixelFormat::Gray => pixel_format::GRAY,
            crate::boot::PixelFormat::Unknown => pixel_format::UNKNOWN,
        },
    };
    unsafe { core::ptr::write(out_ptr as *mut FramebufferInfo, fb) };
    0
}
