# Debugging & Profiling

Two developer tools for the Buitenzorg kernel (v1.0):

1. **Debugger** — attach GDB to the kernel running in QEMU (breakpoints,
   single-step, register/memory inspection in ring 0).
2. **Profiler** — an instrumented, TSC-based zone profiler inside the kernel
   (measure where CPU cycles go, deterministically).

**English** · [Bahasa Indonesia](debugging.id.md) · ← [Documentation index](README.md)

---

## 🐞 Debugger (GDB + QEMU)

QEMU provides a **GDB stub**: the kernel boots **paused** with a GDB server on
`tcp:1234`, then GDB attaches using the **kernel symbols** from the un-stripped
ELF `kernel/target/x86_64-unknown-none/release/bzkernel`.

### The quick way (scripts)

**Windows:**
```powershell
.\scripts\debug-kernel.ps1            # BIOS image, auto-attach GDB
.\scripts\debug-kernel.ps1 -Uefi      # UEFI image
.\scripts\debug-kernel.ps1 -NoAttach  # just start QEMU paused; attach GDB yourself
```
**Linux / macOS:**
```bash
./scripts/debug-kernel.sh             # BIOS image, auto-attach GDB
./scripts/debug-kernel.sh --uefi
./scripts/debug-kernel.sh --no-attach
```

The script: (1) finds the kernel ELF with symbols (release, falling back to
debug), (2) runs QEMU with `-gdb tcp::1234 -S` (paused), (3) attaches GDB with
`scripts/debug-kernel.gdb` + `target remote :1234`. If `gdb` is not on PATH, the
script still starts QEMU and prints the manual attach commands.

Prerequisites: the kernel is built (`build.ps1`/`build.sh`) and **`gdb`** (or
`gdb-multiarch` on Linux) is available. QEMU is auto-detected (or the `QEMU` env var).

### A typical session

```gdb
(gdb) bz-break-main        # break at kernel_main (a Buitenzorg helper)
(gdb) continue             # run to the entry
(gdb) bt                   # backtrace
(gdb) info registers rip rsp
(gdb) stepi                # one instruction
(gdb) break page_fault_handler
(gdb) x/8i $pc             # disassemble 8 instructions at PC
```

`scripts/debug-kernel.gdb` adds these helpers:

| Command | Does |
|---|---|
| `bz-break-main` | break at `kernel_main` (right after the bootloader handoff) |
| `bz-faults` | break on the page/double/GP fault handlers — stop in the debugger instead of scrolling a rodata dump |
| `bz-regs` | a compact general-register dump |
| `bz-help` | list the helpers |

Rust symbols are mangled (`_RNvCs...8bzkernel11kernel_main`); GDB demangles them
automatically, so `break kernel_main` / `break page_fault_handler` work.

> **The manual way** (no script): run `QEMU_EXTRA="-s -S"` through the normal
> runner (e.g. `cargo run -p bzimage -- --run`), then from another terminal:
> ```
> gdb -x scripts/debug-kernel.gdb kernel/target/x86_64-unknown-none/release/bzkernel
> (gdb) target remote :1234
> ```

### Alternative: debug via serial

All kernel logs go to **serial** (COM1) and the framebuffer console. For quick
tracing without GDB, kernel `println!` shows up on serial — the smoke test and
runner already route it there. This is often faster than a breakpoint for
verifying the boot flow.

## 📊 Profiler (instrumented TSC zones)

The kernel has a lightweight zone profiler (`kernel/bzkernel/src/profile.rs`):
wrap a scope in `profile::Guard::new("name")` and the elapsed CPU cycles (from
the **timestamp counter**) accumulate into a per-name bucket. `profile::report()`
prints a sorted table — call count, total/avg/max cycles, and share of the total.

Characteristics:

- **Inert when off.** `Guard::new` does one atomic read when the profiler is off,
  so instrumentation left in the code **does not disturb normal boot timing**.
  Turn it on with `profile::enable()` around the region you want to measure.
- **Deterministic**, not statistical: it measures each scope's actual inclusive
  wall-cycles, so a headless run can assert exact call counts and relative costs
  (unlike a sampling profiler).
- **Single-core / cooperative.** The registry sits behind a spin lock (interrupts
  disabled while held), so it is safe against the timer IRQ; it is not meant to
  be called from an interrupt handler.

### Built-in instrumentation points

Already placed on hot paths (inert unless enabled):

| Zone | Location |
|---|---|
| `syscall` | total time servicing ring-3 syscalls (`dispatch_from_user`) |
| `wm::compose` | the compositor building a frame (the deepest part of `WIN_PRESENT`) |
| `fb::present` | blitting the back buffer to the framebuffer |

### From the shell

