# Run in a VM — VMware, VirtualBox & Hyper-V

Besides QEMU, the Buitenzorg image runs in **VMware Workstation Player**,
**Oracle VirtualBox**, and **Microsoft Hyper-V**. The build produces a raw disk
(`dist/buitenzorg-bios.img`, MBR/BIOS); the script below converts it to each
hypervisor's format.

Prerequisite: you have built the OS (`.\scripts\build.ps1` / `./scripts/build.sh`
or quickstart). Conversion uses **`qemu-img`** (part of QEMU).

**English** · [Bahasa Indonesia](run-in-vm.id.md) · ← [Documentation index](README.md)

---

## Build the VM images (one command)

```powershell
.\scripts\make-vm-images.ps1     # Linux/macOS: ./scripts/make-vm-images.sh
```

Produces in `dist/`:

| File | For |
|---|---|
| `buitenzorg.vmdk` | VMware (Player / Workstation / Fusion) |
| `buitenzorg.vdi`  | Oracle VirtualBox |
| `buitenzorg.vhdx` | Microsoft Hyper-V (Generation 1 / BIOS) |
| `Buitenzorg.vmx`  | a ready-to-open VMware VM config |

If `VBoxManage` is on PATH, the script also **registers a VirtualBox VM** named
`Buitenzorg` automatically.

> Manual conversion (if needed):
> ```
> qemu-img convert -f raw -O vmdk dist/buitenzorg-bios.img dist/buitenzorg.vmdk
> qemu-img convert -f raw -O vdi  dist/buitenzorg-bios.img dist/buitenzorg.vdi
> ```

## 🟦 VMware Workstation Player

1. Run `make-vm-images` (produces `dist/Buitenzorg.vmx` + `.vmdk`).
2. VMware Player → **Open a Virtual Machine** → pick `dist/Buitenzorg.vmx`.
3. Click **Play**.

The VM is configured with **BIOS** firmware, **512 MB** RAM, the `.vmdk` as an
IDE disk, and a sound card (ES1371). Serial is logged to `buitenzorg-serial.log`
in the VM folder.

> To build it manually: New VM → *I will install the OS later* → Guest OS
> **Other** → remove the default disk, **Add → Hard Disk → Use an existing disk**
> → `buitenzorg.vmdk` (IDE) → ensure firmware is **BIOS** (not UEFI).

## 🟧 Oracle VirtualBox

**Automatic** (if `VBoxManage` is on PATH, `make-vm-images` already did it):
```
VBoxManage startvm Buitenzorg
```

**Manual (GUI):**
1. **New** → Name `Buitenzorg`, Type **Other**, Version **Other/Unknown**.
2. Memory **512 MB**.
3. **Use an existing virtual hard disk file** → pick `dist/buitenzorg.vdi`.
4. Once created: **Settings → System → Motherboard** → ensure **BIOS** (disable
   *Enable EFI*). Optional **Audio** → **ICH AC97**.
5. **Start**.

**Manual (CLI):**
```
VBoxManage createvm --name Buitenzorg --ostype Other --register
VBoxManage modifyvm Buitenzorg --memory 512 --firmware bios --audiocontroller ac97
VBoxManage storagectl Buitenzorg --name IDE --add ide
VBoxManage storageattach Buitenzorg --storagectl IDE --port 0 --device 0 --type hdd --medium dist/buitenzorg.vdi
VBoxManage startvm Buitenzorg
```

## 🟦 Microsoft Hyper-V

Buitenzorg boots via **MBR on an IDE disk**, so use **Generation 1** (BIOS).
Generation 2 is UEFI + SCSI + Secure Boot, and the Buitenzorg bootloader is not
signed — do not use Gen 2.

**Automatic** — from an **elevated PowerShell** with Hyper-V enabled:
```powershell
.\scripts\make-vm-images.ps1        # produces dist\buitenzorg.vhdx
.\scripts\make-hyperv-vm.ps1        # create a Gen-1 VM "Buitenzorg" from the VHDX
.\scripts\make-hyperv-vm.ps1 -Start # create, then power it on
```
The script detects Hyper-V; if it is not enabled it prints the manual steps
instead of failing. Options: `-Name`, `-MemoryMB`, `-Switch`, `-Force` (replace
an existing VM).

**Manual (Hyper-V Manager):**
1. **New → Virtual Machine…**
2. **Generation 1** (REQUIRED — BIOS/MBR).
3. Memory **512 MB**.
4. **Connect Virtual Hard Disk → Use an existing virtual hard disk** →
   `dist\buitenzorg.vhdx`.
5. **Finish**, then **Start** + **Connect**.

**Manual (PowerShell):**
```powershell
New-VM -Name Buitenzorg -Generation 1 -MemoryStartupBytes 512MB -VHDPath dist\buitenzorg.vhdx
Set-VMProcessor -VMName Buitenzorg -Count 1
Start-VM Buitenzorg
vmconnect localhost Buitenzorg
```

> **VHDX note:** `make-vm-images` produces a **dynamic 64 MiB** VHDX (whole-MiB,
> so Hyper-V accepts it — a bare `-O vhdx` conversion yields an odd 5.47 MiB disk
> Hyper-V can reject, and this qemu build cannot `resize` a vhdx). The extra
> space is unused — the MBR only describes the sectors the OS needs. Networking
> is optional (the OS stack is loopback-only), so a VM without a NIC still boots.

## ❓ Notes & troubleshooting

- **Must be BIOS, not UEFI** — the `-bios.img` image boots via MBR. (For UEFI in
  QEMU there is `buitenzorg-uefi.img`; the VMware/VirtualBox examples above use
  the most portable BIOS path.)
- **A black screen for a few seconds** — the kernel renders to the framebuffer;
  wait for the desktop. The full log is on serial (the VMware/VBox log file).
- **Won't boot / "no bootable medium"** — make sure the disk is attached as
  **IDE** and the firmware is **BIOS**.
- **Audio** — the AC'97 driver; select the **AC97 / ICH AC97** controller in the
  VM (optional). Hyper-V has no audio emulation — audio being off there is normal.
- **Hyper-V VM won't boot / "boot failure"** — make sure it is **Generation 1**
  (not Gen 2); Gen 2 (UEFI) will not boot this MBR disk.
- **Hyper-V "The system cannot find the file" / format rejected** — use the VHDX
  from `make-vm-images` (64 MiB whole-MiB), not a bare `qemu-img -O vhdx` result
  (5.47 MiB, which Hyper-V can reject).
- **Regenerate** — after rebuilding the OS, run `make-vm-images` again to refresh
  the `.vmdk` / `.vdi` / `.vhdx`.

---

Want to boot a **physical machine** (not a VM)? See
**[Install on Hardware](install-hardware.md)** — write the image to USB with
`scripts/flash-usb.ps1` / `.sh`.

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
