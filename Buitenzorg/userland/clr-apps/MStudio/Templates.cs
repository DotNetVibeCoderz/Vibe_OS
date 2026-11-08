// Project templates of Buitenzorg Studio: many small, runnable starting points
// per language and use case. JavaScript, TypeScript, Python and shell
// templates run on the device (F5) through the kernel's script interpreter
// (integers, strings, functions, if/else, while, for, print/str/len/abs) and
// the shell; C# templates are built on a PC (bflat / dotnet) and come with
// BUILD.TXT. Every file name is 8.3 (the device's volumes are FAT).

namespace MStudio;

public sealed class Template
{
    public string Id { get; }
    public string Language { get; }   // display group
    public string UseCase { get; }    // what it is for
    public string Title { get; }
    public string Description { get; }
    public string Entry { get; }      // the file opened first and run by F5
    public (string Name, string Text)[] Files { get; }

    public Template(string id, string language, string useCase, string title, string description, string entry, params (string, string)[] files)
    {
        Id = id; Language = language; UseCase = useCase; Title = title; Description = description; Entry = entry; Files = files;
    }

    public override string ToString() => Language + " - " + Title;
}

public static class Templates
{
    static string Readme(string title, string what, string run) =>
        title + "\n" + new string('=', title.Length) + "\n\n" + what + "\n\nMenjalankan: " + run + "\n";

