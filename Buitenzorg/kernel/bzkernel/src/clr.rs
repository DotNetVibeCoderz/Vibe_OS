//! The managed C# path: **RustCLR** (the runtime of RustNetRuntime) as a
//! ring-3 program, `/disk/CLRHOST.ELF` (built from `userland/clrhost` for each
//! architecture). C# on Buitenzorg now has two paths:
//!
//! - **native** -- bflat / NativeAOT (`--stdlib:zero` + the `bzstart.rs` shim):
//!   machine code, fast, but no garbage collector and none of what needs one
//!   (static reference fields, delegates, `object[]`, ...);
//! - **managed** -- a plain `dotnet build` assembly (`.dll`), interpreted by
//!   RustCLR with a tracing GC and its BCL (generics, LINQ, exceptions,
//!   strings, collections, ...). Architecture-neutral: the same `.dll` runs on
//!   x86-64, ARM64 and RISC-V -- which is how C# reaches RISC-V, where bflat
//!   has no target.
//!
//! Generic code: it only spawns a process, so it is shared by all three
//! kernels.

use alloc::format;

/// The host program on /disk.
pub const HOST: &str = "/disk/CLRHOST.ELF";

/// Run the assembly at `dll` (a VFS path) with `args` as a managed process
/// and wait for it. Its exit code, or an error if it could not start.
pub fn run(dll: &str, args: &str) -> Result<u64, &'static str> {
    let line = if args.is_empty() { alloc::string::String::from(dll) } else { format!("{} {}", dll, args) };
    let pid = crate::app::spawn_with_args(HOST, &line)?;
    Ok(crate::app::wait(pid))
}

/// True when the managed runtime is installed (`/disk/CLRHOST.ELF`).
pub fn available() -> bool {
    crate::vfs::read(HOST).is_ok()
}

/// Boot self-test of the managed path: `HELLOCLR.DLL` (strings, `object[]`,
/// interfaces, LINQ + lambdas, exceptions, a GC that reclaims, a static
/// reference field -- each something the native path cannot do) must pass
/// all its checks, then RustCLR's own modern-syntax suite runs (`MODERN.DLL`;
/// it prints `checks=35 failures=0`, which the smoke test requires).
/// RustCLR's main conformance suite is not run yet: it starts real threads
/// (`Thread`, `Monitor.Wait`, `Task.Run`), which RustCLR only provides with
/// `std` -- the host's threads (THREAD_CREATE + futexes) come next.
pub fn boot_demo(arch: &str) {
    if !available() {
        crate::println!("[clr] managed runtime not installed ({} missing: scripts/build-clrhost)", HOST);
        return;
    }
    let t0 = crate::interrupts::ticks();
    match run("/disk/HELLOCLR.DLL", "") {
        Ok(0) => crate::println!(
            "[kernel] MILESTONE: CLR OK (managed C# on {}: a dotnet-built assembly run by RustCLR in ring 3 -- GC, generics, LINQ, exceptions)",
            arch
        ),
        Ok(code) => crate::println!("[clr] HELLOCLR.DLL failed (exit {:#x})", code),
        Err(e) => crate::println!("[clr] cannot start {}: {}", HOST, e),
    }
    for dll in ["/disk/MODERN.DLL"] {
        if crate::vfs::read(dll).is_err() {
            continue;
        }
        match run(dll, "") {
            Ok(code) => crate::println!("[clr] {} exited with {}", dll, code),
            Err(e) => crate::println!("[clr] {}: {}", dll, e),
        }
    }
    let secs10 = (crate::interrupts::ticks() - t0) * 10 / 18;
    crate::println!("[clr] managed-runtime demo took {}.{} s", secs10 / 10, secs10 % 10);
}
