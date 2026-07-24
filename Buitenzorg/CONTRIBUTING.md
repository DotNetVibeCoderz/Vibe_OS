# Contributing — Buitenzorg OS

## Prinsip

Ikuti [requirements.md](requirements.md) §1: safety by default (Rust di ring 0),
productivity by default (C# di user-space), batas ABI tegas, microkernel-leaning,
optimasi sebagai kebijakan.

## Standar Koding

**Rust (kernel/)**
- `cargo fmt` + `cargo clippy` bersih sebelum PR.
- `unsafe` selalu diberi komentar `// Safety:` yang menjelaskan invariannya.
- Kernel `no_std`; dependensi baru harus `no_std`-compatible dan dipertimbangkan
  matang (setiap crate menambah permukaan kepercayaan di ring 0).
- Target hanya `x86_64-unknown-none` untuk `bzkernel` (bukan default-member).

**C# (runtime/, sdk/)**
- Nullable + ImplicitUsings aktif; `Buitenzorg.Runtime` wajib tetap
  NativeAOT-compatible (`IsAotCompatible=true`) — hindari reflection dinamis.
- Struct interop: `[StructLayout(LayoutKind.Sequential)]` + test ukuran byte.

**Kontrak ABI (kernel/abi ↔ runtime/.../Sys)**
- Perubahan ABI harus mengubah **kedua sisi + test kontrak keduanya + docs/abi.md**
  dalam satu PR. Nomor syscall append-only.

## Alur PR

1. Branch dari `main`, satu topik per PR.
2. Wajib hijau: `cargo test -p bz-abi`, build kernel, boot smoke test QEMU,
   `dotnet test`. CI menjalankan semuanya (`.github/workflows/ci.yml`).
3. Update checklist di requirements.md §17 bila menyelesaikan item.
4. Commit message: baris ringkas bahasa Inggris, isi bebas (EN/ID).

## Menandai progres

Item selesai ditandai `[x]` di [requirements.md §17](requirements.md) pada PR
yang sama dengan implementasinya — checklist itu adalah papan status proyek.

## Lisensi kontribusi

Proyek berlisensi **MIT** ([LICENSE](LICENSE)). Dengan mengirim kontribusi,
Anda setuju kontribusi itu dilisensikan di bawah MIT juga.
