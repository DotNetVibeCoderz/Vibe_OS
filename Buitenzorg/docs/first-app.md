# Your First App

Buitenzorg apps are written in **C#** and compiled to native ring-3 ELF with
**bflat** (`--stdlib:zero`). An app draws its own UI using the built-in
libraries and blits the result to a window with one syscall — the WPF/Avalonia
compositor model. This guide shows two paths — **scaffold with the SDK** (fastest
to start) and **add an app to the image** (how the preloaded suite is built) —
followed by an **example catalog** using the built-in libraries.

Prerequisite: you can already build and boot the OS (see
[Getting Started](getting-started.md)).

**English** · [Bahasa Indonesia](first-app.id.md) · ← [Documentation index](README.md)

---

## Path A — Scaffold with the `bz` SDK

```powershell
dotnet run --project sdk\bz -- new desktop-csharp MyApp
cd MyApp
dotnet run                                    # runs on the host (simulation backend)
dotnet run --project ..\sdk\bz -- manifest validate app.manifest
```

Available templates: `console-csharp`, `desktop-csharp`, `js-app`, `ts-app`,
`python-app`. This is great for developing logic on the host with the same API
as the target.

## Path B — Add a native app to the OS image

Every preloaded app (calculator, 2048, clock, piano, store) uses this pattern.
Apps live in `userland/hello-csharp/*.cs` and link against the built-in
libraries.

### 1. Write the app — `userland/hello-csharp/myapp.cs`

```csharp
using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

class MyApp
{
    static void Main()
    {
        Console.WriteLine("MyApp: starting...");         // → serial log
        Font font = Font.Default();
        UIHost host = new UIHost("MyApp", 300, 200);     // create a 300x200 window

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);

        root.Add(new TextBlock("HELLO WORLD", font));
        Button b = new Button("CLICK", font); b.Width = 120; b.Height = 30;
        root.Add(b);

        host.Root = root;
        host.Layout();                                   // compute layout
        host.Render(new Color(0xFF141820));              // draw into a Bitmap
        host.Present();                                  // blit to the window

        Console.WriteLine("MILESTONE: MYAPP OK");        // marker (optional)
    }
}
```

### 2. Register it in the build and the kernel

- **`scripts/build-hello-csharp.ps1`** (and `.sh`): add the program to the list,
  e.g. `@{src=@("myapp.cs","bzui.cs","bzgfx.cs"); elf="myapp.elf"}`.
- **`kernel/bzimage/build.rs`**: add `("myapp.elf","myapp.elf")` so the ELF is
  embedded in the image.
- **`kernel/bzkernel/src/app.rs`** (`app_file`): map the name → an 8.3 file, e.g.
  `"myapp" => Some("MYAPP.ELF")` (name ≤ 8 chars, uppercase).

### 3. Run it

- From the OS shell after boot: `run myapp`
- Or call `app::run_named("myapp")` in a `kernel_main` demo so it runs at boot
  (and its `MILESTONE:` marker can be checked by the smoke test).

Rebuild + boot: `scripts\build-hello-csharp.ps1`, then
`cargo run --release -p bzimage -- --run`.

## Built-in libraries

Link the relevant library `.cs` files at build time (see step 2 above).

### `Buitenzorg.Drawing` (`bzgfx.cs`) — 2D graphics

A client-side software renderer (System.Drawing-style). `Bitmap` (an ARGB
buffer) + `Graphics`: `FillRectangle` / `FillRoundedRectangle` (anti-aliased
corners), `FillRoundedGradientV`, `DrawShadow` (drop shadow), `DrawLine`
(thick), `DrawCircle` / `FillCircleAA`, `FillEllipse`, `FillPolygon`,
`FillGradientV/H`, `DrawImage` / `DrawImageScaled`, 2D transforms (`Matrix`,
`RotateTransform` + `SinFx` / `CosFx`), `GraphicsPath`, clipping (`SetClip`),
`FillHatch`, 24-bit BMP (`Bmp.Save/Load`), baseline JPEG (`Jpeg.Load`), and an
8×8 `Font` (`DrawString`, `DrawChars`, `MeasureString`).

