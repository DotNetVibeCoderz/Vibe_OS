# Boot smoke test (Windows): boot the images in dist/ -- x86-64 on every boot
# medium (IDE/AHCI/NVMe/USB/virtio, UEFI, Limine), ARM64 and RISC-V on QEMU
# `virt` -- and require the milestone markers.
#
# Multi-arch M5 (same switches as smoke-test.sh's SMOKE_ARCH / SMOKE_STRICT):
#   -Arch x86_64|aarch64|riscv64|all   which architecture(s) to boot
#   -Strict                            a skipped step of a selected
#                                      architecture fails the run
param(
    [ValidateSet("all", "x86_64", "aarch64", "riscv64")]
    [string]$Arch = "all",
    [switch]$Strict
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$image = Join-Path $root "dist\buitenzorg-bios.img"
# Every boot below is the self-test boot (bootmode.rs): QEMU's fw_cfg file
# switches on all demos, self-tests and milestones; a normal boot goes
# straight to the desktop (see the [fastboot] step).
$selftest = @('-fw_cfg', 'name=opt/buitenzorg/selftest,string=1')

if (-not (Test-Path $image)) { & (Join-Path $PSScriptRoot "build.ps1") }

function Test-Arch([string]$Name) { return ($Arch -eq "all") -or ($Arch -eq $Name) }
function Skip-Step([string]$Message) {
    Write-Host $Message
    if ($Strict) { Write-Error "SMOKE TEST FAILED: step skipped in strict mode (-Strict)"; exit 1 }
}

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
        # ...and an xHCI with a hub and a USB keyboard + mouse behind it (R4).
        "ahci" { @('-smp', '2', '-drive', "id=bzdisk,format=raw,file=$image,if=none",
                   '-device', 'ahci,id=ahci0', '-device', 'ide-hd,drive=bzdisk,bus=ahci0.0',
                   '-device', 'qemu-xhci,id=xhci', '-device', 'usb-hub,bus=xhci.0,port=1',
                   '-device', 'usb-kbd,bus=xhci.0,port=1.1', '-device', 'usb-mouse,bus=xhci.0,port=1.2') }
        "nvme" { @('-drive', "id=bzdisk,format=raw,file=$image,if=none",
                   '-device', 'nvme,drive=bzdisk,serial=bz0001') }
        "usb"  { @('-drive', "id=bzdisk,format=raw,file=$image,if=none",
                   '-device', 'qemu-xhci,id=xhci', '-device', 'usb-storage,bus=xhci.0,drive=bzdisk') }
        # Multi-arch M2: the boot disk on virtio-blk (2 CPUs).
        # Multi-arch M2: the boot disk on virtio-blk, a virtio keyboard + tablet,
        # and a QEMU monitor to type/move through them once the desktop is up.
        "virtio" { @('-smp', '2', '-drive', "id=bzdisk,format=raw,file=$image,if=none",
                     '-device', 'virtio-blk-pci,drive=bzdisk,bootindex=0,disable-legacy=on',
                     '-device', 'virtio-keyboard-pci', '-device', 'virtio-tablet-pci',
                     '-netdev', 'user,id=n0', '-device', 'virtio-net-pci,netdev=n0,disable-legacy=on',
                     '-device', 'virtio-gpu-pci', '-audiodev', 'none,id=vsnd', '-device', 'virtio-sound-pci,audiodev=vsnd',
                     '-monitor', 'tcp:127.0.0.1:4481,server,nowait') }
    }
    $qargs += @('-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
                '-display', 'none', '-serial', "file:$Log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList ($qargs + $selftest) -PassThru
    foreach ($i in 1..420) {
        Start-Sleep -Seconds 1
        if ((Test-Path $Log) -and (Select-String -Path $Log -Pattern "BUITENZORG READY" -Quiet)) { break }
    }
    if ($Media -eq "virtio") { Send-VirtioInput -Log $Log -Port 4481 }
    Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction SilentlyContinue
}

# Type a few keys and move/click the pointer through the QEMU monitor; the
# events reach the guest through the virtio keyboard/tablet (QEMU routes input
# to the virtio devices once their driver is active).
function Send-VirtioInput {
    param([string]$Log, [int]$Port)
    try {
        $c = New-Object System.Net.Sockets.TcpClient("127.0.0.1", $Port)
        $w = New-Object System.IO.StreamWriter($c.GetStream())
        $w.AutoFlush = $true
        foreach ($cmd in @("sendkey l", "sendkey s", "sendkey ret", "mouse_move 200 150", "mouse_button 1", "mouse_button 0")) {
            $w.WriteLine($cmd); Start-Sleep -Milliseconds 700
        }
        foreach ($i in 1..20) {
            Start-Sleep -Seconds 1
            if (Select-String -Path $Log -Pattern "VIRTIO INPUT OK" -Quiet) { break }
        }
        $c.Close()
    } catch { Write-Host "smoke [virtio]: monitor not reachable ($_)" }
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
           "MILESTONE: SECURITY OK", "MILESTONE: GFX3D OK", "MILESTONE: PROFILER OK", "MILESTONE: IOAPIC OK", "MILESTONE: DTB OK",
           "BUITENZORG READY")

