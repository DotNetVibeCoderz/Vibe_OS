# Syscall ABI v1 — Kontrak Rust ↔ C#

Sumber kebenaran: [`kernel/abi/src/lib.rs`](../kernel/abi/src/lib.rs) (`bz-abi`).
Mirror C#: [`runtime/Buitenzorg.Runtime/Sys/`](../runtime/Buitenzorg.Runtime/Sys/).
Keduanya dijaga test kontrak identik (`cargo test -p bz-abi` ↔ `AbiContractTests.cs`).

## Aturan (requirements.md §4)

1. **C ABI saja** — `extern "C"` + P/Invoke (`[LibraryImport("bzsys")]`).
2. **Marshalling minimal** — hanya primitif, pointer, struct `#[repr(C)]` /
   `[StructLayout(LayoutKind.Sequential)]`.
3. **Nomor stabil** — append-only; tidak pernah dinomori ulang setelah rilis.
4. **Zero-copy** — data besar (framebuffer, file, tensor) lewat shared memory.
5. **GC-aware pinning** — objek managed di-`fixed`/pin selama pointer dipegang Rust.

## Tabel Syscall v1

| # | Nama | a0 | a1 | Hasil |
|---|---|---|---|---|
| 0 | `ABI_VERSION` | — | — | versi ABI (saat ini `1`) |
| 1 | `DEBUG_WRITE` | ptr (u64) | len (u64) | byte tertulis |
| 2 | `EXIT` | exit code | — | tidak kembali |
| 3 | `YIELD` | — | — | 0 |
| 4 | `TICKS` | — | — | tick timer sejak boot (PIT ~18,2 Hz) |
| 5 | `FB_INFO` | ptr → `FramebufferInfo` | — | 0 = sukses |
| 6 | `WIN_CREATE` | title ptr | title len | window id (a2=(w≪32)\|h) |
| 7 | `WIN_CMD` | window id | ptr → `DrawCmd` | 0 = sukses |
| 8 | `WIN_PRESENT` | window id | — | 0 (recompose desktop) |
| 9 | `KEY_READ` | — | — | 1 char (0 bila kosong) |
| 10 | `PROC_LIST` | buffer ptr (ProcInfo[]) | max count | jumlah entri ditulis |
| 11 | `PROC_KILL` | pid | — | 0 = sukses |
| 12 | `SYS_STAT` | ptr → `SysStat` | — | 0 = sukses |
| 13 | `MMAP` | size (u64) | prot (u64) | base VA (syserr di rentang tinggi bila gagal); a2 tak dipakai |
| 14 | `MPROTECT` | addr (u64) | size (u64) | 0 = sukses (a2 = prot) |
| 15 | `MUNMAP` | addr (u64) | size (u64) | 0 = sukses |
| 16 | `THREAD_CREATE` | entry rip (u64) | arg (u64) | thread id (a2 = user stack top); syserr bila gagal |
| 17 | `THREAD_JOIN` | thread id (u64) | — | 0 setelah thread selesai |
| 18 | `THREAD_EXIT` | exit code (u64) | — | tak kembali |
| 19 | `FUTEX_WAIT` | addr (u64) | expected (u64) | 0 (blok bila *addr==expected sampai di-wake) |
| 20 | `FUTEX_WAKE` | addr (u64) | count (u64) | jumlah thread yang dibangunkan |
| 21 | `THREAD_SELF` | — | — | id thread pemanggil |
| 22 | `CLOCK_MONO` | — | — | pencacah monotonik (siklus TSC) |
| 23 | `AUDIO_STAT` | ptr → `AudioInfo` | — | 0 = sukses |
| 24 | `AUDIO_SET_VOLUME` | volume 0..=100 (u64) | — | 0 = sukses (non-zero = un-mute) |
| 25 | `AUDIO_TONE` | frekuensi Hz (u64) | durasi ms (u64) | 0 = sukses (DMA, non-blocking) |
| 26 | `AUDIO_PLAY` | ptr PCM i16 stereo | panjang byte (u64) | 0 = sukses |
| 27 | `PKG_LIST` | ptr `PkgInfo[]` | max count (u64) | jumlah entri ditulis |
| 28 | `PKG_SET` | ptr nama | panjang nama (u64) | 0 = sukses (a2 = 1 pasang / 0 hapus) |
| 29 | `FS_LIST` | ptr path (NUL-term) | ptr `FsEntry[]` | jumlah entri (a2 = max; path kosong = daftar mount) |
| 30 | `FS_READ` | ptr path (NUL-term) | ptr buffer keluaran | jumlah byte dibaca (a2 = max byte; 0 = tidak ada) |
| 31 | `IS_INTERACTIVE` | - | - | 1 jika sesi interaktif (desktop hidup), 0 saat boot-demo headless |
| 32 | `FS_WRITE` | ptr path (NUL-term) | ptr buffer sumber | jumlah byte ditulis (a2 = jumlah byte; 0 = gagal/read-only) |
| 33 | `CLOCK_RTC` | ptr `RtcTime` | - | 0 = sukses (tahun/bulan/hari/jam/menit/detik dari CMOS RTC) |
| 34 | `NET_SOCKET` | kind (0 = UDP) | - | handle socket (>= 1), 0 = gagal |
| 35 | `NET_BIND` | handle | port | 0 = sukses |
| 36 | `NET_SEND` | handle | ptr `NetDatagram` + payload | jumlah byte payload terkirim (a2 = panjang payload) |
| 37 | `NET_RECV` | handle | ptr `NetDatagram` + ruang payload | panjang payload; 0 = tidak ada (non-blocking, a2 = max) |
| 38 | `NET_CLOSE` | handle | - | 0 = sukses |
| 39 | `NET_INFO` | ptr `NetInfo` | - | 0 = sukses (alamat + status + counter) |

