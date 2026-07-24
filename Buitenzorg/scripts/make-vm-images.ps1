# Buitenzorg OS - produce VM disk images for VMware, VirtualBox & Hyper-V (Windows).
#
# Converts the raw BIOS disk image (dist/buitenzorg-bios.img) into:
#   dist/buitenzorg.vmdk   - VMware (Player/Workstation)
#   dist/buitenzorg.vdi    - Oracle VirtualBox
#   dist/buitenzorg.vhdx   - Microsoft Hyper-V (Generation 1 / BIOS)
# and writes a ready-to-open VMware config (dist/Buitenzorg.vmx). If VBoxManage
# is on PATH it also registers a VirtualBox VM. To create the Hyper-V VM, run
# scripts/make-hyperv-vm.ps1 (needs Hyper-V + admin). See docs/run-in-vm.md.
#
# Usage:  .\scripts\make-vm-images.ps1   (run .\scripts\build.ps1 first)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root "dist"
$img  = Join-Path $dist "buitenzorg-bios.img"
if (-not (Test-Path $img)) { throw "dist\buitenzorg-bios.img not found - build first with .\scripts\build.ps1 (or quickstart)." }

# Locate qemu-img (env override, PATH, or the default Windows install).
$qi = $env:QEMU_IMG
if (-not $qi) { $c = Get-Command qemu-img -ErrorAction SilentlyContinue; if ($c) { $qi = $c.Source } }
if (-not $qi) { $qi = "C:\Program Files\qemu\qemu-img.exe" }
if (-not (Test-Path $qi)) { throw "qemu-img not found; install QEMU or set the QEMU_IMG env var." }

$vmdk = Join-Path $dist "buitenzorg.vmdk"
$vdi  = Join-Path $dist "buitenzorg.vdi"
$vhdx = Join-Path $dist "buitenzorg.vhdx"
Write-Host "==> Converting to VMware VMDK..." -ForegroundColor Cyan
& $qi convert -f raw -O vmdk $img $vmdk
Write-Host "==> Converting to VirtualBox VDI..." -ForegroundColor Cyan
& $qi convert -f raw -O vdi  $img $vdi

# Hyper-V VHDX. Hyper-V wants the virtual size to be a whole number of MiB, and
# a bare `convert -O vhdx` yields an odd 5.47 MiB disk that Hyper-V may reject
# (and this qemu build cannot `resize` a vhdx). So pre-create a clean 64 MiB
# dynamic VHDX and stream the raw image into it with `-n` (no target create).
# The extra space is unused - the MBR only describes the sectors Buitenzorg needs.
Write-Host "==> Converting to Hyper-V VHDX (64 MiB, Gen 1 / BIOS)..." -ForegroundColor Cyan
if (Test-Path $vhdx) { Remove-Item $vhdx -Force }
& $qi create -f vhdx $vhdx 64M | Out-Null
& $qi convert -n -f raw -O vhdx $img $vhdx

# VMware config (.vmx): boot the vmdk as an IDE disk with legacy BIOS firmware.
$vmx = @"
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
"@
$vmxPath = Join-Path $dist "Buitenzorg.vmx"
Set-Content -Path $vmxPath -Value $vmx -Encoding ASCII
Write-Host "  ok: $vmdk" -ForegroundColor Green
Write-Host "  ok: $vdi" -ForegroundColor Green
Write-Host "  ok: $vhdx" -ForegroundColor Green
Write-Host "  ok: $vmxPath" -ForegroundColor Green

# VirtualBox: register a VM automatically if the CLI is available.
$vbox = Get-Command VBoxManage -ErrorAction SilentlyContinue
if ($vbox) {
    Write-Host "==> Registering a VirtualBox VM (Buitenzorg)..." -ForegroundColor Cyan
    & VBoxManage unregistervm "Buitenzorg" --delete 2>$null | Out-Null
    & VBoxManage createvm --name "Buitenzorg" --ostype "Other" --register
    & VBoxManage modifyvm  "Buitenzorg" --memory 512 --firmware bios --audio-enabled on --audio-driver none --audiocontroller ac97
    & VBoxManage storagectl "Buitenzorg" --name "IDE" --add ide
    & VBoxManage storageattach "Buitenzorg" --storagectl "IDE" --port 0 --device 0 --type hdd --medium $vdi
    Write-Host "  ok: VM 'Buitenzorg' registered - start it from the VirtualBox UI or:" -ForegroundColor Green
    Write-Host "      VBoxManage startvm Buitenzorg" -ForegroundColor Green
} else {
    Write-Host "==> VirtualBox: open the app, create an 'Other/Unknown' VM (BIOS, 512 MB)," -ForegroundColor Yellow
    Write-Host "    and attach dist\buitenzorg.vdi as an IDE hard disk. See docs/run-in-vm.md." -ForegroundColor Yellow
}

# Hyper-V: creating the VM needs the Hyper-V role + admin, so it is a separate
# opt-in step rather than run here.
Write-Host ""
Write-Host "VMware Player: File > Open... > dist\Buitenzorg.vmx, then Play." -ForegroundColor Cyan
Write-Host "Hyper-V:       .\scripts\make-hyperv-vm.ps1   (elevated; creates a Gen-1 VM from the VHDX)" -ForegroundColor Cyan
