#!/usr/bin/env bash
# The managed C# apps (RustCLR path): `dotnet build` every project under
# userland/clr-apps and copy the assembly to userland/clr-apps/dist under its
# 8.3 /disk name. The results are committed, so images build without the
# .NET SDK; run this after changing an app. RustCLR's own conformance suites
# are copied too when a RustNetRuntime checkout is found (RUSTNET_ROOT).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
apps="$root/userland/clr-apps"
dist="$apps/dist"
mkdir -p "$dist"
# project name -> 8.3 file name on /disk
declare -A names=([HelloClr]=HELLOCLR.DLL [Buitenzorg.App]=BZAPP.DLL [Buitenzorg.UI]=BZUI.DLL [HelloGui]=HELLOGUI.DLL [Buitenzorg.Three]=BZTHREE.DLL [Buitenzorg.Game]=BZGAME.DLL)
for proj in "$apps"/*/*.csproj; do
    name="$(basename "$proj" .csproj)"
    out="${names[$name]:-$(echo "${name:0:8}" | tr '[:lower:]' '[:upper:]').DLL}"
    echo "==> dotnet build $name -> $out"
    dotnet build "$proj" -c Release -nologo -v q
    cp "$(dirname "$proj")/bin/Release/net10.0/$name.dll" "$dist/$out"
done
rustnet="${RUSTNET_ROOT:-$root/../../Vibe_Runtime/RustNetRuntime}"
# The RustCLR fixtures (CONFORM.DLL, MODERN.DLL) are committed, built from the
# commit clrhost pins; rebuilding them from a checkout that is at another
# commit (or has uncommitted work) gives assemblies the pinned runtime cannot
# run. Only on request: BZ_REBUILD_FIXTURES=1 with the checkout at the pin.
if [ "${BZ_REBUILD_FIXTURES:-0}" = "1" ] && [ -d "$rustnet/tests/fixtures" ]; then
    for pair in Conformance:CONFORM.DLL ModernSyntax:MODERN.DLL; do
        fx="${pair%%:*}"; out="${pair#*:}"
        echo "==> RustCLR fixture $fx -> $out"
        dotnet build "$rustnet/tests/fixtures/$fx/$fx.csproj" -c Release -nologo -v q
        cp "$rustnet/tests/fixtures/$fx/bin/Release/net10.0/$fx.dll" "$dist/$out"
    done
fi
ls -la "$dist"
