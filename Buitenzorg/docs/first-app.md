# Membuat App Pertama untuk Buitenzorg OS

App Buitenzorg ditulis dalam **C#** dan dikompilasi ke ELF native ring-3 dengan
**bflat** (`--stdlib:zero`). App menggambar UI-nya sendiri memakai library
built-in dan mem-blit hasilnya ke sebuah window lewat satu syscall — model
kompositor ala WPF/Avalonia. Panduan ini menunjukkan dua jalur: **scaffold via
SDK** (paling cepat memulai) dan **menambah app ke image** (cara suite bawaan
dibangun), lalu **katalog contoh** memakai library built-in.

Prasyarat: sudah bisa build+boot OS (lihat [getting-started.md](getting-started.md)).

---

## Jalur A — Scaffold dengan SDK `bz`

```powershell
dotnet run --project sdk\bz -- new desktop-csharp MyApp
cd MyApp
dotnet run                                    # jalan di host (backend simulasi)
dotnet run --project ..\sdk\bz -- manifest validate app.manifest
```

Template tersedia: `console-csharp`, `desktop-csharp`, `js-app`, `ts-app`,
`python-app`. Ini bagus untuk mengembangkan logika di host memakai API yang sama
dengan target.

---

## Jalur B — Menambah app native ke image OS

Semua app bawaan (kalkulator, 2048, jam, piano, store) memakai pola ini. App
ada di `userland/hello-csharp/*.cs` dan dilink dengan library built-in.

### 1. Tulis app-nya — `userland/hello-csharp/myapp.cs`

```csharp
using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

class MyApp
{
    static void Main()
    {
        Console.WriteLine("MyApp: mulai...");            // -> serial log
        Font font = Font.Default();
        UIHost host = new UIHost("MyApp", 300, 200);     // buat window 300x200

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);

        root.Add(new TextBlock("HALO DUNIA", font));
        Button b = new Button("KLIK", font); b.Width = 120; b.Height = 30;
        root.Add(b);

        host.Root = root;
        host.Layout();                                   // hitung tata letak
        host.Render(new Color(0xFF141820));              // gambar ke Bitmap
        host.Present();                                  // blit ke window

        Console.WriteLine("MILESTONE: MYAPP OK");        // penanda (opsional)
    }
}
```

### 2. Daftarkan di build & di kernel

- **`scripts/build-hello-csharp.ps1`** (dan `.sh`): tambahkan program-nya ke
  daftar, mis. `@{src=@("myapp.cs","bzui.cs","bzgfx.cs"); elf="myapp.elf"}`.
- **`kernel/bzimage/build.rs`**: tambahkan `("myapp.elf","myapp.elf")` agar ELF
  ter-embed ke image.
- **`kernel/bzkernel/src/app.rs`** (`app_file`): petakan nama → file 8.3, mis.
  `"myapp" => Some("MYAPP.ELF")` (nama ≤ 8 karakter, huruf besar).

### 3. Jalankan

- Dari shell OS setelah boot: `run myapp`
- Atau panggil `app::run_named("myapp")` di sebuah demo `kernel_main` agar jalan
  saat boot (dan penanda `MILESTONE:` bisa diverifikasi smoke test).

Rebuild + boot: `scripts\build-hello-csharp.ps1` lalu
`cargo run --release -p bzimage -- --run`.

---

## 📚 Library built-in

Tautkan file `.cs` library yang relevan saat build (lihat langkah 2).

### `Buitenzorg.Drawing` (`bzgfx.cs`) — grafis 2D
Renderer software client-side (System.Drawing-style). `Bitmap` (buffer ARGB) +
`Graphics`: `FillRectangle`/`FillRoundedRectangle` (sudut anti-alias),
`FillRoundedGradientV`, `DrawShadow` (drop shadow), `DrawLine` (tebal),
`DrawCircle`/`FillCircleAA`, `FillEllipse`, `FillPolygon`, `FillGradientV/H`,
`DrawImage`/`DrawImageScaled`, transform 2D (`Matrix`, `RotateTransform` +
`SinFx`/`CosFx`), `GraphicsPath`, clipping (`SetClip`), `FillHatch`, 24-bit BMP
(`Bmp.Save/Load`), teks 8×8 (`Font`, `DrawString`, `DrawChars`, `MeasureString`).

