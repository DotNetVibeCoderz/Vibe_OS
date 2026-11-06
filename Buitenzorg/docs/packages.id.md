# Paket (`.bzpkg`)

Sebuah app bisa dikirim sebagai satu berkas paket alih-alih dimasukkan ke image
disk. `bz pack` di PC membuatnya; `bz install` di perangkat memasangnya.

## Membuat paket (host)

```text
myapp/
  app.manifest          id, name, type, version (manifest SDK)
  package/              semua yang dikirim, nama 8.3 (/disk perangkat memakai FAT)
    MYAPP.ELF           app C# native (bflat), atau MYAPP.DLL (managed), atau MYAPP.SH
    MYAPP.BMP           aset
```

```powershell
dotnet run --project sdk/bz -- pack myapp -o MYAPP.BZP     # [--entry FILE] [--name NAMA]
dotnet run --project sdk/bz -- inspect MYAPP.BZP           # daftar + cek setiap SHA-256
```

Nama paket = bagian terakhir `id` di manifest (`local.my-app` -> `my-app`;
a-z 0-9 -, maks 23 karakter). **Entry** — yang dijalankan saat nama paket
diketik — adalah `--entry`, kalau tidak berkas `.ELF`, `.DLL` atau `.SH`
pertama.

## Pasang, jalankan, hapus (perangkat)

Salin berkas ke `/disk` dengan nama 8.3 (`MYAPP.BZP`), misalnya lewat layanan
deploy (`deploy on` + panel perangkat di Magic App Generator), lalu:

```sh
bz info /disk/MYAPP.BZP       # manifest + berkas, semua hash dicek
bz install /disk/MYAPP.BZP    # berkas ke /disk, pemasangan dicatat
my-app arg1 arg2              # atau: run my-app
bz list                       # registry bawaan + "terpasang dari berkas .bzpkg"
bz install /disk/MYAPP.BZP    # versi lebih baru: pembaruan (berkas lama dihapus)
bz remove my-app              # menghapus berkasnya
```

Setiap image membawa contoh, `/disk/HALOPKT.BZP` (`sdk/samples/halo-paket`).

**Aturan keamanan:** SHA-256 arsip dan setiap berkas dicek sebelum apa pun
ditulis; nama harus 8.3; berkas milik paket lain, berkas sistem (`USERS.CFG`,
`PKGS.CFG`, `CLRHOST.ELF`, ...) atau berkas yang sudah ada di `/disk` tapi bukan
dari paket ini ditolak; paket tidak boleh memakai nama app registry bawaan.

Paket terpasang, dan app registry yang dipasang dengan `bz install NAMA`,
disimpan di `/disk/PKGS.CFG` dan dipulihkan saat boot. Paket dari berkas juga
muncul di daftar App Store (`PKG_LIST`).

## Format, versi 1

```text
"BZPK"  u16 versi (1)  u16 jumlah berkas  u32 panjang manifest
manifest: baris key=value UTF-8 (name, version, title, kind, category,
          description, entry)
per berkas: u8 panjang nama, nama 8.3, u32 ukuran, SHA-256 32 byte, data
penutup:  SHA-256 32 byte dari semua byte sebelumnya
```

Belum ada: tanda tangan (hash hanya mendeteksi kerusakan, bukan penerbit
jahat), dependensi, indeks repository jarak jauh (`bz update`), sub-folder,
izin per app dari manifest.

Implementasi: `kernel/bzkernel/src/bzpkg.rs` (perangkat), `sdk/bz/Program.cs`
(`Pack`, `Inspect`).

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin Kang Fadhil.*