```csharp
Bitmap bmp = new Bitmap(200, 120);
Graphics g = new Graphics(bmp);
g.FillGradientV(0, 0, 200, 120, Color.FromRgb(40,60,90), Color.FromRgb(10,15,25));
g.FillRoundedGradientV(20, 20, 120, 40, 8, Color.FromRgb(96,150,220), Color.FromRgb(48,88,150));
g.DrawString(Font.Default(), "HELLO", Color.White, 30, 30);
```

### `Buitenzorg.UI` (`bzui.cs`) — retained-mode toolkit (needs `bzgfx.cs`)

A `UIElement` visual tree + a `Measure/Arrange` layout pass. Layout:
`StackPanel`, `Grid` (fixed/star), `Canvas`, `Border`. Controls: `TextBlock`,
`Button`, `CheckBox`, `ProgressBar`, `Slider`, `RadioButton` (+`RadioGroup`),
`ListBox`, `TextBox`, `Menu`, `ComboBox`, `TabControl`, `TreeView`,
`ScrollViewer`, `DataGrid`. `UIHost` is the compositor:
`Layout()` / `Render()` / `Present()` + `Mouse(x,y,down)` for event routing
(hover/click/drag) and popups.

```csharp
Grid grid = new Grid();
grid.AddColumn(-1); grid.AddColumn(-1);   // two "star" columns
grid.AddRow(-1);
Button b = new Button("OK", font); b.GridRow = 0; b.GridCol = 0;
grid.Add(b);
```

> **Button clicks** have no event. A `Button` exposes `int Clicks` (bumped per
> click) and `int Tag`; react by calling `host.Mouse(...)` and checking whether
> `Clicks` increased. See `calc.cs` for the full pattern.

### `Buitenzorg.Audio` (`bzaudio.cs`) — audio (needs the kernel AC'97 driver)

`Mixer`: `GetInfo`, `SetVolume(0..100)`, `Mute`, `Beep(freqHz, ms)`,
`Play(short[] pcm, count)`. `Tone.Square(...)` generates a waveform.

```csharp
Mixer.SetVolume(70);
Mixer.Beep(440, 200);           // A4 for 200 ms
```

### `Buitenzorg.Bcl` (`bzbcl.cs`) — .NET-style collections / text / encoding

`BzList<T>`, `BzStack<T>`, `BzQueue<T>`, `BzIntMap<V>`, `BzStrMap<V>`,
`BzIntSet`, `BzRefList<T>`, `BzSort`, LINQ-style operators
(`Where/Select/Sum/Count/Any/All/Max/Min/First/Last/Take/Skip/...`),
`BzStringBuilder`, `BzMath`, `BzRandom`, `BzConvert`, `BzStr`, `BzHex`,
`BzBitConverter`, `BzBase64`.

### `Buitenzorg.Bcl` part 2 (`bzbcl2.cs`) — more .NET namespaces

Add **both `bzbcl.cs` and `bzbcl2.cs`** to the app's source list to use these.