```csharp
Bitmap bmp = new Bitmap(200, 120);
Graphics g = new Graphics(bmp);
g.FillGradientV(0, 0, 200, 120, Color.FromRgb(40,60,90), Color.FromRgb(10,15,25));
g.FillRoundedGradientV(20, 20, 120, 40, 8, Color.FromRgb(96,150,220), Color.FromRgb(48,88,150));
g.DrawString(Font.Default(), "HALO", Color.White, 30, 30);
```

### `Buitenzorg.UI` (`bzui.cs`) — toolkit retained (butuh `bzgfx.cs`)
Visual tree `UIElement` + layout `Measure/Arrange`. Layout: `StackPanel`,
`Grid` (fixed/star), `Canvas`, `Border`. Kontrol: `TextBlock`, `Button`,
`CheckBox`, `ProgressBar`, `Slider`, `RadioButton`(+`RadioGroup`), `ListBox`,
`TextBox`, `Menu`, `ComboBox`, `TabControl`, `TreeView`, `ScrollViewer`,
`DataGrid`. `UIHost` = kompositor: `Layout()`/`Render()`/`Present()` +
`Mouse(x,y,down)` untuk routing event (hover/klik/drag) & popup.

```csharp
Grid grid = new Grid();
grid.AddColumn(-1); grid.AddColumn(-1);   // 2 kolom "star"
grid.AddRow(-1);
Button b = new Button("OK", font); b.GridRow = 0; b.GridCol = 0;
grid.Add(b);
```

### `Buitenzorg.Audio` (`bzaudio.cs`) — audio (butuh driver AC'97 di kernel)
`Mixer`: `GetInfo`, `SetVolume(0..100)`, `Mute`, `Beep(freqHz, ms)`,
`Play(short[] pcm, count)`. `Tone.Square(...)` bikin gelombang.

```csharp
Mixer.SetVolume(70);
Mixer.Beep(440, 200);           // A4 selama 200 ms
```

### `Buitenzorg.Bcl` (`bzbcl.cs`) — koleksi/teks/encoding gaya .NET
`BzList<T>`, `BzStack<T>`, `BzQueue<T>`, `BzIntMap<V>`, `BzStrMap<V>`,
`BzIntSet`, `BzRefList<T>`, `BzSort`, LINQ-style
(`Where/Select/Sum/Count/Any/All/Max/Min/First/Last/Take/Skip/...`),
`BzStringBuilder`, `BzMath`, `BzRandom`, `BzConvert`, `BzStr`, `BzHex`,
`BzBitConverter`, `BzBase64`.

### `Buitenzorg.Bcl` bagian 2 (`bzbcl2.cs`) — namespace .NET lainnya
Tambahkan **`bzbcl.cs` + `bzbcl2.cs`** ke daftar sumber app untuk memakainya.

