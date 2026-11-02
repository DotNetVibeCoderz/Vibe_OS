# The managed C# apps (RustCLR path): `dotnet build` every project under
# userland/clr-apps and copy the assembly to userland/clr-apps/dist under its
# 8.3 /disk name. The results are committed, so images build without the
# .NET SDK; run this after changing an app. RustCLR's modern-syntax suite is
# copied too when a RustNetRuntime checkout is found ($env:RUSTNET_ROOT); its
# main Conformance suite needs threads, which the managed host does not have yet.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$apps = Join-Path $root "userland\clr-apps"
$dist = Join-Path $apps "dist"
New-Item -ItemType Directory -Force $dist | Out-Null
$names = @{ "HelloClr" = "HELLOCLR.DLL" }
foreach ($proj in Get-ChildItem -Path $apps -Filter *.csproj -Recurse) {
    $name = $proj.BaseName
    $out = if ($names.ContainsKey($name)) { $names[$name] } else { ($name.Substring(0, [Math]::Min(8, $name.Length))).ToUpper() + ".DLL" }
    Write-Host "==> dotnet build $name -> $out"
    dotnet build $proj.FullName -c Release -nologo -v q
    if ($LASTEXITCODE -ne 0) { throw "dotnet build failed: $name" }
    Copy-Item (Join-Path $proj.DirectoryName "bin\Release\net10.0\$name.dll") (Join-Path $dist $out) -Force
}
$rustnet = if ($env:RUSTNET_ROOT) { $env:RUSTNET_ROOT } else { Join-Path $root "..\..\Vibe_Runtime\RustNetRuntime" }
if (Test-Path (Join-Path $rustnet "tests\fixtures")) {
    foreach ($pair in @(,@("ModernSyntax", "MODERN.DLL"))) {
        $fx = $pair[0]; $out = $pair[1]
        Write-Host "==> RustCLR fixture $fx -> $out"
        dotnet build (Join-Path $rustnet "tests\fixtures\$fx\$fx.csproj") -c Release -nologo -v q
        if ($LASTEXITCODE -ne 0) { throw "dotnet build failed: $fx" }
        Copy-Item (Join-Path $rustnet "tests\fixtures\$fx\bin\Release\net10.0\$fx.dll") (Join-Path $dist $out) -Force
    }
}
Get-ChildItem $dist
