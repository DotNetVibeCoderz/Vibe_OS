# Testing the 2026-11-22 changes on real hardware

Everything below was tested in QEMU only. This is the short list that needs a
real laptop and a real Raspberry Pi 4: run it, photograph each step marked
**photo**, and send the photos (plus `BOOTLOG.TXT` if something is wrong).
Move the mouse before a photo: the screensaver starts after 12 idle seconds.

## Laptop

**Writing the stick erases it, including notes saved on it.** In PowerShell as
Administrator, from the repository root (the images in `dist/` are the ones to
test):

```powershell
.\scripts\flash-usb.ps1 -List
.\scripts\flash-usb.ps1 -DiskNumber N -Firmware uefi   # N from the list
```

Boot from the stick (UEFI) and open the Terminal.

| # | Do this | Expected |
|---|---|---|
| 1 | Move the touchpad / mouse right after the desktop appears | The pointer follows the first movement, no jump, no stray click |
| 2 | Look at the desktop for 10 s without touching it | Terminal, Welcome and Tips are solid, not see-through outlines (**photo**) |
| 3 | `gpu` | A line `vsync: pipe A frame counter, N vblanks in 100 ms` and the last line `display: vsync from Intel pipe A frame counter at 60.000 Hz ...`. If not, the `vsync:` line says why (**photo**) |
| 4 | Type the two lines under this table | `120 vsync: N ms`, N close to 2000 on a 60 Hz panel (**photo**) |
| 5 | `run crashme null`, wait for it to end, then `crash last` | Unhandled NullReferenceException, backtrace naming `Null3`, `Null2`, `Null1` (**photo**) |
| 6 | `run crashme throw`, then `crash last` | Backtrace naming `Throw2`, `Throw1` (**photo**) |
| 7 | `notify Tes halo`, then click the bell | A toast, then the action center lists it (**photo**) |
| 8 | Open and close Calculator a few times, double-click a title bar | Smooth open / close, nothing left half drawn, the window maximizes and the keys fill it; a second double-click restores it (**photo** maximized) |
| 9 | Pengaturan > Tampilan > Ukuran teks 125 %, look at all six pages | No label cut off, already on Tampilan without switching pages (**photo** of Tampilan) |
| 10 | Mulai > Permainan > DOOM | The icon is there, the game starts and takes keys, and plays sound effects + music (Pi: jack and HDMI; `dmesg pi-audio` shows `DMA stream ok`) (**photo**) |
| 11 | Mulai > Matikan, press Esc; again, press Enter | Esc cancels, Enter powers the laptop off |

```
echo 'let t = ticks(); let i = 0; while (i < 120) { vsync(); i = i + 1; } print("120 vsync: " + (ticks() - t) + " ms");' > /disk/V.JS
run /disk/V.JS
```

The resolution setting is not on the list: it works on QEMU / Bochs std VGA
only. A laptop keeps the resolution its firmware chose.

## Raspberry Pi 4

`bz pi update` replaces the kernel only, and steps 5, 6 and 9 also need new
files on the card. Pick one:

**A. Write the whole card (simplest; erases accounts, settings and files on it):**

```powershell
.\scripts\flash-usb.ps1 -List
.\scripts\flash-usb.ps1 -Image dist\buitenzorg-rpi4.img
```

**B. Over the network, keeping the card** (only if it already holds an image
from 2026-11-22). On the Pi type `deploy on 1234`, then on the PC:

```powershell
dotnet run --project sdk\bz -- device connect PI_ADDRESS 1234
dotnet run --project sdk\bz -- push userland\hello-csharp\arm64\crashme.elf /disk/CRASHME.ELF
dotnet run --project sdk\bz -- push userland\clr-apps\dist\MSETTING.DLL /disk/MSETTING.DLL
dotnet run --project sdk\bz -- pi update dist\kernel8.img --expect "first desktop frame on screen"
```

Then repeat steps 1, 2, 5, 6, 7, 8, 9 and 11 of the laptop table. Differences:

- `gpu` reports `virtual 60 Hz clock`: there is no vblank source on the Pi.
- Images built before 2026-11-24 carry no DOOM on the Pi, so the card answers
  with a notification that the game is not on this medium. Newer images have
  it (see docs/c-programs.md).
- Shut down halts the board; the red LED stays on.
