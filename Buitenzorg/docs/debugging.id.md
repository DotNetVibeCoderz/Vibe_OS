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
```

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
