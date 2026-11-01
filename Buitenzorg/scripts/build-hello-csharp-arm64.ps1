# Build the ARM64 user programs (multi-arch M3): the same C# sources, compiled
# by bflat for linux-arm64 (zerolib) and linked with the aarch64 build of the
# bzstart.rs shim (SVC #0 syscalls) into static ELFs at 0x400000 under
# userland/hello-csharp/arm64/. bzimage puts them on dist/buitenzorg-arm64.img
# and Limine loads them as boot modules (the ARM64 kernel has no disk driver
# yet). Only programs whose syscalls the ARM64 kernel serves are listed.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "userland\hello-csharp"
$out = Join-Path $dir "arm64"
$bflat = Join-Path $root "tools\bflat\bflat.exe"
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
        @{src=@("gcmem.cs");           elf="gcmem.elf"}
        # bcl.cs: not yet -- bflat's arm64 codegen needs
        # ThrowHelpers.ThrowOverflowException, which zerolib lacks.
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
