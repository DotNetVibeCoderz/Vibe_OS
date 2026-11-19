# Shell dan skrip shell

**Terminal** di desktop menjalankan shell bawaan Buitenzorg (`bzsh`). Rasanya
seperti shell POSIX kecil: pipe, redirect, variabel, skrip dan alias, di atas
perintah OS sendiri plus sekumpulan coreutils.

## Mengedit baris perintah

| Tombol | Fungsi |
|---|---|
| ← / → | geser kursor |
| Home / End, Ctrl+A / Ctrl+E | awal / akhir baris |
| Delete, Backspace | hapus di / sebelum kursor |
| Ctrl+U / Ctrl+K / Ctrl+W | hapus ke awal / ke akhir / kata sebelumnya |
| ↑ / ↓ | riwayat perintah (disimpan di `/disk/HISTORY.TXT`, 200 baris) |
| Tab | lengkapi nama perintah atau berkas |
| Ctrl+L / Ctrl+C | bersihkan layar / batalkan baris |
| Ctrl+V | tempel papan klip sistem (baris baru menjadi `; `) |
| PageUp / PageDown, roda mouse | gulir ke atas |

## Baris perintah

```sh
ls /disk | grep -i elf | sort -r | head -n 3     # pipe
echo halo > /ram/A.TXT; cat < /ram/A.TXT >> /ram/B.TXT   # redirect
mkdir /ram/X && cp /ram/A.TXT /ram/X || echo gagal       # ; && ||
X=kebun; echo "nama: $X, status $?"                      # variabel, $?
ls /disk/H*.ELF                                          # glob * ?
echo "$(ls /disk | wc -l) berkas, $(( 6 * 7 )) jawaban"  # $(perintah), $((hitung))
for f in $(ls /ram); do echo "- $f"; done                # $(...) dipecah jadi kata
ls /disk | clip; paste                                   # papan klip sistem
```

`$(perintah)` menjalankan baris perintah dan menaruh keluarannya di situ (tanpa
kutip dipecah menjadi kata, dengan kutip tetap satu string; boleh bersarang).
`$((ekspresi))` adalah aritmetika bilangan bulat: `+ - * / %`, perbandingan
`< <= > >= == !=` (1 atau 0), `&& || !`, kurung, angka dan variabel (`$X` atau
cukup `X`; yang tidak ada = 0); pembagian dengan nol menggagalkan perintah.

**Warna:** Terminal menggambar kode warna ANSI (`ESC[31m` ... `ESC[0m`: warna
teks 30-37 / 90-97, latar 40-47, tebal = terang) yang dicetak program;
`echo -e '\e[32mhijau\e[0m biasa'` untuk mencobanya (kutip tunggal menjaga
garis miring terbalik; `-e` juga mengenal `\n` dan `\t`).