if (Test-Arch "x86_64") {
# Normal boot (no fw_cfg switch): straight to the desktop, no demos. The
# kernel reports when its first desktop frame was on screen; that must stay
# under the budget (milliseconds of kernel time; the BIOS and loader in front
# of it are QEMU's), and no self-test may have run.
$fastLog = Join-Path $root "dist\boot-fast.log"
if (Test-Path $fastLog) { Remove-Item $fastLog -Force -Confirm:$false }
$fastImg = Join-Path $root "dist\fastboot-test.img"
Copy-Item $image $fastImg -Force
$fargs = @('-smp', '2', '-drive', "format=raw,file=$fastImg", '-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
           '-display', 'none', '-serial', "file:$fastLog", '-no-reboot')
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $qemu -ArgumentList $fargs -PassThru
$wall = 0
foreach ($i in 1..1200) {
    Start-Sleep -Milliseconds 250
    if ((Test-Path $fastLog) -and (Select-String -Path $fastLog -Pattern "first desktop frame" -Quiet)) { $wall = $sw.Elapsed.TotalSeconds; break }
}
foreach ($i in 1..60) {
    Start-Sleep -Milliseconds 500
    if (Select-String -Path $fastLog -Pattern "background start-up done" -Quiet) { break }
}
Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
Remove-Item $fastImg -Force -ErrorAction SilentlyContinue
Assert-Markers -Log $fastLog -Markers @("[boot] mode: normal", "BUITENZORG READY", "[boot] first desktop frame on screen", "[boot] background start-up done")
if (Select-String -Path $fastLog -Pattern "MILESTONE: SCHEDULER OK" -Quiet) {
    Write-Error "SMOKE TEST FAILED (boot-fast.log): the normal boot ran the self-test demos"; exit 1
}
$frameMs = [int]((Select-String -Path $fastLog -Pattern "first desktop frame on screen (\d+) ms").Matches[0].Groups[1].Value)
$budgetMs = 8000
if ($frameMs -gt $budgetMs) {
    Write-Error "SMOKE TEST FAILED (boot-fast.log): first desktop frame $frameMs ms after kernel start (budget $budgetMs ms)"; exit 1
}
Write-Host ("smoke [fastboot]: normal boot -- desktop on screen {0} ms after kernel start, {1:N1} s after QEMU start (BIOS + loader included)" -f $frameMs, $wall)

# bz-link: the host CLI (sdk/bz) talks to a running device over BZDP (UDP
# 7070): the service starts at boot from a fw_cfg pairing code, `bz` runs a
# shell line and reads its output, deploys a script that runs as its own
# process (its print() reaches the serial log), and pulls a file back.
$dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
$bzDir = Join-Path $root "dist\bz"
$bzOk = $false
if ($dotnet) {
    & dotnet build (Join-Path $root "sdk\bz\bz.csproj") -c Release -nologo -v q -o $bzDir 2>&1 | Out-Null
    $bzOk = ($LASTEXITCODE -eq 0) -and (Test-Path (Join-Path $bzDir "bz.dll"))
}
if (-not $bzOk) {
    Write-Host "smoke [bzlink]: skipped (no .NET 10 SDK to build sdk/bz)"
} else {
    $linkLog = Join-Path $root "dist\boot-bzlink.log"
    if (Test-Path $linkLog) { Remove-Item $linkLog -Force -Confirm:$false }
    $linkImg = Join-Path $root "dist\bzlink-test.img"
    Copy-Item $image $linkImg -Force
    $largs = @('-smp', '2', '-drive', "format=raw,file=$linkImg", '-m', '512M',
               '-netdev', 'user,id=n0,hostfwd=udp::7171-:7070', '-device', 'e1000,netdev=n0',
               '-fw_cfg', 'name=opt/buitenzorg/deploy,string=4321',
               '-display', 'none', '-serial', "file:$linkLog", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList $largs -PassThru
    foreach ($i in 1..480) {
        Start-Sleep -Milliseconds 250
        if ((Test-Path $linkLog) -and (Select-String -Path $linkLog -Pattern "background start-up done" -Quiet)) { break }
    }
    $env:BZ_DEVICE = "127.0.0.1:7171"; $env:BZ_CODE = "4321"
    $bz = Join-Path $bzDir "bz.dll"
    $script = Join-Path $root "dist\BZLINK.JS"
    Set-Content -Path $script -Value 'print("BZLINK " + (6 * 7));' -Encoding ascii
    $hello = (& dotnet $bz device 2>&1) -join "`n"
    $echo = (& dotnet $bz run "echo hello-bzlink" 2>&1) -join "`n"
    $dep = (& dotnet $bz deploy $script --follow 4 2>&1) -join "`n"
    $pulled = Join-Path $root "dist\bzlink-pull.txt"
    $pull = (& dotnet $bz pull /disk/BATANG.TXT $pulled 2>&1) -join "`n"
    Remove-Item Env:BZ_DEVICE, Env:BZ_CODE -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    Remove-Item $linkImg, $script -Force -ErrorAction SilentlyContinue
    $fail = $null
    if ($hello -notmatch "Buitenzorg OS x86-64") { $fail = "bz device: $hello" }
    elseif ($echo -notmatch "hello-bzlink") { $fail = "bz run: $echo" }
    elseif ($dep -notmatch "/disk/BZLINK.JS") { $fail = "bz deploy: $dep" }
    elseif (-not (Select-String -Path $linkLog -Pattern "BZLINK 42" -Quiet)) { $fail = "the deployed script did not print 'BZLINK 42' (bz deploy said: $dep)" }
    elseif (-not ((Test-Path $pulled) -and ((Get-Content $pulled -Raw) -match "Akar menembus tanah"))) { $fail = "bz pull: $pull" }
    Remove-Item $pulled -Force -ErrorAction SilentlyContinue
    if ($fail) { Write-Error "SMOKE TEST FAILED (boot-bzlink.log): $fail"; exit 1 }
    Assert-Markers -Log $linkLog -Markers @("[deploy] service on at boot", "[deploy] exec: echo hello-bzlink", "[deploy] wrote /disk/BZLINK.JS")
    Write-Host "smoke [bzlink]: bz device/run/deploy/pull against a running OS over BZDP (UDP), the deployed script ran"
}

$ideLog = Join-Path $root "dist\boot-ide.log"
Invoke-Boot -Media ide -Log $ideLog
$ideMarkers = $basic + @(
    "MILESTONE: MEMORY OK", "MILESTONE: SYSCALL ABI V1 OK", "MILESTONE: PCI OK",
    "MILESTONE: STORAGE OK", "MILESTONE: MOUSE OK", "MILESTONE: PIXELS OK",
    "MILESTONE: VFS OK", "MILESTONE: SERVICES OK", "MILESTONE: ASYNC IO OK",
    "MILESTONE: NETWORK OK", "MILESTONE: TERMINAL OK", "MILESTONE: THEME OK",
    "MILESTONE: WORKSPACE OK", "MILESTONE: KANOPI OK",
    "MILESTONE: CLR OK", "MILESTONE: CLR GUI OK", "MILESTONE: MANAGED UI OK", "MILESTONE: MANAGED APPS OK", "Studio: 25 checks, 0 failed", "HelloGui: live clock ticked", "Hello from managed C# on Buitenzorg (RustCLR)!", "checks=515 failures=", "checks=35 failures=0")
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
                     "MILESTONE: HEAP OK", "MILESTONE: STATICS OK", "MILESTONE: CRASH REPORT OK", "MILESTONE: BCL3 OK", "MILESTONE: GCMEM OK",
                     "MILESTONE: BCL OK", "MILESTONE: BCL2 OK",
                     "MILESTONE: DRAW OK",
                     "MILESTONE: UI OK", "MILESTONE: CALC OK",
                     "MILESTONE: GAME OK", "MILESTONE: CLOCK OK",
                     "MILESTONE: PIANO OK", "MILESTONE: STORE OK",
                     "MILESTONE: FILES OK", "MILESTONE: EDITOR OK",
                     "MILESTONE: IMGVIEW OK", "MILESTONE: JPEG OK",
                     "MILESTONE: MULTIPROC OK", "MILESTONE: DISK WRITE OK",
                     "MILESTONE: PROCCTL OK", "MILESTONE: WALLPAPERS OK", "MILESTONE: USERS OK", "MILESTONE: TCP OK", "MILESTONE: FOLDERS OK", "MILESTONE: BZPKG OK", "MILESTONE: ICONS OK",
                     "MILESTONE: SMP SCHED OK", "MILESTONE: SMP KERNEL OK", "MILESTONE: SMP FINELOCK OK", "MILESTONE: SMP THREADS OK")
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

# virtio-blk boot disk (multi-arch M2): the kernel's own virtio driver must
# carry /disk -- every app, the process tests, and a write that reaches the
# image file on the host.
$vioLog = Join-Path $root "dist\boot-virtio.log"
Invoke-Boot -Media virtio -Log $vioLog
$vioMarkers = $basic + @("MILESTONE: VIRTIO BLK OK", "via virtio-blk", "MILESTONE: DISK WRITE OK", "MILESTONE: VIRTIO INPUT OK", "MILESTONE: VIRTIO NET OK", "MILESTONE: VIRTIO GPU OK", "MILESTONE: VIRTIO SND OK")
if (Select-String -Path $vioLog -Pattern "Hello from C#" -Quiet) {
    $vioMarkers += @("MILESTONE: MULTIPROC OK", "MILESTONE: PROCCTL OK", "MILESTONE: EDITOR OK", "MILESTONE: SMP FINELOCK OK")
}
Assert-Markers -Log $vioLog -Markers $vioMarkers
$persist = Select-String -Path $vioLog -Pattern 'BOOTCNT\.TXT = (BZPERSIST[^\r\n]*)' | Select-Object -First 1
if (-not $persist) { Write-Error "SMOKE TEST FAILED: virtio boot did not report BOOTCNT.TXT"; exit 1 }
$needle = $persist.Matches[0].Groups[1].Value
$diskText = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($image))
if (-not $diskText.Contains($needle)) { Write-Error "SMOKE TEST FAILED: '$needle' (virtio write) not found in $image"; exit 1 }
Write-Host "smoke [virtio]: /disk on virtio-blk, apps + processes, write persisted ($needle); keyboard + tablet via virtio-input, ARP + ping over virtio-net, desktop on virtio-gpu, tone via virtio-snd"

