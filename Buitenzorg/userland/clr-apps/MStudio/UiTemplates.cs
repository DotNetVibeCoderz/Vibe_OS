// UI app templates for the script languages: JavaScript, TypeScript and
// Python programs with a window, run as their own process by the script host
// in CLRHOST.ELF (userland/clrhost/src/scriptui.rs). Immediate mode: each
// frame draws everything; button(...) is true in the frame after a click.

namespace MStudio;

public static class UiTemplates
{
    static string Readme(string title, string what) =>
        title + "\n" + new string('=', title.Length) + "\n\n" + what + "\n\n" +
        "Aplikasi ber-UI: F5 di Studio membukanya di jendela sendiri (atau 'run MAIN.xx' di Terminal).\n" +
        "API: window, clear, rect, frame, gradient, line, circle, ring, text, textwidth, button,\n" +
        "present, running, quit, mousex, mousey, mousedown, clicked, clickx, clicky, key, ticks,\n" +
        "wait, random, rgb, arg. Panduan: docs/script-ui.md.\n";

    public static readonly Template[] All =
    {
        new Template("js-ui-penghitung", "JavaScript UI", "Aplikasi UI", "Penghitung (tombol)",
            "Jendela dengan angka dan dua tombol - aplikasi ber-UI paling sederhana.", "MAIN.JS",
            ("MAIN.JS", """
// Penghitung: jendela, teks, dan tombol (immediate mode)
window("Penghitung", 320, 220);
let n = 0;
while (running()) {
  gradient(0, 0, 320, 220, 0x1B3A2A, 0x0E2018);
  text(24, 24, "Penghitung", 0x9CC9D9, 1);
  text(24, 64, "" + n, 0xFFFFFF, 2);
  if (button(24, 140, 124, 40, "Tambah")) { n = n + 1; }
  if (button(164, 140, 124, 40, "Reset")) { n = 0; }
  if (key() == 32) { n = n + 1; }
  present();
}
"""),
            ("README.TXT", Readme("Penghitung", "Tombol Tambah/Reset; tombol spasi juga menambah."))),

        new Template("js-ui-gambar", "JavaScript UI", "Grafis", "Papan Gambar",
            "Menggambar dengan mouse, palet warna, dan tombol hapus.", "MAIN.JS",
            ("MAIN.JS", """
// Papan gambar: tahan tombol mouse lalu gerakkan
window("Papan Gambar", 480, 360);
let warna = 0x2F7BE8;
let kosong = true;
let px = -1;
let py = -1;

function palet(x, c) {
  rect(x, 8, 28, 28, c, 6);
  if (warna == c) { frame(x - 3, 5, 34, 34, 0xFFFFFF, 8); }
  if (clicked() && clickx() >= x && clickx() < x + 28 && clicky() >= 8 && clicky() < 36) { warna = c; }
}

function goresan(x1, y1, x2, y2) {
  let langkah = max(abs(x2 - x1), abs(y2 - y1)) / 2 + 1;
  for (let i = 0; i <= langkah; i = i + 1) {
    circle(x1 + (x2 - x1) * i / langkah, y1 + (y2 - y1) * i / langkah, 4, warna);
  }
}

while (running()) {
  if (kosong) { clear(0xFAFAF5); kosong = false; }
  rect(0, 0, 480, 44, 0x1E3A2B);
  palet(10, 0x2F7BE8);
  palet(46, 0xE5484D);
  palet(82, 0x46A758);
  palet(118, 0xF5A524);
  palet(154, 0x111111);
  if (button(380, 7, 90, 30, "Hapus")) { kosong = true; }
  if (mousedown() && mousey() > 48) {
    if (px >= 0) { goresan(px, py, mousex(), mousey()); }
    px = mousex();
    py = mousey();
  } else {
    px = -1;
  }
  present();
}
"""),
            ("README.TXT", Readme("Papan Gambar", "Kanvas tidak dihapus tiap frame, jadi goresan tetap ada; toolbar digambar ulang."))),

        new Template("js-ui-tangkap", "JavaScript UI", "Game", "Game: Tangkap Bola",
            "Gerakkan papan dengan mouse atau panah kiri/kanan; bola makin cepat.", "MAIN.JS",
            ("MAIN.JS", """
// Game: tangkap bola yang jatuh dengan papan
window("Tangkap Bola", 360, 420);
let papan = 140;
let bx = 180;
let by = 0;
let cepat = 4;
let skor = 0;
let nyawa = 3;

function bolaBaru() {
  bx = 20 + random(320);
  by = 0;
}

while (running()) {
  let k = key();
  if (k == 63234) { papan = papan - 24; }
  if (k == 63235) { papan = papan + 24; }
  if (k == 27) { quit(); }
  if (mousex() >= 0 && mousedown()) { papan = mousex() - 40; }
  papan = max(0, min(280, papan));
  if (nyawa > 0) {
    by = by + cepat;
    if (by >= 380 && by <= 380 + cepat + 10 && bx >= papan - 6 && bx <= papan + 86) {
      skor = skor + 1;
      cepat = cepat + 1;
      bolaBaru();
    }
    if (by > 430) { nyawa = nyawa - 1; bolaBaru(); }
  }
  gradient(0, 0, 360, 420, 0x14301F, 0x061A10);
  circle(bx, by, 10, 0xF3C969);
  rect(papan, 390, 80, 12, 0x7CC4A4, 6);
  text(12, 10, "Skor " + skor + "   Nyawa " + nyawa, 0xFFFFFF, 1);
  if (nyawa <= 0) {
    text(80, 170, "Permainan selesai", 0xFFFFFF, 2);
    if (button(130, 220, 100, 36, "Ulangi")) { skor = 0; nyawa = 3; cepat = 4; bolaBaru(); }
  }
  present();
}
"""),
            ("README.TXT", Readme("Tangkap Bola", "Game loop: input, logika, gambar, present - 30 frame per detik."))),

        new Template("ts-ui-kalkulator", "TypeScript UI", "Aplikasi UI", "Kalkulator",
            "Kalkulator dengan 16 tombol, ditulis dengan tipe TypeScript.", "MAIN.TS",
            ("MAIN.TS", """
// Kalkulator ber-UI (bilangan bulat)
window("Kalkulator", 260, 330);
let layar: string = "0";
let akum: number = 0;
let op: string = "";
let baru: boolean = true;

function label(i: number): string {
  if (i == 0) { return "7"; } if (i == 1) { return "8"; } if (i == 2) { return "9"; } if (i == 3) { return "/"; }
  if (i == 4) { return "4"; } if (i == 5) { return "5"; } if (i == 6) { return "6"; } if (i == 7) { return "*"; }
  if (i == 8) { return "1"; } if (i == 9) { return "2"; } if (i == 10) { return "3"; } if (i == 11) { return "-"; }
  if (i == 12) { return "C"; } if (i == 13) { return "0"; } if (i == 14) { return "="; }
  return "+";
}

function angka(s: string): number {
  let n: number = 0;
  let neg: boolean = false;
  for (let i: number = 0; i < len(s); i = i + 1) {
    let c: string = substr(s, i, 1);
    if (c == "-") { neg = true; } else { n = n * 10 + ord(c) - 48; }
  }
  if (neg) { return 0 - n; }
  return n;
}

function hitung(a: number, b: number, o: string): number {
  if (o == "+") { return a + b; }
  if (o == "-") { return a - b; }
  if (o == "*") { return a * b; }
  if (o == "/") { if (b == 0) { return 0; } return a / b; }
  return b;
}

function tekan(t: string): void {
  if (t == "C") { layar = "0"; akum = 0; op = ""; baru = true; return; }
  if (t == "+" || t == "-" || t == "*" || t == "/" || t == "=") {
    akum = hitung(akum, angka(layar), op);
    layar = "" + akum;
    if (t == "=") { op = ""; } else { op = t; }
    baru = true;
    return;
  }
  if (baru || layar == "0") { layar = t; } else { layar = layar + t; }
  baru = false;
}

while (running()) {
  clear(0x14201A);
  rect(12, 12, 236, 60, 0x0B1610, 10);
  text(236 - textwidth(layar, 2), 28, layar, 0xFFFFFF, 2);
  for (let i: number = 0; i < 16; i = i + 1) {
    let x: number = 12 + (i % 4) * 60;
    let y: number = 88 + (i / 4) * 58;
    if (button(x, y, 52, 50, label(i))) { tekan(label(i)); }
  }
  present();
}
"""),
            ("README.TXT", Readme("Kalkulator", "Tombol digambar dalam perulangan; label dari fungsi (belum ada array)."))),

        new Template("ts-ui-pantul", "TypeScript UI", "Animasi", "Animasi Bola Memantul",
            "Gravitasi dan pantulan dengan bilangan bulat; klik untuk menambah dorongan.", "MAIN.TS",
            ("MAIN.TS", """
// Animasi: bola jatuh dan memantul (posisi dalam 1/100 piksel)
window("Bola Memantul", 400, 300);
let x: number = 4000;
let y: number = 2000;
let vx: number = 300;
let vy: number = 0;

while (running()) {
  vy = vy + 40;
  x = x + vx;
  y = y + vy;
  if (y > 27000) { y = 27000; vy = 0 - vy * 85 / 100; }
  if (x < 1500 || x > 38500) { vx = 0 - vx; }
  if (clicked()) { vy = -1200; }
  gradient(0, 0, 400, 300, 0x203A5A, 0x0A1424);
  rect(0, 285, 400, 15, 0x3F7A58);
  circle(x / 100, y / 100, 15, 0xE5484D);
  text(10, 10, "Klik untuk melempar bola", 0xFFFFFF, 1);
  present();
}
"""),
            ("README.TXT", Readme("Bola Memantul", "Fisika sederhana tanpa pecahan: semua posisi dikali 100."))),

        new Template("py-ui-stopwatch", "Python UI", "Aplikasi UI", "Stopwatch",
            "Mulai / berhenti / reset, waktu dari ticks().", "MAIN.PY",
            ("MAIN.PY", """
# Stopwatch dengan tombol Mulai/Berhenti dan Reset
window("Stopwatch", 300, 200)
jalan = False
mulai = 0
tersimpan = 0

def dua(n):
    if n < 10:
        return "0" + str(n)
    return str(n)

while running():
    total = tersimpan
    if jalan:
        total = tersimpan + ticks() - mulai
    clear(0x14201A)
    waktu = dua(total / 60000) + ":" + dua(total / 1000 % 60) + "." + dua(total / 10 % 100)
    text(150 - textwidth(waktu, 2) / 2, 50, waktu, 0xFFFFFF, 2)
    label = "Mulai"
    if jalan:
        label = "Berhenti"
    if button(20, 120, 120, 40, label):
        if jalan:
            tersimpan = total
            jalan = False
        else:
            mulai = ticks()
            jalan = True
    if button(160, 120, 120, 40, "Reset"):
        tersimpan = 0
        mulai = ticks()
    present()
"""),
            ("README.TXT", Readme("Stopwatch", "Python dengan fungsi, if/else dan waktu nyata."))),

        new Template("py-ui-kuis", "Python UI", "Pendidikan", "Kuis Pilihan Ganda",
            "Tiga soal tentang Bogor dengan skor di akhir.", "MAIN.PY",
            ("MAIN.PY", """
# Kuis pilihan ganda: klik jawaban yang benar
window("Kuis Bogor", 440, 320)
nomor = 0
skor = 0
pesan = ""
jumlah = 3

def soal(n):
    if n == 0:
        return "Kebun Raya Bogor didirikan tahun?"
    if n == 1:
        return "Bogor dijuluki kota ...?"
    return "Bunga raksasa di Kebun Raya?"

def pilihan(n, i):
    if n == 0:
        if i == 0:
            return "1817"
        if i == 1:
            return "1945"
        return "1900"
    if n == 1:
        if i == 0:
            return "Kembang"
        if i == 1:
            return "Hujan"
        return "Pahlawan"
    if i == 0:
        return "Anggrek"
    if i == 1:
        return "Melati"
    return "Bunga bangkai"

def kunci(n):
    if n == 0:
        return 0
    if n == 1:
        return 1
    return 2

while running():
    clear(0x14201A)
    if nomor < jumlah:
        text(20, 20, "Soal " + str(nomor + 1) + " dari " + str(jumlah), 0x9CC9D9, 1)
        text(20, 52, soal(nomor), 0xFFFFFF)
        for i in range(0, 3):
            if button(20, 100 + i * 52, 400, 42, pilihan(nomor, i)):
                if i == kunci(nomor):
                    skor = skor + 1
                    pesan = "Benar!"
                else:
                    pesan = "Kurang tepat"
                nomor = nomor + 1
        text(20, 270, pesan, 0xF3C969, 1)
    else:
        text(20, 100, "Skor kamu: " + str(skor) + " dari " + str(jumlah), 0xFFFFFF, 2)
        if button(20, 170, 160, 40, "Main lagi"):
            nomor = 0
            skor = 0
            pesan = ""
    present()
"""),
            ("README.TXT", Readme("Kuis", "Soal dan jawaban dari fungsi; ubah atau tambah soal di sana."))),

        new Template("py-ui-warna", "Python UI", "Grafis", "Pencampur Warna RGB",
            "Atur merah/hijau/biru dengan tombol dan lihat hasilnya.", "MAIN.PY",
            ("MAIN.PY", """
# Pencampur warna: tombol - dan + untuk tiap kanal
window("Pencampur Warna", 360, 270)
r = 124
g = 196
b = 164

def baris(y, nama, nilai):
    text(20, y + 8, nama + " " + str(nilai), 0xFFFFFF, 1)
    hasil = nilai
    if button(200, y, 60, 32, "-"):
        hasil = max(0, nilai - 15)
    if button(270, y, 60, 32, "+"):
        hasil = min(255, nilai + 15)
    return hasil

while running():
    clear(0x14201A)
    r = baris(20, "Merah", r)
    g = baris(60, "Hijau", g)
    b = baris(100, "Biru ", b)
    rect(20, 150, 320, 96, rgb(r, g, b), 14)
    text(36, 186, "rgb(" + str(r) + ", " + str(g) + ", " + str(b) + ")", 0xFFFFFF, 1)
    present()
"""),
            ("README.TXT", Readme("Pencampur Warna", "Fungsi yang menggambar sekaligus mengembalikan nilai baru."))),
    };
}
