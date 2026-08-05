# Getting Started

Panduan ini membawa Anda dari nol sampai **Buitenzorg OS berjalan di QEMU**,
bahkan kalau Anda belum pernah membangun sistem operasi.

> Buitenzorg OS adalah OS hibrida AI-native: kernel **Rust** (ring 0) dan app &
> layanan **C#/.NET** (ring 3). Ia berjalan di dalam emulator **QEMU** — Anda tak
> pernah menyentuh mesin sungguhan.

[English](getting-started.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

---

## 1. Jalur tercepat (satu perintah)

Skrip **quickstart** memasang semua yang dibutuhkan (Rust, .NET, QEMU, bflat),
membangun OS, lalu mem-boot-nya di QEMU. Buka terminal di folder repo:

**Windows** (PowerShell):
```powershell
.\scripts\quickstart.ps1
```

**Linux / macOS** (bash):
```bash
./scripts/quickstart.sh
```

Selesai — jendela QEMU terbuka dan mem-boot Buitenzorg. Untuk build tanpa
menjalankan, tambah `-NoRun` (Windows) / `--no-run` (Linux); untuk self-test
headless pakai `-SmokeTest` / `--smoke`.

> Skrip aman dijalankan ulang: langkah yang sudah selesai dilewati. Di Windows ia
> memakai **winget**; di Linux, **apt / dnf / pacman / brew** + rustup. Kalau ada
> langkah gagal, ikuti pesan yang tercetak atau pakai setup manual di bawah.

## 2. Dependensi

| Alat | Untuk | Pasang manual |
|---|---|---|
| **Rust** (rustup) | membangun kernel (Rust `no_std`) | [rustup.rs](https://rustup.rs) — toolchain nightly + target `x86_64-unknown-none` di-pin otomatis oleh `kernel/rust-toolchain.toml` |
| **.NET SDK 10** | membangun runtime/SDK C# + menjalankan test | [dotnet.microsoft.com](https://dotnet.microsoft.com/download) |
| **QEMU** | emulator tempat OS boot | [qemu.org/download](https://www.qemu.org/download/) — di Windows terdeteksi otomatis di `C:\Program Files\qemu\` |
| **bflat** | mengompilasi app C# → ELF native (ring 3) | unduh rilis dari [bflattened/bflat](https://github.com/bflattened/bflat/releases) dan ekstrak ke `tools/bflat/` (quickstart melakukannya untuk Anda) |

> `bflat` dan `tools/` di-gitignore. Tanpa bflat kernel tetap boot, tapi app C#
> ring-3 tidak dibangun.

## 3. Setup manual (tanpa quickstart)

1. Pasang **Rust**, **.NET SDK 10**, dan **QEMU** (lihat tabel di atas).
2. Unduh **bflat** (windows-x64 / linux-glibc-x64) dan ekstrak ke `tools/bflat/`,
   sehingga ada `tools/bflat/bflat.exe` (Windows) atau `tools/bflat/bflat` (Linux).
3. Build dan jalankan (lihat alur harian di bawah).

## 4. Alur harian

**Windows:**
```powershell
.\scripts\build.ps1        # build semuanya  → dist\*.img
.\scripts\run-qemu.ps1     # boot + pantau serial; -Uefi untuk jalur UEFI/OVMF
.\scripts\smoke-test.ps1   # verifikasi milestone boot otomatis (4 media)
```

**Linux / macOS:**
```bash
./scripts/build.sh
./scripts/smoke-test.sh
```

**Iterasi kernel saja (tercepat):**
```powershell
cd kernel
cargo run --release -p bzimage -- --run    # build ulang + boot sekaligus
```

`bzimage` adalah pipeline boot: ia mengompilasi `bzkernel` (artifact dependency,
target `x86_64-unknown-none`), membungkusnya dengan crate `bootloader` 0.11 jadi
image GPT/FAT (UEFI) plus image MBR (BIOS), lalu opsional menjalankan QEMU.
Firmware OVMF untuk UEFI diunduh otomatis ke `kernel/target/ovmf/`.

> ⚠️ Jangan pernah menjalankan `cargo build` untuk `bzkernel` tanpa `--target
> x86_64-unknown-none` — kernel tidak bisa (dan tidak perlu) dibangun untuk host,
> itulah sebabnya ia dikecualikan dari `default-members` workspace.

## 5. Sisi C# (host)

```powershell
dotnet build Buitenzorg.slnx     # catatan: .slnx, bukan .sln
dotnet test  Buitenzorg.slnx     # kontrak ABI Rust ↔ C#
dotnet run --project runtime\samples\HelloBuitenzorg
```

Di host, app C# berjalan pada **backend simulasi** (`HostSyscalls`) yang API-nya
identik dengan target sungguhan — jadi kode yang sama jalan di bare metal. Ingin
membuat app sendiri? Lihat **[App Pertama](first-app.id.md)**.

## 6. Debug kernel (GDB)

Cara turnkey adalah `scripts/debug-kernel.ps1` / `.sh` (lihat
[Debugging & Profiling](debugging.id.md)). Cara manual:

```powershell
cd kernel
$env:QEMU_EXTRA = "-s -S"                   # QEMU: server GDB, boot ditahan
cargo run --release -p bzimage -- --run
# dari terminal lain:  gdb → target remote :1234
```

Simbol kernel: `kernel/target/x86_64-unknown-none/release/bzkernel`.

## Di luar QEMU

- **Jalankan di VM** (VMware / VirtualBox / Hyper-V): [run-in-vm.id.md](run-in-vm.id.md).
- **Boot di hardware nyata** dari USB: [install-hardware.id.md](install-hardware.id.md)
  *(eksperimental — belum divalidasi di mesin fisik; lihat tabel kompatibilitas
  yang jujur di dokumen itu).*

## Troubleshooting

- **`rustup` / `dotnet` / `qemu` tak ditemukan setelah quickstart** — buka
  terminal baru (PATH baru saja di-refresh) lalu coba lagi.
- **QEMU tak di PATH (Windows)** — build tetap mendeteksi
  `C:\Program Files\qemu\`. Kalau lokasi Anda beda, set env var `QEMU` ke path
  lengkap `qemu-system-x86_64.exe`.
- **"offset is not a multiple of 16" saat build kernel** — Anda membangun
  `bzkernel` untuk host. Selalu lewat `bzimage` (`cargo run -p bzimage`), yang
  memakai target bare-metal.
- **App C# tak muncul saat boot** — pastikan `tools/bflat/bflat.exe` ada, lalu
  jalankan `scripts/build-hello-csharp.ps1` dan cek error bflat.
- **Boot terasa lama (~1 menit ke READY)** — normal: boot menjalankan banyak demo
  app. Log lengkap selalu mengalir ke serial.
- **Layar QEMU hitam tapi serial jalan** — kernel merender ke framebuffer; beri
  beberapa detik untuk desktop, atau baca output di serial.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
