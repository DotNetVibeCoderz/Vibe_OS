# PLAN.md — Roadmap Pengembangan Produk Buitenzorg OS

> Roadmap produk berorientasi versi. Sumber desain teknis: [requirements.md](requirements.md).
> Status tracking detail per-fitur: [Progress.md](Progress.md).
>
> Codename versi mengikuti pertumbuhan tanaman (penghormatan Kebun Raya Bogor):
> benih → akar → batang → tunas → dahan → daun → kanopi → kembang → serbuk →
> buah → cahaya → nalar → lapis → babel → matang → panen → rilis → rimba.
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
| VM | v0.13 | Lapis | Virtualization (VMM tipe-2 + virtio + snapshot) | ✅ Selesai |
| Polyglot | v0.14 | Babel | JS/TS + Python runtime (interpreter bersama) | ✅ Selesai |
| Runtime | v0.15 | Matang | Managed runtime C#: PAL + heap + model memori GC + Buitenzorg.Bcl | 🟡 Sebagian (GC reklamasi, async/await, EH belum) |
| Rilis | v0.16 | Panen | Drawing + UI + audio + preloaded suite 8 app + optimization pass | ✅ Selesai |
| **Rilis** | **v1.0** | **Buitenzorg** | **Stable release x86-64** | 🔜 **Sedang berjalan** |
| DevEx | pra/pasca-v1.0 | — | Developer Workstation: IDE, terminal, package manager, debug & deploy remote | ⏳ Rencana |
| Media | pasca-v1.0 | — | Multimedia, internet, game 2D/3D & hiburan | ⏳ Rencana |
| Multi-arch | v1.x | Rimba | ARM64 + RISC-V | ⏳ Rencana |

Legend: ✅ selesai · 🟡 sebagian · 🔜 sedang/berikutnya · ⏳ direncanakan *(status dirapikan 2026-11-01)*

---

## Sudah Tercapai (v0.1 – v0.16)

Kernel Rust boot dari BIOS & UEFI di QEMU pada 4 media (IDE/AHCI/NVMe/USB),
dengan: manajemen memori + paging + heap, scheduler kooperatif (preemptive
opsional), IPC, syscall ABI, driver IDE + FAT (read/write), mouse & keyboard
PS/2, VFS, service/init manager, async I/O io_uring-style, stack jaringan
(Ethernet/ARP/IPv4/ICMP loopback). Di atasnya: runtime C# NativeAOT berjalan di
ring 3, desktop environment penuh (compositor, window manager, terminal +
shell, tema dark/light, 4 virtual desktop), app framework, dan (v0.9)
**keempat varian app** (console/desktop/web/widget), library grafik
**`Buitenzorg.Drawing`** bergaya System.Drawing, serta **Task Manager**
(daftar proses + sumber daya + kill). v0.15–v0.16 menambah heap managed +
thread + futex + **Buitenzorg.Bcl**, renderer `Buitenzorg.Drawing`, toolkit
`Buitenzorg.UI`, audio AC'97, desktop shell (Start menu/ikon/tray), dan suite 8
app. Detail per-fitur: [Progress.md](Progress.md).

### Catatan v0.9 "Serbuk" (selesai)
- **Buitenzorg.Drawing** (`bzdraw.cs`): `Graphics`/`Pen`/`Brush`/`Color`/`Point`/
  `Rectangle`, bentuk (line/rect/ellipse fill+outline), `DrawString`/`DrawChars`.
  Draw op baru di window ABI (LINE/ELLIPSE/FILL_ELLIPSE/RECT). Demo: app Paint.
- **Task Manager** (`taskmgr.cs`): registry proses kernel (`process.rs`) +
  akuntansi CPU-time, ABI `PROC_LIST`/`PROC_KILL`/`SYS_STAT`, UI daftar proses +
  uptime/heap/RAM + bar, dan kill task (demo: kill `idle-demo`).
- **Varian app**: Widget (`widget.cs`, ter-dock di widget board) + WebView
  (`webview.cs`, mini renderer subset HTML) melengkapi console/desktop.
- *Belum (backlog)*: engine HTML/CSS/JS penuh, tab terpisah + Details di Task
  Manager, template SDK web/widget, kill app lain (butuh multi-proses).
  *(`Bitmap`/`GraphicsPath`/`Font` sudah ada di `bzgfx.cs` sejak v0.16.)*

### Catatan v0.10 "Buah" (selesai)
- **Theme engine** (`theme.rs`): design token warna + style (border thickness,
  title flat/beveled/tab, shadow none/soft/hard-offset, desktop flat/gradient).
  Compositor merender per style. **8 tema** (Neo Brutalism, Clean, Material,
  Bento, Classic Linux, Classic Windows, Sun, BeOS) + dark/light; live switch
  via `theme <nama|cycle|list>`.
- **Package manager** (`pkg.rs`): registry paket + `bz install/remove/list/
  search`; `run` di-gate oleh status terpasang.
- *Belum (backlog)*: `.theme` package + theme SDK, hot-reload dari file, icon
  pack, sandbox/dependency, update paket. *(App store GUI ✓ v0.16.)*

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

## 📦 v0.13 — "Lapis" · Virtualization ✅
Deteksi hardware VT-x/AMD-V (CPUID + MSR) dengan **fallback VMM software** —
sebuah **virtual CPU "BZVM"** (fetch/decode/execute, register, memori linier,
stack, port I/O virtio, virtual disk RAM, snapshot/restore penuh) yang
**benar-benar menjalankan guest OS mini "NanoOS"**. Virtualization Manager
(`vmm.rs`) + shell `vm list/create/start/snapshot/restore/remove` + `bz vm`,
`bz virt`. Driver native VMX/SVM (VMXON/EPT/VMCB) tetap backlog (nested HW tak
diekspos QEMU/TCG). **Milestone tercapai:** NanoOS boot, komputasi, cetak lewat
virtio console, baca host-tick via guest tools, lalu halt — snapshot terverifikasi.

