# Trying new kernels on a Raspberry Pi 4 without touching the SD card

Putting every new kernel on the card by hand (power off, card out, copy
`kernel8.img`, card back in) is the slow part of working on the Pi. `bz pi
update` does it over the network instead, and it can never leave the board
unreachable:

```
bz device connect 192.168.68.109 1234      # once: the board's IP and pairing code
bz pi update dist/kernel8.img --expect "first desktop frame on screen" --run "lsusb"
```

What happens:

1. The kernel is uploaded to the card as `/disk/KERNTRY.IMG` (bz-link).
2. `pi tryboot` on the board writes `TRYBOOT.TXT` (a copy of `config.txt`
   with `kernel=KERNTRY.IMG` and `cmdline=CMDTRY.TXT`) and `CMDTRY.TXT`
   (`bz.tryboot`), sets the firmware's **tryboot** reboot flag and restarts.
   The firmware reads `tryboot.txt` for that one boot only.
3. The new kernel sees `bz.tryboot` and arms the SoC's **PM watchdog** (15 s)
   before anything else; the desktop loop keeps feeding it. A kernel that
   hangs -- during the boot or later -- is reset by the watchdog, the
   tryboot flag is gone, and the board starts the old kernel.
4. `bz` waits until the board answers again. If it answers with the old
   kernel, the update failed. Otherwise it checks the new kernel: every
   `--expect` text must be in the board's log, every `--run` command must
   succeed.
5. All checks passed: `pi promote` stops the watchdog and copies
   `KERNTRY.IMG` over `KERNEL8.IMG` (read back before it counts). A check
   failed (or `--no-promote`): the board is restarted into the old kernel.

On the board itself: `pi` shows whether this is a trial boot and which
kernel files are on the card, `pi tryboot` / `pi promote` do steps 2 and 5 by
hand, and `pi watchdog-test` proves the reset path (arms the watchdog for
3 s and hangs: the board must restart).

## What the board needs

* The Buitenzorg SD card (direct route, `kernel8.img`), the Ethernet cable,
  and bz-link switched on: `deploy on 1234` in the Terminal (remembered in
  `/disk/DEPLOY.CFG`, so it is on after every boot).
* A Pi 4 bootloader EEPROM from 2021 or later (the tryboot flag). An older
  one makes `pi tryboot` report that the firmware did not take the flag;
  update it with Raspberry Pi Imager's "Bootloader" image.

## Continuous integration on the board

`.github/workflows/pi.yml` runs this on a self-hosted GitHub Actions runner
with the label `rpi4`: a PC on the same network as the board, with Rust,
.NET and the repository's toolchain. It builds `kernel8.img` and runs `bz pi
update` with a few checks. Set the repository secrets `BZ_DEVICE` (the board's
IP) and `BZ_CODE` (its pairing code), then start the workflow by hand
(Actions > Raspberry Pi 4 (board) > Run workflow). No relay is needed: a
kernel that hangs is undone by the watchdog.

What is verified where: under QEMU `raspi4b` the trial boot (watchdog armed
and fed), `pi tryboot`'s files, `pi promote` and the watchdog reset itself.
QEMU's firmware model does not take the tryboot flag and has no network, so
the full round trip runs only on a board.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
