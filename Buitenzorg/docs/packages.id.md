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

## Repo remote (`bz update`, `bz upgrade`)

Paket bisa datang dari repo: folder berisi berkas `.bzpkg` dan `INDEX.TXT`,
disajikan lewat HTTP -- oleh `bz` di PC:

```powershell
bz pack appku --depends libku -o repo\APPKU.BZP   # --depends: paket yang dibutuhkan
bz pack libku -o repo\LIBKU.BZP
bz repo serve repo --port 8080                    # menulis repo\INDEX.TXT, lalu menyajikan
```

Di perangkat:

```text
bz repo http://192.168.1.20:8080    # PC-nya (QEMU user networking: http://10.0.2.2:8080)
bz update                           # unduh indeks: "2 paket ...", yang bisa diperbarui
bz search                           # paket bawaan, terpasang, dan dari repo
bz install appku                    # "dependensi: libku", lalu keduanya terpasang
bz upgrade                          # semua paket terpasang yang versinya lebih baru di indeks
bz repo off                         # lepas repo
```

`INDEX.TXT` = `BZREPO 1`, lalu satu baris per paket:
`nama TAB versi TAB berkas TAB sha256 TAB depends (a,b atau -) TAB judul`
(`bz repo index DIR` menulisnya ulang). Perangkat memeriksa setiap unduhan
terhadap SHA-256 di indeks *dan* hash arsipnya sendiri sebelum memasangnya.
Dependensi dipasang lebih dulu, depth first; dependensi melingkar ditolak.
Versi dibandingkan per bagian, secara angka bila keduanya angka (`1.10` lebih
baru dari `1.9`).

**Publikasi** (PC): `bz publish APPKU.BZP --repo repo` menyalinnya ke folder
repo sebagai `appku-1.2.bzpkg` dan menulis ulang indeksnya (`bz repo serve
repo` yang sedang berjalan langsung menyajikannya). Nama + versi yang sama
ditolak (`--force` menggantinya); versi lama tetap ada, dan indeks
menawarkan yang terbaru.

**Rollback** (perangkat): setiap pemasangan menyimpan arsipnya di
`/disk/PKGHIST/<NAMA>.BZP` dan yang sebelumnya sebagai `.OLD`. `bz rollback
appku` memasang lagi versi sebelumnya; rollback kedua maju kembali.
`bz remove` ikut menghapus riwayatnya.

Batasan: HTTP biasa ke alamat IPv4 (belum ada DNS maupun TLS -- SHA-256 di
indeks melindungi dari berkas rusak, bukan dari server atau jaringan yang
jahat); satu repo; rollback menyimpan satu versi ke belakang; `bz remove`
tidak menghapus dependensi; `bz publish` menulis ke folder (belum unggah ke
server jarak jauh). Pengujian: `MILESTONE: REPO
OK` (versi, indeks, urutan dependensi, siklus) di setiap boot self-test;
`bz repo --selftest` (CI) dan `bz repo --check` (smoke `[bzlink]`: update,
install dengan dependensi, upgrade 1.0 -> 1.1, paket yang diubah ditolak).

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
