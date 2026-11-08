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
| PageUp / PageDown, roda mouse | gulir ke atas |

## Baris perintah

```sh
ls /disk | grep -i elf | sort -r | head -n 3     # pipe
echo halo > /ram/A.TXT; cat < /ram/A.TXT >> /ram/B.TXT   # redirect
mkdir /ram/X && cp /ram/A.TXT /ram/X || echo gagal       # ; && ||
X=kebun; echo "nama: $X, status $?"                      # variabel, $?
ls /disk/H*.ELF                                          # glob * ?
```

Kutip: `'...'` apa adanya, `"..."` mengembangkan `$NAMA`, `${NAMA}`, `$?`; `\`
meloloskan satu karakter. `PATH` (bawaan `/disk:/ram`), `PWD`, `HOME`, `USER`
dan `SHELL` sudah ada; `env`/`set`/`export NAMA=nilai`, `unset`.

**Coreutils:** `cat grep head tail wc sort uniq tee cp mv mkdir rmdir touch
find hexdump/xxd seq basename dirname true false env set export unset history
test [`. Perintah lain (`ls`, `rm`, `ps`, `kill`, `run`, `bz`, `ipconfig`,
`ping`, `theme`, ...) ada di `help`.

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
* **Belum ada:** substitusi perintah `$(...)` dan aritmetika `$((...))`,
  fungsi, `case`, here-document, job latar (`&`).

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