```
prof self       # profile a real workload (recompose the desktop 8×), then report
prof on         # enable the profiler
prof off        # disable it
prof reset      # clear the accumulation
prof report     # print the table (to the serial log)
```

`prof self` enables the profiler, calls `wm::present_now()` a few times, then
prints the report — a fast way to see the cost of a desktop recompose. The full
report goes to the **serial log** (the table is wide for the desktop terminal).

### Example report (from the boot self-test)

```
[profile] zone report (3 zones, 492164098 total cycles):
[profile] zone                        calls          total          avg          max  share
[profile] demo-outer                     20      246925905     12346295     13223062   50.1%
[profile] demo-expensive                 20      233384338     11669216     12610684   47.4%
[profile] demo-cheap                     20       11853855       592692       726510    2.4%
```

The boot self-test (`profiler_demo` in `main.rs`) runs nested zones with a known
cost ratio (one scope spins 20× more), then asserts: exact call counts, the cheap
zone < the expensive one, the outer scope encloses both, and a zone recorded
while the profiler was **off** does not appear → `MILESTONE: PROFILER OK`.

### Adding instrumentation

```rust
fn hot_path() {
    let _z = crate::profile::Guard::new("hot_path");
    // ... work ...
}   // the Guard drops here and records the elapsed cycles
```

Zone names are compared **by content** (not by pointer), so the same literal at
different call sites merges into one bucket. Up to 64 distinct zones; extras are
counted in `overflow` and reported (it degrades loudly, not silently).

Limitation: it measures a scope's **inclusive** time (including its children);
recursion of a same-named zone double-counts. For "self time", split the child
work into its own zone (as with `wm::compose` vs `fb::present`).

## 🪲 Debugger for apps (`dbg`)

