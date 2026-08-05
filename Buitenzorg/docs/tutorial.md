# Tutorial: From Zero to Your First App

A sequential walkthrough from **building and booting the OS** to **writing your
own app** and **debugging/profiling the kernel**. Follow it top to bottom, or
jump to the part you need — each section links a deeper doc for the details.

> Prerequisites and full troubleshooting: [Getting Started](getting-started.md).

**English** · [Bahasa Indonesia](tutorial.id.md) · ← [Documentation index](README.md)

**The journey:**
1. [Build & boot](#1-build--boot-5-minutes) — run the OS in QEMU
2. [Tour the desktop](#2-tour-the-desktop) — start menu, icons, the app suite
3. [The shell](#3-the-shell-terminal) — commands, themes, workspaces, polyglot
4. [Your first app](#4-your-first-app-c) — from template to `run`
5. [Use the libraries](#5-use-the-built-in-libraries) — Drawing / UI / Audio / Bcl
6. [Debug & profile](#6-debug--profile-the-kernel) — GDB + profiler
7. [Leave QEMU](#7-leave-qemu) — VMs & USB hardware
8. [Next steps](#8-next-steps)

---

## 1. Build & boot (5 minutes)

Fastest path — one script installs the dependencies and boots:

```powershell
.\scripts\quickstart.ps1     # Linux/macOS: ./scripts/quickstart.sh
```

Or manually, if the dependencies are already installed (Rust nightly, .NET SDK,
QEMU, bflat):

```powershell
.\scripts\build.ps1          # → dist\buitenzorg-{bios,uefi}.img
.\scripts\run-qemu.ps1       # boot with a display + serial
```

It takes about a minute to reach `BUITENZORG READY` (the kernel runs dozens of
milestone demos on the way). The kernel log appears on serial **and** on the
framebuffer; once the desktop renders, it covers the boot text.

**Verify without a display** (what CI does):

```powershell
.\scripts\smoke-test.ps1     # headless boot, assert every MILESTONE marker
```

➡️ Setup details, dependency list, troubleshooting: **[Getting Started](getting-started.md)**.

## 2. Tour the desktop

After `READY`, the desktop is live (mouse and keyboard work in QEMU):

- **Start button** (bottom-left, green) → the **start menu**: app list + power actions.
- **Desktop icons** (top-left) → double-click to launch an app.
- **Taskbar**: running-window buttons + a **tray** (theme name + a **live RTC
  clock** + workspace pips).
- **Preloaded suite** (8 apps): Calculator, Text Editor, 2048, Clock, File
  Manager, Piano, Image Viewer, App Store.

![The Buitenzorg desktop](img/desktop-shell.png)

➡️ Desktop concepts (compositor, window manager, themes, workspaces):
**[Desktop Environment](desktop-environment.md)** · **[Window System](window-system.md)**.

## 3. The shell (terminal)

Open the Terminal from the desktop. Try:

```
help                 # list commands
ls /disk             # disk contents (the app suite lives here)
cat /ram/DAHAN.TXT   # read a file
theme cycle          # cycle the 8 themes (live)
ws 2                 # switch to workspace 2
run calc             # launch the Calculator
run editor           # the Editor — interactive: type, Ctrl+S to save
prof self            # profile a desktop recompose (report on serial)
ask hello world      # the local LLM completes text
bz model list        # the Hugging Face-style model gallery
vm create nanovm     # create a VM (guest: NanoOS)
vm start nanovm      # boot the tiny guest OS on the software VMM
vm list              # list VMs + status
```

**Polyglot** — run JS/TS/Python on the in-kernel interpreter:

```
js                   # the built-in JavaScript demo
py main.py           # run a Python file from the VFS
script ts main.ts    # TypeScript (transpiled, then interpreted)
```

➡️ System services (VFS, service manager, async I/O, networking):
**[System Services](system-services.md)** · AI & power: **[AI & Power](ai-power.md)**.

## 4. Your first app (C#)

Two paths. **Fast path — use an SDK template:**

```powershell
dotnet run --project sdk\bz -- new console-csharp MyApp
```

**Native path — add a ring-3 C# app** to the build (like the suite apps):

1. Write `userland/hello-csharp/myapp.cs` (a class with `static void Main`).
2. Register it in `scripts/build-hello-csharp.ps1` **and** `.sh` (the program list).
3. Embed it in `kernel/bzimage/build.rs` (`("myapp.elf", "myapp.elf")`).
4. Give it a launch name in `kernel/bzkernel/src/app.rs` (`"myapp" => Some("MYAPP.ELF")`).
5. Rebuild the apps + image, then `run myapp` in the shell.

A minimal example that prints a milestone:

```csharp
using System;
class Program {
    static void Main() {
        Console.WriteLine("Hello from my app!");
        Console.WriteLine("MILESTONE: MYAPP OK");
    }
}
```

➡️ The full guide for both paths + an example catalog: **[Your First App](first-app.md)**.

> ⚠️ **The zerolib rules (must read).** Freestanding apps: the heap **works**
> (`new`, arrays, generics), but there are **no** static reference fields, **no**
> method-group→delegate conversions (use function pointers), **no** storing a
> reference into an `object[]` element (use a linked list), and **no**
> `new string()` / `ToString()` / concatenation (use `char[]` +
> `Graphics.DrawChars`). Details: [first-app.md](first-app.md).

## 5. Use the built-in libraries

C# apps have four libraries (add their source files to your build):

| Library | File | For |
|---|---|---|
| **Buitenzorg.Drawing** | `bzgfx.cs` | graphics: Graphics/Bitmap/transforms/Font, BMP + JPEG |
| **Buitenzorg.UI** | `bzui.cs` | retained-mode toolkit: Button/Grid/ListBox/… (needs Drawing) |
| **Buitenzorg.Audio** | `bzaudio.cs` | mixer + tone/PCM (AC'97) |
| **Buitenzorg.Bcl** | `bzbcl.cs` + `bzbcl2.cs` | collections/LINQ + System.IO/Text/Regex/Net/Tasks/… |

Example — a UI window + a file read + the real time:

```csharp
using Buitenzorg;                 // the BCL
using Buitenzorg.UI;

var host = new UIHost("Demo", 320, 200);
var root = new StackPanel { Padding = 12 };
root.Add(new TextBlock("Hello Buitenzorg", Font.Default()));
host.Root = root; host.Layout();
host.Render(new Buitenzorg.Drawing.Color(0xFF1C2028)); host.Present();

// System.Globalization + System.IO from Buitenzorg.Bcl:
var now = BzDateTime.Now();                    // the real CMOS clock
byte[] data; BzFile.ReadAllBytes("/disk/PHOTO.BMP", 400*1024, out data);
```

➡️ The per-library API catalog + real app examples: **[Your First App](first-app.md)**
(see the suite apps `calc.cs`, `clock.cs`, `store.cs`, `imgview.cs`).

## 6. Debug & profile the kernel

**GDB attach** — boot QEMU paused, then step through ring 0:

```powershell
.\scripts\debug-kernel.ps1        # Linux/macOS: ./scripts/debug-kernel.sh
```
```gdb
(gdb) bz-break-main               # break at kernel_main
(gdb) continue
(gdb) bt
```

**Profiler** — measure where the cycles go (in the OS shell):

```
prof self                         # profile a desktop recompose; report on serial
```

➡️ The full flow + GDB helpers + how to add a profiler zone:
**[Debugging & Profiling](debugging.md)**.

## 7. Leave QEMU

**VM (VMware / VirtualBox / Hyper-V):**

```powershell
.\scripts\make-vm-images.ps1      # → .vmdk + .vdi + .vhdx
```
➡️ **[Run in a VM](run-in-vm.md)**.

**Physical machine (boot from USB):**

```powershell
.\scripts\flash-usb.ps1 -List     # list USB disks
.\scripts\flash-usb.ps1 -DiskNumber <N> -Firmware uefi
```
➡️ **[Install on Hardware](install-hardware.md)** — firmware choice, boot menu,
capability matrix. *(Hardware boot is still experimental.)*

## 8. Next steps

- **Roadmap & status:** [PLAN.md](../PLAN.md) · [Progress.md](../Progress.md) · [CHANGELOG.md](../CHANGELOG.md).
- **The syscall ABI** (if you add a syscall): [Syscall ABI](abi.md).
- **C# ↔ kernel** (interop, ELF loader): [C# in Ring 3](csharp-userland.md).
- **App framework & SDK:** [App Framework](app-framework.md).
- **Full technical spec:** [requirements.md](../requirements.md) *(ID)*.
- **Contributing:** [CONTRIBUTING.md](../CONTRIBUTING.md).
- **MagicAppGen** — generate Buitenzorg apps from a prompt with LLM help:
  `tools/MagicAppGen/README.md`.

Happy hacking — *zonder zorg, without worries.* 🌱

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
