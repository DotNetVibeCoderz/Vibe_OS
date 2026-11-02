# Progress.md — Tracking Pengembangan Buitenzorg OS

> Checklist status pengembangan per-versi & per-subsistem. Verifikasi tiap
> boot lewat marker `MILESTONE: ... OK` (dicek smoke test 4-media).
> Roadmap ke depan: [PLAN.md](PLAN.md) · Desain teknis: [requirements.md](requirements.md).

**Legend:** `[x]` selesai · `[~]` sebagian (lihat catatan) · `[ ]` belum.
**Status keseluruhan (dirapikan 2026-11-01):** v0.1 – v0.14 milestone **tercapai**. **v0.15 "Matang" sebagian besar tercapai** — PAL (memori/thread/futex/TLS-dasar/clock), managed heap (`new`/array/generics), model memori GC (reserve/commit), dan **Buitenzorg.Bcl** (`bzbcl.cs` + `bzbcl2.cs`: collections, LINQ, StringBuilder, IO, Encoding UTF-8, Regex, Globalization/DateTime, Diagnostics, Net/UDP, Tasks, Timers) semua terverifikasi di ring-3; yang **belum**: GC yang mereklamasi memori + GC statics, exception unwinding, async/await, thread pool. **v0.16 "Panen" tercapai** (Drawing + UI + audio + suite 8 app + optimization pass). **v1.0 sedang berjalan** (sisa: validasi hardware nyata & Hyper-V, DAP). Rencana baru: **Developer Workstation** dan **Multimedia, Internet, Game & Hiburan** (lihat bawah).
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
| v0.13 Lapis | Virtualization (VMM tipe-2 + virtio + snapshot; guest OS jalan) | `VMX/VM/VIRTIO/SNAPSHOT/LAPIS OK` | [x] |
| v0.14 Babel | Polyglot runtime (JS/TS/Python, interpreter bersama) | `SCRIPT JS/TS/PY, POLYGLOT, BABEL OK` | [x] |
| v0.15 Matang | Managed runtime C#: PAL + heap + model memori GC + Buitenzorg.Bcl (GC reklamasi/async belum) | `MATANG/THREAD/SYNC/HEAP/GCMEM/BCL/BCL2 OK` | [~] |
| v0.16 Panen | Drawing + UI + audio + preloaded suite 8 app + optimization pass | `DRAW/UI/AUDIO/AUDIO PANEL/SUITE/CALC/GAME/CLOCK/PIANO/STORE/FILES/EDITOR/IMGVIEW/JPEG OK` | [x] |
| v1.0 Buitenzorg | Stable release x86-64 | `SECURITY/PROFILER/DESKTOP SHELL OK` | [~] Berjalan |
| v1.x Rimba | Multi-arch ARM64 + RISC-V di QEMU `virt` + CI matrix | `ARM64/RISCV BOOT OK`, `RISCV SMP SCHED OK` | [x] |
| v1.x Pucuk | Raspberry Pi 4 & 5 (perangkat keras nyata) | `RPI4 BOOT OK` + desktop di HDMI + 4 core (Pi 4 fisik + QEMU `raspi4b`) | [~] R1–R2 ✓ |

---

## ✅ v0.1 – v0.8 (Selesai)

### 🧱 Fondasi & Tooling
- [x] Repo + struktur monorepo (kernel, runtime, sdk, userland, ai, apps, docs)
- [x] Toolchain build (cargo + .NET SDK + bflat + rust-lld)
- [x] Pipeline boot QEMU otomatis (`bzimage` + scripts)
- [x] CI (build + test + boot smoke test + benchmark regresi `bench.sh`) — workflow di root repo Vibe_OS (`.github/workflows/ci.yml`, jalan dari subfolder `Buitenzorg/`)
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
- [x] HAL — *M0 selesai 2026-11-01: `arch/x86` (cpu, context, paging, aspace, gdt, interrupts, usermode, smp, vmx) + `platform/pc`; pemeriksa batas CI dengan allowlist kosong (lihat §Multi-Arch)*
- [~] SMP multi-core — **increment 1 ✓ (2026-11-01):** CPU ditemukan dari ACPI MADT, Local APIC BSP diaktifkan, AP dibangunkan INIT-SIPI-SIPI lewat trampolin real-mode→long-mode di 0x8000 (page table sementara dengan identity map; memori < 1 MiB tak pernah dialokasikan) — `MILESTONE: SMP OK` (QEMU `-smp 4`: 3 AP online; smoke AHCI + UEFI `-smp 2`). **Increment 2 ✓:** setiap AP punya GDT + TSS sendiri (stack double-fault + ring-0 privat), memuat IDT, menjalankan **timer LAPIC** 100 Hz yang dikalibrasi terhadap PIT, dan menjawab **IPI** (ping + **TLB shootdown** dengan ack); state per-CPU dicari lewat ID LAPIC (`arch::cpu::id()`; jalur cepat GS menunggu `swapgs`) — `MILESTONE: SMP PERCPU OK` (wajib di smoke AHCI + UEFI; kontrol negatif: timer AP mati → gagal), shell `cpu`. AP masih idle saat itu. **Increment 3 ✓:** proses ring-3 berjalan paralel di semua core (multiprosesor asimetris: kernel tetap di BSP, AP menjalankan kode ring-3 saja; task pindah ke AP saat kembali ke ring 3 dan pulang ke BSP untuk syscall/fault/kill; root page table, stub SYSCALL, dan rsp0 per-AP; TLB shootdown setelah unmap/mprotect; ruang alamat baru dihapus setelah semua thread-nya pulang; thread dalam satu proses tetap berurutan) — `MILESTONE: SMP SCHED OK` (`-smp 4`: 3 AP menjalankan ring-3 bersamaan; kontrol negatif: AP scheduling mati → gagal), smoke IDE kini 2 CPU + satu boot 1 CPU. Kernel tetap di BSP saat itu. **Increment 4 ✓:** kernel SMP simetris — semua CPU menjalankan scheduler yang sama dan kode kernel (syscall dilayani di CPU pemanggil, task kernel jalan di core mana pun), diserialkan oleh Big Kernel Lock (ticket lock, dilepas di ring 3 / saat idle / saat context switch); task idle per CPU; pemetaan user lewat root CPU yang aktif; konteks exit disimpan di task — `MILESTONE: SMP KERNEL OK` (`-smp 4`: task kernel jalan di keempat CPU; kontrol negatif gagal). **Increment 5 ✓:** lock berbutir halus — syscall memori (MMAP/MPROTECT/MUNMAP), scheduler (YIELD/THREAD_SELF/FUTEX), konsol (DEBUG_WRITE), window system (WIN_*, KEY_READ), registry proses (PROC_LIST/ARGS, SYS_STAT) dan CLOCK_RTC berjalan tanpa BKL, paralel di semua CPU; flag validasi pointer kini per-task; TLB shootdown tidak lagi di bawah lock dan frame dibebaskan setelah semua CPU ack — `MILESTONE: SMP FINELOCK OK` (`-smp 4`: 4 syscall di kernel sekaligus, antrean BKL ~4000 → ~8, throughput ×2,25; kontrol negatif gagal). **IOAPIC ✓:** IRQ perangkat lewat I/O APIC (PIT → CPU 0, keyboard + mouse → CPU lain; 8259 dimask; self-test memicu IRQ sungguhan lewat 8042 0xD2/0xD3) — `MILESTONE: IOAPIC OK`. Lock WM dipecah: kanvas tiap jendela punya lock sendiri dan komposisi memakai snapshot (≈1.000 gambar terjadi bersamaan dengan komposisi CPU lain). **Thread paralel ✓:** thread dalam satu proses berjalan di beberapa CPU sekaligus; runtime ring-3 jadi aman-SMP (heap ber-lock, mutex CAS, atomik `bz_atomic_*`) — `MILESTONE: SMP THREADS OK` (4 thread di 4 CPU; kontrol negatif: heap tanpa lock → alokasi tumpang-tindih terdeteksi). **FS tanpa BKL ✓** (FS_LIST/READ/WRITE di bawah lock mount VFS), syscall kini dilayani dengan interrupt menyala (memperbaiki livelock 4 CPU: CPU 0 berputar dengan IF mati membekukan tick PIT), present digabung, race argumen spawn diperbaiki. **Proses/jaringan/paket/audio tanpa BKL ✓** (PROC_SPAWN/PROC_WAIT, NET_*, PKG_*, AUDIO_*; PROC_WAIT memeriksa "anak masih hidup?" dan memblokir dalam satu langkah di bawah lock scheduler, sehingga wake-up tidak hilang — workload SMPPROC ×1,95–2,0, kontrol negatif yang sengaja dibuat racy macet dan gagal); bug lama diperbaiki: task baru dulu mulai dengan interrupt menyala di tengah penyelesaian context switch (hang/fault UEFI langka di demo scheduler). Sisa di bawah BKL: EXIT/PROC_KILL (pembongkaran proses) dan THREAD_CREATE/JOIN · [x] APIC (LAPIC di semua CPU, timer LAPIC di AP, IPI, I/O APIC untuk IRQ perangkat — 8259 dimask) · [ ] ekstensi virtualization

