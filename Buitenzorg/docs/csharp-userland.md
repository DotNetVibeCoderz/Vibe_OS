# Menjalankan C# di ring 3 (v0.4 "Tunas")

Milestone v0.4: program **C#** yang dikompilasi ahead-of-time berjalan di
user-space (ring 3) di atas kernel Rust, dan memanggil kernel lewat syscall.
Inilah "Layer 4 — Managed Runtime" (requirements.md §3, §5.1) dalam bentuk
paling ringan: jalur **NativeAOT**, tanpa JIT, tanpa GC.

## Pipeline build

```
hello.cs ──bflat(ILC/NativeAOT, zerolib)──► hello.o ─┐
                                                     ├─rust-lld─► hello.elf (ELF statis)
bzstart.rs ──rustc(x86_64-unknown-none)──► bzstart.o ┘
```

- **bflat** (`--stdlib:zero`) mengompilasi C# ke objek freestanding: tanpa
  runtime .NET penuh, tanpa GC. `Console.Write` memanggil `SystemNative_Log`.
- **bzstart.rs** adalah shim startup + PAL: menyediakan `_start` (memanggil
  entry NativeAOT `__managed__Main`) dan `SystemNative_Log/Malloc/Abort` yang
  menerjemahkan ke syscall Buitenzorg. Ini menggantikan glibc + libSystem.Native.
- **rust-lld** menaut keduanya jadi ELF statis (tanpa interpreter) di `0x400000`.

Build: `scripts/build-hello-csharp.ps1` (atau `.sh`). Butuh `tools/bflat`
(diunduh dari github.com/bflattened/bflat) + Rust nightly.

## Jalur eksekusi di kernel

1. `bzimage/build.rs` menanam `hello.elf` ke image (sebagai `HELLO.ELF`).
2. Saat boot, `tunas_demo` membaca `HELLO.ELF` dari disk lewat driver IDE + FAT.
3. `elf::load` memetakan segmen PT_LOAD sebagai halaman **user-accessible**.
4. `usermode::enter_user` melakukan `sysretq` ke ring 3 di entry point.
5. C# mencetak via `SystemNative_Log` → syscall `DEBUG_WRITE` → kernel.
6. `Main` selesai → `exit` syscall → `usermode::exit_user` (longjmp) → kembali
   ke kernel dengan exit code.

## ABI ring-3

`syscall` dengan `rax` = nomor syscall, `rdi/rsi/rdx` = argumen, hasil di `rax`
(lihat `docs/abi.md`). SFMASK mematikan IF saat entry sehingga syscall tak
terinterupsi; timer tetap men-preempt kode *user* lewat TSS ring-0 stack.

## Dua gotcha page-table (penting)

Untuk mengeksekusi halaman user, **setiap** level page-table pada walk harus:
- punya bit `USER_ACCESSIBLE` (izin = AND semua level), dan
- **tidak** punya bit `NX` (fetch instruksi gagal bila NX di level manapun).

Entri level-atas dibagi dengan mapping kernel (dibuat tanpa USER, dan entri
PML4 low milik bootloader punya NX). `memory::make_user_path` meng-OR USER dan
meng-clear NX pada entri induk sepanjang jalur ke tiap halaman user. Leaf tetap
memakai bit-nya sendiri, jadi memori kernel tetap terlindungi & non-executable.

## Berikutnya (v0.8 "Kembang")

Jalur CoreCLR + JIT untuk fitur C# lengkap (reflection, dynamic loading),
integrasi GC dengan memory manager, dan BCL penuh — di atas fondasi ring-3 ini.
