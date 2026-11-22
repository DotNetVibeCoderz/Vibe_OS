# Fitur desktop: notifikasi, pencarian, personalisasi, layar

*English: [desktop-features.md](desktop-features.md)*

Apa yang ditawarkan desktop Buitenzorg di luar jendela dan menu Start, dan
bagaimana program memakainya.

## Notifikasi dan pusat notifikasi

Program mengirim notifikasi lewat syscall `NOTIFY` (73: judul, isi). Desktop
menampilkannya sebagai toast di pojok kanan atas selama ~4,5 detik dan
menyimpan 50 terakhir di **pusat notifikasi**: klik lonceng di tray (titik =
ada yang belum dibaca), lalu hapus satu atau semuanya. Kernel juga mengirim
notifikasinya sendiri (disk USB dicolok, tangkapan layar disimpan, aplikasi
yang gagal dibuka).

| Dari | Caranya |
|---|---|
| C# (managed) | `Buitenzorg.App.Notifications.Show("Unduhan", "selesai")` |
| Skrip UI | `notify("Unduhan", "selesai")` |
| Shell | `notify Judul isi...`, `notify list`, `notify read`, `notify rm 3`, `notify clear` |

## Pencarian global

Ketik di kotak cari menu Start (atau `cari KATA` di shell): aplikasi, halaman
pengaturan, serta berkas/folder di setiap volume (akar + satu tingkat folder)
muncul sambil mengetik. Enter atau klik membuka hasilnya: aplikasi dijalankan,
pengaturan membuka Pengaturan, berkas/folder dibuka di Berkas
(`mfiles --open PATH`).

## Jendela

* Membuka dan menutup jendela beranimasi (membesar / mengecil dari tengah);
  pengaturan `ANIMATIONS` mematikannya.
* Klik dua kali bilah judul untuk memaksimalkan atau mengembalikan jendela.
* Matikan dan mulai ulang dari menu Start meminta konfirmasi dulu (Enter
  setuju, Esc batal).

## Personalisasi (Pengaturan > Tampilan)

| Pengaturan | Kunci | Berlaku |
|---|---|---|
| Tema | `THEME` (0) | langsung; tema bawaan + paket tema |
| Ukuran teks | `UI_SCALE` (13): 100 / 110 / 125 % | langsung |
| Paket ikon | `ICON_PACK` (14): bawaan, lalu `/disk/ICONS/*.BIN` | langsung |
| Resolusi layar | `RESOLUTION` (12): 800x600 .. 1920x1080 | saat mulai berikutnya; hanya VGA standar QEMU / Bochs / VirtualBox (di tempat lain mode firmware tetap). Heap kernel membesar untuk layar yang lebih besar (paling banyak setengah RAM); lebar VGA standar kelipatan 8, jadi 1366x768 tampil 1360x768 |

**Disimpan di disk:** tema, wallpaper, screensaver + jedanya, animasi, kursor,
ukuran teks, paket ikon dan resolusi ditulis ke `/disk/DESKTOP.CFG` setiap
kali berubah (dari Pengaturan, perintah shell `theme` / `bg` / `saver`, atau
menu Start) dan dipulihkan saat mulai berikutnya.

### Paket tema

Paket tema adalah berkas teks `/disk/THEMES/NAMA.THM` (nama 8.3):

```
name=senja
base=kebun            # tema bawaan; kunci yang tidak ditulis diambil darinya
dark=1
desktop_top=#2B1B2E
desktop_bottom=#6B3A3A
accent=#F2A65A
title_style=flat      # flat | beveled | tab
shadow_style=soft     # none | soft | hard
glass=170             # 0-255, tembus pandang panel shell
```

Kunci: `name`, `base`, `dark`, `desktop_top`, `desktop_bottom`, `gradient`,
`win_body`, `win_title_active`, `win_title_inactive`, `win_border`, `border`
(px), `title_style`, `shadow_style`, `text`, `title_text`, `taskbar_bg`,
`accent`, `cursor`, `shadow`, `glass`. Setiap image membawa contoh
`SENJA.THM` (`assets/themes/senja.thm`). Kirim tema dalam `.bzpkg` dengan
berkas di `package/THEMES/NAMA.THM`; `theme reload` (atau mulai berikutnya)
memuatnya, `theme senja` atau Pengaturan memilihnya.

### Paket ikon

Paket ikon adalah berkas BZIC `/disk/ICONS/NAMA.BIN` (format
`/disk/ICONS.BIN`, ukuran ikon sama: 44 dan 18 px). Paket ditumpuk di atas
ikon bawaan: aplikasi yang punya ikon di paket memakainya, sisanya tetap.
`icons list`, `icons daun`, `icons bawaan` di shell; contoh `DAUN.BIN` (semua
ikon dalam dua warna hijau daun) dibuat oleh bzimage.

## Layar: permukaan tanpa salin, vsync, layar penuh (`WIN_CTL`, 74)

| Operasi | Fungsinya |
|---|---|
| `SURFACE` | memetakan dua buffer 32-bit seukuran area klien ke program; compositor membaca buffer yang tampil di tempatnya (tanpa salinan BLIT). Buffer terkunci: MUNMAP menolaknya |
| `SURFACE_PRESENT` | tampilkan buffer 0 atau 1 mulai sekarang (gambar ke yang lain) |
| `VSYNC_WAIT` / `VSYNC_INFO` | tunggu vertical blank berikutnya / laju refresh + penghitung. Sumbernya dari layar (penghitung frame pipe A Intel, dipakai setelah terlihat bergerak -- transcoder A atau EDP) atau timer 60 Hz |
| `FULLSCREEN` | area klien jendela menjadi seluruh layar tanpa bingkai dan taskbar; off mengembalikannya. Ditolak untuk jendela yang punya permukaan |
| `RESIZABLE` | program menggambar ulang di ukuran apa pun: maximize / restore mengubah ukuran pikselnya dan mengirim event pointer `RESIZE` (jenis 6, x = lebar, y = tinggi). Semua jendela Buitenzorg.UI menyalakannya, jadi app managed mengisi jendela yang di-maximize |

Lapisan SDL di runtime C memakainya (`SDL_SetWindowFullscreen`,
`SDL_RenderSetVSync`; DOOM tampil lewat permukaan), aplikasi managed punya
`Display.WaitVsync()` / `Window.SetFullscreen(bool)`, skrip UI `vsync()` /
`fullscreen(on)`. Ujinya `userland/riscv/surface.rs` (`SURFACE OK`).
