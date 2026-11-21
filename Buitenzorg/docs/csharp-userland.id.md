# Menjalankan C# di Ring 3 (v0.4 "Tunas")

Milestone v0.4: sebuah program **C#** yang dikompilasi ahead-of-time berjalan di
user-space (ring 3) di atas kernel Rust dan memanggil kernel lewat syscall.
Inilah "Layer 4 — Managed Runtime" (requirements.md §3, §5.1) dalam bentuk paling
ringan: jalur **NativeAOT**, tanpa JIT -- kini dengan garbage collector yang
benar-benar membebaskan memori dan exception.

[English](csharp-userland.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

## Pipeline build

```
hello.cs + bzexc.cs ──bflat build-il (zerolib)──► il/hello.dll
il/hello.dll + zerolib.dll ──ILCompiler 8.0.26 (NativeAOT)──► hello.o ─┐
                                                                       ├─rust-lld─► hello.elf
bzstart.rs ──rustc (x86_64-unknown-none / aarch64-unknown-none)──► bzstart.o ┘
```

- **`bflat build-il`** (`--stdlib:zero`) mengompilasi C# ke IL terhadap zerolib,
  pustaka inti yang mungil (tanpa runtime .NET penuh). Yang dipakai salinan
  bflat pribadi di `kernel/target/bflat-patched`, yang zerolib-nya diperluas
  `tools/ZerolibPatch` (`ThrowOverflowException`, `System.IDisposable`, ekspor
  milik shim diganti nama).
- **ILCompiler 8.0.26 upstream** (`scripts/prepare-ilc.*`: dari cache NuGet,
  kalau tidak ada diunduh dari nuget.org ke `kernel/target/ilc`, SHA-256
  dipatok) mengubah IL menjadi kode mesin. bflat membawa compiler yang sama
  tetapi mematikan tabel unwind dan tabel klausa exception untuk semua pustaka
  standar selain milik dotnet, jadi `throw` tidak pernah bisa jalan; versi
  upstream menghasilkan `.eh_frame`, LSDA per method, dan tabel klausa. Flag:
  `--systemmodule:zerolib --targetos:linux --Os --reflectiondata:none
  --directpinvoke:libSystem.Native` (tanpa flag terakhir, import milik zerolib
  sendiri diikat secara lazy dan berulang tanpa henti).
- **bzstart.rs** adalah shim startup + PAL: `_start` (memanggil
  `__managed__Main`), `SystemNative_Log/Malloc/Abort` di atas syscall
  Buitenzorg, cast / dispatch interface / penyimpanan array kovarian, thread,
  dan sekarang **`bzgc.rs`** (kolektor di balik `RhpNewFast` / `RhpNewArray`)
  serta **`bzeh.rs`** (dispatcher exception di balik `RhpThrowEx` / `RhpRethrow`).
- **rust-lld** menaut semuanya jadi ELF statis di `0x400000` (`user.ld`
  menyimpan `.eh_frame` dan menandai rentang data/bss yang dipindai kolektor).

Setiap program dikompilasi dengan **`bzexc.cs`** (System.Exception dan jenis
exception yang lazim). Build: `scripts/build-hello-csharp.ps1` (atau `.sh`;
ARM64: `build-hello-csharp-arm64.*`); template SDK `desktop-csharp` dan
MagicAppGen memakai langkah yang sama. Butuh `tools/bflat`, Rust nightly, dan
(diunduh saat pertama dipakai) ILCompiler 8.0.26.

## Garbage collector

`bzgc.rs` adalah **kolektor mark-sweep yang tidak memindahkan objek**. Ia
mencadangkan ruang alamat (`MMAP` tanpa akses) dan meng-commit blok 64 KiB saat
pertama dipakai; blok kecil berisi slot satu kelas ukuran (16 B .. 8 KiB)
dengan bitmap teralokasi dan bitmap tanda, objek yang lebih besar mengambil
deretan blok utuh. Koleksi berjalan begitu jumlah yang dialokasikan sudah
sebanyak yang bertahan dari koleksi sebelumnya (minimal 8 MiB), saat blok
habis, atau lewat `BzGC.Collect()`.

- **Root, secara konservatif:** data + bss program (tempat basis GC static)
  dan stack thread yang mengoleksi, dengan register callee-saved-nya ditumpahkan
  dulu. Setiap word yang menunjuk ke slot teralokasi membuatnya tetap hidup.
- **Objek:** instance kelas dipindai word demi word, array hanya kalau
  MethodTable-nya menyatakan berisi referensi (`byte[]` / `int[]` tidak pernah).
- **Sweep:** slot yang tidak bertanda dibebaskan dan di-nol-kan saat dibagikan
  lagi; blok kosong kembali ke pool.

**Batasan:** koleksi dilewati selama program masih punya thread lain yang
hidup (dihitung `bz_thread_create`): referensinya mungkin ada di register yang
tidak terlihat kolektor, jadi heap tumbuh sampai thread itu selesai. Keluarga
`malloc` gaya libc (dipakai shim sendiri) adalah heap terpisah yang tidak
pernah dikoleksi.

`BzGC` (bzbcl2.cs): `Collect()`, `CollectionCount()`, `LiveBytes()`,
`FreedBytes()`, `SkippedCount()`, `LastPauseMicroseconds()`, `SetEnabled()`,
ditambah angka alokasi yang lama. `gc.cs` mengalokasikan 128 MiB sampah sambil
menjaga linked list dan array besar tetap hidup: 18 koleksi, 187 MiB
dibebaskan, puncak commit 14 MiB, data hidup utuh -> `MILESTONE: GC OK`
(x86-64 dan ARM64).

## Exception

`try` / `catch (T e)` / `catch ... when (filter)` / `finally` / `using` /
`throw;` berjalan, termasuk exception yang dilempar di dalam blok catch dan
finally, di kode generik, lewat delegate, dan di thread pekerja. `bzeh.rs`
melakukan yang dilakukan runtime .NET dalam dua tahap: tahap 1 menelusuri
stack ke atas (CFI DWARF dari `.eh_frame`) sampai klausa pertama yang
menerima exception -- klausa bertipe yang tipenya cocok dengan objek, atau
funclet filter yang menjawab true; tahap 2 menelusuri frame yang sama sekali
lagi, memanggil setiap funclet finally / fault di jalan lalu funclet catch,
dan melanjutkan method di alamat yang dikembalikan funclet. Funclet berjalan
di frame pointer method-nya (x86-64: `rbp`, exception di `rsi`; ARM64: `x29`,
exception di `x0`); funclet dipanggil lewat `bz_call_funclet`, yang
meninggalkan catatan di atas alamat kembalinya sehingga exception yang
dilempar di dalam funclet berlanjut di method pemiliknya. Klausa dicocokkan
dengan instruksi call (alamat kembali - 1): di ARM64 sebuah call bisa menjadi
instruksi terakhir blok `try`.

Tanpa handler: `Unhandled exception: <Message>` lalu program berakhir (blok
finally tidak dijalankan, seperti di .NET). Pemeriksaan yang dilakukan zerolib
sendiri (null, batas array, pembagian dengan nol) tetap mengakhiri program
dengan pesan alih-alih melempar, dan exception yang dilempar di dalam filter
`when` belum didukung. `exc.cs` memeriksa 13 kasus -> `MILESTONE: EXCEPTIONS OK`
(x86-64 dan ARM64).

## Jalur eksekusi di kernel

1. `bzimage/build.rs` menanam `hello.elf` di image (sebagai `HELLO.ELF`).
2. Saat boot, `tunas_demo` membaca `HELLO.ELF` dari disk lewat driver IDE + FAT.
3. `elf::load` memetakan segmen PT_LOAD sebagai halaman **user-accessible**.
4. `usermode::enter_user` melakukan `sysretq` ke ring 3 di entry point.
5. C# mencetak via `SystemNative_Log` → syscall `DEBUG_WRITE` → kernel.
6. `Main` selesai → syscall `exit` → `usermode::exit_user` (longjmp) → kembali ke
   kernel dengan exit code.

## ABI ring-3

`syscall` dengan `rax` = nomor syscall, `rdi/rsi/rdx` = argumen, hasil di `rax`
(lihat [Syscall ABI](abi.id.md)). SFMASK mematikan IF saat entry sehingga syscall
tak terinterupsi; timer tetap men-preempt kode *user* lewat stack TSS ring-0.

## Dua gotcha page-table (penting)

Untuk mengeksekusi halaman user, **setiap** level page-table pada walk harus:
- punya bit `USER_ACCESSIBLE` (izin = AND semua level), dan
- **tidak** punya bit `NX` (fetch instruksi gagal bila NX di level manapun).

Entri level-atas dibagi dengan mapping kernel (dibuat tanpa USER, dan entri PML4
low milik bootloader punya NX). `memory::make_user_path` meng-OR USER dan
meng-clear NX pada entri induk sepanjang jalur ke tiap halaman user. Leaf tetap
memakai bit-nya sendiri, jadi memori kernel tetap terlindungi & non-executable.

## async / await (native)

Tambahkan `bzasync.cs` (bersama `bzbcl3.cs`) ke program, dan `async` / `await`
berjalan di zerolib:

```csharp
static async Task<int> Ambil(int n) { await Task.Delay(10); return n * 2; }

static int Main()
{
    BzAsync.Run(async () =>
    {
        int a = await Ambil(1);
        await Task.WhenAll(Task.Delay(30), Task.Delay(10));
        BzDebug.WriteLine(a == 2 ? "ok" : "salah");
    });
    return 0;
}
```

Semuanya berjalan di satu thread, di event loop yang dipompa `BzAsync.Run`
(model JavaScript): `Task.Delay` adalah timer di loop itu, `Task.Yield`
memberi giliran ke pekerjaan lain, `Task.Run` mengantrekan pekerjaan,
`.Result` / `Wait()` memompa loop sampai task selesai. Exception yang lolos
dari method async membuat task-nya gagal, dan `await`, `.Result`, serta
`Wait()` melemparnya lagi (exception aslinya, bukan AggregateException);
exception dari method `async void` dibuang. Untuk pustaka .NET lengkap (LINQ,
reflection, GC yang aman untuk banyak thread), pakai jalur managed (RustCLR).

## Berikutnya

Di jalur native: koleksi saat thread lain berjalan (menghentikannya di safe
point dan memindai stack-nya), mengubah pemeriksaan zerolib sendiri (null,
batas array, pembagian dengan nol, cast gagal) menjadi exception yang bisa
ditangkap, exception yang dilempar dari filter `when`, dan build RISC-V (bflat
dan ILC tidak punya target RISC-V -- C# sampai ke RISC-V lewat RustCLR). .NET
lengkap (reflection, dynamic loading, seluruh BCL) adalah jalur managed RustCLR.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
