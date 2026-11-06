# Packages (`.bzpkg`)

An app can be shipped as one package file instead of being baked into the disk
image. `bz pack` on your PC makes it; `bz install` on the device installs it.

## Make a package (host)

```text
myapp/
  app.manifest          id, name, type, version (the SDK manifest)
  package/              everything to ship, 8.3 names (the device's /disk is FAT)
    MYAPP.ELF           a native C# app (bflat), or MYAPP.DLL (managed), or MYAPP.SH
    MYAPP.BMP           assets
```

```powershell
dotnet run --project sdk/bz -- pack myapp -o MYAPP.BZP     # [--entry FILE] [--name NAME]
dotnet run --project sdk/bz -- inspect MYAPP.BZP           # list + verify every SHA-256
```

The package name is the last part of the manifest `id` (`local.my-app` ->
`my-app`; a-z 0-9 -, up to 23 characters). The **entry** — what starts when the
package name is typed — is `--entry`, else the first `.ELF`, `.DLL` or `.SH`.

## Install, run, remove (device)

Copy the file to `/disk` under an 8.3 name (`MYAPP.BZP`), e.g. with the deploy
service (`deploy on` + Magic App Generator's device panel), then:

```sh
bz info /disk/MYAPP.BZP       # manifest + files, every hash checked
bz install /disk/MYAPP.BZP    # files go to /disk, the install is recorded
my-app arg1 arg2              # or: run my-app
bz list                       # built-in registry + "installed from .bzpkg files"
bz install /disk/MYAPP.BZP    # a newer version: an upgrade (old files removed)
bz remove my-app              # deletes its files
```

Every image carries a sample, `/disk/HALOPKT.BZP` (`sdk/samples/halo-paket`).

**Safety rules:** the archive's own SHA-256 and every file's SHA-256 are
checked before anything is written; names must be 8.3; a file another package
owns, a system file (`USERS.CFG`, `PKGS.CFG`, `CLRHOST.ELF`, ...) or a file
already on `/disk` that is not from this package is refused; a package cannot
take the name of a built-in registry app.

Installed packages, and registry apps installed with `bz install NAME`, are
kept in `/disk/PKGS.CFG` and restored at boot. Packages from files show up in
the App Store list (`PKG_LIST`) too.

## Format, version 1

```text
"BZPK"  u16 version (1)  u16 file count  u32 manifest length
manifest: UTF-8 key=value lines (name, version, title, kind, category,
          description, entry)
per file: u8 name length, 8.3 name, u32 size, 32-byte SHA-256, data
trailer:  32-byte SHA-256 of all bytes before it
```

Not yet: signatures (hashes only detect damage, not a malicious publisher),
dependencies, a remote repository index (`bz update`), sub-folders, per-app
permissions from the manifest.

Implementation: `kernel/bzkernel/src/bzpkg.rs` (device), `sdk/bz/Program.cs`
(`Pack`, `Inspect`).

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
