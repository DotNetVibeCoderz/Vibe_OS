//! Virtual filesystem (v0.5 "Dahan"): a mount table over block devices, with
//! path resolution `/<mount>/<FILE.EXT>`. Backed by the FAT driver today; the
//! trait leaves room for the custom journaled FS (§8.1) later.
//!
//! Paths are simple: `/disk/BATANG.TXT`, `/ram/DAHAN.TXT`. Only the root
//! directory of each volume is exposed (matching the FAT reader's scope).

use alloc::{boxed::Box, format, string::String, vec::Vec};
use spin::Mutex;

use crate::driver::BlockDevice;
use crate::fat::FatVolume;

pub struct Mount {
    pub name: String,
    pub read_only: bool,
    device: Box<dyn BlockDevice>,
    volume: FatVolume,
}

static MOUNTS: Mutex<Vec<Mount>> = Mutex::new(Vec::new());

/// Mount a FAT volume on `device` at `/name`.
pub fn mount(name: &str, mut device: Box<dyn BlockDevice>, read_only: bool) -> Result<(), &'static str> {
    let volume = FatVolume::mount(device.as_mut())?;
    crate::println!(
        "[vfs] mounted /{} ({}, {}) on {}",
        name,
        volume.kind_name(),
        if read_only { "ro" } else { "rw" },
        device.name()
    );
    MOUNTS.lock().push(Mount {
        name: String::from(name),
        read_only,
        device,
        volume,
    });
    Ok(())
}

/// Split `/mount/FILE.EXT` into (mount, file). A trailing-less mount path
/// (`/ram`) yields an empty file component (used for listing).
fn split_path(path: &str) -> Result<(&str, &str), &'static str> {
    let rest = path.strip_prefix('/').ok_or("path must be absolute")?;
    match rest.split_once('/') {
        Some((m, f)) => Ok((m, f)),
        None => Ok((rest, "")),
    }
}

fn with_mount<R>(
    mount_name: &str,
    f: impl FnOnce(&mut Mount) -> Result<R, &'static str>,
) -> Result<R, &'static str> {
    let mut mounts = MOUNTS.lock();
    let mount = mounts
        .iter_mut()
        .find(|m| m.name == mount_name)
        .ok_or("no such mount")?;
    f(mount)
}

/// Read a whole file at `path`.
pub fn read(path: &str) -> Result<Vec<u8>, &'static str> {
    let (mount, file) = split_path(path)?;
    if file.is_empty() {
        return Err("path names a directory, not a file");
    }
    with_mount(mount, |m| m.volume.read_file(m.device.as_mut(), file))
}

/// Write a whole file at `path` (creates or overwrites).
pub fn write(path: &str, data: &[u8]) -> Result<(), &'static str> {
    let (mount, file) = split_path(path)?;
    if file.is_empty() {
        return Err("path names a directory, not a file");
    }
    with_mount(mount, |m| {
        if m.read_only {
            return Err("mount is read-only");
        }
        m.volume.write_file(m.device.as_mut(), file, data)
    })
}

/// List the root directory of a mount (`/disk`).
pub fn list(path: &str) -> Result<Vec<String>, &'static str> {
    let (mount, _) = split_path(path)?;
    with_mount(mount, |m| m.volume.list_root(m.device.as_mut()))
}

/// Names of all current mounts, e.g. `["disk", "ram"]`.
pub fn mounts() -> Vec<String> {
    MOUNTS.lock().iter().map(|m| format!("/{}", m.name)).collect()
}
