# Debugging & Profiling

Dua alat pengembang untuk kernel Buitenzorg (v1.0):

1. **Debugger** — attach GDB ke kernel yang berjalan di QEMU (breakpoint,
   single-step, inspeksi register/memori di ring 0).
2. **Profiler** — profiler zona ter-instrumentasi berbasis TSC di dalam kernel
   (ukur ke mana siklus CPU dihabiskan, deterministik).

[English](debugging.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

---

## 🐞 Debugger (GDB + QEMU)

QEMU menyediakan **GDB stub**: kernel di-boot **ditahan** (paused) dengan server
GDB di `tcp:1234`, lalu GDB di-attach memakai **simbol kernel** dari ELF
`kernel/target/x86_64-unknown-none/release/bzkernel` (tidak di-strip).

### Cara cepat (skrip)

**Windows:**
```powershell
.\scripts\debug-kernel.ps1            # image BIOS, auto-attach GDB
.\scripts\debug-kernel.ps1 -Uefi      # image UEFI
.\scripts\debug-kernel.ps1 -NoAttach  # QEMU ditahan saja; attach GDB sendiri
```
**Linux / macOS:**
```bash
./scripts/debug-kernel.sh             # image BIOS, auto-attach GDB
./scripts/debug-kernel.sh --uefi
./scripts/debug-kernel.sh --no-attach
```

Skrip: (1) mencari ELF kernel ber-simbol (release, fallback debug), (2)
menjalankan QEMU dengan `-gdb tcp::1234 -S` (ditahan), (3) meng-attach GDB dengan
`scripts/debug-kernel.gdb` + `target remote :1234`. Kalau `gdb` tak ada di PATH,
skrip tetap menjalankan QEMU dan mencetak perintah attach manual.

Prasyarat: kernel sudah di-build (`build.ps1`/`build.sh`) dan **`gdb`** (atau
`gdb-multiarch` di Linux) tersedia. QEMU dideteksi otomatis (atau env `QEMU`).

### Sesi khas

```gdb
(gdb) bz-break-main        # break di kernel_main (helper Buitenzorg)
(gdb) continue             # jalankan sampai entry
(gdb) bt                   # backtrace
(gdb) info registers rip rsp
(gdb) stepi                # satu instruksi
(gdb) break page_fault_handler
(gdb) x/8i $pc             # disassemble 8 instruksi di PC
```

`scripts/debug-kernel.gdb` menambah helper ini:

| Perintah | Fungsi |
|---|---|
| `bz-break-main` | break di `kernel_main` (tepat setelah handoff bootloader) |
| `bz-faults` | break di handler page/double/GP fault — berhenti di debugger, bukan dump rodata |
| `bz-regs` | dump ringkas register umum |
| `bz-help` | daftar helper |

Simbol Rust ter-mangle (`_RNvCs...8bzkernel11kernel_main`); GDB men-demangle
otomatis, jadi `break kernel_main` / `break page_fault_handler` tetap jalan.

> **Cara manual** (tanpa skrip): jalankan `QEMU_EXTRA="-s -S"` lewat runner biasa
> (mis. `cargo run -p bzimage -- --run`), lalu dari terminal lain:
> ```
> gdb -x scripts/debug-kernel.gdb kernel/target/x86_64-unknown-none/release/bzkernel
> (gdb) target remote :1234
> ```

### Alternatif: debug lewat serial

Semua log kernel dicetak ke **serial** (COM1) dan framebuffer console. Untuk
tracing cepat tanpa GDB, `println!` di kernel muncul di serial — skrip smoke &
runner sudah mengarahkannya. Ini sering lebih cepat daripada breakpoint untuk
memverifikasi alur boot.

## 📊 Profiler (zona TSC ter-instrumentasi)

Kernel punya profiler zona ringan (`kernel/bzkernel/src/profile.rs`): bungkus
sebuah scope dengan `profile::Guard::new("nama")`, dan siklus CPU yang berlalu
(dari **timestamp counter**) diakumulasi ke bucket per-nama. `profile::report()`
mencetak tabel terurut — jumlah panggilan, total/avg/max siklus, dan porsi dari
total.

Karakteristik:

- **Inert saat mati.** `Guard::new` hanya membaca satu atomic ketika profiler off,
  jadi instrumentasi yang ditinggal di kode **tidak mengganggu timing boot
  normal**. Nyalakan dengan `profile::enable()` di sekitar area yang diukur.
- **Deterministik**, bukan statistik: mengukur siklus wall inklusif sebenarnya
  dari tiap scope — sebuah run headless bisa meng-assert jumlah panggilan & biaya
  relatif yang tepat (beda dari sampling profiler).
- **Single-core / kooperatif.** Registry di balik spin lock (interrupt dimatikan
  saat lock), aman terhadap timer IRQ; bukan untuk dipanggil dari interrupt
  handler.

### Titik instrumentasi bawaan

Sudah dipasang di jalur panas (inert kecuali di-enable):

| Zona | Lokasi |
|---|---|
| `syscall` | total waktu melayani syscall ring-3 (`dispatch_from_user`) |
| `wm::compose` | compositor menyusun frame (bagian terdalam `WIN_PRESENT`) |
| `fb::present` | blit back buffer ke framebuffer |

### Dari shell

```
prof self       # profil workload nyata (recompose desktop 8x) lalu laporkan
prof on         # aktifkan profiler
prof off        # matikan
prof reset      # bersihkan akumulasi
prof report     # cetak tabel (ke serial log)
```

`prof self` menyalakan profiler, memanggil `wm::present_now()` beberapa kali,
lalu mencetak laporan — cara cepat melihat biaya recompose desktop. Laporan
lengkap ada di **serial log** (tabelnya lebar untuk terminal desktop).

### Contoh laporan (dari self-test boot)

```
[profile] zone report (3 zones, 492164098 total cycles):
[profile] zone                        calls          total          avg          max  share
[profile] demo-outer                     20      246925905     12346295     13223062   50.1%
[profile] demo-expensive                 20      233384338     11669216     12610684   47.4%
[profile] demo-cheap                     20       11853855       592692       726510    2.4%
```

Self-test boot (`profiler_demo` di `main.rs`) menjalankan zona bersarang dengan
rasio biaya diketahui (satu scope spin 20x lebih banyak), lalu meng-assert:
jumlah panggilan tepat, zona murah < zona mahal, outer mengurung keduanya, dan
zona yang direkam saat profiler **off** tak muncul → `MILESTONE: PROFILER OK`.

### Menambah instrumentasi

```rust
fn hot_path() {
    let _z = crate::profile::Guard::new("hot_path");
    // ... kerja ...
}   // Guard drop di sini mencatat siklus yang berlalu
```

Nama zona dibandingkan **per-isi** (bukan pointer), jadi literal sama di call site
berbeda menyatu ke satu bucket. Maksimum 64 zona berbeda; kelebihannya dihitung di
`overflow` dan dilaporkan (degradasi berisik, bukan diam-diam).

Batasan: mengukur waktu **inklusif** scope (termasuk anak-anaknya); rekursi zona
bernama sama menghitung ganda. Untuk "self time", pisahkan kerja anak ke zona
bernama sendiri (seperti `wm::compose` vs `fb::present`).

## ⏱️ Profiler untuk aplikasi (`prof app`, `bz prof`)

Di mana *program saya* menghabiskan waktunya? Profiler per aplikasi
(`kernel/bzkernel/src/appprof.rs`) mengambil sampel satu proses: di setiap
interupsi timer, di CPU mana pun, yang jatuh di proses itu, ia mencatat
program counter (atau satu tik "kernel" kalau program sedang di dalam
syscall). Laporannya mengelompokkan sampel per fungsi, memakai simbol yang
disimpan kernel untuk laporan crash:

```text
> prof app crashme spin
prof: memprofil pid 1000 'crashme' ('prof stop' atau tunggu ia selesai, lalu 'prof last')
> prof last
profil pid 1000 'crashme': 149 sampel dalam 1506 ms (147 di program, 2 di kernel/syscall)
   sampel     %  fungsi
      118  79.1  crashme_Crash__Hot
       28  18.7  crashme_Crash__Cold
        2   1.3  [kernel: syscall]
        1   0.6  bz_clock_nanos
```

| Perintah | |
|---|---|
| `prof app NAMA [arg]` | jalankan program lalu profil |
| `prof pid N` | profil proses yang sudah berjalan |
| `prof show` / `prof stop` | laporan sejauh ini / berhenti + laporan |
| `prof last` | laporan terakhir yang selesai (juga dicetak ke log saat program berakhir) |
| `bz prof APP [arg] [--seconds N]` (PC) | sama, lewat bz-link: menunggu program selesai (atau N detik) lalu mencetak laporannya |
| `bz prof --pid N [--seconds N]` (PC) | profil proses yang sedang berjalan |

Batasan: hanya program native (ELF dengan tabel simbol; aplikasi managed atau
skrip tampil sebagai fungsi interpreter di CLRHOST.ELF yang di-strip --
`(tanpa simbol)`); satu proses sekaligus; laju sampel = laju timer (100 Hz di
ARM64/RISC-V dan CPU sekunder x86, 18,2 Hz di CPU 0 x86), jadi profil program
yang berjalan sedetik atau lebih; paling banyak 16.384 sampel. Boot
self-test memprofil `crashme spin` (1,2 dtk di `Hot`, 0,3 dtk di `Cold`) dan
mewajibkan `Hot` dominan -> `MILESTONE: APP PROFILER OK`.

## 🪲 Debugger untuk aplikasi (`dbg`)

Program ring-3 (ELF native: C# dari bflat, Rust, ...) bisa dijalankan di
bawah debugger dari Terminal -- atau dari PC dengan `bz run "dbg ..."`
([bz-link](bz-link.id.md)):

```text
> dbg run crashme
pid 1000 stopped (entry) at 0x40008c  _start
> dbg break Level3
breakpoint 1 at 0x401480  crashme_Crash__Level3
> dbg cont
pid 1000 stopped (breakpoint) at 0x401480  crashme_Crash__Level3
> dbg bt
backtrace (return addresses found on the stack, innermost first):
  #0  0x401480  crashme_Crash__Level3
  #1  0x4014a6  crashme_Crash__Level2+0xa
  #2  0x4014b4  crashme_Crash__Level1+0x8
> dbg cont
pid 1000 stopped (fault) at 0x401481  crashme_Crash__Level3+0x1
> dbg cont
pid 1000 exited with code 0xdead
```

| Perintah | Fungsi |
|---|---|
| `dbg run APP [arg]` | jalankan program di bawah debugger; berhenti di entry-nya |
| `dbg break SIM\|ALAMAT` | breakpoint pada fungsi (namanya atau bagian unik dari namanya) atau alamat `0x` |
| `dbg delete SIM\|ALAMAT`, `dbg breaks` | hapus / daftar breakpoint |
| `dbg cont`, `dbg step` | lanjut ke breakpoint berikutnya / satu instruksi |
| `dbg regs`, `dbg set REG NILAI` | semua register / ubah satu |
| `dbg mem ALAMAT [N]` | memori dalam hex + teks (breakpoint ditampilkan sebagai byte aslinya) |
| `dbg bt` | backtrace bersimbol (seperti di laporan crash) |
| `dbg kill`, `dbg detach` | akhiri program / biarkan jalan tanpa breakpoint |
| `dbg`, `dbg pid N` | program yang di-debug / mana yang dituju perintah |

Program yang **fault** saat di-debug berhenti di sana (`stopped (fault)`):
periksa dengan `bt` / `mem`, lalu `cont` mengakhirinya.

Cara kerjanya: breakpoint berupa `int3` (x86), `brk #0` (ARM64) atau
`c.ebreak` (RISC-V) yang ditulis ke halaman kode. Program yang berhenti
menunggu *di task-nya sendiri* di dalam kernel, dengan address space-nya
terpasang, dan menjalankan sendiri setiap permintaan `dbg`. Single step: trap
flag x86, software step ARM64; RISC-V tidak punya dari S-mode, jadi satu step
memasang breakpoint sementara di setiap instruksi berikut yang mungkin
(lanjutan dan target cabang / lompatan). App managed dan skrip berjalan di
dalam `CLRHOST.ELF` dan tidak di-debug di tingkat kode sumbernya. Boot
self-test menjalankan satu sesi utuh -> `MILESTONE: DEBUGGER OK`.

## 🧭 VS Code: breakpoint di perangkat yang berjalan (`bz dap`)

Ekstensi VS Code (`sdk/vscode-extension`) men-debug program **di perangkat
Buitenzorg yang sedang berjalan** -- QEMU atau hardware nyata -- lewat
`bz dap`, server Debug Adapter Protocol di CLI `bz` (`sdk/bz/Dap.cs`) yang
bicara dengan perangkat lewat bz-link ([bz-link](bz-link.id.md)).

1. Di perangkat: `deploy on` (menampilkan kode pairing). Di PC: `bz device
   connect <ip> <kode>` -- atau isi `"device"` dan `"code"` di `launch.json`.
2. Buka berkas `.js`, `.ts`, `.py` atau `.csx`, klik di gutter untuk memasang
   breakpoint, tekan **F5** (tanpa `launch.json` pun bisa: berkas aktif yang
   di-debug).

| Program | Yang terjadi | Yang Anda dapat |
|---|---|---|
| **Skrip** (`.js .ts .py .csx`) | diunggah ke `/disk/<nama 8.3>`, dijalankan sekali di debugger perekam (`script debug LANG FILE all`) | breakpoint baris, continue, step over / in / out, **step back + reverse continue**, call stack, variabel lokal + global, hover, keluaran sesuai urutan cetaknya, galat runtime sebagai henti exception |
| **ELF native** (`"program": "crashme"`, atau `.ELF` lokal yang diunggah) | dijalankan dengan `dbg run` (berhenti di entry) | **breakpoint fungsi** (nama simbol, atau bagian unik darinya), continue, step = satu instruksi, step out = sampai pemanggil, call stack bersimbol, **register** (bisa diubah), Debug Console meneruskan teks ke `dbg` (`mem 0x400000 32`, `bt`) |

```jsonc
// .vscode/launch.json
{ "type": "buitenzorg", "request": "launch", "name": "Debug di perangkat",
  "program": "${file}",                 // atau "crashme" (native, menurut nama)
  "stopOnEntry": false,
  "device": "127.0.0.1:7070", "code": 4321 }   // opsional: selain itu ~/.bz/device.json
```

Di QEMU teruskan port-nya: `-netdev user,id=n0,hostfwd=udp::7070-:7070
-device e1000,netdev=n0`. Ekstensi menjalankan `dotnet dist/bz/bz.dll dap`
(atau `sdk/bz/bin/Debug/net10.0/bz.dll`, atau setelan `buitenzorg.bzPath`,
atau `bz` di PATH).

**Batasan (jujur).** Skrip *diputar ulang*: dijalankan sekali, lalu Anda
bergerak di dalam rekamannya -- tidak bisa mengubah nilai lalu melanjutkan,
dan hanya 400 henti pertama yang direkam. Skrip UI (yang membuka jendela)
berjalan di jendelanya di perangkat sambil merekam; launch menunggu sampai
jendelanya Anda tutup (atau skripnya selesai), lalu pemutaran ulang dimulai.
Program native tidak punya informasi baris (tidak ada
DWARF di perangkat), jadi breakpoint-nya nama fungsi dan frame-nya menunjukkan
alamat, bukan baris sumber; jeda (pause) tidak didukung (program berhenti di
breakpoint, saat fault, atau saat selesai). Aplikasi C# managed (RustCLR)
belum bisa di-debug di tingkat sumber. Pengujian: `bz dap --selftest`
(perangkat simulasi, CI) dan `bz dap --check` (perangkat yang tersambung;
langkah `[bzlink]` smoke test menjalankannya di x86 dengan skrip, skrip UI, dan
`crashme`). Ekstensinya sendiri tidak diuji oleh instance VS Code di CI --
adapternya yang diuji.

## 🔌 GDB ke perangkat yang berjalan (`gdbserver`)

`dbg` menjalankan debugger dari shell perangkat itu sendiri; `gdbserver`
menaruh mesin yang sama di balik **GDB Remote Serial Protocol** di sebuah port
TCP, sehingga GDB di PC bisa men-debug program yang berjalan di hardware nyata
(atau VM):

```text
(perangkat) gdbserver 1234 crashme
            gdbserver: pid 1000 stopped (entry) at 0x40008c  _start -- menunggu GDB di 10.0.2.15:1234
(PC)        gdb userland/hello-csharp/crashme.elf
            (gdb) target remote 192.168.1.30:1234    # QEMU: hostfwd=tcp::1235-:1234 -> localhost:1235
            (gdb) break Crash__Level2
            (gdb) continue
            (gdb) info registers / x/4xb $pc / stepi / bt / kill
            (gdb) monitor sym Crash__Level2          # pencarian simbol milik kernel
            (gdb) monitor bt                         # teks monitor lain diteruskan ke `dbg`
```

Paket yang didukung: `?`, `g`/`p` (register dalam urutan GDB -- amd64
rax..r15, rip, eflags; aarch64 x0..x30, sp, pc, cpsr; riscv64 x0..x31, pc),
`P`, `m` (breakpoint disembunyikan), `Z0`/`z0`, `c`, `s`, `k`, `D`, query
thread (satu thread), `QStartNoAckMode`, `qRcmd`. Belum: menulis memori
(`M`/`X`), watchpoint, Ctrl+C (jeda), banyak thread, attach ke proses yang
sudah berjalan, dan debugging kernel (tetap lewat stub QEMU, di atas). Satu
sesi sekaligus; program yang ditinggal GDB dalam keadaan berhenti diakhiri
bersama sesinya; tanpa GDB dalam 10 menit programnya juga diakhiri.

Diverifikasi dengan klien yang mengirim paket-paket GDB (`bz gdb-check`: 13
pemeriksaan, smoke `[bzlink]` di x86 bila `crashme` C# ada di image). **GDB
asli belum dijalankan terhadapnya di sini** (tidak terpasang di PC
pengembangan, dan image CI tidak punya program native) -- laporan dipersilakan.

## 💥 Laporan crash (program ring-3)

Saat sebuah program fault (pointer salah, alamat tak terpetakan, instruksi
tak dikenal), kernel hanya mengakhiri program itu -- dan menulis laporan:
jenis fault, pc / sp (plus link register di ARM64 / RISC-V), dan **backtrace
bersimbol**, ke log dan ke `/ram/CRASH/P<pid>.TXT` (`/disk/CRASH/` bila tidak
ada `/ram`).

```text
=== crash report: pid 1009 'crashme' ===
page fault (read), address 0x8
pc 0x401481  crashme_Crash__Level3+0x1
sp 0x7001ff90
backtrace (return addresses found on the stack, innermost first):
  #0  0x401481  crashme_Crash__Level3+0x1
  #1  0x4014a6  crashme_Crash__Level2+0xa
  #2  0x4014b4  crashme_Crash__Level1+0x8
  #3  0x4016a7  crashme__Module___StartupCodeMain+0x47
  #4  0x4000b2  _start+0x26
```

- Di Terminal: `crash` mendaftar laporan, `crash PID` / `crash last`
  menampilkan satu; dari PC, `bz pull /ram/CRASH/P1009.TXT` atau `bz logs`.
- **Simbol** diambil dari tabel simbol ELF program itu sendiri, disimpan saat
  program dimuat: C# native (bflat) menamai metode `assembly_Class__Method`,
  simbol Rust di-demangle (`crashme::level3`). Program yang di-strip hanya
  mendapat alamat.
- **Backtrace-nya hasil pindaian stack:** kode C# dan Rust hasil kompilasi
  tidak wajib menyimpan frame pointer, jadi kernel mencari kata di stack yang
  menunjuk ke kode program *tepat setelah instruksi call*. Itu menemukan
  pemanggilan yang aktif; alamat kembali basi di slot stack yang sudah mati
  bisa ikut muncul.
- Boot self-test sengaja membuat `CRASHME.ELF` crash dan mewajibkan ketiga
  tingkatnya ada di laporan: `MILESTONE: CRASH REPORT OK`.

## Ringkasan perintah

```powershell
# Debugger
.\scripts\debug-kernel.ps1                 # Windows  (Linux/macOS: debug-kernel.sh)

# Profiler (di shell OS)
prof self

# Debugger aplikasi (di shell OS, atau bz run "dbg ..." dari PC)
dbg run crashme   # lalu: break Level3, cont, bt, regs, step, kill, wait

# Debug adapter VS Code (di PC)
bz dap --selftest  # perangkat simulasi;  bz dap --check = yang tersambung
```

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