## 🗣️ v0.14 — "Babel" · Polyglot App Support ✅
Runtime polyglot **di dalam OS** (`script.rs`): satu interpreter tree-walking
dengan AST/evaluator/**binding API seragam** (`print`/`console.log` → host
`emit` yang sama), front-end **JavaScript**, **TypeScript** (transpile nyata:
strip `interface`/`type`/anotasi `: Tipe` → JS), dan **Python** (subset,
indentasi). Fungsi+rekursi, if/else, while, for (C-style & `range()`), operator,
konkatenasi string. Shell `script <lang> [file]` + `js/ts/py` + `bz script`;
template app `sdk/templates/{js,ts,python}-app`. **Milestone tercapai:** program
sama (fib) di JS/TS/Python jalan berdampingan app C# — semua cetak fib(10)=55.
*(Engine V8/CPython penuh & app polyglot ring-3 native = backlog CoreCLR/GC.)*

## 🧩 v0.15 — "Matang" · Managed Runtime C# Lengkap (GC + .NET BCL) — *sebagian besar tercapai*
> **Status 2026-11-01:** inc 1–7 + `bzbcl2.cs` (IO/Encoding/Regex/Globalization/
> Diagnostics/Net-UDP/Tasks/Timers) selesai & terverifikasi. **Sisa:** GC yang
> mereklamasi memori + GC statics, exception unwinding (try/catch), `async`/
> `await`, thread pool, TimeSpan/Guid/Interlocked, LINQ OrderBy/GroupBy/Distinct.
> **Increment 1 ✓:** fondasi **PAL memori** — syscall user `MMAP/MPROTECT/MUNMAP`
> (ABI 13–15, arena `memory.rs`), diverifikasi dari C# ring-3 (`matang.cs`).
>
> **Increment 2 ✓:** **thread ring-3 kooperatif** — syscall `THREAD_CREATE/JOIN/
> EXIT` (ABI 16–18). Tiap thread = kernel task dengan SYSCALL kernel stack
> sendiri (terpisah dari stack interrupt TSS, jadi tak ada tabrakan), konteks
> longjmp/user-rsp per-thread di-swap saat context switch; app non-thread tak
> terpengaruh. Diverifikasi dari C# (`thread.cs`: main+worker menaikkan counter
> bersama 2×1000 lalu join → `MILESTONE: THREAD OK`, lolos 4-media). Juga
> memperbaiki bug clobber register syscall (`r8/r9/r10`).
>
> **Increment 3 ✓:** **PAL sync + TLS + clock** — syscall `FUTEX_WAIT/WAKE`
> (ABI 19–20, state scheduler **Blocked** → blok sejati bukan busy-yield),
> `THREAD_SELF` (21, fondasi pthread_self/TLS), `CLOCK_MONO` (22, TSC). Shim
> `bz_mutex_lock/unlock` di atas futex. Diverifikasi dari C# (`sync.cs`: 2 worker
> rebutan mutex + uji **mutual-exclusion** sejati [stamp id di CS + yield] +
> thread-self benar + clock monotonik → `MILESTONE: SYNC OK`, lolos 4-media).
>
> **Increment 4 ✓:** **managed heap** — `new` / array / generic instance kini
> JALAN di ring-3 C# (zerolib `new`/`RhpNewArray` lewat `SystemNative_Malloc`,
> kini heap bump **tumbuh via mmap** + zeroed) + libc `malloc/free/calloc/realloc`
> (fondasi PAL CoreLib). Diverifikasi (`heap.cs`: `new int[100000]` [paksa
> pertumbuhan mmap] + linked-list objek + `Pair<T>` generic → `MILESTONE: HEAP
> OK`, lolos 4-media). *(Static reference fields masih belum — GC statics.)*
>
> **Increment 5 ✓:** **model memori GC** — `mmap PROT_NONE` = **reserve lazy**
> (tanpa frame), `mprotect` = **commit on-demand** (`memory.rs`), persis yang
> dibutuhkan GC .NET untuk memesan heap besar di muka. Diverifikasi (`gcmem.cs`:
> reserve 256 MiB lazy [tak OOM] + commit halaman → `MILESTONE: GCMEM OK`, lolos
> 4-media).
>
> **Increment 6 ✓:** **Buitenzorg.Bcl** — pustaka gaya .NET tulisan-sendiri di C#
> (`userland/hello-csharp/bzbcl.cs`) di atas heap inc4: `BzList<T>` generik,
> **LINQ** (`Where/Select/Sum`), **StringBuilder** (dgn output angka dinamis),
> **BitConverter**, **Base64**. Nyata & dipakai app ring-3 (bukan CoreLib resmi).
> Diverifikasi (`bcl.cs`: LINQ sumOfSquares=120, base64=`eFY0Eg==` → `MILESTONE:
> BCL OK`, lolos 4-media). *(Gotcha: konversi method-group→delegate di-cache di
> GC static [belum jalan] → pakai **function pointer** `delegate*<int,bool>`.)*
>
> **Investigasi CoreLib asli:** `--stdlib:dotnet` default link **dinamis ke
> glibc** + butuh glue entry internal bflat (`__managed__Startup`/`RhInitialize`)
> → link statik-freestanding manual = reverse-engineering; boot penuh = usaha
> multi-sesi (fault startup GC/EEType/cctor). Ditunda; jalur tulisan-sendiri
> dipilih utk nilai nyata sekarang.
>
> **Increment 7 ✓ (perluasan Bcl):** `BzStack<T>`, `BzQueue<T>` (circular),
> `BzIntMap<V>` (Dictionary int-keyed, open-addressing), LINQ `Count/Any/All/
> Max/Min`, `BzStringBuilder.AppendHex`. Diverifikasi (`bcl.cs` → `count(even)=5
> max=9 min=0 stack/queue/map=111`, `MILESTONE: BCL OK`, lolos 4-media).
> **Berikutnya:** lanjut perluas Bcl (HashSet, Dictionary<K,V> generik, sortir,
> lebih banyak format) atau lanjut ke v0.16 "Panen". Link CoreLib asli
> dijadwalkan sebagai **eksperimen pasca-v1.0** (lihat bawah).

Menaikkan app C# dari **zerolib (tanpa GC)** ke **.NET managed penuh** — runtime
yang "matang". Ini "make-or-break" Layer 4 dan **prasyarat** preloaded suite
(v0.16) yang kaya. Jalur: **NativeAOT + BCL penuh dulu** (`bflat --stdlib:dotnet`
/ ILC), **CoreCLR/JIT menyusul**.

- **GC + managed heap** untuk app ring-3 (ganti bump allocator zerolib dengan
  GC bawaan NativeAOT; init GC statics; syscall alokasi/commit/free halaman
  gaya mmap untuk user-space).
- **PAL (Platform Abstraction Layer)** untuk runtime .NET: memori, thread,
  sinkronisasi (mutex/condvar/futex), TLS, waktu presisi tinggi, environment,
  **exception unwinding** (EH funclets / DWARF).
- **Thread ring-3 sungguhan** (sekarang app kooperatif satu-satu) + primitif
  sync + **thread pool** — fondasi Multithreading/Timers/Tasks.
- **BCL penuh** → membuka: **Generics, Tuple, Collections** (List/Dictionary/
  HashSet/Queue/Stack), **LINQ**, **Regex**, **StringBuilder** + string
  interpolation, **ToString/format**, **Encoding** (UTF8/Unicode, **Base64**
  via `Convert`, **BitConverter**), **DateTime/TimeSpan/Guid**.
- **System.Threading**: Thread, ThreadPool, **Timer**, lock/Monitor,
  Interlocked, ManualResetEvent, dll.
- **Tasks & async/await (TPL)**: Task/Task<T>, `async`/`await`,
  SynchronizationContext di atas thread pool.
- **Streams / System.IO**: MemoryStream + FileStream di atas VFS kernel.
- **Uji kontrak + sample**: app C# yang benar-benar memakai GC + generics +
  LINQ + Regex + StringBuilder + Base64/Encoding + collections + Threads/Tasks/
  async + Streams, **diverifikasi jalan di ring 3 (QEMU)**.
- **(Menyusul) CoreCLR/JIT + reflection penuh + Reflection.Emit** — untuk
  skenario dinamis yang tak bisa NativeAOT (dibatasi trimming).

**Milestone:** app C# ber-GC memakai LINQ/Regex/StringBuilder/Base64/
Collections/Tasks-async/Streams berjalan & terverifikasi di QEMU.

## 🌾 v0.16 — "Panen" · Preloaded Suite & Optimization Pass ✅
Dibangun **di atas BCL v0.15**. Utilities, multimedia, AI apps, games, themes,
productivity, store bawaan; optimization pass (fast boot, fast I/O, startup,
footprint, SIMD); benchmark regresi di CI. **Milestone:** OS ringan & cepat,
siap pakai.
> **Progres suite ✓ (mulai, `panen_suite_demo`):** **Kalkulator**
> (`calc.cs`→CALC.ELF) — `Buitenzorg.UI` Grid 4×4 tombol ter-tema (digit biru,
> operator oranye; rounded+gradient+shadow) di atas display numerik (render
> angka via `Graphics.DrawChars` char[], tanpa managed string); klik didispatch
> lewat `Button.Tag` ke engine `Calc`. Verified simulasi `12+3=`→15 →
> `MILESTONE: CALC OK` (IDE) + `SUITE OK` (4-media); screenshot
> `docs/img/desktop-calc.png`. **+ 2048** (`game2048.cs`→G2048.ELF) — game
> geser-ubin: `Board2048:UIElement` papan 4×4 (ubin rounded berwarna per-nilai +
> angka DrawChars) + engine slide/merge; verified `2+2`→4, `4 4 8 8`→`8 16` →
> `MILESTONE: GAME OK`, screenshot `docs/img/desktop-2048.png`. **+ Jam**
> (`clock.cs`→CLOCK.ELF) — jam analog (face AA + 12 tick + jarum jam/menit/detik
> rotasi `SinFx/CosFx`) + digital HH:MM:SS (DrawChars) → `MILESTONE: CLOCK OK`,
> screenshot `docs/img/desktop-clock.png`. **+ Piano** (`piano.cs`→PIANO.ELF)
> — keyboard 1 oktaf (tuts putih berlabel + tuts hitam) → klik main nada via
> `Buitenzorg.Audio` `Mixer.Beep`; verified arpeggio C-E-G-C → `MILESTONE:
> PIANO OK`, screenshot `docs/img/desktop-piano.png`. **+ App Store**
> (`store.cs`→STORE.ELF) — store front: `StoreView` katalog app (nama +
> kategori + badge TERPASANG/TERSEDIA) + tombol PASANG/HAPUS → `MILESTONE:
> STORE OK`, screenshot `docs/img/desktop-store.png` — **terhubung ke `pkg.rs`**
> via syscall `PKG_LIST`/`PKG_SET` (ABI 27-28 + `PkgInfo`), install nyata gating
> `run`, diverifikasi baca-ulang registry. **+ File Manager** (`filemgr.cs`→
> FILES.ELF) jelajah VFS via syscall `FS_LIST` (ABI 29 + `FsEntry`; mount/file,
> path bar + navigasi) → `MILESTONE: FILES OK`, screenshot desktop-files.png.
> **+ Text Editor** (`editor.cs`→EDITOR.ELF) editor multi-baris (`TextArea`
> buffer+caret+wrap, Type/Newline/Backspace) + Menu → `MILESTONE: EDITOR OK`,
> screenshot desktop-editor.png. **+ Image Viewer** (BMP/JPEG). **Suite kini 8 app** (semua kategori:
> productivity/game/utilitas/multimedia/store). **+ Optimization pass:** pangkas Sleep
> animasi demo → boot 69.8s→58.5s (~16%). **Fix penting:**
> Heisenbug `from_raw_parts` atas memori user (laten, dipicu geser layout kernel
> saat menambah suite demo) — di-harden dgn `copy_user_bytes` (read_volatile) di
> semua syscall string. App `/disk` hanya jalan di media IDE (driver ATA-PIO).

### 🎨 Framework grafis & UI (prasyarat suite)
> **Progres ✓:** **renderer software client-side** (`bzgfx.cs`) + **kernel BLIT
> op** (ABI draw_op 7): app menggambar ke `Bitmap` managed (semua primitif di C#)
> lalu blit 1-syscall ke window — model kompositor WPF/Avalonia. Sudah:
> `Bitmap`/`Color`/`Graphics` (Fill/DrawRectangle, DrawLine tebal, Circle/Ellipse,
> FillPolygon scanline, gradient, DrawImage, **alpha blending**). Verifikasi
> `draw.cs` → `MILESTONE: DRAW OK` (4-media) + screenshot
> `docs/img/desktop-drawing.png`. **+ transform 2D** (`Matrix` Translate/Scale/
> Rotate, sin/cos fixed-point), **`GraphicsPath`** (MoveTo/LineTo/AddRectangle/
> AddEllipse + FillPath/DrawPath), **BMP 24-bit load/save** (round-trip) — semua
> terverifikasi (`draw.cs`, kotak cyan ter-rotasi di screenshot). *(Gotcha kernel:
> JANGAN buat `&[u32]` slice dari pointer user memory — LLVM asумsikan region
> dereferenceable/immutable → korupsi boot; baca per-pixel `read_volatile`.)*
> **+ DrawString + Font 8×8 embedded + MeasureString** (teks dirender ke Bitmap),
> **SetClip/ResetClip**, **FillHatch** (6 pola), **DrawImageScaled** — semua
> terverifikasi (screenshot menampilkan teks "BUITENZORG.DRAWING", hatch, clip,
> gambar ter-skala). **+ Enhance visual ✓:** **FillRoundedRectangle**/
> **DrawRoundedRectangle** (sudut anti-alias via coverage), **FillRoundedGradientV**
> (tombol bergradasi + sudut membulat), **DrawShadow** (drop shadow lembut
> berlapis), **FillGradientH/FillGradient**, **FillCircleAA** — untuk mempercantik
> tampilan UI (diterapkan ke kontrol di Pendalaman UI). Diverifikasi (`draw.cs` →
> MILESTONE DRAW OK, 4-media). **Buitenzorg.Drawing ~selesai (usable-complete).**
> Menyusul (opsional): Region non-rect, PNG, texture brush, Pen dash, AA garis/teks
> penuh, font-pack penuh.
- **`Buitenzorg.Drawing` selengkap `System.Drawing`.** Lengkapi API: `Graphics`
  (transform/clip, `DrawImage`, `DrawPath`, `DrawString` + `Font`/`FontFamily`,
  `MeasureString`, gradient/`LinearGradientBrush`/`TextureBrush`, antialias/blend
  mode), `Pen` (width/dash/cap/join), `Brush` (solid/gradient/hatch), `Bitmap`
  (per-pixel, `LockBits`, load/save PNG/BMP), `Color` (named + HSL + alpha),
  `GraphicsPath`, `Region`, `Matrix`, `Point`/`Rectangle`/`Size` (F & int),
  `Image` (resize/crop/rotate). Target: mesin gambar 2D setara System.Drawing.