### 🔌 Drivers
- [x] PCI enumeration + block-device registry
- [x] Storage IDE/PATA (PIO LBA28)
- [x] Keyboard PS/2 + Mouse PS/2
- [x] Framebuffer graphics + direct pixel
- [ ] Driver NVMe/AHCI/USB native · [ ] Network e1000 · [ ] WiFi · [x] Audio (AC'97, v0.16) · [ ] virtio (net/blk/gpu)

### 🌉 Managed Runtime (C#)
- [x] Port NativeAOT (via bflat + zerolib, freestanding)
- [x] FFI shim Rust ↔ C# (SYSCALL/SYSRET + `bzstart.rs`)
- [x] Syscall wrapper C# (`BzSys`) + kontrak ABI dua sisi teruji
- [x] ELF64 loader + ring-3 user-space + lifecycle (load→run→unmap)
- [x] "Hello from C#" di bare metal
- [~] **Integrasi GC ↔ memory manager + pinning** — *heap mmap + reserve/commit jalan (v0.15); kolektor & pinning belum*
- [ ] **CoreCLR + JIT (fitur C# lengkap)** · [ ] reflection/dynamic loading
- [ ] async/await · [x] threading map ke scheduler (thread kooperatif + futex, v0.15) · [~] BCL (Buitenzorg.Bcl tulisan-sendiri; CoreLib resmi = eksperimen pasca-v1.0)

### 🗂️ System Services
- [x] VFS (mount table `/disk` rw sejak 2026-11-01 + `/ram` rw)
- [x] FAT12/16/32 read + **write** (FAT12/16/32 sejak 2026-11-01; teruji lintas reboot)
- [x] Service/init manager (parallel, dependency-aware)
- [~] Async I/O io_uring-style (SQ/CQ + benchmark) — *zero-copy & syscall submit belum*
- [~] Networking (Ethernet/ARP/IPv4/ICMP loopback) — *TCP/UDP & driver NIC belum*
- [ ] Filesystem kustom journaled · [ ] permission broker · [x] package manager (`pkg.rs`, v0.10)

### 🖼️ Graphics & Window System
- [x] Compositor (double-buffered, present)
- [~] Window manager floating (title bar, z-order, move, resize) — *tiling belum*
- [x] Event routing (mouse → window; keyboard → terminal)
- [x] Rendering: rect/outline/gradient/alpha/teks + blit gambar (`BLIT` op) + path (client-side di Buitenzorg.Drawing)
- [~] Animasi (ripple/hover, v0.11; animasi window belum) · [ ] compositor GPU

### 🖥️ Desktop Environment
- [x] Shell / taskbar — workspace switcher + window buttons + **Start menu + ikon desktop + tray (tema + jam RTC)** (v0.16)
- [x] Multi-desktop / workspaces (4) + wallpaper per-desktop
- [x] Dark/Light system theme (toggle)
- [x] Terminal + shell (ls/dir, cat/type, cd, pwd, echo, mounts, clear/cls, ver, about, theme, ws, run, bz, settings/bg/saver/cursor/anim, ask, vm, script/js/ts/py, prof, shutdown/restart/sleep)
- [ ] Notifikasi/action center · [ ] Settings app · [ ] Global search
- [ ] Pipes/redirection/history/tab-completion · [ ] kompatibilitas `.sh` — *dijadwalkan di §Developer Workstation*

### 🧰 App Framework & SDK
- [x] App manifest & lifecycle
- [x] SDK + CLI (`bz new/manifest`, shell `run`)
- [x] Window syscall ABI (WIN_CREATE/CMD/PRESENT + KEY_READ) + `DrawCmd`
- [x] Template Console (console-csharp) + Desktop (desktop-csharp + `BzUi`)
- [x] Template XOX (Tic-Tac-Toe) — desktop app menggambar via syscall
- [~] VS Code extension — new project (pemilih template)/build-run/deploy/GDB kernel/validasi manifest; IntelliSense/DAP belum
- [~] UI toolkit (`Buitenzorg.UI`, v0.16; binding/MVVM/XAML belum) · [~] template utilitas (Kalkulator ada sbg app suite + template MagicAppGen; Minesweeper belum)
- [~] WebView + web app (mini WebView, v0.9) · [x] widget host (v0.9)
- [~] Debug bridge + DAP debugging — *GDB kernel via QEMU ✓ (v1.0); debug bridge & DAP dijadwalkan di §Developer Workstation*

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
- [x] Gambar: `Bitmap` + `DrawImage` (blit) · [x] `GraphicsPath` + transform · [~] `Font` (8×8 embedded; font-pack/TTF belum) — *di `bzgfx.cs`, v0.16*
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
> task kernel + app aktif dan bisa kill task kernel. *(2026-11-01: multi-proses
> preemptif sudah ada — app yang di-spawn sebagai proses kini bisa di-kill lewat
> `PROC_KILL`; app yang diluncurkan sinkron oleh demo boot tetap tidak.)*

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
- [ ] Sandbox per-app · [ ] dependency resolution · [x] app store GUI (`store.cs`, v0.16) · [ ] update
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
- [x] **Perombakan UI/UX desktop ✓ (2026-11-01, skill frontend-design)** — tema default baru **kebun** (diambil dari wallpaper kolam teratai: panel kaca lumut `Theme.glass`, teks kabut, aksen tunggal pink teratai, jam kuning serbuk sari); **Start menu per kategori** (Kantor, Multimedia, Permainan, Utilitas, Pengaturan) dengan rel kategori berwarna-daun + jumlah app, kartu ikon, header tanggal, footer Tidur/Mulai ulang/Matikan; aksi sistem Wallpaper & Tema; taskbar kaca (daun Mulai, label window tenang + garis teratai untuk yang fokus, tray tanggal + jam); tipografi Noto Sans Mono Bold + 24 px untuk hierarki; label ikon desktop di pil kaca; window fokus bergaris teratai; **sesi dimulai rapi** (18 window beku demo boot ditutup, terminal bersih + kartu Tips). 4 ikon baru (Tic-Tac-Toe, Audio, Wallpaper, Tema). `DESKTOP SHELL OK` kini menguji klik kategori → kartu (editor, lalu Permainan → 2048)
- [x] **Ikon app unik ✓ (2026-11-01)** — 10 ikon AI (Rodin / Nano Banana 2) satu gaya: files, editor, calc, store, imgview, clock, piano, 2048, taskmgr, paint (`assets/icons/`). `build.rs` → 44 px + 18 px + siluet rounded-square beranti-alias (alpha) → `/disk/ICONS.BIN`; `icons.rs` + `wm.rs` blend di desktop & Start menu; fallback tile berwarna + inisial per app di media tanpa `/disk` (diverifikasi screenshot AHCI). `MILESTONE: ICONS OK`; README hero kini screenshot Start menu baru
- [x] **8 wallpaper resmi Buitenzorg OS ✓ (2026-11-01)** — karya seni AI (Rodin / Nano Banana 2): nature (default, Kebun Raya Bogor), futuristic, city, art (batik), surreal, imaginary, space, minimal. JPEG di `assets/wallpapers/` → `bzimage/build.rs` decode + box-filter 800×450 → BMP di `/disk`; kernel resample bilinear sekali ke ukuran layar + cache (frame = 1 salinan, bukan stretch per piksel). Shell `bg list` / `bg <nama>`; `MILESTONE: WALLPAPERS OK`; galeri di README + `docs/window-system.md`
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
- [ ] Double-click title bar → maximize · [~] anti-alias sudut (AA di kontrol Buitenzorg.UI; sudut window WM belum)
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
- [ ] ACPI S3 (suspend-to-RAM) · [ ] flush VFS + app save-state · [~] item power di Start menu GUI (v0.16; dialog konfirmasi belum)
- [x] Milestone: **Shutdown, Restart, Sleep dari shell/`bz power`** (`POWER OK`; shutdown teruji QEMU)

---

## ✅ v0.13 "Lapis" (Selesai)

### 📦 Virtualization
- [~] Hypervisor tipe-2 — deteksi HW VT-x/AMD-V (`vmx.rs`, CPUID/MSR); driver native VMXON/EPT/VMCB backlog (nested HW tak diekspos QEMU/TCG)
- [x] VMM software (virtual CPU **BZVM**) — fetch/decode/execute, 8 register, memori linier, stack, CALL/RET, bounds-checked (`vmm.rs`)
- [x] Guest OS mini **NanoOS** benar-benar jalan di VM (boot → komputasi → cetak → halt)
- [x] virtio (port I/O paravirtual): console char/num, disk-write, host-tick (guest tools)
- [x] Virtual disk RAM + **snapshot/restore** state penuh (terverifikasi: mutate → restore → cocok)
- [x] Virtualization manager: `vm list/create/start/snapshot/restore/remove` + `bz vm`, `bz virt`
- [x] Marker: `VMX/VM/VIRTIO/SNAPSHOT/LAPIS OK` (4 media) · screenshot `docs/img/desktop-lapis.png`

---

## ✅ v0.14 "Babel" (Selesai)

### 🗣️ Polyglot App Support
- [x] Runtime polyglot di kernel (`script.rs`): interpreter tree-walking, AST/Value/evaluator bersama
- [x] **Binding API seragam** — `print` (Python) & `console.log` (JS/TS) → host `emit` yang sama
- [x] **JavaScript**: lexer + parser (brace/`;`), fungsi+rekursi, if/else, while, for C-style, operator
- [x] **TypeScript**: transpile nyata — strip `interface`/`type`/anotasi `: Tipe` → JS lalu jalan
- [x] **Python**: lexer indentasi (INDENT/DEDENT) + parser, `def`, if/elif/else, while, `for x in range()`
- [x] Guard: batas langkah (anti loop tak berujung) + batas output; error parse/runtime dilaporkan
- [x] Shell `script <lang> [file]` + `js`/`ts`/`py` + `bz script`; template `sdk/templates/{js,ts,python}-app`
- [x] Milestone: fib sama di JS/TS/Python jalan berdampingan app C# — `SCRIPT JS/TS/PY, POLYGLOT, BABEL OK` (4 media); screenshot `docs/img/desktop-babel.png`
- [~] Engine V8/CPython penuh & app polyglot ring-3 native — backlog (butuh CoreCLR/GC)

---

## 🔄 v0.15 – v1.0 (Berjalan)

### 🧩 v0.15 "Matang" — Managed Runtime C# Lengkap (GC + .NET BCL)
Prasyarat preloaded suite (v0.16). Jalur: **NativeAOT + BCL penuh dulu** (`bflat
--stdlib:dotnet`/ILC) → **CoreCLR/JIT menyusul**. (Sekarang app = zerolib tanpa
GC/heap/BCL.)
- [~] **GC + managed heap** ring-3 — **inc 1 ✓:** syscall halaman `MMAP/MPROTECT/MUNMAP`; **inc 4 ✓:** heap tumbuh via mmap → `new`/array/objek/generic JALAN (`heap.cs`) + libc `malloc/free/calloc/realloc`; **inc 5 ✓:** model memori GC — `mmap PROT_NONE`=reserve lazy + `mprotect`=commit-on-demand (`gcmem.cs`: reserve 256 MiB tanpa OOM). GC sungguhan (reclaim/finalizer) + init GC statics menyusul (NativeAOT `--stdlib:dotnet`)
- [~] **PAL runtime .NET**: **memori ✓** (mmap/mprotect/munmap) · **thread ✓** (`THREAD_CREATE/JOIN/EXIT`) · **sync ✓** (`FUTEX_WAIT/WAKE` → mutex; state scheduler Blocked) · **TLS-dasar ✓** (`THREAD_SELF`) · **clock ✓** (`CLOCK_MONO`/TSC); condvar, TLS penuh (pthread key), waktu-nyata/environment menyusul
- [~] **Thread ring-3 sungguhan** — **increment 2 ✓:** thread kooperatif (kernel task + SYSCALL stack per-thread, konteks di-swap saat switch; `task.rs`/`usermode.rs`); worker C# berbagi address space + join terverifikasi (`thread.cs`). **thread pool** (preemptif) menyusul
- [ ] **Exception handling/unwinding** (EH funclets / DWARF) untuk try/catch lintas-BCL
- [~] Primitif sync ring-3 — **increment 3 ✓:** futex (`FUTEX_WAIT/WAKE`, state Blocked) + mutex shim + `THREAD_SELF` + `CLOCK_MONO`, uji mutual-exclusion terverifikasi (`sync.cs`); condvar + **thread pool** preemptif menyusul
- [~] **Generics · Collections** — inc 6 + v0.16: **Buitenzorg.Bcl** `BzList<T>`, `BzStack<T>`, `BzQueue<T>` (circular), `BzIntMap<V>`/`BzStrMap<V>` (Dictionary), `BzIntSet` (HashSet), **`BzRefList<T>`** (list linked-list untuk tipe REFERENSI — hindari fault `stelem.ref` object-array); Tuple + Dictionary<K,V> ref generik menyusul
- [~] **LINQ** — inc 6 + v0.16: `BzLinq.Where/Select/Sum/Count/Any/All/Max/Min/Contains/IndexOf/Reverse` + **First/Last/Take/Skip/Average/Aggregate** (function-pointer, bukan delegate krn cache GC static); GroupBy/OrderBy/Distinct menyusul
- [x] **Regex** — `BzRegex` (bzbcl2.cs): kelas/anchor/kuantifier/alternasi/grup + backtracking lintas grup; *belum: capture, `{n,m}`, lazy, backreference, lookaround*
- [~] **StringBuilder + string/char/math/random** — inc 6 + v0.16: `BzStringBuilder` (Append string/int/long/char/hex + **Clear/AppendLine/AppendChars/CopyTo**); **`BzMath`** (Abs/Min/Max/Clamp/Sign/Pow/ISqrt/Gcd/Lcm); **`BzRandom`** (xorshift64* PRNG); **`BzStr`** (Equals/Compare/StartsWith/EndsWith/IndexOf/Count/Upper/Lower/IsDigit/IsAlpha — pakai `(object)` cast krn `string ==` tak ada); interpolation/ToString penuh menyusul
- [~] **Encoding/konversi** — inc 6 + v0.16: **Base64** (`BzBase64`) + **BitConverter** (`BzBitConverter`) + **`BzHex`** (bytes→hex) + **`BzConvert`** (ParseInt/ParseLong/ParseHex + LongToChars) jalan; **UTF-8/ASCII ✓** (`BzEncoding`, bzbcl2.cs)
- [~] **DateTime** ✓ (`BzDateTime` di atas CMOS RTC) · [ ] TimeSpan · [ ] Guid · [ ] Tuple
- [~] **System.Threading**: Thread ✓ (`THREAD_CREATE`) · **Timer ✓** (`BzTimer`, polled) · mutex ✓ (`BzMutex`/futex) · [ ] ThreadPool · [ ] Monitor/condvar · [ ] Interlocked · [ ] reset events
- [~] **Tasks**: `BzTask` Run/Wait/WhenAll ✓ · [ ] Task<T> · [ ] `async`/`await` · [ ] SynchronizationContext
- [x] **Streams / System.IO**: `BzMemoryStream` + `BzFile` read/write + `BzDir`/`BzPath` di atas VFS (`FS_READ/FS_WRITE/FS_LIST`); *FileStream ber-seek di atas file belum*
- [~] Sample C# terverifikasi di QEMU — `bcl.cs` + `bcl2.cs` (LINQ/Regex/StringBuilder/Base64/Collections/Tasks/Streams) → `BCL OK`/`BCL2 OK`; *tanpa GC reklamasi & async/await*
- [ ] **(Menyusul) CoreCLR/JIT + reflection penuh + Reflection.Emit** (skenario dinamis di luar batas NativeAOT/trimming)
- [~] **Jalur managed: RustCLR (RustNetRuntime)** — dua jalur C#: **native** (bflat/NativeAOT) + **managed** (RustCLR, interpreter IL + GC mark-sweep + RustBCL). **Increment 1 ✓ (2026-11-02):** host ring-3 `CLRHOST.ELF` (`userland/clrhost`, `no_std`, crate RustCLR sebagai git dependency ter-pin) untuk x86-64/ARM64/RISC-V; DLL hasil `dotnet build` jalan tanpa diubah; `HELLOCLR.DLL` 9/9 (static ref, `object[]`, string, LINQ+lambda, exception, GC mereklamasi ~40 ribu objek) + `MODERN.DLL` `checks=35 failures=0` → `CLR OK` di ketiga arsitektur (C# pertama di RISC-V). **Increment 2 ✓ (2026-11-02):** thread sungguhan — di RustCLR (repo Vibe_Runtime, `main` 6e31a5a): crate baru `rustclr-thread` + fitur `threads` (heap bersama, safepoint, monitor, task pool tanpa `std`, lewat hook embedder); di Buitenzorg: syscall `FUTEX_WAIT_TIMEOUT` (43) + `CLOCK_NANOS` (44), wake berbatas waktu di scheduler, TSC x86 terkalibrasi, dan **bug kernel: kunci futex kini per address space** (wake bisa nyasar ke proses lain → `Monitor.Pulse` macet di x86). Suite Conformance RustCLR **488/488 di ARM64 & RISC-V**; x86 (TCG) 487/488 — satu cek waktu-dinding. **Increment 3 ✓:** interop GUI — pustaka `Buitenzorg.App` (`BZAPP.DLL`: `Window`/`Keyboard`/`Screen`/`Session`/`Programs`, `[DllImport("buitenzorg")]` → native host `pinvoke:buitenzorg!…`, marshalling di host), host memuat referensi assembly dari `/disk`, `run hellogui` / `run x.dll` / menu Mulai / PROC_SPAWN menjalankan app managed, app `HELLOGUI.DLL` (jam analog, thread latar, Esc menutup) → `CLR GUI OK` di ketiga arsitektur; 2 bug kernel diperbaiki (timed wait di CPU tunggal desktop, screensaver saat mengetik di app).

- [x] **Buitenzorg.UI + Managed Apps (2026-11-02):** toolkit gaya Avalonia untuk jalur managed (`userland/clr-apps/Buitenzorg.UI` → `BZUI.DLL`): `StyledProperty` berlapis (lokal/animasi/gaya + pewarisan), Measure/Arrange dengan cache + invalidasi ke atas, panel `Grid`/`DockPanel`/`WrapPanel`/`UniformGrid`/`Canvas`/`ScrollViewer`/`SplitView`, gaya + selector + pseudo-class (`Button` = tipe persis, `:is(Button)` = turunan), transisi, binding/`ICommand`, popup + `Flyout`/`Menu`/`ToolTip`/`Dialog`/notifikasi, tema Kebun gelap + terang, dan kontrol yang belum ada: `ToggleSwitch`, `Expander`, `NumericUpDown`, `SplitButton`, `DropDownButton`, `RepeatButton`, `HyperlinkButton`, `AutoCompleteBox`, `GridSplitter`, `Calendar`, `CalendarDatePicker`. Kernel: `WIN_EVENT` (45), `TEXT_METRICS` (46), `draw_op` 8–13, `win_flags::CLIENT_SIZE`, tombol navigasi U+F700.., Ctrl+huruf, glyph Latin-1, klik tak hilang (`mouse::presses`). Sembilan app dalam folder **Managed Apps** (ikon desktop): Kalkulator, 2048, Catatan, Berkas, Pemantau, XOX, Jam, Galeri Kontrol, Jam Analog → `MANAGED UI OK` + `MANAGED APPS OK`. Panduan: [docs/managed-apps.md](docs/managed-apps.md).

### 🌾 v0.16 "Panen" — Preloaded Suite & Optimasi
**Prasyarat: framework grafis/UI + audio (lihat PLAN.md §v0.16 untuk rincian).**

🎨 `Buitenzorg.Drawing` selengkap `System.Drawing` — **renderer software (client-side) + BLIT** (`bzgfx.cs`, `draw.cs`)
- [~] **Graphics ✓:** Clear, Fill/DrawRectangle(thick), DrawLine(+thick, Bresenham), Draw/FillCircle, Draw/FillEllipse, FillPolygon(scanline)/DrawPolygon, FillGradientV/**FillGradientH/FillGradient**, DrawImage/**DrawImageScaled**(nearest), **alpha blending**, **transform 2D (Matrix Translate/Scale/Rotate, sin/cos Bhaskara)**, **FillPath/DrawPath**, **SetClip/ResetClip**, **FillHatch** (6 pola), **DrawString + Font (8×8 embedded) + MeasureString**. **Enhance visual ✓:** **FillRoundedRectangle/DrawRoundedRectangle** (sudut AA), **FillRoundedGradientV** (tombol), **DrawShadow** (drop shadow lembut berlapis), **FillCircleAA** (lingkaran anti-alias), BlendCov (coverage AA). Menyusul: Region non-rect, PointF/RectangleF, texture brush, Pen dash/cap, AA garis/teks penuh
- [~] **Bitmap ✓** (uint[] ARGB, Get/SetPixel, Clear) · **Color ✓** (ARGB, FromArgb/FromRgb, A/R/G/B, named) · **Point/Size/Rectangle ✓** · **Matrix ✓** · **GraphicsPath ✓** · **Font ✓** (8×8, ASCII subset) · **BMP 24-bit load/save ✓**. Menyusul: LockBits, PNG, HSL, Image crop/rotate, font-pack penuh
- [~] Pen width ✓ · [ ] dash/cap/join · Brush solid/gradient/hatch ✓ · [ ] texture brush · GraphicsPath ✓ · Matrix ✓ · [ ] Region
- [x] **Kernel BLIT op** (ABI draw_op 7): app render ke Bitmap managed → 1 syscall blit ke window (model kompositor WPF/Avalonia). Verifikasi `MILESTONE: DRAW OK`/`DRAWING2 OK` (4-media) + screenshot `docs/img/desktop-drawing.png`

🪟 `Buitenzorg.UI` — toolkit gaya WPF/Avalonia (ringan, performa tinggi, GPU) — `bzui.cs`
- [~] **Model retained + visual tree ✓** (`UIElement` base, anak via linked-list `ChildNode` — hindari fault store elemen object[]) · **layout Measure/Arrange ✓** (StackPanel V/H + Padding/Spacing, explicit Width/Height menang atas stretch; **Grid** fixed+star kolom/baris via int[]; **Canvas** absolut CanvasLeft/Top) · **hit-testing ✓** (virtual, tab/dropdown extend region) · **event mouse ✓** (`UIHost.Mouse`: hover Enter/Leave, press/capture/click/drag); Dock/Wrap + DPI menyusul
- [x] **Komponen ✓ (lengkap dasar):** TextBlock, Button (normal/hover/pressed + Clicks), CheckBox (Checked, klik toggle), ProgressBar, Border, **Slider** (drag 0..100), **RadioButton** + **RadioGroup** (pilih satu hapus lainnya), **ListBox**, **TextBox** (fokus + caret), **Menu**, **ComboBox** (dropdown select), **TabControl** (tab strip + panel konten), **TreeView** (`TreeNode` hirarki, expand/collapse), **ScrollViewer** (clip + scroll + thumb), **DataGrid** (kolom + baris linked-list, header + pilih baris). Menyusul: Dialog/ToolTip/DatePicker/menu popup berlapis
- [x] **Popup/overlay layer ✓:** ComboBox dropdown dirender di ATAS seluruh tree (`RenderPopup`) & di-hit-test lebih dulu (`PopupHitTest` → `UIHost.WalkPopupHit`) — klik dropdown menang atas sibling di belakang (memperbaiki "last-hit-wins"). Verified: item dropdown di atas TabControl memilih combo, tab tak berubah
- [x] **Tema visual ✓ (enhance tampilan):** kontrol pakai primitif Drawing baru — Button (rounded + gradient vertikal + drop shadow + hover/pressed), Border (CornerRadius + Shadow opsional), ProgressBar/Slider (pill rounded + gradient, thumb AA), CheckBox (kotak rounded), RadioButton (lingkaran AA), ComboBox (header rounded gradient)
- [ ] Data-binding + MVVM (INotifyPropertyChanged/Command) · styles/templates + theme engine · markup XAML-like (opsional)
- [~] **Rendering ✓ (dasar):** compositor software — seluruh tree dirender ke Bitmap via Buitenzorg.Drawing (rounded/gradient/shadow/AA), blit 1-syscall (`UIHost` + popup pass); dirty-region/double-buffer/animasi 60fps menyusul
- [ ] Akselerasi GPU via compute/GPU API (`compute.rs` `Backend::Gpu`), fallback CPU — lightweight & high-performance
- [x] Verifikasi: `ui.cs` — window 1 (StackPanel + Menu/Slider/ListBox/Radio/TextBox/Button/CheckBox/ProgressBar, layout, event mouse, cek Grid + pixel) + window 2 "Lanjutan" (ComboBox open+pick, TabControl switch, TreeView expand+select, ScrollViewer scroll, DataGrid pilih baris, RadioGroup) → `MILESTONE: UI OK` (4-media); screenshot `docs/img/desktop-ui.png`

🔊 Subsistem audio OS
- [~] **Driver audio kernel ✓ (Rust, `audio.rs`):** AC'97 (QEMU) — enumerasi PCI (kelas 0x04/0x01), cold-reset codec, mixer (master+PCM-out volume), **playback speaker** PCM 16-bit stereo 48 kHz lewat **DMA bus-master** (buffer descriptor list). Menyusul: capture **mic** (kotak PCM-in), IRQ (kini polling), Intel HDA
- [x] **ABI audio (syscall) ✓:** `AUDIO_STAT` (23, → `AudioInfo`), `AUDIO_SET_VOLUME` (24), `AUDIO_TONE` (25), `AUDIO_PLAY` (26, PCM i16 stereo). Mirror C# + kontrak (`cargo test -p bz-abi`, `AbiContractTests` — 9 lulus) + `docs/abi.md`
- [~] **Layanan `Buitenzorg.Audio` (C#) ✓ (dasar, `bzaudio.cs`):** `Mixer` (GetInfo/SetVolume/GetVolume/Mute/Beep/Play), `Tone` (generator square). **Kontrol volume master** + mute + tone + streaming PCM. Menyusul: record, mixer per-app, pilih perangkat, meter level
- [~] **UI pengaturan audio ✓ (dasar, `audiopanel.cs`):** panel `Buitenzorg.UI` (slider volume + checkbox mute + tombol tes nada + info perangkat) yang **terhubung langsung ke `Mixer`** — slider→SetVolume, checkbox→Mute, tombol→Beep. Menyusul: pilih perangkat, meter level, widget volume di taskbar
- [x] Verifikasi: kernel bunyikan tone 440 Hz (DMA CIV maju) + `audio.cs` (`Mixer` round-trip volume 45→45, tone, PCM stream) → `MILESTONE: AUDIO OK`; `audiopanel.cs` (slider→device 35, mute→device 0, tes nada) → `MILESTONE: AUDIO PANEL OK` (semua 4-media)

📦 Suite & optimasi
- [x] **Preloaded suite ✓ (`panen_suite_demo`):** (1) **Kalkulator** (`calc.cs`→CALC.ELF) — `Buitenzorg.UI` Grid 4×4 tombol ter-tema (digit biru + operator oranye) di atas display numerik; klik via `Button.Tag`→engine `Calc`; `12+3=`→15 → `MILESTONE: CALC OK`; screenshot `docs/img/desktop-calc.png`. (2) **2048** (`game2048.cs`→G2048.ELF) — game geser-ubin: papan 4×4 `Board2048:UIElement` (ubin rounded berwarna per-nilai + angka via DrawChars), engine slide+merge per arah; `2+2`→4 & `4 4 8 8`→`8 16` → `MILESTONE: GAME OK`; screenshot `docs/img/desktop-2048.png`. (3) **Jam** (`clock.cs`→CLOCK.ELF) — jam analog: face AA + 12 tick + jarum jam/menit/detik (rotasi via `SinFx/CosFx`) + digital HH:MM:SS (DrawChars); verified geometri jarum + format waktu → `MILESTONE: CLOCK OK`; screenshot `docs/img/desktop-clock.png`. (4) **Piano** (`piano.cs`→PIANO.ELF) — keyboard 1 oktaf: `Piano:UIElement` tuts putih (rounded + label C-D-E-F-G-A-B-C) + tuts hitam, klik tuts → `Mixer.Beep(freq)` (Buitenzorg.Audio); verified arpeggio C-E-G-C (4 nada) → `MILESTONE: PIANO OK`; screenshot `docs/img/desktop-piano.png`. (5) **App Store** (`store.cs`→STORE.ELF) — store front **TERHUBUNG ke `pkg.rs`**: katalog dari syscall **`PKG_LIST`** (registry + status terpasang), install/hapus via **`PKG_SET`** (gating `run`); `StoreView:UIElement` (nama + kategori + badge TERPASANG/TERSEDIA) + tombol PASANG/HAPUS; verified: load katalog + install app + **baca ulang registry buktikan state kernel berubah** → `MILESTONE: STORE OK`; screenshot `docs/img/desktop-store.png`. ABI: `PKG_LIST`=27/`PKG_SET`=28 (+`PkgInfo` 48B, mirror C# + kontrak 9+4 lulus + `docs/abi.md`); registry (`pkg.rs`) diperluas + `category`. (6) **File Manager** (`filemgr.cs`→FILES.ELF) — jelajah VFS via syscall **`FS_LIST`** (path kosong=daftar mount, path mount=daftar file); `FileView:UIElement` (path bar + ikon folder/file + nama) + navigasi (klik folder / `..`); verified: daftar mount + masuk `/disk` + cek `CALC.ELF` ada → `MILESTONE: FILES OK`; screenshot `docs/img/desktop-files.png` (isi `/disk` NYATA dari FAT). ABI: `FS_LIST`=29 (+`FsEntry` 32B, mirror + kontrak lulus). (7) **Text Editor** (`editor.cs`→EDITOR.ELF) — editor multi-baris: `TextArea:UIElement` (buffer char growable, caret, wrap `\n`, Type/Newline/Backspace) di bawah `Menu` bar; verified ketik 2 baris + backspace → cek buffer → `MILESTONE: EDITOR OK`; screenshot `docs/img/desktop-editor.png`. **SUITE OK** (4-media, kernel-side).
- [x] **Suite bawaan (8 app, semua kategori):** productivity (Kalkulator, Editor), game (2048), utilitas (Jam, File Manager), multimedia (Piano, Image Viewer), store (App Store) — di atas BCL v0.15 + Drawing/UI/Audio; masing-masing verified + screenshot. Menyusul (opsional): AI app, themes bawaan tambahan
- [x] **Image Viewer** (`imgview.cs`→IMGVIEW.ELF) — memuat `/disk/PHOTO.BMP` lewat syscall baru **`FS_READ`** (ABI 30: baca isi file VFS ke buffer klien — juga fondasi file-open untuk editor/apps lain), dekode via `Bmp.Load` (Buitenzorg.Drawing), tampil di `ImageView:UIElement` (fit-to-box + aspect ratio, backdrop papan-catur, caption nama+dimensi); verified dekode 320×200 + piksel non-trivial → `MILESTONE: IMGVIEW OK`; screenshot `docs/img/desktop-imgview.png`. ABI: `FS_READ`=30 (mirror C# + kontrak dua sisi + `docs/abi.md`).
- [x] **Bugfix ABI syscall userland (ditemukan oleh Image Viewer):** inline-asm `syscall` di shim (`bzstart.rs`) mendeklarasikan rdi/rsi/rdx sebagai `in` (janji "tidak berubah") — padahal kernel entry memanggil dispatcher C dan hanya me-restore rcx/r11/rsp sebelum `sysretq`. LLVM menyimpan `chunk` di rdi melintasi syscall mmap di `SystemNative_Malloc`, `HEAP_CAP` terisi sampah, grow-check tidak pernah aktif lagi → alokasi besar KEDUA menabrak akhir chunk yang benar-benar termap (page fault USER). Fix: rdi/rsi/rdx dideklarasi clobbered (`inlateout => _`) — kelas bug yang sama dengan fix r8/r9/r10 sebelumnya. `heap.cs` kini menguji multi-growth (4×400 KiB) agar path ini tetap teruji; heap juga disederhanakan (mmap-first, tanpa arena .bss; kursor `AtomicUsize`).
- [~] **Optimization pass (mulai):** fast boot — pangkas durasi `Sleep()` animasi demo app (xox/paint/taskmgr/widget/webview) dari ~12 s → ~3 s; boot **69.8 s → 58.5 s** (~16% lebih cepat), semua milestone tetap. Kernel cetak estimasi tick di akhir boot. **Catatan fast-I/O:** dicoba multi-sector IDE-PIO + in-memory FAT + contiguous-run batching di `read_file` — ternyata **2× lebih lambat** di QEMU (emulasi PIO per-sector QEMU sudah cepat; baca-seluruh-FAT per file + churn buffer justru menambah overhead) → **di-revert**. **Benchmark regresi CI ✓** (`bench.sh`, v1.0). Menyusul: startup app, footprint, SIMD hot-path

### 🧑‍💻 Developer Experience & Onboarding
- [x] **Getting-started ramah pemula** (`docs/getting-started.md`): langkah dari nol (dependency → build → boot QEMU) + troubleshooting
- [x] **Skrip quick-start** (`scripts/quickstart.ps1`/`.sh`): pasang otomatis semua dependency (Rust nightly+target, .NET SDK, QEMU, bflat) → build → boot OS di QEMU
- [x] **Panduan app pertama** (`docs/first-app.md`): scaffold template → tulis → build → jalankan; + katalog contoh app pakai library built-in (Drawing/UI/Audio/Bcl) dengan cuplikan kode
- [x] **Jalankan di VM** (`docs/run-in-vm.md` + `scripts/make-vm-images.ps1`/`.sh`): konversi image ke VMware `.vmdk` & VirtualBox `.vdi` via `qemu-img` + berkas config/`.vmx` + langkah setup (TESTED: vmdk+vdi terbuat)
- [~] **Ekstensi VS Code** (`sdk/vscode-extension`): dicek & dilengkapi — new project (+pemilih template), build&run, validasi manifest, deploy, GDB attach; **debugging DAP penuh** (breakpoint adapter) masih TODO

### 🖱️ Interaktivitas, Format Gambar & Desktop UX (pra-v1.0, permintaan 2026-07-24)
- [x] **Keyboard input routing untuk app interaktif ✓ (2026-07-24):** `run editor`/`run files` dari shell → ketikan pengguna dirutekan ke app via `KEY_READ` (busy-poll, bukan `bz_yield` yg context-switch di SYSCALL stack). Editor: ketik/backspace/newline nyata (verified: baris live "AKU KETIK DI EDITOR"); File Manager: navigasi W/K/S/J + Enter buka folder + Backspace naik + ESC keluar (verified: jelajah /disk live). Syscall baru **`IS_INTERACTIVE`**=31 (COUNT=32, `crate::interactive` AtomicBool set true sebelum `desktop_loop`): 0 saat boot-demo headless (app skip loop, exit, tak blokir boot) / 1 saat desktop hidup. **Root-cause bug PENTING diperbaiki:** `PRIVILEGE_STACK` (rangkap TSS rsp0 + SYSCALL stack thread utama non-threaded) tadinya 20 KiB — `WIN_PRESENT`→`present_now`→`compose_into` (compose ~20 window: title/teks Noto/blit canvas) OVERFLOW ke statics sebelah secara INTERMITTEN (layout-dependent) → smash return address → cascade breakpoint/#PF/#DF + rodata dump (mirip Heisenbug from_raw_parts tapi bug BEDA = stack kekecilan). Fix: `PRIV_STACK_SIZE`=64 KiB (gdt.rs). Plus `present_now` pakai `PRESENT_BUF` static reuse (bukan Vec 3.7MB tiap present → fragmentasi/OOM saat interactive) + heap kernel 32 MiB (back buffer + PRESENT_BUF + ~20 canvas + ramdisk > 16 MiB). Smoke 4-media LOLOS bersih (rodata:0).
- [x] **Dukungan JPG di `Buitenzorg.Drawing` ✓ (2026-07-24):** cek → sebelumnya HANYA BMP 24-bit (class `Bmp`). Ditambah **decoder JPEG baseline** (`Jpeg.Load` di `bzgfx.cs`): JFIF/SOF0 sequential, Huffman entropy (canonical min/max/valptr), dequant + **IDCT integer separable** (fixed-point cos, TANPA float), YCbCr→RGB, chroma subsampling 4:4:4/4:2:2/4:2:0, restart markers (DRI/RSTn). Progressive (SOF2)/arithmetic/CMYK → return null. **Semua state flat value-type array** (bukan jagged/class[] — hindari zerolib stelem.ref: Huffman+quant+plane di-pack ke satu int[]/byte[] dgn offset per-tabel). Image Viewer kini format-aware (magic "BM"→Bmp, 0xFFD8→Jpeg). Verified: `jpgtest.cs` dekode GRAD.JPG (64×64 gradien merah→biru 4:2:0, di-generate ffmpeg, embed via build.rs) vs referensi ffmpeg — piksel (8,32)≈R221/B32, (32,32)≈R125/B130, (56,32)≈R28/B225 dalam toleransi + arah gradien benar → `MILESTONE: JPEG OK`; smoke 4-media LOLOS. Progressive JPEG menyusul (opsional).
- [x] **Enhance UI/UX desktop — taskbar, start menu, desktop ✓ (2026-07-24):** shell desktop baru di `wm.rs` gabung ide macOS (ikon rounded, jam pojok, tile membulat) + Ubuntu/GNOME (launcher aplikasi) + Win XP (tombol **Start** hijau + start menu). **Taskbar** (34px, gradien): tombol Start (pill gradien + logo), tombol window berjalan (rounded, gradien saat aktif), **tray kanan** = nama tema + **jam HH:MM live dari CMOS RTC** + pip workspace. **Start menu** (klik Start): panel membulat + shadow, header "Buitenzorg OS / Gravicode Studios", daftar 10 app (Files/Editor/Kalkulator/App Store/Gambar/Jam/Piano/2048/Task Manager/Paint) + baris power (Matikan/Restart). **Ikon desktop** (kiri-atas, 4): tile gradien + label, **klik-ganda luncurkan app**. Launch dirutekan via `wm::take_pending_launch()` → `desktop_loop` panggil `app::run_named` (power → `power::shutdown/restart`). Helper baru: `gradient_v`/`fill_rounded_gradient`/`darken`/`lerp`/`read_clock`. Verified: `wm::self_test()` (klik Start→buka menu→klik row "files"→launch ter-antre) → `MILESTONE: DESKTOP SHELL OK`; render terverifikasi screenshot `docs/img/desktop-shell.png` (ikon+Start+jam 07:36+taskbar); smoke 4-media LOLOS. Menyusul (opsional): dock magnify, tray volume, drag ikon, wallpaper per-user di menu.

### 🪄 MagicAppGen — Magic App Generator (tools host, pra-v1.0, permintaan 2026-07-24)
- [x] **App Avalonia UI ✓ (2026-07-24)** `tools/MagicAppGen` (net10.0, Avalonia 11.2 + AvaloniaEdit + Semantic Kernel 1.61): code editor + generate-app-with-prompt via LLM; asisten AI **"Jack - The Code Bender"**. Verified: `dotnet build` LOLOS + app benar-benar JALAN (screenshot `docs/img/magicappgen.png`)
- [x] **Uji end-to-end generate app ✓ (2026-07-24)** — mode headless baru `--generate <outDir> <prompt>` di `Program.cs` menjalankan Jack lewat CLI (panggilan LLM nyata pakai provider di app.config). Diuji dgn OpenAI key nyata (gpt-4o-mini): prompt "app console jumlahkan 1..10" → Jack panggil `ScaffoldProject`+`WriteFile` → `main.cs` (zerolib-correct: `bz_write`, format digit manual, tanpa ToString) **compile dgn bflat --stdlib:zero + link ke ELF ring-3 + JALAN di Buitenzorg OS** (`SalamApp: 1..10 = 55` / `MILESTONE: SALAM OK`, exit 0). Wiring OS uji di-revert setelah verifikasi (repo bersih); fitur `--generate` disimpan
- [x] **Uji app kompleks (UI + tombol) + compile-repair loop ✓ (2026-07-24)** — prompt "app desktop UI window dgn Button penghitung". Temuan: gpt-4o-mini menghasilkan API halusinasi (`Button.OnClick`, `TextBlock(char[])`, `Array.Reverse`, `AsSpan`) → TAK compile. Ditambah **kernel function `CompileCheck`** (compile main.cs dgn bflat --stdlib:zero, sumber library auto dari `using`, kembalikan OK/error) + prompt instruksi loop perbaikan + `--model` override + guidance UI diperkuat (pola klik tombol `.Clicks`+`host.Mouse` bukan event, TextBlock string-only + custom UIElement+DrawChars utk angka, usings wajib, no Array.Reverse). **Bug CompileCheck diperbaiki:** argumen bflat kurang subcommand `build` → semua check gagal "unrecognized argument" (kode sebenarnya benar); + drain pipe async anti-deadlock + obj temp unik. Hasil: **gpt-4o konvergen 3 iterasi** (halusinasi BzMath.FormatInt→fix→OK) → app UI (custom `CounterDisplay:UIElement`+DrawChars, tombol via Clicks+host.Mouse) compile+link+**JALAN di Buitenzorg** (`MILESTONE: COUNTER OK`, exit 0). Catatan: gpt-4o-mini tak konvergen di API UI walau ada loop; pakai gpt-4o utk app non-trivial. Wiring OS uji di-revert
- [x] **LLM multi-provider ✓:** OpenAI, Claude (Anthropic), Gemini (Google AI), Ollama — `AiService.BuildKernel` per-provider; Claude tak punya konektor SK resmi → di-bridge dari `Anthropic.SDK` `IChatClient` via `AsChatCompletionService()`. Setting (model/API key/endpoint/temperature/system prompt) di **app.config**, semua bisa diubah dari dialog Settings (screenshot `docs/img/magicappgen-settings.png`); pemilihan provider+model di bagian atas Chat Panel. Balasan **streaming**
- [x] **Kernel functions Buitenzorg ✓** (`Ai/BuitenzorgPlugin.cs`): `GetApiReference(drawing|ui|audio|bcl|syscalls|gotchas)` berisi **tanda tangan persis** (urutan argumen — sumber kesalahan utama LLM) + gotcha zerolib; `ListTemplates`/`GetTemplateSource`/`ScaffoldProject`; `WriteFile`/`ReadFile`; `BuildApp` (jalankan `scripts/build.ps1`, output ke logs panel) dan `RunApp` (`scripts/smoke-test.ps1`)
- [x] **Common functions ✓** (`Ai/CommonPlugin.cs`): `SearchInternet` (**Tavily**, answer + top results), `ScrapeWebPage` (HTML→teks), `MathCalculation`, `CheckDateTime`
- [x] **Chat panel kanan ✓:** attach gambar (dikirim sbg `ImageContent`), **resize width** (GridSplitter), hide/show (View menu), kirim **Ctrl+Enter** atau tombol Send, tombol **Clear** thread
- [x] **Menu+toolbar ✓:** New Project (Blank / **From Template**), Open Project/File, Save, Close Project, Go To Line, Build, Run, Deploy, Exit; pilihan bahasa di toolbar (default **C#** + JS/TS/Python); **status bar** (status + provider·model) + **logs panel** bawah; show/hide line number; tema gelap modern
- [x] **8 template proyek ✓** (`Services/ProjectTemplates.cs`): `console`/`desktop-ui`/`drawing`/`game`/`audio` (C#) + `js`/`ts`/`python` (polyglot), masing-masing + `app.manifest` + README. **Kelima template C# TERVERIFIKASI ter-compile dengan bflat `--stdlib:zero` sungguhan** (bukan hanya "terlihat benar") — bug urutan argumen (`FillGradientV`/`DrawShadow`/`DrawString`/`DrawChars`/`Mixer.Beep`) ketahuan & diperbaiki lewat uji compile ini. Ada mode headless `--list-templates`/`--scaffold <id> <dir> [Nama]` untuk verifikasi tanpa display (screenshot picker: `docs/img/magicappgen-templates.png`)

### 📚 Kelengkapan BCL & Adopsi di App (pra-v1.0, permintaan 2026-07-24)
Semua di `userland/hello-csharp/bzbcl2.cs` (melengkapi `bzbcl.cs`), diverifikasi
oleh `bcl2.cs` di ring-3 QEMU -> **`MILESTONE: BCL2 OK`**, smoke 4-media LOLOS,
0 fault. ABI baru: `FS_WRITE`=32, `CLOCK_RTC`=33, `NET_SOCKET/BIND/SEND/RECV/CLOSE/INFO`=34..39
(COUNT=40) + struct `RtcTime`/`NetDatagram`/`NetInfo`; kontrak Rust (5 test) + C# (11 test) + `docs/abi.md` diupdate.
- [x] **`System.IO` ✓** — `BzPath` (Combine/GetFileName/GetDirectoryName/GetExtension/HasExtension/Up), `BzFile` (ReadAllBytes/ReadAllChars/**WriteAllBytes/WriteAllChars**/Exists), `BzDir` (GetEntries/GetMounts/Count/Contains via `FsEntry`), `BzMemoryStream` (Read/Write/Seek/SetLength/ToArray, buffer tumbuh). **Syscall `FS_WRITE` BARU** (vfs::write) — verified: tulis `/ram/BCL2.TXT` lalu baca balik identik; baca `/disk/PHOTO.BMP` (4096 B, magic "BM"); listing `/disk` = 34 entri berisi CALC.ELF
- [x] **`System.Text` ✓** — `BzEncoding` UTF-8 (GetBytes/GetChars/ByteCount, 1-4 byte + surrogate pair) + ASCII (fallback '?'); verified round-trip 5 char -> 8 byte -> 5 char termasuk U+00E9 dan U+2713
- [x] **`System.Text.RegularExpressions` ✓** — `BzRegex` backtracking: literal, `.`, `[abc]`/`[^a-z]` + range, `^`/`$`, `*`/`+`/`?`, `|`, grup `(...)`, escape `\d \D \w \W \s \S`; IsMatch/Match/Replace/Split. **Kontinuasi eksplisit** (`BzRxCont` linked list) supaya backtrack BENAR melewati batas grup — `(a|ab)c` cocok "abc" DAN "ac" (naive matcher gagal di kasus pertama). Verified: `^[0-9]+$`, `\w+@\w+\.[a-z]+`, `colou?r`, Replace `[0-9]+`->`#` = "a#b#c#d", Split. BELUM: backreference, lazy quantifier, `{n,m}`, lookaround, ekstraksi capture
- [x] **`System.Globalization` ✓** — `BzCulture`: FormatInt/FormatIntAt/FormatGrouped (1234567 -> "1,234,567"), FormatFixed (fixed-point, tanpa float: -31415/1e4 -> "-3.1415"), FormatPercent, FormatBytes (1536 -> "1.5KiB"), ToUpper/ToLowerInvariant, MonthAbbrev. Plus **`BzDateTime`** (jam dinding NYATA lewat **syscall `CLOCK_RTC`** + modul kernel baru `rtc.rs`): Now/IsValid/IsLeapYear/DaysInMonth/DayOfWeek (Sakamoto)/FormatDate/FormatTime/Format — verified `2026-07-24 03:29:36` dari CMOS
- [x] **`System.Diagnostics` ✓** — `BzStopwatch` (StartNew/Start/Stop/Restart/ElapsedTicks/Timestamp di atas `CLOCK_MONO`=TSC; satuan tick TSC krn frekuensinya tak diekspos — pakai `BzSystemInfo` utk detik), `BzProcess` (GetProcesses/Count/FindByName/Kill via `PROC_LIST`/`PROC_KILL` — verified 2 proses, nama "kernel"), `BzDebug` (Write/WriteLine/Assert -> serial)
- [x] **`System.Management` ✓** — `BzSystemInfo.Query()` gabung `SYS_STAT` + `AUDIO_STAT`: uptime/TickHz/heap/TaskCount/RAM + audio present/rate/channels/bits/volume/muted, plus UptimeSeconds & HeapPercent. Verified: uptime 37 s, heap kernel 33%, RAM 505 MiB, audio 48 kHz. (bcl2 dipindah jalan SETELAH demo audio supaya subsistemnya benar-benar hidup saat diinspeksi)
- [x] **`System.Net` + `System.Net.Sockets` ✓ (UDP NYATA)** — kernel `net.rs` dapat **UDP**: `UdpSocket` (port + backlog 16), `udp_socket/bind/send/recv/close`, build header UDP + **checksum pseudo-header IPv4** yang benar, dispatch `IPPROTO_UDP` di `handle_ipv4`. C#: `BzIPAddress` (Parse/Format/Equals, tolak "10.0.2" & "300.1.1.1"), `BzSocket` (CreateUdp/Bind/SendTo/Receive/ReceiveWithRetry/Close, non-blocking), `BzNetInfo` (alamat + counter). **Verified round-trip nyata:** kirim 24 byte port 7001 -> 7000 di 10.0.0.1, payload identik, RemotePort/RemoteAddress benar, receive kedua = 0 (non-blocking), counter tx/rx naik
- [~] **`System.Net.Http`** — `BzHttp` BuildGet/BuildPost/ParseStatus/GetHeader (verified: status 200 diparse, offset body benar, header `content-length` = "5"). **Lapisan pesan saja**: kernel BELUM punya TCP (`sock_kind::STREAM` ditolak), jadi belum bisa connect. Begitu TCP mendarat, klien = BzHttp + stream. Perangkat juga masih loopback (driver NIC e1000 menyusul)
- [x] **`System.Threading.Tasks` ✓** — `BzTask` Run/Wait/WhenAll/Yield di atas `THREAD_CREATE`/`THREAD_JOIN` + `BzMutex` (futex). Body = **function pointer** (`delegate*<ulong,void>` + `&Worker`), bukan delegate (delegate di-cache di GC static -> fault). **Temuan bagus: function pointer C# BERHASIL jadi thread entry** di zerolib (yang gagal cuma `[UnmanagedCallersOnly]`). Verified: 2 task x 200 increment atas counter mmap bersama -> 400, WhenAll + IsCompleted benar. Kooperatif, tanpa thread pool
- [x] **`System.Timers` ✓** — `BzTimer` (Interval tick, Start/Stop/AutoReset/Poll/Remaining/Count) di atas `TICKS`. Dipompa dari loop app (ring-3 tak punya delivery sinyal — polled timer itu desain yang jujur, bukan keterbatasan tersembunyi); interval yang terlewat TIDAK menumpuk. Verified: 2x fire auto-reset + 1x one-shot lalu Enabled=false
- [x] **`GC` (System) ✓** — `BzGC` GetAllocatedBytes/GetTotalMemory/ChunkCount/AllocationCount/FreeInChunk dari akuntansi allocator NYATA (`bz_heap_stats` BARU di `bzstart.rs`, bukan taksiran). `Collect()` mengembalikan **false** — jujur: bump heap belum punya kolektor. Verified: alokasi 200000 byte tercatat, committed >= allocated
- [x] **`Pkg` ✓** — `BzPkg` List/Count/Find/Search/IsInstalled/Install/Remove via `PKG_LIST`/`PKG_SET`. Verified 11 paket, install "calc" lalu baca-ulang registry membuktikan state KERNEL berubah (bukan cuma state lokal)
- [x] **Verifikasi ✓** — `bcl2.cs` (BCL2.ELF, `run bcl2`) menguji tiap namespace -> **`MILESTONE: BCL2 OK`**; marker `BCL2 OK` (app-side, IDE-only) + `BCL2 PAL OK` (kernel-side) masuk smoke test ps1+sh; smoke 4-media LOLOS, rodata/fault = 0
- [x] **Audit adopsi app & widget ✓ (2026-07-24)** — semua 14 app/widget diperiksa; **7 diadopsi** (yang benar-benar untung), **7 sengaja TIDAK** (alasan dicatat), plus **2 bug nyata ketemu lewat audit**. Smoke 4-media LOLOS, 0 fault, semua MILESTONE app tetap OK.
  - **Diadopsi:**
    - `clock.cs` → **`BzDateTime.Now()`**: jam sekarang menampilkan **waktu CMOS SUNGGUHAN + baris tanggal** (sebelumnya hardcode 10:08:37). Verifikasi ikut berubah: format dicek terhadap nilai yang benar-benar ditampilkan, plus tanggal harus kalender valid (bukti `CLOCK_RTC` jalan). Log boot: `2026-07-24` → `MILESTONE: CLOCK OK`
    - `taskmgr.cs` → **`BzProcess` + `BzSystemInfo`**: **menghapus salinan struct `ProcInfo`/`SysStat` buatan sendiri** (tempat kedua yang harus diedit manual tiap ABI berubah — risiko rot nyata) + 3 P/Invoke; heap kini tampil "1.5MiB" via `BzCulture.FormatBytes`
    - `widget.cs` → **`BzSystemInfo` + `BzCulture` + `BzTimer`**: hapus mirror `SysStat` ke-2, refresh pakai timer (bukan `Sleep` telanjang) — sesuai maksud varian widget "update periodik"
    - `store.cs` → **`BzPkg`**: hapus dekode `PkgInfo` lewat pointer-aritmetik (`*(ulong*)(e + 40)`) + 2 P/Invoke; verifikasi re-read registry tetap membuktikan state KERNEL berubah
    - `filemgr.cs` → **`BzDir` + `BzPath`**: hapus dekode `FsEntry` (`*(ulong*)(e + 24)`) + P/Invoke; path pakai `char[]` bukan `byte*` stackalloc
    - `imgview.cs` → **`BzFile` + `BzPath` + `BzCulture`**: hapus `ReadFile` + `PutInt` sendiri; nama file di caption kini **diturunkan dari path** (`BzPath.GetFileName`) bukan ditulis dua kali
    - `editor.cs` → **`BzFile`**: menu FILE jadi **NYATA** — Ctrl+S simpan & Ctrl+O buka `/ram/NOTE.TXT` *(sejak 2026-11-01: `/disk/NOTE.TXT`, persisten)*; demo boot menyimpan lalu membaca ulang dan membandingkan (round-trip masuk kriteria `MILESTONE: EDITOR OK`)
  - **Sengaja TIDAK diadopsi** (menautkan ~30 KB library hanya demi satu formatter angka = rugi; formatternya lokal, dipakai di dalam `UIElement` custom): `calc.cs`, `game2048.cs`, `piano.cs`, `paint.cs`, `webview.cs`, `xox.cs`, `audiopanel.cs`. `hello.cs`/`svc.cs` sengaja minimal (milestone v0.4/v0.5)
  - **2 bug ketemu saat audit:** (1) `scripts/build-hello-csharp.sh` (jalur Linux/CI) **tidak punya `imgview` & `jpgtest`** — hanya ditambahkan ke `.ps1`, jadi build Linux menghasilkan image tanpa IMGVIEW.ELF/JPGTEST.ELF dan smoke CI akan gagal di marker itu; kedua skrip kini sama-sama 27 program. (2) `sdk/templates/desktop-csharp/README.md` masih menulis "belum punya GC — **hindari `new T[]`**", padahal heap sudah jalan sejak v0.15 inc 4 — diganti dengan aturan zerolib yang benar + katalog library
  - **Tool host:** `MagicAppGen` — `GetApiReference("bcl")` diperluas jadi katalog tanda-tangan **lengkap** `bzbcl.cs`+`bzbcl2.cs` (12 namespace, termasuk batas jujur "UDP loopback saja / tak ada TCP" dan "`Collect()` = false"), dan system prompt di `app.config` menyebut `bzbcl2.cs` + menyuruh panggil `GetApiReference` dulu — tanpa ini AI menghasilkan kode yang tak tahu System.IO/Regex/dll ada. `docs/first-app.md` dapat tabel katalog namespace baru. `bz` CLI tak butuh perubahan (scaffolding manifest/template, tak menyentuh API app)

### 🛠️ Developer Workstation — IDE, Terminal, Package Manager, Debug & Deploy (permintaan 2026-11-01)
Rencana lengkap + urutan fase: [PLAN.md §Developer Workstation](PLAN.md). Fase 1 tak butuh prasyarat baru.
- **Prasyarat**
  - [x] **Semua app = proses ✓ (2026-11-01)** — `run_named` (demo boot) kini spawn + wait yang memblokir, bukan menjalankan app di task peluncur; mode "attached" dihapus (tak ada app lagi di privilege stack boot task, semua bisa di-kill). Ke-86 milestone tetap lolos; Editor interaktif + close-to-quit diuji ulang di desktop live
  - [x] **Reklamasi frame memori ✓ (2026-11-01)** — free list intrusif di frame allocator; `aspace::destroy` mengembalikan semua halaman user + page table + PD proses yang mati (slot identity-map bootloader yang dibagi dilewati); `MUNMAP` juga membebaskan frame. Diverifikasi `multiproc_demo`: 0 frame bocor setelah semua proses uji (normal/di-kill/fault) + 0 pertumbuhan selama 20 siklus spawn/exit (12.991 frame didaur ulang saat boot)
  - [x] **Spawn + argv + wait dari ring 3 ✓ (2026-11-01)** — syscall `PROC_SPAWN`=40 / `PROC_WAIT`=41 / `PROC_ARGS`=42 (COUNT=43, kontrak Rust+C# + `docs/abi.md`); wait memblokir sungguhan (futex, dibangunkan saat proses keluar); `BzProcess.Start/WaitForExit/GetArgs`; shell `run <app> <arg..>`. Diverifikasi `proctest.cs`: induk men-spawn dirinya (via path `/disk/PROCTEST.ELF` + via nama app) dengan `child 7`/`child 42`, menunggu → exit 7 dan 42; wait pid tak dikenal ditolak → `MILESTONE: PROCCTL OK`
  - [x] **Multi-proses ring-3 preemptif ✓ (increment 1, 2026-11-01)** — beberapa app ring-3 jalan bersamaan, di-preempt timer, masing-masing di **address space sendiri** (`aspace.rs`: 2 page directory privat per proses untuk 2 GiB user, ditukar di PDPT saat switch); stack SYSCALL + **TSS rsp0** + **FXSAVE** per task; spawn/kill (`app::spawn_named`/`kill`); fault ring-3 (#PF/#GP) hanya mematikan app-nya (exit 0xDEAD), kernel tetap hidup; desktop + shell `run` meluncurkan app sebagai proses; `KEY_READ` mengikuti fokus window. Diverifikasi `mproc.cs` (3 salinan non-yield + 1 proses fault): memori di VA sama tetap privat, register SSE utuh, ≥10 preemption ring-3 nyata (36), 1 di-kill, 1 fault terisolasi → `MILESTONE: MULTIPROC OK`; **kontrol negatif**: tanpa preemption ring-3 tes GAGAL (`preemptions=0, concurrent=false`). Bonus: frame allocator O(n)→kursor, boot ke READY ~35 s → ~20 s (tick).
  - [x] **FS tulis persisten di `/disk` ✓ (2026-11-01)** — driver ATA kini `WRITE SECTORS` + `CACHE FLUSH`; penulisan FAT generik **FAT12/16/32** (alokasi cluster, semua salinan FAT, entri dir + stempel waktu RTC, rantai lama dibebaskan *setelah* entri baru ditulis, `delete_file`, `free_space`, FSInfo FAT32 di-invalidate); `/disk` di-mount **rw**; shell `echo > / >>`, `rm`, `df`; Editor menyimpan ke `/disk/NOTE.TXT`. **Bug laten diperbaiki:** penulis FAT12 lama menulis salinan FAT kedua *ke dalam root directory* `/ram` (offset dihitung dari `(data_start-fat_start)/2`), dan overwrite membocorkan rantai cluster lama. Diverifikasi: penghitung boot `BOOTCNT.TXT` naik 1→2 lintas reboot pada image yang sama; smoke test mencari byte yang ditulis kernel di dalam file image host; pemeriksa FAT independen (Python) → salinan FAT identik, 0 cluster bocor, tanpa cross-link → `MILESTONE: DISK WRITE OK` (IDE). Belum: FS journaled, subdirektori, nama panjang (LFN), AHCI/NVMe.
  - [ ] Driver NIC (virtio-net → e1000) + TCP
  - [ ] Kernel log ring buffer (`dmesg`) + log per-app
- **Terminal & Shell**
  - [ ] Line editing (kursor, Home/End, Ctrl+A/E/U/K/W) + history (↑/↓, persist) + tab-completion perintah/path
  - [ ] Pipes `|`, redirection `> >> <`, `&& || ;`, env vars (`export`/`env`/`$PATH`/`$?`), alias, skrip `.bzsh` + `source`
  - [~] Coreutils: [ ] `cp mv rm mkdir rmdir touch head tail wc grep find hexdump which tee sort` · [x] `ps` + `kill <pid>` (2026-11-01, terverifikasi `multiproc_demo` + desktop live) · [ ] `top` · [ ] `dmesg free uptime df`
  - [ ] App Terminal mandiri (multi-tab, scrollback, copy/paste, ANSI) · [ ] job control (`&`/`jobs`/`fg`/Ctrl+C) — butuh multi-proses
- **Package Manager**
  - [ ] Format `.bzpkg` (manifest + ELF/skrip + aset + SHA-256) + `bz install ./x.bzpkg`
  - [ ] Host `bz pack` / `bz publish`
  - [ ] Repo index remote + `bz update/upgrade` + versi + dependency resolution + rollback — butuh TCP
  - [ ] State terpasang persisten · [ ] library SDK sebagai paket ber-versi · [ ] izin manifest ditegakkan permission broker
- **IDE "Buitenzorg Studio"**
  - [ ] Editor multi-tab + nomor baris + syntax highlighting (C#/JS/TS/Python/manifest) + find/replace + undo/redo
  - [ ] Explorer proyek + New Project dari template + panel Terminal/Problems/Output
  - [ ] Run instan JS/TS/Python via `script.rs`
  - [ ] C# lewat remote build server (`bz build-server` di host) · [ ] compiler C# di dalam OS (jangka panjang, butuh CoreCLR)
  - [ ] Integrasi debugger (breakpoint gutter, variabel, call stack)
- **Debugging & Diagnostik**
  - [ ] Crash report app ring-3: register + backtrace ter-simbolisasi (symtab ELF) → `/ram/crash/`
  - [ ] Debugger app ring-3 (`int3` + single-step, baca/tulis reg/memori, `bz debug <app>`)
  - [ ] GDB remote stub di kernel (serial COM2 → TCP) — debug di hardware nyata
  - [ ] VS Code DAP adapter (breakpoint/step/variabel/stack)
  - [ ] Debugger skrip JS/TS/Python di `script.rs`
  - [ ] Profiler app (`bz prof <app>`: zona via syscall + sampling RIP user) · [ ] `bz logs [-f] [app]`
- **Deploy & Run (lokal + remote)**
  - [ ] `run <path>` untuk ELF/skrip di path mana pun + Run dari Studio + hot-reload skrip
  - [ ] Agen `bzd` (bz-link) di OS + host `bz device/deploy/run/logs/shell/push/pull/debug --remote`
  - [ ] Transport serial COM2 (QEMU `-serial tcp`, hardware via USB-serial) · [ ] UDP · [ ] TCP
  - [ ] Developer Mode (agen mati default) + pairing token + verifikasi hash + audit log
  - [ ] Ekstensi VS Code + MagicAppGen deploy lewat bz-link (bukan build ulang image)
- [ ] **Milestone:** `bz deploy --target serial` dari host → app jalan di OS yang sedang berjalan tanpa build ulang image, log mengalir balik, breakpoint kena; Studio menjalankan skrip Python di dalam OS; crash app menampilkan backtrace ter-simbolisasi

### 🎬 Multimedia, Internet, Game (2D/3D) & Hiburan (permintaan 2026-11-01)
Rencana lengkap + urutan fase: [PLAN.md §Multimedia, Internet, Game & Hiburan](PLAN.md). Fase 1 tak butuh prasyarat baru.
- **Prasyarat (kernel & driver)**
  - [x] Multi-proses preemptif (musik di latar saat game/browsing) — ✓ increment 1 (2026-11-01), lihat §Developer Workstation
  - [ ] Surface shared-memory zero-copy (ganti salinan per-piksel `BLIT`) + vsync/frame pacing + fullscreen eksklusif
  - [ ] Timer resolusi tinggi (APIC timer/HPET)
  - [ ] Audio berbasis IRQ + ring buffer latensi rendah · [ ] driver Intel HDA · [ ] capture mic
  - [ ] Event keyboard tekan/lepas + status tombol (kini `KEY_READ` hanya karakter) · [ ] mouse relatif/capture
  - [ ] Stack USB xHCI + HID (gamepad, keyboard/mouse USB) · [ ] UVC webcam · [ ] audio USB
  - [ ] Storage AHCI/NVMe + FS tulis persisten untuk media & aset game
  - [ ] Float/SIMD di ring-3 teruji (SSE/AVX untuk decoder & 3D)
- **Multimedia**
  - [ ] `Buitenzorg.Media`: pipeline demux→decode→render, jam media (A/V sync), `MediaPlayer`, media key
  - [ ] Mixer software multi-stream + volume per-app + resampling
  - [ ] Codec audio: WAV · FLAC · Ogg Vorbis/Opus · MP3 · [ ] synth MIDI (wavetable/FM)
  - [ ] Gambar: PNG (inflate) · GIF animasi · JPEG progresif · thumbnail/EXIF
  - [ ] Video: MJPEG · container MP4/MKV/WebM · VP8/VP9/AV1 software · YUV→RGB SIMD · subtitle SRT · decode hardware
  - [ ] App: Media Player · Music Player + perpustakaan · Photo Viewer (slideshow/zoom) · Sound Recorder · Camera · screenshot/perekam layar · widget Now Playing + OSD volume
- **Internet**
  - [ ] Driver NIC: virtio-net → e1000 → Realtek · [ ] Wi-Fi (jangka panjang)
  - [ ] TCP penuh · DHCP · DNS resolver · socket `STREAM` + `poll` · [ ] IPv6
  - [ ] Kripto + TLS 1.3/1.2: CSPRNG, SHA-2, AES-GCM, ChaCha20-Poly1305, X25519, RSA/ECDSA, store sertifikat root
  - [ ] `BzHttp` jadi klien nyata (HTTP/1.1 + HTTPS) · WebSocket · download manager
  - [ ] Browser: parser HTML5 · CSS (box model/flexbox/font) · layout + gambar · JS (subset ES + DOM di `script.rs`) · tab/bookmark/riwayat
  - [ ] App: Web Browser · radio internet/podcast · klien chat · pengaturan jaringan · firewall + izin jaringan per-app
- **Game 2D**
  - [ ] `Buitenzorg.Game`: game loop fixed-timestep, input (keyboard state/mouse/gamepad), sprite + animasi, tilemap, kamera, collision, particle, scene, SFX/musik, loader aset, save game
  - [ ] Performa: blit SIMD · dirty-rect · surface zero-copy · fullscreen
  - [ ] Game bawaan: Minesweeper · Solitaire · Snake · Tetris-like · Breakout · platformer demo
  - [ ] Template game di SDK + MagicAppGen · binding game untuk JS/Python
- **Game 3D**
  - [ ] Rasterizer software `Buitenzorg.Graphics3D`: math vektor/matriks, perspektif, clipping, z-buffer, culling, texture mapping, shading, loader OBJ/glTF
  - [ ] Demo: raycaster gaya Wolfenstein · kubus bertekstur berputar
  - [ ] GPU: virtio-gpu 2D → virgl 3D → API gaya Vulkan/WebGPU `Buitenzorg.GPU` (+ `compute.rs` `Backend::Gpu`) · [ ] driver GPU nyata (Intel dulu)
  - [ ] Engine: scene graph · animasi skeletal · fisika rigid body · audio 3D posisional
  - [ ] Lapisan libc (musl/newlib) + subset SDL2 → **port DOOM**
- **Hiburan & UX**
  - [ ] Mode game (fullscreen, tunda notifikasi, prioritas) · media key + OSD · hotkey screenshot
  - [ ] Kategori Game/Media di App Store · perpustakaan media lintas app
  - [ ] Pembaca e-book (TXT/EPUB) + komik (CBZ) · emulator showcase (CHIP-8/NES) · launcher ramah gamepad
  - [ ] Integrasi GenAI lokal (wallpaper, aset game, playlist)
- [ ] **Milestone:** musik Ogg/FLAC di latar + game 2D 60 fps dengan gamepad USB; video dengan A/V sync; browser membuka halaman HTTPS; adegan 3D bertekstur ≥30 fps (software dan virtio-gpu)

### 🚀 Stabilisasi & Rilis (v1.0) — *sedang berjalan*
- [x] **Security hardening: validasi pointer user di syscall ✓ (2026-07-24)** — **LUBANG NYATA ditutup.** Sebelumnya syscall menyalin lewat pointer mentah tanpa cek, jadi app ring-3 bisa: (a) `DEBUG_WRITE(alamat_kernel)` → kernel mencetak memorinya sendiri ke serial (**bocor info**); (b) `SYS_STAT`/`PROC_LIST`/`PKG_LIST`/`FS_READ`/`NET_RECV` dengan `out_ptr` = alamat kernel → kernel **menulis hasil ke memori kernel** (**tulis-sembarang = eskalasi privilege penuh**); (c) pointer tak-terpeta → kernel **page fault di dalam syscall lalu mati**. Fix: `memory::validate_user_range(ptr,len,need_write)` (tanpa wrap + seluruhnya < `USER_ADDR_MAX`=0x8000_0000 + **tiap halaman present & USER_ACCESSIBLE** + writable utk buffer keluaran) & `validate_user_cstr` (path NUL-term, dicek ulang tiap batas halaman). Dipasang di **SEMUA** syscall berpointer: DEBUG_WRITE/FB_INFO/WIN_CREATE/WIN_CMD (termasuk buffer BLIT & teks)/PROC_LIST/SYS_STAT/AUDIO_STAT/AUDIO_PLAY/FUTEX_WAIT/PKG_LIST/PKG_SET/FS_LIST/FS_READ/FS_WRITE/CLOCK_RTC/NET_SEND/NET_RECV/NET_INFO. Dua TODO lama di `sys_win_create`/`sys_debug_write` ("full address-space validation arrives with multi-process isolation") akhirnya ditepati. **Penting:** cek hanya di jalur ring-3 (`dispatch_from_user` dari entry SYSCALL); pemanggil kernel-internal (`dispatch`) kirim alamatnya sendiri & tetap dipercaya — ini ketahuan saat `syscall_smoke_test` kernel sendiri langsung ditolak validator (bukti validatornya bekerja). Verifikasi: `syscall::security_self_test()` menembak **14 probe bermusuhan** (alamat kernel, halaman tak-terpeta, rentang meluap, panjang wrap, null) — semua ditolak `INVAL` → **`MILESTONE: SECURITY OK`**; smoke 4-media LOLOS, 0 fault
- [x] **Stabilkan ABI ✓ (2026-07-24)** — tabel v1 **dibekukan**: `abi_v1_is_frozen` (Rust, 6 test) + `AbiV1IsFrozen` (C#, 12 test) memaku `ABI_VERSION`, `COUNT`=40, **ukuran + alignment 10 struct** lintas-batas, dan kode error. Renumber/ubah-layout gagal di test sebelum bisa masuk image rilis. Kebijakan didokumentasikan di `docs/abi.md` (§Pembekuan ABI v1): tambah = append + naikkan COUNT + update kedua mirror; ubah yang ada = versi mayor baru
- [x] **Benchmark regresi di CI ✓ (2026-07-24)** — `scripts/bench.sh`: boot headless lalu ambil dua angka yang memang sudah dicetak kernel (boot-to-READY dalam tick, throughput async I/O ops/s) dan gagal bila lewat budget (`BOOT_BUDGET_S`=90, `AIO_MIN_OPS`=10000; longgar sengaja — menangkap regresi nyata, bukan jitter runner). Job baru di `ci.yml`. Logika parsing diverifikasi terhadap log boot sungguhan (boot=45s, aio=36418 ops/s) — uji itu **menemukan bug pola sed**: kernel mencetak `~N ops/sec` atau `>N ops/sec` tergantung resolusi tick, pola awal cuma menangani `>`
- [x] **CI diperbaiki ✓ (2026-07-24)** — **BUG NYATA:** `ci.yml` memicu di `branches: [main]` padahal default branch repo ini **`master`**, jadi **CI tak pernah jalan sekali pun saat push**. Sekarang `[master, main]` + `workflow_dispatch`
- [x] **Debugger + profiler ✓ (2026-07-24)** — **Debugger:** `scripts/debug-kernel.ps1`/`.sh` boot QEMU ditahan dgn GDB stub `tcp:1234` lalu attach GDB pakai simbol kernel (`bzkernel` ELF tak-strip) + `scripts/debug-kernel.gdb` (helper `bz-break-main`/`bz-faults`/`bz-regs`; simbol Rust ter-mangle, GDB demangle otomatis). Fallback cetak perintah attach manual kalau `gdb` tak ada. Diverifikasi: GDB stub QEMU merespons RSP stop-reply `T05` (SIGTRAP) via probe TCP; simbol `kernel_main`/`page_fault_handler`/`double_fault_handler` ada di ELF (via llvm-objdump). **Profiler:** `profile.rs` — profiler zona ter-instrumentasi berbasis **TSC**, `Guard::new("nama")` scope-timer (inert saat off = 1 atomic load, tak ganggu boot), registry 64-zona di spin-lock (interrupt off, aman thd timer IRQ), `report()` tabel terurut (calls/total/avg/max/share permille tanpa float). Instrumentasi jalur panas: `syscall` (dispatch_from_user), `wm::compose`, `fb::present`. Shell `prof [self|on|off|reset|report]`. Verifikasi `profiler_demo()`: 3 zona bersarang rasio biaya 20x, assert jumlah panggilan tepat (20 tiap) + cheap<expensive + outer mengurung + zona-saat-off tak terekam → **`MILESTONE: PROFILER OK`** (laporan nyata: demo-outer 50.1% / demo-expensive 47.4% / demo-cheap 2.4%, rasio 19.7x). Docs `docs/debugging.md`. Smoke 4-media LOLOS. *(DAP breakpoint penuh di VS Code masih menyusul — GDB kernel-level sudah lengkap.)*
- [x] **Dokumentasi/tutorial rilis ✓ (2026-07-24)** — **`CHANGELOG.md`** (riwayat rilis per codename v0.1→jalur v1.0, format Keep-a-Changelog, tiap milestone + marker), **`docs/tutorial.md`** (tutorial berurutan 8-langkah: build→keliling desktop→shell→bikin app→pakai library→debug/profil→bawa keluar QEMU, menautkan doc mendalam tiap bagian), **`docs/README.md`** (indeks navigasi seluruh docs berkelompok: mulai/menjalankan/referensi/perencanaan). **README di-refresh** (status usang "v0.1–v0.14" → "v0.1–v0.16 ✓ + jalur v1.0", boot log diperbarui s/d SECURITY/PROFILER OK, tautan tutorial+CHANGELOG+docs-index). Semua cross-link diverifikasi resolve (docs/README, tutorial, CHANGELOG, README — 0 broken), perintah tutorial (`run calc`/`editor`, `prof self`, `vm create/start nanovm`, `bz model list`) diverifikasi ada di kernel. **Lisensi kini DITETAPKAN: MIT** (berkas `LICENSE`, © 2026 Gravicode Studios — atas permintaan pemilik 2026-07-24; CHANGELOG/README/PLAN diperbarui). Tak ada kode berubah (build/smoke tak terpengaruh)
- [~] **Boot hardware nyata — perkakas & dokumentasi SIAP ✓ (2026-07-24), validasi mesin fisik menyusul.** Image build (`buitenzorg-bios.img` MBR / `buitenzorg-uefi.img` GPT+ESP) memang raw & langsung ditulis ke USB. Skrip flash **`scripts/flash-usb.ps1`** (Windows, akses disk mentah via PhysicalDrive) + **`.sh`** (Linux/macOS `dd`): pengaman berlapis (hanya disk USB/removable ditawarkan, target eksplisit bukan ditebak, disk sistem/boot DITOLAK mentah, konfirmasi ketik ERASE, **verifikasi baca-ulang** byte-per-byte) + mode `-List`/`--list`. Panduan **`docs/install-hardware.md`**: pilih firmware (bios↔Legacy/CSM, uefi↔UEFI, Secure Boot OFF), jalur GUI (Etcher/Rufus DD-mode) sbg alternatif, boot menu, **checklist verifikasi HW jujur** (PS/2 vs USB-HID, IDE-PIO vs NVMe, PIC/PIT vs APIC, ACPI shutdown) + tabel kompatibilitas + troubleshooting. Di-cross-link dari README + getting-started. `.ps1` parse-clean, `.sh` `bash -n` OK, enumerasi disk diuji (disk sistem SATA ke-0 benar ter-flag `IsSystem/IsBoot` → ditolak guard). **Belum: boot di mesin fisik nyata** (tak bisa dari lingkungan ini) — ditandai eksperimental
- [x] **Image resmi Hyper-V (VHDX) ✓ (2026-07-24)** — `make-vm-images.ps1`/`.sh` kini juga emit `dist/buitenzorg.vhdx` untuk Hyper-V. **Gotcha nyata ditangani:** konversi `qemu-img -O vhdx` telanjang menghasilkan virtual-size 5,47 MiB ganjil (bisa ditolak Hyper-V), DAN qemu build ini tak bisa `resize` vhdx ("format driver does not support resize") → solusi: **pre-create VHDX 64 MiB lalu `convert -n`** (stream raw ke disk yang sudah berukuran). Verified: virtual-size 67108864 B (64 MiB, 1MiB-aligned), dinamis 7 MiB di disk, konten prefix identik dgn source (convert-back + cmp). Helper baru **`scripts/make-hyperv-vm.ps1`** membuat VM **Generation 1** (BIOS/MBR — Gen 2 UEFI tak cocok disk MBR unsigned) dari VHDX; guarded (cek modul Hyper-V + admin), fallback cetak langkah manual kalau Hyper-V mati (diverifikasi: New-VM absen di lingkungan dev → fallback jalan bersih). Docs `run-in-vm.md` dapat bagian Hyper-V + troubleshooting. **Belum: boot di Hyper-V nyata** (feature tak aktif di lingkungan dev) — konversi VHDX + pembuatan VM terverifikasi, boot aktual menyusul spt VMware/VBox

### 🌍 Multi-Arch (v1.x) — rencana lengkap: [PLAN.md §v1.x](PLAN.md)
- [x] **Audit keterikatan arsitektur (2026-11-01):** 24/47 modul kernel netral (WM, VFS, FAT, shell, interpreter, AI, VMM, net, pkg, process, wallpaper/ikon, …); 23 terikat x86 — inti CPU (gdt, interrupts, usermode, task asm, smp, paging di memory/aspace/elf/allocator, RDTSC), perangkat PC berbasis port I/O (serial, ata, ps/2, pci CF8, rtc CMOS, power, ac97), boot (`bootloader` 0.11 x86-only). ABI syscall + mirror C# + format disk/aset sudah netral (little-endian)
- [x] **M0 — Ekstraksi HAL — increment 1 ✓ (2026-11-01):** kode inti CPU dipindah ke `src/arch/x86/` (gdt, interrupts, usermode, smp, vmx) dan perangkat PC port I/O ke `src/platform/pc/` (serial, ata, mouse, pci, rtc, power, audio); fasad `arch::cpu` (halt, interrupt on/off, `without_interrupts`, timestamp, flush TLB) dipakai modul generik (keyboard, ipc, framebuffer, syscall, profile, task, main, memory/aspace/elf); **`scripts/check-arch-boundary.sh` di CI** menolak `x86_64::`/`asm!`/port I/O/MSR di luar `arch/`+`platform/` (kontrol negatif diuji) dengan allowlist yang hanya bisa menyusut. Smoke 4-media + UEFI lolos tanpa perubahan perilaku. **Increment 2 ✓:** HAL context switch — `arch::context` (`FpuState`, `prepare_stack`, `switch`; asm FXSAVE + stack switch di `arch/x86/context.rs`), scheduler `task.rs` kini netral arsitektur (smoke penuh lolos: preemption 35, THREAD/SYNC/MULTIPROC/PROCCTL OK). **Increment 3 ✓:** HAL paging — `arch::paging` (`map`/`unmap`/`protect`/`is_mapped` dengan `Flags { write, user, exec }` netral, `user_access` untuk validasi pointer syscall, `PageMap`, `ELF_MACHINE`); `memory.rs` jadi kebijakan generik (alokator frame `alloc()/free()` dengan frame `u64`, arena mmap, validasi), `elf`/`allocator`/`main` lewat `MemCtx` (dipasang sebelum heap), `aspace` pindah ke `arch/x86/` (model PDPT-swap memang khas x86). **Allowlist pemeriksa batas kini kosong** — tidak ada modul generik yang menyentuh detail arsitektur. Smoke 4-media + UEFI lolos (89 milestone IDE, READY ~21 s)
- [x] **M1 — Boot multi-arch:** **increment 1 ✓ (2026-11-01):** abstraksi `boot::BootInfo` netral-protokol (peta memori, framebuffer, RSDP/DTB, offset peta fisik) — `bootloader_api` hanya di `arch/x86/boot.rs`, pemeriksa batas CI menolaknya di modul generik; parser Device Tree `dtb.rs` (penemuan RAM/CPU/UART/GIC-PLIC/timer/virtio-mmio/PSCI) diuji dengan DTB asli QEMU `virt` aarch64 + riscv64, blob rusak ditolak (`DTB OK`). **increment 2 ✓:** kernel yang sama di-boot lewat **protokol Limine** (base revision 6; request di `arch/x86/limine.rs`, kernel di higher half, tanpa identity map) dari image `buitenzorg-limine-uefi.img` (MBR + partisi FAT berisi Limine, kernel, semua file /disk); binary Limine v12.9.3 diunduh dengan SHA-256 terpin (`scripts/fetch-limine.*`); langkah smoke `[limine]` boot sampai READY di 2 CPU dengan /disk + proses — set milestone identik dengan boot IDE. Belum: Limine di BIOS lama (BIOS tetap lewat crate bootloader)
- [x] **M2 — Driver virtio ✓ (2026-11-01):** inti virtio generik (`src/virtio/`: virtqueue split, negosiasi fitur) di atas transport **virtio-PCI modern**; **blk** (seluruh OS boot dari disk virtio, tulisan /disk sampai ke file image), **input** (keyboard + tablet lewat QEMU monitor), **net** (menggantikan loopback: ARP + ping ke gateway 10.0.2.2 lewat kabel virtual; lalu lintas ke IP sendiri tetap lokal), **gpu** (2D, desktop dicerminkan identik piksel dengan layar VGA), **sound** (nada uji 48 kHz S16 stereo, semua periode OK). Langkah smoke `[virtio]` dengan kelima perangkat. Batas jujur: masih polling (belum interrupt/MSI-X); transport virtio-mmio menyusul di M3; syscall AUDIO_* masih ke AC'97; GPU masih mode cermin di samping VGA
- [x] **M3 — Port ARM64** (QEMU `virt`): PL011, GICv3, generic timer, MMU TTBR0/TTBR1, EL0/EL1 + SVC, PSCI; C# via bflat `linux-arm64` + shim aarch64 — **increment 1 ✓ (2026-11-01):** crate kernel yang sama dikompilasi untuk `aarch64-unknown-none-softfloat` (`main.rs` memilih `pc_main.rs`/`arm_main.rs`), di-boot Limine (`BOOTAA64.EFI`) di QEMU `virt` + edk2; vektor exception EL1, DTB hidup dibaca (PL011, GICv3, 32 slot virtio-mmio), MMIO dipetakan di tabel TTBR1, GICv3 + timer virtual 100 Hz, konsol framebuffer + PL011 (`ARM64 BOOT OK`, smoke `[arm64]`, screenshot `docs/img/arm64-boot.png`). **increment 2 ✓:** `memory.rs` + `allocator.rs` generik berjalan di ARM64 di atas HAL paging AArch64 (TTBR1 kernel, TTBR0 user), heap 32 MiB dipetakan lewat paging, address space per proses = root TTBR0 sendiri (`ARM64 MEMORY OK`, `ARM64 ASPACE OK`, 0 frame bocor). **increment 3 ✓:** context switch thread kernel (state FP/SIMD per task) + program EL0 di address space sendiri, syscall lewat `SVC #0` (x8 = nomor, ABI sama dengan x86), EXIT kembali ke kernel, akses ke memori kernel hanya mengakhiri program itu (`ARM64 CONTEXT OK`, `ARM64 EL0 OK`). **increment 4 ✓:** program ELF (loader `elf.rs` generik) dari modul Limine; app C# pertama di ARM64 — bflat `linux-arm64` + shim `bzstart.rs` versi aarch64 (`svc #0`, write barrier NativeAOT arm64): `Hello from C#!`, `MATANG OK`, `HEAP OK`, `GCMEM OK`, semua keluar 0 tanpa frame bocor (`ARM64 CSHARP OK`). Belum: `bcl.cs` (codegen arm64 butuh `ThrowOverflowException` yang tak ada di zerolib). **increment 5 ✓:** scheduler `task.rs` + registry `process.rs` dipakai tanpa perubahan di ARM64 (shim tipis `usermode`/`gdt`/`smp`/`interrupts`; frame exception kini menyimpan ELR/SPSR/SP_EL0); program jalan sebagai proses preemptive — dua `mproc` berbagi CPU lewat 197 preemption dengan state FP dan memori masing-masing (`ARM64 MULTIPROC OK`, kontrol negatif gagal sesuai harapan), thread ring-3 + futex (`THREAD OK`, `SYNC OK`), 0 frame bocor. **increment 6 ✓:** transport virtio-mmio v2 (slot dari DTB) — driver virtio yang sama jalan di ARM64: `/disk` dari virtio-blk lewat FAT + VFS bersama (baca/tulis, program dimuat dari `/disk`), jam PL031 (`ARM64 RTC OK`), virtio-gpu (`VIRTIO GPU OK`) — `ARM64 DISK OK`. Catatan: dengan virtio-gpu, konsol Limine perlu di-present lewat GPU (menyusul bersama desktop). **increment 7 ✓:** seluruh kode generik kernel PC (syscall, app, window manager, shell, jaringan, paket, VMM, skrip, AI) dikompilasi untuk ARM64 tanpa diubah; platform ARM: PSCI (`ARM64 PSCI OK`), pointer virtio, audio absen; `SECURITY OK`, `NETWORK OK`, `VIRTIO NET OK`; app C# GUI (Kalkulator, 2048, UI, Drawing, XOX) + `BCL OK` + `PROCCTL OK` di ARM64 — zerolib arm64 ditambal (`tools/ZerolibPatch`, `ThrowOverflowException`) — dan frame desktop tersusun (`ARM64 DESKTOP FRAME OK`, `docs/img/arm64-desktop.png`). **increment 8 ✓:** loop desktop dipindah ke modul bersama `desktop.rs`; desktop ARM64 kini interaktif — ketikan virtio-keyboard ke terminal (`ls /disk`), pointer + klik virtio-tablet, tampil lewat virtio-gpu (`VIRTIO INPUT OK`, `docs/img/arm64-desktop-live.png`). **increment 9 ✓ — M3 selesai:** SMP ARM64 — CPU sekunder dinyalakan lewat PSCI CPU_ON (trampolin MMU-mati + identity map sementara), ID CPU di TPIDR_EL1, redistributor GIC + timer per CPU, scheduler bersama; proses berjalan paralel di semua core (4 `mproc` di 4 CPU sekaligus, `ARM64 SMP OK`, `ARM64 SMP SCHED OK`; kontrol negatif gagal sesuai harapan)
- [x] **M4 — Port RISC-V** (QEMU `virt` + OpenSBI): 16550-MMIO, PLIC + timer SBI, Sv39/48, ECALL, SBI HSM; app awal lewat JS/TS/Python (NativeAOT RISC-V belum resmi di .NET). **Increment 1 ✓ (2026-11-02):** boot OpenSBI → edk2 → Limine → kernel S-mode (`riscv64imac-unknown-none-elf`), konsol SBI lalu NS16550 dari DTB hidup, vektor trap `stvec` (skema `sscratch`), memori generik + heap di atas HAL paging Sv48, address space per proses (setengah kernel dibagi lewat entri top-level), timer SBI 100 Hz — `RISCV MEMORY OK`, `RISCV ASPACE OK`, `RISCV TIMER OK`, `RISCV BOOT OK`. **Increment 2 ✓:** context switch (ra/s0-s11 + f0-f31/fcsr per task) + U-mode + `ECALL` (program U-mode mencetak, ditimer-interrupt di U-mode, memakai FPU, EXIT; baca alamat kernel → page fault → hanya program yang berhenti) — `RISCV CONTEXT OK`, `RISCV UMODE OK`; kontrol negatif (`sie.STIE` mati) gagal sesuai harapan. Smoke `[riscv64]`. **Increment 3 ✓:** program ELF RISC-V (Rust, `userland/riscv/`, dibangun otomatis oleh `bzimage/build.rs`, dimuat sebagai modul Limine) berjalan sebagai proses scheduler bersama (`task.rs`/`process.rs` tanpa perubahan) — preemptif, thread ring-3, futex, mmap/mprotect/munmap: `RISCV ELF OK`, `RISCV USERMEM OK`, `RISCV MULTIPROC OK` (143 preemption U-mode, jumlah FP tepat), `RISCV THREAD OK`, `RISCV SYNC OK`, `RISCV PROCESSES OK` (0 frame bocor). Kontrol negatif: preemption U-mode mati → 0 preemption, tanpa milestone; FP tidak disimpan saat switch → jumlah salah. **Increment 4 ✓:** PLIC (konteks S-mode hart dari `interrupts-extended` DTB; interrupt THRE UART lewat PLIC → `RISCV PLIC OK`, kontrol negatif tanpa bit enable → 0 interrupt), Goldfish RTC (`RISCV RTC OK`), virtio-mmio: `/disk` dari virtio-blk lewat FAT + VFS bersama, baca + tulis + baca ulang (`RISCV DISK OK`, byte-nya ditemukan di file image), program user dimuat dari `/disk`, virtio-gpu (`VIRTIO GPU OK`). **Increment 5 ✓:** seluruh modul generik kernel PC ikut di RISC-V tanpa perubahan kode (`syscall.rs` asli, `app.rs`, window system, shell, net, pkg, VMM, `script.rs`); power lewat SBI SRST (`RISCV POWER OK`), virtio-net (`VIRTIO NET OK`), JS/TS/Python (`RISCV POLYGLOT OK`), app GUI Rust (`RISCV GUI OK`), spawn/wait + file (`RISCV PROCCTL OK`), frame desktop (`RISCV DESKTOP FRAME OK`) dan desktop interaktif lewat virtio keyboard/tablet (`VIRTIO INPUT OK`). Bug laten virtio diperbaiki: used ring 256 entri meluap 6 byte ke frame fisik berikutnya (semua arsitektur). **Increment 6 ✓ — M4 selesai:** SMP lewat SBI HSM (`hart_start` + trampolin paging-mati + root identity sementara), timer SBI + konteks PLIC per hart, TLB shootdown lewat SBI RFENCE, IPI bangun lewat SBI IPI; `RISCV SMP OK` + `RISCV SMP SCHED OK` (paralel 2 hart / 4 hart, hasil FP tepat); kontrol negatif (scheduling AP mati) → paralel 1, tanpa milestone. Smoke `[riscv64]` dengan `-smp 2`.
- [x] **M5 — CI matrix** x86_64 / aarch64 / riscv64 dengan smoke test marker yang sama — **✓ 2026-11-02:** job `build` (3 kernel + semua image → artifact) lalu matrix `smoke` per arsitektur (QEMU + firmware masing-masing, `SMOKE_ARCH` + `SMOKE_STRICT=1` sehingga langkah yang ter-skip = gagal, log serial di-upload walau gagal); boot UEFI + Limine x86 kini benar-benar jalan di CI (dulu ter-skip). Sekaligus memperbaiki `RISCV UMODE OK` yang flaky di runner GitHub sejak M4 increment 1 (spin berbasis waktu `rdtime`).
- [ ] SMP/NUMA matang · marketplace · container lanjut

### 🍓 Raspberry Pi 4 & 5 (v1.x "Pucuk") — rencana lengkap: [PLAN.md](PLAN.md)
Boot lewat UEFI komunitas (edk2) + Limine seperti di QEMU; perbedaan utama dari port ARM64 `virt`: GICv2 (GIC-400), driver perangkat keras nyata (bukan virtio), uji di papan fisik.
- [x] **R1 — Boot Pi 4 ke konsol** — **terverifikasi di Raspberry Pi 4 Model B fisik (8 GB) lewat UART dan di QEMU `raspi4b` (2026-11-02).** Boot pertama di papan menemukan bug yang tidak terlihat di QEMU: program user mode mati dengan "undefined instruction" karena cache instruksi tidak disinkronkan setelah kode ditulis -- kini `arch::cpu::sync_code` (`dc cvau` + `ic ialluis`) untuk setiap halaman executable di `elf.rs`; boot kedua lolos. Log: [docs/logs/rpi4-first-boot.txt](docs/logs/rpi4-first-boot.txt). `kernel8.img` (entri langsung `arch/aarch64/rpi.rs`: relokasi ke 2 MiB, EL2→EL1, tabel halaman + MMU sendiri, peta memori dari DTB tanpa heap), driver **GICv2** (`gic.rs` v2 + v3), PL011 dari DTB, translasi `ranges` di `dtb.rs` (`0x7e201000` → `0xfe201000`), image SD `buitenzorg-rpi4.img` (`config.txt` memilih rute; bawaan: langsung; UEFI + Limine belum pernah berjalan), smoke `[rpi4]` → `RPI4 BOOT OK`. Panduan: [docs/install-raspberry-pi.md](docs/install-raspberry-pi.md)
- [x] **R2 — Desktop di layar Pi 4** — **terverifikasi di Pi 4 fisik (desktop tampil di HDMI) dan QEMU `raspi4b` (2026-11-02).** Tetap di rute langsung (keputusan R2): framebuffer 1280x720 lewat **mailbox VideoCore** (kanal properti 8, sebelum `kernel_main`, dipetakan Normal non-cacheable di jendela sendiri, diserahkan lewat `BootInfo` seperti GOP UEFI -- konsol, WM dan desktop tanpa kode khusus Pi), dan **4 core lewat spin table** (`cpu-release-addr` dari DTB; stub `bz_ap_spin_entry` turun EL2→EL1 lalu memakai trampoline AP yang sama) → `ARM64 SMP OK (4 CPUs: spin tables …)`. Input sementara lewat **serial** (UART RX → antrean keyboard, urutan escape VT100 untuk panah dkk; diuji di papan: `cpu`, `ps`, `ver` diketik dari PC muncul di terminal desktop HDMI). Belum: USB keyboard/mouse (R4), wallpaper/ikon dari kartu SD (R3). Log: [docs/logs/rpi4-r2-boot.txt](docs/logs/rpi4-r2-boot.txt)
- [x] **R3 — Penyimpanan Pi 4** — **terverifikasi di Pi 4 fisik (kartu 32 GB) dan QEMU `raspi4b` (2026-11-02).** Driver `platform/virt/sdhci.rs`: SDHCI v3, akses register 32-bit saja (kebiasaan Broadcom), PIO per blok 512 byte tanpa DMA/interupsi; inisialisasi kartu CMD0 → CMD8 → ACMD41 (SDHC) → CMD2/3/9 (kapasitas dari CSD) → CMD7 → ACMD6 (bus 4-bit) → 25 MHz. Clock dasar EMMC2 dari mailbox (tag 0x00030002, id 12: 100 MHz di papan), jika tidak ada dari register capabilities. Urutan: `brcm,bcm2711-emmc2` dulu, lalu `brcm,bcm2835-sdhci` (QEMU memasang kartunya di sana). `/disk` FAT32 rw dari kartu: wallpaper, 15 ikon, program C#, CLRHOST + app managed. Image kartu kini membawa semua file `/disk` dan berukuran 128 MiB (QEMU menuntut pangkat dua). `MILESTONE: SD CARD OK`, smoke `[rpi4]` memakai salinan image sebagai kartu. Ikut diperbaiki: direktori root FAT32 yang penuh kini bertambah satu cluster (dulu "root directory is full"; app Catatan gagal menyimpan di kartu Pi).
- [ ] **R4 — Input + jaringan Pi 4**: PCIe BCM2711 + xHCI (VL805) + USB HID keyboard/mouse; Ethernet GENET
- [ ] **R5 — Raspberry Pi 5**: image `buitenzorg-rpi5.img` (EEPROM → rpi5-uefi → Limine), Cortex-A76, **RP1** lewat PCIe (UART/USB/Ethernet GEM), SDHCI, RTC bawaan, NVMe PCIe
- [ ] **R6 — Matang di perangkat nyata**: GPIO/I2C/SPI (selaras `rustclr-hal`), audio PWM/HDMI, power via firmware, `docs/install-raspberry-pi.md`, runner CI self-hosted dengan papan Pi

### 🧪 Eksperimen pasca-v1.0 — link CoreLib .NET ASLI (`--stdlib:dotnet`)
Proyek riset tersendiri (multi-sesi, hasil tak pasti); ganti Buitenzorg.Bcl
tulisan-sendiri dgn BCL resmi (LINQ/Regex/Tasks/StringBuilder). Jalur dipetakan
di v0.15 inc 6 (lihat PLAN.md §Eksperimen pasca-v1.0):
- [ ] Link statik-freestanding (bukan dinamis-glibc default) + crt/_start sendiri
- [ ] Glue entry internal bflat (`__managed__Startup`/`RhInitialize`)
- [ ] Sisa ~150 simbol PAL (stdio/TLS `__tls_get_addr`/pthread_key/`LowLevelMonitor_*`/new-delete C++) — banyak bisa stub; inti (mmap/thread/futex/clock/malloc) SUDAH dari v0.15
- [ ] Link `libbootstrapperdll.o`+`libRuntime.WorkstationGC.a` lalu tembus fault startup GC/EEType/cctor
