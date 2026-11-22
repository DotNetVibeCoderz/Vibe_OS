# Menguji perubahan 22-11-2026 di perangkat nyata

Semua di bawah ini baru diuji di QEMU. Ini daftar pendek yang butuh laptop dan
Raspberry Pi 4 sungguhan: jalankan, foto tiap langkah bertanda **foto**, lalu
kirim fotonya (plus `BOOTLOG.TXT` bila ada yang salah). Gerakkan mouse sebelum
memotret: screensaver mulai setelah 12 detik tanpa input.

## Laptop

**Menulis flash disk menghapus isinya, termasuk catatan yang tersimpan di
sana.** Di PowerShell sebagai Administrator, dari akar repositori (image di
`dist/` itulah yang diuji):

```powershell
.\scripts\flash-usb.ps1 -List
.\scripts\flash-usb.ps1 -DiskNumber N -Firmware uefi   # N dari daftar
```

Boot dari flash disk (UEFI), lalu buka Terminal.

| # | Lakukan | Yang diharapkan |
|---|---|---|
| 1 | Gerakkan touchpad / mouse begitu desktop tampil | Penunjuk ikut sejak gerakan pertama, tanpa lompat atau klik liar |
| 2 | Lihat desktop 10 detik tanpa menyentuh apa pun | Terminal, Welcome dan Tips padat, bukan bayangan tembus pandang (**foto**) |
| 3 | `gpu` | Ada baris `vsync: pipe A frame counter, N vblanks in 100 ms` dan baris terakhir `display: vsync from Intel pipe A frame counter at 60.000 Hz ...`. Bila tidak, baris `vsync:` menyebut sebabnya (**foto**) |
| 4 | Ketik dua baris di bawah tabel ini | `120 vsync: N ms`, N mendekati 2000 pada panel 60 Hz (**foto**) |
| 5 | `run crashme null`, tunggu selesai, lalu `crash last` | NullReferenceException tak tertangani, backtrace menyebut `Null3`, `Null2`, `Null1` (**foto**) |
| 6 | `run crashme throw`, lalu `crash last` | Backtrace menyebut `Throw2`, `Throw1` (**foto**) |
| 7 | `notify Tes halo`, lalu klik lonceng | Toast muncul, lalu action center memuatnya (**foto**) |
| 8 | Buka-tutup Kalkulator beberapa kali, klik ganda bilah judul | Buka / tutup mulus, tak ada sisa setengah gambar, jendela membesar dan tombolnya mengisi jendela; klik ganda lagi mengembalikannya (**foto** saat membesar) |
| 9 | Pengaturan > Tampilan > Ukuran teks 125 %, lihat keenam halaman | Tidak ada label terpotong, langsung di halaman Tampilan tanpa pindah halaman (**foto** halaman Tampilan) |
| 10 | Mulai > Permainan > DOOM | Ikonnya ada, game mulai dan menerima tombol (**foto**) |
| 11 | Mulai > Matikan, tekan Esc; ulangi, tekan Enter | Esc membatalkan, Enter mematikan laptop |

```
echo 'let t = ticks(); let i = 0; while (i < 120) { vsync(); i = i + 1; } print("120 vsync: " + (ticks() - t) + " ms");' > /disk/V.JS
run /disk/V.JS
```

Setelan resolusi tidak ada di daftar: ia hanya bekerja di std VGA QEMU /
Bochs. Laptop memakai resolusi pilihan firmware-nya.

## Raspberry Pi 4

`bz pi update` hanya mengganti kernel, sedangkan langkah 5, 6 dan 9 juga butuh
berkas baru di kartu. Pilih salah satu:

**A. Tulis seluruh kartu (paling mudah; menghapus akun, setelan dan berkas di sana):**

```powershell
.\scripts\flash-usb.ps1 -List
.\scripts\flash-usb.ps1 -Image dist\buitenzorg-rpi4.img
```

**B. Lewat jaringan, isi kartu tetap** (hanya bila kartu sudah berisi image
22-11-2026). Di Pi ketik `deploy on 1234`, lalu di PC:

```powershell
dotnet run --project sdk\bz -- device connect ALAMAT_PI 1234
dotnet run --project sdk\bz -- push userland\hello-csharp\arm64\crashme.elf /disk/CRASHME.ELF
dotnet run --project sdk\bz -- push userland\clr-apps\dist\MSETTING.DLL /disk/MSETTING.DLL
dotnet run --project sdk\bz -- pi update dist\kernel8.img --expect "first desktop frame on screen"
```

Kemudian ulangi langkah 1, 2, 5, 6, 7, 8, 9 dan 11 dari tabel laptop. Bedanya:

- `gpu` melaporkan `virtual 60 Hz clock`: Pi tidak punya sumber vblank.
- Image Pi tidak membawa DOOM, jadi kartunya menjawab dengan notifikasi bahwa
  game itu tidak ada di media ini.
- Matikan menghentikan papan; LED merah tetap menyala.
