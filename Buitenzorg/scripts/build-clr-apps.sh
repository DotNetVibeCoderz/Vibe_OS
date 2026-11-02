#!/usr/bin/env bash
# The managed C# apps (RustCLR path): `dotnet build` every project under
# userland/clr-apps and copy the assembly to userland/clr-apps/dist under its
# 8.3 /disk name. The results are committed, so images build without the
# .NET SDK; run this after changing an app. RustCLR's modern-syntax suite is
# copied too when a RustNetRuntime checkout is found (RUSTNET_ROOT); its main
# Conformance suite needs threads, which the managed host does not have yet.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
apps="$root/userland/clr-apps"
dist="$apps/dist"
mkdir -p "$dist"
# project name -> 8.3 file name on /disk
declare -A names=([HelloClr]=HELLOCLR.DLL)
for proj in "$apps"/*/*.csproj; do
    name="$(basename "$proj" .csproj)"
    out="${names[$name]:-$(echo "${name:0:8}" | tr '[:lower:]' '[:upper:]').DLL}"
    echo "==> dotnet build $name -> $out"
    dotnet build "$proj" -c Release -nologo -v q
    cp "$(dirname "$proj")/bin/Release/net10.0/$name.dll" "$dist/$out"
done
rustnet="${RUSTNET_ROOT:-$root/../../Vibe_Runtime/RustNetRuntime}"
if [ -d "$rustnet/tests/fixtures" ]; then
    for pair in ModernSyntax:MODERN.DLL; do
        fx="${pair%%:*}"; out="${pair#*:}"
        echo "==> RustCLR fixture $fx -> $out"
        dotnet build "$rustnet/tests/fixtures/$fx/$fx.csproj" -c Release -nologo -v q
        cp "$rustnet/tests/fixtures/$fx/bin/Release/net10.0/$fx.dll" "$dist/$out"
    done
fi
ls -la "$dist"
