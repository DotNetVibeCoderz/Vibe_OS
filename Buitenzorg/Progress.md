# Progress.md — Tracking Pengembangan Buitenzorg OS

> Checklist status pengembangan per-versi & per-subsistem. Verifikasi tiap
> boot lewat marker `MILESTONE: ... OK` (dicek smoke test 4-media).
> Roadmap ke depan: [PLAN.md](PLAN.md) · Desain teknis: [requirements.md](requirements.md).

**Legend:** `[x]` selesai · `[~]` sebagian (lihat catatan) · `[ ]` belum.
**Status keseluruhan:** v0.1 – v0.14 milestone **tercapai**; **v0.15 "Matang" sedang berjalan** (inc 1: PAL memori `MMAP/MPROTECT/MUNMAP` ✓; inc 2: thread ring-3 kooperatif `THREAD_CREATE/JOIN/EXIT` ✓; inc 3: sync `FUTEX_WAIT/WAKE`+mutex, `THREAD_SELF` (TLS), `CLOCK_MONO` ✓; inc 4: **managed heap** — `new`/array/generics jalan + `malloc/free/calloc/realloc` ✓; inc 5: **model memori GC** — mmap `PROT_NONE`=reserve lazy + mprotect=commit-on-demand ✓; inc 6: **Buitenzorg.Bcl** — List/Stack/Queue/Dictionary(int)/LINQ/StringBuilder/BitConverter/Base64 tulisan-sendiri di C# ✓ — semua terverifikasi dari C# ring-3, lolos 4-media).
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
| **v0.15 Matang** | **Managed runtime C# lengkap (GC + .NET BCL: LINQ/Regex/Tasks/…)** | — | **[ ] Berikutnya** |
| v0.16 Panen | Preloaded suite + optimization pass (di atas BCL v0.15) | — | [ ] Rencana |

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

## ⏳ v0.15+ (Rencana — ringkas)

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
- [ ] **Regex** (System.Text.RegularExpressions)
- [~] **StringBuilder + string/char/math/random** — inc 6 + v0.16: `BzStringBuilder` (Append string/int/long/char/hex + **Clear/AppendLine/AppendChars/CopyTo**); **`BzMath`** (Abs/Min/Max/Clamp/Sign/Pow/ISqrt/Gcd/Lcm); **`BzRandom`** (xorshift64* PRNG); **`BzStr`** (Equals/Compare/StartsWith/EndsWith/IndexOf/Count/Upper/Lower/IsDigit/IsAlpha — pakai `(object)` cast krn `string ==` tak ada); interpolation/ToString penuh menyusul
- [~] **Encoding/konversi** — inc 6 + v0.16: **Base64** (`BzBase64`) + **BitConverter** (`BzBitConverter`) + **`BzHex`** (bytes→hex) + **`BzConvert`** (ParseInt/ParseLong/ParseHex + LongToChars) jalan; UTF8/Unicode penuh menyusul
- [ ] **DateTime/TimeSpan · Guid** dan tipe .NET modern lain
- [ ] **System.Threading**: Thread · ThreadPool · **Timer** · lock/Monitor · Interlocked · reset events
- [ ] **Tasks & async/await (TPL)**: Task/Task<T> · `async`/`await` · SynchronizationContext
- [ ] **Streams / System.IO**: MemoryStream · FileStream di atas VFS
- [ ] Uji kontrak + sample C# (GC+LINQ+Regex+StringBuilder+Base64+Collections+Tasks+Streams) **terverifikasi di QEMU**
- [ ] **(Menyusul) CoreCLR/JIT + reflection penuh + Reflection.Emit** (skenario dinamis di luar batas NativeAOT/trimming)

### 🌾 v0.16 "Panen" — Preloaded Suite & Optimasi
**Prasyarat: framework grafis/UI + audio (lihat PLAN.md §v0.16 untuk rincian).**

🎨 `Buitenzorg.Drawing` selengkap `System.Drawing` — **renderer software (client-side) + BLIT** (`bzgfx.cs`, `draw.cs`)
- [~] **Graphics ✓:** Clear, Fill/DrawRectangle(thick), DrawLine(+thick, Bresenham), Draw/FillCircle, Draw/FillEllipse, FillPolygon(scanline)/DrawPolygon, FillGradientV/**FillGradientH/FillGradient**, DrawImage/**DrawImageScaled**(nearest), **alpha blending**, **transform 2D (Matrix Translate/Scale/Rotate, sin/cos Bhaskara)**, **FillPath/DrawPath**, **SetClip/ResetClip**, **FillHatch** (6 pola), **DrawString + Font (8×8 embedded) + MeasureString**. **Enhance visual ✓:** **FillRoundedRectangle/DrawRoundedRectangle** (sudut AA), **FillRoundedGradientV** (tombol), **DrawShadow** (drop shadow lembut berlapis), **FillCircleAA** (lingkaran anti-alias), BlendCov (coverage AA). Menyusul: Region non-rect, PointF/RectangleF, texture brush, Pen dash/cap, AA garis/teks penuh
- [~] **Bitmap ✓** (uint[] ARGB, Get/SetPixel, Clear) · **Color ✓** (ARGB, FromArgb/FromRgb, A/R/G/B, named) · **Point/Size/Rectangle ✓** · **Matrix ✓** · **GraphicsPath ✓** · **Font ✓** (8×8, ASCII subset) · **BMP 24-bit load/save ✓**. Menyusul: LockBits, PNG, HSL, Image crop/rotate, font-pack penuh
- [ ] Pen (width/dash/cap/join) · Brush (solid/gradient/hatch) · GraphicsPath · Region · Matrix
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
- [~] **Preloaded suite ✓ (mulai, `panen_suite_demo`):** (1) **Kalkulator** (`calc.cs`→CALC.ELF) — `Buitenzorg.UI` Grid 4×4 tombol ter-tema (digit biru + operator oranye) di atas display numerik; klik via `Button.Tag`→engine `Calc`; `12+3=`→15 → `MILESTONE: CALC OK`; screenshot `docs/img/desktop-calc.png`. (2) **2048** (`game2048.cs`→G2048.ELF) — game geser-ubin: papan 4×4 `Board2048:UIElement` (ubin rounded berwarna per-nilai + angka via DrawChars), engine slide+merge per arah; `2+2`→4 & `4 4 8 8`→`8 16` → `MILESTONE: GAME OK`; screenshot `docs/img/desktop-2048.png`. (3) **Jam** (`clock.cs`→CLOCK.ELF) — jam analog: face AA + 12 tick + jarum jam/menit/detik (rotasi via `SinFx/CosFx`) + digital HH:MM:SS (DrawChars); verified geometri jarum + format waktu → `MILESTONE: CLOCK OK`; screenshot `docs/img/desktop-clock.png`. (4) **Piano** (`piano.cs`→PIANO.ELF) — keyboard 1 oktaf: `Piano:UIElement` tuts putih (rounded + label C-D-E-F-G-A-B-C) + tuts hitam, klik tuts → `Mixer.Beep(freq)` (Buitenzorg.Audio); verified arpeggio C-E-G-C (4 nada) → `MILESTONE: PIANO OK`; screenshot `docs/img/desktop-piano.png`. (5) **App Store** (`store.cs`→STORE.ELF) — store front **TERHUBUNG ke `pkg.rs`**: katalog dari syscall **`PKG_LIST`** (registry + status terpasang), install/hapus via **`PKG_SET`** (gating `run`); `StoreView:UIElement` (nama + kategori + badge TERPASANG/TERSEDIA) + tombol PASANG/HAPUS; verified: load katalog + install app + **baca ulang registry buktikan state kernel berubah** → `MILESTONE: STORE OK`; screenshot `docs/img/desktop-store.png`. ABI: `PKG_LIST`=27/`PKG_SET`=28 (+`PkgInfo` 48B, mirror C# + kontrak 9+4 lulus + `docs/abi.md`); registry (`pkg.rs`) diperluas + `category`. (6) **File Manager** (`filemgr.cs`→FILES.ELF) — jelajah VFS via syscall **`FS_LIST`** (path kosong=daftar mount, path mount=daftar file); `FileView:UIElement` (path bar + ikon folder/file + nama) + navigasi (klik folder / `..`); verified: daftar mount + masuk `/disk` + cek `CALC.ELF` ada → `MILESTONE: FILES OK`; screenshot `docs/img/desktop-files.png` (isi `/disk` NYATA dari FAT). ABI: `FS_LIST`=29 (+`FsEntry` 32B, mirror + kontrak lulus). (7) **Text Editor** (`editor.cs`→EDITOR.ELF) — editor multi-baris: `TextArea:UIElement` (buffer char growable, caret, wrap `\n`, Type/Newline/Backspace) di bawah `Menu` bar; verified ketik 2 baris + backspace → cek buffer → `MILESTONE: EDITOR OK`; screenshot `docs/img/desktop-editor.png`. **SUITE OK** (4-media, kernel-side).
- [x] **Suite bawaan (8 app, semua kategori):** productivity (Kalkulator, Editor), game (2048), utilitas (Jam, File Manager), multimedia (Piano, Image Viewer), store (App Store) — di atas BCL v0.15 + Drawing/UI/Audio; masing-masing verified + screenshot. Menyusul (opsional): AI app, themes bawaan tambahan
- [x] **Image Viewer** (`imgview.cs`→IMGVIEW.ELF) — memuat `/disk/PHOTO.BMP` lewat syscall baru **`FS_READ`** (ABI 30: baca isi file VFS ke buffer klien — juga fondasi file-open untuk editor/apps lain), dekode via `Bmp.Load` (Buitenzorg.Drawing), tampil di `ImageView:UIElement` (fit-to-box + aspect ratio, backdrop papan-catur, caption nama+dimensi); verified dekode 320×200 + piksel non-trivial → `MILESTONE: IMGVIEW OK`; screenshot `docs/img/desktop-imgview.png`. ABI: `FS_READ`=30 (mirror C# + kontrak dua sisi + `docs/abi.md`).
- [x] **Bugfix ABI syscall userland (ditemukan oleh Image Viewer):** inline-asm `syscall` di shim (`bzstart.rs`) mendeklarasikan rdi/rsi/rdx sebagai `in` (janji "tidak berubah") — padahal kernel entry memanggil dispatcher C dan hanya me-restore rcx/r11/rsp sebelum `sysretq`. LLVM menyimpan `chunk` di rdi melintasi syscall mmap di `SystemNative_Malloc`, `HEAP_CAP` terisi sampah, grow-check tidak pernah aktif lagi → alokasi besar KEDUA menabrak akhir chunk yang benar-benar termap (page fault USER). Fix: rdi/rsi/rdx dideklarasi clobbered (`inlateout => _`) — kelas bug yang sama dengan fix r8/r9/r10 sebelumnya. `heap.cs` kini menguji multi-growth (4×400 KiB) agar path ini tetap teruji; heap juga disederhanakan (mmap-first, tanpa arena .bss; kursor `AtomicUsize`).
- [~] **Optimization pass (mulai):** fast boot — pangkas durasi `Sleep()` animasi demo app (xox/paint/taskmgr/widget/webview) dari ~12 s → ~3 s; boot **69.8 s → 58.5 s** (~16% lebih cepat), semua milestone tetap. Kernel cetak estimasi tick di akhir boot. **Catatan fast-I/O:** dicoba multi-sector IDE-PIO + in-memory FAT + contiguous-run batching di `read_file` — ternyata **2× lebih lambat** di QEMU (emulasi PIO per-sector QEMU sudah cepat; baca-seluruh-FAT per file + churn buffer justru menambah overhead) → **di-revert**. Menyusul: startup app, footprint, SIMD hot-path, benchmark regresi CI

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
    - `editor.cs` → **`BzFile`**: menu FILE jadi **NYATA** — Ctrl+S simpan & Ctrl+O buka `/ram/NOTE.TXT`; demo boot menyimpan lalu membaca ulang dan membandingkan (round-trip masuk kriteria `MILESTONE: EDITOR OK`)
  - **Sengaja TIDAK diadopsi** (menautkan ~30 KB library hanya demi satu formatter angka = rugi; formatternya lokal, dipakai di dalam `UIElement` custom): `calc.cs`, `game2048.cs`, `piano.cs`, `paint.cs`, `webview.cs`, `xox.cs`, `audiopanel.cs`. `hello.cs`/`svc.cs` sengaja minimal (milestone v0.4/v0.5)
  - **2 bug ketemu saat audit:** (1) `scripts/build-hello-csharp.sh` (jalur Linux/CI) **tidak punya `imgview` & `jpgtest`** — hanya ditambahkan ke `.ps1`, jadi build Linux menghasilkan image tanpa IMGVIEW.ELF/JPGTEST.ELF dan smoke CI akan gagal di marker itu; kedua skrip kini sama-sama 27 program. (2) `sdk/templates/desktop-csharp/README.md` masih menulis "belum punya GC — **hindari `new T[]`**", padahal heap sudah jalan sejak v0.15 inc 4 — diganti dengan aturan zerolib yang benar + katalog library
  - **Tool host:** `MagicAppGen` — `GetApiReference("bcl")` diperluas jadi katalog tanda-tangan **lengkap** `bzbcl.cs`+`bzbcl2.cs` (12 namespace, termasuk batas jujur "UDP loopback saja / tak ada TCP" dan "`Collect()` = false"), dan system prompt di `app.config` menyebut `bzbcl2.cs` + menyuruh panggil `GetApiReference` dulu — tanpa ini AI menghasilkan kode yang tak tahu System.IO/Regex/dll ada. `docs/first-app.md` dapat tabel katalog namespace baru. `bz` CLI tak butuh perubahan (scaffolding manifest/template, tak menyentuh API app)

### 🚀 Stabilisasi & Rilis (v1.0) — *sedang berjalan*
- [x] **Security hardening: validasi pointer user di syscall ✓ (2026-07-24)** — **LUBANG NYATA ditutup.** Sebelumnya syscall menyalin lewat pointer mentah tanpa cek, jadi app ring-3 bisa: (a) `DEBUG_WRITE(alamat_kernel)` → kernel mencetak memorinya sendiri ke serial (**bocor info**); (b) `SYS_STAT`/`PROC_LIST`/`PKG_LIST`/`FS_READ`/`NET_RECV` dengan `out_ptr` = alamat kernel → kernel **menulis hasil ke memori kernel** (**tulis-sembarang = eskalasi privilege penuh**); (c) pointer tak-terpeta → kernel **page fault di dalam syscall lalu mati**. Fix: `memory::validate_user_range(ptr,len,need_write)` (tanpa wrap + seluruhnya < `USER_ADDR_MAX`=0x8000_0000 + **tiap halaman present & USER_ACCESSIBLE** + writable utk buffer keluaran) & `validate_user_cstr` (path NUL-term, dicek ulang tiap batas halaman). Dipasang di **SEMUA** syscall berpointer: DEBUG_WRITE/FB_INFO/WIN_CREATE/WIN_CMD (termasuk buffer BLIT & teks)/PROC_LIST/SYS_STAT/AUDIO_STAT/AUDIO_PLAY/FUTEX_WAIT/PKG_LIST/PKG_SET/FS_LIST/FS_READ/FS_WRITE/CLOCK_RTC/NET_SEND/NET_RECV/NET_INFO. Dua TODO lama di `sys_win_create`/`sys_debug_write` ("full address-space validation arrives with multi-process isolation") akhirnya ditepati. **Penting:** cek hanya di jalur ring-3 (`dispatch_from_user` dari entry SYSCALL); pemanggil kernel-internal (`dispatch`) kirim alamatnya sendiri & tetap dipercaya — ini ketahuan saat `syscall_smoke_test` kernel sendiri langsung ditolak validator (bukti validatornya bekerja). Verifikasi: `syscall::security_self_test()` menembak **14 probe bermusuhan** (alamat kernel, halaman tak-terpeta, rentang meluap, panjang wrap, null) — semua ditolak `INVAL` → **`MILESTONE: SECURITY OK`**; smoke 4-media LOLOS, 0 fault
- [x] **Stabilkan ABI ✓ (2026-07-24)** — tabel v1 **dibekukan**: `abi_v1_is_frozen` (Rust, 6 test) + `AbiV1IsFrozen` (C#, 12 test) memaku `ABI_VERSION`, `COUNT`=40, **ukuran + alignment 10 struct** lintas-batas, dan kode error. Renumber/ubah-layout gagal di test sebelum bisa masuk image rilis. Kebijakan didokumentasikan di `docs/abi.md` (§Pembekuan ABI v1): tambah = append + naikkan COUNT + update kedua mirror; ubah yang ada = versi mayor baru
- [x] **Benchmark regresi di CI ✓ (2026-07-24)** — `scripts/bench.sh`: boot headless lalu ambil dua angka yang memang sudah dicetak kernel (boot-to-READY dalam tick, throughput async I/O ops/s) dan gagal bila lewat budget (`BOOT_BUDGET_S`=90, `AIO_MIN_OPS`=10000; longgar sengaja — menangkap regresi nyata, bukan jitter runner). Job baru di `ci.yml`. Logika parsing diverifikasi terhadap log boot sungguhan (boot=45s, aio=36418 ops/s) — uji itu **menemukan bug pola sed**: kernel mencetak `~N ops/sec` atau `>N ops/sec` tergantung resolusi tick, pola awal cuma menangani `>`
- [x] **CI diperbaiki ✓ (2026-07-24)** — **BUG NYATA:** `ci.yml` memicu di `branches: [main]` padahal default branch repo ini **`master`**, jadi **CI tak pernah jalan sekali pun saat push**. Sekarang `[master, main]` + `workflow_dispatch`
- [x] **Debugger + profiler ✓ (2026-07-24)** — **Debugger:** `scripts/debug-kernel.ps1`/`.sh` boot QEMU ditahan dgn GDB stub `tcp:1234` lalu attach GDB pakai simbol kernel (`bzkernel` ELF tak-strip) + `scripts/debug-kernel.gdb` (helper `bz-break-main`/`bz-faults`/`bz-regs`; simbol Rust ter-mangle, GDB demangle otomatis). Fallback cetak perintah attach manual kalau `gdb` tak ada. Diverifikasi: GDB stub QEMU merespons RSP stop-reply `T05` (SIGTRAP) via probe TCP; simbol `kernel_main`/`page_fault_handler`/`double_fault_handler` ada di ELF (via llvm-objdump). **Profiler:** `profile.rs` — profiler zona ter-instrumentasi berbasis **TSC**, `Guard::new("nama")` scope-timer (inert saat off = 1 atomic load, tak ganggu boot), registry 64-zona di spin-lock (interrupt off, aman thd timer IRQ), `report()` tabel terurut (calls/total/avg/max/share permille tanpa float). Instrumentasi jalur panas: `syscall` (dispatch_from_user), `wm::compose`, `fb::present`. Shell `prof [self|on|off|reset|report]`. Verifikasi `profiler_demo()`: 3 zona bersarang rasio biaya 20x, assert jumlah panggilan tepat (20 tiap) + cheap<expensive + outer mengurung + zona-saat-off tak terekam → **`MILESTONE: PROFILER OK`** (laporan nyata: demo-outer 50.1% / demo-expensive 47.4% / demo-cheap 2.4%, rasio 19.7x). Docs `docs/debugging.md`. Smoke 4-media LOLOS. *(DAP breakpoint penuh di VS Code masih menyusul — GDB kernel-level sudah lengkap.)*
- [x] **Dokumentasi/tutorial rilis ✓ (2026-07-24)** — **`CHANGELOG.md`** (riwayat rilis per codename v0.1→jalur v1.0, format Keep-a-Changelog, tiap milestone + marker), **`docs/tutorial.md`** (tutorial berurutan 8-langkah: build→keliling desktop→shell→bikin app→pakai library→debug/profil→bawa keluar QEMU, menautkan doc mendalam tiap bagian), **`docs/README.md`** (indeks navigasi seluruh docs berkelompok: mulai/menjalankan/referensi/perencanaan). **README di-refresh** (status usang "v0.1–v0.14" → "v0.1–v0.16 ✓ + jalur v1.0", boot log diperbarui s/d SECURITY/PROFILER OK, tautan tutorial+CHANGELOG+docs-index). Semua cross-link diverifikasi resolve (docs/README, tutorial, CHANGELOG, README — 0 broken), perintah tutorial (`run calc`/`editor`, `prof self`, `vm create/start nanovm`, `bz model list`) diverifikasi ada di kernel. **Lisensi kini DITETAPKAN: MIT** (berkas `LICENSE`, © 2026 Gravicode Studios — atas permintaan pemilik 2026-07-24; CHANGELOG/README/PLAN diperbarui). Tak ada kode berubah (build/smoke tak terpengaruh)
- [~] **Boot hardware nyata — perkakas & dokumentasi SIAP ✓ (2026-07-24), validasi mesin fisik menyusul.** Image build (`buitenzorg-bios.img` MBR / `buitenzorg-uefi.img` GPT+ESP) memang raw & langsung ditulis ke USB. Skrip flash **`scripts/flash-usb.ps1`** (Windows, akses disk mentah via PhysicalDrive) + **`.sh`** (Linux/macOS `dd`): pengaman berlapis (hanya disk USB/removable ditawarkan, target eksplisit bukan ditebak, disk sistem/boot DITOLAK mentah, konfirmasi ketik ERASE, **verifikasi baca-ulang** byte-per-byte) + mode `-List`/`--list`. Panduan **`docs/install-hardware.md`**: pilih firmware (bios↔Legacy/CSM, uefi↔UEFI, Secure Boot OFF), jalur GUI (Etcher/Rufus DD-mode) sbg alternatif, boot menu, **checklist verifikasi HW jujur** (PS/2 vs USB-HID, IDE-PIO vs NVMe, PIC/PIT vs APIC, ACPI shutdown) + tabel kompatibilitas + troubleshooting. Di-cross-link dari README + getting-started. `.ps1` parse-clean, `.sh` `bash -n` OK, enumerasi disk diuji (disk sistem SATA ke-0 benar ter-flag `IsSystem/IsBoot` → ditolak guard). **Belum: boot di mesin fisik nyata** (tak bisa dari lingkungan ini) — ditandai eksperimental
- [x] **Image resmi Hyper-V (VHDX) ✓ (2026-07-24)** — `make-vm-images.ps1`/`.sh` kini juga emit `dist/buitenzorg.vhdx` untuk Hyper-V. **Gotcha nyata ditangani:** konversi `qemu-img -O vhdx` telanjang menghasilkan virtual-size 5,47 MiB ganjil (bisa ditolak Hyper-V), DAN qemu build ini tak bisa `resize` vhdx ("format driver does not support resize") → solusi: **pre-create VHDX 64 MiB lalu `convert -n`** (stream raw ke disk yang sudah berukuran). Verified: virtual-size 67108864 B (64 MiB, 1MiB-aligned), dinamis 7 MiB di disk, konten prefix identik dgn source (convert-back + cmp). Helper baru **`scripts/make-hyperv-vm.ps1`** membuat VM **Generation 1** (BIOS/MBR — Gen 2 UEFI tak cocok disk MBR unsigned) dari VHDX; guarded (cek modul Hyper-V + admin), fallback cetak langkah manual kalau Hyper-V mati (diverifikasi: New-VM absen di lingkungan dev → fallback jalan bersih). Docs `run-in-vm.md` dapat bagian Hyper-V + troubleshooting. **Belum: boot di Hyper-V nyata** (feature tak aktif di lingkungan dev) — konversi VHDX + pembuatan VM terverifikasi, boot aktual menyusul spt VMware/VBox

### 🌍 Multi-Arch (v1.x)
- [ ] Port ARM64 · Port RISC-V · SMP/NUMA · marketplace · container lanjut

### 🧪 Eksperimen pasca-v1.0 — link CoreLib .NET ASLI (`--stdlib:dotnet`)
Proyek riset tersendiri (multi-sesi, hasil tak pasti); ganti Buitenzorg.Bcl
tulisan-sendiri dgn BCL resmi (LINQ/Regex/Tasks/StringBuilder). Jalur dipetakan
di v0.15 inc 6 (lihat PLAN.md §Eksperimen pasca-v1.0):
- [ ] Link statik-freestanding (bukan dinamis-glibc default) + crt/_start sendiri
- [ ] Glue entry internal bflat (`__managed__Startup`/`RhInitialize`)
- [ ] Sisa ~150 simbol PAL (stdio/TLS `__tls_get_addr`/pthread_key/`LowLevelMonitor_*`/new-delete C++) — banyak bisa stub; inti (mmap/thread/futex/clock/malloc) SUDAH dari v0.15
- [ ] Link `libbootstrapperdll.o`+`libRuntime.WorkstationGC.a` lalu tembus fault startup GC/EEType/cctor