## 🔒 Model Keamanan Pointer (hardening v1.0)

**Setiap pointer dari ring 3 tidak dipercaya.** Sebelum hardening ini, syscall
menyalin lewat pointer mentah apa adanya, sehingga app tak-istimewa bisa:

- `DEBUG_WRITE(alamat_kernel, len)` → kernel mencetak memorinya sendiri ke serial
  (**kebocoran informasi**);
- `SYS_STAT` / `PROC_LIST` / `PKG_LIST` / `FS_READ` / `NET_RECV`
  (`out_ptr = alamat_kernel`) → kernel **menulis hasil ke memori kernel**
  (**tulis-sembarang = eskalasi privilege penuh**);
- pointer tak-terpeta → kernel **page fault di dalam syscall** dan mati.

Sekarang `memory::validate_user_range(ptr, len, need_write)` mensyaratkan:

1. rentang tidak `wrap` dan seluruhnya **di bawah `USER_ADDR_MAX` = 0x8000_0000**
   (semua peta kernel — heap `0x4444_4444_0000`, jendela memori fisik, image
   kernel — ada di atasnya);
2. **setiap halaman** dalam rentang **present** dan ber-`USER_ACCESSIBLE`;
3. untuk buffer keluaran, halamannya juga **writable** (`user_write`).

`validate_user_cstr` melakukan hal sama untuk path NUL-terminated (dicek ulang
tiap batas halaman). Pengecekan hanya berlaku pada jalur **ring 3**
(`dispatch_from_user`, dipanggil dari entry SYSCALL); pemanggil kernel-internal
(`dispatch`) memang mengirim alamatnya sendiri dan tetap dipercaya.

Diverifikasi headless oleh `syscall::security_self_test()` — 14 probe bermusuhan
(alamat kernel, halaman tak-terpeta, rentang meluap, panjang wrap, null) harus
semuanya ditolak dengan `INVAL` → `MILESTONE: SECURITY OK`.

## 🧊 Pembekuan ABI v1 (v1.0)

