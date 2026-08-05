# Getting Started — Menjalankan Buitenzorg OS

Panduan ini membawa kamu dari nol sampai **Buitenzorg OS berjalan di QEMU**,
bahkan jika kamu belum pernah membangun sistem operasi. Buitenzorg dibuat oleh
**Gravicode Studios**, dipimpin oleh **Kang Fadhil**.

> Buitenzorg OS adalah OS hibrida AI-native: kernel di **Rust** (ring 0),
> aplikasi & layanan di **C#/.NET** (ring 3). Ia berjalan di dalam emulator
> **QEMU** — kamu tidak perlu memformat komputermu.

---

## 🚀 Jalur cepat (satu perintah)

Skrip **quickstart** memasang semua yang dibutuhkan (Rust, .NET, QEMU, bflat),
membangun OS, lalu menjalankannya di QEMU. Buka terminal di folder repo:

**Windows** (PowerShell):
```powershell
.\scripts\quickstart.ps1
```

**Linux / macOS** (bash):
```bash
./scripts/quickstart.sh
```

Selesai — sebuah jendela QEMU akan terbuka dan mem-boot Buitenzorg OS. Untuk
hanya membangun tanpa menjalankan tambahkan `-NoRun` (Windows) / `--no-run`
(Linux); untuk uji-otomatis headless gunakan `-SmokeTest` / `--smoke`.

> Skrip aman dijalankan berulang: langkah yang sudah terpasang dilewati. Di
> Windows ia memakai **winget**; di Linux memakai **apt/dnf/pacman/brew** +
> rustup. Jika sebuah langkah gagal, ikuti pesan yang muncul atau lanjut ke
> **Setup manual** di bawah.

---

## 🧰 Apa saja dependensinya

| Alat | Untuk apa | Cara pasang manual |
|------|-----------|--------------------|
| **Rust** (rustup) | membangun kernel (Rust `no_std`) | [rustup.rs](https://rustup.rs) — toolchain nightly + target `x86_64-unknown-none` dipasang otomatis via `kernel/rust-toolchain.toml` |
| **.NET SDK 10** | membangun runtime/SDK C# + menjalankan test | [dotnet.microsoft.com](https://dotnet.microsoft.com/download) |
| **QEMU** | emulator tempat OS boot | [qemu.org/download](https://www.qemu.org/download/) — di Windows default terdeteksi di `C:\Program Files\qemu\` |
| **bflat** | mengompilasi app C# → ELF native (ring 3) | Unduh rilis dari [github.com/bflattened/bflat](https://github.com/bflattened/bflat/releases), ekstrak ke `tools/bflat/` (quickstart melakukannya otomatis) |

> `bflat` dan `tools/` di-gitignore. Tanpa bflat, kernel tetap boot tetapi app
> C# ring-3 tidak dibangun.

---

## 🛠️ Setup manual (kalau tidak pakai quickstart)

1. Pasang **Rust**, **.NET SDK 10**, dan **QEMU** (lihat tabel di atas).
2. Unduh **bflat** (windows-x64 / linux-glibc-x64) dan ekstrak isinya ke
   `tools/bflat/` sehingga ada `tools/bflat/bflat.exe` (Windows) atau
   `tools/bflat/bflat` (Linux).
3. Build + jalankan (lihat **Alur harian**).

---

## 📅 Alur harian

**Windows:**
```powershell
.\scripts\build.ps1        # build semuanya -> dist\*.img
.\scripts\run-qemu.ps1     # boot + lihat serial; -Uefi untuk jalur UEFI/OVMF
.\scripts\smoke-test.ps1   # verifikasi milestone boot otomatis (4 media)
```

**Linux/macOS:**
```bash
./scripts/build.sh
./scripts/smoke-test.sh
```

**Iterasi kernel saja (paling cepat):**
```powershell
cd kernel
cargo run --release -p bzimage -- --run    # build ulang + langsung boot
```

`bzimage` adalah pipeline boot: ia mengompilasi `bzkernel` (artifact dependency,
target `x86_64-unknown-none`), membungkusnya dengan bootloader `bootloader` 0.11
menjadi image GPT/FAT (UEFI) + MBR (BIOS), lalu (opsional) menjalankan QEMU.
Firmware OVMF untuk UEFI diunduh otomatis ke `kernel/target/ovmf/`.

> ⚠️ Jangan jalankan `cargo build` untuk `bzkernel` tanpa
> `--target x86_64-unknown-none` — kernel tidak bisa (dan tidak perlu) dibangun
> untuk host; karena itu ia bukan `default-members` workspace.

---

## 🖥️ Menjalankan di VMware / VirtualBox

Ingin menjalankannya di VMware Player atau Oracle VirtualBox alih-alih QEMU?
Lihat **[run-in-vm.md](run-in-vm.md)** — ada skrip `scripts/make-vm-images.ps1`
(`.sh`) yang mengonversi image ke `.vmdk`/`.vdi`.

## 💽 Boot dari USB di hardware nyata

Buitenzorg juga bisa boot dari stik USB di komputer fisik (BIOS atau UEFI).
Lihat **[install-hardware.md](install-hardware.md)** — ada skrip
`scripts/flash-usb.ps1` (`.sh`) yang menulis image ke USB dengan pengaman
berlapis + verifikasi. *(Boot hardware masih eksperimental — belum tervalidasi
tim; lihat batasan di dokumen itu.)*

---

## 👩‍💻 Sisi C# (host)

```powershell
dotnet build Buitenzorg.slnx     # catatan: .slnx, bukan .sln
dotnet test  Buitenzorg.slnx     # kontrak ABI Rust<->C#
dotnet run --project runtime\samples\HelloBuitenzorg
```

App C# di host berjalan pada **backend simulasi** (`HostSyscalls`) dengan API
identik dengan target — jadi kode yang sama jalan di bare metal.

Ingin membuat app sendiri? Lihat **[first-app.md](first-app.md)**.

---

## 🐞 Debugging kernel (GDB)

```powershell
cd kernel
$env:QEMU_EXTRA = "-s -S"                   # QEMU: GDB server, boot ditahan
cargo run --release -p bzimage -- --run
# dari terminal lain: gdb -> target remote :1234
```
Simbol kernel: `kernel/target/x86_64-unknown-none/release/bzkernel`.

---

## ❓ Troubleshooting

- **`rustup`/`dotnet`/`qemu` tidak ditemukan setelah quickstart** — buka
  terminal baru (PATH baru di-refresh), lalu jalankan lagi.
- **QEMU tidak di PATH (Windows)** — build tetap mendeteksi
  `C:\Program Files\qemu\`. Kalau lokasimu beda, set env var `QEMU` ke path
  `qemu-system-x86_64.exe`.
- **"offset is not a multiple of 16" saat build kernel** — kamu membangun
  `bzkernel` untuk host. Selalu lewat `bzimage` (`cargo run -p bzimage`), yang
  memakai target bare-metal.
- **App C# tidak muncul saat boot** — pastikan `tools/bflat/bflat.exe` ada;
  jalankan `scripts/build-hello-csharp.ps1` dan cek tidak ada error bflat.
- **Boot terasa lama (~1 menit ke READY)** — normal: banyak demo app dijalankan
  saat boot. Log lengkap selalu tampil di serial.
- **Layar QEMU hitam tapi serial jalan** — kernel merender ke framebuffer;
  beri beberapa detik hingga desktop tampil, atau baca output di serial.
