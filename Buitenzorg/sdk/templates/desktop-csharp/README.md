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

## Catatan (zerolib: heap jalan, GC belum)

Sejak v0.15 "Matang" **`new`, array, objek heap, dan generic BEKERJA** (heap
bump yang tumbuh lewat `mmap`). Yang masih belum:

- **Tidak ada static reference field** — GC statics tak diinisialisasi, jadi
  `static readonly char[] X = ...` membaca sampah. Simpan state di **lokal**
  atau **field instance**.
- **Tidak ada konversi method-group -> delegate** (delegate-nya di-cache di GC
  static). Pakai **function pointer**: `delegate*<int,bool>` + `&Method`.
- **Tidak boleh menyimpan referensi ke elemen `object[]`** (`stelem.ref`).
  Pakai **linked list** atau field objek.
- **Tidak ada `new string()` / `ToString()` / concat / `string ==`.** Bangun
  teks di `char[]` lalu gambar dengan `Graphics.DrawChars`.
- **Belum ada GC yang mereklamasi** — memori baru bebas saat app keluar.

Library yang tersedia: `Buitenzorg.Drawing` (`bzgfx.cs`), `Buitenzorg.UI`
(`bzui.cs`), `Buitenzorg.Audio` (`bzaudio.cs`), dan `Buitenzorg.Bcl`
(`bzbcl.cs` + `bzbcl2.cs` — koleksi/LINQ + System.IO/Text/Regex/Globalization/
Diagnostics/Management/Net.Sockets/Tasks/Timers/GC/Pkg). Lihat
`docs/first-app.md` untuk katalog API-nya.

## Contoh lengkap

Lihat `userland/hello-csharp/xox.cs` (Tic-Tac-Toe) untuk contoh yang berjalan.
