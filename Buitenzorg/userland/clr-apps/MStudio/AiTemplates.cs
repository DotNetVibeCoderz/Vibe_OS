// Templates that use the OS's built-in AI from scripts: the local language
// model through aiload / aistart + ainext (streamed, one piece per frame) /
// aigenerate (whole), aibackend, aiinfo, aispeed -- in JavaScript, Python and
// C# script (Ai.Load / Ai.Start / Ai.Next ...). The model is /disk/STORIES.GGU
// (stories260K: it writes short children's stories in English). Guide:
// docs/ai-compute.md ("For app authors").

namespace MStudio;

public static class AiTemplates
{
    static string Readme(string title, string what) =>
        title + "\n" + new string('=', title.Length) + "\n\n" + what + "\n\n" +
        "AI lokal: model bahasa /disk/STORIES.GGU berjalan di mesin ini (GPU bila ada), tanpa internet.\n" +
        "Fungsi: aiload([berkas]), aistart(awal, token[, suhu/1000]) + ainext() (\"\" = selesai),\n" +
        "aigenerate(awal, token[, suhu/1000]), aibackend(), aiinfo(n), aispeed().\n" +
        "C# script: Ai.Load(), Ai.Start(...), Ai.Next(), Ai.Generate(...), Ai.Backend(), Ai.Speed().\n" +
        "F5 membukanya di jendela sendiri. Panduan: docs/ai-compute.md.\n";

