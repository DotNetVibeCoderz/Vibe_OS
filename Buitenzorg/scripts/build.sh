#!/usr/bin/env bash
# Buitenzorg OS - full build (Linux/macOS/CI).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"

if [ -x "$root/tools/bflat/bflat" ]; then
    echo "==> user program (C# -> ELF)"
    "$root/scripts/build-hello-csharp.sh"
else
    echo "==> skipping C# user program (tools/bflat not installed)"
fi

echo "==> kernel (Rust)"
(cd "$root/kernel" && cargo build --release && cargo run --release -p bzimage -- --out "$root/dist")

echo "==> runtime + sdk (.NET)"
dotnet build "$root/Buitenzorg.slnx" -c Release

echo "==> done. images in dist/"
