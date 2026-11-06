// More UI templates for JavaScript / TypeScript / Python: forms (text fields
// with input()), games, drawing and utilities -- so every language has each
// kind. Same immediate-mode API as UiTemplates.cs (docs/script-ui.md).

namespace MStudio;

public static class UiTemplates2
{
    static string Readme(string title, string what) =>
        title + "\n" + new string('=', title.Length) + "\n\n" + what + "\n\n" +
        "Aplikasi ber-UI: F5 di Studio membukanya di jendela sendiri (atau 'run MAIN.xx' di Terminal).\n" +
        "Form: input(x, y, w, id [, placeholder]) mengembalikan teks; setinput(id, teks); focus(id).\n" +
        "Panduan: docs/script-ui.md.\n";

    public static readonly Template[] All =
    {
        // ---------------- forms ----------------
        new Template("js-ui-form", "JavaScript UI", "Form", "Form Pendaftaran",
            "Tiga kotak isian, validasi (nama, email, umur) dan pesan hasil.", "MAIN.JS",
            ("MAIN.JS", """
// Form pendaftaran: kotak isian, validasi, dan pesan hasil
window("Form Pendaftaran", 420, 360);
let pesan = "";
let warnaPesan = 0xF3C969;
let terdaftar = 0;

function label(y, teks) { text(24, y, teks, 0x9CC9D9, 1); }
function kosongkan() { setinput("nama", ""); setinput("email", ""); setinput("umur", ""); }

while (running()) {
  clear(0x14201A);
  text(24, 16, "Pendaftaran Kebun Raya", 0xFFFFFF, 2);
  label(64, "Nama");
  let nama = input(24, 86, 372, "nama", "nama lengkap");
  label(128, "Email");
  let email = input(24, 150, 372, "email", "nama@contoh.id");
  label(192, "Umur");
  let umur = input(24, 214, 120, "umur", "tahun");
  if (button(24, 266, 140, 40, "Daftar") || key() == 10) {
    if (len(trim(nama)) < 3) {
      pesan = "Nama minimal 3 huruf"; warnaPesan = 0xE5484D;
    } else if (indexOf(email, "@") < 1 || indexOf(email, ".") < 0) {
      pesan = "Email tidak valid"; warnaPesan = 0xE5484D;
    } else if (int(umur) < 5 || int(umur) > 120) {
      pesan = "Umur harus 5-120 tahun"; warnaPesan = 0xE5484D;
    } else {
      terdaftar = terdaftar + 1;
      pesan = "Terima kasih, " + trim(nama) + "! (pendaftar ke-" + terdaftar + ")";
      warnaPesan = 0x7CC4A4;
      kosongkan();
    }
  }
  if (button(180, 266, 140, 40, "Kosongkan")) { kosongkan(); pesan = ""; }
  text(24, 322, pesan, warnaPesan, 1);
  present();
}
"""),
            ("README.TXT", Readme("Form Pendaftaran", "Klik kotak lalu ketik; Enter atau tombol Daftar memeriksa isinya."))),

        new Template("ts-ui-kurs", "TypeScript UI", "Form", "Konversi Mata Uang",
            "Isi jumlah rupiah, pilih mata uang; hasil dua desimal dengan bilangan bulat.", "MAIN.TS",
            ("MAIN.TS", """
// Konversi mata uang (kurs contoh, bukan data langsung)
window("Konversi Mata Uang", 420, 320);
let pilihan: number = 0;

function kode(i: number): string {
  if (i == 0) { return "USD"; } if (i == 1) { return "EUR"; } if (i == 2) { return "SGD"; }
  return "JPY";
}

// rupiah untuk 100 unit mata uang (contoh)
function kurs100(i: number): number {
  if (i == 0) { return 1620000; } if (i == 1) { return 1750000; } if (i == 2) { return 1210000; }
  return 10800;
}

function duaDesimal(x100: number): string {
  let sen: number = x100 % 100;
  let s: string = "" + sen;
  if (sen < 10) { s = "0" + s; }
  return "" + x100 / 100 + "," + s;
}

while (running()) {
  clear(0x14201A);
  text(24, 16, "Konversi Mata Uang", 0xFFFFFF, 2);
  text(24, 64, "Jumlah (Rupiah)", 0x9CC9D9, 1);
  let rp: number = int(input(24, 86, 220, "rp", "mis. 1500000"));
  for (let i: number = 0; i < 4; i = i + 1) {
    if (button(24 + i * 94, 140, 84, 36, kode(i))) { pilihan = i; }
  }
  frame(22 + pilihan * 94, 138, 88, 40, 0xF3C969, 9);
  let hasil: number = rp * 10000 / kurs100(pilihan);
  text(24, 204, "Rp " + rp + " =", 0xFFFFFF, 1);
  text(24, 232, duaDesimal(hasil) + " " + kode(pilihan), 0x7CC4A4, 2);
  text(24, 282, "Kurs contoh: 1 " + kode(pilihan) + " = Rp " + kurs100(pilihan) / 100, 0x6F8F7A);
  present();
}
"""),
            ("README.TXT", Readme("Konversi Mata Uang", "Uang dihitung dalam satuan sen (x100), karena belum ada bilangan pecahan."))),

        new Template("py-ui-bmi", "Python UI", "Form", "Kalkulator BMI",
            "Isi berat dan tinggi, hitung BMI dan kategorinya.", "MAIN.PY",
            ("MAIN.PY", """
# Kalkulator BMI: isi berat dan tinggi, lalu tekan Hitung (atau Enter)
window("Kalkulator BMI", 380, 330)
hasil = ""
kategori = ""
warna = 0xFFFFFF

def satu_desimal(x10):
    return str(x10 / 10) + "," + str(x10 % 10)

while running():
    clear(0x14201A)
    text(24, 16, "Kalkulator BMI", 0xFFFFFF, 2)
    text(24, 64, "Berat badan (kg)", 0x9CC9D9, 1)
    berat = input(24, 86, 160, "berat", "mis. 60")
    text(24, 128, "Tinggi badan (cm)", 0x9CC9D9, 1)
    tinggi = input(24, 150, 160, "tinggi", "mis. 165")
    if button(24, 200, 140, 40, "Hitung") or key() == 10:
        b = int(berat)
        t = int(tinggi)
        if b <= 0 or t <= 0:
            hasil = "Isi dengan angka"
            kategori = ""
            warna = 0xE5484D
        else:
            bmi10 = b * 100000 / (t * t)
            hasil = "BMI " + satu_desimal(bmi10)
            if bmi10 < 185:
                kategori = "Berat badan kurang"
                warna = 0x7FC8E8
            elif bmi10 < 250:
                kategori = "Normal"
                warna = 0x7CC4A4
            elif bmi10 < 300:
                kategori = "Berat badan berlebih"
                warna = 0xF3C969
            else:
                kategori = "Obesitas"
                warna = 0xE5484D
    text(24, 256, hasil, 0xFFFFFF, 2)
    text(24, 292, kategori, warna, 1)
    present()
"""),
            ("README.TXT", Readme("Kalkulator BMI", "BMI = berat / tinggi^2, dihitung x10 untuk satu angka desimal."))),

        // ---------------- games ----------------
        new Template("py-ui-tebak", "Python UI", "Game", "Tebak Angka",
            "Ketik tebakan 1-100; petunjuk lebih besar/kecil sampai benar.", "MAIN.PY",
            ("MAIN.PY", """
# Tebak angka 1-100: ketik tebakan, tekan Enter atau Tebak
window("Tebak Angka", 380, 300)
rahasia = random(100) + 1
coba = 0
pesan = "Aku memikirkan angka 1 sampai 100"
warna = 0x9CC9D9
menang = False
focus("tebak")

while running():
    clear(0x14201A)
    text(24, 16, "Tebak Angka", 0xFFFFFF, 2)
    text(24, 60, pesan, warna, 1)
    tebakan = input(24, 100, 140, "tebak", "1-100")
    if not menang and (button(180, 98, 120, 38, "Tebak") or key() == 10):
        t = int(tebakan)
        setinput("tebak", "")
        focus("tebak")
        if t < 1 or t > 100:
            pesan = "Ketik angka 1 sampai 100"
            warna = 0xE5484D
        else:
            coba = coba + 1
            if t < rahasia:
                pesan = str(t) + " terlalu kecil"
                warna = 0xF3C969
            elif t > rahasia:
                pesan = str(t) + " terlalu besar"
                warna = 0xF3C969
            else:
                pesan = "Benar! " + str(rahasia) + " dalam " + str(coba) + " tebakan"
                warna = 0x7CC4A4
                menang = True
    text(24, 160, "Tebakan: " + str(coba), 0xFFFFFF)
    if menang:
        if button(24, 200, 160, 40, "Main lagi"):
            rahasia = random(100) + 1
            coba = 0
            menang = False
            pesan = "Angka baru sudah dipilih"
            warna = 0x9CC9D9
            focus("tebak")
    present()
"""),
            ("README.TXT", Readme("Tebak Angka", "Strategi terbaik: pencarian biner, paling lama 7 tebakan."))),

        new Template("ts-ui-pong", "TypeScript UI", "Game", "Pong",
            "Panah atas/bawah atau seret mouse melawan komputer.", "MAIN.TS",
            ("MAIN.TS", """
// Pong: panah atas/bawah (atau tahan mouse dan geser) melawan komputer
window("Pong", 480, 320);
let py: number = 130;
let cy: number = 130;
let bx: number = 24000;
let by: number = 16000;
let vx: number = 400;
let vy: number = 250;
let skorKamu: number = 0;
let skorKomputer: number = 0;

function servis(arah: number): void {
  bx = 24000; by = 16000; vx = 400 * arah; vy = 250 - random(500);
}

while (running()) {
  let k: number = key();
  if (k == 63232) { py = py - 24; }
  if (k == 63233) { py = py + 24; }
  if (mousedown()) { py = mousey() - 30; }
  py = max(0, min(260, py));
  // komputer mengikuti bola, tapi tidak sempurna
  if (by / 100 > cy + 34) { cy = cy + 4; }
  if (by / 100 < cy + 26) { cy = cy - 4; }
  cy = max(0, min(260, cy));
  bx = bx + vx;
  by = by + vy;
  if (by < 600 || by > 31400) { vy = 0 - vy; }
  if (vx < 0 && bx < 2400 && bx > 1200 && by / 100 >= py && by / 100 <= py + 60) {
    vx = 0 - vx + 20;
    vy = vy + (by / 100 - py - 30) * 15;
  }
  if (vx > 0 && bx > 45600 && bx < 46800 && by / 100 >= cy && by / 100 <= cy + 60) { vx = 0 - vx - 20; }
  if (bx < 0) { skorKomputer = skorKomputer + 1; servis(1); }
  if (bx > 48000) { skorKamu = skorKamu + 1; servis(-1); }
  clear(0x0E2018);
  for (let y: number = 0; y < 320; y = y + 20) { rect(238, y, 4, 10, 0x2E5A42); }
  rect(12, py, 10, 60, 0x7CC4A4, 4);
  rect(458, cy, 10, 60, 0xE9C46A, 4);
  circle(bx / 100, by / 100, 6, 0xFFFFFF);
  text(180, 12, "" + skorKamu, 0x7CC4A4, 2);
  text(284, 12, "" + skorKomputer, 0xE9C46A, 2);
  present();
}
"""),
            ("README.TXT", Readme("Pong", "Posisi bola dalam 1/100 piksel supaya geraknya halus tanpa pecahan."))),

        // ---------------- drawing ----------------
        new Template("ts-ui-spirograf", "TypeScript UI", "Grafis", "Spirograf",
            "Pola hipotrokoid digambar bertahap; sinus dari pendekatan Bhaskara.", "MAIN.TS",
            ("MAIN.TS", """
// Spirograf: pola hipotrokoid digambar sedikit demi sedikit
window("Spirograf", 440, 440);
let R: number = 150;
let r: number = 52;
let d: number = 90;
let t: number = 0;
let warna: number = 0x7CC4A4;
let px: number = 0;
let py: number = 0;
let mulai: boolean = true;

// sin(derajat) x 1000 dengan rumus Bhaskara (tanpa pecahan)
function sin1000(deg: number): number {
  let x: number = ((deg % 360) + 360) % 360;
  let tanda: number = 1;
  if (x > 180) { x = x - 180; tanda = -1; }
  return tanda * 4000 * x * (180 - x) / (40500 - x * (180 - x));
}
function cos1000(deg: number): number { return sin1000(deg + 90); }

function titikX(a: number): number { return 220 + ((R - r) * cos1000(a) + d * cos1000(a * (R - r) / r)) / 1000; }
function titikY(a: number): number { return 240 + ((R - r) * sin1000(a) - d * sin1000(a * (R - r) / r)) / 1000; }

function pola(nr: number, nd: number, c: number): void { r = nr; d = nd; warna = c; mulai = true; }

while (running()) {
  if (mulai) { clear(0x0E2018); t = 0; px = titikX(0); py = titikY(0); mulai = false; }
  rect(0, 0, 440, 42, 0x1B3A2A);
  if (button(8, 5, 100, 32, "Bunga")) { pola(52, 90, 0x7CC4A4); }
  if (button(116, 5, 100, 32, "Bintang")) { pola(90, 70, 0xF3C969); }
  if (button(224, 5, 100, 32, "Cincin")) { pola(33, 60, 0xC792EA); }
  if (button(332, 5, 100, 32, "Acak")) {
    pola(20 + random(110), 30 + random(100), rgb(100 + random(155), 100 + random(155), 100 + random(155)));
  }
  for (let i: number = 0; i < 30 && t < 10800; i = i + 1) {
    t = t + 2;
    let x: number = titikX(t);
    let y: number = titikY(t);
    line(px, py, x, y, warna);
    px = x;
    py = y;
  }
  present();
}
"""),
            ("README.TXT", Readme("Spirograf", "Trigonometri dengan bilangan bulat: sin x 1000."))),

        new Template("py-ui-mandala", "Python UI", "Grafis", "Mandala Simetri",
            "Gambar dengan mouse; setiap goresan dicerminkan delapan kali.", "MAIN.PY",
            ("MAIN.PY", """
# Mandala: gambar dengan mouse, setiap goresan dicerminkan 8 kali
window("Mandala", 440, 470)
warna = 0xF3C969
kosong = True
px = -1
py = -1
cx = 220
cy = 255

def cermin(x1, y1, x2, y2):
    a1 = x1 - cx
    b1 = y1 - cy
    a2 = x2 - cx
    b2 = y2 - cy
    line(cx + a1, cy + b1, cx + a2, cy + b2, warna)
    line(cx - a1, cy + b1, cx - a2, cy + b2, warna)
    line(cx + a1, cy - b1, cx + a2, cy - b2, warna)
    line(cx - a1, cy - b1, cx - a2, cy - b2, warna)
    line(cx + b1, cy + a1, cx + b2, cy + a2, warna)
    line(cx - b1, cy + a1, cx - b2, cy + a2, warna)
    line(cx + b1, cy - a1, cx + b2, cy - a2, warna)
    line(cx - b1, cy - a1, cx - b2, cy - a2, warna)

def palet(x, c):
    rect(x, 8, 26, 26, c, 6)
    if warna == c:
        frame(x - 3, 5, 32, 32, 0xFFFFFF, 8)
    return clicked() and clickx() >= x and clickx() < x + 26 and clicky() < 36

while running():
    if kosong:
        clear(0x0B1610)
        ring(cx, cy, 200, 0x1B3A2A)
        kosong = False
    rect(0, 0, 440, 42, 0x1B3A2A)
    if palet(10, 0xF3C969):
        warna = 0xF3C969
    if palet(44, 0xE5484D):
        warna = 0xE5484D
    if palet(78, 0x7CC4A4):
        warna = 0x7CC4A4
    if palet(112, 0x7FC8E8):
        warna = 0x7FC8E8
    if palet(146, 0xC792EA):
        warna = 0xC792EA
    if button(330, 6, 100, 30, "Hapus"):
        kosong = True
    if mousedown() and mousey() > 46:
        if px >= 0:
            cermin(px, py, mousex(), mousey())
        px = mousex()
        py = mousey()
    else:
        px = -1
    present()
"""),
            ("README.TXT", Readme("Mandala", "Pencerminan titik terhadap pusat: (x, y), (-x, y), (y, x), ..."))),

        // ---------------- utilities ----------------
        new Template("js-ui-pomodoro", "JavaScript UI", "Utilitas", "Timer Pomodoro",
            "25 menit fokus, 5 menit istirahat, bilah kemajuan dan hitungan sesi.", "MAIN.JS",
            ("MAIN.JS", """
// Timer Pomodoro: 25 menit fokus, 5 menit istirahat
window("Pomodoro", 340, 300);
let fokus = true;
let jalan = false;
let sisa = 25 * 60 * 1000;
let terakhir = ticks();
let sesi = 0;

function dua(n) { if (n < 10) { return "0" + n; } return "" + n; }
function panjang() { if (fokus) { return 25 * 60 * 1000; } return 5 * 60 * 1000; }

while (running()) {
  let kini = ticks();
  if (jalan) { sisa = sisa - (kini - terakhir); }
  terakhir = kini;
  if (sisa <= 0) {
    if (fokus) { sesi = sesi + 1; }
    fokus = !fokus;
    sisa = panjang();
  }
  let warna = 0xE5484D;
  let judul = "Fokus";
  if (!fokus) { warna = 0x7CC4A4; judul = "Istirahat"; }
  clear(0x14201A);
  text(24, 16, judul, warna, 2);
  let detik = (sisa + 999) / 1000;
  let jam = dua(detik / 60) + ":" + dua(detik % 60);
  text(170 - textwidth(jam, 2) / 2, 70, jam, 0xFFFFFF, 2);
  rect(24, 120, 292, 14, 0x0B1610, 7);
  rect(24, 120, 292 * (panjang() - sisa) / panjang(), 14, warna, 7);
  let label = "Mulai";
  if (jalan) { label = "Jeda"; }
  if (button(24, 156, 140, 40, label)) { jalan = !jalan; }
  if (button(176, 156, 140, 40, "Ulang")) { jalan = false; sisa = panjang(); }
  if (button(24, 208, 292, 34, "Lewati")) { sisa = 0; }
  text(24, 262, "Sesi fokus selesai: " + sesi, 0x9CC9D9, 1);
  present();
}
"""),
            ("README.TXT", Readme("Timer Pomodoro", "Waktu dari ticks() (milidetik sejak program mulai)."))),
    };
}
