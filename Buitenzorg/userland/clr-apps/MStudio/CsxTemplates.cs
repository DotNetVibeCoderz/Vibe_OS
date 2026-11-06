// C# script (.csx) templates: run by the bytecode engine csx.rs -- in the
// kernel for console scripts (`csx FILE`), in CLRHOST.ELF for UI scripts
// (`Ui.Window(...)`, `run FILE.CSX`). See docs/csharp-script.md.

namespace MStudio;

public static class CsxTemplates
{
    static string Readme(string title, string what, bool ui) =>
        title + "\n" + new string('=', title.Length) + "\n\n" + what + "\n\n" +
        (ui ? "C# script ber-UI: F5 membukanya di jendela sendiri (atau 'run MAIN.CSX' di Terminal).\n"
            : "Menjalankan: F5 di Studio, atau 'csx MAIN.CSX' di Terminal.\n") +
        "C# script dikompilasi ke bytecode dan dijalankan VM (kernel/bzkernel/src/csx.rs).\n" +
        "Panduan: docs/csharp-script.md.\n";

    public static readonly Template[] All =
    {
        new Template("csx-halo", "C# script", "Dasar", "Halo C# Script",
            "Top-level statements, var, interpolasi string, method.", "MAIN.CSX",
            ("MAIN.CSX", """
// Halo C# Script: tanpa class, tanpa Main -- langsung jalan
using System;

string Sapa(string nama) => $"Halo, {nama}!";

var nama = "Buitenzorg";
int tahun = 2026;
Console.WriteLine(Sapa(nama));
Console.WriteLine($"Tahun {tahun}, {nama.Length} huruf, huruf besar: {nama.ToUpper()}");
for (int i = 1; i <= 3; i++)
    Console.WriteLine($"  baris {i} dari 3");
"""),
            ("README.TXT", Readme("Halo C# Script", "Program C# pertama: tidak perlu class atau Main.", false))),

        new Template("csx-koleksi", "C# script", "Koleksi", "List & Dictionary",
            "List<T>, Dictionary<K,V>, foreach, Sort, Sum, Max, Average.", "MAIN.CSX",
            ("MAIN.CSX", """
// Koleksi: nilai siswa per kelas
using System.Collections.Generic;

var nilai = new Dictionary<string, List<int>>();
nilai["Anggrek"] = new List<int> { 88, 92, 75 };
nilai["Melati"] = new List<int> { 70, 81 };
nilai["Kenanga"] = new List<int> { 95, 67, 84, 90 };

foreach (var kelas in nilai)
{
    var daftar = kelas.Value;
    daftar.Sort();
    Console.WriteLine($"{kelas.Key,-8} n={daftar.Count} rata={daftar.Average():F1} tertinggi={daftar.Max()} urut=[{string.Join(", ", daftar)}]");
}

var semua = new List<int>();
foreach (var d in nilai.Values) semua.AddRange(d);
Console.WriteLine($"total {semua.Count} nilai, jumlah {semua.Sum()}");
"""),
            ("README.TXT", Readme("List & Dictionary", "Koleksi generik dan metode agregat.", false))),

        new Template("csx-teks", "C# script", "Teks", "Analisis Teks",
            "Split, Dictionary penghitung kata, StringBuilder, char.", "MAIN.CSX",
            ("MAIN.CSX", """
// Analisis teks: frekuensi kata dan huruf
using System.Text;

string teks = "kebun raya bogor adalah kebun botani di kota bogor dan kebun raya tertua";
var hitung = new Dictionary<string, int>();
foreach (var kata in teks.Split(" "))
{
    if (hitung.ContainsKey(kata)) hitung[kata]++;
    else hitung[kata] = 1;
}
foreach (var kv in hitung)
    if (kv.Value > 1) Console.WriteLine($"'{kv.Key}' muncul {kv.Value} kali");

int vokal = 0;
foreach (char c in teks) if ("aiueo".Contains(c.ToString())) vokal++;
Console.WriteLine($"{teks.Length} karakter, {vokal} vokal");

var sb = new StringBuilder();
foreach (var kata in teks.Split(" ")) sb.Append(char.ToUpper(kata[0]));
Console.WriteLine($"Akronim: {sb}");
"""),
            ("README.TXT", Readme("Analisis Teks", "Pemrosesan string khas C#.", false))),

        new Template("csx-sort", "C# script", "Algoritma", "Algoritma Pengurutan",
            "Bubble sort vs quicksort pada 300 angka acak.", "MAIN.CSX",
            ("MAIN.CSX", """
// Pengurutan: bubble sort vs quicksort (engine bytecode -- cepat)
void Bubble(int[] a)
{
    for (int i = 0; i < a.Length; i++)
        for (int j = 0; j < a.Length - 1 - i; j++)
            if (a[j] > a[j + 1]) { var t = a[j]; a[j] = a[j + 1]; a[j + 1] = t; }
}

void Quick(int[] a, int lo, int hi)
{
    if (lo >= hi) return;
    int p = a[(lo + hi) / 2], i = lo, j = hi;
    while (i <= j)
    {
        while (a[i] < p) i++;
        while (a[j] > p) j--;
        if (i <= j) { var t = a[i]; a[i] = a[j]; a[j] = t; i++; j--; }
    }
    Quick(a, lo, j);
    Quick(a, i, hi);
}

bool Urut(int[] a) { for (int i = 1; i < a.Length; i++) if (a[i - 1] > a[i]) return false; return true; }

var rnd = new Random(7);
int n = 300;
var x = new int[n];
var y = new int[n];
for (int i = 0; i < n; i++) { x[i] = rnd.Next(100000); y[i] = x[i]; }
Bubble(x);
Quick(y, 0, n - 1);
Console.WriteLine($"bubble urut: {Urut(x)}, quick urut: {Urut(y)}, sama: {x[n / 2] == y[n / 2]}");
Console.WriteLine($"terkecil {y[0]}, terbesar {y[n - 1]}");
"""),
            ("README.TXT", Readme("Algoritma Pengurutan", "Array, rekursi, perbandingan algoritma O(n^2) dan O(n log n).", false))),

        new Template("csx-exception", "C# script", "Penanganan error", "Exception & Validasi",
            "try/catch/finally, throw, int.Parse yang gagal.", "MAIN.CSX",
            ("MAIN.CSX", """
// Exception: validasi input dengan try/catch
int Umur(string teks)
{
    int n = int.Parse(teks);
    if (n < 0 || n > 150) throw new ArgumentException($"umur {n} tidak masuk akal");
    return n;
}

foreach (var input in new[] { "25", "abc", "-3", "70" })
{
    try
    {
        Console.WriteLine($"'{input}' -> umur {Umur(input)}");
    }
    catch (Exception e)
    {
        Console.WriteLine($"'{input}' ditolak: {e.Message}");
    }
    finally
    {
        Console.WriteLine("  (diperiksa)");
    }
}
"""),
            ("README.TXT", Readme("Exception", "Error ditangkap lalu program berlanjut.", false))),

        new Template("csx-matematika", "C# script", "Matematika", "Statistik & Matematika",
            "Math.Sqrt/Pow/Sin, double, format angka N2/F3, simulasi Monte Carlo pi.", "MAIN.CSX",
            ("MAIN.CSX", """
// Matematika: Monte Carlo pi, akar persamaan kuadrat, deret
var rnd = new Random(2026);
int masuk = 0, total = 20000;
for (int i = 0; i < total; i++)
{
    double x = rnd.NextDouble(), y = rnd.NextDouble();
    if (x * x + y * y <= 1.0) masuk++;
}
Console.WriteLine($"pi ~ {4.0 * masuk / total:F4} (Math.PI = {Math.PI:F4})");

// akar persamaan kuadrat ax^2 + bx + c = 0
double a = 1, b = -3, c = 2;
double disk = b * b - 4 * a * c;
if (disk >= 0)
{
    double akar = Math.Sqrt(disk);
    Console.WriteLine($"akar x^2 - 3x + 2: {(-b + akar) / (2 * a):F3} dan {(-b - akar) / (2 * a):F3}");
}

// deret 1/n^2 menuju pi^2/6
double s = 0;
for (int n = 1; n <= 2000; n++) s += 1.0 / ((double)n * n);
Console.WriteLine($"jumlah 1/n^2 = {s:F5}, pi^2/6 = {Math.PI * Math.PI / 6:F5}");
Console.WriteLine($"{1234567.891:N2} | {Math.Pow(2, 0.5):F6} | {Math.Sin(Math.PI / 6):F3}");
"""),
            ("README.TXT", Readme("Matematika", "Bilangan pecahan, Math, dan format angka.", false))),

        // ---------------- UI ----------------
        new Template("csx-ui-penghitung", "C# script UI", "Aplikasi UI", "Penghitung (Ui.Button)",
            "Jendela dengan tombol, ditulis dalam C# script.", "MAIN.CSX",
            ("MAIN.CSX", """
// Penghitung C# script ber-UI (immediate mode: gambar ulang tiap frame)
Ui.Window("Penghitung C#", 320, 220);
int n = 0;
while (Ui.Running())
{
    Ui.Gradient(0, 0, 320, 220, 0x1B3A2A, 0x0E2018);
    Ui.Text(24, 24, "Penghitung C#", 0x9CC9D9, 1);
    Ui.Text(24, 64, $"{n}", 0xFFFFFF, 2);
    if (Ui.Button(24, 140, 124, 40, "Tambah")) n++;
    if (Ui.Button(164, 140, 124, 40, "Reset")) n = 0;
    if (Ui.Key() == ' ') n++;
    Ui.Present();
}
"""),
            ("README.TXT", Readme("Penghitung C#", "Ui.Window, Ui.Text, Ui.Button, Ui.Present.", true))),

        new Template("csx-ui-snake", "C# script UI", "Game", "Game Ular (Snake)",
            "Ular dengan List<int>, panah untuk bergerak, makan buah, skor.", "MAIN.CSX",
            ("MAIN.CSX", """
// Game Ular: panah untuk bergerak, Spasi untuk mulai lagi
const int Sel = 16, Lebar = 24, Tinggi = 18;
Ui.Window("Ular", Lebar * Sel, Tinggi * Sel + 30);
var rnd = new Random();
var xs = new List<int>();
var ys = new List<int>();
int dx = 1, dy = 0, buahX = 0, buahY = 0, skor = 0, tick = 0;
bool mati = false;

void Mulai()
{
    xs.Clear(); ys.Clear();
    for (int i = 0; i < 4; i++) { xs.Add(8 - i); ys.Add(9); }
    dx = 1; dy = 0; skor = 0; mati = false;
    Buah();
}

void Buah() { buahX = rnd.Next(Lebar); buahY = rnd.Next(Tinggi); }

Mulai();
while (Ui.Running())
{
    int k = Ui.Key();
    if (k == 63232 && dy == 0) { dx = 0; dy = -1; }
    if (k == 63233 && dy == 0) { dx = 0; dy = 1; }
    if (k == 63234 && dx == 0) { dx = -1; dy = 0; }
    if (k == 63235 && dx == 0) { dx = 1; dy = 0; }
    if (k == ' ' && mati) Mulai();
    tick++;
    if (!mati && tick % 4 == 0)
    {
        int nx = xs[0] + dx, ny = ys[0] + dy;
        if (nx < 0 || ny < 0 || nx >= Lebar || ny >= Tinggi) mati = true;
        for (int i = 0; i < xs.Count; i++) if (xs[i] == nx && ys[i] == ny) mati = true;
        if (!mati)
        {
            xs.Insert(0, nx); ys.Insert(0, ny);
            if (nx == buahX && ny == buahY) { skor++; Buah(); }
            else { xs.RemoveAt(xs.Count - 1); ys.RemoveAt(ys.Count - 1); }
        }
    }
    Ui.Clear(0x0E2018);
    Ui.Rect(buahX * Sel + 2, buahY * Sel + 2, Sel - 4, Sel - 4, 0xE5484D, 6);
    for (int i = 0; i < xs.Count; i++)
        Ui.Rect(xs[i] * Sel + 1, ys[i] * Sel + 1, Sel - 2, Sel - 2, i == 0 ? 0xB5E8C8 : 0x7CC4A4, 4);
    Ui.Rect(0, Tinggi * Sel, Lebar * Sel, 30, 0x1B3A2A);
    Ui.Text(10, Tinggi * Sel + 6, mati ? $"Tamat! Skor {skor} - Spasi untuk main lagi" : $"Skor {skor}", 0xFFFFFF, 1);
    Ui.Present();
}
"""),
            ("README.TXT", Readme("Game Ular", "List<int>, method, const, operator ?:, input panah.", true))),

        new Template("csx-ui-grafik", "C# script UI", "Grafis", "Grafik Fungsi",
            "Plot sin, cos dan parabola dengan Math dan double; klik untuk ganti fungsi.", "MAIN.CSX",
            ("MAIN.CSX", """
// Grafik fungsi: klik di jendela untuk mengganti fungsi
Ui.Window("Grafik Fungsi", 480, 320);
int pilihan = 0;
string[] nama = { "sin(x)", "cos(2x)", "x^2 / 10 - 1" };

double F(int k, double x)
{
    switch (k)
    {
        case 0: return Math.Sin(x);
        case 1: return Math.Cos(2 * x);
        default: return x * x / 10 - 1;
    }
}

while (Ui.Running())
{
    if (Ui.Clicked()) pilihan = (pilihan + 1) % nama.Length;
    Ui.Clear(0x0E2018);
    Ui.Line(0, 160, 480, 160, 0x3F6B52);
    Ui.Line(240, 0, 240, 320, 0x3F6B52);
    int px = 0, py = 0;
    for (int sx = 0; sx < 480; sx += 4)
    {
        double x = (sx - 240) / 40.0;
        int sy = 160 - (int)(F(pilihan, x) * 60);
        if (sx > 0) Ui.Line(px, py, sx, sy, 0xF3C969);
        px = sx;
        py = sy;
    }
    Ui.Text(12, 10, $"y = {nama[pilihan]}  (klik untuk ganti)", 0xFFFFFF, 1);
    Ui.Present();
}
"""),
            ("README.TXT", Readme("Grafik Fungsi", "double, Math.Sin/Cos, array string, garis antar titik.", true))),
        new Template("csx-ui-kontak", "C# script UI", "Form", "Buku Kontak",
            "Form nama + telepon, daftar dengan pencarian dan hapus (List<string>).", "MAIN.CSX",
            ("MAIN.CSX", """
// Buku kontak: tambah, cari, dan hapus (List<string>)
Ui.Window("Buku Kontak", 460, 420);
var nama = new List<string> { "Ayu Lestari", "Budi Santoso" };
var telp = new List<string> { "0812-1111-2222", "0813-3333-4444" };
string pesan = "";

while (Ui.Running())
{
    Ui.Clear(0x14201A);
    Ui.Text(20, 14, "Buku Kontak", 0xFFFFFF, 2);
    string n = Ui.Input(20, 56, 200, "nama", "nama");
    string t = Ui.Input(228, 56, 130, "telp", "telepon");
    if (Ui.Button(366, 54, 74, 34, "Simpan"))
    {
        if (n.Trim().Length < 2 || t.Trim().Length < 4) pesan = "Isi nama dan telepon";
        else
        {
            nama.Add(n.Trim());
            telp.Add(t.Trim());
            Ui.SetInput("nama", "");
            Ui.SetInput("telp", "");
            pesan = $"{n.Trim()} disimpan";
        }
    }
    string cari = Ui.Input(20, 104, 420, "cari", "cari nama...").ToLower();
    int y = 152, tampil = 0;
    for (int i = 0; i < nama.Count; i++)
    {
        if (cari.Length > 0 && !nama[i].ToLower().Contains(cari)) continue;
        Ui.Rect(20, y, 420, 36, tampil % 2 == 0 ? 0x1B3A2A : 0x173224, 6);
        Ui.Text(32, y + 10, nama[i], 0xFFFFFF, 1);
        Ui.Text(240, y + 10, telp[i], 0x9CC9D9);
        if (Ui.Button(390, y + 4, 44, 28, "x"))
        {
            pesan = $"{nama[i]} dihapus";
            nama.RemoveAt(i);
            telp.RemoveAt(i);
            break;
        }
        y += 40;
        tampil++;
        if (y > 360) break;
    }
    Ui.Text(20, 392, $"{nama.Count} kontak   {pesan}", 0xF3C969);
    Ui.Present();
}
"""),
            ("README.TXT", Readme("Buku Kontak", "Ui.Input untuk form, List<string> untuk data, pencarian dengan Contains.", true))),

        new Template("csx-ui-tugas", "C# script UI", "Utilitas", "Daftar Tugas",
            "Tambah tugas (Enter), centang selesai, hapus yang selesai.", "MAIN.CSX",
            ("MAIN.CSX", """
// Daftar tugas: tambah, centang selesai, hapus yang selesai
Ui.Window("Daftar Tugas", 420, 440);
var tugas = new List<string> { "Siram anggrek", "Belajar C# script" };
var selesai = new List<bool> { true, false };
Ui.Focus("baru");

while (Ui.Running())
{
    Ui.Clear(0x14201A);
    Ui.Text(20, 14, "Daftar Tugas", 0xFFFFFF, 2);
    string baru = Ui.Input(20, 56, 290, "baru", "tugas baru, Enter untuk menambah");
    bool tambah = Ui.Button(318, 54, 82, 34, "Tambah");
    if (Ui.Key() == 10) tambah = true;
    if (tambah && baru.Trim().Length > 0)
    {
        tugas.Add(baru.Trim());
        selesai.Add(false);
        Ui.SetInput("baru", "");
        Ui.Focus("baru");
    }
    int y = 104;
    for (int i = 0; i < tugas.Count && y < 380; i++)
    {
        Ui.Rect(20, y, 380, 34, 0x1B3A2A, 6);
        if (Ui.Button(26, y + 4, 26, 26, selesai[i] ? "v" : "")) selesai[i] = !selesai[i];
        Ui.Text(62, y + 9, tugas[i], selesai[i] ? 0x6F8F7A : 0xFFFFFF);
        if (selesai[i]) Ui.Line(62, y + 17, 62 + Ui.TextWidth(tugas[i]), y + 17, 0x6F8F7A);
        y += 40;
    }
    int jumlahSelesai = 0;
    foreach (var s in selesai) if (s) jumlahSelesai++;
    Ui.Text(20, 396, $"{jumlahSelesai} dari {tugas.Count} selesai", 0x9CC9D9, 1);
    if (Ui.Button(270, 388, 130, 34, "Hapus selesai"))
    {
        for (int i = tugas.Count - 1; i >= 0; i--)
            if (selesai[i]) { tugas.RemoveAt(i); selesai.RemoveAt(i); }
    }
    Ui.Present();
}
"""),
            ("README.TXT", Readme("Daftar Tugas", "Dua List sejajar (teks dan status), foreach, for mundur untuk menghapus.", true))),
    };
}
