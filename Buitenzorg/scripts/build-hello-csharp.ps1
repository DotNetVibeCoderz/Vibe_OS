# Build the v0.4 "Tunas" milestone program: compile hello.cs (C#) to a
# freestanding static ELF that runs in ring 3 on Buitenzorg.
#
# Pipeline: bflat (C# -> object, zerolib) + rustc (startup/PAL shim -> object)
# then rust-lld links both into a no-interpreter static ELF.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "userland\hello-csharp"
$bflat = Join-Path $root "tools\bflat\bflat.exe"

$lld = Get-ChildItem "$env:USERPROFILE\.rustup\toolchains\nightly-*\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe" |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $lld) { throw "rust-lld not found (install rust nightly)" }

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
        @{src=@("taskmgr.cs", "bzdraw.cs"); elf="taskmgr.elf"},
        @{src=@("widget.cs", "bzdraw.cs");  elf="widget.elf"},
        @{src=@("webview.cs", "bzdraw.cs"); elf="webview.elf"}
    )
    foreach ($prog in $programs) {
        $obj = [IO.Path]::ChangeExtension($prog.elf, ".o")
        Write-Host "==> bflat: $($prog.src -join ', ') -> $obj (zerolib)"
        & $bflat build @($prog.src) --stdlib:zero --os:linux --arch:x64 -c -Os `
            --no-debug-info --no-reflection --no-stacktrace-data -o $obj
        if ($LASTEXITCODE -ne 0) { throw "bflat failed for $($prog.elf)" }

        Write-Host "==> rust-lld: link -> $($prog.elf)"
        & $lld -flavor gnu -o $prog.elf -T user.ld --static --no-dynamic-linker `
            -e _start $obj bzstart.o
        if ($LASTEXITCODE -ne 0) { throw "link failed for $($prog.elf)" }
        Write-Host "==> $($prog.elf): $((Get-Item $prog.elf).Length) bytes" -ForegroundColor Green
    }
} finally { Pop-Location }
