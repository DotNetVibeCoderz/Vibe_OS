# Buitenzorg Studio

**Studio** is the IDE inside Buitenzorg OS: write a program, run it with F5,
read its output and its errors — without leaving the desktop. Open it from the
**Studio** desktop icon, Start menu → Kantor → Studio, or `run mstudio`.

![Buitenzorg Studio](img/studio.png)

## The window

| Part | What it does |
|---|---|
| **Project** pane (left, Ctrl+B) | The files of the current project folder; click a folder to enter it, `../` to go up, a file to open it. **+ Proyek baru** / **+ Berkas baru**. |
| Tabs + editor | One tab per open file (`*` = unsaved). Line numbers, syntax colors (C#, C# script, JavaScript, TypeScript, Python, shell, JSON), the current line, a red mark on lines with errors. |
| **Jalankan (F5)** | Saves every file and runs the current one: `.CSX` through the [C# script](csharp-script.md) bytecode engine, `.JS`/`.TS`/`.PY` through the kernel's script interpreter, `.SH` through the shell; a script that calls `window(...)` opens as its own app with a window ([UI apps](script-ui.md)). |
| **Keluaran** | The program's output and how long it took. |
| **Masalah** | Errors with file and line (`parse: baris 2: ...`); click one to jump there. |
| **Terminal** | The same shell as the desktop Terminal (pipes, scripts, `bz`, ...). |
| Status bar | File, language, line and column, saved or not, path. |

## Keys

| Key | Does |
|---|---|
| F5 | run the current file |
| Ctrl+S / Ctrl+W | save / close the tab |
| Ctrl+N | new project (template picker) |
| Ctrl+Z / Ctrl+Y | undo / redo (typing is grouped per word) |
| Ctrl+D / Ctrl+K | duplicate / delete the line |
| Ctrl+F / Ctrl+R | find / replace (bar above the editor; Enter = next) |
| Ctrl+G | go to line |
| Tab, Enter | indent; a new line keeps the indentation (one level more after `{`, `(`, `[`, a Python `:` or a shell `then`/`do`); `}` on its own line steps back |
| Home | first non-blank character, then column 0 |
| Ctrl+B / Ctrl+T | project pane / light-dark theme |

## Project templates (52)

**Ctrl+N** opens the template picker; each template is a small project that
runs as it is (C# ones are built on a PC and carry `BUILD.TXT`).

| Language | Use cases |
|---|---|
| C# script | hello · List & Dictionary · text analysis · sorting algorithms · exceptions · statistics & math |
| C# script UI | counter · Snake game · function plotter · address book · to-do list |
| JavaScript | Hello world · recursion (Fibonacci, factorial) · primes, GCD and LCM · FizzBuzz · unit conversion table · savings simulation |
| JavaScript UI | counter · drawing board · catch-the-ball game · sign-up form · Pomodoro timer |
| TypeScript | Hello with types/interfaces · student grade statistics · two-player dice game (LCG random) · cashier receipt |
| TypeScript UI | calculator · bouncing-ball animation · currency converter · Pong · spirograph |
| Python | Hello · multiplication table · binary-search guessing · calendar (leap years, Zeller) · Collatz sequence |
| Python UI | stopwatch · multiple-choice quiz · RGB color mixer · BMI calculator · number guessing · mandala |
| Shell script | arguments/loops/conditions · text-file backup · system report · temporary-file cleaner · Terminal profile with aliases |
| C# | native console (bflat) · native desktop (Buitenzorg.UI) · managed app (RustCLR) |
| Package | a `.bzpkg` project (manifest + script) |
| JSON | a data file |

Projects live in `/disk/PROYEK/<NAME>` (8-letter folder names; `/ram/PROYEK`
where there is no `/disk`). The first start creates `HALO` from the "Hello
world" template.

`run mstudio --list` prints the catalog; `run mstudio --export /ram/T` writes
every template's main file as `/ram/T/T01.CSX` ... `T52.JSON`, so one can be
tried straight from the Terminal (`run /ram/T/T21.JS`).

![UI templates running side by side](img/studio-ui-templates.png)

## What the script languages can do

The kernel's interpreter (`script.rs`) is a small subset: integers and
strings, `+ - * / %`, comparisons, `&& || !`, functions and recursion,
`if/else` (`elif` in Python), `while`, `for` (C-style in JS/TS, `range()` in
Python), `print` / `console.log`, `str`, `len`, `abs`. No arrays, objects,
floats or imports yet; every run has a step budget. Parse errors carry the
line number shown in **Masalah**.

## Limits (honest)

- C# is not compiled inside the OS: the C# templates explain the build on a PC
  (bflat / `dotnet build`, `bz pack`, deploy).
- No breakpoints or debugger yet; no mouse selection or clipboard in the
  editor.
- Runtime errors of a script have no line number (only parse errors do).
- Large files: the editor keeps lines in memory and draws only the visible
  ones, but it runs in the RustCLR interpreter — files of a few thousand lines
  stay usable.

Implementation: `userland/clr-apps/MStudio` (`Studio.cs`, `CodeEditor.cs`,
`Syntax.cs`, `Templates.cs`); runs through syscall **`SHELL_RUN` (58)** —
`Buitenzorg.App.Shell.Run(line)`. Self-test: 25 checks (it runs every runnable template, the UI ones as headless processes), part of
`Managed --selftest` at boot.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