    public static readonly Template[] All =
    {
        new Template("js-ui-ai-cerita", "JavaScript UI", "AI", "Penulis Cerita AI",
            "Model bahasa lokal melanjutkan kalimat Anda, kata demi kata.", "MAIN.JS",
            ("MAIN.JS", """
// Penulis cerita: model bahasa lokal OS melanjutkan kalimat, kata demi kata.
// aistart() memulai, ainext() memberi potongan berikutnya ("" = selesai):
// satu potongan per frame, jadi teksnya tampil saat ditulis.
window("Penulis Cerita AI", 520, 400);
aiload();
let cerita = "";
let menulis = false;
let info = "Model: " + aibackend() + ", " + aiinfo(6) + " parameter";

// teks dibungkus per kata, w karakter per baris
function bungkus(s, x, y, w, warna) {
  let pos = 0;
  let n = len(s);
  while (pos < n) {
    let akhir = pos + w;
    if (akhir >= n) { text(x, y, substr(s, pos, n - pos), warna, 1); return y + 20; }
    let potong = akhir;
    while (potong > pos && substr(s, potong, 1) != " ") { potong = potong - 1; }
    if (potong == pos) { potong = akhir; }
    text(x, y, substr(s, pos, potong - pos), warna, 1);
    y = y + 20;
    pos = potong + 1;
  }
  return y;
}

while (running()) {
  clear(0x14201A);
  text(20, 14, "Penulis Cerita AI", 0xFFFFFF, 2);
  let awal = input(20, 56, 360, "awal", "Once upon a time");
  if ((button(392, 52, 108, 36, "Tulis") || key() == 10) && !menulis) {
    if (len(trim(awal)) == 0) { awal = "Once upon a time"; }
    cerita = awal;
    aistart(awal, 80);
    menulis = true;
    info = "menulis ...";
  }
  if (menulis) {
    let potongan = ainext();
    if (potongan == "") {
      menulis = false;
      info = aispeed() + " token/detik di " + aibackend();
    } else {
      cerita = cerita + potongan;
    }
  }
  bungkus(cerita, 20, 108, 58, 0xF0FFE0);
  text(20, 370, info, 0xF3C969, 1);
  present();
}
"""),
            ("README.TXT", Readme("Penulis Cerita AI", "Ketik awal cerita (bahasa Inggris), Enter atau Tulis; jawabannya muncul kata demi kata."))),

        new Template("py-ui-ai-lengkapi", "Python UI", "AI", "Pelengkap Kalimat AI",
            "Tombol Lengkapi: model lokal menyelesaikan kalimat sekaligus.", "MAIN.PY",
            ("MAIN.PY", """
# Pelengkap kalimat: model bahasa lokal menyelesaikan kalimat Anda sekaligus
window("Pelengkap Kalimat AI", 520, 360)
aiload()
hasil = ""
suhu = 0

def bungkus(s, x, y, w):
    pos = 0
    n = len(s)
    while pos < n:
        akhir = pos + w
        if akhir >= n:
            text(x, y, substr(s, pos, n - pos), 0xF0FFE0, 1)
            return y + 20
        potong = akhir
        while potong > pos and substr(s, potong, 1) != " ":
            potong = potong - 1
        if potong == pos:
            potong = akhir
        text(x, y, substr(s, pos, potong - pos), 0xF0FFE0, 1)
        y = y + 20
        pos = potong + 1
    return y

while running():
    clear(0x14201A)
    text(20, 14, "Pelengkap Kalimat AI", 0xFFFFFF, 2)
    awal = input(20, 56, 360, "awal", "The little dog")
    if button(392, 52, 108, 36, "Lengkapi"):
        if len(trim(awal)) == 0:
            awal = "The little dog"
        hasil = awal + aigenerate(awal, 40, suhu)
    label = "Variasi: mati"
    if suhu > 0:
        label = "Variasi: nyala"
    if button(20, 100, 160, 32, label):
        if suhu > 0:
            suhu = 0
        else:
            suhu = 800
    bungkus(hasil, 20, 150, 58)
    text(20, 330, "Model: " + aibackend() + ", " + str(aiinfo(6)) + " parameter, " + str(aispeed()) + " token/detik", 0x9CC9D9, 1)
    present()
"""),
            ("README.TXT", Readme("Pelengkap Kalimat AI", "aigenerate() menunggu sampai jawabannya selesai; 'Variasi' memakai suhu 0.8 (jawaban berbeda tiap kali)."))),

        new Template("csx-ui-ai-cerita", "C# script UI", "AI", "Cerita AI (C# script)",
            "Ai.Start + Ai.Next dari C# script: cerita ditulis kata demi kata.", "MAIN.CSX",
            ("MAIN.CSX", """
// Cerita AI dalam C# script: Ai.Start memulai, Ai.Next memberi potongan berikutnya
Ui.Window("Cerita AI (C#)", 520, 380);
Ai.Load();
string cerita = "";
bool menulis = false;
string info = "Model: " + Ai.Backend();
while (Ui.Running())
{
    Ui.Gradient(0, 0, 520, 380, 0x1B3A2A, 0x0E2018);
    Ui.Text(20, 14, "Cerita AI (C# script)", 0xFFFFFF, 2);
    if (Ui.Button(20, 56, 160, 36, "Ceritakan") && !menulis)
    {
        cerita = "Once upon a time";
        Ai.Start(cerita, 60);
        menulis = true;
    }
    if (menulis)
    {
        string p = Ai.Next();
        if (p == "") { menulis = false; info = Ai.Speed() + " token/detik"; }
        else cerita = cerita + p;
    }
    // bungkus per kata, 58 karakter per baris
    int y = 110, pos = 0;
    while (pos < cerita.Length)
    {
        int akhir = pos + 58;
        if (akhir >= cerita.Length) { Ui.Text(20, y, cerita.Substring(pos, cerita.Length - pos), 0xF0FFE0, 1); break; }
        int potong = akhir;
        while (potong > pos && cerita.Substring(potong, 1) != " ") potong--;
        if (potong == pos) potong = akhir;
        Ui.Text(20, y, cerita.Substring(pos, potong - pos), 0xF0FFE0, 1);
        y += 20;
        pos = potong + 1;
    }
    Ui.Text(20, 350, info, 0xF3C969, 1);
    Ui.Present();
}
"""),
            ("README.TXT", Readme("Cerita AI (C# script)", "Klik Ceritakan; potongan teks ditambahkan satu per frame."))),
    };
}
