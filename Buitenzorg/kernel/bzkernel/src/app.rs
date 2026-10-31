//! App launcher (v0.8 "Kembang"): load a desktop app (ELF from the VFS) and
//! run it in ring 3. Apps draw their UI through the window syscalls
//! (WIN_CREATE / WIN_CMD / WIN_PRESENT) and read input via KEY_READ.
//!
//! Uses the global memory context so it can be invoked from the shell after
//! boot. Every program gets its own address space (`aspace.rs`), so several
//! can be resident at once. Two ways to launch:
//! - [`run_named`]: synchronous — the program runs on the calling task and the
//!   call returns its exit code (the boot demos, which check results in order).
//! - [`spawn_named`]: a separate, preemptively scheduled process — the call
//!   returns its pid at once (the desktop and the shell's `run`).

use alloc::vec::Vec;

use crate::{aspace, elf, memory, process, task, usermode, vfs};

const USER_STACK_BASE: u64 = 0x7000_0000;
const USER_STACK_PAGES: u64 = 32; // 128 KiB (apps use more stack than hello)

/// Exit code recorded for a program terminated with PROC_KILL.
pub const KILLED_EXIT_CODE: u64 = 0xDEAD_0001;

/// Create address space `pid`, load `image` plus a user stack into it, and
/// return (space, entry, stack_top). The new space is only *installed* while
/// loading — it is not made the caller's space, so the loading time is not
/// charged to the new program — and the caller's space is reinstalled after.
/// Safe because kernel code is never preempted and loading never yields.
fn load_program(pid: u64, image: &[u8]) -> Result<(aspace::Space, u64, u64), &'static str> {
    let space = aspace::create(pid)?;
    let prev = task::current_space();
    aspace::install(space);
    let loaded = memory::with_ctx(|ctx| {
        let program = elf::load(image, &mut ctx.mapper, &mut ctx.frames)?;
        let (stack_top, _) = memory::map_user_region(
            USER_STACK_BASE,
            USER_STACK_PAGES,
            &mut ctx.mapper,
            &mut ctx.frames,
        )?;
        Ok::<_, &'static str>((program.entry, stack_top))
    });
    aspace::install(prev);
    match loaded {
        Ok((entry, stack_top)) => Ok((space, entry, stack_top)),
        Err(e) => {
            aspace::destroy(pid);
            Err(e)
        }
    }
}

/// Load and run an app image on the calling task, in its own address space.
/// Returns its exit code.
pub fn run_image(name: &str, image: &[u8]) -> Result<u64, &'static str> {
    let pid = process::app_start(name, false);
    let (space, entry, stack_top) = match load_program(pid, image) {
        Ok(v) => v,
        Err(e) => {
            process::app_exit(pid, u64::MAX);
            return Err(e);
        }
    };
    let prev = task::current_space();
    task::set_current_space(space);

    let code = usermode::enter_user(entry, stack_top);

    // Tear down any user threads the app spawned, restore the launcher's
    // default (TSS) syscall stack and address space, then drop the app's.
    task::terminate_user_threads();
    task::set_current_space(prev);
    aspace::destroy(pid);
    process::app_exit(pid, code);
    Ok(code)
}

/// Start `image` as its own preemptively scheduled process. Returns its pid.
pub fn spawn_image(name: &str, image: &[u8]) -> Result<u64, &'static str> {
    spawn_image_at(name, image, None)
}

/// Like [`spawn_image`], optionally entering at `entry_override` instead of
/// the ELF entry — the boot self-test uses an unmapped address to prove that
/// a faulting program is terminated without taking the kernel down.
pub fn spawn_image_at(
    name: &str,
    image: &[u8],
    entry_override: Option<u64>,
) -> Result<u64, &'static str> {
    let pid = process::app_start(name, true);
    match load_program(pid, image) {
        Ok((space, entry, stack_top)) => {
            task::spawn_process(entry_override.unwrap_or(entry), stack_top, space);
            Ok(pid)
        }
        Err(e) => {
            process::app_exit(pid, u64::MAX);
            Err(e)
        }
    }
}

/// Called on a spawned process's own task once its program has exited (EXIT
/// syscall or a fatal fault): stop its other threads, leave and drop its
/// address space, and record the exit code.
pub fn process_exited(pid: u64, code: u64) {
    task::detach_current_from_space(pid);
    aspace::destroy(pid);
    process::app_exit(pid, code);
    crate::println!("[app] process {} exited with code {:#x}", pid, code);
}

/// Terminate spawned process `pid` from outside (PROC_KILL / Task Manager).
pub fn kill(pid: u64) -> bool {
    if !task::kill_space(pid) {
        return false;
    }
    aspace::destroy(pid);
    process::app_exit(pid, KILLED_EXIT_CODE);
    crate::println!("[app] process {} killed", pid);
    true
}

