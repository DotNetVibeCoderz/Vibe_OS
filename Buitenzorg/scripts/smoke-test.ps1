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
    $p = Start-Process -FilePath $qemu -ArgumentList $qargs -PassThru
    foreach ($i in 1..130) {
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
           "MILESTONE: SECURITY OK", "MILESTONE: PROFILER OK", "MILESTONE: IOAPIC OK", "MILESTONE: DTB OK",
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

# Limine boot (multi-arch M1): the same kernel through the Limine protocol,
# UEFI, 2 CPUs, /disk on an MBR FAT partition. Needs OVMF + the Limine
# binaries (scripts/fetch-limine.ps1) so bzimage built the image.
$limineImage = Join-Path $root "dist\buitenzorg-limine-uefi.img"
if ((Test-Path (Join-Path $ovmf "code.fd")) -and (Test-Path $limineImage)) {
    $log = Join-Path $root "dist\boot-limine.log"
    if (Test-Path $log) { Remove-Item $log -Force -Confirm:$false }
    $qargs = @('-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'code.fd')",
               '-drive', "if=pflash,format=raw,readonly=on,file=$(Join-Path $ovmf 'vars.fd')",
               '-drive', "format=raw,file=$limineImage", '-smp', '2',
               '-m', '512M', '-audiodev', 'none,id=snd0', '-device', 'AC97,audiodev=snd0',
               '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemu -ArgumentList $qargs -PassThru
    foreach ($i in 1..150) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "BUITENZORG READY" -Quiet)) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers ($basic + @("[boot] loader=limine", "MILESTONE: SMP OK (2 CPU(s): 1 AP(s)", "MILESTONE: DISK WRITE OK", "MILESTONE: MULTIPROC OK"))
    Write-Host "smoke [limine]: Limine protocol boot (UEFI) to READY on 2 CPUs, /disk + processes"
} else {
    Write-Host "smoke [limine]: skipped (needs OVMF and scripts/fetch-limine.ps1 before the build)"
}

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
    $p = Start-Process -FilePath $qemuArm -ArgumentList $qargs -PassThru
    foreach ($i in 1..90) {
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
        "MILESTONE: ARM64 RTC OK", "MILESTONE: VIRTIO BLK OK (virtio-mmio", "MILESTONE: ARM64 DISK OK", "MILESTONE: VIRTIO GPU OK",
        "MILESTONE: SECURITY OK", "MILESTONE: ARM64 PSCI OK", "MILESTONE: NETWORK OK", "MILESTONE: VIRTIO NET OK",
        "[desktop] tidied", "MILESTONE: VIRTIO INPUT OK", "MILESTONE: ARM64 SMP OK (2 CPUs", "MILESTONE: ARM64 TIMER OK", "MILESTONE: ARM64 BOOT OK", "BUITENZORG ARM64 READY")
    if (Select-String -Path $log -Pattern "program 'hello'" -Quiet) {
        Assert-Markers -Log $log -Markers @("Hello from C#!", "MILESTONE: ARM64 CSHARP OK", "MILESTONE: MATANG OK",
            "MILESTONE: HEAP OK", "MILESTONE: GCMEM OK", "MILESTONE: ARM64 MULTIPROC OK", "MILESTONE: THREAD OK", "MILESTONE: SYNC OK", "frames leaked: 0", "from /disk",
            "MILESTONE: BCL OK", "MILESTONE: PROCCTL OK", "MILESTONE: CALC OK", "MILESTONE: GAME OK", "MILESTONE: UI OK",
            "MILESTONE: DRAW OK", "MILESTONE: ARM64 DESKTOP FRAME OK", "MILESTONE: ARM64 SMP SCHED OK")
    }
    Write-Host "smoke [arm64]: ARM64 kernel via Limine: live DTB, PL011, GICv3 + timer, paging + heap + address spaces, threads + EL0 programs, C# apps, preemptive processes + threads, virtio-mmio disk + GPU + net, PL031, PSCI, shared syscalls, C# GUI apps + interactive desktop, 2 CPUs (PSCI)"
} else {
    Write-Host "smoke [arm64]: skipped (needs qemu-system-aarch64, edk2-aarch64-code.fd and dist\buitenzorg-arm64.img)"
}

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
    $qargs = @('-M', 'virt,acpi=off', '-cpu', 'rv64', '-m', '512M',
               '-drive', "`"if=pflash,format=raw,unit=0,file=$rvFw,readonly=on`"",
               '-drive', "`"if=pflash,format=raw,unit=1,file=$rvVars`"",
               '-global', 'virtio-mmio.force-legacy=false',
               '-drive', "`"id=d0,format=raw,file=$rvImage,if=none`"", '-device', 'virtio-blk-device,drive=d0',
               '-device', 'virtio-gpu-device', '-device', 'ramfb', '-display', 'none', '-serial', "file:$log", '-no-reboot')
    $p = Start-Process -FilePath $qemuRv -ArgumentList $qargs -PassThru
    foreach ($i in 1..90) {
        Start-Sleep -Seconds 1
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern "RISCV READY" -Quiet)) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Assert-Markers -Log $log -Markers @("MILESTONE: HELLO KERNEL OK", "loader=limine", "loader DTB: parsed", "MILESTONE: DTB OK", "MILESTONE: RISCV MEMORY OK", "MILESTONE: MEMORY OK", "MILESTONE: RISCV ASPACE OK", "MILESTONE: RISCV CONTEXT OK", "MILESTONE: RISCV TIMER OK", "Hello from U-mode", "MILESTONE: RISCV UMODE OK", "Hello from Rust on RISC-V", "MILESTONE: RISCV ELF OK", "MILESTONE: RISCV USERMEM OK", "MILESTONE: RISCV MULTIPROC OK", "MILESTONE: RISCV THREAD OK", "MILESTONE: RISCV SYNC OK", "MILESTONE: RISCV PROCESSES OK", "MILESTONE: RISCV RTC OK", "MILESTONE: VIRTIO BLK OK (virtio-mmio", "MILESTONE: RISCV DISK OK", "MILESTONE: RISCV PLIC OK", "user programs from /disk", "MILESTONE: VIRTIO GPU OK", "MILESTONE: RISCV BOOT OK", "BUITENZORG RISCV READY")
    Write-Host "smoke [riscv64]: RISC-V kernel via Limine: S-mode under OpenSBI, live DTB, NS16550, Sv48 paging + heap + address spaces, SBI timer, context switch, U-mode programs + ECALL, ELF programs as preemptive processes + threads + futexes, PLIC, Goldfish RTC, virtio-mmio /disk + GPU"
    # The file the kernel wrote to /disk must be inside the image file.
    # (QEMU must be gone first: it holds the image open.)
    $p.WaitForExit(15000) | Out-Null
    $rvStamp = Select-String -Path $log -Pattern 'RISCV\.TXT = (BZRISCV[^\r\n]*)' | Select-Object -First 1
    if (-not $rvStamp) { Write-Error "SMOKE TEST FAILED: RISC-V boot did not report RISCV.TXT"; exit 1 }
    $needle = $rvStamp.Matches[0].Groups[1].Value
    if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($rvImage)).Contains($needle)) { Write-Error "SMOKE TEST FAILED: '$needle' not found in $rvImage"; exit 1 }
    Write-Host "smoke [riscv64]: /disk write persisted to the image ($needle)"
} else {
    Write-Host "smoke [riscv64]: skipped (needs qemu-system-riscv64, edk2-riscv-code.fd/-vars.fd and dist\buitenzorg-riscv64.img)"
}

Write-Host "SMOKE TEST PASSED: ide/ahci/nvme/usb" -ForegroundColor Green
