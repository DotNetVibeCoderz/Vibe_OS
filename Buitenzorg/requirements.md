# Buitenzorg OS — Desain Arsitektur & Roadmap Pengembangan

> **Codename: Buitenzorg** (nama Belanda lama untuk Bogor — "tanpa kekhawatiran")
> Sistem operasi hibrida & **AI-native**: **kernel + driver ditulis dengan Rust**, **runtime aplikasi, UI, & layanan AI ditulis dengan C#**.
> Terinspirasi konsep Cosmos, tapi dengan dukungan C# lengkap, akselerasi GPU, LLM lokal built-in, computer vision, GenAI (audio/video/image), integrasi Hugging Face, dukungan aplikasi multi-bahasa (C#/JS/TS/Python), multi-desktop, tema dark/light, virtualization, dan boot dari NVMe/SATA/IDE/USB.
>
> **Tema versi:** mengikuti pertumbuhan tanaman (penghormatan pada Kebun Raya Bogor): benih → akar → batang → daun → kanopi → kembang → buah → panen.
>
> **Dibuat oleh Gravicode Studios — dipimpin oleh Kang Fadhil.**

---

## Daftar Isi
1. [Filosofi & Prinsip Desain](#1-filosofi--prinsip-desain)
2. [Kenapa Rust + C#](#2-kenapa-rust--c)
3. [Arsitektur Sistem (Layered)](#3-arsitektur-sistem-layered)
4. [Interop Rust ↔ C# (Jembatan Kritis)](#4-interop-rust--c-jembatan-kritis)
5. [Strategi Runtime C# & Polyglot](#5-strategi-runtime-c--polyglot)
6. [Subsistem AI-Native](#6-subsistem-ai-native)
7. [Akselerasi GPU & Compute](#7-akselerasi-gpu--compute)
8. [Boot, Storage & Performa](#8-boot-storage--performa)
9. [Virtualization](#9-virtualization)
10. [Daftar Fitur Lengkap](#10-daftar-fitur-lengkap)
11. [Model Aplikasi (4 Varian, Multi-Bahasa)](#11-model-aplikasi-4-varian-multi-bahasa)
12. [Preloaded Suite (Apps, Games, Themes, dll)](#12-preloaded-suite)
13. [VS Code Extension, Template & Debugging](#13-vs-code-extension-template--debugging)
14. [Default Terminal & Shell](#14-default-terminal--shell)
15. [Galeri Tema (8 Style)](#15-galeri-tema-8-style)
16. [Roadmap Pengembangan per Versi](#16-roadmap-pengembangan-per-versi)
17. [Checklist Tracking Development](#17-checklist-tracking-development)
18. [Testing & VM Images](#18-testing--vm-images)
19. [Tech Stack & Tooling](#19-tech-stack--tooling)
20. [Risiko & Tantangan](#20-risiko--tantangan)

---

## 1. Filosofi & Prinsip Desain

| Prinsip | Penjelasan |
|---|---|
| **Safety by default** | Kernel & driver di Rust → memory safety tanpa GC di ring 0. |
| **Productivity by default** | Aplikasi, UI, & AI di C# → developer produktif, ekosistem .NET kaya. |
| **AI-native** | Inference LLM/CV/GenAI adalah layanan sistem kelas satu, bukan add-on. |
| **Fast & light** | Fast boot, fast I/O, footprint kecil; optimasi jadi kebijakan, bukan afterthought. |
| **Clear boundary** | Batas tegas "unsafe world" (Rust, ring 0) vs "managed world" (C#, ring 3) lewat ABI stabil. |
| **Microkernel-leaning** | Driver & service sebisa mungkin user-space agar crash terisolasi. |
| **Batteries included** | Theme engine, package manager, multi-desktop, preloaded suite, SDK — bawaan. |
| **Polyglot friendly** | Satu app model, banyak bahasa: C#, JS, TypeScript, Python. |
| **Portable** | HAL dirancang lintas-arsitektur sejak awal: x86-64 → ARM64 → RISC-V. |

---

## 2. Kenapa Rust + C#

**Rust** menangani lapisan tanpa GC & butuh kontrol memori manual: bootloader, kernel, memory manager, scheduler, interrupt, driver hardware. Precedent: **Redox OS**, Rust-for-drivers di Windows & Linux.

**C#** menangani lapisan butuh produktivitas & ekspresivitas: runtime aplikasi, UI toolkit, desktop environment, layanan AI, SDK. Precedent: **Cosmos** (IL2CPU), **Singularity** (Microsoft Research), **.NET NativeAOT**.

**Beda dari Cosmos:** Cosmos menaruh kernel di C# via IL2CPU. Buitenzorg memisahkan tanggung jawab — kernel keras di Rust (lebih aman & mature), sementara C# fokus penuh di user-space dengan runtime .NET **lengkap** (reflection, LINQ, async/await, dynamic loading, generics bekerja penuh).

---

## 3. Arsitektur Sistem (Layered)

```
┌──────────────────────────────────────────────────────────────────────────┐
│  LAYER 9 — APLIKASI (C# / JS / TS / Python)                               │
│  Console · Desktop · Web · Widget   +   Preloaded Suite                    │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 8 — APPLICATION FRAMEWORK / SDK (Polyglot)                         │
│  UI Toolkit(XAML) · WebView · Widget Host · Console API                    │
│  Language Runtimes: .NET(C#) · JS/TS(engine) · Python(CPython)             │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 7 — DESKTOP ENVIRONMENT (C#)                                        │
│  Shell · Taskbar · Launcher · Multi-Desktop · Settings · Widget Board      │
│  Dark/Light System Theme                                                   │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 6 — AI SUBSYSTEM (C#)  ◄── AI-NATIVE                               │
│  LLM Engine(lokal) · Computer Vision · GenAI(audio/video/image)           │
│  Model Manager + Hugging Face Gallery · Inference Scheduler                │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 5 — SYSTEM SERVICES (C#, user-space)                               │
│  Compositor · Window Mgr · Theme Engine · Package Mgr · VFS Service        │
│  Init/Service Mgr · Security Broker · Networking · Virtualization Mgr      │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 4 — MANAGED RUNTIME (Jembatan Rust ↔ C#)                           │
│  .NET Runtime(CoreCLR/NativeAOT) · GC · BCL · Interop/FFI Shim             │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 3 — DRIVER SUBSYSTEM (Rust; sebagian C# user-space)               │
│  Storage(NVMe/SATA/IDE/USB) · Net · GPU/Compute · Input · Audio           │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 2 — KERNEL (Rust, ring 0)                                          │
│  Memory · Scheduler · Interrupts · IPC · Syscall · HAL · Virtualization    │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 1 — BOOTLOADER (Rust, UEFI + legacy)                              │
│  Boot dari: NVMe · SATA · IDE · USB                                        │
├──────────────────────────────────────────────────────────────────────────┤
│  LAYER 0 — HARDWARE  (x86-64 → ARM64 → RISC-V)                           │
└──────────────────────────────────────────────────────────────────────────┘
```

Perubahan dari desain sebelumnya: penambahan **Layer 6 AI Subsystem**, **GPU/Compute** di driver, **Virtualization** di kernel & services, **polyglot runtimes** di app framework, dan multi-arch (RISC-V) di layer hardware.

---

## 4. Interop Rust ↔ C# (Jembatan Kritis)

```
   C# (managed)                    Rust (unsafe / ring 0)
 ┌──────────────┐   P/Invoke     ┌──────────────────────┐
 │  App / BCL   │ ─────────────► │  FFI Shim (extern C) │
 │  Syscall API │ ◄───────────── │  Syscall dispatcher  │
 └──────────────┘   marshal      └──────────────────────┘
```

**Aturan desain:**
1. **ABI = C ABI.** Semua panggilan lintas-bahasa lewat `extern "C"` + `[DllImport]`/syscall.
2. **Marshalling minimal.** Hanya primitif, pointer, struct `#[repr(C)]` yang menyeberang.
3. **Syscall sebagai kontrak.** Tabel syscall bernomor stabil (mirip Linux).
4. **Zero-copy buffer.** Data besar (framebuffer, file, tensor AI) lewat shared memory, bukan copy — penting untuk performa I/O & GPU/AI.
5. **GC-aware pinning.** Objek managed di-*pin* saat pointernya dikirim ke Rust.

---

## 5. Strategi Runtime C# & Polyglot

### 5.1 Runtime C#
| Fase | Teknik | Kelebihan | Kekurangan |
|---|---|---|---|
| **Awal** | NativeAOT | Tanpa JIT, footprint kecil, boot cepat | Reflection dinamis terbatas |
| **Menengah** | CoreCLR + JIT (RyuJIT) | Fitur C# LENGKAP: reflection, dynamic assembly | Berat di-port |
| **Target** | Hybrid: services=NativeAOT, apps=CoreCLR/JIT | Terbaik dari keduanya | Maintain dua jalur |

### 5.2 Polyglot (JS / TypeScript / Python)
Agar app bisa dibuat dengan banyak bahasa, disediakan **language runtimes** di Layer 8:
- **JavaScript/TypeScript** — embed JS engine (mis. QuickJS/V8-like); TS ditranspile ke JS. Cocok untuk web app & widget.
- **Python** — port CPython (atau IronPython di atas CLR). Cocok untuk scripting, tooling, dan app AI (banyak library ML memakai Python-style API).
- Semua bahasa memakai **app model & manifest yang sama** dan mengakses API sistem via binding yang seragam.

```
              ┌─────────── App Model & Manifest (seragam) ───────────┐
   C# ──►.NET │  JS/TS ──► JS Engine │  Python ──► CPython/IronPython │
              └──────────────── System API Bindings ─────────────────┘
```

---

## 6. Subsistem AI-Native

Buitenzorg menjadikan AI **fitur kelas satu di level OS** (Layer 6), bukan aplikasi terpisah.

### 6.1 Komponen
- **LLM Engine (lokal)** — menjalankan model bahasa di perangkat (offline-first), dipercepat GPU/NPU. Format: GGUF/ONNX/safetensors. Digunakan oleh: asisten sistem, fitur teks di app, dev tools.
- **Computer Vision** — deteksi objek, OCR, segmentation, face/scene understanding untuk kamera & gambar.
- **GenAI Multimodal**:
  - *Image* — text-to-image, upscaling, background removal, inpainting.
  - *Audio* — TTS, STT (speech-to-text), musik/efek suara generatif.
  - *Video* — generasi & editing pendek, frame interpolation.
- **Inference Scheduler** — mengatur antrean & alokasi GPU/NPU antar-app secara adil, dengan prioritas & batasan memori.
- **AI System API** — API C#/JS/TS/Python yang seragam agar app apa pun bisa memanggil LLM/CV/GenAI.

### 6.2 Integrasi Hugging Face (Model Gallery)
- **Model Manager** dengan galeri terintegrasi Hugging Face Hub: telusuri, unduh, kelola, dan update model langsung dari OS.
- Metadata model (ukuran, lisensi, kebutuhan VRAM, tugas) ditampilkan sebelum unduh.
- Verifikasi checksum & lisensi; sandbox saat menjalankan model.
- Cache lokal + resume download; konversi otomatis ke format inference yang didukung bila memungkinkan.
- **Offline-first**: model yang sudah diunduh berjalan tanpa internet.

### 6.3 Privasi
Inference lokal secara default (data tak keluar perangkat). Permission broker mengatur akses app ke model, kamera, mikrofon, dan galeri.

---

## 7. Akselerasi GPU & Compute

- **Driver GPU** (Rust, kernel/user-space) + **HAL grafis** untuk portabilitas.
- **Compositor akselerasi GPU** — animasi, transparansi, efek desktop mulus & hemat CPU.
- **Compute API** untuk workload paralel (dipakai AI subsystem & app): abstraksi mirip Vulkan compute / WebGPU.
- **Zero-copy** antara app ↔ GPU memory untuk render & tensor AI.
- **Fallback CPU** bila GPU tak tersedia (SIMD-optimized) agar tetap jalan di VM/hardware minim.
- Target bertahap: framebuffer → 2D akselerasi → 3D → GPU compute (untuk AI).

---

## 8. Boot, Storage & Performa

### 8.1 Boot & Storage
- **Bootloader** mendukung UEFI (utama) + legacy, dan boot dari media:
  - **NVMe**, **SATA (AHCI)**, **IDE (PATA)**, **USB**.
- Deteksi otomatis media boot; boot manager sederhana bila ada banyak OS.
- Filesystem: mulai FAT32 (untuk USB/EFI) & FS baca ext-like, lalu **filesystem kustom** yang cepat & journaled.

### 8.2 Fast Boot
- Parallel init service (dependency-aware, bukan serial).
- Lazy-loading service non-esensial.
- NativeAOT untuk komponen boot (tanpa warm-up JIT).
- Target: waktu boot ke desktop yang kompetitif.

### 8.3 Fast I/O
- Asynchronous I/O (io_uring-style) end-to-end.
- Zero-copy path untuk file & jaringan.
- Buffer/cache cerdas, batching, dan DMA pada driver storage.

### 8.4 Optimasi Umum (OS & Apps)
- Profil & benchmark jadi bagian CI (regresi performa terdeteksi dini).
- Startup app cepat (AOT untuk app kritis; snapshot/warm-start).
- Footprint memori kecil (service ringan, share library).
- SIMD & vektorisasi untuk jalur panas (grafis, AI, kompresi).
- Ukuran image OS ramping; komponen opsional dimuat sesuai kebutuhan.

---

## 9. Virtualization

- **Dukungan menjalankan VM** di atas Buitenzorg: hypervisor tipe-2 memakai ekstensi CPU (VT-x/AMD-V; nanti ARM/RISC-V equivalent).
- **Virtualization Manager** (service C#) untuk buat/kelola/jalankan VM, snapshot, dan virtual disk.
- Paravirtualized drivers (virtio) untuk I/O cepat pada guest.
- Guest tools untuk integrasi (clipboard, resize, shared folder).
- **Buitenzorg sebagai guest** juga dioptimalkan (lihat §15) agar mulus di VMware/QEMU/Hyper-V/VirtualBox.

---

## 10. Daftar Fitur Lengkap

### 10.1 Kernel & Core (Rust)
- Bootloader UEFI + legacy, boot dari NVMe/SATA/IDE/USB
- Manajemen memori virtual (paging, demand paging, CoW)
- Scheduler preemptive + prioritas + SMP multi-core
- Interrupt & timer (APIC)
- IPC (message passing)
- Syscall ABI stabil
- HAL multi-arsitektur (x86-64 → ARM64 → RISC-V)
- Ekstensi virtualization (VT-x/AMD-V)

### 10.2 Driver
- Framework driver (kernel-space & user-space terisolasi)
- Storage: NVMe, SATA/AHCI, IDE/PATA, USB Mass Storage
- Input: keyboard, mouse, touchpad, touchscreen
- Display & **GPU** (framebuffer → 2D/3D → compute)
- Network: Ethernet, WiFi
- Audio: output & input
- SDK driver dalam C# (user-space)
- virtio drivers (untuk guest & host virtualization)

### 10.3 Runtime & Sistem
- Runtime .NET penuh (reflection, LINQ, async/await, generics, dynamic loading)
- **Polyglot runtimes**: JS/TS engine, Python (CPython/IronPython)
- Garbage Collector terintegrasi
- VFS + filesystem (FAT32 → FS kustom cepat & journaled)
- Package Manager (install/update/remove, dependency, sandbox)
- Service/Init Manager (parallel, dependency-aware) untuk fast boot
- Security: permission per-app, sandbox, capability-based
- Networking stack (TCP/IP), async I/O
- **Virtualization Manager**

### 10.4 AI-Native (Layer 6)
- LLM engine lokal (offline-first, GPU/NPU accelerated)
- Computer Vision (OCR, deteksi, segmentation)
- GenAI: image, audio (TTS/STT), video
- Inference scheduler (alokasi GPU adil antar-app)
- **Model Manager + galeri Hugging Face**
- AI System API seragam untuk semua bahasa

### 10.5 UI, Tema & Desktop
- Compositor akselerasi GPU (transparansi, animasi)
- Window Manager (tiling + floating)
- **Multi-Desktop / Workspaces** (wallpaper per-desktop, gesture)
- **Dark/Light System Theme** (auto/manual)
- Theme Engine: design tokens, hot-reload, tema kustom `.theme`, accent color
- Icon pack system
- Notifikasi & action center
- Shell: taskbar, launcher, tray, global search

### 10.6 Developer Experience
- SDK multi-bahasa (C#/JS/TS/Python) + template (4 varian app)
- **VS Code extension + template** (Minesweeper, XOX, utilities, console app, calculator, Notes web, widget) — lihat §13
- **Debugging app dari VS Code** (DAP: breakpoint, step, watch, remote attach)
- Debugger & profiler (termasuk profil performa)
- **Default Terminal** familiar Windows+Linux (command populer) — lihat §14
- `bz` CLI (kelola app/paket/tema/model AI/VM)
- Dokumentasi API + contoh
- App Store / registry paket

---

## 11. Model Aplikasi (4 Varian, Multi-Bahasa)

Semua varian berbagi runtime, SDK, & manifest yang sama; bisa ditulis dengan **C#, JS, TypeScript, atau Python**.

### 11.1 Console App
Teks, jalan di terminal. API: stdin/stdout/stderr, ANSI, arg parsing. Use case: tool, script, worker, AI CLI.

### 11.2 Desktop App
GUI native pakai UI Toolkit XAML-based (binding, MVVM, kontrol, tema sistem). Use case: produktivitas, editor, media player, app AI.

### 11.3 Web App
Jalan di WebView + backend C#/JS/TS (Blazor-style / SPA). Use case: dashboard, porting web app.

### 11.4 Widget
Komponen ringan deklaratif di widget board. Update periodik, izin terbatas. Use case: jam, cuaca, monitor sistem, asisten AI mini.

**Manifest terpadu:**
```json
{
  "id": "com.example.myapp",
  "name": "My App",
  "type": "desktop",              // console | desktop | web | widget
  "language": "csharp",           // csharp | js | ts | python
  "version": "1.0.0",
  "permissions": ["filesystem.read", "network", "ai.llm", "camera"],
  "theme": "system"
}
```

---

## 12. Preloaded Suite

OS terpasang dengan aplikasi & konten bawaan siap pakai:

- **Utilities** — File Manager, Terminal, Settings, Task/System Monitor, Text Editor, Calculator, Archive Manager, Screenshot.
- **Multimedia** — Music Player, Video Player, Image Viewer, Camera, Voice Recorder, (opsional) AI media editor.
- **AI Apps** — Asisten (LLM lokal), Model Gallery (Hugging Face), Image/Audio/Video generator, OCR.
- **Games** — beberapa game ringan bawaan (puzzle, kartu, arcade sederhana) sebagai showcase toolkit.
- **Themes** — koleksi tema dark & light + 8 style bawaan (Neo Brutalism, Clean Design, Material, Bento, Classic Linux, Classic Windows, Sun Microsystem, BeOS — lihat §15) + wallpaper.
- **Productivity** — Notes, Calendar, Clock, Web Browser.
- **Store** — App Store untuk menambah app/tema/game.

Semua ringan & mengikuti sistem tema (dark/light).

---

## 13. VS Code Extension, Template & Debugging

Developer membangun app Buitenzorg langsung dari **Visual Studio Code** memakai extension resmi + kumpulan template.

### 13.1 Extension "Buitenzorg SDK for VS Code"
- **Project scaffolding** — buat project baru dari template lewat Command Palette (`Buitenzorg: New Project`).
- **Build & Run** — build app dan langsung jalankan di **emulator/VM** (QEMU) atau perangkat target dengan satu klik.
- **Debugging** — integrasi **Debug Adapter Protocol (DAP)**: breakpoint, step over/into/out, watch, call stack, variable inspection, hot-reload untuk UI/web/widget. Bisa attach ke proses app yang jalan di VM/OS (remote debugging via debug bridge).
- **Manifest editor** — form & validasi untuk `app.manifest` (type, language, permissions, theme).
- **Preview** — live preview untuk desktop/web/widget UI.
- **AI-assist** (opsional) — akses AI System API OS untuk generate boilerplate/asset.
- **Multi-bahasa** — mendukung C#, JS, TypeScript, Python (sesuai polyglot runtime OS).
- **Snippets & IntelliSense** — definisi API sistem agar autocomplete jalan.

### 13.2 Template Bawaan
| Template | Varian App | Bahasa contoh | Tujuan belajar |
|---|---|---|---|
| **Minesweeper** | Desktop/Widget | C# (UI Toolkit) | Grid, event input, state game, rendering |
| **XOX (Tic-Tac-Toe)** | Desktop/Web | C# / JS-TS | Logika game sederhana, turn, win-check, UI binding |
| **Utilities** | Desktop | C# | Akses sistem (file, proses), layout tool |
| **Console System App** | Console | C# / Python | stdin/stdout, argument, syscall/sys-info API |
| **Calculator** | Desktop/Widget | C# | UI grid tombol, state, parsing ekspresi |
| **Notes (Web App)** | Web | TS + C# backend | WebView, penyimpanan lokal, CRUD, sync tema |
| **Widget** | Widget | C# / JS | Widget board, update periodik, izin terbatas |

Setiap template datang lengkap dengan `app.manifest`, struktur folder, README, dan konfigurasi debug (`launch.json`) siap pakai.

### 13.3 Debug Bridge
Komponen di OS (mirip `adb`) yang membuka channel debug ke VS Code:
- Deploy app ke VM/perangkat, pasang breakpoint, streaming log.
- Profiling performa & memori dari dalam VS Code.
- Bekerja lewat jaringan (VM) atau lokal.

---

## 14. Default Terminal & Shell

Buitenzorg punya **terminal bawaan** yang terasa familiar bagi pengguna Windows *dan* Linux.

### 14.1 Terminal Emulator
- Tab, split-pane, scrollback, pencarian, copy/paste.
- Font ligatur, warna 24-bit (truecolor), dukungan ANSI/VT100.
- Mengikuti sistem tema (dark/light) & profil kustom.
- GPU-accelerated rendering untuk teks (cepat & mulus).

### 14.2 Shell & Command Support
Shell bawaan mendukung **command populer lintas-ekosistem** agar tidak asing:

- **Gaya Unix/Linux:** `ls`, `cd`, `pwd`, `cp`, `mv`, `rm`, `mkdir`, `cat`, `grep`, `find`, `chmod`, `ps`, `kill`, `top`, `df`, `du`, `echo`, `touch`, `head`, `tail`, `less`, `man`, `curl`, `ping`, `tar`, `ssh`.
- **Gaya Windows (alias):** `dir`, `cls`, `copy`, `move`, `del`, `type`, `tasklist`, `ipconfig` — dipetakan ke perilaku setara.
- **Fitur shell:** pipes `|`, redirection `> >> <`, environment variables, globbing/wildcard, command history, tab-completion, aliases, scripting sederhana.
- **Command khas Buitenzorg:** `bz` CLI untuk kelola app/paket/tema/model AI/VM — mis. `bz app install`, `bz theme set neo-brutalism`, `bz model pull <hf-model>`, `bz vm start`.

### 14.3 Kompatibilitas
- POSIX-leaning agar banyak script `.sh` bisa jalan.
- Package manager terintegrasi (`bz`) untuk memasang tool CLI tambahan.

---

## 15. Galeri Tema (8 Style)

Selain dark/light system theme, tersedia koleksi tema dengan karakter visual berbeda. Semua dibangun di atas **design tokens** theme engine (warna, tipografi, spacing, radius, shadow) sehingga bisa hot-reload & punya varian dark/light bila relevan.

| Tema | Karakter Visual |
|---|---|
| **Neo Brutalism** | Border tebal hitam, warna blok berani, shadow keras (offset solid), tipografi besar, tanpa gradient — mentah & tegas. |
| **Clean Design** | Minimalis, banyak white-space, netral, tipografi ringan, elemen tipis — fokus pada konten. |
| **Material Design** | Elevation/shadow berlapis, ripple effect, warna primer/aksen, FAB, motion terukur — gaya Google. |
| **Bento Layout** | Panel kartu modular dengan sudut membulat tersusun grid rapat (bento box), tiap "kotak" satu fungsi. |
| **Classic Linux** | Nuansa retro GTK/KDE lawas / CDE-ish: panel abu, taskbar klasik, ikon tradisional. |
| **Classic Windows** | Gaya Windows klasik (95/2000/XP-ish): title bar biru, tombol 3D beveled, start menu klasik. |
| **Sun Microsystem** | Estetika Solaris/CDE: palet ungu-abu, header tegas, workspace switcher khas, vibe workstation era 90an. |
| **BeOS** | Signature kuning tab window, garis rapi, geometris, responsif — homage ke BeOS/Haiku. |

Detail implementasi:
- Setiap tema = paket `.theme` berisi token, aset (ikon, wallpaper), dan definisi kontrol.
- Bisa dipasang/diganti via Settings, `bz theme set <nama>`, atau live dari VS Code preview.
- Pihak ketiga bisa membuat tema baru dengan format yang sama (theme SDK).
- Aksesibilitas: kontras & ukuran font dapat disesuaikan lintas-tema.

---

## 16. Roadmap Pengembangan per Versi

> Urutan logis; durasi tergantung ukuran tim. Ini proyek skala tahunan. Codename bertema pertumbuhan tanaman (Kebun Raya Bogor).

### 🌱 v0.1 — "Benih" · Bootstrap
Bootloader UEFI (Rust) · kernel minimal masuk long mode · print ke serial/VGA · panic handler · pipeline build & boot QEMU. Boot logo dengan ASCII Art 'Buitenzorg OS' yang super keren
**Milestone:** "Hello Kernel" tampil.

### 🌱 v0.2 — "Akar" · Kernel Core
Memory manager (paging, heap) · interrupt + timer · scheduler preemptive single-core · syscall ABI awal.
**Milestone:** dua task berjalan bergantian.

### 🌱 v0.3 — "Batang" · Driver Baseline & Boot Media
Driver framework · keyboard & mouse · framebuffer · **storage: NVMe, SATA/AHCI, IDE, USB** · boot dari keempat media.
**Milestone:** boot dari USB & NVMe, baca file dari disk, gambar piksel.

### 🌱 v0.4 — "Tunas" · Managed Runtime Bring-up
Port runtime C# (NativeAOT) · interop/FFI shim · integrasi GC ↔ memory manager · BCL subset.
**Milestone:** program C# pertama "Hello from C#!".

### 🌿 v0.5 — "Dahan" · System Services, VFS & Fast I/O
VFS + FAT32 · service/init manager (parallel, dependency-aware) · async I/O (io_uring-style) · networking awal.
**Milestone:** service C# jalan sebagai proses; I/O asinkron benchmark-able.

### 🌿 v0.6 — "Daun" · Graphics & Window System
Compositor · window manager (floating) · event routing · rendering font/shape/image.
**Milestone:** dua window bisa dipindah & di-resize.

### 🌿 v0.7 — "Kanopi" · Desktop Environment, Terminal & Multi-Desktop
Shell (taskbar/launcher/tray) · **multi-desktop/workspaces** · **dark/light system theme** · **default terminal + shell** (command populer Unix/Windows, `bz` CLI) · notifikasi · settings dasar · tiling.
**Milestone:** pindah antar virtual desktop, ganti dark/light, jalankan `ls`/`dir` di terminal.

### 🌸 v0.8 — "Kembang" · App Framework + VS Code Tooling
SDK + template console & desktop · UI Toolkit XAML-based · app manifest & lifecycle · **port CoreCLR/JIT (fitur C# lengkap)** · **VS Code extension** (scaffolding, build & run) · **template**: Minesweeper, XOX, utilities, console app, calculator, Notes(web), widget · **debugging dari VS Code (DAP) + debug bridge**.
**Milestone:** buat & debug app dari VS Code, template jalan; desktop app pihak ketiga jalan.

### 🌸 v0.9 — "Serbuk" · Web App, Widget, Drawing & Task Manager ✅
WebView + web app runtime · widget host & board · **library `Buitenzorg.Drawing`
(mirip System.Drawing)** · **Task Manager (proses + sumber daya + kill)**.
**Milestone:** keempat varian app berjalan; app menggambar via Buitenzorg.Drawing;
Task Manager menampilkan proses & sumber daya dan bisa kill proses. *(Tercapai —
mini WebView subset HTML; engine web penuh & GC lanjutan.)*

### 🍎 v0.10 — "Buah" · Theme Engine, 8 Style & Package Manager ✅
Theme engine (design tokens + style: border/title/shadow/gradient) · **8 tema bawaan**: Neo Brutalism, Clean Design, Material, Bento, Classic Linux, Classic Windows, Sun Microsystem, BeOS · package manager (install/remove/list) · app registry.
**Milestone:** install app dari registry + ganti antar 8 tema secara live. *(Tercapai — `.theme` kustom, hot-reload, icon pack, sandbox/dependency menyusul.)*

### ⚡ v0.11 — "Cahaya" · GPU Compute & Desktop Polish ✅
Compute API (backend CPU, interface siap GPU) · **screensaver** (6 saver gaya Win 3.1/98) · **personalization** (wallpaper bawaan + gambar user BMP, tema, kursor, screensaver) · **micro-interactions** (hover, click ripple, reduce-motion) · **kontrol window** (min/max/close + sudut membulat).
**Milestone:** compute API siap untuk AI; screensaver idle-activated; atur tampilan (wallpaper/tema/saver/kursor); window punya min/max/close + rounded. *(Tercapai — driver GPU hardware & compositor GPU menyusul; interface siap.)*

### 🧠 v0.12 — "Nalar" · Subsistem AI-Native & Power Management ✅
LLM engine lokal (model bigram nyata) · computer vision (edge detect) · GenAI (text-to-image) · **Model Manager + galeri Hugging Face-style** · AI System API · **Power (Shutdown/Restart/Sleep, ACPI + fallback)**.
**Milestone:** LLM lokal jalan & "unduh" model dari galeri; app memakai AI API; sistem bisa Shutdown/Restart/Sleep. *(Tercapai — LLM skala produksi GGUF/GPU, inference scheduler, ACPI S3 menyusul.)*

### 📦 v0.13 — "Lapis" · Virtualization ✅
Deteksi hardware VT-x/AMD-V (CPUID/MSR) · VMM tipe-2 software (virtual CPU BZVM) · virtualization manager · port I/O virtio · virtual disk RAM + snapshot/restore · guest tools (host-tick).
**Milestone:** menjalankan OS lain sebagai VM di dalam Buitenzorg. *(Tercapai — guest "NanoOS" boot & jalan di virtual CPU; driver native VMX/SVM (VMXON/EPT/VMCB) & nested HW menyusul.)*

### 🗣️ v0.14 — "Babel" · Polyglot App Support ✅
Runtime polyglot di dalam OS (`script.rs`): satu interpreter tree-walking (AST/evaluator/binding seragam) · front-end JavaScript · TypeScript (transpile: strip tipe → JS) · Python (subset, indentasi) · shell `script <lang>`/`js`/`ts`/`py` · template app `sdk/templates/{js,ts,python}-app`.
**Milestone:** app JS, TypeScript, dan Python berjalan berdampingan dengan C#. *(Tercapai — fib sama di tiga bahasa jalan berdampingan app C#; engine V8/CPython penuh & runtime polyglot ring-3 native menyusul CoreCLR/GC.)*

### 🧩 v0.15 — "Matang" · Managed Runtime C# Lengkap (GC + .NET BCL)
Menaikkan app C# dari zerolib (tanpa GC) ke .NET managed penuh — prasyarat suite v0.16. Jalur: **NativeAOT + BCL penuh dulu** (`bflat --stdlib:dotnet`/ILC), **CoreCLR/JIT menyusul**.
- GC + managed heap ring-3 (GC bawaan NativeAOT; init GC statics; syscall alokasi/commit/free halaman mmap-style).
- PAL runtime .NET: memori, thread, sinkronisasi (mutex/condvar/futex), TLS, waktu presisi, environment, **exception unwinding** (EH funclets/DWARF).
- Thread ring-3 sungguhan + primitif sync + **thread pool** (sekarang app kooperatif satu-satu).
- BCL penuh: **Generics, Tuple, Collections, LINQ, Regex, StringBuilder, ToString/format, Encoding (UTF8/Base64/BitConverter), DateTime/Guid, Streams/System.IO** (di atas VFS).
- **System.Threading** (Thread/ThreadPool/**Timer**/lock/Interlocked) + **Tasks & async/await (TPL)** + SynchronizationContext.
- (Menyusul) CoreCLR/JIT + **reflection penuh** + Reflection.Emit (di luar batas NativeAOT/trimming).
- **Milestone:** app C# ber-GC memakai LINQ/Regex/StringBuilder/Base64/Collections/Tasks-async/Streams **jalan & terverifikasi di QEMU**.

### 🌾 v0.16 — "Panen" · Preloaded Suite & Optimization Pass
Dibangun di atas BCL v0.15. **Prasyarat framework** (rincian di PLAN.md/Progress.md §v0.16):
- **`Buitenzorg.Drawing` selengkap `System.Drawing`** (Graphics/Pen/Brush/Bitmap/Font/GraphicsPath/Matrix/Color, gradient, antialias, load/save PNG/BMP).
- **`Buitenzorg.UI`** — toolkit gaya WPF/Avalonia: model retained + layout + komponen lengkap (Button/TextBox/ListBox/TreeView/Menu/DataGrid/…) + data-binding/MVVM + styles/templates + rendering halus & animasi (60 FPS) + **akselerasi GPU** (fallback CPU), ringan & performa tinggi. **[increment 1–3 ✓ `bzui.cs`: UIElement tree (anak linked-list) + Measure/Arrange (StackPanel/Grid/Canvas) + hit-test (virtual) + **event mouse** (UIHost.Mouse: hover/press/click/drag) + set kontrol lengkap-dasar (TextBlock/Button/CheckBox/ProgressBar/Border/Slider/RadioButton+RadioGroup/ListBox/TextBox/Menu/**ComboBox**/**TabControl**/**TreeView**/**ScrollViewer**/**DataGrid**) + UIHost compositor → MILESTONE UI OK; menyusul: Dialog/popup, MVVM/data-binding, animasi 60fps, GPU]**
- **Subsistem audio OS**: driver kernel HDA/AC'97 (speaker + mic), ABI audio PCM, layanan `Buitenzorg.Audio` + **kontrol volume/mute/perangkat**, UI pengaturan audio. **[✓ dasar `audio.rs`+`bzaudio.cs`: driver AC'97 (PCI enum + cold-reset + mixer + playback DMA 48 kHz stereo), ABI `AUDIO_STAT/SET_VOLUME/TONE/PLAY` (23–26) + mirror C# + kontrak, `Buitenzorg.Audio` (Mixer/Tone) + kontrol volume master → MILESTONE AUDIO OK; **UI pengaturan audio** `audiopanel.cs` (panel Buitenzorg.UI slider/mute/tes-nada terhubung ke Mixer) → MILESTONE AUDIO PANEL OK (4-media); menyusul: mic capture, IRQ, Intel HDA, mixer per-app, pilih perangkat, widget volume]**

Lalu: Utilities, multimedia, AI apps, games, themes, productivity, store bawaan · **optimization pass**: fast boot, fast I/O, startup app, footprint, SIMD hot-path · benchmark regression di CI. **[Suite mulai ✓: Kalkulator (`calc.cs`→CALC.ELF, Buitenzorg.UI Grid tombol ter-tema + display, `12+3=`→15 → CALC OK) + 2048 (`game2048.cs`→G2048.ELF, papan 4×4 ubin berwarna + slide/merge → GAME OK) + Jam (`clock.cs`→CLOCK.ELF, jam analog rotasi jarum + digital → CLOCK OK) + Piano (`piano.cs`→PIANO.ELF, keyboard 1 oktaf → Buitenzorg.Audio Beep → PIANO OK) + App Store (`store.cs`→STORE.ELF, TERHUBUNG pkg.rs via syscall PKG_LIST/PKG_SET, katalog registry + install nyata → STORE OK) + File Manager (`filemgr.cs`→FILES.ELF, jelajah VFS via syscall FS_LIST → FILES OK) + Text Editor (`editor.cs`→EDITOR.ELF, TextArea multi-baris + caret + Menu → EDITOR OK) + Image Viewer (`imgview.cs`→IMGVIEW.ELF, muat PHOTO.BMP via syscall baru FS_READ=30 + dekode Bmp.Load + fit-to-box → IMGVIEW OK); suite 8 app semua kategori; fix ABI userland: rdi/rsi/rdx wajib clobbered di inline-asm syscall (kernel hanya restore rcx/r11/rsp) — heap growth kedua tadinya page fault; Editor+FileMgr INTERAKTIF via `run` (KEY_READ busy-poll + syscall IS_INTERACTIVE=31, boot-demo skip); fix Heisenbug ke-3: PRIVILEGE_STACK 20→64 KiB (overflow di compose_into via WIN_PRESENT → smash return addr) + PRESENT_BUF reuse + heap 32 MiB; JPEG baseline decoder (Jpeg.Load, IDCT integer) + Image Viewer format-aware → JPEG OK; desktop shell (Start button+menu+ikon+jam RTC di wm.rs, macOS+Ubuntu+WinXP, launch via desktop_loop) → DESKTOP SHELL OK; optimization pass mulai (boot 69.8s→58.5s); SUITE OK 4-media; screenshot desktop-calc/2048.png. Fix: Heisenbug `from_raw_parts` atas memori user di-harden dgn `copy_user_bytes` read_volatile di syscall string. App `/disk` hanya di media IDE.]**
**Milestone:** OS terasa ringan & cepat, siap pakai out-of-the-box.

### 🏛️ v1.0 — "Buitenzorg" · Stable Release (x86-64) — *sedang berjalan (2026-07-24)*
Stabilkan API & ABI (versioning) · security hardening (sandbox, permission broker) · dokumentasi lengkap + tutorial SDK · debugger & profiler · **instalasi ke hardware nyata** · **image resmi untuk VMware/QEMU/Hyper-V/VirtualBox**.
**Sudah:** pembekuan ABI v1, hardening validasi pointer syscall, debugger GDB + profiler zona, benchmark regresi CI, dokumentasi+tutorial rilis, image VMware/VirtualBox/Hyper-V, lisensi **MIT**. **Sisa:** validasi boot di hardware/hypervisor fisik nyata + tag rilis (perkakas siap; butuh mesin fisik). Sandbox/permission-broker penuh menyusul pasca-1.0.
**Milestone:** rilis stabil; developer bisa self-host workflow dasar.

### 🌍 v1.x — "Rimba" · Multi-Arch
Port **ARM64**, lalu **RISC-V** (memanfaatkan HAL yang sudah disiapkan) · optimasi per-arsitektur.
**Milestone:** boot & jalan di ARM64 dan RISC-V.

### 🔮 Pasca-1.x (jangka panjang)
- SMP multi-core matang & NUMA-aware
- Driver GPU modern lebih luas + akselerasi AI (NPU) lebih dalam
- WiFi/Bluetooth/USB device luas
- Container & sandboxing tingkat lanjut
- Marketplace app, tema, game, & model AI
- Ekspansi GenAI video/audio real-time

---

## 17. Checklist Tracking Development

> Tandai `[x]` bila selesai. Dikelompokkan per subsistem agar mudah diparalelkan antar-tim.

### 🧱 Fondasi & Tooling
- [x] Repo + struktur monorepo (kernel/rust, runtime/csharp, ai, sdk, apps, docs)
- [x] Toolchain build (cargo + .NET SDK + linker script)
- [x] Pipeline boot QEMU otomatis (`bzimage` + `scripts/`)
- [x] **CI (build + test + boot smoke test + benchmark performa) ✓** — semua di `.github/workflows/ci.yml`: `cargo test -p bz-abi`, build kernel, smoke test QEMU, `dotnet test`, dan **`scripts/bench.sh`** (benchmark regresi boot+I/O). *(Bug diperbaiki 2026-07-24: trigger `[main]` padahal branch `master` → CI tak pernah jalan saat push; kini `[master, main]` + `workflow_dispatch`.)*
- [x] Coding standard & contribution guide (CONTRIBUTING.md)

### 🔩 Bootloader & Boot Media (v0.1, v0.3)
- [x] UEFI init + masuk long mode (crate `bootloader` 0.11, teruji OVMF/QEMU)
- [x] Legacy boot support (image BIOS/MBR, teruji QEMU)
- [x] Load kernel + page table awal + handoff (BootInfo: memory map, framebuffer, phys-offset)
- [x] Boot dari **NVMe** — *teruji QEMU (SeaBIOS + bootloader); hardware nyata belum*
- [x] Boot dari **SATA/AHCI** — *teruji QEMU*
- [x] Boot dari **IDE/PATA** — *teruji QEMU*
- [x] Boot dari **USB** — *teruji QEMU (usb-storage)*
- [ ] Boot manager (multi-OS)
- [x] Boot logo ASCII Art 'Buitenzorg OS' yang keren

### ⚙️ Kernel Core (v0.1–0.2)
- [x] Serial/VGA output + panic handler (COM1 + framebuffer console)
- [x] Physical frame allocator
- [x] Virtual memory / paging + heap (OffsetPageTable + heap 1 MiB)
- [ ] IDT & interrupt + timer (APIC) — *IDT + exception + timer + keyboard jalan via PIC 8259; migrasi APIC belum*
- [x] Scheduler preemptive + context switch (round-robin single-core, preempt via timer IRQ; milestone "dua task bergantian" tercapai)
- [x] Syscall ABI v1 — *tabel v1 + dispatcher + kontrak `bz-abi`↔C# + entry ring-3 SYSCALL/SYSRET (dipakai C# di ring 3)*
- [ ] IPC (message passing) — *channel antar-task kernel-space (send/recv, teruji di boot); IPC antar-proses user-space belum*
- [ ] HAL (abstraksi arsitektur)
- [ ] SMP multi-core
- [~] Ekstensi virtualization (VT-x/AMD-V) — dideteksi (CPUID/MSR, `vmx.rs`); driver native belum, VMM software dipakai

### 🔌 Drivers (v0.3, v0.11)
- [ ] Driver framework (kernel-space & user-space terisolasi) — *registry block-device kernel-space + enumerasi PCI jalan; isolasi user-space belum*
- [ ] Keyboard / mouse / touchpad — *keyboard PS/2 (IRQ1, echo) & mouse PS/2 (IRQ12, streaming) jalan; touchpad belum*
- [x] Framebuffer graphics (text console + font bitmap + direct pixel drawing, BGR/RGB/gray)
- [ ] Storage: NVMe / SATA / IDE / USB — *IDE/PATA PIO LBA28 jalan (baca file FAT dari disk); NVMe/AHCI/USB driver belum*
- [ ] Network (Ethernet)
- [ ] WiFi
- [~] Audio (in/out) — *AC'97 out ✓ (`audio.rs`: PCI enum + cold-reset + mixer + playback DMA 48 kHz stereo, ABI `AUDIO_*` + `Buitenzorg.Audio`); mic in / IRQ / Intel HDA belum*
- [ ] **GPU driver** (framebuffer → 2D → 3D → compute)
- [~] virtio drivers — port I/O virtio (console char/num, disk-write, host-tick) di VMM software (`vmm.rs`); virtio-pci host/guest nyata belum
- [ ] SDK driver C# (user-space)

### 🌉 Managed Runtime (v0.4, v0.8)
- [x] Port runtime NativeAOT — *C# dikompilasi via bflat (ILC/NativeAOT, zerolib) → ELF statis, jalan di ring 3*
- [x] FFI shim Rust ↔ C# (C ABI) — *entry SYSCALL/SYSRET + shim `bzstart.rs` (SystemNative_Log/Malloc/Abort) memetakan zerolib ke syscall Buitenzorg; kontrak ABI v1 & test dua sisi tetap berlaku*
- [x] Syscall wrapper di C# (`BzSys`: backend native P/Invoke + simulasi host, teruji)
- [ ] Integrasi GC ↔ memory manager + pinning — *user pakai bump-allocator freestanding; GC penuh belum*
- [ ] Shared memory / zero-copy buffer
- [ ] BCL subset → penuh — *subset zerolib (Console, String, Span, dll); BCL penuh belum*
- [x] "Hello from C#" di bare metal — *milestone v0.4 tercapai: HELLO.ELF dibaca dari disk, dijalankan di ring 3, mencetak lewat syscall*
- [ ] Port CoreCLR + JIT (fitur C# lengkap)
- [ ] Reflection & dynamic assembly loading
- [ ] async/await + threading map ke scheduler
- [x] ELF64 loader + ring-3 user-space (GDT user, TSS rsp0, paging user-accessible)

### 🗂️ System Services (v0.5)
- [x] VFS + FAT32 (read/write) — *VFS mount table (`/disk` ro, `/ram` rw) + FAT read (12/16/32) & FAT12 write, teruji round-trip di ramdisk*
- [ ] Filesystem kustom (cepat, journaled)
- [x] Service/init manager (parallel, dependency-aware) — *start_all topologis paralel di atas scheduler; urutan dependency teruji*
- [ ] Async I/O (io_uring-style) + zero-copy — *ring SQ/CQ + worker task + benchmark ops/detik jalan; zero-copy & syscall submit belum*
- [ ] Networking stack (TCP/IP) — *Ethernet/ARP/IPv4/ICMP echo di atas loopback (round-trip teruji); TCP/UDP & driver NIC (e1000) belum*
- [ ] Security / permission broker + capability
- [ ] Package manager (install/update/remove, sandbox, dependency)
- [x] Service C# jalan sebagai proses (ring-3, diluncurkan init manager) — *milestone v0.5*

### 🖼️ Graphics & Window System (v0.6, v0.11)
- [x] Compositor (double-buffered, full-screen recompose + present)
- [ ] Window manager (floating + tiling) — *floating (title bar, z-order, move, resize) jalan; tiling belum*
- [x] Event routing (input → window) — *hit-test + mouse press/move/release → move/resize window teratas*
- [x] Rendering: font / shapes / images — *fill/outline rect, gradient, alpha blend, teks Noto; blit gambar belum*
- [~] Animasi & transparansi — *v0.11: click ripple + loop kontinu; animasi buka/tutup window belum*
- [ ] **Compositor akselerasi GPU** — *v0.11 compute API + backend CPU; driver GPU hardware belum*
- [x] Compute API (Vulkan/WebGPU-style) + fallback CPU — *v0.11: `compute.rs` (SAXPY/blend), backend CPU; GPU menyusul*
- [x] Screensaver (6 saver gaya Win 3.1/98) — *v0.11*
- [x] Kontrol window (min/max/close + rounded corners) — *v0.11*
- [x] Personalization (wallpaper bawaan/gambar user, tema, kursor, saver) — *v0.11*
- [x] Micro-interactions (hover, click ripple, reduce-motion) — *v0.11*

### 🖥️ Desktop Environment (v0.7)
- [x] Shell / taskbar / launcher / tray — *taskbar (workspace switcher + window buttons + theme label) jalan; launcher/tray belum*
- [x] **Multi-desktop / workspaces** + wallpaper per-desktop — *4 workspace, window per-workspace, wallpaper bergeser per-desktop, switch teruji*
- [x] **Dark/Light system theme** (auto/manual) — *design token dark & light, toggle via `theme` command/`toggle`, teruji*
- [ ] Notifikasi & action center
- [ ] Settings app
- [ ] Global search
- [x] **Default terminal emulator** (tab, split, truecolor, GPU-accel) — *terminal window + scrollback + line editing jalan; tab/split/GPU-accel belum*
- [x] Shell + command populer (Unix + alias Windows) — *ls/dir, cat/type, cd, pwd, echo, clear/cls, ver/uname, mounts, theme, ws, bz*
- [ ] Pipes/redirection/env/history/tab-completion/alias
- [ ] `bz` CLI (app/paket/tema/model/VM) — *`bz version/theme/ws` di shell kernel; CLI penuh (host `sdk/bz`) terpisah*
- [ ] Kompatibilitas script `.sh` (POSIX-leaning)

### 🧠 AI Subsystem (v0.12)
- [~] LLM engine lokal — *v0.12: model bigram char-level nyata (`ai.rs`), jalan di kernel/CPU; LLM produksi (GGUF/ONNX + GPU/NPU) belum*
- [~] Computer Vision — *v0.12: edge detect (Sobel); OCR/segmentation belum*
- [~] GenAI Image — *v0.12: text-to-image prosedural; upscaling/inpainting belum*
- [ ] GenAI Audio (TTS, STT, generatif)
- [ ] GenAI Video (generasi/editing pendek)
- [ ] Inference scheduler (alokasi GPU adil)
- [x] **Model Manager + galeri Hugging Face-style** — *v0.12: `model.rs` + `bz model list/pull/info`; download nyata (multi-GB) belum*
- [ ] Verifikasi checksum/lisensi + sandbox model
- [x] AI System API — *v0.12: `ai::llm_complete/vision_edges/genai_image` + shell `ask`; binding polyglot (JS/TS/Python) belum*
- [ ] Kontrol privasi & permission (kamera/mic/model)

### 🔌 Power Management (v0.12)
- [x] Parser ACPI minimal (RSDP → FADT → PM1a_CNT; scan DSDT `\_S5`)
- [x] Shutdown (ACPI + fallback QEMU/VBox) — *teruji QEMU power off*
- [x] Restart (ACPI reset + kbd-controller + triple-fault)
- [x] Sleep (light: blank + `hlt` sampai input) — *ACPI S3 belum*
- [x] CLI `shutdown/restart/sleep` + `bz power` · [ ] power menu GUI + konfirmasi · [ ] flush/save-state

### 🧰 App Framework & SDK (v0.8–0.9, v0.14)
- [x] App manifest & lifecycle — *manifest (`AppManifest`) + lifecycle app ring-3 (load ELF → jalankan → unmap) via `app::run_named`*
- [x] SDK + CLI (create/build/run) — *`bz new/manifest validate`, `run <app>` di shell OS; build via scripts/build-hello-csharp*
- [ ] UI Toolkit XAML-based (binding, MVVM, kontrol) — *window syscall + primitif gambar (`BzUi`) jalan; XAML/MVVM belum*
- [ ] Template Console / Desktop / Web / Widget — *console-csharp & desktop-csharp jalan; web/widget belum (v0.9)*
- [~] WebView engine + web app runtime — *mini WebView (subset HTML) jalan; engine HTML/CSS/JS penuh belum*
- [x] Widget host & board — *widget board (dock kanan) + widget monitor sistem*
- [~] Library grafik `Buitenzorg.Drawing` (mirip System.Drawing) — *v0.9: Graphics/Pen/Brush syscall-per-primitif; **v0.16: renderer software client-side (`bzgfx.cs`) — Bitmap/Color/Graphics (fill/outline, line tebal, circle/ellipse, FillPolygon, gradient, DrawImage/DrawImageScaled, alpha-blend, transform 2D Matrix Translate/Scale/Rotate, GraphicsPath+FillPath/DrawPath, SetClip, FillHatch 6 pola, DrawString+Font 8×8+MeasureString, BMP 24-bit load/save) + kernel BLIT op (draw_op 7); ~usable-complete. Menyusul: Region non-rect, PNG, texture brush, Pen dash, antialias.***
- [x] Task Manager / Monitor Sistem — *v0.9: proses list + CPU/memori + kill; ABI PROC_LIST/PROC_KILL/SYS_STAT*
- [~] **JS/TS engine** + transpile TS — *v0.14: interpreter JS + transpile TS (strip tipe) di `script.rs`; engine V8-kelas belum*
- [~] **Python runtime** (CPython/IronPython) — *v0.14: interpreter Python subset (indentasi) di `script.rs`; CPython/IronPython penuh belum*
- [x] Binding API seragam lintas-bahasa — *v0.14: `print`/`console.log` → host `emit` yang sama; `script <lang>`/`bz script`*
- [ ] **VS Code extension** (scaffolding, build & run, manifest editor, preview, IntelliSense/snippets) — *skeleton di `sdk/vscode-extension` (new project, build&run, validate manifest); preview/IntelliSense belum*
- [ ] **Debugging dari VS Code** (DAP: breakpoint, step, watch, call stack)
- [ ] **Debug bridge** (deploy/log/profile ke VM/perangkat, remote attach)
- [ ] Template: **Minesweeper**
- [x] Template: **XOX (Tic-Tac-Toe)** — *desktop app C# jalan, menggambar UI via window syscall (milestone v0.8)*
- [ ] Template: **Utilities**
- [x] Template: **Console System App**
- [ ] Template: **Calculator**
- [ ] Template: **Notes (Web App)**
- [ ] Template: **Widget**
- [ ] **Port CoreCLR/JIT (fitur C# lengkap)** — *masih jalur NativeAOT/bflat (zerolib, tanpa GC/reflection); CoreCLR/JIT + GC penuh adalah pekerjaan besar lanjutan*
- [x] Desktop app pihak ketiga jalan (window syscall ABI: WIN_CREATE/CMD/PRESENT + KEY_READ) — *milestone v0.8*

### 🎨 Theme & Package (v0.10)
- [x] Theme engine (design token warna + style: border/title/shadow/gradient, live switch)
- [ ] Tema kustom pihak ketiga (`.theme`) + theme SDK · [ ] hot-reload dari file
- [ ] Icon pack system
- [x] App registry + package manager (`bz install/remove/list`; `run` di-gate) — *sandbox/dependency/update belum*
- [x] Tema: **Neo Brutalism**
- [x] Tema: **Clean Design**
- [x] Tema: **Material Design**
- [x] Tema: **Bento Layout**
- [x] Tema: **Classic Linux**
- [x] Tema: **Classic Windows**
- [x] Tema: **Sun Microsystem**
- [x] Tema: **BeOS**

### 📦 Virtualization (v0.13)
- [~] Hypervisor tipe-2 (VT-x/AMD-V) — deteksi HW (CPUID/MSR) + VMM software (virtual CPU BZVM) jalan; driver native VMXON/EPT/VMCB backlog (nested HW tak diekspos QEMU/TCG)
- [x] Virtualization manager (buat/kelola/jalankan VM) — `vmm.rs` + shell `vm`/`bz vm`
- [x] Virtual disk + snapshot — virtual disk RAM + snapshot/restore state penuh (terverifikasi)
- [~] Guest tools — integrasi host-tick lewat port virtio; clipboard/resize/shared-folder backlog

### 🧩 v0.15 "Matang" — Managed Runtime C# Lengkap (GC + .NET BCL) — *sedang berjalan*
Prasyarat suite v0.16. NativeAOT+BCL penuh dulu (`bflat --stdlib:dotnet`/ILC) → CoreCLR/JIT menyusul. (Sekarang app = zerolib tanpa GC/heap/BCL.)
- [~] GC + managed heap ring-3 — **inc 1 ✓:** syscall halaman `MMAP/MPROTECT/MUNMAP`; **inc 4 ✓:** heap tumbuh via mmap → `new`/array/objek/generic jalan (`heap.cs`) + libc malloc/free/calloc/realloc; **inc 5 ✓:** model memori GC (mmap `PROT_NONE`=reserve lazy + mprotect=commit-on-demand, `gcmem.cs`); GC sungguhan + init GC statics (NativeAOT `--stdlib:dotnet`) menyusul
- [~] BCL (Generics/Collections/LINQ/StringBuilder/Base64/BitConverter) — **inc 6 ✓:** **Buitenzorg.Bcl** tulisan-sendiri di C# di atas heap (`bzbcl.cs`: `BzList<T>`, LINQ Where/Select/Sum via fn-pointer, StringBuilder, Base64, BitConverter; `bcl.cs` → MILESTONE BCL OK). Bukan CoreLib resmi (LINQ/Regex/Tasks penuh perlu link `--stdlib:dotnet`, ditunda: link statik-freestanding + glue bflat = multi-sesi)
- [~] PAL runtime .NET — **memori ✓** (mmap/mprotect/munmap) · **thread ✓** (`THREAD_CREATE/JOIN/EXIT`, ABI 16–18) · **sync ✓** (`FUTEX_WAIT/WAKE`→mutex, ABI 19–20) · **TLS-dasar ✓** (`THREAD_SELF` 21) · **clock ✓** (`CLOCK_MONO`/TSC 22); condvar/TLS-penuh/waktu/environment + exception unwinding (EH/DWARF) menyusul
- [~] Thread ring-3 sungguhan — **increment 2 ✓:** thread kooperatif (kernel task + SYSCALL stack per-thread, `task.rs`/`usermode.rs`), worker C# berbagi address space + join terverifikasi (`thread.cs`); primitif sync + thread pool preemptif menyusul
- [x] Generics · Collections (List/Dictionary/HashSet/Queue/Stack) — `bzbcl.cs`; Tuple menyusul
- [x] LINQ (fn-pointer) · **Regex ✓ (pra-v1.0, `BzRegex` di `bzbcl2.cs`)** — backreference/lazy/`{n,m}`/lookaround/capture menyusul
- [x] StringBuilder + format angka/tanggal (`BzStringBuilder`, `BzCulture`, `BzDateTime`) — string interpolation & `ToString()` sejati butuh CoreLib
- [x] **Encoding UTF8/ASCII ✓ (`BzEncoding`, pra-v1.0)** · Base64 (`BzBase64`) · BitConverter (`BzBitConverter`) · **DateTime ✓ (`BzDateTime` + syscall `CLOCK_RTC`)**; TimeSpan/Guid menyusul
- [x] System.Threading (thread + futex-mutex + **Timer ✓ `BzTimer` polled**, pra-v1.0); ThreadPool & Interlocked menyusul
- [~] Tasks — **`BzTask` Run/Wait/WhenAll ✓ (pra-v1.0)** di atas thread kooperatif (function-pointer body); `async`/`await` + SynchronizationContext butuh CoreCLR
- [x] **Streams/System.IO ✓ (pra-v1.0)** — `BzPath`/`BzFile` (baca + **tulis** lewat syscall `FS_WRITE` baru)/`BzDir`/`BzMemoryStream` di atas VFS
- [x] **System.Diagnostics / System.Management / Pkg / GC ✓ (pra-v1.0)** — `BzStopwatch`/`BzProcess`/`BzDebug`, `BzSystemInfo`, `BzPkg`, `BzGC` (statistik nyata; `Collect()` = `false`, heap masih bump-only)
- [~] **System.Net + Sockets ✓ UDP NYATA (pra-v1.0)** — `net.rs` dapat UDP (socket/bind/send/recv + checksum pseudo-header) + syscall `NET_*` 34–39; `BzIPAddress`/`BzSocket`/`BzNetInfo`. **TCP belum** → `System.Net.Http` (`BzHttp`) baru lapisan pesan, dan perangkat masih loopback (driver e1000 menyusul)
- [x] Uji kontrak + sample C# terverifikasi di QEMU — `bcl.cs` → `MILESTONE: BCL OK`; **`bcl2.cs` → `MILESTONE: BCL2 OK`** (IO/Text/Regex/Globalization/Diagnostics/Management/Net/Tasks/Timers/GC/Pkg), smoke 4-media LOLOS
- [ ] (Menyusul) CoreCLR/JIT + reflection penuh + Reflection.Emit + GC sungguhan (kolektor) + TCP

### 🌾 v0.16 "Panen" — Preloaded Suite & Optimasi (di atas BCL v0.15)
- [ ] Utilities (file manager, terminal, settings, monitor, editor, dll)
- [ ] Multimedia (music/video/image, camera, recorder)
- [ ] AI apps (asisten, model gallery, generator, OCR)
- [ ] Games ringan bawaan
- [ ] Koleksi themes dark & light + wallpaper
- [ ] Productivity (notes, calendar, clock, browser) + Store
- [ ] **Fast boot** (parallel init, lazy load, AOT boot)
- [ ] **Fast I/O** (async, zero-copy, DMA, cache)
- [ ] Startup app cepat (AOT/warm-start)
- [ ] Footprint memori kecil + shared libs
- [ ] SIMD/vektorisasi hot-path
- [x] **Benchmark regression di CI ✓ (2026-07-24)** — `scripts/bench.sh` boot headless lalu ambil dua angka yang dicetak kernel (boot-to-READY dalam tick, throughput async-I/O ops/s) dan gagal bila lewat budget longgar (`BOOT_BUDGET_S`=90, `AIO_MIN_OPS`=10000 — tangkap regresi nyata, bukan jitter runner); job baru di `ci.yml`

### 🚀 Stabilisasi & Rilis (v1.0) — *sedang berjalan*
- [x] **Stabilkan API & ABI (versioning) ✓ (2026-07-24)** — tabel ABI v1 **dibekukan**: `abi_v1_is_frozen` (Rust, 6 test) + `AbiV1IsFrozen` (C#, 12 test) memaku `ABI_VERSION`, `COUNT`=40, ukuran+alignment 10 struct lintas-batas, dan kode error; renumber/ubah-layout gagal di test sebelum masuk image. Kebijakan di `docs/abi.md` (§Pembekuan ABI v1): tambah = append+bump COUNT+update kedua mirror; ubah yang ada = versi mayor baru
- [~] **Security hardening — validasi pointer syscall ✓ (2026-07-24); sandbox/permission-broker menyusul** — validasi pointer user di **semua** syscall berpointer (`memory::validate_user_range`/`validate_user_cstr`, hanya jalur ring-3 `dispatch_from_user`): tutup celah nyata bocor memori kernel (`DEBUG_WRITE`), tulis-sembarang ke memori kernel (`SYS_STAT`/`PROC_LIST`/`FS_READ`/`NET_RECV` = eskalasi privilege), dan page-fault kernel dari pointer tak-terpeta. 14 probe bermusuhan → `MILESTONE: SECURITY OK`. Docs `docs/abi.md` §Model Keamanan Pointer. **Sandbox per-app + permission broker** butuh isolasi multi-proses (model sekarang single-proses run-to-completion) → pasca-1.0
- [x] **Debugger + profiler ✓ (2026-07-24)** — **Debugger:** `scripts/debug-kernel.ps1`/`.sh`/`.gdb` (attach GDB ke QEMU-paused pakai simbol kernel + helper break/fault/register; verified GDB stub jawab RSP `T05`). **Profiler:** `profile.rs` zona ter-instrumentasi TSC (inert saat off; jalur panas syscall/compositor terinstrumentasi; shell `prof`) → `MILESTONE: PROFILER OK`. Docs `docs/debugging.md`. (DAP breakpoint penuh VS Code menyusul)
- [x] **Dokumentasi lengkap + tutorial ✓ (2026-07-24)** — `CHANGELOG.md` (riwayat per codename), `docs/tutorial.md` (tutorial berurutan nol→app 8-langkah), `docs/README.md` (indeks docs), README di-refresh ke status v0.16+jalur v1.0; semua cross-link diverifikasi resolve
- [~] **Boot di hardware nyata** — **perkakas SIAP** (2026-07-24): `scripts/flash-usb.ps1`/`.sh` (tulis image ke USB, pengaman berlapis + verifikasi baca-ulang) + `docs/install-hardware.md` (pilih firmware, boot menu, tabel kompatibilitas jujur: PS/2 bukan USB-HID, IDE-PIO bukan NVMe, PIC/PIT bukan APIC, loopback saja). **Validasi di mesin fisik BELUM** (tak bisa dari lingkungan dev) — ditandai eksperimental
- [~] **Image resmi VMware / QEMU / Hyper-V / VirtualBox** — QEMU = jalur boot native (4 media, teruji smoke); `make-vm-images.ps1`/`.sh` emit **VMware `.vmdk` + VirtualBox `.vdi` + Hyper-V `.vhdx`** (VHDX 64 MiB whole-MiB via create+`convert -n`, krn `-O vhdx` telanjang 5,47 MiB ditolak Hyper-V) + `make-hyperv-vm.ps1` (VM Gen-1/BIOS, guarded). Konversi terverifikasi; **boot di VMware/VBox/Hyper-V nyata BELUM divalidasi tim** (sama spt hardware)
- [x] **Lisensi ✓ (2026-07-24): MIT** — berkas `LICENSE` (© 2026 Gravicode Studios)
- [ ] Rilis v1.0 — tag rilis + validasi boot HW/hypervisor nyata (satu-satunya item tersisa; butuh mesin fisik)

### 🌍 Multi-Arch & Pasca-1.x
- [ ] Port ARM64
- [ ] Port **RISC-V**
- [ ] SMP/NUMA matang
- [ ] Marketplace app/tema/game/model
- [ ] Container & sandboxing lanjut
- [ ] GenAI audio/video real-time

---

## 18. Testing & VM Images

Untuk uji coba tanpa hardware nyata, sediakan **image resmi** untuk hypervisor umum:

| Platform | Format image | Catatan |
|---|---|---|
| **QEMU** | `.img` / `.qcow2` | Utama untuk dev harian & CI; virtio drivers dioptimalkan |
| **VMware** | `.vmdk` (+ .ovf/.ova) | Workstation/Player/ESXi |
| **Hyper-V** | `.vhdx` | Gen 2 (UEFI); integration services |
| **VirtualBox** | `.vdi` (+ .ova) | Guest additions dasar |

Praktik:
- **Guest tools** per platform (resize, clipboard, shared folder) demi pengalaman mulus.
- **virtio** dipakai bila tersedia untuk I/O cepat.
- **CI** menjalankan boot smoke test + benchmark di QEMU tiap commit.
- Rilis menyertakan `.ova`/`.ova`-equivalent agar mudah diimpor lintas-hypervisor.
- Dokumentasi "cara jalankan di masing-masing hypervisor" disertakan.

---

## 19. Tech Stack & Tooling

| Area | Pilihan |
|---|---|
| Kernel & driver | Rust (`no_std`), crate: `x86_64`, `uefi-rs`, `spin`, `bitflags` |
| Runtime C# | .NET (NativeAOT → CoreCLR/RyuJIT) |
| Polyglot | JS engine (QuickJS/V8-like), CPython/IronPython |
| UI Toolkit | XAML-based kustom (referensi Avalonia/WPF) |
| Dev tooling | VS Code extension + Debug Adapter Protocol (DAP) + debug bridge |
| Terminal | Emulator GPU-accel + shell POSIX-leaning (`bz` CLI) |
| Web runtime | WebView engine + model Blazor |
| GPU/Compute | Abstraksi Vulkan/WebGPU-style |
| AI | Format GGUF/ONNX/safetensors; Hugging Face Hub API |
| Interop | C ABI, P/Invoke, shared memory |
| Virtualization | VT-x/AMD-V, virtio |
| Emulasi/dev | QEMU (utama), Bochs, GDB |
| VM target uji | VMware, Hyper-V, VirtualBox |
| Build | Cargo + .NET SDK + orchestrator |
| VCS/CI | Git monorepo + boot smoke test + benchmark |
| Arsitektur | x86-64 → ARM64 → RISC-V |

---

## 20. Risiko & Tantangan

| Risiko | Mitigasi |
|---|---|
| **Port .NET runtime ke OS baru berat** | Mulai NativeAOT, CoreCLR belakangan; belajar dari Cosmos. |
| **AI subsystem butuh GPU compute matang** | Selesaikan GPU (v0.11) sebelum AI (v0.12); sediakan fallback CPU. |
| **Interop boundary rawan bug/unsafe** | ABI ketat + `#[repr(C)]` + fuzzing + test kontrak syscall. |
| **Polyglot menambah kompleksitas** | Satu app model + binding seragam; tambah bahasa bertahap. |
| **Port multi-arch (RISC-V) mahal** | HAL disiapkan sejak awal; port setelah stabil di x86-64. |
| **Scope sangat besar (skala tahunan)** | Rilis inkremental per versi, milestone jelas, komunitas open-source. |
| **Model AI besar (VRAM/lisensi)** | Model Manager tampilkan kebutuhan & lisensi; quantized model (GGUF) untuk perangkat kecil. |
| **Regresi performa** | Benchmark otomatis di CI sejak dini. |

---

### Catatan Penutup
Kombinasi **Rust (aman di bawah) + C# (produktif di atas)** dengan **AI-native**, **akselerasi GPU**, **polyglot**, dan **virtualization** menjadikan Buitenzorg jauh melampaui konsep Cosmos. Kunci keberhasilan tetap di **Layer 4 (Managed Runtime)**: begitu C# lengkap jalan mulus di atas kernel Rust, sisa fitur (AI, tema, multi-desktop, 4 varian app, polyglot) menjadi pekerjaan aplikasi yang jauh lebih mudah. Urutan realistis: **fondasi & runtime dulu → GPU → AI → virtualization → polyglot → optimasi → rilis → multi-arch**.

Proyek sekelas ini realistis sebagai upaya jangka panjang/komunitas (bandingkan Redox OS). Fokus pada milestone kecil yang bisa di-boot & diuji di VM tiap iterasi.

*Codename versi bertema pertumbuhan tanaman sebagai penghormatan pada Kebun Raya Bogor — silakan sesuaikan.*

---

### Kredit
**Buitenzorg OS dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.**
