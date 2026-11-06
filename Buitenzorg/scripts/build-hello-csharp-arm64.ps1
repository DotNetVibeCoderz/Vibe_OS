# Build the ARM64 user programs (multi-arch M3): the same C# sources, compiled
# by bflat for linux-arm64 (zerolib) and linked with the aarch64 build of the
# bzstart.rs shim (SVC #0 syscalls) into static ELFs at 0x400000 under
# userland/hello-csharp/arm64/. bzimage puts them on dist/buitenzorg-arm64.img
# and the ARM64 kernel loads them from /disk (virtio-blk).
#
# arm64 code needs ThrowHelpers.ThrowOverflowException (division checks),
# which bflat's zerolib lacks: tools/ZerolibPatch adds it to a copy of
# zerolib, used by a private bflat copy in kernel/target/bflat-arm64 (the
# installed bflat is not modified).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "userland\hello-csharp"
$out = Join-Path $dir "arm64"
$bflatHome = Join-Path $root "tools\bflat"
$patched = Join-Path $root "kernel\target\bflat-arm64"
$bflat = Join-Path $patched "bflat.exe"
if (-not (Test-Path $bflat)) {
    Write-Host "==> preparing a bflat copy with the patched arm64 zerolib"
    New-Item -ItemType Directory -Force (Split-Path -Parent $patched) | Out-Null
    Copy-Item -Recurse -Force $bflatHome $patched
    & dotnet run --project (Join-Path $root "tools\ZerolibPatch") -c Release -- `
        (Join-Path $bflatHome "lib\linux\arm64\zerolib.dll") (Join-Path $patched "lib\linux\arm64\zerolib.dll")
    if ($LASTEXITCODE -ne 0) { Remove-Item -Recurse -Force $patched; throw "ZerolibPatch failed" }
}
$lld = Get-ChildItem "$env:USERPROFILE\.rustup\toolchains\nightly-*\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe" |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $lld) { throw "rust-lld not found (install rust nightly)" }
New-Item -ItemType Directory -Force $out | Out-Null

Push-Location $dir
try {
    Write-Host "==> rustc: bzstart.rs -> arm64/bzstart.o (aarch64 shim)"
    & rustc +nightly --edition 2021 --crate-type staticlib --emit obj `
        --target aarch64-unknown-none -C panic=abort -C opt-level=2 `
        -o arm64/bzstart.o bzstart.rs
    if ($LASTEXITCODE -ne 0) { throw "rustc failed" }

    $programs = @(
        @{src=@("hello.cs");           elf="hello.elf"},
        @{src=@("matang.cs");          elf="matang.elf"},
        @{src=@("heap.cs");            elf="heap.elf"},
        @{src=@("statics.cs");         elf="statics.elf"},
        @{src=@("gcmem.cs");           elf="gcmem.elf"},
        @{src=@("mproc.cs");           elf="mproc.elf"},
        @{src=@("thread.cs");          elf="thread.elf"},
        @{src=@("sync.cs");            elf="sync.elf"},
        @{src=@("bcl.cs", "bzbcl.cs"); elf="bcl.elf"},
        @{src=@("proctest.cs", "bzbcl.cs", "bzbcl2.cs"); elf="proctest.elf"},
        @{src=@("calc.cs", "bzui.cs", "bzgfx.cs"); elf="calc.elf"},
        @{src=@("game2048.cs", "bzui.cs", "bzgfx.cs"); elf="g2048.elf"},
        @{src=@("ui.cs", "bzui.cs", "bzgfx.cs"); elf="ui.elf"},
        @{src=@("draw.cs", "bzgfx.cs"); elf="draw.elf"},
        @{src=@("xox.cs");             elf="xox.elf"},
        @{src=@("hwdemo.cs", "bzhw.cs", "bzbcl.cs"); elf="hwdemo.elf"}
    )
    foreach ($prog in $programs) {
        $obj = "arm64/" + [IO.Path]::ChangeExtension($prog.elf, ".o")
        Write-Host "==> bflat (linux-arm64): $($prog.src -join ', ') -> $obj"
        & $bflat build @($prog.src) --stdlib:zero --os:linux --arch:arm64 -c -Os `
            --no-debug-info --no-reflection --no-stacktrace-data -o $obj
        if ($LASTEXITCODE -ne 0) { throw "bflat failed for $($prog.elf)" }
        & $lld -flavor gnu -o "arm64/$($prog.elf)" -T user.ld --static --no-dynamic-linker `
            -e _start $obj arm64/bzstart.o
        if ($LASTEXITCODE -ne 0) { throw "link failed for $($prog.elf)" }
        Write-Host "==> arm64/$($prog.elf): $((Get-Item "arm64/$($prog.elf)").Length) bytes" -ForegroundColor Green
    }
} finally { Pop-Location }
