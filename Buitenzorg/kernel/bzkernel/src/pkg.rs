//! Package manager + registry (v0.10 "Buah"). A registry lists available
//! packages (name, version, description, backing ELF on /disk); install/remove
//! track which are installed, and only installed apps launch via `run`/`bz`.
//!
//! This is the "package manager (install/update/remove) + app registry" from
//! requirements.md §10.3/§16 in miniature. Sandboxing/dependencies are later
//! work; the payload already lives on /disk (embedded in the image).

use alloc::{string::String, vec::Vec};
use spin::Mutex;

pub struct Package {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub elf: &'static str, // 8.3 file on /disk
    pub kind: &'static str, // desktop | console | web | widget
}

/// The registry of available packages (the "app store" catalog).
pub const REGISTRY: &[Package] = &[
    Package { name: "paint", version: "0.9.0", description: "Buitenzorg.Drawing demo", elf: "PAINT.ELF", kind: "desktop" },
    Package { name: "taskmgr", version: "0.9.0", description: "Task Manager / monitor", elf: "TASKMGR.ELF", kind: "desktop" },
    Package { name: "xox", version: "0.8.0", description: "Tic-Tac-Toe game", elf: "XOX.ELF", kind: "desktop" },
    Package { name: "widget", version: "0.9.0", description: "System monitor widget", elf: "WIDGET.ELF", kind: "widget" },
    Package { name: "webview", version: "0.9.0", description: "Mini web-app runtime", elf: "WEBVIEW.ELF", kind: "web" },
    Package { name: "hello", version: "0.4.0", description: "Hello from C# (console)", elf: "HELLO.ELF", kind: "console" },
];

/// Names of packages installed at boot (the rest must be `bz install`-ed).
static INSTALLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Pre-install a base set at boot (so the desktop has apps out of the box).
pub fn seed(names: &[&str]) {
    let mut inst = INSTALLED.lock();
    for n in names {
        if !inst.iter().any(|x| x == n) {
            inst.push(String::from(*n));
        }
    }
}

pub fn find(name: &str) -> Option<&'static Package> {
    REGISTRY.iter().find(|p| p.name == name)
}

pub fn is_installed(name: &str) -> bool {
    INSTALLED.lock().iter().any(|x| x == name)
}

/// Install a package from the registry. Returns Ok(version) or an error.
pub fn install(name: &str) -> Result<&'static str, &'static str> {
    let pkg = find(name).ok_or("package not in registry")?;
    // Verify the payload is actually available on /disk.
    let path = alloc::format!("/disk/{}", pkg.elf);
    crate::vfs::read(&path).map_err(|_| "package payload missing on /disk")?;
    if is_installed(name) {
        return Err("already installed");
    }
    INSTALLED.lock().push(String::from(pkg.name));
    Ok(pkg.version)
}

/// Remove an installed package.
pub fn remove(name: &str) -> Result<(), &'static str> {
    let mut inst = INSTALLED.lock();
    if let Some(pos) = inst.iter().position(|x| x == name) {
        inst.remove(pos);
        Ok(())
    } else {
        Err("not installed")
    }
}
