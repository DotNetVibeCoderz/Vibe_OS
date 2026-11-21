# Find (or download) the upstream NativeAOT compiler, ILCompiler 8.0.26, that
# turns the native C# programs' IL into machine code. bflat bundles the same
# compiler but switches its exception-handling and unwind tables off for
# every non-dotnet standard library, so `throw` / `catch` could not work;
# the upstream ILC emits them (scripts/build-hello-csharp.ps1 compiles with
# `bflat build-il` and this ILC). Prints the path of ilc.exe last.
#
# The package comes from the NuGet cache when a .NET SDK already has it,
# else from nuget.org into kernel/target/ilc, checked against a pinned
# SHA-256.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$version = "8.0.26"
$id = "runtime.win-x64.microsoft.dotnet.ilcompiler"
$sha256 = "8528bdeb6225b81b7f3682a076f76afe6b5548e3ea2ba0cdcc0b4e4ad3b29e95"

$cached = Join-Path $env:USERPROFILE ".nuget\packages\$id\$version\tools\ilc.exe"
if (Test-Path $cached) { Write-Output $cached; return }

$dir = Join-Path $root "kernel\target\ilc\$version"
$ilc = Join-Path $dir "tools\ilc.exe"
if (Test-Path $ilc) { Write-Output $ilc; return }

New-Item -ItemType Directory -Force $dir | Out-Null
$pkg = Join-Path $dir "ilc.nupkg"
$url = "https://api.nuget.org/v3-flatcontainer/$id/$version/$id.$version.nupkg"
Write-Host "==> downloading ILCompiler $version (~60 MB)"
Invoke-WebRequest -Uri $url -OutFile $pkg -UseBasicParsing
$got = (Get-FileHash -Algorithm SHA256 $pkg).Hash.ToLower()
if ($got -ne $sha256) { Remove-Item $pkg; throw "ILCompiler package hash mismatch ($got)" }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::ExtractToDirectory($pkg, $dir)
Remove-Item $pkg
if (-not (Test-Path $ilc)) { throw "ilc.exe not found in the package" }
Write-Output $ilc