Tabel v1 **beku**: nomor syscall append-only, layout struct tidak boleh berubah.
Penjaganya mekanis — `abi_v1_is_frozen` (Rust) + `AbiV1IsFrozen` (C#) memaku
`ABI_VERSION`, `COUNT`, serta **ukuran + alignment tiap struct** dan kode error.
Menambah syscall = tambahkan konstanta, naikkan `COUNT`, perbarui kedua test.
Mengubah syscall/struct yang sudah ada = **versi ABI mayor baru**, bukan edit.

**Kelengkapan BCL (pra-v1.0, permintaan 2026-07-24):** `FS_WRITE` melengkapi
`System.IO` (tulis file — butuh mount yang writable, mis. RAM disk FAT12 `/ram`);
`CLOCK_RTC` memberi `System.Globalization`/`BzDateTime` jam dinding sungguhan
(`rtc.rs`, CMOS, BCD/biner + 12/24 jam, dibaca ulang sampai dua sampel sama agar
tak ter-tear); `NET_*` memberi `System.Net.Sockets` socket **UDP nyata** di atas
stack loopback (`net.rs`: Ethernet/ARP/IPv4/ICMP + UDP dengan checksum
pseudo-header). Wrapper shim: `bz_fs_write`, `bz_clock_rtc`, `bz_net_socket`/
`bind`/`send`/`recv`/`close`/`info`.

**Batas jujur:** hanya **UDP** yang ada. `sock_kind::STREAM` (TCP) ditolak
dengan `INVAL`, jadi `System.Net.Http` (`BzHttp`) baru lapisan pesan
(build request / parse response), bukan klien — begitu TCP mendarat, klien =
`BzHttp` + stream. Perangkatnya juga masih **loopback**, belum ada driver NIC
(e1000 menyusul), jadi datagram hanya sampai ke mesin ini sendiri.

**Package manager (v0.16 "Panen" App Store):** `PKG_LIST` mengembalikan katalog
registry (`pkg.rs`) + status terpasang; `PKG_SET` memasang/menghapus paket by
nama (gating `run`). Wrapper shim: `bz_pkg_list`/`bz_pkg_set`.

**File I/O (v0.16 "Panen"):** `FS_LIST` menjelajah direktori VFS; `FS_READ`
membaca isi file ke buffer klien (mis. Image Viewer memuat `PHOTO.BMP`, editor
membuka file). Wrapper shim: `bz_fs_list`/`bz_fs_read`.

**Audio (v0.16 "Panen"):** driver AC'97 (`audio.rs`) — enumerasi PCI (kelas
0x04/0x01), cold-reset codec, mixer (master + PCM-out volume), dan playback PCM
16-bit stereo 48 kHz lewat DMA bus-master (buffer descriptor list). `AUDIO_TONE`
membangkitkan sinus di kernel; `AUDIO_PLAY` menyalin PCM klien ke buffer DMA.
Wrapper shim: `bz_audio_stat`/`bz_audio_set_volume`/`bz_audio_tone`/`bz_audio_play`;
library `Buitenzorg.Audio` (`Mixer`/`Tone`) di atasnya.

**Sync/TLS/clock (v0.15 "Matang" increment 3):** `FUTEX_WAIT`/`FUTEX_WAKE`
menambah state scheduler **Blocked** (thread benar-benar diblok, bukan busy-yield)
— fondasi mutex/cond. Shim menyediakan `bz_mutex_lock`/`bz_mutex_unlock` di
atasnya. `THREAD_SELF` = fondasi `pthread_self`/TLS. `CLOCK_MONO` = TSC (PAL
memasangkannya dengan frekuensi untuk Stopwatch/`GetTimestamp`).

**Threading (v0.15 "Matang" increment 2, kooperatif):** `THREAD_CREATE`
menjalankan `entry(arg)` di ring 3 pada stack `a2`, berbagi address space; thread
dijadwalkan kooperatif (yield lewat `YIELD`). Tiap thread punya SYSCALL kernel
stack sendiri (terpisah dari stack interrupt TSS). Wrapper shim: `bz_thread_create`
/`bz_thread_join`/`bz_thread_exit`/`bz_yield`.

> **Catatan register (penting):** syscall meng-clobber `rcx`+`r11` (instruksi
> `syscall`) **dan** `r8`/`r9`/`r10` + register caller-saved lain (kernel entry
> marshaling + C dispatch). Inline-asm syscall di sisi user harus mendeklarasikan
> `r8`/`r9`/`r10` sebagai clobbered, jika tidak nilai yang disimpan di register
> itu bisa rusak melintasi syscall.