    public static readonly Template[] All =
    {
        // ---------------- JavaScript ----------------
        new Template("js-halo", "JavaScript", "Dasar", "Halo Dunia",
            "Variabel, string, dan console.log - titik awal paling sederhana.", "MAIN.JS",
            ("MAIN.JS", """
// Halo Dunia - program JavaScript pertama di Buitenzorg OS
let nama = "Buitenzorg";
let versi = 1;
console.log("Halo, dunia! Salam dari " + nama + " v" + versi);
console.log("Ubah teks di atas lalu tekan F5 untuk menjalankan lagi.");
"""),
            ("README.TXT", Readme("Halo Dunia (JavaScript)", "Contoh paling kecil: variabel dan console.log.", "F5 di Studio, atau 'script js MAIN.JS' di Terminal."))),

        new Template("js-rekursi", "JavaScript", "Algoritma", "Rekursi: Fibonacci & Faktorial",
            "Fungsi yang memanggil dirinya sendiri, dengan tabel hasil.", "MAIN.JS",
            ("MAIN.JS", """
// Rekursi: Fibonacci dan faktorial
function fib(n) {
  if (n < 2) { return n; }
  return fib(n - 1) + fib(n - 2);
}

function faktorial(n) {
  if (n <= 1) { return 1; }
  return n * faktorial(n - 1);
}

for (let i = 0; i <= 12; i = i + 1) {
  console.log("n=" + i + "  fib=" + fib(i) + "  faktorial=" + faktorial(i));
}
"""),
            ("README.TXT", Readme("Rekursi", "Fibonacci dan faktorial sebagai fungsi rekursif.", "F5."))),

        new Template("js-prima", "JavaScript", "Algoritma", "Bilangan Prima, FPB & KPK",
            "Uji prima, daftar prima, dan algoritma Euclid.", "MAIN.JS",
            ("MAIN.JS", """
// Bilangan prima, FPB (Euclid) dan KPK
function prima(n) {
  if (n < 2) { return false; }
  let d = 2;
  while (d * d <= n) {
    if (n % d == 0) { return false; }
    d = d + 1;
  }
  return true;
}

function fpb(a, b) {
  while (b != 0) {
    let t = a % b;
    a = b;
    b = t;
  }
  return a;
}

function kpk(a, b) { return a / fpb(a, b) * b; }

let daftar = "";
let jumlah = 0;
for (let n = 2; n < 60; n = n + 1) {
  if (prima(n)) { daftar = daftar + n + " "; jumlah = jumlah + 1; }
}
console.log("Prima di bawah 60 (" + jumlah + "): " + daftar);
console.log("FPB(84, 36) = " + fpb(84, 36));
console.log("KPK(4, 6) = " + kpk(4, 6));
"""),
            ("README.TXT", Readme("Prima, FPB & KPK", "Perulangan while dan operator modulo.", "F5."))),

        new Template("js-fizzbuzz", "JavaScript", "Latihan logika", "FizzBuzz",
            "Latihan klasik if / else if / else.", "MAIN.JS",
            ("MAIN.JS", """
// FizzBuzz: kelipatan 3 -> Fizz, kelipatan 5 -> Buzz, keduanya -> FizzBuzz
for (let i = 1; i <= 30; i = i + 1) {
  if (i % 15 == 0) {
    console.log("FizzBuzz");
  } else if (i % 3 == 0) {
    console.log("Fizz");
  } else if (i % 5 == 0) {
    console.log("Buzz");
  } else {
    console.log(i);
  }
}
"""),
            ("README.TXT", Readme("FizzBuzz", "Percabangan berantai.", "F5."))),

        new Template("js-konversi", "JavaScript", "Utilitas", "Tabel Konversi Suhu & Satuan",
            "Fungsi kecil untuk Celsius/Fahrenheit/Kelvin dan km/mil.", "MAIN.JS",
            ("MAIN.JS", """
// Konversi suhu dan jarak (bilangan bulat)
function keFahrenheit(c) { return c * 9 / 5 + 32; }
function keKelvin(c) { return c + 273; }
function kmKeMil(km) { return km * 621 / 1000; }

console.log("Celsius  Fahrenheit  Kelvin");
for (let c = -10; c <= 40; c = c + 10) {
  console.log(c + "  ->  " + keFahrenheit(c) + "  ->  " + keKelvin(c));
}
console.log("");
console.log("100 km = " + kmKeMil(100) + " mil (dibulatkan ke bawah)");
"""),
            ("README.TXT", Readme("Konversi", "Fungsi murni dan tabel.", "F5."))),

        new Template("js-tabungan", "JavaScript", "Simulasi", "Simulasi Tabungan & Bunga",
            "Setoran bulanan dengan bunga majemuk, dalam rupiah bulat.", "MAIN.JS",
            ("MAIN.JS", """
// Simulasi tabungan: setoran tiap bulan, bunga 6% per tahun (0,5% per bulan)
let saldo = 0;
let setoran = 500000;
let bunga = 0;
for (let bulan = 1; bulan <= 12; bulan = bulan + 1) {
  saldo = saldo + setoran;
  let b = saldo * 5 / 1000;
  bunga = bunga + b;
  saldo = saldo + b;
  console.log("bulan " + bulan + ": saldo Rp " + saldo);
}
console.log("total bunga setahun: Rp " + bunga);
"""),
            ("README.TXT", Readme("Simulasi Tabungan", "Perulangan dengan akumulasi.", "F5."))),

        // ---------------- TypeScript ----------------
        new Template("ts-halo", "TypeScript", "Dasar", "Halo TypeScript",
            "Anotasi tipe, interface dan type alias (dihapus saat dijalankan).", "MAIN.TS",
            ("MAIN.TS", """
// Halo TypeScript - tipe diperiksa oleh pembaca, dihapus saat dijalankan
interface Salam { teks: string; }
type Angka = number;

function sapa(nama: string, kali: Angka): string {
  return "Halo " + nama + " (ke-" + kali + ")";
}

for (let i: number = 1; i <= 3; i = i + 1) {
  console.log(sapa("Buitenzorg", i));
}
"""),
            ("README.TXT", Readme("Halo TypeScript", "TypeScript ditranspilasi (tipe dihapus) lalu dijalankan sebagai JavaScript.", "F5, atau 'script ts MAIN.TS'."))),

        new Template("ts-nilai", "TypeScript", "Analisis data", "Statistik Nilai Siswa",
            "Nilai minimum, maksimum, rata-rata dan predikat.", "MAIN.TS",
            ("MAIN.TS", """
// Statistik nilai: data disimpan sebagai fungsi indeks (tanpa array)
function nilai(i: number): number {
  if (i == 0) { return 78; }
  if (i == 1) { return 92; }
  if (i == 2) { return 65; }
  if (i == 3) { return 88; }
  if (i == 4) { return 71; }
  return 84;
}

function predikat(n: number): string {
  if (n >= 85) { return "A"; }
  if (n >= 70) { return "B"; }
  return "C";
}

let jumlahSiswa: number = 6;
let total: number = 0;
let terendah: number = 100;
let tertinggi: number = 0;
for (let i: number = 0; i < jumlahSiswa; i = i + 1) {
  let n: number = nilai(i);
  total = total + n;
  if (n < terendah) { terendah = n; }
  if (n > tertinggi) { tertinggi = n; }
  console.log("siswa " + (i + 1) + ": " + n + " (" + predikat(n) + ")");
}
console.log("rata-rata " + total / jumlahSiswa + ", terendah " + terendah + ", tertinggi " + tertinggi);
"""),
            ("README.TXT", Readme("Statistik Nilai", "Fungsi bertipe dan agregasi.", "F5."))),

        new Template("ts-dadu", "TypeScript", "Game", "Permainan Dadu (acak semu)",
            "Generator acak LCG, dua pemain, sepuluh ronde.", "MAIN.TS",
            ("MAIN.TS", """
// Permainan dadu dua pemain dengan generator acak semu (LCG)
let benih: number = 2026;

function acak(batas: number): number {
  benih = (benih * 1103515245 + 12345) % 2147483648;
  return benih % batas;
}

let skorA: number = 0;
let skorB: number = 0;
for (let ronde: number = 1; ronde <= 10; ronde = ronde + 1) {
  let a: number = acak(6) + 1;
  let b: number = acak(6) + 1;
  if (a > b) { skorA = skorA + 1; }
  else if (b > a) { skorB = skorB + 1; }
  console.log("ronde " + ronde + ": A=" + a + " B=" + b);
}
if (skorA > skorB) { console.log("Pemain A menang " + skorA + "-" + skorB); }
else if (skorB > skorA) { console.log("Pemain B menang " + skorB + "-" + skorA); }
else { console.log("Seri " + skorA + "-" + skorB); }
"""),
            ("README.TXT", Readme("Permainan Dadu", "Ubah 'benih' untuk permainan yang berbeda.", "F5."))),

        new Template("ts-kasir", "TypeScript", "Bisnis", "Kasir Sederhana",
            "Struk belanja: harga x jumlah, diskon, pajak.", "MAIN.TS",
            ("MAIN.TS", """
// Kasir: tiga barang, diskon 10% di atas Rp 100.000, PPN 11%
function baris(nama: string, harga: number, jumlah: number): number {
  let sub: number = harga * jumlah;
  console.log(nama + " x" + jumlah + " @ Rp " + harga + " = Rp " + sub);
  return sub;
}

let total: number = 0;
total = total + baris("Kopi Bogor", 35000, 2);
total = total + baris("Talas", 12000, 3);
total = total + baris("Asinan", 18000, 1);
let diskon: number = 0;
if (total > 100000) { diskon = total / 10; }
let pajak: number = (total - diskon) * 11 / 100;
console.log("subtotal Rp " + total + ", diskon Rp " + diskon + ", PPN Rp " + pajak);
console.log("BAYAR Rp " + (total - diskon + pajak));
"""),
            ("README.TXT", Readme("Kasir", "Perhitungan uang dalam rupiah bulat.", "F5."))),

        // ---------------- Python ----------------
        new Template("py-halo", "Python", "Dasar", "Halo Python",
            "print, variabel, dan str() untuk menggabung angka.", "MAIN.PY",
            ("MAIN.PY", """
# Halo Python - program pertama
nama = "Buitenzorg"
tahun = 2026
print("Halo dari " + nama + "!")
print("Tahun " + str(tahun) + ", panjang nama: " + str(len(nama)) + " huruf")
"""),
            ("README.TXT", Readme("Halo Python", "Python subset: indentasi, def, if/elif/else, while, for-range.", "F5, atau 'script py MAIN.PY'."))),

        new Template("py-perkalian", "Python", "Pendidikan", "Tabel Perkalian",
            "Perulangan bersarang dengan range().", "MAIN.PY",
            ("MAIN.PY", """
# Tabel perkalian 1-9
for a in range(1, 10):
    baris = ""
    for b in range(1, 10):
        baris = baris + str(a * b) + " "
    print(baris)
"""),
            ("README.TXT", Readme("Tabel Perkalian", "for di dalam for.", "F5."))),

        new Template("py-tebak", "Python", "Algoritma", "Tebak Angka (pencarian biner)",
            "Komputer menebak angka rahasia dengan membagi dua rentang.", "MAIN.PY",
            ("MAIN.PY", """
# Pencarian biner: menebak angka rahasia 1..1000
rahasia = 737
bawah = 1
atas = 1000
langkah = 0
while bawah <= atas:
    tebak = (bawah + atas) / 2
    langkah = langkah + 1
    if tebak == rahasia:
        print("ketemu " + str(tebak) + " dalam " + str(langkah) + " langkah")
        bawah = atas + 1
    elif tebak < rahasia:
        print(str(tebak) + " terlalu kecil")
        bawah = tebak + 1
    else:
        print(str(tebak) + " terlalu besar")
        atas = tebak - 1
"""),
            ("README.TXT", Readme("Tebak Angka", "Pencarian biner selesai paling lama 10 langkah untuk 1..1000.", "F5."))),

        new Template("py-kalender", "Python", "Utilitas", "Kalender: Kabisat & Hari",
            "Tahun kabisat, jumlah hari per bulan, dan nama hari (Zeller).", "MAIN.PY",
            ("MAIN.PY", """
# Kalender: tahun kabisat, hari per bulan, nama hari (rumus Zeller)
def kabisat(t):
    return (t % 4 == 0 and t % 100 != 0) or t % 400 == 0

def hari_dalam_bulan(b, t):
    if b == 2:
        if kabisat(t):
            return 29
        return 28
    if b == 4 or b == 6 or b == 9 or b == 11:
        return 30
    return 31

def nama_hari(h):
    if h == 0:
        return "Sabtu"
    if h == 1:
        return "Minggu"
    if h == 2:
        return "Senin"
    if h == 3:
        return "Selasa"
    if h == 4:
        return "Rabu"
    if h == 5:
        return "Kamis"
    return "Jumat"

def zeller(d, m, y):
    if m < 3:
        m = m + 12
        y = y - 1
    k = y % 100
    j = y / 100
    return (d + 13 * (m + 1) / 5 + k + k / 4 + j / 4 + 5 * j) % 7

tahun = 2026
print(str(tahun) + " kabisat? " + str(kabisat(tahun)))
for b in range(1, 13):
    print("bulan " + str(b) + ": " + str(hari_dalam_bulan(b, tahun)) + " hari, tanggal 1 hari " + nama_hari(zeller(1, b, tahun)))
"""),
            ("README.TXT", Readme("Kalender", "Fungsi dengan banyak cabang.", "F5."))),

        new Template("py-collatz", "Python", "Matematika", "Barisan Collatz",
            "Berapa langkah sampai 1? Mencari bilangan dengan barisan terpanjang.", "MAIN.PY",
            ("MAIN.PY", """
# Barisan Collatz: n genap -> n/2, ganjil -> 3n+1, sampai 1
def langkah(n):
    k = 0
    while n != 1:
        if n % 2 == 0:
            n = n / 2
        else:
            n = 3 * n + 1
        k = k + 1
    return k

terbaik = 1
terpanjang = 0
for n in range(1, 60):
    k = langkah(n)
    if k > terpanjang:
        terpanjang = k
        terbaik = n
print("di bawah 60, barisan terpanjang milik " + str(terbaik) + " (" + str(terpanjang) + " langkah)")
print("27 butuh " + str(langkah(27)) + " langkah")
"""),
            ("README.TXT", Readme("Collatz", "while dengan kondisi yang belum terbukti selalu berhenti (tapi berhenti untuk semua yang pernah dicoba).", "F5."))),

        // ---------------- Shell ----------------
        new Template("sh-argumen", "Skrip shell", "Dasar", "Argumen, Perulangan & Kondisi",
            "$1..$9, $#, $@, for, if dan test.", "MAIN.SH",
            ("MAIN.SH", """
# Skrip shell: argumen, perulangan dan kondisi.
# Jalankan dari Terminal dengan argumen: sh MAIN.SH apel jeruk mangga
if [ $# -eq 0 ]; then
    echo "tanpa argumen - memakai contoh"
    for buah in apel jeruk mangga; do
        echo "buah: $buah"
    done
else
    echo "ada $# argumen"
    for a in $@; do
        echo "argumen: $a"
    done
fi
"""),
            ("README.TXT", Readme("Argumen & Kondisi", "Skrip .SH memakai shell bawaan (lihat docs/shell.md).", "F5, atau 'sh MAIN.SH a b c'."))),

        new Template("sh-backup", "Skrip shell", "Otomasi", "Cadangan Berkas Teks",
            "Menyalin semua berkas .TXT dari /disk ke folder cadangan (/ram, atau /disk bila tidak ada /ram).", "MAIN.SH",
            ("MAIN.SH", """
# Cadangkan berkas teks /disk ke folder CADANGAN
# (di /ram bila ada -- PC -- selain itu di /disk: ARM64, RISC-V, Raspberry Pi)
dasar=/ram
test -d /ram || dasar=/disk
tujuan=$dasar/CADANGAN
test -d $tujuan || mkdir $tujuan
n=0
for f in /disk/*.TXT; do
    cp $f $tujuan && echo "disalin: $f"
done
echo "isi $tujuan:"
ls $tujuan | wc -l
"""),
            ("README.TXT", Readme("Cadangan Berkas", "Glob, test -d, mkdir, cp dan pipe.", "F5."))),

        new Template("sh-laporan", "Skrip shell", "Administrasi", "Laporan Sistem",
            "Versi OS, ruang disk, jumlah proses dan berkas.", "MAIN.SH",
            ("MAIN.SH", """
# Laporan sistem singkat
echo "== versi =="
ver
echo "== ruang disk =="
df
echo "== jumlah baris daftar proses =="
ps | wc -l
echo "== program di /disk =="
ls /disk | grep -c ELF
ls /disk | grep -c DLL
"""),
            ("README.TXT", Readme("Laporan Sistem", "Menggabungkan perintah bawaan dengan pipe.", "F5."))),

        new Template("sh-bersih", "Skrip shell", "Otomasi", "Pembersih Berkas Sementara",
            "Membuat lalu menghapus berkas uji (di /ram, atau /disk) dengan aman.", "MAIN.SH",
            ("MAIN.SH", """
# Buat beberapa berkas sementara lalu bersihkan lagi
# (di /ram bila ada, selain itu di /disk)
d=/ram
test -d /ram || d=/disk
for i in 1 2 3; do
    echo "sementara $i" > $d/TMP$i.TXT
done
echo "sebelum:"
ls $d | grep TMP
for f in $d/TMP*.TXT; do
    rm $f
done
echo "sesudah:"
ls $d | grep -c TMP
"""),
            ("README.TXT", Readme("Pembersih", "Redirect, glob dan rm di dalam for.", "F5."))),

        new Template("sh-profil", "Skrip shell", "Konfigurasi", "Profil Terminal (alias)",
            "Contoh /disk/PROFILE.SH: alias dan variabel yang aktif tiap Terminal dibuka.", "PROFILE.SH",
            ("PROFILE.SH", """
# Contoh profil Terminal. Salin ke /disk/PROFILE.SH agar aktif:
#   cp PROFILE.SH /disk/PROFILE.SH
alias ll='ls /disk'
alias h='history'
alias proyek='ls /disk/PROYEK'
EDITOR=mstudio
echo "profil dimuat: alias ll, h, proyek"
"""),
            ("README.TXT", Readme("Profil Terminal", "Alias dan variabel untuk setiap sesi Terminal.", "F5 untuk mencoba; salin ke /disk/PROFILE.SH untuk memakainya."))),

        // ---------------- C# (built on a PC) ----------------
        new Template("cs-konsol", "C#", "Native (bflat)", "Aplikasi Konsol Native",
            "C# yang dikompilasi bflat --stdlib:zero menjadi ELF ring-3 yang cepat.", "PROGRAM.CS",
            ("PROGRAM.CS", """
// Aplikasi konsol native C# (bflat --stdlib:zero + shim bzstart.rs).
// Yang berfungsi: new, array, generic, static, lambda, cast, interface, object[].
// Belum ada: exception dan GC yang mereklamasi memori.
using System;

static class Program
{
    static int Kuadrat(int x) => x * x;

    static int Main()
    {
        Console.WriteLine("Halo dari C# native di Buitenzorg!");
        int total = 0;
        for (int i = 1; i <= 10; i++) total += Kuadrat(i);
        Console.WriteLine(total == 385 ? "jumlah kuadrat 1..10 = 385" : "salah hitung");
        return 0;
    }
}
"""),
            ("BUILD.TXT", """
Build di PC (Windows, dari folder repositori Buitenzorg):
  1. Salin PROGRAM.CS ke userland/hello-csharp/myapp.cs
  2. powershell scripts/build-hello-csharp.ps1   (atau build template SDK desktop-csharp)
  3. Kemas sebagai paket: dotnet run --project sdk/bz -- pack <proyek> -o MYAPP.BZP
  4. Salin MYAPP.BZP ke /disk (layanan deploy), lalu di Terminal: bz install /disk/MYAPP.BZP
Panduan: docs/first-app.md (aturan zerolib), docs/packages.md.
""")),

        new Template("cs-desktop", "C#", "Native (bflat)", "Aplikasi Desktop Native (Buitenzorg.UI)",
            "Jendela dengan tombol dan teks memakai toolkit native bzui.cs.", "PROGRAM.CS",
            ("PROGRAM.CS", """
// Aplikasi desktop native: jendela Buitenzorg.UI (bzui.cs + bzgfx.cs).
// Diringkas dari sdk/templates/desktop-csharp/app.cs (proyek lengkap + skrip build).
using System;
using System.Runtime.InteropServices;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

class App
{
    [DllImport("*")] static extern uint bz_key_read();
    [DllImport("*")] static extern ulong bz_is_interactive();

    static void Main()
    {
        Font font = Font.Default();
        UIHost host = new UIHost("Penghitung", 320, 200);
        StackPanel root = new StackPanel();
        root.Padding = 14; root.Spacing = 12;
        root.Background = new Color(0xFF141C16);
        TextBlock title = new TextBlock("SPASI = +1, ESC = keluar", font);
        title.Foreground = new Color(0xFF7FD48C);
        Button plus = new Button("+1", font);
        plus.Width = 90; plus.Height = 30;
        root.Add(title);
        root.Add(plus);
        host.Root = root;
        host.Layout();
        host.Render(new Color(0xFF141820));
        host.Present();
        if (bz_is_interactive() == 0) return;   // boot self-test: draw once
        while (true)
        {
            uint k = bz_key_read();
            if (k == 27) return;                  // ESC
            if (k == ' ') { host.Render(new Color(0xFF141820)); host.Present(); }
        }
    }
}
"""),
            ("BUILD.TXT", """
Template lengkap ada di sdk/templates/desktop-csharp (bz new desktop-csharp NamaApp).
Build: lihat README template itu; hasilnya ELF yang bisa dikemas dengan 'bz pack'.
API yang tepat (UIHost, Button.Clicks, Graphics): MagicAppGen atau docs/first-app.md.
""")),

        new Template("cs-managed", "C#", "Managed (RustCLR)", "Aplikasi Managed Buitenzorg.UI",
            "dotnet build biasa: LINQ, exception, GC penuh; jalan di x86, ARM64 dan RISC-V.", "PROGRAM.CS",
            ("PROGRAM.CS", """
// Aplikasi managed: assembly .NET biasa yang dijalankan RustCLR di Buitenzorg.
// Referensikan Buitenzorg.UI (userland/clr-apps/Buitenzorg.UI).
using Buitenzorg.UI;

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        var w = new Window("Halo Managed", 360, 220);
        int klik = 0;
        var teks = new TextBlock("Belum diklik");
        var tombol = new Button(new TextBlock("Klik saya"), _ => { klik++; teks.Text = "Diklik " + klik + " kali"; });
        var isi = new StackPanel { Spacing = 12, Margin = new Thickness(24) };
        isi.Children.Add(teks);
        isi.Children.Add(tombol);
        w.Content = isi;
        app.Run(w);
        return 0;
    }
}
"""),
            ("BUILD.TXT", """
Build lewat build server (F5 di Studio = bz build <folder ini> --run):
  1. Di PC, dari folder repositori Buitenzorg: bz build-server
  2. Di Terminal perangkat: bz buildsrv http://<IP PC>:8090   (QEMU: http://10.0.2.2:8090)
  3. F5: berkas .CS dikirim ke PC, dotnet build di sana, <FOLDER>.DLL kembali dan dijalankan
     (galat kompilasi muncul di panel Masalah dengan nomor barisnya)
Manual di PC: proyek net10.0 dengan ProjectReference ke Buitenzorg.UI, dotnet build,
salin DLL ke /disk, lalu 'run NAMA'. Panduan: docs/managed-apps.md.
""")),

        // ---------------- packages / data ----------------
        new Template("paket", "Paket", "Distribusi", "Proyek Paket .bzpkg",
            "Struktur proyek yang dikemas 'bz pack': manifest + folder package.", "APP.MAN",
            ("APP.MAN", """
{
  "id": "local.paketku",
  "name": "Paketku",
  "type": "console",
  "language": "csharp",
  "version": "1.0.0",
  "permissions": [],
  "theme": "system"
}
"""),
            ("MAIN.SH", """
# Isi paket: skrip yang dijalankan saat nama paket diketik
echo "Halo dari paketku v1.0.0, argumen: $@"
"""),
            ("README.TXT", """
Proyek paket
============

Di PC: buat folder dengan app.manifest (isi APP.MAN ini) dan folder package/
berisi MAIN.SH, lalu:
  dotnet run --project sdk/bz -- pack <folder> -o PAKETKU.BZP
Di perangkat:
  bz install /disk/PAKETKU.BZP
  paketku satu dua
Lihat docs/packages.md.
""")),

        new Template("json-data", "JSON", "Data", "Data JSON",
            "Berkas konfigurasi/data dengan pewarnaan sintaks JSON.", "DATA.JSN",
            ("DATA.JSN", """
{
  "nama": "Kebun Raya Bogor",
  "didirikan": 1817,
  "luas_hektar": 87,
  "koleksi": ["anggrek", "palem", "bunga bangkai"],
  "buka": true,
  "catatan": null
}
"""),
            ("README.TXT", Readme("Data JSON", "Contoh berkas data. Studio mewarnai kunci, angka dan literal.", "Tidak dijalankan; buka dan edit."))),
    };

    static readonly string[] Order = { "C# script", "C# script UI", "JavaScript", "JavaScript UI", "TypeScript", "TypeScript UI", "Python", "Python UI", "Skrip shell", "C#", "Paket", "JSON" };

    /// <summary>Every template (console and UI), grouped by language.</summary>
    public static readonly Template[] Catalog = All.Concat(UiTemplates.All).Concat(UiTemplates2.All).Concat(CsxTemplates.All)
        .OrderBy(t => Array.IndexOf(Order, t.Language) < 0 ? 99 : Array.IndexOf(Order, t.Language))
        .ToArray();

    public static Template? Find(string id)
    {
        foreach (var t in Catalog) if (t.Id == id) return t;
        return null;
    }
}
