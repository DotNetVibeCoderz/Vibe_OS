# Build the v0.4 "Tunas" milestone program: compile hello.cs (C#) to a
# freestanding static ELF that runs in ring 3 on Buitenzorg.
#
# Pipeline: `bflat build-il` (C# -> IL, zerolib) + the upstream ILCompiler
# (IL -> object with unwind + exception-clause tables, which bflat leaves out
# for zerolib) + rustc (startup/PAL shim with the garbage collector and the
# exception dispatcher -> object), then rust-lld links them into a
# no-interpreter static ELF. Every program gets bzexc.cs (System.Exception
# and friends).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "userland\hello-csharp"
# a private bflat copy whose zerolib is patched (scripts/prepare-bflat.ps1)
$bflat = & (Join-Path $PSScriptRoot "prepare-bflat.ps1") | Select-Object -Last 1
$ilc = & (Join-Path $PSScriptRoot "prepare-ilc.ps1") | Select-Object -Last 1
$zerolib = Join-Path (Split-Path -Parent $bflat) "lib\linux\x64\zerolib.dll"

$lld = Get-ChildItem "$env:USERPROFILE\.rustup\toolchains\nightly-*\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe" |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $lld) { throw "rust-lld not found (install rust nightly)" }

New-Item -ItemType Directory -Force (Join-Path $dir "il") | Out-Null
Push-Location $dir
try {
    Write-Host "==> rustc: bzstart.rs -> bzstart.o (freestanding shim)"
    & rustc +nightly --edition 2021 --crate-type staticlib --emit obj `
        --target x86_64-unknown-none -C panic=abort -C opt-level=2 `
        -o bzstart.o bzstart.rs
    if ($LASTEXITCODE -ne 0) { throw "rustc failed" }

    # Build each C# program: source(s) -> object (bflat) -> static ELF (rust-lld).
    # `src` may list several .cs files (e.g. a shared library like bzdraw.cs).
    $programs = @(
        @{src=@("hello.cs");             elf="hello.elf"},
        @{src=@("svc.cs");               elf="svc.elf"},
        @{src=@("xox.cs");               elf="xox.elf"},
        @{src=@("paint.cs", "bzdraw.cs");   elf="paint.elf"},
        @{src=@("taskmgr.cs", "bzdraw.cs", "bzbcl.cs", "bzbcl2.cs"); elf="taskmgr.elf"},
        @{src=@("widget.cs", "bzdraw.cs", "bzbcl.cs", "bzbcl2.cs"); elf="widget.elf"},
        @{src=@("webview.cs", "bzdraw.cs"); elf="webview.elf"},
        @{src=@("matang.cs");            elf="matang.elf"},
        @{src=@("thread.cs");            elf="thread.elf"},
        @{src=@("sync.cs");              elf="sync.elf"},
        @{src=@("heap.cs");              elf="heap.elf"},
        @{src=@("statics.cs");           elf="statics.elf"},
        @{src=@("crashme.cs");           elf="crashme.elf"},
        @{src=@("bcl3.cs", "bzbcl.cs", "bzbcl2.cs", "bzbcl3.cs"); elf="bcl3.elf"},
        @{src=@("bcl4.cs", "bzbcl.cs", "bzbcl2.cs", "bzbcl3.cs", "bzbcl4.cs"); elf="bcl4.elf"},
        @{src=@("async.cs", "bzbcl.cs", "bzbcl2.cs", "bzbcl3.cs", "bzasync.cs"); elf="async.elf"},
        @{src=@("gcmem.cs");             elf="gcmem.elf"},
        @{src=@("bcl.cs", "bzbcl.cs");   elf="bcl.elf"},
        @{src=@("bcl2.cs", "bzbcl.cs", "bzbcl2.cs"); elf="bcl2.elf"},
        @{src=@("draw.cs", "bzgfx.cs");  elf="draw.elf"},
        @{src=@("ui.cs", "bzui.cs", "bzgfx.cs"); elf="ui.elf"},
        @{src=@("audio.cs", "bzaudio.cs"); elf="audio.elf"},
        @{src=@("audiopanel.cs", "bzui.cs", "bzgfx.cs", "bzaudio.cs"); elf="audioset.elf"},
        @{src=@("calc.cs", "bzui.cs", "bzgfx.cs"); elf="calc.elf"},
        @{src=@("game2048.cs", "bzui.cs", "bzgfx.cs"); elf="g2048.elf"},
        @{src=@("clock.cs", "bzui.cs", "bzgfx.cs", "bzbcl.cs", "bzbcl2.cs"); elf="clock.elf"},
        @{src=@("piano.cs", "bzui.cs", "bzgfx.cs", "bzaudio.cs"); elf="piano.elf"},
        @{src=@("store.cs", "bzui.cs", "bzgfx.cs", "bzbcl.cs", "bzbcl2.cs"); elf="store.elf"},
        @{src=@("filemgr.cs", "bzui.cs", "bzgfx.cs", "bzbcl.cs", "bzbcl2.cs"); elf="files.elf"},
        @{src=@("editor.cs", "bzui.cs", "bzgfx.cs", "bzbcl.cs", "bzbcl2.cs"); elf="editor.elf"},
        @{src=@("imgview.cs", "bzui.cs", "bzgfx.cs", "bzbcl.cs", "bzbcl2.cs"); elf="imgview.elf"},
        @{src=@("jpgtest.cs", "bzgfx.cs"); elf="jpgtest.elf"},
        @{src=@("mproc.cs");             elf="mproc.elf"},
        @{src=@("smpsys.cs");            elf="smpsys.elf"},
        @{src=@("smpgui.cs");            elf="smpgui.elf"},
        @{src=@("pthread.cs");           elf="pthread.elf"},
        @{src=@("smpfs.cs");             elf="smpfs.elf"},
        @{src=@("smpproc.cs");           elf="smpproc.elf"},
        @{src=@("proctest.cs", "bzbcl.cs", "bzbcl2.cs"); elf="proctest.elf"},
        @{src=@("exc.cs");               elf="exc.elf"},
        @{src=@("gc.cs");                elf="gc.elf"}
    )
    foreach ($prog in $programs) {
        $obj = [IO.Path]::ChangeExtension($prog.elf, ".o")
        # IL goes to il/<name>.dll: the file name becomes the symbol prefix
        $il = "il/" + [IO.Path]::ChangeExtension($prog.elf, ".dll")
        Write-Host "==> bflat build-il: $($prog.src -join ', ') -> $il (zerolib)"
        & $bflat build-il @($prog.src) "bzexc.cs" --stdlib:zero -O -o $il
        if ($LASTEXITCODE -ne 0) { throw "bflat failed for $($prog.elf)" }
        Write-Host "==> ilc: $il -> $obj"
        & $ilc $il $zerolib --systemmodule:zerolib --targetos:linux --targetarch:x64 `
            --Os --reflectiondata:none --directpinvoke:libSystem.Native -o $obj
        if ($LASTEXITCODE -ne 0) { throw "ilc failed for $($prog.elf)" }

        Write-Host "==> rust-lld: link -> $($prog.elf)"
        & $lld -flavor gnu -o $prog.elf -T user.ld --static --no-dynamic-linker `
            -e _start $obj bzstart.o
        if ($LASTEXITCODE -ne 0) { throw "link failed for $($prog.elf)" }
        Write-Host "==> $($prog.elf): $((Get-Item $prog.elf).Length) bytes" -ForegroundColor Green
    }
} finally { Pop-Location }