Kutip: `'...'` apa adanya, `"..."` mengembangkan `$NAMA`, `${NAMA}`, `$?`; `\`
meloloskan satu karakter. `PATH` (bawaan `/disk:/ram`), `PWD`, `HOME`, `USER`
dan `SHELL` sudah ada; `env`/`set`/`export NAMA=nilai`, `unset`.

**Coreutils:** `cat grep head tail wc sort uniq tee cp mv mkdir rmdir touch
find hexdump/xxd seq basename dirname true false env set export unset history
test [ clip paste`. **Sistem:** `ps`, `top` (proses menurut waktu CPU, memori,
uptime), `free` (RAM, frame, heap kernel), `uptime`, `dmesg`, `df`, `lsusb`.
Perintah lain (`ls`, `rm`, `kill`, `run`, `bz`, `ipconfig`, `ping`, `theme`,
...) ada di `help`.

**Papan klip:** satu teks yang dipakai bersama oleh Terminal dan semua app
(Ctrl+C / X / V di kotak teks dan di Studio). `clip TEKS` atau `... | clip`
menyalin, `paste` mencetaknya, Ctrl+V menempelkannya di baris perintah.

## Tab dan job

**Tab.** Satu jendela Terminal menampung sampai 8 sesi. Masing-masing punya
scrollback, baris input, direktori kerja dan variabelnya sendiri; riwayat
perintah dipakai bersama. Kalau tab lebih dari satu, baris pertama adalah
bilah tab (`[1:/disk]  2:/ram*` -- `*` = ada job berjalan).

| Tombol / perintah | |
|---|---|
| Ctrl+T, `tab new` | tab baru |
| Ctrl+N / Ctrl+P, `tab N` | tab berikutnya / sebelumnya, tab N |
| Ctrl+D di baris kosong, `tab close` | tutup tab (program yang berjalan tetap jalan) |
| `tab` | daftar tab |

**Job.** Program yang dijalankan dari sebuah tab (`run hello`, atau cukup
`hello`) adalah *job* tab itu, dan apa yang dicetaknya muncul di tab itu.

| | |
|---|---|
| `run X` | foreground: prompt menunggu sampai X selesai; **Ctrl+C** menghentikannya, **Ctrl+Z** membiarkannya jalan di latar belakang |
| `run X &` | latar belakang: prompt langsung kembali (`[1] 1004`); tab memberi tahu `[1]  Selesai (kode 0)` saat selesai |
| `jobs` | job tab ini, yang berjalan maupun selesai, dengan kode keluarnya |
| `fg [%N]` | tunggu lagi job yang berjalan (yang terbaru bila tanpa argumen) |
| `kill %N` | hentikan job N |

Aplikasi berjendela yang dijalankan di foreground membuat prompt menunggu
sampai jendelanya ditutup -- tambahkan `&`, atau tekan Ctrl+Z. Batasan: tidak
ada stdin (program membaca keyboard hanya lewat jendelanya sendiri), jadi `fg`
hanya menunggu; job milik Terminal (`jobs` dari `bz run` mengatakannya);
program dari tab yang ditutup tetap berjalan tanpa terdaftar. Boot self-test
menjalankan semuanya lewat penekanan tombol (`MILESTONE: TERMINAL TABS OK`).

## Konsol: aplikasi terminal

**Konsol** (`run mkonsol`, Mulai > Utilitas) adalah terminal di jendelanya
sendiri, di samping Terminal bawaan desktop: tab (Ctrl+T baru, Ctrl+W tutup,
Ctrl+N / Ctrl+P pindah, atau klik strip; `+` menambah), masing-masing dengan
folder dan riwayat sendiri; scrollback 2000 baris (roda, PageUp/PageDown);
warna ANSI; seret di atas baris lalu Ctrl+C untuk menyalin, Ctrl+V menempel;
F2 mengganti huruf normal/besar; Tab melengkapi nama berkas; Atas/Bawah
menelusuri riwayat (dimulai dari `/disk/HISTORY.TXT` Terminal). Perintah
berjalan di shell yang sama dengan Terminal; program yang dimulai dengan `run`
didaftar oleh `jobs` dan dihentikan dengan `kill %N`. Perintah yang sedang
berjalan tidak bisa disela (Ctrl+C hanya mengosongkan baris input).

## Alias dan `which`

```sh
alias lsd='ls /disk'      # definisikan
lsd | grep -c ELF         # kata pertama tiap tahap pipe diganti
alias                     # daftar;  unalias lsd  /  unalias -a
which lsd cat ls hello    # alias, bawaan, skrip, paket atau program
```

## Skrip

Skrip adalah berkas teks berakhiran `.SH` (volume memakai FAT, nama 8.3).
Jalankan dengan `sh FILE [arg]`, `source FILE` / `. FILE`, atau cukup namanya:
path (`/disk/BUILD.SH`, `./BUILD.SH`) atau `build` bila `BUILD.SH` ada di
folder `$PATH`.

```sh
# /disk/SALAM.SH -- sapa setiap argumen
if [ $# -eq 0 ]; then
    echo "pakai: salam NAMA..."
    exit 1
fi
for nama in $@; do
    echo "Halo, $nama!"
done
touch /ram/BUSY
while test -f /ram/BUSY; do
    echo "bekerja..."
    rm /ram/BUSY
done
```

* `$0`..`$9`, `$#`, `$@` (satu kata per argumen, juga di dalam `"$@"`).
* `if CMD; then ...; elif CMD; then ...; else ...; fi`,
  `for X in KATA; do ...; done` (tanpa `in`: argumen skrip),
  `while CMD; do ...; done`, `exit [N]`, komentar `#`; beberapa pernyataan
  dalam satu baris dipisah `;`.
* Kondisi adalah perintah; `test` / `[ ]` mengenal `-e -f -d PATH`,
  `-z -n STR`, `A = B`, `A != B`, `N -eq -ne -lt -le -gt -ge M`, `!`, `-a`, `-o`.
* **Fungsi:** `nama() { ...; }` atau `function nama { ...; }`, dalam satu
  baris atau beberapa; dipanggil seperti perintah dengan `$1`..`$9`, `$#`,
  `$@` sendiri; `return [N]` keluar dari fungsi; rekursi jalan (8 tingkat:
  fungsi dan skrip berbagi batas sarang). `functions` menampilkan daftarnya;
  `type NAMA` membedakan fungsi dari perintah. Simpan fungsi Anda di
  `/disk/PROFILE.SH`.

```sh
fakt() {
    if [ $1 -le 1 ]; then echo 1; return 0; fi
    echo $(( $1 * $(fakt $(( $1 - 1 ))) ))
}
fakt 5          # 120
```

* **`local NAMA[=nilai]`** di dalam fungsi (atau skrip) mengembalikan nilai
  variabel sebelumnya -- atau menghapusnya -- saat fungsi selesai.
* **`case KATA in pola) ...;; esac`**: pola memakai `*` dan `?`, `a|b`
  berarti salah satu, pola berkutip dibaca harfiah, `$X` di dalam pola
  diperluas; yang pertama cocok dijalankan, `*)` menangkap sisanya.
* **Here-document:** `cat <<EOF` mengirim baris-baris berikutnya, sampai baris
  `EOF`, ke input perintah; `$X`, `$(cmd)` dan `$((..))` diperluas kecuali
  pembatasnya berkutip (`<<'EOF'`); `<<-EOF` membuang tab di awal baris. Di
  skrip (beberapa baris), bukan di satu baris yang diketik.

```sh
case $1 in
  start|mulai) echo memulai ;;
  *.TXT)       cat $1 ;;
  *)           echo "tidak dikenal: $1" ;;
esac
```

**Batas:** skrip berjalan di task kernel milik desktop, jadi satu run paling
banyak 20.000 pernyataan dan satu `while` paling banyak 5.000 putaran; skrip
yang lepas kendali berhenti dengan pesan, bukan membekukan desktop. Skrip boleh
memanggil skrip lain sampai 8 tingkat.

## Profil saat mulai

`/disk/PROFILE.SH` dijalankan setiap kali Terminal dibuka — tempat untuk alias
dan variabel Anda:

```sh
alias ll='ls /disk'
alias h='history'
EDITOR=editor
```

Implementasi: `kernel/bzkernel/src/shell.rs` (parsing, pipe), `bzsh.rs`
(alias, skrip, `which`), `coreutils.rs`, `terminal.rs` (edit baris, riwayat,
pelengkapan).

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin Kang Fadhil.*
