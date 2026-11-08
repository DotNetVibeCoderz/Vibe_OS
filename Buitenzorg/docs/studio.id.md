# Buitenzorg Studio

**Studio** adalah IDE di dalam Buitenzorg OS: tulis program, jalankan dengan
F5, baca keluaran dan galatnya — tanpa meninggalkan desktop. Buka dari ikon
desktop **Studio**, menu Mulai → Kantor → Studio, atau `run mstudio`.

![Buitenzorg Studio](img/studio.png)

## Jendela

| Bagian | Fungsi |
|---|---|
| Panel **Proyek** (kiri, Ctrl+B) | Berkas folder proyek; klik folder untuk masuk, `../` untuk naik, berkas untuk membuka. **+ Proyek baru** / **+ Berkas baru**. |
| Tab + editor | Satu tab per berkas (`*` = belum disimpan). Nomor baris, pewarnaan sintaks (C#, C# script, JavaScript, TypeScript, Python, shell, JSON), baris aktif, tanda merah di baris yang bergalat. |
| **Jalankan (F5)** | Menyimpan semua berkas lalu menjalankan berkas aktif: `.CSX` lewat engine bytecode [C# script](csharp-script.id.md), `.JS`/`.TS`/`.PY` lewat interpreter skrip kernel, `.SH` lewat shell; skrip yang memanggil `window(...)` terbuka sebagai aplikasi berjendela sendiri ([aplikasi UI](script-ui.id.md)). |
| **Debug (F6)** | Menjalankan skrip di bawah debugger (lihat di bawah). |
| **Keluaran** | Keluaran program dan lama eksekusinya. |
| **Masalah** | Galat beserta berkas dan barisnya (`parse: baris 2: ...`); klik untuk melompat. |
| **Terminal** | Shell yang sama dengan Terminal desktop (pipe, skrip, `bz`, ...). |
| **Debug** | Henti debugger, tumpukan panggilan, variabel dan keluaran sejauh itu. |
| Bar status | Berkas, bahasa, baris dan kolom, status simpan, path. |

## Tombol

| Tombol | Fungsi |
|---|---|
| F5 | jalankan berkas aktif |
| F6 | debug berkas aktif |
| F9 | breakpoint hidup / mati di baris kursor (atau klik nomor baris) |
| F10 / F11 | henti debugger berikutnya / sebelumnya |
| Ctrl+S / Ctrl+W | simpan / tutup tab |
| Ctrl+N | proyek baru (pilih template) |
| Ctrl+Z / Ctrl+Y | urungkan / ulangi (ketikan dikelompokkan per kata) |
| Ctrl+D / Ctrl+K | duplikat / hapus baris |
| Ctrl+F / Ctrl+R | cari / ganti (bar di atas editor; Enter = berikutnya) |
| Ctrl+G | ke baris |
| Tab, Enter | indentasi; baris baru mengikuti indentasi (satu tingkat lebih setelah `{`, `(`, `[`, `:` Python, atau `then`/`do` shell); `}` sendirian mundur satu tingkat |
| Home | karakter pertama yang bukan spasi, lalu kolom 0 |
| Ctrl+B / Ctrl+T | panel proyek / tema terang-gelap |

![Proyek baru](img/studio-new-project.png)

## Template proyek (52)

**Ctrl+N** membuka pemilih template; tiap template adalah proyek kecil yang
langsung bisa dijalankan (template C# dibangun di PC dan membawa `BUILD.TXT`).

| Bahasa | Kasus penggunaan |
|---|---|
| C# script | halo · List & Dictionary · analisis teks · algoritma pengurutan · exception · statistik & matematika |
| C# script UI | penghitung · game Ular · grafik fungsi · buku kontak · daftar tugas |
| JavaScript | Halo dunia · rekursi (Fibonacci, faktorial) · prima, FPB, KPK · FizzBuzz · tabel konversi satuan · simulasi tabungan |
| JavaScript UI | penghitung · papan gambar · game tangkap bola · form pendaftaran · timer Pomodoro |
| TypeScript | Halo bertipe/interface · statistik nilai siswa · permainan dadu dua pemain (acak LCG) · struk kasir |
| TypeScript UI | kalkulator · animasi bola memantul · konversi mata uang · Pong · spirograf |
| Python | Halo · tabel perkalian · tebak angka (pencarian biner) · kalender (kabisat, Zeller) · barisan Collatz |
| Python UI | stopwatch · kuis pilihan ganda · pencampur warna RGB · kalkulator BMI · tebak angka · mandala |
| Skrip shell | argumen/perulangan/kondisi · cadangan berkas teks · laporan sistem · pembersih berkas sementara · profil Terminal dengan alias |
| C# | konsol native (bflat) · desktop native (Buitenzorg.UI) · app managed (RustCLR) |
| Paket | proyek `.bzpkg` (manifest + skrip) |
| JSON | berkas data |

Proyek disimpan di `/disk/PROYEK/<NAMA>` (nama folder maks 8 huruf;
`/ram/PROYEK` bila tidak ada `/disk`). Saat pertama dibuka, Studio membuat
`HALO` dari template "Halo Dunia".

`run mstudio --list` menampilkan katalognya; `run mstudio --export /ram/T`
menulis berkas utama setiap template sebagai `/ram/T/T01.CSX` ... `T52.JSON`,
sehingga satu template bisa langsung dicoba dari Terminal (`run /ram/T/T21.JS`).

![Template UI berjalan berdampingan](img/studio-ui-templates.png)

## Kemampuan bahasa skrip

Interpreter kernel (`script.rs`) adalah subset kecil: bilangan bulat dan
string, `+ - * / %`, perbandingan, `&& || !`, fungsi dan rekursi, `if/else`
(`elif` di Python), `while`, `for` (gaya C di JS/TS, `range()` di Python),
`print` / `console.log`, `str`, `len`, `abs`. Belum ada array, objek, pecahan
atau import; setiap run punya batas langkah. Galat parse membawa nomor baris
yang tampil di **Masalah**.

## Debugger (F6)

Skrip **JavaScript, TypeScript, Python dan C# script** bisa di-debug:

1. Pasang breakpoint: **F9** di baris kursor, atau klik nomor baris (titik
   merah). Tanpa breakpoint = berhenti di setiap baris.
2. **F6** (atau **Debug (F6)**): berkas dijalankan sekali di interpreter kernel
   dengan debugger aktif, yang merekam keadaan program di setiap henti.
3. Tab **Debug** mendaftar henti-hentinya (`#3  baris 7  fib  (n=2)`); **F10**
   / **F11** maju / mundur. Editor menandai barisnya, panel menampilkan
   **tumpukan panggilan**, **variabel lokal**, **global** (list dan dictionary
   beserta isinya) dan keluaran yang tercetak sampai titik itu.

![Debugger di Studio: breakpoint di baris 3, hentinya, variabel global](img/studio-debugger.png)

Ini debugger *rekam, lalu putar ulang*: bisa mundur dalam waktu, tetapi tidak
bisa mengubah nilai lalu melanjutkan. Paling banyak 400 henti direkam per
eksekusi. Program native di-debug dengan `dbg` di Terminal
([Debugging](debugging.id.md)). Rekaman yang sama tersedia dari shell:
`script debug js MAIN.JS 4,9` (atau `all`).

**Skrip UI** (yang membuka jendela) juga bisa di-debug: F6 menjalankan
aplikasinya di jendelanya sendiri *sambil merekam* -- klik tombolnya, ketik
di dalamnya -- dan saat jendelanya ditutup, henti-hentinya muncul di tab
Debug. Pasang breakpoint di tempat penanganan event (baris yang bereaksi pada
klik), jangan hanya mengandalkan "setiap baris": tanpa breakpoint, 400 henti
habis di beberapa frame pertama. Di baliknya: `script debug` menjawab
`@dbgui PID FILE` dan menjalankan `CLRHOST.ELF FILE --bzdebug BARIS LAPORAN`;
Studio menanyakan `script debug status PID FILE` tiap setengah detik sampai
laporannya ada.

## Batasan (jujur)

- C# belum dikompilasi di dalam OS: template C# menjelaskan build di PC
  (bflat / `dotnet build`, `bz pack`, deploy).
- Debugger memutar ulang eksekusi yang direkam (tidak bisa mengubah nilai di
  tengah jalan); henti skrip UI baru tampil setelah jendelanya ditutup. Belum
  ada seleksi mouse atau clipboard di editor.
- Berkas besar: editor hanya menggambar baris yang terlihat, tetapi berjalan di
  interpreter RustCLR — berkas beberapa ribu baris masih nyaman.

Implementasi: `userland/clr-apps/MStudio` (`Studio.cs`, `CodeEditor.cs`,
`Syntax.cs`, `Templates.cs`, `Debugger.cs`); menjalankan lewat syscall **`SHELL_RUN` (58)** —
`Buitenzorg.App.Shell.Run(line)`. Self-test: 32 pemeriksaan di perangkat (menjalankan setiap template yang bisa dijalankan, yang ber-UI sebagai proses headless, dan men-debug satu skrip UI), bagian dari
`Managed --selftest` saat boot.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin Kang Fadhil.*
