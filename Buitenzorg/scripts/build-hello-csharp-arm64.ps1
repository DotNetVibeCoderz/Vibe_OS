# Build the ARM64 user programs (multi-arch M3): the same C# sources, compiled
# for linux-arm64 (zerolib; bflat build-il + the upstream ILCompiler, see
# build-hello-csharp.ps1) and linked with the aarch64 build of the
# bzstart.rs shim (SVC #0 syscalls) into static ELFs at 0x400000 under
# userland/hello-csharp/arm64/. bzimage puts them on dist/buitenzorg-arm64.img
# and the ARM64 kernel loads them from /disk (virtio-blk).
#
# arm64 code needs ThrowHelpers.ThrowOverflowException (division checks),
# which bflat's zerolib lacks: tools/ZerolibPatch adds it to a copy of
# zerolib, used by a private bflat copy in kernel/target/bflat-patched
# (scripts/prepare-bflat.ps1; the installed bflat is not modified).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "userland\hello-csharp"
$out = Join-Path $dir "arm64"
$bflat = & (Join-Path $PSScriptRoot "prepare-bflat.ps1") | Select-Object -Last 1
$ilc = & (Join-Path $PSScriptRoot "prepare-ilc.ps1") | Select-Object -Last 1
$zerolib = Join-Path (Split-Path -Parent $bflat) "lib\linux\arm64\zerolib.dll"
$lld = Get-ChildItem "$env:USERPROFILE\.rustup\toolchains\nightly-*\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe" |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $lld) { throw "rust-lld not found (install rust nightly)" }
New-Item -ItemType Directory -Force (Join-Path $out "il") | Out-Null

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
        @{src=@("crashme.cs");         elf="crashme.elf"},
        @{src=@("bcl3.cs", "bzbcl.cs", "bzbcl2.cs", "bzbcl3.cs"); elf="bcl3.elf"},
        @{src=@("async.cs", "bzbcl.cs", "bzbcl2.cs", "bzbcl3.cs", "bzasync.cs"); elf="async.elf"},
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
        @{src=@("hwdemo.cs", "bzhw.cs", "bzbcl.cs"); elf="hwdemo.elf"},
        @{src=@("exc.cs");             elf="exc.elf"},
        @{src=@("gc.cs");              elf="gc.elf"}
    )
    foreach ($prog in $programs) {
        $obj = "arm64/" + [IO.Path]::ChangeExtension($prog.elf, ".o")
        # IL goes to arm64/il/<name>.dll: the file name becomes the symbol prefix
        $il = "arm64/il/" + [IO.Path]::ChangeExtension($prog.elf, ".dll")
        Write-Host "==> bflat build-il + ilc (linux-arm64): $($prog.src -join ', ') -> $obj"
        & $bflat build-il @($prog.src) "bzexc.cs" --stdlib:zero -O -o $il
        if ($LASTEXITCODE -ne 0) { throw "bflat failed for $($prog.elf)" }
        & $ilc $il $zerolib --systemmodule:zerolib --targetos:linux --targetarch:arm64 `
            --Os --reflectiondata:none --directpinvoke:libSystem.Native -o $obj
        if ($LASTEXITCODE -ne 0) { throw "ilc failed for $($prog.elf)" }
        & $lld -flavor gnu -o "arm64/$($prog.elf)" -T user.ld --static --no-dynamic-linker `
            -e _start $obj arm64/bzstart.o
        if ($LASTEXITCODE -ne 0) { throw "link failed for $($prog.elf)" }
        Write-Host "==> arm64/$($prog.elf): $((Get-Item "arm64/$($prog.elf)").Length) bytes" -ForegroundColor Green
    }
} finally { Pop-Location }