foreach ($media in @("ahci", "nvme", "usb")) {
    $log = Join-Path $root "dist\boot-$media.log"
    Invoke-Boot -Media $media -Log $log
    Assert-Markers -Log $log -Markers $basic
    if ($media -eq "ahci") {
        Assert-Markers -Log $log -Markers @("MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: SMP PERCPU OK",
            "MILESTONE: USB OK (xhci-pci: xHCI, 3 device(s): 1 hub(s), 1 keyboard(s), 1 mouse")
        Write-Host "smoke [ahci]: second CPU started (SMP); USB hub + keyboard + mouse enumerated on xHCI"
    }
    $on = @{ ahci = "ahci0"; nvme = "nvme0"; usb = "usb" }[$media]
    Assert-Markers -Log $log -Markers @("mounted /disk (FAT16, rw) on $on")
    Write-Host "smoke [$media]: boots to READY with /disk on $on"
}

# UEFI boot (OVMF, 2 CPUs). UEFI firmware leaves the 8259 PIC fully masked —
# that once hung every UEFI boot at the first timer wait while the BIOS-only
# smoke test stayed green. Needs the OVMF files `bzimage --run --uefi` downloads.
$ovmf = Join-Path $root "kernel\target\ovmf\x64"
$uefiImage = Join-Path $root "dist\buitenzorg-uefi.img"
if ((Test-Path (Join-Path $ovmf "code.fd")) -and (Test-Path $uefiImage)) {
    $log = Join-Path $root "dist\boot-uefi.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    # writable vars copy: with read-only vars OVMF writes NvVars into the image's
    # FAT root, which can fill a FAT16 root and fail DISK WRITE (seen in CI)
    $uefiVars = Join-Path $root "dist\uefi-vars.fd"
    Copy-Item (Join-Path $ovmf 'vars.fd') $uefiVars -Force
    $qargs = @('-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'code.fd')",
               '-drive', "if=pflash,format=raw,file=$uefiVars",
               '-drive', "format=raw,file=$uefiImage", '-smp', '2',
               '-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
               '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList ($qargs + $selftest) -PassThru
    foreach ($i in 1..420) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "BUITENZORG READY" -Quiet)) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers ($basic + @("MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: SMP PERCPU OK"))
    Write-Host "smoke [uefi]: boots to READY on 2 CPUs"
} else {
    Skip-Step "smoke [uefi]: skipped (OVMF not downloaded; run 'cargo run -p bzimage -- --run --uefi' once)"
}

