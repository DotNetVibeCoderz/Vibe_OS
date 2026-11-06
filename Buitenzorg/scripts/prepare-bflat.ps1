# Prepare the private bflat copy the C# build scripts use:
# kernel/target/bflat-patched = tools/bflat with zerolib.dll (linux x64 and
# arm64) run through tools/ZerolibPatch (adds ThrowOverflowException for
# arm64 division checks; renames zerolib's RhpStelemRef export so the shim's
# covariant array store is used). The installed bflat is never modified.
# Prints the path of the patched bflat executable.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bflatHome = Join-Path $root "tools\bflat"
$patched = Join-Path $root "kernel\target\bflat-patched"
$bflat = Join-Path $patched "bflat.exe"
$patchVersion = "2"   # bump whenever tools/ZerolibPatch changes what it does
$marker = Join-Path $patched "ZEROLIB_PATCH"
$current = if (Test-Path $marker) { (Get-Content $marker -Raw).Trim() } else { "" }
if (-not (Test-Path $bflat) -or $current -ne $patchVersion) {
    Write-Host "==> preparing a bflat copy with the patched zerolib (x64 + arm64, patch v$patchVersion)"
    if (Test-Path $patched) { Remove-Item -Recurse -Force $patched }
    New-Item -ItemType Directory -Force (Split-Path -Parent $patched) | Out-Null
    Copy-Item -Recurse -Force $bflatHome $patched
    foreach ($a in @("x64", "arm64")) {
        & dotnet run --project (Join-Path $root "tools\ZerolibPatch") -c Release -- `
            (Join-Path $bflatHome "lib\linux\$a\zerolib.dll") (Join-Path $patched "lib\linux\$a\zerolib.dll") | Write-Host
        if ($LASTEXITCODE -ne 0) { Remove-Item -Recurse -Force $patched; throw "ZerolibPatch failed ($a)" }
    }
    Set-Content -Path $marker -Value $patchVersion -Encoding ascii
}
$bflat