| Namespace .NET | Tipe | Contoh |
|---|---|---|
| `System.IO` | `BzPath`, `BzFile`, `BzDir`, `BzMemoryStream` | `BzFile.ReadAllChars("/disk/A.TXT", 4096, out t)` · `BzFile.WriteAllChars("/ram/N.TXT", t, n)` |
| `System.Text` | `BzEncoding` | `BzEncoding.Utf8GetBytes(chars, n, bytes)` |
| `System.Text.RegularExpressions` | `BzRegex` | `new BzRegex("^[0-9]+$").IsMatch("123")` |
| `System.Globalization` | `BzCulture`, `BzDateTime` | `BzCulture.FormatGrouped(1234567, buf)` · `BzDateTime.Now()` |
| `System.Diagnostics` | `BzStopwatch`, `BzProcess`, `BzDebug` | `BzProcess.GetProcesses(64)` · `BzProcess.Kill(pid)` |
| `System.Management` | `BzSystemInfo` | `BzSystemInfo.Query().UptimeSeconds` |
| `System.Net(.Sockets)` | `BzIPAddress`, `BzSocket`, `BzNetInfo` | `sock.SendTo(ip, 7000, "halo")` (UDP, loopback) |
| `System.Net.Http` | `BzHttp` | `BzHttp.BuildGet(host, path, buf)` — *lapisan pesan saja, TCP belum ada* |
| `System.Threading.Tasks` | `BzTask`, `BzMutex` | `BzTask.Run(&Worker, arg)` — body **function pointer**, bukan delegate |
| `System.Timers` | `BzTimer` | `timer.Start(); if (timer.Poll()) { ... }` (di-poll dari loop app) |
| `GC` | `BzGC` | `BzGC.GetAllocatedBytes()` — `Collect()` = `false` (heap masih bump-only) |
| `Pkg` | `BzPkg` | `BzPkg.List(32)` · `BzPkg.Install(pkg)` |

Contoh nyata pemakaiannya ada di app suite: `clock.cs` (BzDateTime),
`taskmgr.cs`/`widget.cs` (BzProcess/BzSystemInfo), `store.cs` (BzPkg),
`filemgr.cs` (BzDir), `imgview.cs`/`editor.cs` (BzFile/BzPath).

---

## 💡 Ide contoh app (semua bisa dibuat sekarang)

| App | Library utama | Pola |
|-----|---------------|------|
| **Kalkulator** | UI (Grid + Button) | `calc.cs` — dispatch klik via `Button.Tag` ke engine |
| **Game (2048 / Snake / puzzle)** | Drawing + UI | papan `UIElement` custom + logika di value-array |
| **Jam** | Drawing (Matrix/AA) | jarum via `SinFx/CosFx`, digital via `DrawChars` |
| **Piano / musik** | UI + Audio | tuts → `Mixer.Beep(freq)` |
| **App Store** | UI (DataGrid) + syscall PKG | katalog dari `PKG_LIST`, install via `PKG_SET` |
| **Editor teks** | UI (`TextBox`/`Menu`) | area teks + menu (butuh input keyboard) |
| **Viewer gambar** | Drawing + Bcl | `BzFile.ReadAllBytes` → `Bmp.Load`/`Jpeg.Load` + `DrawImageScaled` |
| **Dashboard sistem** | UI + Bcl | `BzSystemInfo`/`BzProcess` → `DataGrid`/`ProgressBar` |

Lihat kode nyata di `userland/hello-csharp/` (calc.cs, game2048.cs, clock.cs,
piano.cs, store.cs) sebagai contoh lengkap.

---

## ⚠️ Batasan zerolib (WAJIB dibaca app author)

App freestanding memakai zerolib (belum ada GC penuh). Yang **tidak** bekerja:

- **Static field bertipe referensi** baca sampah (GC statics belum di-init) →
  simpan state di **field instance / lokal**, bukan `static` ref.
- **`new string(...)` / `ToString()` / concat string dinamis** → bangun teks ke
  `char[]`/`stackalloc` lalu `Graphics.DrawChars`.
- **`string == string`** butuh `op_Equality` (tak ada) → banding referensi
  by-reference atau char demi char.
- **Store referensi ke elemen `object[]`** (`stelem.ref`) fault → pakai
  **linked-list** (store ke FIELD objek), bukan array-of-object.
- **method-group → delegate** (cache di GC static) fault → pakai **function
  pointer** `delegate*<...>` + `&Method`.

Yang **bekerja**: `new`, array nilai (`int[]`, `short[]`), objek heap, generic,
virtual dispatch, `stackalloc`. Detail lengkap di `CLAUDE.md`.
