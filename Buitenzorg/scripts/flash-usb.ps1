# Buitenzorg OS - write a bootable image to a USB drive (Windows).
#
# The build produces two raw, directly-writable disk images:
#   dist/buitenzorg-bios.img   MBR, for legacy-BIOS / CSM boot
#   dist/buitenzorg-uefi.img   GPT + FAT ESP, for UEFI boot
# This script writes one of them byte-for-byte onto a physical USB disk, so the
# machine can boot Buitenzorg from that stick. See docs/install-hardware.md.
#
# *** THIS ERASES THE TARGET DISK. *** Guardrails:
#   - only USB / removable disks are offered (override with -Force for the rest)
#   - the target is chosen by disk number, never guessed
#   - size + model are shown and typed confirmation is required
#   - the write is verified by reading the disk back and comparing
#
# Usage:
#   .\scripts\flash-usb.ps1                 # interactive: list disks, then pick
#   .\scripts\flash-usb.ps1 -DiskNumber 2   # write BIOS image to physical disk 2
#   .\scripts\flash-usb.ps1 -DiskNumber 2 -Firmware uefi
#   .\scripts\flash-usb.ps1 -List           # just list candidate disks and exit
#
# Must run in an ELEVATED PowerShell (raw disk access needs Administrator).

[CmdletBinding()]
param(
    [int]    $DiskNumber = -1,
    [ValidateSet("bios", "uefi")]
    [string] $Firmware = "bios",
    [string] $Image,
    [switch] $List,
    [switch] $Force,      # allow a non-USB / non-removable target (dangerous)
    [switch] $Yes         # skip the typed confirmation (for automation)
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root "dist"

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    $p  = New-Object Security.Principal.WindowsPrincipal($id)
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-CandidateDisks {
    # Removable / USB disks only; these are what a user flashes.
    Get-Disk | Where-Object { $_.BusType -eq "USB" -or $_.IsRemovable } |
        Sort-Object Number
}

function Show-Disks {
    param($disks)
    if (-not $disks) {
        Write-Host "No USB / removable disks found. Plug in a stick, or use -Force to target a fixed disk (dangerous)." -ForegroundColor Yellow
        return
    }
    Write-Host ""
    Write-Host "Candidate disks:" -ForegroundColor Cyan
    $disks | ForEach-Object {
        $gb = [math]::Round($_.Size / 1GB, 1)
        "{0,3}  {1,-28} {2,7} GB  {3}" -f $_.Number, $_.FriendlyName, $gb, $_.BusType
    } | Write-Host
    Write-Host ""
}

if (-not (Test-Admin)) {
    throw "Administrator required. Re-open PowerShell as Administrator and run this again."
}

# Resolve the image path.
if (-not $Image) {
    $Image = Join-Path $dist "buitenzorg-$Firmware.img"
}
if (-not (Test-Path $Image)) {
    throw "Image not found: $Image`nBuild it first (.\scripts\build.ps1 or quickstart), or pass -Image."
}

$disks = Get-CandidateDisks
if ($List) { Show-Disks $disks; return }

# Choose the target disk.
if ($DiskNumber -lt 0) {
    Show-Disks $disks
    if (-not $disks) { return }
    $answer = Read-Host "Enter the disk number to WRITE (or blank to cancel)"
    if ([string]::IsNullOrWhiteSpace($answer)) { Write-Host "Cancelled."; return }
    $DiskNumber = [int]$answer
}

$disk = Get-Disk -Number $DiskNumber -ErrorAction SilentlyContinue
if (-not $disk) { throw "No physical disk with number $DiskNumber." }

# Safety: refuse a fixed / non-USB disk unless -Force is given.
$isRemovable = ($disk.BusType -eq "USB") -or $disk.IsRemovable
if (-not $isRemovable -and -not $Force) {
    throw "Disk $DiskNumber ($($disk.FriendlyName)) is not a USB/removable disk. Refusing without -Force. This is your safety net against erasing the wrong drive."
}
if ($disk.IsBoot -or $disk.IsSystem) {
    throw "Disk $DiskNumber is the SYSTEM/BOOT disk. Refusing outright - this would destroy Windows."
}

$sizeGb  = [math]::Round($disk.Size / 1GB, 1)
$imgSize = (Get-Item $Image).Length
Write-Host ""
Write-Host "About to ERASE and write:" -ForegroundColor Red
Write-Host "  Target : disk $DiskNumber  $($disk.FriendlyName)  $sizeGb GB  ($($disk.BusType))"
Write-Host "  Image  : $Image  ($([math]::Round($imgSize/1MB,2)) MB, $Firmware firmware)"
Write-Host ""

if (-not $Yes) {
    $confirm = Read-Host "Type ERASE to continue"
    if ($confirm -ne "ERASE") { Write-Host "Cancelled."; return }
}

# Take the disk offline / clear it so no volume holds a lock during the write.
Write-Host "==> Clearing partition table on disk $DiskNumber..." -ForegroundColor Cyan
try {
    Clear-Disk -Number $DiskNumber -RemoveData -RemoveOEM -Confirm:$false -ErrorAction Stop
} catch {
    Write-Host "    (Clear-Disk: $($_.Exception.Message) - continuing to raw write)" -ForegroundColor DarkYellow
}

# Raw write to \\.\PhysicalDriveN. The image is smaller than the disk; only the
# leading bytes are written, the rest is left as-is (the MBR/GPT describes only
# what Buitenzorg needs).
$devPath = "\\.\PhysicalDrive$DiskNumber"
Write-Host "==> Writing image to $devPath ..." -ForegroundColor Cyan

$src = $null; $dst = $null
try {
    $src = [System.IO.File]::OpenRead($Image)
    # FileShare.None takes an exclusive lock on the raw device for the write.
    $dst = New-Object System.IO.FileStream($devPath, [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)

    $bufSize = 1MB
    $buffer  = New-Object byte[] $bufSize
    $written = 0L
    while (($read = $src.Read($buffer, 0, $bufSize)) -gt 0) {
        $dst.Write($buffer, 0, $read)
        $written += $read
        $pct = [math]::Round(($written / $imgSize) * 100)
        Write-Progress -Activity "Writing Buitenzorg to disk $DiskNumber" -Status "$([math]::Round($written/1MB,1)) MB" -PercentComplete $pct
    }
    $dst.Flush()
    Write-Progress -Activity "Writing" -Completed
} finally {
    if ($dst) { $dst.Dispose() }
    if ($src) { $src.Dispose() }
}
Write-Host "    wrote $([math]::Round($written/1MB,2)) MB" -ForegroundColor Green

# Verify by reading the written region back and comparing.
Write-Host "==> Verifying..." -ForegroundColor Cyan
$ok = $true
$src = $null; $dev = $null
try {
    $src = [System.IO.File]::OpenRead($Image)
    $dev = New-Object System.IO.FileStream($devPath, [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    $a = New-Object byte[] (1MB)
    $b = New-Object byte[] (1MB)
    while (($n = $src.Read($a, 0, $a.Length)) -gt 0) {
        $off = 0
        while ($off -lt $n) {
            $r = $dev.Read($b, $off, $n - $off)
            if ($r -le 0) { break }
            $off += $r
        }
        for ($i = 0; $i -lt $n; $i++) {
            if ($a[$i] -ne $b[$i]) { $ok = $false; break }
        }
        if (-not $ok) { break }
    }
} finally {
    if ($dev) { $dev.Dispose() }
    if ($src) { $src.Dispose() }
}

if ($ok) {
    Write-Host ""
    Write-Host "DONE - disk $DiskNumber now boots Buitenzorg OS ($Firmware)." -ForegroundColor Green
    Write-Host "Eject it, then boot the target machine from USB. See docs/install-hardware.md." -ForegroundColor Green
} else {
    throw "Verification FAILED - the data read back does not match the image. Do not boot from this disk; re-flash."
}
