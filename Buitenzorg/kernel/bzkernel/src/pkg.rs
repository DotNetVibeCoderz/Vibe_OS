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
    pub category: &'static str, // user-facing category (App Store)
    /// installed out of the box (the preloaded managed apps): `run` works
    /// until someone removes it
    pub pre: bool,
}

/// The registry of available packages (the "app store" catalog).
pub const REGISTRY: &[Package] = &[
    Package { name: "calc", version: "1.0.0", description: "Kalkulator (Buitenzorg.UI)", elf: "CALC.ELF", kind: "desktop", category: "PRODUKTIVITAS", pre: false },
    Package { name: "2048", version: "1.0.0", description: "Game geser-ubin 2048", elf: "G2048.ELF", kind: "desktop", category: "GAME", pre: false },
    Package { name: "clock", version: "1.0.0", description: "Jam analog + digital", elf: "CLOCK.ELF", kind: "desktop", category: "UTILITAS", pre: false },
    Package { name: "piano", version: "1.0.0", description: "Piano (audio)", elf: "PIANO.ELF", kind: "desktop", category: "MULTIMEDIA", pre: false },
    Package { name: "store", version: "1.0.0", description: "App Store", elf: "STORE.ELF", kind: "desktop", category: "SISTEM", pre: false },
    Package { name: "paint", version: "0.9.0", description: "Buitenzorg.Drawing demo", elf: "PAINT.ELF", kind: "desktop", category: "GRAFIS", pre: false },
    Package { name: "taskmgr", version: "0.9.0", description: "Task Manager / monitor", elf: "TASKMGR.ELF", kind: "desktop", category: "UTILITAS", pre: false },
    Package { name: "xox", version: "0.8.0", description: "Tic-Tac-Toe game", elf: "XOX.ELF", kind: "desktop", category: "GAME", pre: false },
    Package { name: "widget", version: "0.9.0", description: "System monitor widget", elf: "WIDGET.ELF", kind: "widget", category: "UTILITAS", pre: false },
    Package { name: "webview", version: "0.9.0", description: "Mini web-app runtime", elf: "WEBVIEW.ELF", kind: "web", category: "INTERNET", pre: false },
    Package { name: "m2048", version: "1.0.0", description: "2048 (C#, bisa di-resize)", elf: "M2048.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mxox", version: "1.0.0", description: "Tic-Tac-Toe (C#)", elf: "MXOX.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mular", version: "1.0.0", description: "Ular -- game ular klasik", elf: "MULAR.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mranjau", version: "1.0.0", description: "Ranjau -- sapu ranjau", elf: "MRANJAU.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mbata", version: "1.0.0", description: "Bata -- pemecah bata", elf: "MBATA.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mbalok", version: "1.0.0", description: "Balok -- balok jatuh", elf: "MBALOK.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mkartu", version: "1.0.0", description: "Kartu -- solitaire", elf: "MKARTU.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mlompat", version: "1.0.0", description: "Lompat -- platformer", elf: "MLOMPAT.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mlabirin", version: "1.0.0", description: "Labirin 3D (raycast)", elf: "MLABIRIN.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mbuah", version: "1.0.0", description: "Panen Buah 3D", elf: "MBUAH.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mchip8", version: "1.0.0", description: "Emulator CHIP-8", elf: "MCHIP8.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mnes", version: "1.0.0", description: "Emulator NES + game homebrew orisinal", elf: "MNES.DLL", kind: "desktop", category: "GAME", pre: true },
    Package { name: "doom", version: "1.0.0", description: "DOOM (Freedoom)", elf: "DOOM.ELF", kind: "desktop", category: "GAME", pre: true },
    Package { name: "mmusik", version: "1.0.0", description: "Musik -- WAV/FLAC/MP3/Ogg/MIDI", elf: "MMUSIK.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mvideo", version: "1.0.0", description: "Video -- AVI/MKV/WebM/MP4 + subtitle", elf: "MVIDEO.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mimage", version: "1.0.0", description: "Gambar -- BMP/PNG/JPEG/GIF + EXIF", elf: "MIMAGE.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mrekam", version: "1.0.0", description: "Perekam Suara", elf: "MREKAM.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mpiano", version: "1.0.0", description: "Piano (C#)", elf: "MPIANO.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mpaint", version: "1.0.0", description: "Paint (C#)", elf: "MPAINT.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mgallery", version: "1.0.0", description: "Galeri", elf: "MGALLERY.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mbaca", version: "1.0.0", description: "Baca -- EPUB/TXT/CBZ", elf: "MBACA.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "mruang", version: "1.0.0", description: "Ruang 3D", elf: "MRUANG.DLL", kind: "desktop", category: "MULTIMEDIA", pre: true },
    Package { name: "hello", version: "0.4.0", description: "Hello from C# (console)", elf: "HELLO.ELF", kind: "console", category: "KONSOL", pre: false },
];

/// Fill `out` with registry entries (name + category + installed state) for the
/// PKG_LIST syscall. Returns the number written (capped to `out.len()`).
pub fn list(out: &mut [bz_abi::PkgInfo]) -> usize {
    fn put(dst: &mut [u8], src: &str) {
        let b = src.as_bytes();
        let n = b.len().min(dst.len() - 1);
        dst[..n].copy_from_slice(&b[..n]);
        dst[n] = 0;
    }
    let mut n = 0;
    for pkg in REGISTRY.iter() {
        if n >= out.len() {
            break;
        }
        let mut e = bz_abi::PkgInfo { name: [0; 24], category: [0; 16], installed: 0 };
        put(&mut e.name, pkg.name);
        put(&mut e.category, pkg.category);
        e.installed = if is_installed(pkg.name) { 1 } else { 0 };
        out[n] = e;
        n += 1;
    }
    // packages installed from .bzpkg files
    for p in crate::bzpkg::installed() {
        if n >= out.len() {
            break;
        }
        let mut e = bz_abi::PkgInfo { name: [0; 24], category: [0; 16], installed: 1 };
        put(&mut e.name, &p.name);
        put(&mut e.category, &p.category);
        out[n] = e;
        n += 1;
    }
    n
}

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

/// Preinstalled packages the user removed (this boot).
static REMOVED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn is_installed(name: &str) -> bool {
    if INSTALLED.lock().iter().any(|x| x == name) {
        return true;
    }
    find(name).is_some_and(|p| p.pre) && !REMOVED.lock().iter().any(|x| x == name)
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
    if pkg.pre {
        REMOVED.lock().retain(|x| x != name);
    } else {
        INSTALLED.lock().push(String::from(pkg.name));
    }
    crate::bzpkg::note_registry(pkg.name, true);
    Ok(pkg.version)
}

/// Remove an installed package.
pub fn remove(name: &str) -> Result<(), &'static str> {
    if find(name).is_some_and(|p| p.pre) {
        if !is_installed(name) {
            return Err("not installed");
        }
        REMOVED.lock().push(String::from(name));
        crate::bzpkg::note_registry(name, false);
        return Ok(());
    }
    let mut inst = INSTALLED.lock();
    if let Some(pos) = inst.iter().position(|x| x == name) {
        inst.remove(pos);
        drop(inst);
        crate::bzpkg::note_registry(name, false);
        Ok(())
    } else {
        Err("not installed")
    }
}