**`prot`** (flag `mmap_prot`, OR-kan): `NONE=0`, `READ=1`, `WRITE=2`, `EXEC=4`.
`MMAP` (v0.15 "Matang") memetakan `ceil(size/4096)` halaman anonim di arena
mmap user (0x2000_0000..0x6000_0000), di-reset tiap proses. Ini fondasi memori
yang dipakai runtime .NET/GC untuk managed heap.

**Reserve/commit (increment 5):** `MMAP` dengan `prot=NONE` hanya **mereservasi**
rentang alamat (tanpa frame) — model yang dipakai GC .NET untuk memesan heap
besar di muka. `MPROTECT` dengan akси (READ/WRITE) **meng-commit on demand**:
halaman yang belum ter-map dapat frame zeroed baru; yang sudah ter-map hanya
di-re-flag. Jadi reservasi 256 MiB tak menghabiskan RAM fisik sampai
benar-benar dipakai.

Error dikembalikan di rentang atas `u64`: `NOSYS = u64::MAX`, `INVAL = u64::MAX - 1`.

## Struct Bersama

`FramebufferInfo` — `#[repr(C)]`, 7 × u64 = **56 byte**:
`address, size, width, height, stride, bytes_per_pixel, pixel_format`.
Pixel format: `0 = RGB`, `1 = BGR`, `2 = GRAY`, `255 = UNKNOWN`.

`DrawCmd` (v0.8) — `#[repr(C)]`, **48 byte**:
`op:u64, x,y,w,h:i32, color:u32, _pad:u32, text_ptr:u64, text_len:u64`.
Op: `0 = fill_rect`, `1 = draw_text`, `2 = clear`, `3 = line`, `4 = ellipse`,
`5 = fill_ellipse`, `6 = rect` (v0.9), `7 = blit` (v0.16). Warna `0x00RRGGBB`.
**`blit`**: `text_ptr` = buffer piksel ARGB klien (`w`×`h` `u32`, `text_len`
byte), disalin ke canvas window di (x,y). Ini fondasi renderer software
client-side `Buitenzorg.Drawing` (model kompositor WPF/Avalonia).

`ProcInfo` (v0.9) — `#[repr(C)]`, **64 byte**:
`pid:u64, state:u64, cpu_ticks:u64, kind:u64, name:[u8;32]`.
State: `0=runnable, 1=running, 2=finished`. Kind: `0=kernel task, 1=user app`.

`SysStat` (v0.9) — `#[repr(C)]`, **48 byte**:
`uptime_ticks, tick_hz, heap_used, heap_total, task_count, mem_total_mib` (semua u64).

`AudioInfo` (v0.16) — `#[repr(C)]`, **48 byte**:
`present, sample_rate, channels, bits, volume, muted` (semua u64).

`PkgInfo` (v0.16) — `#[repr(C)]`, **48 byte**:
`name:[u8;24], category:[u8;16], installed:u64`. Nama/kategori null-padded ASCII.

`FsEntry` (v0.16) — `#[repr(C)]`, **32 byte**:
`name:[u8;24], is_dir:u64`. Nama null-padded ASCII; `is_dir=1` untuk mount/direktori.
`present`/`muted` = 0/1; `volume` = 0..=100.

## Status Implementasi

- **Kernel** (`bzkernel/src/syscall.rs`): dispatcher lengkap untuk tabel v1,
  saat ini dipanggil dari konteks kernel (self-test saat boot).
- **C#** (`BzSys`): facade seragam — `NativeSyscalls` (P/Invoke `bzsys`, dipakai
  ketika berjalan di Buitenzorg) atau `HostSyscalls` (simulasi host untuk dev/test).
- **Belum ada**: entry point ring-3 (`syscall`/`sysret`) dan validasi pointer
  lintas address-space — menyusul bersama user-space task (v0.2 lanjutan → v0.4).

## Menambah Syscall Baru

1. Tambah konstanta di `bz-abi` (`sysno`) **di akhir tabel**, naikkan `COUNT`.
2. Mirror di `SyscallNumbers.cs`.
3. Implementasi di `bzkernel/src/syscall.rs` + (bila perlu) `HostSyscalls`.
4. Update test kontrak **di kedua sisi** dan tabel di dokumen ini.
