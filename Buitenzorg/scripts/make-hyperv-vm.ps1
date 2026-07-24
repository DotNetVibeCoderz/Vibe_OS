# Buitenzorg OS - create a Hyper-V VM from the VHDX (Windows).
#
# Registers a Generation 1 (BIOS) Hyper-V VM named "Buitenzorg" backed by
# dist/buitenzorg.vhdx, matching the BIOS/MBR disk the build produces. Gen 1 is
# used because Buitenzorg boots via MBR on an IDE disk (Gen 2 is UEFI + SCSI +
# Secure Boot, which the bootloader is not signed for).
#
# Requires: the Hyper-V role enabled (so the Hyper-V PowerShell module exists)
# and an ELEVATED shell. If Hyper-V is unavailable the script prints the manual
# steps instead of failing.
#
# Usage (elevated PowerShell):
#   .\scripts\make-hyperv-vm.ps1                 # create the VM (default switch)
#   .\scripts\make-hyperv-vm.ps1 -Switch "External"
#   .\scripts\make-hyperv-vm.ps1 -Start          # create, then power it on
#
# Run .\scripts\make-vm-images.ps1 first to produce the VHDX.

[CmdletBinding()]
param(
    [string] $Name   = "Buitenzorg",
    [int]    $MemoryMB = 512,
    [string] $Switch = "Default Switch",
    [switch] $Start,
    [switch] $Force   # remove an existing VM of the same name first
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$vhdx = Join-Path $root "dist\buitenzorg.vhdx"

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-Path $vhdx)) {
    throw "VHDX not found: $vhdx`nRun .\scripts\make-vm-images.ps1 first (after .\scripts\build.ps1)."
}

# Manual fallback if Hyper-V is not present.
function Show-Manual {
    Write-Host ""
    Write-Host "Hyper-V is not available here. To create the VM manually:" -ForegroundColor Yellow
    Write-Host "  1. Enable Hyper-V (Windows Features), reboot." -ForegroundColor Yellow
    Write-Host "  2. In Hyper-V Manager: New > Virtual Machine..." -ForegroundColor Yellow
    Write-Host "  3. Generation: 1  (BIOS/MBR - required)." -ForegroundColor Yellow
    Write-Host "  4. Memory: ${MemoryMB} MB." -ForegroundColor Yellow
    Write-Host "  5. Connect an existing virtual hard disk:" -ForegroundColor Yellow
    Write-Host "       $vhdx" -ForegroundColor Yellow
    Write-Host "  6. Finish, then Start + Connect." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Or, from an elevated PowerShell with Hyper-V enabled:" -ForegroundColor DarkGray
    Write-Host "     New-VM -Name '$Name' -Generation 1 -MemoryStartupBytes ${MemoryMB}MB -VHDPath '$vhdx'" -ForegroundColor DarkGray
    Write-Host "     Start-VM '$Name'; vmconnect localhost '$Name'" -ForegroundColor DarkGray
}

# Hyper-V module present?
if (-not (Get-Command New-VM -ErrorAction SilentlyContinue)) {
    Show-Manual
    return
}
if (-not (Test-Admin)) {
    throw "Administrator required for Hyper-V. Re-open PowerShell as Administrator."
}

# Remove a prior VM of the same name if asked.
$existing = Get-VM -Name $Name -ErrorAction SilentlyContinue
if ($existing) {
    if (-not $Force) {
        throw "A VM named '$Name' already exists. Re-run with -Force to replace it, or pass -Name <other>."
    }
    Write-Host "==> Removing existing VM '$Name'..." -ForegroundColor DarkYellow
    if ($existing.State -ne 'Off') { Stop-VM -Name $Name -TurnOff -Force }
    Remove-VM -Name $Name -Force
}

Write-Host "==> Creating Gen-1 Hyper-V VM '$Name' ($MemoryMB MB) from the VHDX..." -ForegroundColor Cyan
# -Generation 1: BIOS + IDE, matching the MBR boot disk.
$vm = New-VM -Name $Name -Generation 1 -MemoryStartupBytes ($MemoryMB * 1MB) -VHDPath $vhdx
Set-VM -Name $Name -AutomaticCheckpointsEnabled $false -ErrorAction SilentlyContinue | Out-Null
Set-VMProcessor -VMName $Name -Count 1

# Networking: attach the requested switch if it exists (optional - the OS only
# has a loopback stack today, so a NIC is not required to boot).
$sw = Get-VMSwitch -Name $Switch -ErrorAction SilentlyContinue
if ($sw) {
    Get-VMNetworkAdapter -VMName $Name | Connect-VMNetworkAdapter -SwitchName $Switch -ErrorAction SilentlyContinue
    Write-Host "  network: connected to switch '$Switch'" -ForegroundColor Green
} else {
    Write-Host "  network: switch '$Switch' not found - VM created without a network (fine; OS is loopback-only)." -ForegroundColor DarkYellow
}

Write-Host "  ok: VM '$Name' created (Generation 1, BIOS, IDE, $MemoryMB MB)." -ForegroundColor Green

if ($Start) {
    Write-Host "==> Starting VM '$Name'..." -ForegroundColor Cyan
    Start-VM -Name $Name
    Write-Host "  started. Open the console:" -ForegroundColor Green
    Write-Host "     vmconnect localhost '$Name'" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "Start it from Hyper-V Manager, or:" -ForegroundColor Cyan
    Write-Host "   Start-VM '$Name'; vmconnect localhost '$Name'" -ForegroundColor Cyan
}