Ring-3 programs (native ELF: C# built with bflat, Rust, ...) can be run
under a debugger in the Terminal -- or from a PC with `bz run "dbg ..."`
([bz-link](bz-link.md)):

```text
> dbg run crashme
pid 1000 stopped (entry) at 0x40008c  _start
> dbg break Level3
breakpoint 1 at 0x401480  crashme_Crash__Level3
> dbg cont
pid 1000 stopped (breakpoint) at 0x401480  crashme_Crash__Level3
> dbg bt
backtrace (return addresses found on the stack, innermost first):
  #0  0x401480  crashme_Crash__Level3
  #1  0x4014a6  crashme_Crash__Level2+0xa
  #2  0x4014b4  crashme_Crash__Level1+0x8
> dbg cont
pid 1000 stopped (fault) at 0x401481  crashme_Crash__Level3+0x1
> dbg cont
pid 1000 exited with code 0xdead
```

| Command | Does |
|---|---|
| `dbg run APP [args]` | start a program under the debugger; it stops at its entry |
| `dbg break SYM\|ADDR` | breakpoint on a function (its name or a unique part of it) or `0x` address |
| `dbg delete SYM\|ADDR`, `dbg breaks` | remove / list breakpoints |
| `dbg cont`, `dbg step` | continue to the next breakpoint / one instruction |
| `dbg regs`, `dbg set REG VALUE` | all registers / change one |
| `dbg mem ADDR [N]` | memory in hex + text (breakpoints show the original bytes) |
| `dbg bt` | symbolized backtrace (as in a crash report) |
| `dbg kill`, `dbg detach` | end the program / let it run on without breakpoints |
| `dbg`, `dbg pid N` | the debugged programs / which one the commands act on |

A program that **faults** while debugged stops there (`stopped (fault)`):
look with `bt` / `mem`, then `cont` ends it.

How it works: breakpoints are `int3` (x86), `brk #0` (ARM64) or `c.ebreak`
(RISC-V) written into the code page. A stopped program waits *in its own
task* inside the kernel, with its address space installed, and executes each
`dbg` request itself. Single step: the x86 trap flag, ARM64 software step;
RISC-V has none from S-mode, so a step puts temporary breakpoints on every
possible next instruction (fall-through and branch / jump targets).
Managed apps and scripts run inside `CLRHOST.ELF` and are not debugged at the
source level. The self-test boot drives a whole session -> `MILESTONE:
DEBUGGER OK`.

## 🧭 VS Code: breakpoints on a running device (`bz dap`)

The VS Code extension (`sdk/vscode-extension`) debugs programs **on a running
Buitenzorg device** -- QEMU or real hardware -- through `bz dap`, a Debug
Adapter Protocol server in the `bz` CLI (`sdk/bz/Dap.cs`) that talks to the
device over bz-link ([bz-link](bz-link.md)).

1. On the device: `deploy on` (shows the pairing code). On the PC: `bz device
   connect <ip> <code>` -- or put `"device"` and `"code"` in `launch.json`.
2. Open a `.js`, `.ts`, `.py` or `.csx` file, click in the gutter to set
   breakpoints, press **F5** (no `launch.json` needed: it debugs the active
   file).

| Program | What happens | You get |
|---|---|---|
| **Script** (`.js .ts .py .csx`) | uploaded to `/disk/<8.3 name>`, run once under the recording debugger (`script debug LANG FILE all`) | line breakpoints, continue, step over / in / out, **step back + reverse continue**, call stack, locals + globals, hover, output as it was printed, the runtime error as an exception stop |
| **Native ELF** (`"program": "crashme"`, or a local `.ELF` that is uploaded) | started with `dbg run` (stopped at its entry) | **function breakpoints** (symbol names, or a unique part), continue, step = one instruction, step out = to the caller, symbolized call stack, **registers** (editable), the Debug Console passes text to `dbg` (`mem 0x400000 32`, `bt`) |

```jsonc
// .vscode/launch.json
{ "type": "buitenzorg", "request": "launch", "name": "Debug on device",
  "program": "${file}",                 // or "crashme" (native, by name)
  "stopOnEntry": false,
  "device": "127.0.0.1:7070", "code": 4321 }   // optional: else ~/.bz/device.json
```

Under QEMU forward the port: `-netdev user,id=n0,hostfwd=udp::7070-:7070
-device e1000,netdev=n0`. The extension starts `dotnet dist/bz/bz.dll dap`
(or `sdk/bz/bin/Debug/net10.0/bz.dll`, or the setting `buitenzorg.bzPath`, or
`bz` on PATH).

**Honest limits.** A script is *replayed*: it runs once, then you move through
the recording -- you cannot change a value and continue, and only the first
400 statement stops are recorded. A UI script (one that opens a window)
runs in its window on the device while it records; the launch waits until
you close it (or the script ends), then the replay starts. Native programs have no line information (there is no DWARF on the
device), so their breakpoints are function names and their frames show
addresses, not source lines; pause is not supported (a program stops at a
breakpoint, a fault, or when it ends). Managed C# apps (RustCLR) are not
debugged at the source level yet. Tests: `bz dap --selftest` (a simulated
device, CI) and `bz dap --check` (the connected device; the smoke test's
`[bzlink]` step runs it on x86 with a script, a UI script and `crashme`). The
extension itself is not tested by a VS Code instance in CI -- the adapter is.

## 💥 Crash reports (ring-3 programs)

When a program faults (a bad pointer, an unmapped address, an undefined
instruction), the kernel ends just that program -- and writes a report: the
fault, pc / sp (and the link register on ARM64 / RISC-V), and a
**symbolized backtrace**, to the log and to `/ram/CRASH/P<pid>.TXT`
(`/disk/CRASH/` where there is no `/ram`).

```text
=== crash report: pid 1009 'crashme' ===
page fault (read), address 0x8
pc 0x401481  crashme_Crash__Level3+0x1
sp 0x7001ff90
backtrace (return addresses found on the stack, innermost first):
  #0  0x401481  crashme_Crash__Level3+0x1
  #1  0x4014a6  crashme_Crash__Level2+0xa
  #2  0x4014b4  crashme_Crash__Level1+0x8
  #3  0x4016a7  crashme__Module___StartupCodeMain+0x47
  #4  0x4000b2  _start+0x26
```

- In the Terminal: `crash` lists the reports, `crash PID` / `crash last`
  shows one; from a PC, `bz pull /ram/CRASH/P1009.TXT` or `bz logs`.
- **Symbols** come from the program's own ELF symbol table, kept when the
  program is loaded: native C# (bflat) names methods `assembly_Class__Method`,
  Rust symbols are demangled (`crashme::level3`). A stripped program gets
  addresses only.
- **The backtrace is a stack scan:** compiled C# and Rust code need not keep
  frame pointers, so the kernel looks for stack words that point into the
  program's code *right after a call instruction*. That finds the active
  calls; a stale return address in a dead stack slot can appear too.
- The self-test boot crashes `CRASHME.ELF` on purpose and requires the three
  levels in the report: `MILESTONE: CRASH REPORT OK`.

## Command summary

```powershell
# Debugger
.\scripts\debug-kernel.ps1                 # Windows  (Linux/macOS: debug-kernel.sh)

# Profiler (in the OS shell)
prof self

# App debugger (in the OS shell, or bz run "dbg ..." from a PC)
dbg run crashme   # then: break Level3, cont, bt, regs, step, kill, wait

# VS Code debug adapter (on the PC)
bz dap --selftest  # simulated device;  bz dap --check = the connected one

# Crash reports (in the OS shell)
crash            # list
crash last       # the newest report
```

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