- **`Buitenzorg.UI` — toolkit UI gaya WPF/Avalonia.** Ringan & performa tinggi.
  > **Progres ✓ (increment 1–3, `bzui.cs`):** model retained (`UIElement` visual
  > tree, anak via linked-list) + layout Measure/Arrange (StackPanel V/H +
  > Padding/Spacing, **Grid** fixed+star, **Canvas** absolut; explicit
  > Width/Height menang atas stretch) + hit-testing (virtual) + **event mouse**
  > (`UIHost.Mouse`: hover Enter/Leave, press/capture/click/drag) + **set kontrol
  > lengkap-dasar** (TextBlock, Button, CheckBox, ProgressBar, Border, Slider,
  > RadioButton+RadioGroup, ListBox, TextBox, Menu, **ComboBox**, **TabControl**,
  > **TreeView**, **ScrollViewer**, **DataGrid**) + compositor software (render
  > tree ke Bitmap via Drawing, blit 1-syscall, `UIHost`). Diverifikasi (`ui.cs`
  > dua window: dasar + "Lanjutan" combo/tab/tree/scroll/grid → `MILESTONE: UI
  > OK`, screenshot `docs/img/desktop-ui.png`). *(Gotcha zerolib: store referensi
  > ke elemen OBJECT-ARRAY (`stelem.ref`→RhpStelemRef) fault → pakai linked-list;
  > `string ==` butuh `op_Equality` yg tak ada → banding by-reference / manual.)*
  > **Increment 4 ✓ Pendalaman UI:** (a) **popup/overlay layer** — ComboBox
  > dropdown dirender di atas seluruh tree + di-hit-test lebih dulu (`RenderPopup`/
  > `PopupHitTest`/`UIHost.WalkPopupHit`), memperbaiki "last-hit-wins"; (b) **tema
  > visual** — kontrol memakai primitif Drawing baru (Button rounded+gradient+
  > shadow, Slider/ProgressBar pill+gradient+thumb AA, CheckBox/Radio/ComboBox
  > rounded/AA) untuk tampilan modern (screenshot desktop-ui.png). Menyusul:
  > Dialog/ToolTip/menu popup berlapis, data-binding/MVVM, animasi 60fps, GPU.
  - **Model retained + tree visual/logical**, layout (Stack/Grid/Dock/Wrap/
    Canvas), `Measure`/`Arrange`, DPI-aware.
  - **Komponen lengkap:** Button, TextBox, CheckBox, RadioButton, Slider,
    ComboBox, ListBox, TreeView, TabControl, Menu, ScrollViewer, ProgressBar,
    ToolTip, Dialog/Window, DataGrid, Toggle, dsb.
  - **Data-binding + MVVM** (INotifyPropertyChanged, Command), **styles/templates**
    + integrasi **theme engine** (`theme.rs`), **XAML-like** markup (opsional).
  - **Rendering halus + animasi:** compositor retained, dirty-region, double-buffer,
    easing/storyboard, transisi, **60 FPS**, hemat memori.
  - **Akselerasi GPU:** backend render lewat **compute/GPU API** (`compute.rs`,
    `Backend::Gpu`) — path CPU sebagai fallback; **lightweight & high-performance**.
- **Milestone:** app suite digambar & dianimasikan mulus via `Buitenzorg.UI`
  di atas `Buitenzorg.Drawing`, dengan akselerasi GPU (fallback CPU).

### 🔊 Subsistem audio OS (prasyarat multimedia)
  > **Progres ✓ (`audio.rs` + `bzaudio.cs`):** driver **AC'97** — enumerasi PCI
  > (kelas 0x04/0x01), cold-reset codec, mixer (master + PCM-out volume),
  > **playback speaker** PCM 16-bit stereo 48 kHz lewat **DMA bus-master** (BDL).
  > ABI: `AUDIO_STAT`/`AUDIO_SET_VOLUME`/`AUDIO_TONE`/`AUDIO_PLAY` (23–26) +
  > mirror C# + kontrak + `docs/abi.md`. Library `Buitenzorg.Audio` (`Mixer`,
  > `Tone`) + **panel pengaturan** `audiopanel.cs` (Buitenzorg.UI: slider
  > volume + checkbox mute + tombol tes nada, terhubung ke `Mixer`).
  > Diverifikasi (kernel tone 440 Hz DMA + `audio.cs` volume round-trip 45→45 +
  > PCM stream → `MILESTONE: AUDIO OK`; `audiopanel.cs` slider→35 / mute→0 →
  > `MILESTONE: AUDIO PANEL OK`, 4-media). Menyusul: mic capture, IRQ, Intel
  > HDA, mixer per-app, pilih perangkat, meter, widget volume taskbar.
- **Driver audio kernel** (Rust): HDA/AC'97 (QEMU `intel-hda`/`ac97`) →
  playback speaker ✓ + capture mic; ring buffer + mixer ✓; IRQ/DMA (DMA ✓).
- **ABI audio** (syscall): ✓ stat/volume/tone/play (PCM), format 48 kHz stereo
  16-bit; menyusul open/read-stream + mixer per-stream.
