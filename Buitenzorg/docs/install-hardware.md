# Install & Boot on Real Hardware

Besides QEMU and VMs (VMware/VirtualBox/Hyper-V — see [run-in-vm.md](run-in-vm.md)),
Buitenzorg OS can **boot from USB on a physical machine**. The build produces two
raw disk images you can write directly to a USB stick:

| File | Firmware | Disk scheme |
|---|---|---|
| `dist/buitenzorg-bios.img` | Legacy BIOS / CSM | MBR |
| `dist/buitenzorg-uefi.img` | UEFI | GPT + ESP (FAT) |

**English** · [Bahasa Indonesia](install-hardware.id.md) · ← [Documentation index](README.md)

> ⚠️ **Status:** the USB boot path is fully prepared (scripts + read-back
> verification), but it has **not yet been verified on a physical machine** — so
> far Buitenzorg is tested automatically in QEMU (4 media) and in VMware/
> VirtualBox. Treat hardware boot as an **experiment**: use a machine and a USB
> stick you can afford to lose, and read [Compatibility &
> limitations](#-compatibility--limitations) below.

---

## 0. Prerequisites

1. **Build the OS first:**
   ```powershell
   .\scripts\build.ps1        # Windows   (Linux/macOS: ./scripts/build.sh)
   ```
   Afterwards `dist\buitenzorg-bios.img` and `dist\buitenzorg-uefi.img` exist.
2. **A USB stick** whose contents can be erased (the image is tiny, ~5 MB — any
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
4. Buitenzorg boots: ASCII logo → kernel log on screen → desktop.

If the USB doesn't appear in the boot menu: make sure **Secure Boot is OFF**, and
for the bios image enable **Legacy/CSM**; for the uefi image ensure **UEFI** mode
is active.

## 4. Verify on physical hardware (please report)

Because hardware boot is not yet validated, note the following if you try it — it
helps a lot toward marking it "tested":

- **Boot & framebuffer:** does the logo appear? Are the resolution and colors
  right? The bootloader requests a linear framebuffer; some GPUs/monitors may
  hand back a different mode.
- **PS/2 vs USB input:** the Buitenzorg keyboard/mouse drivers are **PS/2**.
  Modern laptops/PCs often only have **USB HID** (no USB driver yet) → input may
  be dead even though boot succeeds. A PS/2 port or adapter, or the BIOS
  **"USB Legacy / emulation"** option (which presents HID as PS/2), helps.
- **Storage:** the disk driver is **IDE/SATA PIO (ATA)** only. The app suite is on
  `/disk`, which needs it; native AHCI/NVMe are not implemented, so on an
  NVMe-only machine `/disk` may be unreadable (the kernel still boots).
- **Timer & interrupts:** legacy PIT + PIC 8259 (not APIC). Usually still
  supported via legacy emulation on modern chipsets.
- **ACPI shutdown:** the shell `shutdown` uses ACPI (the QEMU/VBox port fallback
  does not apply on hardware) — report whether it actually powers off.

The serial log is not automatically available on hardware as it is in QEMU; rely
on the screen (all kernel logs are also printed to the framebuffer console).

## 🧭 Compatibility & limitations

| Area | Current support | Notes |
|---|---|---|
| Firmware | UEFI **and** legacy BIOS | Secure Boot must be OFF (unsigned) |
| CPU | x86-64 | ARM64/RISC-V = v1.x "Rimba" |
| Graphics | linear framebuffer from the bootloader | no GPU driver/acceleration |
| Keyboard/Mouse | **PS/2** | no USB HID yet; use USB-legacy BIOS |
| Storage | IDE/SATA **PIO (ATA)** | no native AHCI/NVMe/USB-MSD yet |
| Interrupt/timer | PIC 8259 + PIT | no APIC yet (blocks SMP) |
| Networking | loopback only | no NIC driver (e1000) yet |
| SMP | single-core | no multi-core yet |

All the "not yet" items are in the backlog in **[PLAN.md](../PLAN.md)** (Technical
Debt) and tracked in **[Progress.md](../Progress.md)**.

## 🆘 Troubleshooting

- **USB not in the boot menu** → Secure Boot OFF; match the firmware
  (bios↔Legacy/CSM, uefi↔UEFI); try writing the **other** image; try a different
  USB port (sometimes only certain ports are bootable).
- **Black screen after selecting the USB** → usually a firmware mismatch (you
  wrote the bios image but booted in UEFI mode, or vice versa) — write the
  matching image.
- **Boots but the keyboard is dead** → see the PS/2 vs USB note above; enable
  **USB Legacy** in the BIOS.
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
