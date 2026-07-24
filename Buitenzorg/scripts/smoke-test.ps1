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
        "ide"  { @('-drive', "format=raw,file=$image") }
        "ahci" { @('-drive', "id=bzdisk,format=raw,file=$image,if=none",
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
                     "MILESTONE: IMGVIEW OK", "MILESTONE: JPEG OK")
}
Assert-Markers -Log $ideLog -Markers $ideMarkers
Write-Host "smoke [ide]: all milestones present"

foreach ($media in @("ahci", "nvme", "usb")) {
    $log = Join-Path $root "dist\boot-$media.log"
    Invoke-Boot -Media $media -Log $log
    Assert-Markers -Log $log -Markers $basic
    Write-Host "smoke [$media]: boots to READY"
}

Write-Host "SMOKE TEST PASSED: ide/ahci/nvme/usb" -ForegroundColor Green
