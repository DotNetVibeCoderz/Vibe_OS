# Mencoba kernel baru di Raspberry Pi 4 tanpa mencabut kartu SD

Memasang setiap kernel baru ke kartu dengan tangan (matikan, cabut kartu,
salin `kernel8.img`, pasang lagi) adalah bagian paling lambat saat bekerja di
Pi. `bz pi update` melakukannya lewat jaringan, dan tidak mungkin membuat
papan tak terjangkau:

```
bz device connect 192.168.68.109 1234      # sekali: IP papan dan kode pairing
bz pi update dist/kernel8.img --expect "first desktop frame on screen" --run "lsusb"
```

Yang terjadi:

1. Kernel diunggah ke kartu sebagai `/disk/KERNTRY.IMG` (bz-link).
2. `pi tryboot` di papan menulis `TRYBOOT.TXT` (salinan `config.txt` dengan
   `kernel=KERNTRY.IMG` dan `cmdline=CMDTRY.TXT`) dan `CMDTRY.TXT`
   (`bz.tryboot`), memasang flag reboot **tryboot** firmware, lalu restart.
   Firmware membaca `tryboot.txt` hanya untuk satu boot itu.
3. Kernel baru melihat `bz.tryboot` dan menyalakan **watchdog PM** SoC (15 s)
   sebelum hal lain; loop desktop terus "memberinya makan". Kernel yang macet
   -- saat boot atau sesudahnya -- di-reset oleh watchdog, flag tryboot sudah
   hilang, dan papan menyalakan kernel lama.
4. `bz` menunggu sampai papan menjawab lagi. Jika yang menjawab kernel lama,
   pembaruan gagal. Selain itu kernel baru diperiksa: setiap teks `--expect`
   harus ada di log papan, setiap perintah `--run` harus sukses.
5. Semua lulus: `pi promote` mematikan watchdog dan menyalin `KERNTRY.IMG`
   menimpa `KERNEL8.IMG` (dibaca balik dulu). Ada cek yang gagal (atau
   `--no-promote`): papan di-restart ke kernel lama.

Di papan sendiri: `pi` menampilkan apakah ini boot percobaan dan berkas
kernel apa saja di kartu, `pi tryboot` / `pi promote` menjalankan langkah 2
dan 5 secara manual, dan `pi watchdog-test` membuktikan jalur reset
(watchdog 3 s lalu sengaja macet: papan harus restart).

## Yang dibutuhkan papan

* Kartu SD Buitenzorg (rute langsung, `kernel8.img`), kabel Ethernet, dan
  bz-link menyala: `deploy on 1234` di Terminal (diingat di
  `/disk/DEPLOY.CFG`, jadi menyala setiap boot).
* EEPROM bootloader Pi 4 tahun 2021 atau lebih baru (flag tryboot). Yang
  lebih lama membuat `pi tryboot` melapor bahwa firmware tidak menerima flag;
  perbarui dengan image "Bootloader" dari Raspberry Pi Imager.

## Integrasi berkelanjutan di papan

`.github/workflows/pi.yml` menjalankan ini di runner GitHub Actions
self-hosted berlabel `rpi4`: sebuah PC di jaringan yang sama dengan papan,
dengan Rust, .NET dan toolchain repositori. Workflow membangun `kernel8.img`
dan menjalankan `bz pi update` dengan beberapa cek. Isi secret repositori
`BZ_DEVICE` (IP papan) dan `BZ_CODE` (kode pairing), lalu jalankan workflow
secara manual (Actions > Raspberry Pi 4 (board) > Run workflow). Tidak perlu
relay: kernel yang macet dibatalkan oleh watchdog.

Apa yang diuji di mana: di QEMU `raspi4b` boot percobaan (watchdog menyala
dan diberi makan), berkas `pi tryboot`, `pi promote`, dan reset watchdog itu
sendiri. Model firmware QEMU tidak menerima flag tryboot dan tidak punya
jaringan, jadi perjalanan penuhnya hanya berjalan di papan.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
