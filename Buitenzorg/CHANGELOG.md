# Changelog — Buitenzorg OS

Riwayat rilis per codename versi (mengikuti pertumbuhan tanaman, penghormatan
Kebun Raya Bogor). Setiap milestone diverifikasi otomatis di QEMU 4 media
(IDE/AHCI/NVMe/USB) lewat marker `MILESTONE: … OK` yang dicek smoke test.

Dibuat oleh **Gravicode Studios**, dipimpin oleh **Kang Fadhil**.

Format longgar mengikuti [Keep a Changelog](https://keepachangelog.com);
penomoran mengikuti codename versi di [PLAN.md](PLAN.md). Detail teknis penuh
ada di [Progress.md](Progress.md) dan [requirements.md](requirements.md) §17.

---

## [Belum dirilis] — jalur v1.0 "Buitenzorg" (stabilisasi)

Pemantapan menuju rilis stabil x86-64.

### Ditambahkan
- **Security hardening — validasi pointer syscall.** Setiap pointer dari ring 3
  divalidasi (`memory::validate_user_range`/`validate_user_cstr`): tak `wrap`,
  seluruhnya di bawah `USER_ADDR_MAX`, tiap halaman *present* & `USER_ACCESSIBLE`
  (writable untuk buffer keluaran). Menutup celah nyata (bocor memori kernel
  lewat `DEBUG_WRITE`, tulis-sembarang lewat `SYS_STAT`/`PROC_LIST`/`FS_READ`/
  `NET_RECV`, page-fault kernel dari pointer tak-terpeta). Cek hanya di jalur
  ring-3 (`dispatch_from_user`). 14 probe bermusuhan → `MILESTONE: SECURITY OK`.
- **Pembekuan ABI v1.** `abi_v1_is_frozen` (Rust) + `AbiV1IsFrozen` (C#) memaku
  versi, `COUNT`, ukuran+alignment 10 struct lintas-batas, dan kode error.
- **Debugger** — `scripts/debug-kernel.ps1`/`.sh`/`.gdb`: attach GDB ke QEMU
  yang ditahan dengan simbol kernel + helper break/fault/register.
- **Profiler** — `profile.rs`: profiler zona ter-instrumentasi berbasis TSC,
  inert saat off, jalur panas syscall/compositor terinstrumentasi, shell `prof`
  → `MILESTONE: PROFILER OK`.
- **Benchmark regresi di CI** — `scripts/bench.sh` (boot-to-READY + throughput
  async-I/O vs budget) sebagai job CI.
- **Boot USB hardware** — `scripts/flash-usb.ps1`/`.sh` (tulis image ke USB,
  pengaman berlapis + verifikasi baca-ulang) + `docs/install-hardware.md`.
  *(Validasi di mesin fisik menyusul — ditandai eksperimental.)*
- **Image Hyper-V VHDX** — `make-vm-images` emit `dist/buitenzorg.vhdx`
  (64 MiB whole-MiB, Hyper-V friendly) + `scripts/make-hyperv-vm.ps1` (buat VM
  Generation 1 / BIOS, guarded). Dokumentasi di `docs/run-in-vm.md`.

### Diperbaiki
- **CI tak pernah jalan saat push** — trigger `branches: [main]` padahal branch
  repo `master`; kini `[master, main]` + `workflow_dispatch`.
- **`build-hello-csharp.sh` (Linux/CI) kehilangan `imgview`+`jpgtest`** (hanya
  di `.ps1`) → build Linux tanpa ELF itu; kedua skrip kini 27 program.
- TODO lama validasi alamat di `sys_win_create`/`sys_debug_write` ditepati.

---

## v0.16 "Panen" — Preloaded Suite, Audio & Optimasi

Suite aplikasi bawaan lengkap di atas BCL v0.15, subsistem audio, dan pass
optimasi.

### Ditambahkan
- **Subsistem audio OS** — driver **AC'97** (`audio.rs`): enumerasi PCI,
  cold-reset codec, mixer (master + PCM-out), playback PCM 16-bit stereo 48 kHz
  via DMA bus-master + BDL. Syscall `AUDIO_STAT`/`SET_VOLUME`/`TONE`/`PLAY` +
  library `Buitenzorg.Audio` + panel pengaturan audio (`MILESTONE: AUDIO OK`).
- **`Buitenzorg.Drawing`** sebagai renderer software klien (`bzgfx.cs`):
  Graphics/Color/Bitmap, transform (Matrix), GraphicsPath, clipping, hatch,
  `DrawString`/`Font` 8×8, rounded/gradient/shadow/anti-alias, BMP load/save,
  dan **decoder JPEG baseline** (`Jpeg.Load`, IDCT integer). Blit 1-syscall
  (`draw_op::BLIT`) — model kompositor WPF/Avalonia (`MILESTONE: DRAW OK`).
- **`Buitenzorg.UI`** (`bzui.cs`) — toolkit retained gaya WPF/Avalonia: pohon
  `UIElement`, layout Measure/Arrange (Stack/Grid/Canvas), set kontrol penuh
  (Button/CheckBox/Slider/ListBox/TextBox/Menu/ComboBox/TabControl/TreeView/
  ScrollViewer/DataGrid), popup/overlay layer, routing mouse (`MILESTONE: UI OK`).
- **Preloaded suite (8 app)**: Kalkulator, Text Editor (produktivitas), 2048
  (game), Jam, File Manager (utilitas), Piano, Image Viewer (multimedia), App
  Store (toko — terhubung ke `pkg.rs` lewat `PKG_LIST`/`PKG_SET`).
- **Desktop UX shell** — taskbar + tombol Start + start menu + ikon desktop +
  jam tray RTC live (gabung macOS/Ubuntu/Win XP) di `wm.rs`
  (`MILESTONE: DESKTOP SHELL OK`).
- **Editor & File Manager interaktif** saat di-`run` dari shell (routing
  keyboard `KEY_READ` + syscall `IS_INTERACTIVE`).
- Syscall baru: `PKG_LIST`/`PKG_SET`, `FS_LIST`, `FS_READ`.
- **MagicAppGen** (`tools/MagicAppGen`) — code editor Avalonia dengan asisten AI
  "Jack - The Code Bender" (Semantic Kernel, multi-LLM) yang men-generate app
  Buitenzorg; 8 template proyek (5 C# terverifikasi kompilasi bflat).
- **Kelengkapan BCL** — `bzbcl2.cs`: System.IO/Text/RegularExpressions/
  Globalization/Diagnostics/Management/Net(+Sockets)/Threading.Tasks/Timers/GC/
  Pkg. Syscall `FS_WRITE`, `CLOCK_RTC`, `NET_*` (UDP nyata) (`MILESTONE: BCL2 OK`).
- Onboarding: `quickstart.ps1`/`.sh`, `docs/getting-started.md`,
  `docs/first-app.md`, `docs/run-in-vm.md` + `make-vm-images`.

### Diperbaiki (Heisenbug — semua sensitif layout binary)
- **`from_raw_parts` atas memori user** di syscall string → korupsi boot; helper
  `copy_user_bytes`/`read_volatile`.
- **Register argumen syscall (`rdi`/`rsi`/`rdx`) harus `inlateout`** di shim
  userland — kernel cuma restore rcx/r11/rsp; `in` merusak `HEAP_CAP`.
- **`PRIVILEGE_STACK` 20 KiB overflow** di `compose_into` via `WIN_PRESENT` →
  fix `PRIV_STACK_SIZE` 64 KiB + `PRESENT_BUF` reuse + heap kernel 32 MiB.

---

## v0.15 "Matang" — Managed Runtime C# (heap + BCL tulisan-sendiri)

Menaikkan app ring-3 dari zerolib murni ke heap yang berfungsi + BCL.

### Ditambahkan
- **PAL memori** — syscall `MMAP`/`MPROTECT`/`MUNMAP`; arena user; reserve
  lazy (PROT_NONE) + commit-on-demand (pola heap GC .NET).
- **Thread ring-3 kooperatif** — `THREAD_CREATE`/`JOIN`/`EXIT` (stack SYSCALL
  per-thread).
- **Sync/TLS/clock** — `FUTEX_WAIT`/`WAKE` (state scheduler `Blocked`),
  `THREAD_SELF`, `CLOCK_MONO` (TSC).
- **Managed heap berfungsi** — `new`/array/objek/generic jalan (heap bump
  tumbuh via mmap) + libc `malloc`/`free`/`calloc`/`realloc`.
- **`Buitenzorg.Bcl`** (`bzbcl.cs`) — `BzList/Stack/Queue/IntMap/StrMap/IntSet/
  RefList`, LINQ (function-pointer), `BzStringBuilder`, `BzMath`/`BzRandom`/
  `BzConvert`/`BzStr`/`BzHex`/`BzBase64`/`BzBitConverter` (`MILESTONE: BCL OK`).

---

## v0.14 "Babel" — Runtime Polyglot

- **`script.rs`** — satu interpreter tree-walking dengan 3 front-end:
  **JavaScript**, **TypeScript** (transpile strip anotasi), **Python**
  (indentasi INDENT/DEDENT). Fungsi+rekursi, if/while/for, operator, budget
  langkah. Shell `script <lang>` + `js`/`ts`/`py` (`MILESTONE: BABEL OK`).

## v0.13 "Lapis" — Virtualisasi

- **`vmx.rs`** deteksi VT-x/AMD-V; **`vmm.rs`** VMM software (BZVM virtual CPU,
  virtio, RAM disk, snapshot/restore penuh). Guest "NanoOS" (assembler in-kernel
  2-pass) boot & jalan. Manajer VM + shell `vm` (`MILESTONE: LAPIS OK`).

## v0.12 "Nalar" — AI Subsystem & Power

- **`ai.rs`** — LLM bigram char-level, Sobel edge-detect CV, text-to-image
  prosedural (CPU-lokal); **`model.rs`** galeri model gaya Hugging Face; shell
  `ask`. **`power.rs`** — ACPI shutdown/restart + light sleep (`MILESTONE:
  NALAR OK`).

## v0.11 "Cahaya" — GPU Compute API & Desktop Polish

- **`compute.rs`** (SAXPY/blend, backend CPU, Gpu direservasi); screensaver
  (6 gaya klasik); wallpaper (BMP user); window controls (min/max/close +
  rounded); micro-interaction. **Bugfix laten:** re-enable interrupt setelah app
  ring-3 keluar (`MILESTONE: CAHAYA OK`).

## v0.10 "Buah" — Theme Engine & Package Manager

- **`theme.rs`** design token + 8 tema bawaan + dark/light; **`pkg.rs`** registry
  + `bz install/remove/list/search` (`MILESTONE: BUAH OK`).

## v0.9 "Serbuk" — 4 Varian App, Drawing, Task Manager

- 4 varian app (console/desktop/web/widget); `Buitenzorg.Drawing` (gaya
  System.Drawing); Task Manager + registry proses `process.rs` (`PROC_LIST`/
  `PROC_KILL`/`SYS_STAT`) (`MILESTONE: SERBUK OK`).

## v0.8 "Kembang" — App Framework

- Window syscall ABI (`WIN_CREATE`/`WIN_CMD`/`WIN_PRESENT`/`KEY_READ`) +
  `DrawCmd`; `app.rs` launcher; app XOX; template SDK desktop (`MILESTONE:
  KEMBANG OK`).

## v0.7 "Kanopi" — Desktop Environment

- `theme.rs` (dark/light), 4 workspace, `terminal.rs`+`shell.rs`, `keyboard.rs`
  (`MILESTONE: KANOPI OK`).

## v0.6 "Daun" — Compositor & Window Manager

- `gfx.rs` + `wm.rs` (kompositor double-buffered, window floating, taskbar,
  cursor) (`MILESTONE: WINDOWS OK`).

## v0.5 "Dahan" — VFS, Services, Async I/O, Networking

- VFS + FAT12 write; service/init manager paralel; async I/O io_uring-style;
  stack loopback Ethernet/ARP/IPv4/ICMP; service C# sebagai proses (`MILESTONE:
  DAHAN OK`).

## v0.4 "Tunas" — C# di Ring 3

- SYSCALL/SYSRET, ELF64 loader, ring-3 user segment + TSS; `hello.cs` via bflat
  (zerolib) + shim `bzstart.rs` → "Hello from C#!" (`MILESTONE: TUNAS OK`).

## v0.3 "Batang" — Driver, Storage, Boot 4 Media

- PCI, block-device registry, IDE/ATA PIO, FAT12/16/32 read, PS/2 mouse, pixel
  demo, 4 media boot (`MILESTONE: BATANG` group).

## v0.2 "Akar" — Kernel Core

- Manajemen memori + paging + heap, scheduler + context switch, syscall ABI v1,
  IPC kernel (`MILESTONE: MEMORY/SYSCALL/SCHEDULER/IPC OK`).

## v0.1 "Benih" — Bootloader + Kernel Minimal

- Boot BIOS + UEFI di QEMU, framebuffer console, GDT/IDT/PIC (timer+keyboard),
  boot logo ASCII (`MILESTONE: HELLO KERNEL OK`).

---

## Lisensi

Dirilis di bawah **Lisensi MIT** — lihat berkas [LICENSE](LICENSE).
© 2026 Gravicode Studios (dipimpin oleh Kang Fadhil).
