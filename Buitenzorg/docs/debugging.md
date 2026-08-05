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

## Command summary

```powershell
# Debugger
.\scripts\debug-kernel.ps1                 # Windows  (Linux/macOS: debug-kernel.sh)

# Profiler (in the OS shell)
prof self
```

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
