# The managed C# runtime host (RustCLR, userland/clrhost) for every
# architecture: userland/clrhost/target/<target>/release/clrhost, which
# bzimage packs as /disk/CLRHOST.ELF into the matching image. Fetches RustCLR
# (github.com/DotNetVibeCoderz/Vibe_Runtime, pinned commit) on first use.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location (Join-Path $root "userland\clrhost")
try {
    foreach ($target in @("x86_64-unknown-none", "aarch64-unknown-none", "riscv64imac-unknown-none-elf")) {
        Write-Host "==> clrhost ($target)"
        cargo build --release --target $target
        if ($LASTEXITCODE -ne 0) { throw "clrhost build failed for $target" }
    }
} finally {
    Pop-Location
}
