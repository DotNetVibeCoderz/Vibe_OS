# Buitenzorg Desktop App (C#)

A starter **desktop** app for Buitenzorg OS (requirements.md §11.2), built on the
retained-mode **Buitenzorg.UI** toolkit over **Buitenzorg.Drawing**. The template
renders a window (title + a live counter + a gauge + a `+1` button), verifies
itself headlessly, and runs a live keyboard loop when launched from the desktop.

Buitenzorg OS is made by **Gravicode Studios**, led by **Kang Fadhil**.

## Files

| File | Purpose |
| --- | --- |
| `app.cs` | The app — a `UIHost` window with `StackPanel` / `TextBlock` / `Button` / `Gauge` + a custom `CounterView` |
| `app.manifest` | Unified manifest (`type=desktop`, `language=csharp`) |
| `build.ps1` / `build.sh` | Compile → link → **deploy** the app as `/disk/USERAPP.ELF` |
| `.vscode/tasks.json` | `Ctrl+Shift+B` build tasks (Build & Deploy; Build, Deploy & Run) |
| `.vscode/launch.json` | `F5` launch configs (Deploy & Run; Debug Kernel via GDB) |
| `.template.config/` | `dotnet new` template metadata (`dotnet new bzapp`) |

## Build, deploy & run

The app is compiled **freestanding** with bflat (`--stdlib:zero`) against the
Buitenzorg.UI / .Drawing library sources, linked with the `bzstart` shim into a
static ELF, and deployed as `userland/hello-csharp/userapp.elf` — which the
kernel image embeds as `/disk/USERAPP.ELF`. Launch it in the OS with `run myapp`.

### From the command line

```powershell
.\build.ps1            # build + deploy the ELF
.\build.ps1 -Run       # build + deploy, rebuild the image, boot QEMU → type: run myapp
```

```bash
./build.sh             # build + deploy
./build.sh --run       # build + deploy, rebuild image, boot QEMU → type: run myapp
```

Pull in more libraries with `-Libs` / `LIBS` (e.g. add `bzbcl.cs bzbcl2.cs` for
the BCL, or `bzaudio.cs` for sound):

```powershell
.\build.ps1 -Libs bzgfx.cs,bzui.cs,bzbcl.cs,bzbcl2.cs
```

The build auto-detects the repo root (it walks up for `tools/bflat`); pass
`-RepoRoot` / `REPO_ROOT` if the app lives outside the repo tree. You need the
toolchain from `scripts/quickstart` (bflat + rust nightly).

### From VS Code

Install the **Buitenzorg SDK** extension (`sdk/vscode-extension`), then open this
folder:

- **`Ctrl+Shift+B`** → *Buitenzorg: Build & Deploy* — compile + deploy the ELF.
- **Run Task → *Build, Deploy & Run*** — also rebuilds the image and boots QEMU;
  type `run myapp` at the OS prompt.
- **`F5` → *Deploy & Run*** — the same, as a launch config.
- **`F5` → *Debug Kernel (QEMU + GDB)*** — boots QEMU paused with a GDB server on
  `:1234` for kernel-level debugging (attach against the un-stripped
  `bzkernel`). Managed-app breakpoint debugging (DAP) is future work; for app
  logic use serial `Console.WriteLine` (visible on the serial log / QEMU stdio).

### From MagicAppGen

In **MagicAppGen** (`tools/MagicAppGen`), ask Jack to scaffold or edit the app;
he knows the Buitenzorg.UI API (via `GetApiReference("ui")`) and validates code
with the `CompileCheck` function. Point `Buitenzorg.Root` at the repo, then use
**Build ▸ Run** (or Jack's `BuildApp`/`RunApp`) to build the image and smoke-test
it. To iterate on this exact app, run `build.ps1` from the project folder.

## zerolib notes (heap works, no reclaiming GC yet)

Since v0.15 "Matang", **`new`, arrays, heap objects, and generics work** (a
growing bump heap over `mmap`). Still unsupported:

- **No static reference fields** — GC statics are uninitialized, so
  `static readonly char[] X = ...` reads garbage. Keep state in **locals** or
  **instance fields** (this app keeps the count on `CounterView.Value`).
- **No method-group → delegate** conversions (the delegate caches in a GC
  static). Use **function pointers**: `delegate*<int,bool>` + `&Method`.
- **No storing a reference into an `object[]` element** (`stelem.ref` faults).
  Use a **linked list** or object fields (Buitenzorg.UI children are a list).
- **No `new string()` / `ToString()` / concat / `string ==`.** Build text in a
  `char[]` and draw with `Graphics.DrawChars` — see `CounterView` and
  `Buitenzorg.UI.UiText.Int`.
- **No reclaiming GC yet** — memory is freed only when the app exits.

Available libraries: `Buitenzorg.Drawing` (`bzgfx.cs`), `Buitenzorg.UI`
(`bzui.cs`), `Buitenzorg.Audio` (`bzaudio.cs`), and `Buitenzorg.Bcl`
(`bzbcl.cs` + `bzbcl2.cs` — collections/LINQ + System.IO/Text/Regex/
Globalization/Diagnostics/Management/Net.Sockets/Tasks/Timers/GC/Pkg). See
`docs/first-app.md` for the API catalogue.

## More examples

See `userland/hello-csharp/` for complete, running apps: `ui.cs` (the full
control gallery), `calc.cs` (Calculator), `game2048.cs` (2048), `editor.cs`
(interactive Text Editor), `imgview.cs` (Image Viewer).
