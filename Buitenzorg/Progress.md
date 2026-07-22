# Progress.md — Tracking Pengembangan Buitenzorg OS

> Checklist status pengembangan per-versi & per-subsistem. Verifikasi tiap
> boot lewat marker `MILESTONE: ... OK` (dicek smoke test 4-media).
> Roadmap ke depan: [PLAN.md](PLAN.md) · Desain teknis: [requirements.md](requirements.md).

**Legend:** `[x]` selesai · `[~]` sebagian (lihat catatan) · `[ ]` belum.
**Status keseluruhan:** v0.1 – v0.12 milestone **tercapai**; v0.13 berikutnya.
**Dibuat oleh:** Gravicode Studios — dipimpin oleh Kang Fadhil.

---

## Ringkasan Milestone per Versi

| Versi | Milestone | Marker boot | Status |
|---|---|---|---|
| v0.1 Benih | "Hello Kernel" tampil | `HELLO KERNEL OK` | [x] |
| v0.2 Akar | Dua task bergantian; syscall; IPC | `MEMORY/SYSCALL ABI V1/SCHEDULER/IPC OK` | [x] |
| v0.3 Batang | Boot 4 media, baca file, gambar piksel | `PCI/STORAGE/MOUSE/PIXELS OK` | [x] |
| v0.4 Tunas | "Hello from C#!" di ring 3 | `TUNAS OK` | [x] |
| v0.5 Dahan | Service C# jalan; async I/O benchmark | `VFS/SERVICES/ASYNC IO/NETWORK/DAHAN OK` | [x] |
| v0.6 Daun | Dua window dipindah & di-resize | `WINDOWS OK` | [x] |
| v0.7 Kanopi | Terminal ls/dir, tema, workspace | `TERMINAL/THEME/WORKSPACE/KANOPI OK` | [x] |
| v0.8 Kembang | Desktop app pihak-ketiga jalan | `KEMBANG OK` | [x] |
| v0.9 Serbuk | 4 varian app; Drawing; Task Manager | `DRAWING/TASKMGR/APPVARIANTS/SERBUK OK` | [x] |
| v0.10 Buah | Theme engine, 8 tema, package manager | `THEMES/PACKAGE/BUAH OK` | [x] |
| v0.11 Cahaya | GPU compute + screensaver, personalisasi, micro-interaction, window controls | `COMPUTE/WINDOWCTL/SAVER/PERSONALIZE/CAHAYA OK` | [x] |
| v0.12 Nalar | Subsistem AI-native + Hugging Face; power (shutdown/restart/sleep) | `AI/POWER/NALAR OK` | [x] |
| **v0.13 Lapis** | **Virtualization (hypervisor tipe-2)** | — | **[ ] Berikutnya** |

---

## ✅ v0.1 – v0.8 (Selesai)

### 🧱 Fondasi & Tooling
- [x] Repo + struktur monorepo (kernel, runtime, sdk, userland, ai, apps, docs)
- [x] Toolchain build (cargo + .NET SDK + bflat + rust-lld)
- [x] Pipeline boot QEMU otomatis (`bzimage` + scripts)
- [~] CI (build + test + boot smoke test) — *benchmark performa belum*
- [x] Coding standard & contribution guide

### 🔩 Bootloader & Boot Media
- [x] UEFI init + long mode; legacy BIOS boot
- [x] Load kernel + page table + handoff (BootInfo)
- [x] Boot dari NVMe / SATA-AHCI / IDE-PATA / USB (teruji QEMU)
- [x] Boot logo ASCII Art
- [ ] Boot manager multi-OS · [ ] boot hardware nyata

### ⚙️ Kernel Core
- [x] Serial/VGA + framebuffer console + panic handler
- [x] Physical frame allocator; paging + heap (16 MiB)
- [x] IDT + interrupt + timer (PIT); GDT + TSS + SSE
- [x] Scheduler preemptive + context switch (preemption opsional)
- [x] Syscall ABI (v1 + window ABI v0.8) + entry ring-3 SYSCALL/SYSRET
- [x] IPC (message passing, kernel-space)
- [~] HAL — *implisit x86-64; abstraksi multi-arch belum*
- [ ] SMP multi-core · [ ] APIC · [ ] ekstensi virtualization

