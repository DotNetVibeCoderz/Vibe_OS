#!/usr/bin/env bash
# Find (or download) the upstream NativeAOT compiler, ILCompiler 8.0.26, that
# turns the native C# programs' IL into machine code. bflat bundles the same
# compiler but switches its exception-handling and unwind tables off for
# every non-dotnet standard library, so `throw` / `catch` could not work;
# the upstream ILC emits them (scripts/build-hello-csharp.sh compiles with
# `bflat build-il` and this ILC). Prints the path of ilc last.
#
# The package comes from the NuGet cache when a .NET SDK already has it,
# else from nuget.org into kernel/target/ilc, checked against a pinned
# SHA-256.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version="8.0.26"
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        id="runtime.win-x64.microsoft.dotnet.ilcompiler"; exe="ilc.exe"
        sha256="8528bdeb6225b81b7f3682a076f76afe6b5548e3ea2ba0cdcc0b4e4ad3b29e95"
        home="${USERPROFILE:-$HOME}" ;;
    *)
        id="runtime.linux-x64.microsoft.dotnet.ilcompiler"; exe="ilc"
        sha256="e800825c86f702cad2cc877d898eb42c6265d3627eaabb60da498830f4db7dd3"
        home="$HOME" ;;
esac

cached="$home/.nuget/packages/$id/$version/tools/$exe"
if [ -f "$cached" ]; then echo "$cached"; exit 0; fi

dir="$root/kernel/target/ilc/$version"
ilc="$dir/tools/$exe"
if [ -f "$ilc" ]; then echo "$ilc"; exit 0; fi

mkdir -p "$dir"
pkg="$dir/ilc.nupkg"
echo "==> downloading ILCompiler $version (~60 MB)" >&2
curl -fsSL -o "$pkg" "https://api.nuget.org/v3-flatcontainer/$id/$version/$id.$version.nupkg"
got="$(sha256sum "$pkg" | cut -d' ' -f1)"
if [ "$got" != "$sha256" ]; then rm -f "$pkg"; echo "ILCompiler package hash mismatch ($got)" >&2; exit 1; fi
(cd "$dir" && unzip -q -o ilc.nupkg 'tools/*') >&2
rm -f "$pkg"
chmod +x "$ilc" 2>/dev/null || true
[ -f "$ilc" ] || { echo "ilc not found in the package" >&2; exit 1; }
echo "$ilc"
