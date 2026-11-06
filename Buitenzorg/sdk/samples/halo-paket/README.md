# halo-paket — a sample `.bzpkg`

The smallest package: one shell script (`package/HALOPKT.SH`) and its
`app.manifest`. `HALOPKT.BZP` is what `bz pack` makes of this folder; the image
builder copies it to `/disk/HALOPKT.BZP`, so it can be tried on any image:

```text
bz info /disk/HALOPKT.BZP      # name, version, files, SHA-256 checked
bz install /disk/HALOPKT.BZP   # files to /disk, recorded in /disk/PKGS.CFG
halo-paket satu dua            # runs the entry (a .SH, .ELF or .DLL)
bz list                        # "terpasang dari berkas .bzpkg"
bz remove halo-paket
```

Rebuild the package after changing the script:

```powershell
dotnet run --project sdk/bz -- pack sdk/samples/halo-paket -o sdk/samples/halo-paket/HALOPKT.BZP
```

Format and rules: `kernel/bzkernel/src/bzpkg.rs` (module documentation).
Everything to ship goes in `package/`, with 8.3 names (the device's `/disk`
is FAT); `--entry FILE` picks the file `run`/the package name starts
(default: the first `.ELF`, `.DLL` or `.SH`).
