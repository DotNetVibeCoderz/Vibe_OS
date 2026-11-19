# Suara dan media

Setiap program bisa membunyikan suara bersamaan: kernel menyimpan satu stream
per program, mencampurnya, dan mengisi kartu suara dari ring yang terus
berputar selama ada yang diputar. **Musik** memutar berkas WAV di disk; dua
lagu orisinal ikut di setiap image.

![Musik memutar "Embun Pagi di Kebun Raya" (QEMU, AC'97)](img/musik.png)

## Musik

`run mmusik`, atau **Mulai > Multimedia > Musik**. Daftarnya berisi berkas WAV,
FLAC (`.FLA` di disk FAT, `.FLAC`) dan MP3 di `/disk` dan folder scratch (`/ram`); klik satu, atau Spasi untuk putar /
jeda; Sebelumnya / Berikutnya; klik bilah progres untuk melompat; slider
mengatur volume *program ini* di mixer (volume master tetap di Pengaturan).
Dua lagu di setiap image -- *Embun Pagi di Kebun Raya* dan *Hujan Sore di
Bogor* (`LAGU1.WAV`, `LAGU2.WAV`) -- disintesis saat image dibangun
(`synth_song` di `kernel/bzimage/build.rs`: nada bel pada skala lima nada di
atas bass gong yang lambat); lagu ketiga, *Tetes Embun* (`LAGU3.FLA`,
`assets/music`), berformat FLAC lossless. FLAC di-decode ke PCM oleh host saat
lagu dimuat (`userland/clrhost/src/flac.rs`: prediktor fixed dan LPC, residu
Rice, dekorelasi stereo, 8 sampai 24 bit -- hasilnya bit-exact dengan
ffmpeg), judulnya dari komentar `TITLE=`. MP3 (juga MP1 / MP2) lewat crate
`nanomp3`, port Rust aman dari minimp3 (selisih paling banyak satu langkah
dari keluaran ffmpeg; tag ID3 dilewati, jeda dan padding encoder dipangkas),
judulnya dari frame ID3v2 `TIT2`; `LAGU4.MP3` (*Gerimis*) ada di setiap image.
**Ogg Vorbis** (`.OGG`, `.OGA`) dan **Ogg Opus** (`.OPU`, `.OPUS`) juga
di-decode di host (`userland/clrhost/src/ogg.rs` membaca halaman Ogg): Vorbis
lewat port stb_vorbis, Opus lewat dekoder Opus Rust murni (keduanya di-vendor
di `userland/clrhost/vendor`, lihat `NOTICE.md`-nya), masing-masing selisih
paling banyak satu langkah dari keluaran ffmpeg; Opus diputar 48 kHz,
pre-skip dan padding akhirnya dipotong; judul dari komentar `TITLE=`. *Angin
Sore* (`LAGU6.OGG`) dan *Embun Pagi* (`LAGU7.OPU`, dibuat
`scripts/make-ogg-songs.py`) ada di setiap image.
Berkas MIDI (`.MID`) dimainkan synthesizer kecil di host
(`userland/clrhost/src/midi.rs`): format 0 dan 1, perubahan tempo, 48 suara
dengan warna bunyi per keluarga instrumen General MIDI (piano, organ, gitar,
bass, string, brass / reed, flute, synth) dan envelope, kanal 10 sebagai drum
(kick, snare, hi-hat, tom), dirender 22 kHz mono; judulnya nama track
pertama. *Kupu-kupu di Taman* (`LAGU5.MID`, dibuat
`scripts/make-midi-song.py`) ada di setiap image -- MIDI juga cocok sebagai
musik game: beberapa KB per lagu.

## Perekam Suara

`run mrekam`, atau **Mulai > Multimedia > Perekam Suara**. **Rekam** (atau
Spasi) merekam dari mikrofon, dengan waktu dan meter level; **Berhenti**
menyimpannya sebagai `/disk/REKAMAN/REKAMnnn.WAV` (48 kHz stereo, maks 60
detik); **Putar** memutar rekaman terpilih, **Hapus** menghapusnya. Merekam
berfungsi di PC dengan kartu suara AC'97 (`-device AC97` di QEMU); input HD
Audio (kebanyakan laptop) dan Raspberry Pi belum bisa merekam -- aplikasinya
memberi tahu. Program merekam lewat `Buitenzorg.Media.Recorder` (`Start()`,
`Read(byte[])`, `Stop()`), yang butuh izin `microphone` di manifest paket.

## Gambar

`run mimage`, atau **Mulai > Multimedia > Gambar**. Daftarnya berisi gambar di
`/disk` dan folder scratch -- **BMP, PNG, JPEG (baseline dan progresif) dan
GIF** -- dan gambar terpilih ditampilkan pas di jendela. Zoom dengan Perkecil /
Pas / 1:1 / Perbesar, `+` `-` `0` `1` atau roda tetikus; seret (atau W A S D)
untuk menggeser gambar yang diperbesar; Kiri / Kanan atau `<` `>` untuk gambar
sebelumnya / berikutnya; **Slide show** (atau Spasi) berganti setiap 3 detik.
GIF animasi diputar. Tiga contoh ada di setiap image: `KEBUN.JPG`,
`ANGGREK.PNG` (transparan, tampil di atas papan catur) dan `DAUN.GIF` (12
bingkai).

![Gambar menampilkan ANGGREK.PNG (QEMU)](img/gambar.png)

## Video

![Video memutar *Fajar di Kebun Raya*](img/video.png)

`run mvideo`, atau **Mulai > Multimedia > Video**: memutar berkas AVI berisi
gambar Motion-JPEG dan suara PCM (`/disk/*.AVI`); Spasi putar / jeda, klik
bilah untuk melompat, **Dari awal** mengulang. Gambar mengikuti posisi suara
(yang benar-benar sudah diputar mixer), jadi keduanya tetap selaras. Setiap
image membawa klip orisinal 12 detik, *Fajar di Kebun Raya* (`FAJAR.AVI`:
320x180, 10 fps, suara 16 kHz), disintesis saat image dibangun. Belum
didukung: codec lain (H.264, VP9 ...), kontainer MP4/MKV, suara terkompresi.

## Untuk program

`Buitenzorg.Media.MediaPlayer`: `Load(path)` (WAV PCM 8/16-bit, mono/stereo,
laju apa pun), `Play`, `Pause`, `Stop`, `Seek`, `Position`, `Duration`,
`Title` (chunk LIST/INFO INAM), `Level`, `Ended`; panggil `Pump()` dari timer
UI (~100 ms) -- ia menjaga ~0,6 detik antre di mixer. Tingkat bawah:
`Audio.Write(data, offset, count, Audio.Format(laju, kanal, bit))`,
`Audio.QueuedMs`, `Audio.CanStream`, `Audio.AppVolume(pid, volume)`.

Gambar untuk program: `Buitenzorg.Media.Picture` -- `Picture.Load(path)`
(BMP, PNG, JPEG, GIF; null bila bukan gambar), `Width`, `Height`, `Format`,
`FrameCount`, lalu `Render(frame, piksel, dw, dh, sx, sy, sw, sh)` mengisi
array untuk satu `Blit` (mengembalikan jeda bingkai dalam ms). Dekode dan
penskalaan berjalan di host (`imgdecode.rs`, native `ImageInfo` /
`ImageDecodeScaled`, hasil dekode di-cache), jadi interpreter tidak pernah
mengulang per piksel. Di luar perangkat hanya BMP.

Syscall: **`AUDIO_WRITE` (62)** dan **`AUDIO_APP_VOLUME` (63)**. Di Terminal,
`mixer` mendaftar stream (`mixer PID VOLUME` mengaturnya).

## Cara kerja

`mixer.rs` mengubah setiap tulisan ke 48 kHz stereo 16-bit (interpolasi linear
untuk laju lain), menyimpan sampai dua detik per program, dan menjumlahkan
stream sesuai volumenya dengan saturasi. Pump driver mengisi ring: di AC'97 32
buffer DMA (680 ms) yang indeks valid terakhirnya bergerak di depan buffer yang
sedang diputar; di Intel HD Audio 16 halaman siklik (340 ms) di belakang posisi
link. Satu ring penuh hening menghentikan kartu; tulisan berikutnya
menyalakannya lagi. Cek boot: `MILESTONE: MIXER OK` (semua arsitektur) dan
`MILESTONE: AUDIO STREAM OK` (kartu PC).

## Batasan

- Streaming bekerja di kartu suara **PC** (AC'97, Intel HD Audio -- streaming
  diverifikasi di QEMU). Di ARM64 / RISC-V `virt` dan Raspberry Pi mixernya
  ada tetapi belum ada kartu yang streaming (`Audio.CanStream` = false; Pi
  tetap memutar bunyi sekali-putar di jack dan HDMI).
- Gambar: belum ada rotasi EXIF, thumbnail atau penyuntingan; bingkai GIF
  didekode di awal (cukup untuk animasi kecil).
- WAV, FLAC, MP3, MIDI, Ogg Vorbis dan Ogg Opus; Opus lebih dari dua kanal
  (surround) ditolak, Vorbis disimpan dua kanal pertamanya; synth MIDI
  belum punya SoundFont, reverb atau panning stereo; lagu di-decode
  utuh saat dimuat (beberapa detik untuk MP3 panjang di QEMU); merekam baru lewat AC'97 (belum HD Audio, virtio-snd atau Pi); meter
  menampilkan puncak, bukan spektrum.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
