//! App launcher (v0.8 "Kembang"): load a desktop app (ELF from the VFS) and
//! run it in ring 3. Apps draw their UI through the window syscalls
//! (WIN_CREATE / WIN_CMD / WIN_PRESENT) and read input via KEY_READ.
//!
//! Uses the global memory context so it can be invoked from the shell after
//! boot. Ring-3 execution model is the same single-process one as v0.4: the
//! app runs to completion, then its pages are unmapped.

use alloc::vec::Vec;

use crate::{elf, memory, usermode, vfs};

const USER_STACK_BASE: u64 = 0x7000_0000;
const USER_STACK_PAGES: u64 = 32; // 128 KiB (apps use more stack than hello)

/// Load and run an app image already in memory. Returns its exit code.
pub fn run_image(image: &[u8]) -> Result<u64, &'static str> {
    let (entry, prog_pages, stack_top, stack_pages) = memory::with_ctx(|ctx| {
        let program = elf::load(image, &mut ctx.mapper, &mut ctx.frames)?;
        let (stack_top, stack_pages) = memory::map_user_region(
            USER_STACK_BASE,
            USER_STACK_PAGES,
            &mut ctx.mapper,
            &mut ctx.frames,
        )?;
        Ok::<_, &'static str>((program.entry, program.pages, stack_top, stack_pages))
    })?;

    let code = usermode::enter_user(entry, stack_top);

    memory::with_ctx(|ctx| {
        memory::unmap_user_pages(&prog_pages, &mut ctx.mapper);
        memory::unmap_user_pages(&stack_pages, &mut ctx.mapper);
    });
    Ok(code)
}

/// Resolve an app name to a VFS path and run it. Known apps live on `/disk`
/// as uppercase 8.3 ELF files (e.g. "xox" -> /disk/XOX.ELF).
pub fn run_named(name: &str) -> Result<u64, &'static str> {
    let file = app_file(name).ok_or("unknown app")?;
    let path = alloc::format!("/disk/{}", file);
    let bytes: Vec<u8> = vfs::read(&path).map_err(|_| "app not found on /disk")?;
    crate::process::app_start(name);
    let result = run_image(&bytes);
    crate::process::app_exit();
    result
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
        _ => None,
    }
}

/// True if `name` is a launchable app.
pub fn is_app(name: &str) -> bool {
    app_file(name).is_some()
}
