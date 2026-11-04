# Install & Boot on Real Hardware

Besides QEMU and VMs (VMware/VirtualBox/Hyper-V — see [run-in-vm.md](run-in-vm.md)),
Buitenzorg OS can **boot from USB on a physical machine**. The build produces two
raw disk images you can write directly to a USB stick:

| File | Firmware | Disk scheme |
|---|---|---|
| `dist/buitenzorg-bios.img` | Legacy BIOS / CSM | MBR |
| `dist/buitenzorg-uefi.img` | UEFI | GPT + ESP (FAT) |

**English** · [Bahasa Indonesia](install-hardware.id.md) · ← [Documentation index](README.md)

> ✅ **Status:** booted on a physical PC: a **Dell Latitude 7290** (UEFI, from a USB
> stick) reaches the desktop with its apps, wallpaper and icons, keyboard and touchpad,
> files saved to the stick, restart and shutdown. Other machines are still untested.
> The smoke test boots a QEMU machine built to resemble a modern PC (`[realpc]`:
> UEFI, no PS/2 controller, 2560x1440, booted from a USB stick on xHCI, an e1000,
> and a second FAT disk that must stay untouched). Real firmware and chips differ,
> so use a machine and a USB stick you can afford to lose. Buitenzorg writes only
> to its own volume (the one with the file `BZDISK.ID`), never to other disks.

---

## 0. Prerequisites

1. **Build the OS first:**
   ```powershell
   .\scripts\build.ps1        # Windows   (Linux/macOS: ./scripts/build.sh)
   ```
   Afterwards `dist\buitenzorg-bios.img` and `dist\buitenzorg-uefi.img` exist.
2. **A USB stick** whose contents can be erased (the image is about 20 MB — any
   stick is big enough).