/// Terminate spawned processes whose last window the user just closed — an
/// interactive app (editor, file manager) otherwise keeps running invisibly,
/// polling for keys. Called from the desktop loop.
pub fn reap_closed_windows() {
    for pid in crate::wm::take_closed_owners() {
        if process::is_detached(pid) && !crate::wm::has_windows(pid) {
            kill(pid);
        }
    }
}

/// Read a named app's ELF from `/disk`.
fn read_app(name: &str) -> Result<Vec<u8>, &'static str> {
    let file = app_file(name).ok_or("unknown app")?;
    let path = alloc::format!("/disk/{}", file);
    vfs::read(&path).map_err(|_| "app not found on /disk")
}

/// Resolve an app name to a VFS path and run it synchronously. Known apps live
/// on `/disk` as uppercase 8.3 ELF files (e.g. "xox" -> /disk/XOX.ELF).
pub fn run_named(name: &str) -> Result<u64, &'static str> {
    let bytes = read_app(name)?;
    run_image(name, &bytes)
}

/// Resolve an app name and start it as its own process. Returns its pid.
pub fn spawn_named(name: &str) -> Result<u64, &'static str> {
    spawn_with_args(name, "")
}

/// Start a program with an argument string. `target` is an app name
/// (`"calc"`) or an absolute VFS path to an ELF (`"/disk/MYAPP.ELF"`).
pub fn spawn_with_args(target: &str, args: &str) -> Result<u64, &'static str> {
    let (name, bytes) = if target.starts_with('/') {
        let file = target.rsplit('/').next().unwrap_or(target);
        let name = file.split('.').next().unwrap_or(file);
        (alloc::string::String::from(name).to_lowercase(), vfs::read(target).map_err(|_| "file not found")?)
    } else {
        (alloc::string::String::from(target), read_app(target)?)
    };
    // The child cannot run before this returns (kernel code is never preempted
    // and spawning does not yield), so its first PROC_ARGS sees these args.
    let pid = spawn_image(&name, &bytes)?;
    process::set_args(pid, args);
    Ok(pid)
}

/// Map a short app name to its 8.3 ELF filename.
fn app_file(name: &str) -> Option<&'static str> {
    match name {
        "xox" | "tictactoe" => Some("XOX.ELF"),
        "hello" => Some("HELLO.ELF"),
        "svc" => Some("SVC.ELF"),
        "taskmgr" | "monitor" => Some("TASKMGR.ELF"),
        "paint" | "draw" => Some("PAINT.ELF"),
        "widget" => Some("WIDGET.ELF"),
        "web" | "webview" => Some("WEBVIEW.ELF"),
        "matang" => Some("MATANG.ELF"),
        "thread" => Some("THREAD.ELF"),
        "sync" => Some("SYNC.ELF"),
        "heap" => Some("HEAP.ELF"),
        "gcmem" => Some("GCMEM.ELF"),
        "bcl" => Some("BCL.ELF"),
        "bcl2" => Some("BCL2.ELF"),
        "drawing" | "gfx" => Some("DRAW.ELF"),
        "ui" => Some("UI.ELF"),
        "audio" | "sound" => Some("AUDIO.ELF"),
        "audiopanel" | "sound-settings" => Some("AUDIOSET.ELF"),
        "calc" | "calculator" => Some("CALC.ELF"),
        "2048" | "game2048" | "game" => Some("G2048.ELF"),
        "clock" | "jam" => Some("CLOCK.ELF"),
        "piano" | "music" => Some("PIANO.ELF"),
        "store" | "appstore" => Some("STORE.ELF"),
        "files" | "filemanager" | "fm" => Some("FILES.ELF"),
        "editor" | "notepad" | "edit" => Some("EDITOR.ELF"),
        "imgview" | "image" | "viewer" | "photo" => Some("IMGVIEW.ELF"),
        "jpgtest" | "jpg" | "jpeg" => Some("JPGTEST.ELF"),
        "mproc" => Some("MPROC.ELF"),
        "proctest" => Some("PROCTEST.ELF"),
        // Generic slot for a user-authored app: the SDK/VS Code/MagicAppGen
        // template builds its ELF to userland/hello-csharp/userapp.elf, which
        // build.rs embeds as /disk/USERAPP.ELF. `run myapp` then launches it.
        "myapp" | "userapp" | "app" => Some("USERAPP.ELF"),
        _ => None,
    }
}

/// True if `name` is a launchable app.
pub fn is_app(name: &str) -> bool {
    app_file(name).is_some()
}
