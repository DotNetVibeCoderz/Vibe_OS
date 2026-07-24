#!/usr/bin/env bash
# Buitenzorg OS — produce VM disk images for VMware, VirtualBox & Hyper-V.
#
# Converts the raw BIOS disk image (dist/buitenzorg-bios.img) into:
#   dist/buitenzorg.vmdk   — VMware (Player/Workstation/Fusion)
#   dist/buitenzorg.vdi    — Oracle VirtualBox
#   dist/buitenzorg.vhdx   — Microsoft Hyper-V (Generation 1 / BIOS)
# and writes a VMware config (dist/Buitenzorg.vmx). If VBoxManage is present it
# also registers a VirtualBox VM. The VHDX can be copied to a Hyper-V host and
# attached to a Gen-1 VM (see docs/run-in-vm.md / scripts/make-hyperv-vm.ps1).
#
# Usage:  ./scripts/make-vm-images.sh    (run ./scripts/build.sh first)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/dist"
img="$dist/buitenzorg-bios.img"
[ -f "$img" ] || { echo "dist/buitenzorg-bios.img not found — build first (./scripts/build.sh or quickstart)."; exit 1; }

qi="${QEMU_IMG:-qemu-img}"
command -v "$qi" >/dev/null 2>&1 || { echo "qemu-img not found; install QEMU or set QEMU_IMG."; exit 1; }

vmdk="$dist/buitenzorg.vmdk"; vdi="$dist/buitenzorg.vdi"; vhdx="$dist/buitenzorg.vhdx"
echo "==> Converting to VMware VMDK..."
"$qi" convert -f raw -O vmdk "$img" "$vmdk"
echo "==> Converting to VirtualBox VDI..."
"$qi" convert -f raw -O vdi  "$img" "$vdi"

# Hyper-V VHDX. Hyper-V wants a whole-MiB virtual size (a bare `convert -O vhdx`
# yields an odd 5.47 MiB disk Hyper-V may reject, and vhdx can't be resized), so
# pre-create a clean 64 MiB dynamic VHDX and stream the raw into it with -n. The
# extra space is unused — the MBR only describes the sectors Buitenzorg needs.
echo "==> Converting to Hyper-V VHDX (64 MiB, Gen 1 / BIOS)..."
rm -f "$vhdx"
"$qi" create -f vhdx "$vhdx" 64M >/dev/null
"$qi" convert -n -f raw -O vhdx "$img" "$vhdx"

cat > "$dist/Buitenzorg.vmx" <<'EOF'
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "19"
displayName = "Buitenzorg OS"
guestOS = "other"
firmware = "bios"
memsize = "512"
numvcpus = "1"
ide0:0.present = "TRUE"
ide0:0.deviceType = "disk"
ide0:0.fileName = "buitenzorg.vmdk"
sound.present = "TRUE"
sound.autoDetect = "TRUE"
sound.virtualDev = "es1371"
svga.present = "TRUE"
serial0.present = "TRUE"
serial0.fileType = "file"
serial0.fileName = "buitenzorg-serial.log"
EOF
echo "  ok: $vmdk"
echo "  ok: $vdi"
echo "  ok: $vhdx"
echo "  ok: $dist/Buitenzorg.vmx"

if command -v VBoxManage >/dev/null 2>&1; then
  echo "==> Registering a VirtualBox VM (Buitenzorg)..."
  VBoxManage unregistervm "Buitenzorg" --delete >/dev/null 2>&1 || true
  VBoxManage createvm --name "Buitenzorg" --ostype "Other" --register
  VBoxManage modifyvm  "Buitenzorg" --memory 512 --firmware bios --audio-enabled on --audio-driver none --audiocontroller ac97
  VBoxManage storagectl "Buitenzorg" --name "IDE" --add ide
  VBoxManage storageattach "Buitenzorg" --storagectl "IDE" --port 0 --device 0 --type hdd --medium "$vdi"
  echo "  ok: VM 'Buitenzorg' registered — start with:  VBoxManage startvm Buitenzorg"
else
  echo "==> VirtualBox: create an 'Other/Unknown' VM (BIOS, 512 MB) and attach"
  echo "    dist/buitenzorg.vdi as an IDE hard disk. See docs/run-in-vm.md."
fi

echo ""
echo "VMware Player: File > Open… > dist/Buitenzorg.vmx, then Play."
echo "Hyper-V:       copy dist/buitenzorg.vhdx to a Windows host, then run"
echo "               scripts/make-hyperv-vm.ps1 (elevated) or attach it to a Gen-1 VM."