# A modern PC, as closely as QEMU can: q35 without a PS/2 controller, UEFI, a
# 2560x1440 screen, 4 CPUs, booted from a USB stick on xHCI with USB keyboard +
# mouse and an e1000. The *normal* boot, the one real hardware gets. On AHCI
# sits a stranger's FAT disk (no marker file), which must stay untouched: /disk
# is written to at every boot, and it must never be someone's EFI partition.
if ((Test-Path (Join-Path $ovmf "code.fd")) -and (Test-Path $uefiImage)) {
    $log = Join-Path $root "dist\boot-realpc.log"
    $stick = Join-Path $root "dist\realpc-stick.img"
    $other = Join-Path $root "dist\realpc-other.img"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    Copy-Item $uefiImage $stick -Force
    $latin = [Text.Encoding]::GetEncoding(28591)
    [IO.File]::WriteAllBytes($other, $latin.GetBytes($latin.GetString([IO.File]::ReadAllBytes($image)).Replace('BZDISK  ID ', 'XZDISK  ID ')))
    $otherSum = (Get-FileHash $other).Hash
    # writable variables: with read-only ones OVMF saves them in a file NvVars on
    # the first FAT disk it finds, and this step is about what the *kernel* writes
    $vars = Join-Path $root "dist\realpc-vars.fd"
    Copy-Item (Join-Path $ovmf 'vars.fd') $vars -Force
    $qargs = @('-M', 'q35,i8042=off', '-m', '2G', '-smp', '4',
               '-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'code.fd')",
               '-drive', "if=pflash,format=raw,file=$vars",
               '-drive', "id=other,format=raw,file=$other,if=none", '-device', 'ahci,id=ahci0', '-device', 'ide-hd,drive=other,bus=ahci0.0',
               '-drive', "id=stick,format=raw,file=$stick,if=none", '-device', 'qemu-xhci,id=xhci',
               '-device', 'usb-storage,bus=xhci.0,drive=stick,bootindex=0', '-device', 'usb-kbd,bus=xhci.0', '-device', 'usb-mouse,bus=xhci.0',
               '-netdev', 'user,id=n0', '-device', 'e1000,netdev=n0', '-device', 'VGA,xres=2560,yres=1440',
               '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList $qargs -PassThru
    foreach ($i in 1..280) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -SimpleMatch "background start-up done" -Quiet)) { break }
    }
    Start-Sleep -Seconds 2
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; $p.WaitForExit() }
    Assert-Markers -Log $log -Markers @("[boot] mode: normal", "framebuffer 2560x1440", "4 CPU(s) / 3 AP(s)", "1 keyboard(s), 1 mouse/mice, 1 disk(s)",
        "mounted /disk (FAT16, rw) on usb", "MILESTONE: NIC OK (e1000", "[boot] background start-up done")
    if (Select-String -Path $log -Pattern "PANIC|mounted /disk .* on ahci" -Quiet) { throw "SMOKE TEST FAILED (boot-realpc.log): panic, or /disk went to the other disk" }
    if ((Get-FileHash $other).Hash -ne $otherSum) { throw "SMOKE TEST FAILED (boot-realpc.log): the other disk was written to" }
    $stickText = $latin.GetString([IO.File]::ReadAllBytes($stick))
    if (-not ($stickText.Contains("administrator") -and $stickText.Contains("[boot] mode: normal"))) { throw "SMOKE TEST FAILED (boot-realpc.log): the accounts or BOOTLOG.TXT did not reach the stick" }
    Remove-Item $stick, $other, $vars -Force -Confirm:$false
    Write-Host "smoke [realpc]: UEFI on q35 without PS/2, 2560x1440: /disk on a USB stick (rw), USB input, e1000 + DHCP, the other FAT disk untouched"
} else {
    Skip-Step "smoke [realpc]: skipped (OVMF not downloaded)"
}

