# Booting Buitenzorg from USB (English)

This document describes how to write a built image to a USB stick and boot it on real hardware. The repository includes scripts to flash images to USB with verification.

Important notes

- Booting on physical hardware remains experimental. Verify target machine firmware (BIOS vs UEFI) and secure boot settings.
- Always test in a VM first (QEMU, VMware, VirtualBox).

Flashing the USB

- Windows: .\scripts\flash-usb.ps1 <image> <target-drive>
- Linux/macOS: ./scripts/flash-usb.sh <image> <device>

The scripts perform multiple safety checks and optional verification steps. Follow prompts carefully and ensure you select the correct target device.

Troubleshooting

- If the machine does not boot: check the firmware boot order, disable secure boot if required, and confirm the image was flashed in the correct mode (MBR vs GPT/UEFI).