| .NET namespace | Types | Example |
|---|---|---|
| `System.IO` | `BzPath`, `BzFile`, `BzDir`, `BzMemoryStream` | `BzFile.ReadAllChars("/disk/A.TXT", 4096, out t)` · `BzFile.WriteAllChars("/ram/N.TXT", t, n)` |
| `System.Text` | `BzEncoding` | `BzEncoding.Utf8GetBytes(chars, n, bytes)` |
| `System.Text.RegularExpressions` | `BzRegex` | `new BzRegex("^[0-9]+$").IsMatch("123")` |
| `System.Globalization` | `BzCulture`, `BzDateTime` | `BzCulture.FormatGrouped(1234567, buf)` · `BzDateTime.Now()` |
| `System.Diagnostics` | `BzStopwatch`, `BzProcess`, `BzDebug` | `BzProcess.GetProcesses(64)` · `BzProcess.Kill(pid)` |
| `System.Management` | `BzSystemInfo` | `BzSystemInfo.Query().UptimeSeconds` |
| `System.Net(.Sockets)` | `BzIPAddress`, `BzSocket`, `BzNetInfo` | `sock.SendTo(ip, 7000, "hi")` (UDP, loopback) |
| `System.Net.Http` | `BzHttp` | `BzHttp.BuildGet(host, path, buf)` — *message layer only, no TCP yet* |
| `System.Threading.Tasks` | `BzTask`, `BzMutex` | `BzTask.Run(&Worker, arg)` — the body is a **function pointer**, not a delegate |
| `System.Timers` | `BzTimer` | `timer.Start(); if (timer.Poll()) { ... }` (polled from the app loop) |
| `GC` | `BzGC` | `BzGC.GetAllocatedBytes()` — `Collect()` returns `false` (bump-only heap) |
| `Pkg` | `BzPkg` | `BzPkg.List(32)` · `BzPkg.Install(pkg)` |

Real usage lives in the suite apps: `clock.cs` (BzDateTime),
`taskmgr.cs` / `widget.cs` (BzProcess/BzSystemInfo), `store.cs` (BzPkg),
`filemgr.cs` (BzDir), `imgview.cs` / `editor.cs` (BzFile/BzPath).

## Example app ideas (all buildable today)

| App | Main library | Pattern |
|---|---|---|
| **Calculator** | UI (Grid + Button) | `calc.cs` — dispatch clicks via `Button.Tag` into an engine |
| **Game (2048 / Snake / puzzle)** | Drawing + UI | a custom `UIElement` board + logic in a value array |
| **Clock** | Drawing (Matrix/AA) | hands via `SinFx/CosFx`, digital via `DrawChars` |
| **Piano / music** | UI + Audio | keys → `Mixer.Beep(freq)` |
| **App Store** | UI (DataGrid) + PKG syscalls | catalog from `PKG_LIST`, install via `PKG_SET` |
| **Text editor** | UI (`TextBox`/`Menu`) | a text area + menu (needs keyboard input) |
| **Image viewer** | Drawing + Bcl | `BzFile.ReadAllBytes` → `Bmp.Load`/`Jpeg.Load` + `DrawImageScaled` |
| **System dashboard** | UI + Bcl | `BzSystemInfo`/`BzProcess` → `DataGrid`/`ProgressBar` |

See the real code in `userland/hello-csharp/` (`calc.cs`, `game2048.cs`,
`clock.cs`, `piano.cs`, `store.cs`) for complete examples.

| | | |
|---|---|---|
| ![Calculator](img/desktop-calc.png) | ![Clock](img/desktop-clock.png) | ![2048](img/desktop-2048.png) |
| ![Piano](img/desktop-piano.png) | ![App Store](img/desktop-store.png) | ![Image Viewer](img/desktop-imgview.png) |

## ⚠️ zerolib limitations (app authors must read)

Freestanding apps use zerolib (no full GC yet). What does **not** work:

- **Static reference-typed fields** read garbage (GC statics are uninitialized) →
  keep state in **instance fields / locals**, not `static` refs.
- **`new string(...)` / `ToString()` / dynamic string concat** → build text into
  a `char[]` / `stackalloc` and use `Graphics.DrawChars`.
- **`string == string`** needs `op_Equality` (absent) → compare by reference or
  char by char.
- **Storing a reference into an `object[]` element** (`stelem.ref`) faults → use
  a **linked list** (store into an object field), not an array of objects.
- **method-group → delegate** conversions (cached in a GC static) fault → use a
  **function pointer** `delegate*<...>` + `&Method`.

What **does** work: `new`, value arrays (`int[]`, `short[]`), heap objects,
generics, virtual dispatch, `stackalloc`. Full details in `CLAUDE.md`.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