- **Layanan audio** (C#): `Buitenzorg.Audio` ✓ playback + **kontrol volume**
  (master) + mute + tone/PCM; menyusul record, per-app, pilih perangkat, meter.
- **UI pengaturan audio** ✓ (dasar): panel `Buitenzorg.UI` slider/mute/tes-nada
  terhubung ke `Mixer` (`audiopanel.cs`); menyusul pilih perangkat + widget.
- **UI pengaturan audio:** panel volume/perangkat/mute di desktop (pakai
  `Buitenzorg.UI`), plus widget volume + shortcut.
- **Milestone:** OS memutar nada/PCM ke speaker & merekam dari mic (di QEMU),
  dengan kontrol volume dari UI.

## 🧑‍💻 Developer Experience & Onboarding (dokumentasi + tooling)
Agar siapa pun — termasuk orang awam — bisa menjalankan & mengembangkan OS ini.
- **Getting-started ramah pemula** (`docs/getting-started.md`): langkah demi
  langkah dari nol (pasang dependency → build → boot di QEMU), plus
  troubleshooting umum. Untuk **jalur cepat**: skrip **`scripts/quickstart.ps1`**
  (Windows) & **`scripts/quickstart.sh`** (Linux/macOS) yang **memasang semua
  dependency otomatis** (Rust nightly + target, .NET SDK, QEMU, bflat) lalu
  build + boot OS di QEMU dengan satu perintah.
- **Panduan bikin app pertama** (`docs/first-app.md`): dari template SDK →
  tulis kode → build → jalankan di OS; plus **katalog contoh app** yang bisa
  dibuat memakai library built-in (`Buitenzorg.Drawing`/`UI`/`Audio`/`Bcl`) —
  mis. kalkulator, game, jam, piano, editor, dsb (dengan cuplikan kode).
- **Jalankan di VM lain** (`docs/run-in-vm.md` + skrip): konversi image mentah
  ke **VMware** (`.vmdk`) & **Oracle VirtualBox** (`.vdi`) via `qemu-img`, plus
  berkas konfigurasi/`.vmx` dan langkah setup — `scripts/make-vm-images.ps1`/`.sh`.
- **Ekstensi VS Code** (`sdk/vscode-extension`): verifikasi & lengkapi — buat
  proyek (+ **pemilih template**), build & run, validasi manifest, **deploy**,
  dan **debugging DAP** (saat ini baru dideklarasikan, adapter belum ada).
- **Milestone:** pengguna baru bisa `quickstart` → OS jalan di QEMU dalam
  hitungan menit; developer bisa scaffold + jalankan app pertama & bawa image
  ke VMware/VirtualBox mengikuti dokumentasi.

## 🖱️ Interaktivitas, Format Gambar & Desktop UX (pra-v1.0)
Item lanjutan sebelum v1.0 (permintaan 2026-07-24):
- **Keyboard input routing untuk app interaktif:** saat app ring-3 berjalan
  dari shell (`run editor` / `run files`), ketikan pengguna dirutekan ke app
  (via syscall `KEY_READ`) alih-alih ke terminal — Text Editor bisa benar-benar
  mengetik/hapus/newline dan File Manager bisa navigasi dengan keyboard;
  demo boot tetap tersimulasi (non-blocking).
- **Dukungan JPG di `Buitenzorg.Drawing`:** cek status (saat ini hanya BMP
  24-bit) — evaluasi decoder JPEG baseline (Huffman + IDCT integer, tanpa
  float) untuk `Jpeg.Load`, dipakai Image Viewer & wallpaper; progressive JPEG
  menyusul.
- **Enhance UI/UX desktop OS — taskbar, start menu, desktop:** ambil inspirasi
  terbaik dari **macOS** (dock terpusat + ikon besar + magnify, menu bar atas,
  spotlight), **Ubuntu/GNOME** (activities overview, dash, workspace switcher
  visual), dan **Windows XP** (start menu dua kolom dengan app tersemat +
  daftar semua program, taskbar dengan jam/tray, tombol Start hijau ikonik).
  Kombinasikan: taskbar dengan tombol Start + pinned apps terpusat + tray
  (jam/volume), start menu dua panel (app terpasang dari `pkg.rs` + aksi
  power/settings), dan desktop dengan ikon app yang bisa diklik dua kali —
  memakai primitif `Buitenzorg.Drawing` (rounded/gradient/shadow/AA) di
  `wm.rs`/`theme.rs`.
- **Milestone:** editor menerima ketikan nyata di QEMU; JPG terbaca (atau
  keputusan terdokumentasi); desktop baru (taskbar+start menu+ikon) tampil dan
  bisa dipakai meluncurkan app.

## 🪄 MagicAppGen — Magic App Generator (tools host, pra-v1.0)
Aplikasi desktop **Avalonia UI** (cross-platform, .NET) bernama **Magic App
Generator (MagicAppGen)**: code editor + asisten AI untuk men-generate aplikasi
Buitenzorg OS dari prompt.
- **Asisten AI "Jack - The Code Bender"** via **Semantic Kernel**; LLM yang
  didukung: **OpenAI, Claude (Anthropic), Gemini, Ollama** — setting (model,
  API key, endpoint, temperature, system prompt) disimpan di **app.config**
  dan bisa diubah dari UI (dialog Settings).
- **Kernel functions** agar AI bisa membuat app Buitenzorg dengan benar (UI +
  backend): pengetahuan template/API `Buitenzorg.Drawing`/`UI`/`Audio`/`Bcl` +
  gotcha zerolib (no static ref fields, linked-list bukan object[], DrawChars),
  scaffold project, tulis/baca file project, build via bflat + link, jalankan
  smoke/QEMU, validasi manifest. **Common functions:** SearchInternet
  (**Tavily**), ScrapeWebPage, MathCalculation, CheckDateTime, dan util lain
  yang diperlukan.
- **UI (modern):** code editor (show/hide line number) + **panel chat di kanan**
  (bisa attach gambar, resize width, hide/show; kirim dengan **Ctrl+Enter**
  atau tombol Send; tombol clear chat thread; **pilihan model LLM di bagian
  atas panel**). Menu + toolbar: New Project (Blank / **From Template** —
  beberapa template jenis app berbeda), Open Project/File, Close Project,
  Go To Line Number, Build, Run, Deploy, Exit; **pilihan bahasa pemrograman**
  di toolbar (default **C#**; JS/TS/Python menyusul sesuai polyglot OS).
  **Status bar + logs panel** di bawah untuk memantau proses & output build/run.
- Lokasi: `tools/MagicAppGen` (app host .NET, bukan app ring-3 OS).
- **Milestone:** dari prompt ("buatkan app catatan sederhana") Jack menghasilkan
  project C# Buitenzorg yang ter-build dan jalan di QEMU dari dalam MagicAppGen.
- **STATUS ✓ (2026-07-24): TERPASANG & JALAN.** net10.0 + Avalonia 11.2 +
  AvaloniaEdit + Semantic Kernel 1.61 (+ `Anthropic.SDK` sbg jembatan Claude).
  Semua fungsi UI di atas ada; 8 template proyek (5 C# + js/ts/python), dan
  **kelima template C# diverifikasi ter-compile dengan bflat `--stdlib:zero`**.
  Bukti: `dotnet build` lolos, app berjalan, screenshot `docs/img/magicappgen*.png`.
  Menyusul (opsional): syntax highlighting TextMate, diff/apply patch dari chat,
  tab multi-file, dan rantai build→run satu tombol dari editor.

## 📚 Kelengkapan BCL & Adopsi di App (pra-v1.0)
Melengkapi **`Buitenzorg.Bcl`** agar menutupi namespace .NET yang lazim dipakai
app, lalu memastikan app/widget/tool yang sudah ada benar-benar memakainya
(permintaan 2026-07-24). Semua ditulis sendiri di atas heap zerolib
(function-pointer bukan delegate, linked-list bukan `object[]`, tanpa static
ref field) sampai eksperimen `--stdlib:dotnet` pasca-v1.0 berhasil.
- **`System.IO`** (`BzFile`/`BzDir`/`BzPath`/`BzStream`): baca/tulis file lewat
  syscall `FS_READ`/`FS_LIST` (+ FS_WRITE bila perlu), enumerasi direktori &
  mount, gabung/pisah path, stream baca-tulis di atas buffer.
- **`System.Text`** (`BzEncoding`): UTF-8/ASCII encode-decode byte↔char,
  melengkapi `BzStringBuilder` yang sudah ada.
- **`System.Text.RegularExpressions`** (`BzRegex`): mesin regex backtracking
  kecil — literal, `.`, kelas `[...]`, `^`/`$`, `*`/`+`/`?`, alternasi `|`,
  grup, `IsMatch`/`Match`/`Replace`/`Split`.
- **`System.Globalization`** (`BzCulture`): format angka (pemisah ribuan,
  desimal), tanggal/waktu, upper/lower invariant.
- **`System.Diagnostics`** (`BzProcess`/`BzStopwatch`/`BzDebug`): daftar &
  kill proses lewat `PROC_LIST`/`PROC_KILL`, stopwatch dari `CLOCK_MONO`,
  assert/trace ke serial lewat `DEBUG_WRITE`.
- **`System.Management`** (`BzSystemInfo`): info mesin — uptime/heap/RAM dari
  `SYS_STAT`, info audio dari `AUDIO_STAT`, daftar paket dari `PKG_LIST`.
- **`System.Net` / `Sockets` / `Http`** (`BzIPAddress`/`BzSocket`/`BzHttp`):
  di atas stack loopback Ethernet/ARP/IPv4/ICMP yang ada; butuh syscall socket
  + UDP/TCP di kernel — kirim/terima datagram dulu, HTTP client minimal
  (GET/POST teks) menyusul saat TCP siap.
- **`System.Threading.Tasks`** (`BzTask`): task cooperative di atas
  `THREAD_CREATE`/`JOIN` + futex — `Run`/`Wait`/`WhenAll`, tanpa thread pool.
- **`System.Timers`** (`BzTimer`): timer periodik/one-shot di atas
  `CLOCK_MONO`, dipompa dari loop app atau thread.
- **`GC` (System)** (`BzGC`): `Collect`/`GetTotalMemory`/`AddMemoryPressure`
  di atas bump-heap shim (statistik nyata; reklamasi menyusul dgn GC penuh).
- **`Pkg`** (`BzPkg`): API paket sendiri — `List`/`IsInstalled`/`Install`/
  `Remove`/`Search` di atas `PKG_LIST`/`PKG_SET`.
- **Audit adopsi:** periksa ulang seluruh app & widget yang sudah dibuat
  (Kalkulator, Editor, File Manager, Image Viewer, Jam, Piano, 2048, App Store,
  Task Manager, Paint, widget, webview, panel audio) agar memakai fungsi BCL
  yang tersedia (bukan kode ad-hoc), begitu pula tool host (`bz` CLI, ekstensi
  VS Code, MagicAppGen) memakai library/template resmi.
- **Milestone:** `bcl2.cs` memverifikasi tiap namespace baru di ring-3 QEMU
  (`MILESTONE: BCL2 OK`) dan minimal beberapa app suite direfaktor memakainya.
- **STATUS ✓ (2026-07-24): library SELESAI & terverifikasi** (`bzbcl2.cs` +
  `bcl2.cs` -> `MILESTONE: BCL2 OK`, smoke 4-media LOLOS, 0 fault). Syscall
  baru `FS_WRITE`=32, `CLOCK_RTC`=33, `NET_SOCKET/BIND/SEND/RECV/CLOSE/INFO`=34..39
  (COUNT=40) + struct `RtcTime`/`NetDatagram`/`NetInfo`; kernel dapat modul
  `rtc.rs` (CMOS) dan **UDP di `net.rs`** (socket + checksum pseudo-header).
  **Dua batas yang jujur dan belum tertutup:** (1) `System.Net.Http` baru
  lapisan pesan (build request / parse response) karena kernel belum punya
  **TCP** — `sock_kind::STREAM` ditolak; (2) `BzGC.Collect()` mengembalikan
  `false` karena heap ring-3 masih bump-only (statistiknya nyata, kolektornya
  belum). Perangkat jaringan juga masih loopback (driver e1000 menyusul).
- **Audit adopsi ✓ (2026-07-24): SELESAI.** 14 app/widget diperiksa: **7 diadopsi**
  (`clock` → waktu CMOS sungguhan + tanggal, `taskmgr`/`widget` → `BzProcess`/
  `BzSystemInfo` [menghapus dua salinan struct ABI buatan sendiri], `store` →
  `BzPkg`, `filemgr` → `BzDir`, `imgview` → `BzFile`/`BzPath`, `editor` →
  Open/Save NYATA lewat `BzFile`), **7 sengaja tidak** (menautkan ~30 KB library
  hanya untuk satu formatter angka lokal = rugi). Tool host: MagicAppGen
  `GetApiReference("bcl")` + system prompt diperluas ke katalog `bzbcl2.cs`;
  `docs/first-app.md` dapat tabel namespace. **Dua bug ketemu lewat audit ini:**
  `build-hello-csharp.sh` (jalur Linux/CI) kehilangan `imgview` + `jpgtest`
  (hanya masuk `.ps1`), dan README template SDK masih melarang `new T[]`
  padahal heap sudah jalan sejak v0.15. Smoke 4-media LOLOS, 0 fault.

## 🛠️ Developer Workstation — IDE, Terminal, Package Manager, Debug & Deploy (permintaan 2026-11-01)
Tujuan: developer bisa **menulis, menjalankan, men-debug, dan men-deploy app
langsung dari dalam Buitenzorg OS**, maupun dari **mesin lain (remote)** — tanpa
harus build ulang seluruh disk image tiap kali mengubah satu app.

**Kondisi awal (jujur):** shell punya ~30 perintah tapi tanpa history/pipe/
redirection/tab-completion; `bz` di OS hanya install/remove/list/search atas
registry statis di `pkg.rs` (tak ada format/berkas paket); "deploy" di ekstensi
VS Code = build ulang image + boot ulang QEMU; debugger hanya **level kernel**
(GDB ke stub QEMU) — belum ada debugger app ring-3; tak ada jalur host↔OS selain
serial log satu arah. Batas arsitektur yang menentukan urutan: **app ring-3 jalan
satu per satu (run-to-completion)**, **belum ada NIC nyata & TCP**, dan **`/disk`
belum bisa ditulis persisten** (FAT12 write hanya di `/ram`).

### Prasyarat lintas-fitur
- ✅ **Multi-proses ring-3 preemptif** *(increment 1 selesai 2026-11-01:
  address space per proses, preemption ring-3, kill, isolasi fault, fokus
  keyboard; `MILESTONE: MULTIPROC OK`)* — IDE + app yang dijalankan + debugger
  harus hidup bersamaan; tanpa ini IDE tak bisa "Run" lalu tetap tampil.
- ✅ **FS tulis persisten** *(selesai 2026-11-01: FAT12/16/32 write di `/disk`, ATA write + flush, `MILESTONE: DISK WRITE OK`)* — FAT16/32 write di `/disk` (atau FS journaled) agar app/
  paket hasil deploy bertahan setelah reboot.
- **Driver NIC** (virtio-net dulu — paling sederhana di QEMU — lalu e1000) +
  **TCP** — untuk deploy/remote via jaringan. UDP sudah ada.
- **Kernel log ring buffer** (`dmesg`) + saluran log per-app.

### A. Terminal & Shell
- **Line editing**: kursor kiri/kanan, Home/End, Ctrl+A/E/U/K/W, **history**
  (↑/↓, `history`, simpan ke berkas), **tab-completion** perintah + path.
- **Pipes `|`, redirection `>`/`>>`/`<`**, `&&`/`||`/`;`, variabel environment
  (`export`, `env`, `$PATH`, `$?`), alias, **skrip shell** (`.bzsh`) + `source`.
- **Coreutils**: `cp mv rm mkdir rmdir touch head tail wc grep find hexdump
  which tee sort`; proses: `ps kill top`; sistem: `dmesg free uptime df`.
- **App Terminal mandiri** (`terminal.cs`, Buitenzorg.UI): multi-tab, scrollback,
  copy/paste, warna ANSI, ukuran font; job control (`&`, `jobs`, `fg`, Ctrl+C)
  setelah multi-proses.

### B. Package Manager (`bz`) untuk developer
- **Format paket `.bzpkg`**: arsip berisi `app.manifest` + ELF/skrip + aset +
  hash SHA-256 (tanda tangan menyusul). `bz install ./app.bzpkg` dari berkas.
- **Host CLI**: `bz pack` (buat `.bzpkg` dari proyek), `bz publish` (ke repo).
- **Repository index remote** + `bz update/upgrade/search` (butuh TCP/HTTP);
  **versi + dependency resolution + rollback**; state terpasang **persisten**.
- **Library SDK sebagai paket** ber-versi (`Buitenzorg.Drawing/UI/Audio/Bcl`).
- **Izin dari manifest** (fs/net/audio/ai) ditegakkan permission broker.

### C. IDE di dalam OS — "Buitenzorg Studio"
- **Editor kode**: multi-tab, nomor baris, **syntax highlighting** (C#/JS/TS/
  Python/manifest), find/replace, go-to-line, auto-indent, undo/redo.
- **Explorer proyek** (pohon berkas + template "New Project"), **panel terminal
  terintegrasi**, **panel Problems** (error parser dengan baris/kolom), **Output**.
- **Run instan untuk JS/TS/Python** lewat `script.rs` (tanpa compiler — jalur
  pertama yang realistis di dalam OS).
- **C#**: fase awal = **remote build** (Studio kirim sumber ke host build server
  `bz build-server` → bflat → ELF kembali ke OS); compiler C# di dalam OS
  (Roslyn/ILC) jangka panjang, bergantung CoreCLR.
- **Integrasi debugger** (breakpoint di gutter, panel variabel/call stack).

### D. Debugging & Diagnostik
- **Crash report app ring-3**: saat app fault, cetak register + **backtrace
  ter-simbolisasi** (frame pointer + symtab ELF tak-strip) alih-alih hanya
  "USER page fault"; simpan ke `/ram/crash/`.
- **Debugger app ring-3**: breakpoint `int3` + single-step (TF/#DB) dirutekan ke
  debugger, baca/tulis register & memori proses, syscall `DEBUG_ATTACH/...`;
  perintah OS `bz debug <app>`.
- **GDB remote stub di kernel** (lewat serial COM2, lalu TCP) — host GDB/VS Code
  men-debug app & kernel **di hardware nyata**, bukan hanya stub QEMU.
- **VS Code DAP adapter** di atas stub/GDB-MI: breakpoint, step, variabel, stack.
- **Debugger skrip JS/TS/Python** di interpreter `script.rs` (breakpoint/step/
  inspeksi variabel — murah karena tree-walking).
- **Profiler app**: zona `profile.rs` lewat syscall + sampling RIP user dari IRQ
  timer; `bz prof <app>`. **Log**: `bz logs [-f] [app]`.

### E. Deploy & Run — lokal maupun remote
- **Lokal**: `run <path>` menjalankan ELF/skrip dari path mana pun (bukan hanya
  registry); "Run" dari Studio; hot-reload skrip.
- **Agen `bzd` (bz-link) di OS + perintah host `bz remote`**: `bz device list/
  pair`, `bz deploy <proyek> --target <qemu|serial:COM3|net:IP>`, `bz run`,
  `bz logs -f`, `bz shell` (shell remote), `bz push/pull` (transfer berkas),
  `bz debug --remote`. Deploy = kirim ELF/paket ke OS yang sedang berjalan →
  **tanpa build ulang image, tanpa reboot**.
- **Transport bertahap**: (1) **serial COM2** — jalan sekarang juga (QEMU
  `-serial tcp::4555,server`; hardware via kabel USB-serial/null-modem);
  (2) **UDP** setelah NIC; (3) **TCP** untuk throughput & banyak sesi.
- **Keamanan**: agen **mati default**, aktif via "Developer Mode"; pairing dengan
  token; tiap deploy diverifikasi hash; tercatat di log.
- **Tooling host diperbarui**: ekstensi VS Code (Deploy/Run/Debug → bz-link),
  MagicAppGen (Deploy ke perangkat), `bz` CLI host.

### Urutan fase
1. **Tanpa prasyarat baru:** shell line-editing/history/completion/pipes +
   coreutils + `dmesg`; crash report ter-simbolisasi; `run <path>`; format
   `.bzpkg` + `bz pack`/install dari berkas; Studio untuk JS/TS/Python;
   **bz-link lewat serial** (deploy/run/logs/shell dari host).
2. **Setelah multi-proses + FS persisten:** Studio + app berjalan bersamaan,
   debugger app ring-3, GDB stub kernel, VS Code DAP, Terminal app multi-tab +
   job control, paket persisten.
3. **Setelah NIC + TCP:** bz-link via jaringan, repo paket remote, remote build
   server C#, `BzHttp` jadi klien sungguhan.
4. **Jangka panjang:** compiler C# di dalam OS (butuh CoreCLR/Roslyn).

**Milestone:** dari VS Code di host, `bz deploy --target serial` mengirim app ke
Buitenzorg yang sedang berjalan (QEMU) **tanpa build ulang image**, app jalan,
log mengalir balik, dan breakpoint kena; di dalam OS, Studio mengedit & menjalankan
skrip Python, dan app yang crash menampilkan backtrace ter-simbolisasi.

## 🎬 Multimedia, Internet, Game (2D/3D) & Hiburan (permintaan 2026-11-01)
Tujuan: Buitenzorg layak dipakai untuk **memutar musik/video, menjelajah
internet, bermain game 2D dan 3D**, dan hiburan sehari-hari — bukan hanya demo.

**Kondisi awal (jujur):** grafis = framebuffer + compositor software, frame app
dikirim lewat `BLIT` yang menyalin **per piksel** (`read_volatile`) — cukup untuk
UI, berat untuk game 60 fps; **tak ada driver GPU**. Audio = AC'97 playback PCM
**tanpa IRQ** (polling), satu stream, tanpa mic. Input = PS/2, dan `KEY_READ`
hanya mengembalikan **karakter** — tak ada event tekan/lepas atau status tombol
yang dibutuhkan game; tak ada USB (gamepad/webcam). Jaringan = **UDP loopback**
saja (tanpa NIC, TCP, DNS, TLS). Format: BMP + JPEG baseline. WebView = subset
HTML mini. App ring-3 jalan **satu per satu**.

### Prasyarat bersama (kernel & driver)
- ✅ **Multi-proses preemptif** *(increment 1 selesai 2026-11-01)* — musik tetap jalan saat bermain game/browsing.
- **Surface shared-memory zero-copy** — app menggambar langsung ke buffer yang
  dipetakan ke compositor (ganti salinan per-piksel `BLIT`) + **vsync/frame
  pacing** + mode **fullscreen eksklusif**.
- **Timer resolusi tinggi** (APIC timer/HPET) untuk frame pacing & A/V sync.
- **Audio berbasis IRQ** + ring buffer latensi rendah + **driver Intel HDA**
  (hampir semua PC modern) + capture mic.
- **Input game**: syscall event keyboard **tekan/lepas + status tombol**, mouse
  relatif/capture; **stack USB (xHCI) + HID** untuk gamepad, keyboard/mouse USB,
  headset, webcam (UVC).
- **Storage besar & cepat** (AHCI/NVMe + FS tulis persisten) untuk berkas media
  dan aset game.
- **Matematika float/SIMD di ring-3** yang teruji (SSE/AVX untuk decoder & 3D).

### A. Multimedia — audio, gambar, video
- **`Buitenzorg.Media`**: pipeline sumber → demux → decode → render, jam media
  (A/V sync), `MediaPlayer` (play/pause/seek/volume/playlist), event media key.
- **Audio**: mixer software multi-stream + volume per-app, resampling; codec
  **WAV**, **FLAC**, **Ogg Vorbis/Opus**, **MP3**; synth **MIDI** (wavetable/FM)
  untuk musik game; perekam suara.
- **Gambar**: **PNG** (inflate/zlib), **GIF animasi**, JPEG progresif, thumbnail,
  orientasi EXIF.
- **Video**: **MJPEG** dulu (memakai ulang decoder JPEG — kemenangan cepat), lalu
  container MP4/MKV/WebM + codec software (**VP8/VP9/AV1** lebih aman lisensinya
  untuk OS MIT; H.264 perlu kajian paten), YUV→RGB SIMD, subtitle SRT; decode
  hardware setelah driver GPU.
- **App**: Media Player (audio+video), Music Player + perpustakaan musik, Photo
  Viewer (slideshow/zoom/rotasi), Sound Recorder, Camera (UVC), screenshot &
  perekam layar, widget "Now Playing" + OSD volume di tray.

### B. Internet
- **Stack jaringan**: driver **virtio-net** (QEMU) → **e1000** → Realtek;
  **TCP** penuh (state machine, retransmit, window), **DHCP**, **DNS resolver**,
  socket `STREAM` + `poll`, IPv6 menyusul; Wi-Fi jangka panjang.
- **Kriptografi & TLS 1.3/1.2**: CSPRNG (RDRAND/RDSEED + jitter), SHA-2, AES-GCM,
  ChaCha20-Poly1305, X25519, verifikasi RSA/ECDSA, **store sertifikat root**.
- **`Buitenzorg.Net.Http`** jadi klien sungguhan (HTTP/1.1 + HTTPS, redirect,
  chunked), **WebSocket**, download manager — dipakai juga repo paket `bz`.
- **Browser**: tingkatkan WebView bertahap → parser HTML5, **CSS** (box model,
  flexbox, font), layout + gambar, **JS** (perluas `script.rs` ke subset ES + DOM
  binding), tab/bookmark/riwayat; mode "reader/teks dulu" sebagai langkah awal.
- **App**: Web Browser, download manager, radio internet/podcast (streaming
  HTTP), klien chat (WebSocket), app pengaturan jaringan + **firewall** dan
  **izin jaringan per-app** (permission broker).

### C. Game 2D
- **`Buitenzorg.Game`**: game loop fixed-timestep + delta time, frame pacing,
  input (keyboard state, mouse, gamepad dengan pemetaan standar), sprite + sprite
  sheet + animasi, tilemap, kamera, collision (AABB/lingkaran), particle,
  scene manager, SFX + musik lewat mixer, loader aset (PNG/WAV/OGG), bitmap font,
  simpan progres ke disk.
- **Performa**: blit SIMD, dirty-rect, surface zero-copy, fullscreen.
- **Game bawaan**: Minesweeper, Solitaire, Snake, Tetris-like, Breakout,
  platformer demo; template game di SDK + MagicAppGen; game JS/Python lewat
  binding `Buitenzorg.Game`.

### D. Game 3D
- **Rasterizer software dulu** (`Buitenzorg.Graphics3D`): matematika vektor/
  matriks (float SIMD), transform + proyeksi perspektif, clipping, z-buffer,
  back-face culling, **texture mapping perspective-correct**, shading Gouraud/
  Phong ringan, loader **OBJ/glTF**, kamera. Kemenangan cepat: **raycaster**
  gaya Wolfenstein + kubus bertekstur berputar.
- **Jalur GPU**: **virtio-gpu** 2D → 3D (virgl) di QEMU → API gaya
  **Vulkan/WebGPU** `Buitenzorg.GPU` (command buffer, shader SPIR-V) yang juga
  mengisi `compute.rs` `Backend::Gpu`; driver GPU nyata (Intel dulu — dokumentasi
  terbuka) jangka panjang.
- **Engine**: scene graph, animasi skeletal, fisika rigid body, **audio 3D
  posisional**.
- **Port game yang ada**: lapisan **libc (port musl/newlib) + subset SDL2** agar
  game C/C++ open-source bisa di-port — target ikonik: **"Buitenzorg menjalankan
  DOOM"**.

### E. Hiburan & UX
- **Mode game**: fullscreen, tunda notifikasi, prioritas penjadwalan.
- Media key + OSD volume, hotkey screenshot, perpustakaan media lintas app,
  kategori Game/Media di App Store.
- **E-book & komik** (TXT/EPUB = zip + HTML; CBZ), emulator klasik sebagai
  showcase (mis. NES/CHIP-8), launcher "big picture" ramah gamepad.
- Integrasi AI lokal: generate gambar/musik (GenAI v0.12) untuk wallpaper,
  aset game, dan playlist.

### Urutan fase
1. **Tanpa prasyarat baru:** decoder PNG/GIF/WAV/FLAC, synth MIDI, mixer
   software multi-stream, event tekan/lepas keyboard, `Buitenzorg.Game` 2D +
   Minesweeper/Snake/Breakout, rasterizer software + raycaster, player MJPEG,
   Media Player (WAV/FLAC), pembaca e-book TXT.
2. **Kernel & driver:** multi-proses, surface zero-copy + vsync, timer presisi,
   audio IRQ + HDA + mic, USB xHCI + HID gamepad, storage AHCI/NVMe.
3. **Jaringan:** virtio-net/e1000 → TCP/DHCP/DNS → TLS → HTTP/HTTPS → browser,
   radio internet, unduhan.
4. **GPU & ekosistem:** virtio-gpu → virgl 3D → API gaya Vulkan; codec video
   modern; lapisan libc + SDL2 → port DOOM; driver GPU nyata.

**Milestone:** musik Ogg/FLAC diputar di latar sambil memainkan game 2D 60 fps
dengan gamepad USB; video diputar dengan A/V sync; browser membuka halaman
HTTPS; adegan 3D bertekstur ≥30 fps lewat rasterizer software dan lewat
virtio-gpu.

## 🏛️ v1.0 — "Buitenzorg" · Stable Release (x86-64) — *sedang berjalan*
Stabilkan API & ABI, security hardening, dokumentasi + tutorial, debugger +
profiler, instalasi ke hardware nyata, image resmi VMware/QEMU/Hyper-V/VirtualBox.
**Milestone:** rilis stabil.

- **Security hardening ✓ (2026-07-24):** validasi pointer user di **semua**
  syscall berpointer (`memory::validate_user_range`/`validate_user_cstr`).
  Menutup lubang nyata: app ring-3 sebelumnya bisa membuat kernel **membaca**
  memorinya sendiri ke serial (`DEBUG_WRITE`) dan **menulis** hasil syscall ke
  memori kernel (`SYS_STAT`/`PROC_LIST`/`FS_READ`/`NET_RECV`) — tulis-sembarang
  alias eskalasi privilege — atau mematikan kernel dengan pointer tak-terpeta.
  Cek hanya di jalur ring-3 (`dispatch_from_user`). Diverifikasi 14 probe
  bermusuhan → `MILESTONE: SECURITY OK`.
- **ABI dibekukan ✓:** `abi_v1_is_frozen` + `AbiV1IsFrozen` memaku versi,
  `COUNT`, ukuran+alignment 10 struct, dan kode error; kebijakan di `docs/abi.md`.
- **Benchmark regresi di CI ✓:** `scripts/bench.sh` (boot-to-READY + async-I/O
  ops/s vs budget) sebagai job baru. **Sekaligus memperbaiki bug CI:** workflow
  memicu di `main` padahal branch repo `master` — CI tak pernah jalan saat push.
- **Boot USB hardware — perkakas + dokumentasi SIAP ✓ (2026-07-24):**
  `scripts/flash-usb.ps1`/`.sh` menulis image raw ke USB dengan pengaman
  berlapis (hanya disk removable, tolak disk sistem, konfirmasi ketik,
  verifikasi baca-ulang) + `docs/install-hardware.md` (pilih firmware, jalur
  GUI, boot menu, tabel kompatibilitas + checklist verifikasi HW). **Validasi di
  mesin fisik menyusul** — tak bisa dari lingkungan dev; ditandai eksperimental.
- **Debugger + profiler ✓ (2026-07-24):** `scripts/debug-kernel.ps1`/`.sh`
  (GDB attach ke QEMU-paused dgn simbol kernel + helper `debug-kernel.gdb`) dan
  profiler zona TSC ter-instrumentasi (`profile.rs` — inert saat off; jalur
  panas syscall/compositor terinstrumentasi; shell `prof`; verifikasi
  `MILESTONE: PROFILER OK`). Docs `docs/debugging.md`. (DAP VS Code menyusul.)
- **Dokumentasi/tutorial rilis ✓ (2026-07-24):** `CHANGELOG.md` (riwayat per
  codename), `docs/tutorial.md` (tutorial nol→app berurutan), `docs/README.md`
  (indeks docs), README di-refresh ke status v0.16 + jalur v1.0.
- **Image Hyper-V VHDX ✓ (2026-07-24):** `make-vm-images` emit
  `buitenzorg.vhdx` (64 MiB whole-MiB via create+`convert -n`, krn `-O vhdx`
  telanjang 5,47 MiB ditolak Hyper-V & vhdx tak bisa di-resize) +
  `scripts/make-hyperv-vm.ps1` (buat VM Gen-1/BIOS, guarded + fallback manual).
  Konversi & pembuatan VM terverifikasi; boot Hyper-V nyata menyusul.
- **Lisensi ✓ (2026-07-24):** **MIT** — berkas `LICENSE` (© 2026 Gravicode
  Studios).
- *Sisa:* validasi boot di hardware nyata & Hyper-V nyata (perkakas siap).

## 🌍 v1.x — "Rimba" · Multi-Arch (ARM64 + RISC-V)
Port ARM64 lalu RISC-V lewat **HAL** yang dipisahkan dulu di x86-64, lalu
optimasi per-arsitektur. **Milestone:** boot ke desktop & jalankan app di
QEMU `-M virt` untuk ARM64 dan RISC-V.

### Audit keterikatan arsitektur (2026-11-01)
Dari 47 modul kernel, **24 sudah netral arsitektur** (WM/compositor, VFS, FAT,
shell/terminal, interpreter JS/TS/Python, AI, VMM perangkat lunak, net, pkg,
process, wallpaper/ikon, theme, …) — itu yang ikut pindah gratis. **23 modul
terikat x86**, dalam tiga kelompok:

| Kelompok | Modul | Pengganti di ARM64 / RISC-V |
|---|---|---|
| **Inti CPU** | `gdt` (GDT/TSS), `interrupts` (IDT), `usermode` (SYSCALL/SYSRET), `task` (asm context switch + FXSAVE), `smp` (LAPIC/INIT-SIPI), `memory`/`aspace`/`elf`/`allocator` (page table `x86_64::`), `profile`/`syscall` (RDTSC) | Vektor eksepsi EL1 + `SVC` / `stvec` + `ECALL`; tabel halaman TTBR0/TTBR1 (ARM) / Sv39–Sv48 (RISC-V); simpan-pulih FP/NEON / F-D; PSCI / SBI HSM untuk start core; counter `CNTVCT` / `rdtime` |
| **Perangkat PC (I/O port)** | `serial` (16550), `ata` (IDE PIO), `mouse`/`keyboard` (PS/2), `pci` (CF8/CFC), `rtc` (CMOS), `power` (ACPI PM1 + port QEMU), `audio` (AC'97) | PL011 / 16550-MMIO; **virtio-mmio/PCIe ECAM**: virtio-blk, virtio-input, virtio-gpu, virtio-net, virtio-sound; PL031 / Goldfish RTC; PSCI / SBI SRST untuk shutdown |
| **Boot** | `bootloader` 0.11 (BIOS/UEFI x86 saja), `BootInfo`, ACPI-only | Protokol boot multi-arch (**Limine**: x86-64/aarch64/riscv64) atau stub UEFI sendiri; penemuan platform **ACPI *atau* Device Tree** |

Yang sudah netral: **ABI syscall** (`bz-abi`: nomor + struct `#[repr(C)]`,
little-endian di ketiga arsitektur — hanya *instruksi* syscall yang beda),
mirror C#, format disk (FAT), format aset (BMP/ICONS.BIN).

### Rencana bertahap
1. **M0 — Ekstraksi HAL di x86-64 (tanpa ubah perilaku)** *(✓ 2026-11-01 dalam 3 increment: `arch/x86` + `platform/pc` + fasad `arch::cpu`; `arch::context`; `arch::paging` + `aspace` pindah ke `arch/x86` — allowlist pemeriksa batas CI kini **kosong**. Sisa antarmuka `irq`/`timer`/`console`/`smp` ikut dirapikan saat M1–M3 membutuhkannya)*. Modul `arch/`
   dengan antarmuka: `cpu` (halt, irq on/off, relax, timestamp), `irq`
   (controller + EOI + vektor), `timer`, `paging` (map/unmap/switch address
   space — termasuk model PDPT-swap saat ini), `context` (switch + entri user +
   entri syscall + simpan FPU), `smp` (start core, IPI), `console`. Driver port
   I/O pindah ke `platform/pc`. Gerbang: kernel x86-64 identik perilakunya
   (smoke 4-media + UEFI lolos), dan **CI cek bahwa modul generik tak lagi
   memakai `x86_64::`/`asm!`/port I/O**.
2. **M1 — Boot & penemuan platform multi-arch.** *(✓ 2026-11-01: increment 1 `boot::BootInfo` + `dtb.rs` (`DTB OK`); increment 2 boot lewat Limine (UEFI) sampai READY dengan set milestone yang sama — smoke `[limine]`)* Abstraksi `BootInfo` (peta
   memori, framebuffer, RSDP/DTB, offset peta fisik); dukungan Limine; parser
   Device Tree kecil di samping ACPI.
3. **M2 — Driver virtio** *(✓ 2026-11-01: blk/input/net/gpu/snd di atas virtio-PCI modern, smoke `[virtio]`; polling, virtio-mmio menyusul di M3)* (blk, input, gpu, net, sound) — juga mempercepat x86
   di QEMU dan menggantikan IDE PIO/PS/2/AC'97 di semua arsitektur.
4. **M3 — ARM64 (`aarch64-unknown-none`, QEMU `virt`).** *(increment 1 ✓ 2026-11-01: boot via Limine, vektor exception, DTB hidup, PL011, GICv3 + timer — `ARM64 BOOT OK`; target `aarch64-unknown-none-softfloat`. Increment 2 ✓: memori generik + heap berpaging + address space TTBR0 per proses — `ARM64 MEMORY OK`, `ARM64 ASPACE OK`. Increment 3 ✓: context switch + EL0 + syscall `SVC` — `ARM64 CONTEXT OK`, `ARM64 EL0 OK`. Increment 4 ✓: ELF + app C# (bflat linux-arm64) — `ARM64 CSHARP OK`. Increment 5 ✓: scheduler + proses + thread — `ARM64 MULTIPROC OK`. Increment 6 ✓: virtio-mmio — `/disk` (`ARM64 DISK OK`), PL031, virtio-gpu. Increment 7 ✓: seluruh kernel generik + app GUI C# + frame desktop — `ARM64 DESKTOP FRAME OK`. Increment 8 ✓: desktop interaktif (input virtio, tampil lewat virtio-gpu). Increment 9 ✓: SMP lewat PSCI CPU_ON — `ARM64 SMP SCHED OK`. **M3 selesai.**)* PL011, GICv3,
   generic timer, MMU 4K (TTBR0 = user, TTBR1 = kernel: address space per
   proses jadi lebih alami), EL0/EL1 + `SVC`, PSCI. Userland C#: bflat
   `linux-arm64` + shim `bzstart.rs` versi aarch64 (`svc #0`).
5. **M4 — RISC-V (`riscv64gc-unknown-none-elf`, QEMU `virt` + OpenSBI).**
   16550-MMIO, PLIC + timer SBI, Sv39/Sv48, U/S-mode + `ECALL`, SBI HSM.
   *(Increment 1 ✓ 2026-11-02: boot OpenSBI → edk2 → Limine → S-mode, NS16550 dari DTB, trap `stvec`, memori + heap di atas Sv48, address space per proses, timer SBI — `RISCV BOOT OK`. Increment 2 ✓: context switch + U-mode + `ECALL`, fault terisolasi — `RISCV UMODE OK`. Increment 3 ✓: program ELF (Rust) sebagai proses preemptif di scheduler bersama, thread + futex — `RISCV MULTIPROC OK`, `RISCV PROCESSES OK`. Increment 4 ✓: PLIC, Goldfish RTC, virtio-mmio `/disk` (baca/tulis) + virtio-gpu — `RISCV PLIC OK`, `RISCV DISK OK`. Increment 5 ✓: kernel generik penuh + desktop interaktif, JS/TS/Python, app GUI — `RISCV DESKTOP FRAME OK`, `VIRTIO INPUT OK`. Increment 6 ✓: SMP lewat SBI HSM, RFENCE + IPI — `RISCV SMP SCHED OK`. **M4 selesai (2026-11-02).**)*
   **Batas jujur:** .NET NativeAOT untuk RISC-V belum didukung resmi — app
   pertama di RISC-V berjalan lewat runtime JS/TS/Python (netral arsitektur);
   C# menyusul saat toolchain-nya siap.
6. **M5 — CI matrix** `qemu-system-{x86_64,aarch64,riscv64}` dengan smoke
   test yang sama (marker MILESTONE) per arsitektur. *(✓ 2026-11-02: job
   build + matrix smoke per arsitektur, mode strict, log sebagai artifact.
   Roadmap multi-arsitektur M0–M5 selesai.)*

## 🔮 Pasca-1.x (jangka panjang)
SMP/NUMA matang · driver GPU modern + NPU · WiFi/Bluetooth/USB luas ·
container & sandboxing lanjut · marketplace app/tema/game/model AI ·
GenAI video/audio real-time.

### 🧪 Eksperimen pasca-v1.0: link CoreLib .NET ASLI (`--stdlib:dotnet`)
Proyek riset tersendiri (multi-sesi, hasil tak pasti) untuk menautkan **BCL .NET
resmi + GC WorkstationGC** dari bflat, menggantikan Buitenzorg.Bcl tulisan-sendiri
dengan **LINQ/Regex/Tasks/StringBuilder resmi**. Investigasi v0.15 inc 6 memetakan
jalurnya:
- `--stdlib:dotnet` default link **dinamis ke glibc** (`libc.so.6`/`libpthread`/
  `libdl`/`ld-linux`) → perlu **link statik-freestanding** dengan PAL & crt sendiri.
- Butuh **glue entry internal bflat** (`__managed__Startup`/`RhInitialize`) yang
  tak muncul di output `-c` → reverse-engineering.
- ~150 simbol leaf PAL (banyak SUDAH ADA dari v0.15: mmap+reserve/commit, thread,
  futex→mutex, clock, thread-self, malloc/free/calloc/realloc, memcpy/memset;
  sisanya stdio/TLS `__tls_get_addr`/pthread_key/`SystemNative_LowLevelMonitor_*`/
  C++ new-delete/getenv/sigaction — banyak bisa stub).
- Link `libbootstrapperdll.o` + `libRuntime.WorkstationGC.a`, lalu tembus **fault
  startup GC/EEType/cctor** (banyak cctor CoreLib butuh GC+statics; sulit di-debug
  bare-metal).
Dijadwalkan **setelah v1.0** karena berisiko & tak boleh menghambat rilis stabil.

---

## Utang Teknis Lintas-Versi (backlog)

Item yang belum tuntas dan dijadwalkan menyusul (lihat anotasi di [Progress.md](Progress.md)):

- **CoreCLR/JIT + GC + reflection** — runtime C# masih NativeAOT/bflat freestanding (heap bump tanpa kolektor sejak v0.15). Loncatan Layer-4 "Menengah" (§5.1).
- **APIC + SMP** — sebagian (2026-11-01): LAPIC BSP (virtual wire), MADT, AP dibangunkan (`SMP OK`); increment 2: GDT/TSS per-AP, timer LAPIC di AP, IPI ping + TLB shootdown, state per-CPU via ID LAPIC (`SMP PERCPU OK`). Increment 3: proses ring-3 paralel di semua core (`SMP SCHED OK`). Increment 4: kernel SMP simetris dengan Big Kernel Lock — syscall & task kernel di semua core (`SMP KERNEL OK`). Increment 5: lock berbutir halus — syscall memori/scheduler/konsol tanpa BKL (`SMP FINELOCK OK`, ×2,25 di 4 CPU). WM/KEY_READ/registry proses/RTC menyusul. I/O APIC ✓ (`IOAPIC OK`). Lock WM dipecah (kanvas per jendela + komposisi dari snapshot); thread paralel dalam satu proses (`SMP THREADS OK`). FS tanpa BKL; syscall dengan interrupt menyala. Proses (spawn/wait), jaringan, paket, audio kini juga tanpa BKL. Sisa di bawah BKL: EXIT/PROC_KILL dan THREAD_CREATE/JOIN.
- **Driver native NVMe/AHCI/USB & e1000** — saat ini hanya IDE PIO + net loopback.
- **Filesystem kustom journaled** — FAT12/16/32 read + write sudah ada (2026-11-01); journaling, subdirektori, LFN belum.
- **TCP** — sudah Ethernet/ARP/IPv4/ICMP + **UDP** (loopback); TCP + driver NIC belum.
- ~~**Multi-proses preemptive di ring 3**~~ — ✓ increment 1 (2026-11-01). Sisa: argv/`wait`/handle per proses, tutup window saat proses keluar, reklamasi frame, SMP.
- **Tiling WM, settings/notifikasi, pipes/redirection shell** *(launcher Start menu + tray ✓ v0.16)*.
- **VS Code DAP debugging + debug bridge** — extension punya new/build/deploy/GDB-kernel; DAP + bridge ke OS belum (lihat §Developer Workstation).

---

## Urutan & Ketergantungan Kritis

1. **GPU (v0.11) sebelum AI (v0.12)** — AI butuh compute API + fallback CPU.
2. **Runtime C# penuh (GC/CoreCLR)** membuka library kaya (`Buitenzorg.Drawing`
   penuh, task manager kaya, BCL) — dikerjakan bertahap dari v0.9.
3. **Multi-proses** adalah prasyarat task manager yang benar-benar "kill proses
   lain"; versi awal (v0.9) memantau task kernel + app tunggal, disempurnakan
   saat multi-proses matang.
4. **HAL multi-arch** disiapkan sejak awal; port ARM64/RISC-V setelah stabil x86-64.
