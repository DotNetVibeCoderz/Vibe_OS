# Buitenzorg OS

> **Codename: Buitenzorg** (nama Belanda lama untuk Bogor — "tanpa kekhawatiran").
> Sistem operasi hibrida & AI-native: **kernel + driver Rust**, **runtime aplikasi, UI, & layanan AI C#**.
>
> **Dibuat oleh [Gravicode Studios](#credits) — dipimpin oleh Kang Fadhil.**

Spesifikasi lengkap: [requirements.md](requirements.md) · Baru mulai? **[docs/tutorial.md](docs/tutorial.md)** · Riwayat rilis: **[CHANGELOG.md](CHANGELOG.md)**

**Status saat ini: v0.1–v0.16 milestone ✓** ("Benih" → "Panen"), **jalur v1.0 "Buitenzorg" (stabilisasi) sedang berjalan**. Kernel boot dari BIOS & UEFI di QEMU pada **empat media (IDE/AHCI/NVMe/USB)**: memori, syscall ABI, scheduler, IPC, PCI, driver IDE + FAT, mouse, C# di ring 3, VFS + FAT write, service/init manager, async I/O, networking, desktop environment, app framework, 4 varian app, `Buitenzorg.Drawing`, Task Manager, theme engine + 8 tema, package manager, compute API, screensaver, personalization, kontrol window, **AI (LLM lokal + CV + GenAI + Model Manager)** + power (v0.12), **virtualisasi (VMM software + guest OS mini + snapshot)** (v0.13), **runtime polyglot (JS/TS/Python)** (v0.14), **managed runtime C# (heap + Buitenzorg.Bcl: koleksi/LINQ + System.IO/Text/Regex/Net/Tasks/…)** (v0.15 "Matang"), dan **v0.16 "Panen": subsistem audio AC'97, `Buitenzorg.UI` (toolkit retained), suite 8 app bawaan (Kalkulator/Editor/2048/Jam/File Manager/Piano/Image Viewer/App Store), desktop shell (Start menu + ikon + jam tray), decoder JPEG**. Jalur **v1.0**: security hardening (validasi pointer syscall), pembekuan ABI, debugger GDB + profiler zona, benchmark CI, boot USB hardware.

![Desktop Buitenzorg v1.0 Shell — galeri model AI + power CLI di terminal](docs/img/desktop-shell.png)

![Screensaver Mystify gaya Windows 3.1/98](docs/img/screensaver-mystify.png)

```
[kernel] Hello Kernel -- Buitenzorg OS v0.1 'Benih'
[kernel] MILESTONE: HELLO KERNEL OK
[kernel] physical memory: 506 MiB usable
[kernel] paging + 1024 KiB kernel heap online
[kernel] MILESTONE: MEMORY OK
[kernel] framebuffer: 1280x720 @ 24 bpp
[kernel] MILESTONE: SYSCALL ABI V1 OK
[task akar-A] round 1/4 (ticks=2)
[task akar-B] round 1/4 (ticks=3)
[task ipc-producer] sent seed 17
[task ipc-consumer] received seed 17
[kernel] MILESTONE: SCHEDULER OK (two tasks alternated preemptively)
[kernel] MILESTONE: IPC OK (3 messages, checksum verified)
[pci] 00:01.1 8086:7010 class 01.01.80 IDE controller
[kernel] MILESTONE: PCI OK (6 devices enumerated)
[driver] block device registered: ata0.0 (QEMU HARDDISK) (5058 sectors)
[fat] batang.txt: Akar menembus tanah, batang menjulang: ...
[kernel] MILESTONE: STORAGE OK (file read from disk via IDE PIO + FAT)
[kernel] MILESTONE: MOUSE OK
[kernel] MILESTONE: PIXELS OK (direct framebuffer drawing)
[kernel] MILESTONE: TUNAS OK (C# ran in ring 3 -> 'Hello from C#!')
[vfs] /ram/DAHAN.TXT round-trip: Dahan tumbuh: VFS + FAT write bekerja...
[kernel] MILESTONE: VFS OK (FAT write + read-back verified on /ram)
[init] service start order: logger -> netd -> storaged -> app
[kernel] MILESTONE: SERVICES OK (dependency-ordered parallel init)
[aio] 2001 ops in <1 tick (>36418 ops/sec)
[kernel] MILESTONE: ASYNC IO OK (io_uring-style SQ/CQ, benchmark-able)
[net] ICMP echo round-trip over loopback: 1 reply
[kernel] MILESTONE: NETWORK OK (Ethernet/ARP/IPv4/ICMP stack)
[kernel] MILESTONE: DAHAN OK (C# service ran as a process)
[kernel] MILESTONE: WINDOWS OK (two windows moved & resized)
[term] $ ls /disk   [term] $ dir /ram   [term] $ cat /ram/DAHAN.TXT
[kernel] MILESTONE: TERMINAL OK (ran ls/dir over VFS)
[kernel] MILESTONE: THEME OK (dark <-> light switch)
[kernel] MILESTONE: KANOPI OK (desktop environment: terminal, theme, multi-desktop)
[kernel] MILESTONE: KEMBANG OK (C# desktop app drew UI via window syscalls)
[kernel] MILESTONE: DRAWING OK (Buitenzorg.Drawing shapes rendered)
[taskmgr] killed idle-demo (PID freed)
[kernel] MILESTONE: TASKMGR OK (process list + resources + kill)
[kernel] MILESTONE: SERBUK OK (System.Drawing library + Task Manager + 4 app variants)
[theme] cycled 10 themes: dark light neo-brutalism clean material bento ... beos
[kernel] MILESTONE: THEMES OK (8 built-in styles + dark/light, live switch)
[kernel] MILESTONE: BUAH OK (theme engine + 8 styles + package manager)
[kernel] MILESTONE: COMPUTE OK (compute API + CPU fallback; GPU backend menyusul)
[kernel] MILESTONE: WINDOWCTL OK (minimize/maximize/close + rounded corners)
[kernel] MILESTONE: CAHAYA OK (GPU compute + window controls + screensaver + personalization)
[nalar/LLM] completion: kernel danop. kai baike...
[nalar] model gallery: 6 models (1 tersedia offline) -> pulled TinyLlama
[kernel] MILESTONE: AI OK (LLM lokal + CV + GenAI + Model Manager)
[power] acpi=true pm1a_cnt=0x604 ... MILESTONE: POWER OK
[kernel] MILESTONE: NALAR OK (AI subsystem + power management)
[kernel] MILESTONE: LAPIS OK (virtualization: software VMM + guest OS)
[kernel] MILESTONE: BABEL OK (polyglot runtime: JS/TS/Python)
[kernel] MILESTONE: BCL OK (Buitenzorg.Bcl) / BCL2 OK (System.IO/Text/Regex/Net/...)
[kernel] MILESTONE: AUDIO OK (AC'97 mixer + PCM) / UI OK / DRAW OK / JPEG OK
[kernel] MILESTONE: SUITE OK (8 preloaded apps) / DESKTOP SHELL OK
[kernel] MILESTONE: SECURITY OK (syscall pointer validation) / PROFILER OK
[kernel] BUITENZORG READY -- terminal ('run calc', 'prof self', 'ask ...', 'vm start nanovm').
```

## Prasyarat

| Alat | Versi | Keterangan |
|---|---|---|
| Rust (rustup) | nightly (otomatis via `kernel/rust-toolchain.toml`) | + target `x86_64-unknown-none` |
| .NET SDK | 10.0+ | runtime, SDK, `bz` CLI |
| QEMU | qemu-system-x86_64 | emulasi utama (§18) |

## Quickstart

**Cara tercepat (nol dependensi):** skrip berikut memasang semua yang
dibutuhkan (Rust, .NET, QEMU, bflat), build, lalu boot di QEMU:

```powershell
.\scripts\quickstart.ps1     # Linux/macOS: ./scripts/quickstart.sh
```

Panduan lengkap ramah-pemula: **[docs/getting-started.md](docs/getting-started.md)** ·
Bikin app pertama: **[docs/first-app.md](docs/first-app.md)** ·
Jalankan di VMware/VirtualBox: **[docs/run-in-vm.md](docs/run-in-vm.md)** ·
Boot USB di hardware: **[docs/install-hardware.md](docs/install-hardware.md)**.

**Alur harian (dependensi sudah terpasang):**

```powershell
# Build semuanya (kernel + image boot + .NET) → dist/
.\scripts\build.ps1          # Linux/macOS: ./scripts/build.sh

# Boot di QEMU (jendela grafis + serial di console)
.\scripts\run-qemu.ps1       # tambah -Uefi untuk boot UEFI/OVMF
cd kernel; cargo run --release -p bzimage -- --run --media nvme   # atau ahci|usb|ide

# Boot smoke test (headless, verifikasi milestone di serial)
.\scripts\smoke-test.ps1     # CI/Linux: ./scripts/smoke-test.sh

# Test
cd kernel; cargo test -p bz-abi         # kontrak ABI sisi Rust
dotnet test Buitenzorg.slnx             # kontrak ABI sisi C# + manifest

# Sample & CLI
dotnet run --project runtime\samples\HelloBuitenzorg
dotnet run --project sdk\bz -- help
dotnet run --project sdk\bz -- new console-csharp MyApp
```

## Struktur Monorepo (§17 "Fondasi & Tooling")

```
kernel/            # Rust workspace (nightly, no_std)
  abi/             #   bz-abi — kontrak syscall ABI v1 (sumber kebenaran)
  bzkernel/        #   kernel ring-0: boot, console, GDT/IDT, memori, heap, syscall
  bzimage/         #   builder image boot (UEFI+BIOS) + runner QEMU
runtime/           # C# managed world
  Buitenzorg.Runtime/        # mirror ABI, backend syscall (native/host-sim), manifest
  Buitenzorg.Runtime.Tests/  # test kontrak ABI & manifest
  samples/HelloBuitenzorg/   # sample C# di host-sim
userland/          # program yang jalan di atas kernel (ring 3)
  hello-csharp/    #   hello/svc/xox/paint/taskmgr/widget/webview.cs + bzdraw.cs
                   #   (Buitenzorg.Drawing) + bzstart.rs (shim) -> *.elf
sdk/               # bz CLI + template app (console/desktop) + VS Code extension
tools/             # toolchain pihak ketiga (bflat) — di-gitignore
ai/                # Layer 6 AI subsystem (mulai v0.12 "Nalar")
apps/              # preloaded suite (mulai v0.16 "Panen")
docs/              # arsitektur, ABI, panduan
scripts/           # build, run-qemu, smoke-test
dist/              # output image boot (di-gitignore)
```

## Arsitektur Singkat

10 layer (requirements.md §3): hardware → bootloader (Rust) → kernel (Rust, ring 0) →
driver → **managed runtime (.NET — jembatan kritis)** → system services (C#) →
AI subsystem (C#) → desktop environment (C#) → app framework polyglot → aplikasi.

Kontrak Rust ↔ C# (§4): C ABI, tabel syscall bernomor stabil (`kernel/abi` ↔
`runtime/Buitenzorg.Runtime/Sys`), zero-copy untuk data besar, GC-aware pinning.
Kedua sisi dijaga test kontrak yang identik — ubah satu sisi tanpa sisi lain = test merah.

## Roadmap & Progress

- **[PLAN.md](PLAN.md)** — roadmap pengembangan produk per-versi (v0.1 → v1.x).
- **[Progress.md](Progress.md)** — tracking checklist fitur (sudah/sebagian/belum).
- Desain teknis penuh: [requirements.md](requirements.md) (§16 roadmap, §17 checklist).

## Dokumentasi

Indeks lengkap: **[docs/README.md](docs/README.md)**. Sorotan:

- **[docs/tutorial.md](docs/tutorial.md)** — tutorial berurutan nol→app (mulai di sini)
- [docs/getting-started.md](docs/getting-started.md) — setup ramah-pemula + quickstart & troubleshooting
- [docs/first-app.md](docs/first-app.md) — bikin app pertama + katalog contoh pakai library built-in
- [docs/run-in-vm.md](docs/run-in-vm.md) — jalankan image di VMware Player & VirtualBox
- [docs/install-hardware.md](docs/install-hardware.md) — tulis image ke USB & boot di komputer fisik (BIOS/UEFI)
- [docs/debugging.md](docs/debugging.md) — debug kernel dgn GDB + profiler zona (TSC)
- [docs/abi.md](docs/abi.md) — tabel syscall ABI v1 & aturan evolusinya
- [CHANGELOG.md](CHANGELOG.md) — riwayat rilis per codename versi
- [CONTRIBUTING.md](CONTRIBUTING.md) — standar koding & alur kontribusi

## Credits

**Buitenzorg OS** dibuat oleh **Gravicode Studios**, dipimpin oleh **Kang Fadhil**.

Atribusi ini juga tampil di dalam OS: pada boot logo, window **Welcome** di
desktop, serta perintah shell `ver` dan `about`.

## Lisensi

Dirilis di bawah **Lisensi MIT** — lihat [LICENSE](LICENSE).
© 2026 Gravicode Studios (dipimpin oleh Kang Fadhil).
