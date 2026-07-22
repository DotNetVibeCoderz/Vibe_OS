# Desktop App (C#)

Template app Buitenzorg varian **desktop** (requirements.md §11.2). Menggambar
UI lewat **window syscall** (WIN_CREATE / WIN_CMD / WIN_PRESENT / KEY_READ).

## File

- `app.manifest` — manifest terpadu (type=desktop, language=csharp)
- `app.cs` — program utama (buat window, gambar UI)
- `bzui.cs` — helper UI tipis di atas window syscall
- `.vscode/launch.json` — konfigurasi build & run dari VS Code

## Build

App desktop dikompilasi **freestanding** dengan bflat (`--stdlib:zero`) +
shim `bzstart` (di `userland/hello-csharp/bzstart.rs`) menjadi ELF statis,
mirip `xox.cs`. Jalur build ada di `scripts/build-hello-csharp.ps1` (tambahkan
`app.cs` ke daftar program di sana, atau salin polanya).

Hasil ELF ditaruh di `/disk` (di-embed oleh `bzimage/build.rs`) lalu dijalankan
dari terminal Buitenzorg:

```
run app          # atau daftarkan nama di kernel app::app_file()
```

## Catatan (tanpa GC)

Runtime freestanding belum punya GC — **hindari `new T[]`** dan alokasi heap.
Simpan state di stack (`stackalloc`). GC penuh + CoreCLR/JIT menyusul (v0.8+).

## Contoh lengkap

Lihat `userland/hello-csharp/xox.cs` (Tic-Tac-Toe) untuk contoh yang berjalan.
