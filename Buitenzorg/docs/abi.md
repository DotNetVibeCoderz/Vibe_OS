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

Error dikembalikan di rentang atas `u64`: `NOSYS = u64::MAX`, `INVAL = u64::MAX - 1`.

## Struct Bersama

`FramebufferInfo` — `#[repr(C)]`, 7 × u64 = **56 byte**:
`address, size, width, height, stride, bytes_per_pixel, pixel_format`.
Pixel format: `0 = RGB`, `1 = BGR`, `2 = GRAY`, `255 = UNKNOWN`.

`DrawCmd` (v0.8) — `#[repr(C)]`, **48 byte**:
`op:u64, x,y,w,h:i32, color:u32, _pad:u32, text_ptr:u64, text_len:u64`.
Op: `0 = fill_rect`, `1 = draw_text`, `2 = clear`, `3 = line`, `4 = ellipse`,
`5 = fill_ellipse`, `6 = rect` (v0.9). Warna `0x00RRGGBB`.

`ProcInfo` (v0.9) — `#[repr(C)]`, **64 byte**:
`pid:u64, state:u64, cpu_ticks:u64, kind:u64, name:[u8;32]`.
State: `0=runnable, 1=running, 2=finished`. Kind: `0=kernel task, 1=user app`.

`SysStat` (v0.9) — `#[repr(C)]`, **48 byte**:
`uptime_ticks, tick_hz, heap_used, heap_total, task_count, mem_total_mib` (semua u64).

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
