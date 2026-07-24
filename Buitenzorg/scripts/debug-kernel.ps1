# Buitenzorg OS - debug the kernel with GDB (Windows).
#
# Boots the BIOS image in QEMU **paused** with a GDB stub on tcp:1234, then
# attaches GDB with the kernel symbols and the helper commands in
# scripts/debug-kernel.gdb. Set a breakpoint (e.g. `bz-break-main`), `continue`,
# and step through ring-0 code. See docs/debugging.md.
#
# Usage:
#   .\scripts\debug-kernel.ps1            # BIOS image, attach GDB
#   .\scripts\debug-kernel.ps1 -Uefi      # UEFI image
#   .\scripts\debug-kernel.ps1 -NoAttach  # just start QEMU paused; attach GDB yourself
#
# Requires: a built kernel (.\scripts\build.ps1) and gdb on PATH (or set the GDB
# env var). QEMU is auto-detected like the other scripts.

[CmdletBinding()]
param(
    [switch] $Uefi,
    [switch] $NoAttach,
    [int]    $Port = 1234
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# Locate the kernel ELF that carries the debug symbols. Prefer release (what the
# images are built from); fall back to debug.
$elf = Join-Path $root "kernel\target\x86_64-unknown-none\release\bzkernel"
if (-not (Test-Path $elf)) {
    $elf = Join-Path $root "kernel\target\x86_64-unknown-none\debug\bzkernel"
}
if (-not (Test-Path $elf)) {
    throw "Kernel ELF not found. Build first: .\scripts\build.ps1"
}

# Locate QEMU (env override, PATH, default install) - same logic as run-qemu.
$qemu = $env:QEMU
if (-not $qemu) { $c = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue; if ($c) { $qemu = $c.Source } }
if (-not $qemu) { $qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe" }
if (-not (Test-Path $qemu)) { throw "QEMU not found; install it or set the QEMU env var." }

$img = if ($Uefi) { Join-Path $root "dist\buitenzorg-uefi.img" } else { Join-Path $root "dist\buitenzorg-bios.img" }
if (-not (Test-Path $img)) { throw "Disk image not found: $img (run .\scripts\build.ps1)." }

$if = "ide"
$qemuArgs = @(
    "-drive", "file=$img,format=raw,if=$if",
    "-m", "512M",
    "-serial", "stdio",
    "-device", "AC97,audiodev=snd0", "-audiodev", "none,id=snd0",
    "-gdb", "tcp::$Port",     # GDB stub
    "-S"                      # start paused, so GDB can set breakpoints first
)

Write-Host "==> Kernel symbols: $elf" -ForegroundColor Cyan
Write-Host "==> Starting QEMU paused with a GDB stub on :$Port ..." -ForegroundColor Cyan
$qproc = Start-Process -FilePath $qemu -ArgumentList $qemuArgs -PassThru

try {
    if ($NoAttach) {
        Write-Host ""
        Write-Host "QEMU is paused. Attach with:" -ForegroundColor Green
        Write-Host "  gdb -x scripts/debug-kernel.gdb `"$elf`"" -ForegroundColor Green
        Write-Host "  (gdb) target remote :$Port" -ForegroundColor Green
        Write-Host ""
        Write-Host "Press Enter to stop QEMU..." -ForegroundColor DarkGray
        [void](Read-Host)
        return
    }

    $gdb = $env:GDB
    if (-not $gdb) { $c = Get-Command gdb -ErrorAction SilentlyContinue; if ($c) { $gdb = $c.Source } }
    if (-not $gdb) {
        Write-Host ""
        Write-Host "gdb not found on PATH. QEMU is paused on :$Port - attach manually:" -ForegroundColor Yellow
        Write-Host "  gdb -x scripts/debug-kernel.gdb `"$elf`"" -ForegroundColor Yellow
        Write-Host "  (gdb) target remote :$Port" -ForegroundColor Yellow
        Write-Host ""
        Write-Host "Install gdb (e.g. MSYS2 'gdb', or the 'gdb-multiarch' package) or set the GDB env var." -ForegroundColor Yellow
        Write-Host "Press Enter to stop QEMU..." -ForegroundColor DarkGray
        [void](Read-Host)
        return
    }

    $gdbScript = Join-Path $root "scripts\debug-kernel.gdb"
    Write-Host "==> Attaching $gdb ..." -ForegroundColor Cyan
    & $gdb -x $gdbScript -ex "target remote :$Port" $elf
} finally {
    if ($qproc -and -not $qproc.HasExited) {
        Write-Host "==> Stopping QEMU..." -ForegroundColor DarkGray
        Stop-Process -Id $qproc.Id -Force -ErrorAction SilentlyContinue
    }
}
