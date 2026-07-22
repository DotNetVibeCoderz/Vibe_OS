# PLAN.md — Roadmap Pengembangan Produk Buitenzorg OS

> Roadmap produk berorientasi versi. Sumber desain teknis: [requirements.md](requirements.md).
> Status tracking detail per-fitur: [Progress.md](Progress.md).
>
> Codename versi mengikuti pertumbuhan tanaman (penghormatan Kebun Raya Bogor):
> benih → akar → batang → tunas → dahan → daun → kanopi → kembang → serbuk →
> buah → cahaya → nalar → lapis → babel → panen → rilis → rimba.
>
> **Dibuat oleh Gravicode Studios — dipimpin oleh Kang Fadhil.**

---

## Ringkasan Status

| Fase | Versi | Codename | Fokus | Status |
|---|---|---|---|---|
| Fondasi | v0.1 | Benih | Bootloader + kernel minimal | ✅ Selesai |
| Fondasi | v0.2 | Akar | Kernel core (memori, scheduler, syscall, IPC) | ✅ Selesai |
| Fondasi | v0.3 | Batang | Driver + storage + boot 4 media | ✅ Selesai |
| Runtime | v0.4 | Tunas | Managed runtime bring-up (C# di ring 3) | ✅ Selesai |
| Sistem | v0.5 | Dahan | VFS, service manager, async I/O, networking | ✅ Selesai |
| UI | v0.6 | Daun | Compositor + window manager | ✅ Selesai |
| UI | v0.7 | Kanopi | Desktop environment, terminal, tema, workspace | ✅ Selesai |
| App | v0.8 | Kembang | App framework + SDK + window syscall | ✅ Selesai |
| App | v0.9 | Serbuk | Web/Widget app, System.Drawing, Task Manager | ✅ Selesai |
| App | v0.10 | Buah | Theme engine, 8 tema, package manager | ✅ Selesai |
| GPU/UX | v0.11 | Cahaya | GPU compute + desktop polish (screensaver, personalisasi, micro-interaction, window controls) | ✅ Selesai |
| AI/Power | v0.12 | Nalar | Subsistem AI-native + Hugging Face; power (shutdown/restart/sleep) | ✅ Selesai |
| **VM** | **v0.13** | **Lapis** | **Virtualization (hypervisor tipe-2)** | 🔜 **Berikutnya** |
| VM | v0.13 | Lapis | Virtualization (hypervisor tipe-2) | ⏳ Rencana |
| Polyglot | v0.14 | Babel | JS/TS + Python runtime | ⏳ Rencana |
| Rilis | v0.15 | Panen | Preloaded suite + optimization pass | ⏳ Rencana |
| Rilis | v1.0 | Buitenzorg | Stable release x86-64 | ⏳ Rencana |
| Multi-arch | v1.x | Rimba | ARM64 + RISC-V | ⏳ Rencana |

Legend: ✅ selesai · 🔜 sedang/berikutnya · ⏳ direncanakan

---

## Sudah Tercapai (v0.1 – v0.12)

Kernel Rust boot dari BIOS & UEFI di QEMU pada 4 media (IDE/AHCI/NVMe/USB),
dengan: manajemen memori + paging + heap, scheduler kooperatif (preemptive
opsional), IPC, syscall ABI, driver IDE + FAT (read/write), mouse & keyboard
PS/2, VFS, service/init manager, async I/O io_uring-style, stack jaringan
(Ethernet/ARP/IPv4/ICMP loopback). Di atasnya: runtime C# NativeAOT berjalan di
ring 3, desktop environment penuh (compositor, window manager, terminal +
shell, tema dark/light, 4 virtual desktop), app framework, dan (v0.9)
**keempat varian app** (console/desktop/web/widget), library grafik
**`Buitenzorg.Drawing`** bergaya System.Drawing, serta **Task Manager**
(daftar proses + sumber daya + kill). Detail per-fitur: [Progress.md](Progress.md).

### Catatan v0.9 "Serbuk" (selesai)
- **Buitenzorg.Drawing** (`bzdraw.cs`): `Graphics`/`Pen`/`Brush`/`Color`/`Point`/
  `Rectangle`, bentuk (line/rect/ellipse fill+outline), `DrawString`/`DrawChars`.
  Draw op baru di window ABI (LINE/ELLIPSE/FILL_ELLIPSE/RECT). Demo: app Paint.
- **Task Manager** (`taskmgr.cs`): registry proses kernel (`process.rs`) +
  akuntansi CPU-time, ABI `PROC_LIST`/`PROC_KILL`/`SYS_STAT`, UI daftar proses +
  uptime/heap/RAM + bar, dan kill task (demo: kill `idle-demo`).
- **Varian app**: Widget (`widget.cs`, ter-dock di widget board) + WebView
  (`webview.cs`, mini renderer subset HTML) melengkapi console/desktop.
- *Belum (backlog)*: engine HTML/CSS/JS penuh, `Bitmap`/`GraphicsPath`/`Font`
  di Drawing, tab terpisah + Details di Task Manager, template SDK web/widget,
  kill app lain (butuh multi-proses).

### Catatan v0.10 "Buah" (selesai)
- **Theme engine** (`theme.rs`): design token warna + style (border thickness,
  title flat/beveled/tab, shadow none/soft/hard-offset, desktop flat/gradient).
  Compositor merender per style. **8 tema** (Neo Brutalism, Clean, Material,
  Bento, Classic Linux, Classic Windows, Sun, BeOS) + dark/light; live switch
  via `theme <nama|cycle|list>`.
- **Package manager** (`pkg.rs`): registry paket + `bz install/remove/list/
  search`; `run` di-gate oleh status terpasang.
- *Belum (backlog)*: `.theme` package + theme SDK, hot-reload dari file, icon
  pack, sandbox/dependency, app store GUI, update paket.

---

## 🍎 v0.10 — "Buah" · Theme Engine, 8 Style & Package Manager ✅
Theme engine (design tokens + style), 8 tema bawaan + dark/light, package
manager (install/remove/list) + app registry. *(Tercapai — `.theme` kustom,
hot-reload, icon pack, sandbox/dependency menyusul.)*
**Milestone:** install app dari registry + ganti antar 8 tema live. ✅

## ⚡ v0.11 — "Cahaya" · GPU Compute & Desktop Polish ✅

Selesai: compute API (backend CPU, interface siap GPU) + penyempurnaan desktop
(screensaver, personalisasi, micro-interaction, kontrol window + rounded).
Driver GPU hardware nyata masih backlog (interface sudah siap).

### Catatan v0.11 "Cahaya" (selesai)
- **Compute API** (`compute.rs`): SAXPY/blend, backend CPU (SIMD-friendly),
  enum Backend Cpu/Gpu (GPU menyusul). "compute API siap untuk AI (v0.12)".
- **Screensaver** (`screensaver.rs`): 6 saver gaya Win 3.1/98 (Starfield,
  Mystify, 3D Pipes, Marquee, Bouncing, Blank); idle ~12s, dismiss on input.
- **Personalization**: `wallpaper.rs` (bawaan + BMP gambar user dari VFS),
  shell `settings/bg/saver/cursor/anim/theme`; kursor besar/normal.
- **Micro-interactions** (`wm.rs`): hover highlight tombol, click ripple
  beranimasi, loop desktop kontinu; `anim off` = reduce motion.
- **Kontrol window**: min/max/close di tiap window + sudut membulat + state
  normal/minimized/maximized + restore dari taskbar.
- **Bugfix laten**: `enter_user` re-enable interrupts setelah app ring-3 keluar
  (IF dulu tetap mati → timer mati di desktop loop → desktop interaktif rusak
  sejak v0.4).
- *Belum (backlog)*: driver GPU hardware + compositor GPU + zero-copy,
  animasi buka/tutup window, app Personalization GUI, simpan preferensi ke VFS,
  display resolusi/scaling.

### 11.x Rincian (ref)

### 11.1 Akselerasi GPU
- Driver GPU (Rust) + HAL grafis; compositor akselerasi GPU (animasi, transparansi).
- Compute API (abstraksi Vulkan/WebGPU-style) untuk workload paralel (dipakai AI v0.12).
- Fallback CPU (SIMD) bila GPU tak tersedia; zero-copy antara app ↔ GPU memory.

### 11.2 Screen Saver
- **Framework screensaver**: aktif setelah idle (tak ada input mouse/keyboard) sesuai timeout; nonaktif saat ada input.
- **Screensaver bawaan bergaya klasik** (Windows 3.1 / 98):
  - *Starfield / Flying Through Space* — bintang melesat dari titik pusat.
  - *Mystify* — poligon garis memantul warna-warni.
  - *3D Pipes* — pipa tumbuh 3D-ish mengisi layar.
  - *Marquee* — teks bergulir (scrolling text).
  - *Bouncing / Beziers* — kurva/objek memantul.
  - *Blank / Fade* — layar gelap sederhana.
- Idle detection di kompositor; screensaver di-render sebagai overlay full-screen.
- Konfigurasi: pilih screensaver, timeout, preview (via app Personalization).

### 11.3 Personalization & Display Settings (app)
App pengaturan untuk mengkustomisasi tampilan:
- **Desktop background**: pilih dari **wallpaper bawaan** (beberapa gradient/pattern/gambar) *atau* **file gambar milik user** (mis. dari VFS `/disk`, `/ram`).
- **Screensaver**: pilih jenis + timeout + tombol preview.
- **Theme**: pilih dark/light + 8 tema (v0.10), accent color.
- **Kursor**: gaya & ukuran kursor (mis. classic/besar), efek kursor.
- **Display**: info/atur resolusi framebuffer, scaling, orientasi (bila didukung).
- **Opsi lain**: efek animasi on/off, transparansi window, sudut membulat on/off.
- Preferensi disimpan (VFS) agar persist antar-boot (bila FS write siap).

### 11.4 Micro-interactions (UI/UX)
Menjadikan UI terasa hidup & responsif:
- **Hover**: highlight tombol/kontrol window/taskbar saat kursor di atasnya.
- **Klik**: feedback tekan (ripple/bevel), animasi tombol.
- **Window**: animasi buka/tutup/minimize/maximize (scale/fade), bayangan dinamis.
- **Kursor**: efek (trail halus, ripple klik, animasi loading).
- **Transisi**: fade saat ganti tema, slide saat pindah workspace.
- **Taskbar**: hover preview, indikator window aktif beranimasi.
- Hormati opsi "reduce motion" (bisa dimatikan di Personalization).

### 11.5 Kontrol Window (rounded + minimize/maximize/close)
- **Tombol title bar** di setiap window app: **minimize** (ke taskbar), **maximize/restore** (isi workspace), **close**.
- **State window**: normal / minimized / maximized (klik taskbar untuk restore).
- **Sudut membulat** (rounded window corners) — dengan anti-alias bila memungkinkan.
- Interaksi mouse: klik tombol → aksi; double-click title bar → maximize/restore.

**Milestone v0.11:** desktop & animasi dipercepat GPU; compute API siap untuk AI;
screensaver aktif saat idle; app Personalization mengatur background/tema/
screensaver/kursor; window punya tombol minimize/maximize/close + sudut membulat;
micro-interaction (hover/klik/transisi) aktif.

## 🧠 v0.12 — "Nalar" · Subsistem AI-Native & Power Management ✅

Selesai: subsistem AI (LLM/CV/GenAI toy-scale nyata + Model Manager Hugging
Face-style) plus manajemen daya (Shutdown/Restart/Sleep, shutdown teruji QEMU).

### Catatan v0.12 "Nalar" (selesai)
- **AI System API** (`ai.rs`): `llm_complete` (model bigram char-level nyata,
  jalan di kernel/CPU), `vision_edges` (deteksi tepi Sobel), `genai_image`
  (text-to-image prosedural). Shell `ask <prompt>`.
- **Model Manager** (`model.rs`): galeri gaya Hugging Face (TinyLlama, phi-2,
  whisper, trocr, sd-turbo, + model bawaan) dengan metadata; `bz model
  list/pull/info`.
- **Power** (`power.rs`): parser ACPI (RSDP dari bootloader → FADT → PM1a_CNT,
  scan DSDT `\_S5`); Shutdown (ACPI + fallback QEMU port 0x604/0xB004, VBox —
  **teruji: QEMU power off**), Restart (ACPI reset + kbd-controller + triple-
  fault), Sleep (light: blank + `hlt` sampai input). Shell `shutdown/restart/
  sleep` + `bz power`.
- *Belum (backlog)*: LLM skala produksi (GGUF/ONNX + GPU/NPU), inference
  scheduler, CV/GenAI audio/video, download model nyata (multi-GB), sandbox
  model, ACPI S3, power menu GUI + konfirmasi, flush/save-state sebelum power.

### 12.1 Subsistem AI-Native (ref)
LLM engine lokal (GPU/NPU), computer vision, GenAI image/audio/video, inference
scheduler, Model Manager + galeri Hugging Face, AI System API. Memanfaatkan
compute API (v0.11). **Milestone:** LLM lokal jalan & unduh model; app memakai AI API.

### 12.2 Power Management (Shutdown / Restart / Sleep)
Kontrol daya sistem, dari UI (menu Start/power) dan CLI (`bz power ...`), plus
API syscall agar app bisa memicu.
- **Shutdown (Matikan)** — matikan mesin lewat **ACPI** (parse FADT → PM1a_CNT,
  DSDT → nilai SLP_TYP `\_S5`; tulis SLP_TYP|SLP_EN). Fallback QEMU (port 0x604 /
  0xB004) untuk dev/VM.
- **Restart (Mulai Ulang)** — reboot via **ACPI reset register**, fallback ke
  reset keyboard-controller (port 0x64 ← 0xFE), lalu triple-fault sebagai upaya
  terakhir.
- **Sleep (Tidur / Suspend)** — hemat daya: mulai dari *light sleep* (blank layar
  + `hlt` CPU sampai ada input, mirip standby), lalu **ACPI S3** (suspend-to-RAM,
  simpan/restore konteks) sebagai target.
- **Flush sebelum daya berubah** — sinkronkan VFS/disk & beri sinyal app agar
  simpan state sebelum shutdown/restart/sleep.
- **UI**: item power di shell/taskbar/Start (Shutdown, Restart, Sleep) dengan
  konfirmasi; **CLI**: `bz power off|restart|sleep`.
- **Prasyarat**: parser ACPI minimal (RSDP → RSDT/XSDT → FADT), dan (untuk S3)
  penyimpanan konteks + wake vector.

**Milestone v0.12:** LLM lokal jalan & unduh model dari Hugging Face; app memakai
AI API; sistem bisa **Shutdown, Restart, dan Sleep** (dari UI & `bz power`),
teruji di QEMU.

## 📦 v0.13 — "Lapis" · Virtualization
Hypervisor tipe-2 (VT-x/AMD-V), virtualization manager, virtio drivers,
snapshot & virtual disk, guest tools. **Milestone:** jalankan OS lain sebagai VM.

## 🗣️ v0.14 — "Babel" · Polyglot App Support
JS/TS engine + transpile, Python (CPython/IronPython), binding API seragam,
template app JS/TS/Python. **Milestone:** app JS/TS/Python jalan berdampingan C#.

## 🌾 v0.15 — "Panen" · Preloaded Suite & Optimization Pass
Utilities, multimedia, AI apps, games, themes, productivity, store bawaan;
optimization pass (fast boot, fast I/O, startup, footprint, SIMD); benchmark
regresi di CI. **Milestone:** OS ringan & cepat, siap pakai.

## 🏛️ v1.0 — "Buitenzorg" · Stable Release (x86-64)
Stabilkan API & ABI, security hardening, dokumentasi + tutorial, debugger +
profiler, instalasi ke hardware nyata, image resmi VMware/QEMU/Hyper-V/VirtualBox.
**Milestone:** rilis stabil.

## 🌍 v1.x — "Rimba" · Multi-Arch
Port ARM64 lalu RISC-V (via HAL), optimasi per-arsitektur.
**Milestone:** boot & jalan di ARM64 dan RISC-V.

## 🔮 Pasca-1.x (jangka panjang)
SMP/NUMA matang · driver GPU modern + NPU · WiFi/Bluetooth/USB luas ·
container & sandboxing lanjut · marketplace app/tema/game/model AI ·
GenAI video/audio real-time.

---

## Utang Teknis Lintas-Versi (backlog)

Item yang belum tuntas dan dijadwalkan menyusul (lihat anotasi di [Progress.md](Progress.md)):

- **CoreCLR/JIT + GC + reflection** — runtime C# masih NativeAOT/bflat freestanding (tanpa GC/heap). Loncatan Layer-4 "Menengah" (§5.1).
- **APIC** — masih PIC 8259 legacy; migrasi ke APIC untuk SMP.
- **Driver native NVMe/AHCI/USB & e1000** — saat ini hanya IDE PIO + net loopback.
- **Filesystem kustom journaled** — baru FAT12/16/32 read + FAT12 write.
- **TCP/UDP** — baru Ethernet/ARP/IPv4/ICMP.
- **Multi-proses preemptive di ring 3** — model saat ini satu app run-to-completion; task manager & multitasking penuh butuh ini.
- **Tiling WM, launcher/tray/settings/notifikasi, pipes/redirection shell**.
- **VS Code DAP debugging + debug bridge** — extension baru skeleton.
- **Benchmark regresi di CI**.

---

## Urutan & Ketergantungan Kritis

1. **GPU (v0.11) sebelum AI (v0.12)** — AI butuh compute API + fallback CPU.
2. **Runtime C# penuh (GC/CoreCLR)** membuka library kaya (`Buitenzorg.Drawing`
   penuh, task manager kaya, BCL) — dikerjakan bertahap dari v0.9.
3. **Multi-proses** adalah prasyarat task manager yang benar-benar "kill proses
   lain"; versi awal (v0.9) memantau task kernel + app tunggal, disempurnakan
   saat multi-proses matang.
4. **HAL multi-arch** disiapkan sejak awal; port ARM64/RISC-V setelah stabil x86-64.