# Limine boot (multi-arch M1): the same kernel through the Limine protocol,
# UEFI, 2 CPUs, /disk on an MBR FAT partition. Needs OVMF + the Limine
# binaries (scripts/fetch-limine.ps1) so bzimage built the image.
$limineImage = Join-Path $root "dist\buitenzorg-limine-uefi.img"
if ((Test-Path (Join-Path $ovmf "code.fd")) -and (Test-Path $limineImage)) {
    $log = Join-Path $root "dist\boot-limine.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    # writable vars copy: with read-only vars OVMF writes NvVars into the image's
    # FAT root, which can fill a FAT16 root and fail DISK WRITE (seen in CI)
    $limineVars = Join-Path $root "dist\limine-vars.fd"
    Copy-Item (Join-Path $ovmf 'vars.fd') $limineVars -Force
    $qargs = @('-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'code.fd')",
               '-drive', "if=pflash,format=raw,file=$limineVars",
               '-drive', "format=raw,file=$limineImage", '-smp', '2',
               '-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
               '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList ($qargs + $selftest) -PassThru
    foreach ($i in 1..420) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "BUITENZORG READY" -Quiet)) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers ($basic + @("[boot] loader=limine", "MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: DISK WRITE OK"))
    # MULTIPROC runs C# programs (MPROC.ELF): only when bflat built them.
    if (Select-String -Path $log -Pattern "Hello from C#" -Quiet) { Assert-Markers -Log $log -Markers @("MILESTONE: MULTIPROC OK") }
    Write-Host "smoke [limine]: Limine protocol boot (UEFI) to READY on 2 CPUs, /disk + processes"
} else {
    Skip-Step "smoke [limine]: skipped (needs OVMF and scripts/fetch-limine.ps1 before the build)"
}
# G3 on a host GPU: virtio-gpu-gl (virglrenderer) with the QEMU copy that
# scripts/setup-virgl.ps1 prepares (Mesa EGL / llvmpipe next to it). The
# kernel runs the 3D self-test twice -- software, then through virgl -- and
# both must pass. Optional: without kernel\target\qemu-gl the step is
# reported and skipped even in strict mode (CI has no host GL).
$qemuGl = Join-Path $root "kernel\target\qemu-gl\qemu-system-x86_64.exe"
if ((Test-Path $qemuGl) -and (Test-Path $image)) {
    $log = Join-Path $root "dist\boot-virgl.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    $copy = Join-Path $root "dist\virgl-test.img"
    Copy-Item $image $copy -Force
    $env:GALLIUM_DRIVER = "llvmpipe"
    $qargs = @('-drive', "format=raw,file=$copy", '-m', '1G', '-smp', '2', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
               '-device', 'virtio-gpu-gl-pci', '-display', 'egl-headless', '-L', "`"$(Join-Path (Split-Path -Parent $qemu) 'share')`"",
               '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemuGl -ArgumentList ($qargs + $selftest) -PassThru
    foreach ($i in 1..420) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "GFX3D VIRGL OK|GPU \(virgl\) self-test FAILED|virgl\] not used" -Quiet)) { Start-Sleep 1; break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Remove-Item $copy -Force -ErrorAction SilentlyContinue
    Assert-Markers -Log $log -Markers @("[virgl] 3D context up", "MILESTONE: GFX3D OK", "MILESTONE: GFX3D VIRGL OK", "MILESTONE: MANAGED APPS OK", "Studio: 25 checks, 0 failed", "GPU (virgl)): 7 objek")
    Write-Host "smoke [virgl]: 3D API on the host GPU (virtio-gpu-gl): self-test + Ruang 3D through virgl"
} else {
    Write-Host "smoke [virgl]: skipped (no kernel\target\qemu-gl -- scripts\setup-virgl.ps1 prepares one)"
}
} # x86_64

if (Test-Arch "aarch64") {
# ARM64 (multi-arch M3): the ARM64 kernel through Limine on QEMU `virt`
# (GICv3, Cortex-A72, edk2 firmware, Device Tree: acpi=off -- with ACPI on,
# edk2 hides the DTB). Needs qemu-system-aarch64 next to qemu-system-x86_64.
$qemuArm = Join-Path (Split-Path -Parent $qemu) "qemu-system-aarch64.exe"
$armFw = Join-Path (Split-Path -Parent $qemu) "share\edk2-aarch64-code.fd"
$armImage = Join-Path $root "dist\buitenzorg-arm64.img"
if ((Test-Path $qemuArm) -and (Test-Path $armFw) -and (Test-Path $armImage)) {
    $log = Join-Path $root "dist\boot-arm64.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    $qargs = @('-M', 'virt,gic-version=3,acpi=off', '-cpu', 'cortex-a72', '-smp', '2', '-m', '512M', '-bios', "`"$armFw`"",
               '-global', 'virtio-mmio.force-legacy=false',
               '-drive', "`"id=d0,format=raw,file=$armImage,if=none`"", '-device', 'virtio-blk-device,drive=d0',
               '-device', 'virtio-gpu-device', '-device', 'ramfb',
               '-netdev', 'user,id=n0', '-device', 'virtio-net-device,netdev=n0',
               '-device', 'virtio-keyboard-device', '-device', 'virtio-tablet-device',
               '-monitor', 'tcp:127.0.0.1:4482,server,nowait', '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemuArm -ArgumentList ($qargs + $selftest) -PassThru
    # same budget as smoke-test.sh (420 s): the managed app suite runs before READY
    foreach ($i in 1..420) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "ARM64 READY" -Quiet)) { break }
    }
    # The interactive desktop (shared desktop loop): type + move/click through
    # the virtio keyboard / tablet.
    Start-Sleep -Seconds 3
    Send-VirtioInput -Log $log -Port 4482
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers @("MILESTONE: HELLO KERNEL OK", "loader=limine", "loader DTB: parsed",
        "MILESTONE: DTB OK", "MILESTONE: ARM64 MEMORY OK", "MILESTONE: MEMORY OK", "MILESTONE: ARM64 ASPACE OK", "MILESTONE: ARM64 CONTEXT OK", "MILESTONE: ARM64 EL0 OK", "Hello from EL0",
        "MILESTONE: ARM64 RTC OK", "MILESTONE: VIRTIO BLK OK (virtio-mmio", "MILESTONE: ARM64 DISK OK", "MILESTONE: VIRTIO GPU OK", "MILESTONE: DHCP OK",
        "MILESTONE: SECURITY OK", "MILESTONE: GFX3D OK", "MILESTONE: ARM64 PSCI OK", "MILESTONE: NETWORK OK", "MILESTONE: VIRTIO NET OK",
        "MILESTONE: USERS OK", "MILESTONE: TCP OK", "MILESTONE: FOLDERS OK", "MILESTONE: BZPKG OK", "MILESTONE: BOOT SPLASH OK (", "[desktop] tidied", "MILESTONE: VIRTIO INPUT OK", "MILESTONE: ARM64 SMP OK (2 CPUs", "MILESTONE: ARM64 TIMER OK", "MILESTONE: ARM64 BOOT OK", "BUITENZORG ARM64 READY", "MILESTONE: CLR OK", "MILESTONE: CLR GUI OK", "MILESTONE: MANAGED UI OK", "MILESTONE: MANAGED APPS OK", "Studio: 25 checks, 0 failed", "HelloGui: live clock ticked", "Hello from managed C# on Buitenzorg (RustCLR)!", "checks=515 failures=", "checks=35 failures=0")
    if (Select-String -Path $log -Pattern "program 'hello'" -Quiet) {
        Assert-Markers -Log $log -Markers @("Hello from C#!", "MILESTONE: ARM64 CSHARP OK", "MILESTONE: MATANG OK",
            "MILESTONE: HEAP OK", "MILESTONE: STATICS OK", "MILESTONE: CRASH REPORT OK", "MILESTONE: BCL3 OK", "MILESTONE: GCMEM OK", "MILESTONE: ARM64 MULTIPROC OK", "MILESTONE: THREAD OK", "MILESTONE: SYNC OK", "frames leaked: 0", "from /disk",
            "MILESTONE: BCL OK", "MILESTONE: PROCCTL OK", "MILESTONE: CALC OK", "MILESTONE: GAME OK", "MILESTONE: UI OK",
            "MILESTONE: DRAW OK", "MILESTONE: ARM64 DESKTOP FRAME OK", "MILESTONE: ARM64 SMP SCHED OK")
    }
    Write-Host "smoke [arm64]: ARM64 kernel via Limine: live DTB, PL011, GICv3 + timer, paging + heap + address spaces, threads + EL0 programs, C# apps, preemptive processes + threads, virtio-mmio disk + GPU + net, PL031, PSCI, shared syscalls, C# GUI apps + interactive desktop, 2 CPUs (PSCI)"
} else {
    Skip-Step "smoke [arm64]: skipped (needs qemu-system-aarch64, edk2-aarch64-code.fd and dist\buitenzorg-arm64.img)"
}
# Raspberry Pi 4 (roadmap R1): kernel8.img as the Pi firmware would start it
# (flat image, EL2, MMU off, x0 = the real Pi Device Tree) on QEMU `raspi4b`:
# GIC-400, PL011, the /soc address translation.
$kernel8 = Join-Path $root "dist\kernel8.img"
if ((Test-Path $qemuArm) -and (Test-Path $kernel8)) {
    $log = Join-Path $root "dist\boot-rpi4.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    $piDtb = Join-Path $root "kernel\bzkernel\testdata\bcm2711-rpi-4-b.dtb"
    $qargs = @('-M', 'raspi4b', '-m', '2G', '-kernel', "`"$kernel8`"", '-dtb', "`"$piDtb`"",
               '-display', 'none', '-serial', "`"file:$log`"", '-serial', 'null', '-no-reboot')
    # The SD card image as the card (R3), a copy: the kernel writes to /disk.
    $sdImg = Join-Path $root "dist\buitenzorg-rpi4.img"
    $haveSd = Test-Path $sdImg
    if ($haveSd) {
        $sdCopy = Join-Path $root "dist\rpi4-sd-test.img"
        Copy-Item $sdImg $sdCopy -Force
        $qargs += @('-drive', "`"file=$sdCopy,if=sd,format=raw`"")
    }
    $p = Start-Process -FilePath $qemuArm -ArgumentList ($qargs + @('-append', 'bz.selftest')) -PassThru -RedirectStandardError (Join-Path $root "dist\boot-rpi4.err")
    foreach ($i in 1..480) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "ARM64 READY" -Quiet)) { break }
    }
    Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction SilentlyContinue
    Assert-Markers -Log $log -Markers @("BREM", "framebuffer 1280x720", "ARM64 SMP OK (4 CPUs: spin tables", "window system: 1280x720", "MILESTONE: HELLO KERNEL OK", "loader=rpi-direct", "MILESTONE: DTB OK", "MILESTONE: ARM64 MEMORY OK", "MILESTONE: ARM64 ASPACE OK", "MILESTONE: ARM64 CONTEXT OK", "GICv2 at 0xff841000/0xff842000", "MILESTONE: ARM64 TIMER OK", "MILESTONE: ARM64 EL0 OK", "MILESTONE: SECURITY OK", "MILESTONE: GFX3D OK", "MILESTONE: RPI4 BOOT OK", "BUITENZORG ARM64 READY")
    if ($haveSd) { Assert-Markers -Log $log -Markers @("MILESTONE: SD CARD OK", "MILESTONE: ARM64 DISK OK (SD card", "wallpaper 'nature' loaded: true", "MILESTONE: MANAGED APPS OK", "Studio: 25 checks, 0 failed", "Lab Perangkat: 10 checks, 0 failed") }
    # Board I/O for applications, and the native sample when the C# programs were built.
    Assert-Markers -Log $log -Markers @("MILESTONE: RPI4 IO OK")
    if (Select-String -Path $log -Pattern "Hello from C#" -SimpleMatch -Quiet) { Assert-Markers -Log $log -Markers @("MILESTONE: HWDEMO OK") }
    Write-Host "smoke [rpi4]: kernel8.img on QEMU raspi4b: direct entry, GICv2, PL011, Device Tree ranges, EL0 programs, mailbox framebuffer + desktop, 4 cores via spin tables, /disk on the SD card"
} else {
    Skip-Step "smoke [rpi4]: skipped (needs qemu-system-aarch64 and dist\kernel8.img)"
}
} # aarch64

