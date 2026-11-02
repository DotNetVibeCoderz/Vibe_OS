#!/usr/bin/env bash
# The managed C# runtime host (RustCLR, userland/clrhost) for every
# architecture: userland/clrhost/target/<target>/release/clrhost, which
# bzimage packs as /disk/CLRHOST.ELF into the matching image. Fetches RustCLR
# (github.com/DotNetVibeCoderz/Vibe_Runtime, pinned commit) on first use.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/userland/clrhost"
for target in x86_64-unknown-none aarch64-unknown-none riscv64imac-unknown-none-elf; do
    echo "==> clrhost ($target)"
    cargo build --release --target "$target"
done