3. **Pick the firmware** to match the target:
   - Older machine, or a *Legacy*/*CSM* option in the BIOS → use **bios**.
   - Modern UEFI machine → use **uefi**, and **turn off Secure Boot** (the
     Buitenzorg bootloader is not signed).

## 1. Write the image to USB (script)

**The script erases the whole target disk.** It is layered with safeguards: only
**USB/removable** disks are offered, the target is chosen **explicitly** (never
guessed), the **system/boot disk is refused outright**, the size + model are
shown with a typed confirmation, and the write is **verified by reading it back**.

### Windows (PowerShell **as Administrator**)

```powershell
# 1) List candidate USB disks (safe, read-only):
.\scripts\flash-usb.ps1 -List

# 2) Write (interactive — prompts for the disk number + a typed ERASE):
.\scripts\flash-usb.ps1

# or directly, e.g. physical disk 2, UEFI firmware:
.\scripts\flash-usb.ps1 -DiskNumber 2 -Firmware uefi
```

Writing needs raw disk access → an **elevated PowerShell is required**; the
script stops otherwise. A non-USB disk is refused unless you pass `-Force` (the
last safety net).

### Linux / macOS

```bash
# 1) List candidate devices:
./scripts/flash-usb.sh --list

# 2) Write (needs sudo for raw device access):
sudo ./scripts/flash-usb.sh /dev/sdX            # Linux, BIOS image
sudo ./scripts/flash-usb.sh /dev/sdX --uefi     # UEFI image
sudo ./scripts/flash-usb.sh /dev/rdiskN         # macOS: use the RAW rdiskN node
```

Target the **whole disk** (`/dev/sdb`), not a partition (`/dev/sdb1`). The script
refuses the root/system disk and unmounts any mounted partitions first.

## 2. Write the image to USB (GUI tool — recommended for beginners)

If you'd rather not use the script, use a well-known flashing tool — these also
list the disks safely:

- **[balenaEtcher](https://etcher.balena.io/)** (Windows/macOS/Linux) — pick
  `buitenzorg-bios.img` **or** `buitenzorg-uefi.img`, pick the USB, *Flash*.
- **[Rufus](https://rufus.ie/)** (Windows) — *Boot selection* → pick the image,
  choose **DD image** mode when asked.
- **`dd`** manually (Linux/macOS), no script:
  ```bash
  sudo dd if=dist/buitenzorg-bios.img of=/dev/sdX bs=4M conv=fsync status=progress
  sync
  ```

> These images are bootable as-is (not a hybrid ISO, but a full MBR/GPT disk), so
> **DD/raw** mode — not "ISO" mode — is the correct choice in Rufus.

## 3. Boot the machine from USB

1. Plug in the USB, power on / restart.
2. Enter the **boot menu** (usually `F12`, `F10`, `F9`, `Esc`, or `F2` → Boot
   order — depends on the vendor).
3. Pick the USB entry:
   - the **uefi** image shows as **"UEFI: <USB name>"**,
   - the **bios** image shows as a plain USB (non-UEFI).
4. Buitenzorg boots straight to the desktop.

If the USB doesn't appear in the boot menu: make sure **Secure Boot is OFF**, and
for the bios image enable **Legacy/CSM**; for the uefi image ensure **UEFI** mode
is active.

## 4. Check it on the machine, and report back

Go through these on the desktop, then send the results with the log file:

1. **Desktop:** does it appear, with the right colours and the wallpaper? Are the
   app icons there? (They come from the stick.)
2. **Input:** do the USB keyboard and mouse work? Open the Terminal and type `ver`.
3. **Storage:** in the Terminal, `ls /disk`. (`BOOTLOG.TXT` names the disk: `mounted /disk ... on`.)
4. **Network:** with an Intel e1000-family card and a cable, `ipconfig`, then
   `ping <router address>`.
5. **Apps:** start Calculator and Notes from the Start menu; save a note and read
   it back.
6. **Power:** `restart`, then `shutdown`. Does the machine really turn off?

**The log:** every boot saves it as `BOOTLOG.TXT` on the stick. Read it on another
computer (with the **bios** image the stick's FAT partition opens normally; with
the **uefi** image it is the ESP). If the machine stops with a **PANIC** screen,
take a photo of it: the panic is not in that file.

## 🧭 Compatibility & limitations

| Area | Current support | Notes |
|---|---|---|
| Firmware | UEFI **and** legacy BIOS | Secure Boot must be OFF (unsigned) |
| CPU | x86-64, up to 16 cores used | ARM64: [Raspberry Pi 4](install-raspberry-pi.md); RISC-V under QEMU |
| Graphics | the firmware's framebuffer | no GPU driver on PCs: drawn by the CPU. Tested up to 2560x1440; BIOS image 1280x720 |
| Keyboard/mouse | USB (xHCI: HID boot protocol) and PS/2 | no USB 2.0-only (EHCI) controllers; **no hot-plug**: plug in before booting |
| Storage | IDE, SATA (AHCI), NVMe, USB sticks on xHCI | 512-byte sectors; no RAID/Intel RST mode (set SATA to AHCI); a USB 3 hub is not supported |
| Network | Intel e1000 family (82540, 82545, 82574, ...), virtio-net | **no** on-board Intel I217/I219, Realtek or Wi-Fi yet |
| Sound | Intel HD Audio, AC'97, virtio-sound | HD Audio: output only (speaker/headphone/line out), first codec found |
| File system | FAT12/16/32 | no journal: use `shutdown` |

## 🆘 Troubleshooting

- **USB not in the boot menu** → Secure Boot OFF; match the firmware
  (bios↔Legacy/CSM, uefi↔UEFI); try writing the **other** image; try a different
  USB port (sometimes only certain ports are bootable).
- **Black screen, or the text stops** → the screen shows the boot log on real
  hardware: photograph the last lines and report them. If nothing at all appears,
  say whether the firmware logo disappeared.
- **Black screen after selecting the USB** → usually a firmware mismatch (you
  wrote the bios image but booted in UEFI mode, or vice versa) — write the
  matching image.
- **Boots but the keyboard is dead** → plug it in before booting, into a port on
  an xHCI controller (a blue USB 3 port is one; the log says `xHCI running`).
- **Desktop without icons or wallpaper** → `/disk` was not found: see
  `[usb]`/`[ahci]`/`[nvme]` lines. Controllers in RAID/RST mode are not supported.
- **`flash-usb.ps1` says it needs Administrator** → open PowerShell via
  right-click → *Run as administrator*.
- **`flash-usb.ps1` refuses the disk** → it isn't a USB/removable disk (or it is
  the system disk). Confirm the disk number with `-List`; this is a deliberate
  safeguard.
- **Verification FAILED** after writing → do **not** boot from that USB; re-flash
  (the stick may be worn/counterfeit — try another one).
- **Restore the USB to normal** → reformat it: Windows `diskpart` → `clean` →
  create a new partition; Linux/macOS `sudo wipefs -a /dev/sdX`, then partition.

## Command summary

```powershell
# Windows
.\scripts\build.ps1
.\scripts\flash-usb.ps1 -List
.\scripts\flash-usb.ps1 -DiskNumber <N> -Firmware uefi   # or -Firmware bios
```
```bash
# Linux / macOS
./scripts/build.sh
./scripts/flash-usb.sh --list
sudo ./scripts/flash-usb.sh /dev/sdX --uefi              # or --bios
```

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
