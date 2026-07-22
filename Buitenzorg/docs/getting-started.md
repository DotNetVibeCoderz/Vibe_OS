# Getting Started

## Setup

1. **Rust** — pasang [rustup](https://rustup.rs). Toolchain nightly + target
   `x86_64-unknown-none` terpasang otomatis saat build pertama di `kernel/`
   (dipin oleh `kernel/rust-toolchain.toml`).
2. **.NET SDK 10** — [dotnet.microsoft.com](https://dotnet.microsoft.com/download).
3. **QEMU** — [qemu.org](https://www.qemu.org/download/). Di Windows default
   terdeteksi di `C:\Program Files\qemu\`; override dengan env var `QEMU`.

## Alur harian

```powershell
.\scripts\build.ps1        # build semuanya → dist/*.img
.\scripts\run-qemu.ps1     # boot + lihat serial; -Uefi untuk jalur UEFI/OVMF
.\scripts\smoke-test.ps1   # verifikasi milestone boot secara otomatis
```

Iterasi kernel saja:

```powershell
cd kernel
cargo run --release -p bzimage -- --run    # build ulang + langsung boot
```

`bzimage` adalah pipeline boot: ia mengompilasi `bzkernel` (artifact dependency,
target `x86_64-unknown-none`), membungkusnya dengan bootloader `bootloader` 0.11
menjadi image GPT/FAT (UEFI) dan MBR (BIOS), lalu (opsional) menjalankan QEMU.
Firmware OVMF untuk UEFI diunduh otomatis ke `kernel/target/ovmf/`.

> Catatan: jangan jalankan `cargo build` untuk `bzkernel` tanpa
> `--target x86_64-unknown-none` — kernel tidak bisa (dan tidak perlu) dibangun
> untuk host; karena itu ia bukan `default-members` workspace.

## Sisi C#

```powershell
dotnet build Buitenzorg.slnx
dotnet test  Buitenzorg.slnx
dotnet run --project runtime\samples\HelloBuitenzorg
```

App C# hari ini berjalan di **backend simulasi host** (`HostSyscalls`) dengan
API yang sama persis dengan target — begitu managed runtime berjalan di bare
metal (v0.4 "Tunas"), app tidak perlu diubah.

## Membuat app dari template

```powershell
dotnet run --project sdk\bz -- new console-csharp MyApp
cd MyApp; dotnet run
dotnet run --project ..\sdk\bz -- manifest validate app.manifest
```

## Debugging kernel

```powershell
# Jalankan QEMU dengan GDB server (paused), lalu attach dari terminal lain
cd kernel
$env:QEMU_EXTRA = "-s -S"
cargo run --release -p bzimage -- --run
```

Simbol kernel ada di `kernel/target/x86_64-unknown-none/release/bzkernel`.
GDB-remote QEMU mendengarkan di `localhost:1234` (`target remote :1234`).
