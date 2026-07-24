//! Syscall ABI v1 dispatcher (requirements.md §4, §10.1).
//!
//! The numbered contract lives in the shared `bz-abi` crate and is mirrored
//! by `Buitenzorg.Runtime` (C#). Today the dispatcher is exercised from
//! kernel context; wiring it to a ring-3 `syscall` instruction entry point is
//! the remaining v0.2/v0.4 work tracked in requirements.md §17.

use bz_abi::{pixel_format, syserr, sysno, FramebufferInfo, ABI_VERSION};

use crate::{framebuffer, interrupts, print, task};

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

/// True while a syscall issued from ring 3 is being serviced. Pointer arguments
/// are only untrusted on that path: the kernel also calls [`dispatch`] directly
/// (boot self-tests, the shell) and legitimately passes its own addresses.
///
/// Single-core and cooperative, so a plain atomic flag is enough — a syscall is
/// never preempted by another syscall on this scheduler.
static IN_USER_SYSCALL: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Entry point for ring-3 syscalls: marks the call as untrusted, so the
/// pointer-validating helpers below actually check.
pub fn dispatch_from_user(nr: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    use core::sync::atomic::Ordering;
    // Profiler zone: total time spent servicing ring-3 syscalls. Inert unless
    // profiling is enabled.
    let _z = crate::profile::Guard::new("syscall");
    let prev = IN_USER_SYSCALL.swap(true, Ordering::SeqCst);
    let r = dispatch(nr, a0, a1, a2);
    IN_USER_SYSCALL.store(prev, Ordering::SeqCst);
    r
}

fn from_user() -> bool {
    IN_USER_SYSCALL.load(core::sync::atomic::Ordering::SeqCst)
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
            crate::wm::present_now();
            0
        }
        sysno::KEY_READ => crate::keyboard::pop().map(|c| c as u64).unwrap_or(0),
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
        sysno::FUTEX_WAKE => task::futex_wake(a0, a1),
        sysno::THREAD_SELF => task::current_id(),
        sysno::CLOCK_MONO => unsafe { core::arch::x86_64::_rdtsc() },
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
    // Empty path => list the mounts (as directories); else list the directory.
    let (names, is_dir) = if path.is_empty() {
        (crate::vfs::mounts(), true)
    } else {
        match crate::vfs::list(&path) {
            Ok(v) => (v, false),
            Err(_) => return 0,
        }
    };
    let dst = buf_ptr as *mut bz_abi::FsEntry;
    let mut n = 0usize;
    for name in names.iter() {
        if n >= max as usize {
            break;
        }
        // Mount names come back as "/disk"; strip the leading slash for display.
        let disp = name.strip_prefix('/').unwrap_or(name);
        let mut e = bz_abi::FsEntry { name: [0; 24], is_dir: if is_dir { 1 } else { 0 } };
        let b = disp.as_bytes();
        let m = b.len().min(23);
        e.name[..m].copy_from_slice(&b[..m]);
        unsafe { core::ptr::write_volatile(dst.add(n), e) };
        n += 1;
    }
    n as u64
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
    let (year, month, day, hour, minute, second) = crate::rtc::read();
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

fn sys_futex_wait(addr: u64, expected: u64) -> u64 {
    if addr % 4 != 0 || !user_read(addr, 4) {
        return syserr::INVAL;
    }
    // Cooperative wait: while the word still holds the expected value, block.
    let mut spins: u64 = 0;
    loop {
        let val = unsafe { core::ptr::read_volatile(addr as *const u32) } as u64;
        if val != expected {
            return 0;
        }
        task::futex_wait_block(addr);
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
    let w = ((dims >> 32) as i32).clamp(80, 2000);
    let h = ((dims & 0xFFFF_FFFF) as i32).clamp(60, 2000);
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
    let text_buf = if cmd.op == bz_abi::draw_op::DRAW_TEXT {
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
    let text = if cmd.op == bz_abi::draw_op::DRAW_TEXT {
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
        pixel_format: match info.pixel_format {
            bootloader_api::info::PixelFormat::Rgb => pixel_format::RGB,
            bootloader_api::info::PixelFormat::Bgr => pixel_format::BGR,
            bootloader_api::info::PixelFormat::U8 => pixel_format::GRAY,
            _ => pixel_format::UNKNOWN,
        },
    };
    unsafe { core::ptr::write(out_ptr as *mut FramebufferInfo, fb) };
    0
}
