//! App launcher (v0.8 "Kembang"): load a desktop app (ELF from the VFS) and
//! run it in ring 3. Apps draw their UI through the window syscalls
//! (WIN_CREATE / WIN_CMD / WIN_PRESENT) and read input via KEY_READ.
//!
//! Uses the global memory context so it can be invoked from the shell after
//! boot. Every program runs as its own preemptively scheduled process, in its
//! own address space (`aspace.rs`), on its own kernel stack. Two ways to start
//! one:
//! - [`spawn_named`] / [`spawn_with_args`]: returns the pid at once (the
//!   desktop, the shell's `run`, PROC_SPAWN).
//! - [`run_named`]: spawn, then block until it exits and return its exit code
//!   (the boot demos, which check results in order). The program is a normal
//!   process either way — it used to run on the launcher's own task, sharing
//!   the boot task's privilege stack and impossible to kill.

use alloc::vec::Vec;

use crate::{aspace, elf, memory, process, task, vfs};

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
        let program = elf::load(image, ctx)?;
        let (stack_top, _) = memory::map_user_region(USER_STACK_BASE, USER_STACK_PAGES, ctx)?;
        Ok::<_, &'static str>((program.entry, stack_top))
    });
    aspace::install(prev);
    match loaded {
        Ok((entry, stack_top)) => Ok((space, entry, stack_top)),
        Err(e) => {
            aspace::destroy(space);
            Err(e)
        }
    }
}

/// Block the calling task until process `pid` exits; return its exit code
/// (`None` if it is unknown). Sleeps in the scheduler's Blocked state, woken
/// by `process::app_exit`. The "still alive?" check and the block are one step
/// under the scheduler lock (`task::block_while`), and `app_exit` unregisters
/// the child *before* its wake-up takes that lock — so on SMP, with the child
/// exiting on another CPU at the same moment, the wake-up cannot be lost.
pub fn wait_exit(pid: u64) -> Option<u64> {
    while task::block_while(process::wait_key(pid), || process::is_alive(pid)) {}
    process::exit_code(pid)
}

/// [`wait_exit`] for kernel callers (boot demos): `u64::MAX` if unknown.
pub fn wait(pid: u64) -> u64 {
    wait_exit(pid).unwrap_or(u64::MAX)
}

/// Start `image` as its own preemptively scheduled process. Returns its pid.
pub fn spawn_image(name: &str, image: &[u8]) -> Result<u64, &'static str> {
    spawn_image_at(name, image, None)
}

/// [`spawn_image`] with an argument string, visible to the program's first
/// PROC_ARGS. The arguments are recorded *before* its task can be scheduled:
/// on SMP another CPU may start running the child the moment it is spawned.
pub fn spawn_image_args(name: &str, image: &[u8], args: &str) -> Result<u64, &'static str> {
    spawn_image_full(name, image, None, args)
}

/// Like [`spawn_image`], optionally entering at `entry_override` instead of
/// the ELF entry — the boot self-test uses an unmapped address to prove that
/// a faulting program is terminated without taking the kernel down.
pub fn spawn_image_at(
    name: &str,
    image: &[u8],
    entry_override: Option<u64>,
) -> Result<u64, &'static str> {
    spawn_image_full(name, image, entry_override, "")
}

fn spawn_image_full(
    name: &str,
    image: &[u8],
    entry_override: Option<u64>,
    args: &str,
) -> Result<u64, &'static str> {
    let pid = process::app_start(name);
    process::set_args(pid, args);
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
    let space = task::current_space();
    task::detach_current_from_space(pid);
    destroy_when_home(space);
    process::app_exit(pid, code);
    crate::println!("[app] process {} exited with code {:#x}", pid, code);
}

/// Free a dead process's address space once no other CPU still runs one of
/// its threads (they were marked finished; a CPU drops a running one within a
/// tick, or at the end of its syscall). On timeout the space is leaked, never
/// freed under another CPU's feet.
fn destroy_when_home(space: aspace::Space) {
    // 3D contexts, meshes and textures do not live in the address space
    crate::gfx3d::release(space.id);
    if task::wait_space_stopped(space.id) {
        aspace::destroy(space);
    } else {
        crate::println!("[app] process {}: a thread did not stop on another CPU -- address space leaked", space.id);
    }
}

/// Terminate spawned process `pid` from outside (PROC_KILL / Task Manager).
pub fn kill(pid: u64) -> bool {
    let Some(space) = task::kill_space(pid) else {
        return false;
    };
    destroy_when_home(space);
    process::app_exit(pid, KILLED_EXIT_CODE);
    crate::println!("[app] process {} killed", pid);
    true
}

/// Terminate spawned processes whose last window the user just closed — an
/// interactive app (editor, file manager) otherwise keeps running invisibly,
/// polling for keys. Called from the desktop loop.
pub fn reap_closed_windows() {
    for pid in crate::wm::take_closed_owners() {
        if process::is_alive(pid) && !crate::wm::has_windows(pid) {
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

/// Run a named app as a process and wait for it; returns its exit code. Known
/// apps live on `/disk` as uppercase 8.3 ELF files (e.g. "xox" -> /disk/XOX.ELF).
pub fn run_named(name: &str) -> Result<u64, &'static str> {
    let pid = spawn_named(name)?;
    Ok(wait(pid))
}

/// Resolve an app name and start it as its own process. Returns its pid.
pub fn spawn_named(name: &str) -> Result<u64, &'static str> {
    spawn_with_args(name, "")
}

/// Start a program with an argument string. `target` is an app name
/// (`"calc"`) or an absolute VFS path to an ELF (`"/disk/MYAPP.ELF"`).
pub fn spawn_with_args(target: &str, args: &str) -> Result<u64, &'static str> {
    // A managed (.NET) app: `x.dll`, or a name with no native ELF whose
    // /disk/<NAME>.DLL exists. It runs in the RustCLR host, CLRHOST.ELF.
    if app_file(target).is_none() {
        if let Some(dll) = crate::clr::dll_for(target) {
            return crate::clr::spawn(&dll, args);
        }
    }
    let (name, bytes) = if target.starts_with('/') {
        let file = target.rsplit('/').next().unwrap_or(target);
        let name = file.split('.').next().unwrap_or(file);
        (alloc::string::String::from(name).to_lowercase(), vfs::read(target).map_err(|_| "file not found")?)
    } else {
        (alloc::string::String::from(target), read_app(target)?)
    };
    // The args are recorded before the child's task exists: on SMP another
    // CPU can start running it immediately (it used to be set afterwards,
    // which was only safe on one core).
    spawn_image_args(&name, &bytes, args)
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
        "statics" => Some("STATICS.ELF"),
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
        "smpsys" => Some("SMPSYS.ELF"),
        "smpgui" => Some("SMPGUI.ELF"),
        "pthread" => Some("PTHREAD.ELF"),
        "smpfs" => Some("SMPFS.ELF"),
        "smpproc" => Some("SMPPROC.ELF"),
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
    app_file(name).is_some() || crate::clr::dll_for(name).is_some()
}