### 🔌 Drivers
- [x] PCI enumeration + block-device registry
- [x] Storage IDE/PATA (PIO LBA28)
- [x] Keyboard PS/2 + Mouse PS/2
- [x] Framebuffer graphics + direct pixel
- [ ] Driver NVMe/AHCI/USB native · [ ] Network e1000 · [ ] WiFi · [ ] Audio · [ ] virtio

### 🌉 Managed Runtime (C#)
- [x] Port NativeAOT (via bflat + zerolib, freestanding)
- [x] FFI shim Rust ↔ C# (SYSCALL/SYSRET + `bzstart.rs`)
- [x] Syscall wrapper C# (`BzSys`) + kontrak ABI dua sisi teruji
- [x] ELF64 loader + ring-3 user-space + lifecycle (load→run→unmap)
- [x] "Hello from C#" di bare metal
- [ ] **Integrasi GC ↔ memory manager + pinning** — *app tanpa GC (stackalloc)*
- [ ] **CoreCLR + JIT (fitur C# lengkap)** · [ ] reflection/dynamic loading
- [ ] async/await + threading map ke scheduler · [ ] BCL penuh

### 🗂️ System Services
- [x] VFS (mount table `/disk` ro + `/ram` rw)
- [x] FAT12/16/32 read + FAT12 write (teruji round-trip)
- [x] Service/init manager (parallel, dependency-aware)
- [~] Async I/O io_uring-style (SQ/CQ + benchmark) — *zero-copy & syscall submit belum*
- [~] Networking (Ethernet/ARP/IPv4/ICMP loopback) — *TCP/UDP & driver NIC belum*
- [ ] Filesystem kustom journaled · [ ] permission broker · [ ] package manager

### 🖼️ Graphics & Window System
- [x] Compositor (double-buffered, present)
- [~] Window manager floating (title bar, z-order, move, resize) — *tiling belum*
- [x] Event routing (mouse → window; keyboard → terminal)
- [~] Rendering: rect/outline/gradient/alpha/teks — *blit gambar & path belum*
- [ ] Animasi · [ ] compositor GPU (v0.11)

### 🖥️ Desktop Environment
- [~] Shell / taskbar — *workspace switcher + window buttons + theme label; launcher/tray belum*
- [x] Multi-desktop / workspaces (4) + wallpaper per-desktop
- [x] Dark/Light system theme (toggle)
- [x] Terminal + shell (ls/dir, cat/type, cd, pwd, echo, mounts, clear/cls, ver, theme, ws, run, bz)
- [ ] Notifikasi/action center · [ ] Settings app · [ ] Global search
- [ ] Pipes/redirection/history/tab-completion · [ ] kompatibilitas `.sh`

### 🧰 App Framework & SDK
- [x] App manifest & lifecycle
- [x] SDK + CLI (`bz new/manifest`, shell `run`)
- [x] Window syscall ABI (WIN_CREATE/CMD/PRESENT + KEY_READ) + `DrawCmd`
- [x] Template Console (console-csharp) + Desktop (desktop-csharp + `BzUi`)
- [x] Template XOX (Tic-Tac-Toe) — desktop app menggambar via syscall
- [~] VS Code extension — *skeleton (new/build-run/validate); preview/IntelliSense/DAP belum*
- [ ] UI Toolkit XAML (binding/MVVM) · [ ] template Utilities/Calculator/Minesweeper
- [ ] WebView + web app · [ ] widget host (v0.9)
- [ ] Debug bridge + DAP debugging

---

## ✅ v0.9 "Serbuk" (Selesai)

### 🌐 Varian App Web & Widget
- [~] WebView engine + web app runtime — *mini WebView (subset HTML: h1/h2/p/li/hr/button); engine HTML/CSS/JS penuh belum*
- [x] Widget host & board (docked kanan) + widget monitor sistem
- [ ] Template web + template widget di SDK — *app contoh ada; template SDK belum*
- [x] Milestone: **keempat varian app (console/desktop/web/widget) berjalan** (`APPVARIANTS OK`)

### 🎨 Library Grafik — `Buitenzorg.Drawing` (mirip System.Drawing)
- [x] Tipe dasar: `Graphics`, `Color`, `Pen`, `Brush`, `Point`, `Rectangle`, `Size`
- [x] Bentuk: `DrawLine`, `DrawRectangle`/`FillRectangle`, `DrawEllipse`/`FillEllipse`
- [x] Teks: `DrawString` + `DrawChars` (tanpa heap)
- [ ] Gambar: `Bitmap` + `DrawImage` (blit) · [ ] `GraphicsPath` + transform · [ ] `Font` custom
- [x] Primitif syscall tambahan (draw op: LINE, ELLIPSE, FILL_ELLIPSE, RECT) — append-only ABI
- [x] Milestone: **app (Paint) menggambar dengan API bergaya System.Drawing** (`DRAWING OK`)

### 📊 Task Manager / Monitor Sistem (gaya Windows)
- [x] Kernel: registry proses (kernel task + user app) + metadata (id, nama, state)
- [x] Kernel: akuntansi CPU-time per-task + statistik memori (heap used/total)
- [x] ABI baru (append-only): `PROC_LIST` (10), `PROC_KILL` (11), `SYS_STAT` (12)
- [x] App: daftar proses (PID, nama, kind, CPU-ticks) + ringkasan uptime/heap/RAM + bar
- [x] App: aksi **Kill** proses (idle-demo di-kill saat demo)
- [~] Tab Processes/Performance/Details terpisah — *satu panel gabungan; tab belum*
- [x] Milestone: **Task Manager menampilkan proses & sumber daya; bisa kill proses** (`TASKMGR OK`)

> Catatan: model ring-3 masih satu app run-to-completion. Task manager memantau
> task kernel + app aktif dan bisa kill task kernel; kill app lain penuh
> menyusul saat multi-proses preemptive matang (lihat backlog PLAN.md).

---

## ✅ v0.10 "Buah" (Selesai)

### 🎨 Theme Engine & 8 Tema
- [x] Theme engine (design token: warna + style: border/title/shadow/gradient) — live switch
- [x] 8 tema: Neo Brutalism · Clean Design · Material · Bento · Classic Linux · Classic Windows · Sun Microsystem · BeOS (+ dark/light)
- [x] Render per-style di compositor (title flat/beveled/tab, shadow soft/hard-offset, border, desktop flat/gradient)
- [x] `theme <nama|cycle|list>` di shell
- [ ] `.theme` package pihak-ketiga + theme SDK · [ ] hot-reload dari file · [ ] icon pack system
- [x] Milestone: **ganti antar 8 tema secara live** (`THEMES OK`)

### 📦 Package Manager
- [x] Registry paket (nama, versi, deskripsi, ELF, kind) — `pkg.rs`
- [x] `bz install`/`remove`/`list`/`search` — install/uninstall dari registry
- [x] `run` di-gate oleh status terpasang (app registry harus di-install dulu)
- [ ] Sandbox per-app · [ ] dependency resolution · [ ] app store GUI · [ ] update
- [x] Milestone: **install app dari registry** (`PACKAGE OK`)

---

## ✅ v0.11 "Cahaya" (Selesai)

### ⚡ Compute API (irisan GPU)
- [x] Compute API (SAXPY, blend) + backend CPU (SIMD-friendly) — `compute.rs`
- [x] Interface siap backend GPU (enum Backend Cpu/Gpu) + fallback CPU
- [ ] Driver GPU hardware nyata · [ ] compositor GPU-accelerated · [ ] zero-copy GPU memory — *besar; menyusul (interface siap)*
- [x] Milestone: **compute API + CPU fallback** (`COMPUTE OK`)

### 🌌 Screen Saver
- [x] Framework screensaver (idle timeout ~12s, nonaktif saat input) + overlay full-screen
- [x] Idle detection (mouse/keyboard) di desktop loop
- [x] Screensaver bawaan gaya Win 3.1/98: Starfield, Mystify, 3D Pipes, Marquee, Bouncing, Blank
- [x] Konfigurasi: `saver <nama|list|off>` · [ ] preview di app · [ ] set timeout via UI
- [x] Milestone: **screensaver idle-activated** (`SAVER OK`)

### 🖼️ Personalization & Display Settings
- [~] Antarmuka Personalization via shell (`settings`, `bg`, `saver`, `cursor`, `anim`, `theme`) — *app GUI belum*
- [x] Desktop background: **wallpaper bawaan** (theme, waves, grid, dots, aurora)
- [x] Desktop background: **file gambar milik user** (BMP 24-bit dari VFS, mis. `/disk/PHOTO.BMP`)
- [x] Pilih screensaver · [x] pilih theme (8 tema + dark/light) · [x] kursor (normal/besar)
- [x] Opsi: animasi on/off, rounded on/off
- [ ] Display resolusi/scaling · [ ] simpan preferensi ke VFS (persist) · [ ] app GUI
- [x] Milestone: **atur background/tema/saver/kursor** (`PERSONALIZE OK`)

### ✨ Micro-interactions (UI/UX)
- [x] Hover highlight (tombol kontrol window)
- [x] Feedback klik: **ripple** beranimasi di titik klik
- [x] Loop desktop beranimasi (redraw kontinu saat animasi/screensaver)
- [ ] Animasi buka/tutup/minimize window (scale/fade) · [ ] transisi tema/workspace
- [x] Opsi "reduce motion" (`anim off`)
- [x] Milestone: **micro-interaction aktif & bisa dimatikan** (`MICROINT OK`)

### 🪟 Kontrol Window (rounded + min/max/close)
- [x] Tombol title bar di setiap window: **minimize**, **maximize/restore**, **close**
- [x] State window: normal / minimized (ke taskbar) / maximized (isi workspace)
- [x] Restore/focus dari taskbar (klik tombol window)
- [x] **Sudut window membulat** (rounded corners, per-theme)
- [ ] Double-click title bar → maximize · [ ] anti-alias sudut
- [x] Milestone: **min/max/close + sudut membulat** (`WINDOWCTL OK`)

> Bonus: memperbaiki bug laten sejak v0.4 — IF (interrupt flag) tetap mati
> setelah app ring-3 keluar, mematikan timer di desktop loop (desktop
> interaktif/screensaver/animasi tak jalan). `enter_user` kini re-enable
> interrupts setelah app keluar.

---

## ✅ v0.12 "Nalar" (Selesai)

### 🧠 AI Subsystem
- [x] LLM engine lokal (offline-first) — `ai.rs`: model bigram char-level nyata, jalan di kernel
- [x] Computer vision — deteksi tepi Sobel (`vision_edges`)
- [x] GenAI: text-to-image prosedural (`genai_image`)
- [x] AI System API seragam (`ai::llm_complete`/`vision_edges`/`genai_image`) · shell `ask`
- [x] Model Manager + galeri Hugging Face-style (`model.rs`, `bz model list/pull/info`)
- [ ] LLM skala produksi (GGUF/ONNX + GPU/NPU) · inference scheduler · CV/GenAI audio/video · sandbox model · download nyata (multi-GB)
- [x] Milestone: **LLM lokal jalan & "unduh" model dari galeri** (`AI OK`)

### 🔌 Power Management (Shutdown / Restart / Sleep)
- [x] Parser ACPI minimal (RSDP dari bootloader → RSDT/XSDT → FADT; scan DSDT `\_S5`)
- [x] **Shutdown** — ACPI PM1a_CNT (SLP_TYP|SLP_EN) + fallback QEMU (0x604/0xB004) / VBox — **teruji: QEMU power off**
- [x] **Restart** — ACPI reset register + reset keyboard-controller (0x64←0xFE) + triple-fault last resort
- [x] **Sleep** — light sleep (blank + `hlt` sampai input mouse/keyboard)
- [x] CLI: `shutdown`/`restart`/`sleep` + `bz power off|restart|sleep`
- [ ] ACPI S3 (suspend-to-RAM) · flush VFS + app save-state · item power di taskbar/Start GUI + konfirmasi
- [x] Milestone: **Shutdown, Restart, Sleep dari shell/`bz power`** (`POWER OK`; shutdown teruji QEMU)

---

## 🔜 v0.13 "Lapis" (Berikutnya)

### 📦 Virtualization
- [ ] Hypervisor tipe-2 (VT-x/AMD-V) · virtualization manager · virtio · snapshot & virtual disk · guest tools

---

## ⏳ v0.14+ (Rencana — ringkas)

### 🗣️ Polyglot (v0.14)
- [ ] JS/TS engine + transpile · Python (CPython/IronPython) · binding seragam

### 🌾 Preloaded Suite & Optimasi (v0.15)
- [ ] Utilities/multimedia/AI apps/games/themes/productivity/store bawaan
- [ ] Fast boot · fast I/O · startup cepat · footprint · SIMD · benchmark regresi CI

### 🚀 Stabilisasi & Rilis (v1.0)
- [ ] Stabilkan API/ABI · security hardening · debugger + profiler · dokumentasi
- [ ] Boot hardware nyata · image resmi VMware/QEMU/Hyper-V/VirtualBox

### 🌍 Multi-Arch (v1.x)
- [ ] Port ARM64 · Port RISC-V · SMP/NUMA · marketplace · container lanjut
