# Boot smoke test (Windows): boot the BIOS image on all four storage
# controllers (IDE/AHCI/NVMe/USB) and require the milestone markers.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$image = Join-Path $root "dist\buitenzorg-bios.img"

if (-not (Test-Path $image)) { & (Join-Path $PSScriptRoot "build.ps1") }

$qemu = $env:QEMU
if (-not $qemu) {
    $cmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($cmd) { $qemu = $cmd.Source } else { $qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe" }
}

function Invoke-Boot {
    param([string]$Media, [string]$Log)
    if (Test-Path $Log) { Remove-Item $Log -Force -Confirm:$false }
    $qargs = switch ($Media) {
        # The full IDE run uses 2 CPUs (ring-3 processes on the AP, SMP SCHED);
        # "ide1" re-runs the process tests on a single CPU.
        "ide"  { @('-smp', '2', '-drive', "format=raw,file=$image") }
        "ide1" { @('-drive', "format=raw,file=$image") }
        # AHCI also boots with 2 CPUs: SMP bring-up must start the second one.
        "ahci" { @('-smp', '2', '-drive', "id=bzdisk,format=raw,file=$image,if=none",
                   '-device', 'ahci,id=ahci0', '-device', 'ide-hd,drive=bzdisk,bus=ahci0.0') }
        "nvme" { @('-drive', "id=bzdisk,format=raw,file=$image,if=none",
                   '-device', 'nvme,drive=bzdisk,serial=bz0001') }
        "usb"  { @('-drive', "id=bzdisk,format=raw,file=$image,if=none",
                   '-usb', '-device', 'usb-storage,drive=bzdisk') }
    }
    $qargs += @('-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
                '-display', 'none', '-serial', "file:$Log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList $qargs -PassThru
    foreach ($i in 1..130) {
        Start-Sleep -Seconds 1
        if ((Test-Path $Log) -and (Select-String -Path $Log -Pattern "BUITENZORG READY" -Quiet)) { break }
    }
    Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction SilentlyContinue
}

function Assert-Markers {
    param([string]$Log, [string[]]$Markers)
    $content = Get-Content $Log -Raw
    foreach ($marker in $Markers) {
        if ($content -notmatch [regex]::Escape($marker)) {
            Write-Error "SMOKE TEST FAILED ($(Split-Path -Leaf $Log)): missing '$marker'"
            exit 1
        }
    }
}

$basic = @("MILESTONE: HELLO KERNEL OK", "MILESTONE: SCHEDULER OK",
           "MILESTONE: IPC OK", "MILESTONE: WINDOWS OK", "MILESTONE: THEMES OK",
           "MILESTONE: BUAH OK", "MILESTONE: COMPUTE OK", "MILESTONE: WINDOWCTL OK",
           "MILESTONE: SAVER OK", "MILESTONE: CAHAYA OK", "MILESTONE: AI OK",
           "MILESTONE: POWER OK", "MILESTONE: NALAR OK",
           "MILESTONE: VMX OK", "MILESTONE: VM OK", "MILESTONE: VIRTIO OK",
           "MILESTONE: SNAPSHOT OK", "MILESTONE: LAPIS OK",
           "MILESTONE: SCRIPT JS OK", "MILESTONE: SCRIPT TS OK", "MILESTONE: SCRIPT PY OK",
           "MILESTONE: POLYGLOT OK", "MILESTONE: BABEL OK",
           "MILESTONE: PAL MEM OK", "MILESTONE: THREADS OK",
           "MILESTONE: SYNC PAL OK", "MILESTONE: HEAP PAL OK",
           "MILESTONE: GCMEM PAL OK", "MILESTONE: BCL PAL OK",
           "MILESTONE: BCL2 PAL OK",
           "MILESTONE: DRAWING2 OK", "MILESTONE: UI TOOLKIT OK",
           "MILESTONE: AUDIO OK", "MILESTONE: AUDIO PANEL OK",
           "MILESTONE: SUITE OK", "MILESTONE: DESKTOP SHELL OK",
           "MILESTONE: SECURITY OK", "MILESTONE: PROFILER OK",
           "BUITENZORG READY")

$ideLog = Join-Path $root "dist\boot-ide.log"
Invoke-Boot -Media ide -Log $ideLog
$ideMarkers = $basic + @(
    "MILESTONE: MEMORY OK", "MILESTONE: SYSCALL ABI V1 OK", "MILESTONE: PCI OK",
    "MILESTONE: STORAGE OK", "MILESTONE: MOUSE OK", "MILESTONE: PIXELS OK",
    "MILESTONE: VFS OK", "MILESTONE: SERVICES OK", "MILESTONE: ASYNC IO OK",
    "MILESTONE: NETWORK OK", "MILESTONE: TERMINAL OK", "MILESTONE: THEME OK",
    "MILESTONE: WORKSPACE OK", "MILESTONE: KANOPI OK")
# The C# ring-3 milestones only run when the ELFs were embedded (bflat present).
if (Select-String -Path $ideLog -Pattern "Hello from C#" -Quiet) {
    $ideMarkers += @("Hello from C#!", "MILESTONE: TUNAS OK",
                     "svc-csharp", "MILESTONE: DAHAN OK",
                     "MILESTONE: KEMBANG OK", "MILESTONE: DRAWING OK",
                     "MILESTONE: TASKMGR OK", "MILESTONE: APPVARIANTS OK",
                     "MILESTONE: SERBUK OK", "MILESTONE: PACKAGE OK",
                     "MILESTONE: PERSONALIZE OK",
                     "MILESTONE: MMAP OK", "MILESTONE: MATANG OK",
                     "MILESTONE: THREAD OK", "MILESTONE: SYNC OK",
                     "MILESTONE: HEAP OK", "MILESTONE: GCMEM OK",
                     "MILESTONE: BCL OK", "MILESTONE: BCL2 OK",
                     "MILESTONE: DRAW OK",
                     "MILESTONE: UI OK", "MILESTONE: CALC OK",
                     "MILESTONE: GAME OK", "MILESTONE: CLOCK OK",
                     "MILESTONE: PIANO OK", "MILESTONE: STORE OK",
                     "MILESTONE: FILES OK", "MILESTONE: EDITOR OK",
                     "MILESTONE: IMGVIEW OK", "MILESTONE: JPEG OK",
                     "MILESTONE: MULTIPROC OK", "MILESTONE: DISK WRITE OK",
                     "MILESTONE: PROCCTL OK", "MILESTONE: WALLPAPERS OK", "MILESTONE: ICONS OK",
                     "MILESTONE: SMP SCHED OK", "MILESTONE: SMP KERNEL OK", "MILESTONE: SMP FINELOCK OK")
}
$ideMarkers += @("MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: SMP PERCPU OK")
Assert-Markers -Log $ideLog -Markers $ideMarkers
Write-Host "smoke [ide]: all milestones present (2 CPUs, ring-3 on the AP)"


# Persistent /disk: the kernel printed the exact contents it wrote to
# /disk/BOOTCNT.TXT. Those bytes must now be inside the disk image file on the
# host — proof the write reached the medium, not just a cache.
$persist = Select-String -Path $ideLog -Pattern 'BOOTCNT\.TXT = (BZPERSIST[^\r\n]*)' | Select-Object -First 1
if (-not $persist) { Write-Error "SMOKE TEST FAILED: kernel did not report BOOTCNT.TXT"; exit 1 }
$needle = $persist.Matches[0].Groups[1].Value
$diskText = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($image))
if (-not $diskText.Contains($needle)) { Write-Error "SMOKE TEST FAILED: '$needle' not found in $image"; exit 1 }
Write-Host "smoke [ide]: /disk write persisted to the image ($needle)"

# Uniprocessor regression: the same process/thread tests on one CPU.
$ide1Log = Join-Path $root "dist\boot-ide1.log"
Invoke-Boot -Media ide1 -Log $ide1Log
$ide1Markers = $basic + @("MILESTONE: SMP OK (1 CPU(s)")
if (Select-String -Path $ide1Log -Pattern "Hello from C#" -Quiet) {
    $ide1Markers += @("MILESTONE: THREAD OK", "MILESTONE: SYNC OK", "MILESTONE: MULTIPROC OK", "MILESTONE: PROCCTL OK")
}
Assert-Markers -Log $ide1Log -Markers $ide1Markers
Write-Host "smoke [ide1]: process tests pass on a single CPU"

foreach ($media in @("ahci", "nvme", "usb")) {
    $log = Join-Path $root "dist\boot-$media.log"
    Invoke-Boot -Media $media -Log $log
    Assert-Markers -Log $log -Markers $basic
    if ($media -eq "ahci") {
        Assert-Markers -Log $log -Markers @("MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: SMP PERCPU OK")
        Write-Host "smoke [ahci]: second CPU started (SMP)"
    }
    Write-Host "smoke [$media]: boots to READY"
}

# UEFI boot (OVMF, 2 CPUs). UEFI firmware leaves the 8259 PIC fully masked —
# that once hung every UEFI boot at the first timer wait while the BIOS-only
# smoke test stayed green. Needs the OVMF files `bzimage --run --uefi` downloads.
$ovmf = Join-Path $root "kernel\target\ovmf\x64"
$uefiImage = Join-Path $root "dist\buitenzorg-uefi.img"
if ((Test-Path (Join-Path $ovmf "code.fd")) -and (Test-Path $uefiImage)) {
    $log = Join-Path $root "dist\boot-uefi.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    $qargs = @('-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'code.fd')",
               '-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'vars.fd')",
               '-drive', "format=raw,file=$uefiImage", '-smp', '2',
               '-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
               '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList $qargs -PassThru
    foreach ($i in 1..130) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "BUITENZORG READY" -Quiet)) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers ($basic + @("MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: SMP PERCPU OK"))
    Write-Host "smoke [uefi]: boots to READY on 2 CPUs"
} else {
    Write-Host "smoke [uefi]: skipped (OVMF not downloaded; run 'cargo run -p bzimage -- --run --uefi' once)"
}

Write-Host "SMOKE TEST PASSED: ide/ahci/nvme/usb" -ForegroundColor Green
