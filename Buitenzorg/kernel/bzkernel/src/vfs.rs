//! Virtual filesystem (v0.5 "Dahan"): a mount table over block devices, with
//! path resolution `/<mount>/<FILE.EXT>`. Backed by the FAT driver today; the
//! trait leaves room for the custom journaled FS (§8.1) later.
//!
//! Paths: `/disk/BATANG.TXT`, `/ram/DAHAN.TXT`, `/disk/APPS/DEMO/A.TXT`
//! (folders inside a volume, 8.3 names per component).

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

/// Delete the file (or empty folder) at `path`, freeing its clusters.
pub fn remove(path: &str) -> Result<(), &'static str> {
    let (mount, file) = split_path(path)?;
    if file.is_empty() {
        return Err("path names a directory, not a file");
    }
    with_mount(mount, |m| {
        if m.read_only {
            return Err("mount is read-only");
        }
        m.volume.delete_file(m.device.as_mut(), file)
    })
}

/// (free bytes, read_only) of the mount `path` lives on.
pub fn free_space(path: &str) -> Result<(u64, bool), &'static str> {
    let (mount, _) = split_path(path)?;
    with_mount(mount, |m| {
        let (clusters, bytes) = m.volume.free_space(m.device.as_mut())?;
        Ok((clusters * bytes, m.read_only))
    })
}

/// Names in the folder at `path` (`/disk`, `/disk/APPS`).
pub fn list(path: &str) -> Result<Vec<String>, &'static str> {
    Ok(list_entries(path)?.into_iter().map(|e| e.name).collect())
}

/// Entries (name, folder?, size, date) of the folder at `path`.
pub fn list_entries(path: &str) -> Result<Vec<crate::fat::DirEntry>, &'static str> {
    let (mount, dir) = split_path(path)?;
    with_mount(mount, |m| m.volume.list(m.device.as_mut(), dir))
}

/// Size, kind and date of `path` (a mount itself is a folder).
pub fn stat(path: &str) -> Result<crate::fat::DirEntry, &'static str> {
    let (mount, rest) = split_path(path)?;
    with_mount(mount, |m| m.volume.stat(m.device.as_mut(), rest))
}

/// Create the folder `path`.
pub fn mkdir(path: &str) -> Result<(), &'static str> {
    let (mount, rest) = split_path(path)?;
    if rest.trim_matches('/').is_empty() {
        return Err("a mount already exists");
    }
    with_mount(mount, |m| {
        if m.read_only {
            return Err("mount is read-only");
        }
        m.volume.mkdir(m.device.as_mut(), rest)
    })
}

/// Rename / move `from` to `to`. On one mount the entry is relinked; across
/// mounts a file is copied, then deleted (folders cannot cross mounts).
pub fn rename(from: &str, to: &str) -> Result<(), &'static str> {
    let (m1, r1) = split_path(from)?;
    let (m2, r2) = split_path(to)?;
    if r1.trim_matches('/').is_empty() || r2.trim_matches('/').is_empty() {
        return Err("cannot rename a mount");
    }
    if m1 == m2 {
        return with_mount(m1, |m| {
            if m.read_only {
                return Err("mount is read-only");
            }
            m.volume.rename(m.device.as_mut(), r1, r2)
        });
    }
    if stat(from)?.is_dir {
        return Err("folders cannot move to another drive");
    }
    if stat(to).is_ok() {
        return Err("a file or folder with that name exists");
    }
    let data = read(from)?;
    write(to, &data)?;
    remove(from)
}

/// Names of all current mounts, e.g. `["disk", "ram"]`.
pub fn mounts() -> Vec<String> {
    MOUNTS.lock().iter().map(|m| format!("/{}", m.name)).collect()
}

/// Folders end to end on the mount `base` ("/ram", "/disk"): nested
/// mkdir, a sub-folder that outgrows its first cluster, read-back, move a
/// file up a level, rename a folder, refuse to delete a non-empty folder,
/// clean up. Returns a summary for the milestone line.
pub fn folder_self_test(base: &str) -> Result<String, &'static str> {
    let d = format!("{}/BZTEST", base);
    // leftovers of an interrupted run
    if stat(&d).is_ok() {
        for sub in ["SUB", "SUB2"] {
            let p = format!("{}/{}", d, sub);
            for e in list_entries(&p).unwrap_or_default() {
                let _ = remove(&format!("{}/{}", p, e.name));
            }
            let _ = remove(&p);
        }
        for e in list_entries(&d).unwrap_or_default() {
            let _ = remove(&format!("{}/{}", d, e.name));
        }
        let _ = remove(&d);
    }
    mkdir(&d)?;
    mkdir(&format!("{}/SUB", d))?;
    if mkdir(&d).is_ok() {
        return Err("a second mkdir of the same folder succeeded");
    }
    // 20 files: more entries than one 512-byte cluster holds (16), so the
    // folder's chain grows
    for i in 0..20 {
        write(&format!("{}/SUB/F{:02}.TXT", d, i), format!("berkas {}", i).as_bytes())?;
    }
    if read(&format!("{}/SUB/F17.TXT", d))? != b"berkas 17" {
        return Err("read-back in a sub-folder differs");
    }
    let n_sub = list_entries(&format!("{}/SUB", d))?.len();
    if n_sub != 20 {
        return Err("sub-folder listing is not 20 files");
    }
    rename(&format!("{}/SUB/F03.TXT", d), &format!("{}/NAIK.TXT", d))?;
    if read(&format!("{}/NAIK.TXT", d))? != b"berkas 3" || stat(&format!("{}/SUB/F03.TXT", d)).is_ok() {
        return Err("move up a level failed");
    }
    rename(&format!("{}/SUB", d), &format!("{}/SUB2", d))?;
    if !stat(&format!("{}/SUB2", d))?.is_dir || read(&format!("{}/SUB2/F17.TXT", d))? != b"berkas 17" {
        return Err("renamed folder lost its content");
    }
    if remove(&d).is_ok() {
        return Err("a non-empty folder was deleted");
    }
    for e in list_entries(&format!("{}/SUB2", d))? {
        remove(&format!("{}/SUB2/{}", d, e.name))?;
    }
    remove(&format!("{}/SUB2", d))?;
    remove(&format!("{}/NAIK.TXT", d))?;
    remove(&d)?;
    if stat(&d).is_ok() {
        return Err("cleanup left the folder behind");
    }
    Ok(format!("{}: nested mkdir, 20 files in a sub-folder (chain grew), move, rename, non-empty delete refused, cleanup", base))
}