if (Test-Arch "riscv64") {
# RISC-V (multi-arch M4): the RISC-V kernel through Limine on QEMU `virt`
# (OpenSBI -> edk2 in pflash -> Limine; acpi=off so the DTB is handed over).
# Needs qemu-system-riscv64 + share\edk2-riscv-code.fd next to the x86 QEMU.
$qemuRv = Join-Path (Split-Path -Parent $qemu) "qemu-system-riscv64.exe"
$rvFw = Join-Path (Split-Path -Parent $qemu) "share\edk2-riscv-code.fd"
$rvVarsSrc = Join-Path (Split-Path -Parent $qemu) "share\edk2-riscv-vars.fd"
$rvImage = Join-Path $root "dist\buitenzorg-riscv64.img"
if ((Test-Path $qemuRv) -and (Test-Path $rvFw) -and (Test-Path $rvVarsSrc) -and (Test-Path $rvImage)) {
    $log = Join-Path $root "dist\boot-riscv64.log"
    $rvVars = Join-Path $root "dist\riscv64-vars.fd"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    Copy-Item $rvVarsSrc $rvVars -Force
    $qargs = @('-M', 'virt,acpi=off', '-cpu', 'rv64', '-smp', '2', '-m', '512M',
               '-drive', "`"if=pflash,format=raw,unit=0,file=$rvFw,readonly=on`"",
               '-drive', "`"if=pflash,format=raw,unit=1,file=$rvVars`"",
               '-global', 'virtio-mmio.force-legacy=false',
               '-drive', "`"id=d0,format=raw,file=$rvImage,if=none`"", '-device', 'virtio-blk-device,drive=d0',
               '-device', 'virtio-gpu-device', '-device', 'virtio-keyboard-device', '-device', 'virtio-tablet-device',
               '-netdev', 'user,id=n0', '-device', 'virtio-net-device,netdev=n0', '-device', 'ramfb',
               '-monitor', 'tcp:127.0.0.1:4484,server,nowait', '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemuRv -ArgumentList ($qargs + $selftest) -PassThru
    foreach ($i in 1..420) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "RISCV READY" -Quiet)) { break }
    }
    # The interactive desktop (shared desktop loop): type + move/click through
    # the virtio keyboard / tablet.
    Start-Sleep -Seconds 3
    Send-VirtioInput -Log $log -Port 4484
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers @("MILESTONE: HELLO KERNEL OK", "loader=limine", "loader DTB: parsed", "MILESTONE: DTB OK", "MILESTONE: RISCV MEMORY OK", "MILESTONE: MEMORY OK", "MILESTONE: RISCV ASPACE OK", "MILESTONE: RISCV CONTEXT OK", "MILESTONE: RISCV TIMER OK", "Hello from U-mode", "MILESTONE: RISCV UMODE OK", "Hello from Rust on RISC-V", "MILESTONE: RISCV ELF OK", "MILESTONE: RISCV USERMEM OK", "MILESTONE: RISCV MULTIPROC OK", "MILESTONE: RISCV THREAD OK", "MILESTONE: RISCV SYNC OK", "MILESTONE: RISCV PROCESSES OK", "MILESTONE: CRASH REPORT OK", "MILESTONE: CLR OK", "MILESTONE: CLR GUI OK", "MILESTONE: MANAGED UI OK", "MILESTONE: MANAGED APPS OK", "Studio: 25 checks, 0 failed", "HelloGui: live clock ticked", "Hello from managed C# on Buitenzorg (RustCLR)!", "checks=515 failures=", "checks=35 failures=0", "MILESTONE: RISCV SMP OK (2 harts", "MILESTONE: RISCV SMP SCHED OK", "MILESTONE: RISCV RTC OK", "MILESTONE: VIRTIO BLK OK (virtio-mmio", "MILESTONE: RISCV DISK OK", "MILESTONE: RISCV PLIC OK", "user programs from /disk", "MILESTONE: VIRTIO GPU OK", "MILESTONE: SECURITY OK", "MILESTONE: GFX3D OK", "MILESTONE: RISCV POWER OK", "MILESTONE: NETWORK OK", "MILESTONE: VIRTIO NET OK", "MILESTONE: RISCV POLYGLOT OK", "MILESTONE: RISCV PROCCTL OK", "MILESTONE: RISCV GUI OK", "MILESTONE: RISCV DESKTOP FRAME OK", "MILESTONE: USERS OK", "MILESTONE: TCP OK", "MILESTONE: FOLDERS OK", "MILESTONE: BZPKG OK", "MILESTONE: BOOT SPLASH OK (", "[desktop] tidied", "MILESTONE: VIRTIO INPUT OK", "MILESTONE: RISCV BOOT OK", "BUITENZORG RISCV READY")
    Write-Host "smoke [riscv64]: RISC-V kernel via Limine: S-mode under OpenSBI, live DTB, NS16550, Sv48 paging + heap + address spaces, SBI timer, context switch, U-mode programs + ECALL, ELF programs as preemptive processes + threads + futexes, PLIC, Goldfish RTC, virtio-mmio /disk + GPU + input + net, shared syscalls + window system, JS/TS/Python, interactive desktop, 2 harts (SBI HSM)"
    # The file the kernel wrote to /disk must be inside the image file.
    # (QEMU must be gone first: it holds the image open.)
    $p.WaitForExit(15000) | Out-Null
    $rvStamp = Select-String -Path $log -Pattern 'RISCV\.TXT = (BZRISCV[^\r\n]*)' | Select-Object -First 1
    if (-not $rvStamp) { Write-Error "SMOKE TEST FAILED: RISC-V boot did not report RISCV.TXT"; exit 1 }
    $needle = $rvStamp.Matches[0].Groups[1].Value
    if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($rvImage)).Contains($needle)) { Write-Error "SMOKE TEST FAILED: '$needle' not found in $rvImage"; exit 1 }
    Write-Host "smoke [riscv64]: /disk write persisted to the image ($needle)"
} else {
    Skip-Step "smoke [riscv64]: skipped (needs qemu-system-riscv64, edk2-riscv-code.fd/-vars.fd and dist\buitenzorg-riscv64.img)"
}
} # riscv64

# Managed C#: RustCLR's conformance suite (CONFORM.DLL, 499 checks) must pass
# every check except "blocked workers do not stall the queue" -- a wall-clock
# assertion (32 tasks sleeping 200 ms must finish within 1 s; ~0.85 s even on
# .NET itself) that QEMU's TCG emulation can miss, because every pool worker
# it starts copies the interpreter, which is slow under emulation. Any other
# failed check fails the smoke test.
foreach ($pair in @(@("x86_64", "boot-ide.log"), @("aarch64", "boot-arm64.log"), @("riscv64", "boot-riscv64.log"))) {
    if (-not (Test-Arch $pair[0])) { continue }
    $f = Join-Path $root ("dist\" + $pair[1])
    if (-not (Test-Path $f)) { continue }
    $bad = Select-String -Path $f -Pattern '^FAIL ' | Where-Object { $_.Line -notmatch '^FAIL blocked workers do not stall the queue' }
    if ($bad) { Write-Error "SMOKE TEST FAILED ($($pair[1])): RustCLR conformance: $($bad[0].Line)"; exit 1 }
}

Write-Host "SMOKE TEST PASSED ($Arch)" -ForegroundColor Green
