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

/// Start the assembly at `dll` (a VFS path) with `args` as a managed
/// process: `CLRHOST.ELF <dll> <args>`. Its pid.
pub fn spawn(dll: &str, args: &str) -> Result<u64, &'static str> {
    let line = if args.is_empty() { alloc::string::String::from(dll) } else { format!("{} {}", dll, args) };
    // The process is named after the assembly (`/disk/MCALC.DLL` -> `mcalc`),
    // not the host, so `ps`, the task managers and PROC_LIST tell apps apart.
    let file = dll.rsplit('/').next().unwrap_or(dll);
    let name = file.split('.').next().unwrap_or(file).to_lowercase();
    let image = crate::vfs::read(HOST).map_err(|_| "managed runtime (CLRHOST.ELF) not found")?;
    crate::app::spawn_image_args(&name, &image, &line)
}

/// Run the assembly at `dll` (a VFS path) with `args` as a managed process
/// and wait for it. Its exit code, or an error if it could not start.
pub fn run(dll: &str, args: &str) -> Result<u64, &'static str> {
    let pid = spawn(dll, args)?;
    Ok(crate::app::wait(pid))
}

/// The managed assembly a launch target names, if it names one:
/// `"/disk/APP.DLL"` / `"app.dll"` (any `.dll` path), or an app name whose
/// 8.3 file `/disk/<NAME>.DLL` exists (`"hellogui"` -> `/disk/HELLOGUI.DLL`).
/// That is how `run <name>`, the Start menu and PROC_SPAWN reach managed apps
/// with the same names native apps use.
pub fn dll_for(target: &str) -> Option<alloc::string::String> {
    let lower = target.to_ascii_lowercase();
    if lower.ends_with(".dll") {
        return Some(if target.starts_with('/') {
            alloc::string::String::from(target)
        } else {
            format!("/disk/{}", target.to_ascii_uppercase())
        });
    }
    if target.starts_with('/') || target.contains('.') || target.is_empty() {
        return None;
    }
    let stem: alloc::string::String = target.chars().take(8).collect::<alloc::string::String>().to_ascii_uppercase();
    let path = format!("/disk/{}.DLL", stem);
    (available() && crate::vfs::read(&path).is_ok()).then_some(path)
}

/// True when the managed runtime is installed (`/disk/CLRHOST.ELF`).
pub fn available() -> bool {
    crate::vfs::read(HOST).is_ok()
}

/// Boot self-test of the managed path: `HELLOCLR.DLL` (strings, `object[]`,
/// interfaces, LINQ + lambdas, exceptions, a GC that reclaims, a static
/// reference field -- each something the native path cannot do) must pass
/// all its checks, then RustCLR's own suites run: `CONFORM.DLL` (361 checks:
/// real threads, `lock`/`Monitor.Wait`/`Pulse`, `Interlocked`, `Task.Run`,
/// `Parallel`, `async`/`await`, collection under several threads, ...) and
/// `MODERN.DLL` (35 checks). They print `checks=N failures=0`, which the smoke
/// test requires.
pub fn boot_demo(arch: &str) {
    if !available() {
        crate::println!("[clr] managed runtime not installed ({} missing: scripts/build-clrhost)", HOST);
        return;
    }
    let t0 = crate::interrupts::ticks();
    // A managed GUI app, started by *name* the way the shell's `run` and the
    // Start menu do (`hellogui` -> /disk/HELLOGUI.DLL, which references
    // /disk/BZAPP.DLL): a window drawn through Buitenzorg.App's P/Invokes.
    if crate::vfs::read("/disk/HELLOGUI.DLL").is_ok() {
        match crate::app::spawn_with_args("hellogui", "--selftest").map(crate::app::wait) {
            Ok(0) => crate::println!(
                "[kernel] MILESTONE: CLR GUI OK (a managed C# app on {} drew a window through Buitenzorg.App: P/Invoke -> CLRHOST.ELF -> WIN_* syscalls; launched by name)",
                arch
            ),
            Ok(code) => crate::println!("[clr] hellogui failed (exit {:#x})", code),
            Err(e) => crate::println!("[clr] cannot start hellogui: {}", e),
        }
    }
    // The managed app suite (Buitenzorg.UI): the "Aplikasi Managed" folder
    // renders a real window with the toolkit's draw ops, then runs every
    // app's own self-test in the same process (it prints MANAGED UI OK and
    // MANAGED APPS OK itself).
    if crate::vfs::read("/disk/MANAGED.DLL").is_ok() {
        let m0 = crate::interrupts::ticks();
        match crate::app::spawn_with_args("managed", "--selftest").map(crate::app::wait) {
            Ok(0) => {
                let s10 = (crate::interrupts::ticks() - m0) * 10 / 18;
                crate::println!("[clr] managed app suite checked in {}.{} s", s10 / 10, s10 % 10);
            }
            Ok(code) => crate::println!("[clr] managed app suite: {} app(s) failed their self-test", code),
            Err(e) => crate::println!("[clr] cannot start the managed app suite: {}", e),
        }
    }
    match run("/disk/HELLOCLR.DLL", "") {
        Ok(0) => crate::println!(
            "[kernel] MILESTONE: CLR OK (managed C# on {}: a dotnet-built assembly run by RustCLR in ring 3 -- GC, generics, LINQ, exceptions)",
            arch
        ),
        Ok(code) => crate::println!("[clr] HELLOCLR.DLL failed (exit {:#x})", code),
        Err(e) => crate::println!("[clr] cannot start {}: {}", HOST, e),
    }
    for dll in ["/disk/CONFORM.DLL", "/disk/MODERN.DLL"] {
        if crate::vfs::read(dll).is_err() {
            continue;
        }
        // By .dll path, as `run /disk/X.DLL` does.
        match crate::app::spawn_with_args(dll, "").map(crate::app::wait) {
            Ok(code) => crate::println!("[clr] {} exited with {}", dll, code),
            Err(e) => crate::println!("[clr] {}: {}", dll, e),
        }
    }
    let secs10 = (crate::interrupts::ticks() - t0) * 10 / 18;
    crate::println!("[clr] managed-runtime demo took {}.{} s", secs10 / 10, secs10 % 10);
}
