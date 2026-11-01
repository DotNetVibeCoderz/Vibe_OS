# Download the Limine bootloader binaries (pinned release + SHA-256) into
# kernel\target\limine\limine-binary. bzimage then also builds
# dist\buitenzorg-limine-uefi.img: the same kernel booted through the Limine
# protocol (multi-arch M1 -- the loader the ARM64 / RISC-V ports use).
$ErrorActionPreference = "Stop"
$version = "v12.9.3"
$sha256 = "9f42fe9ea2e84d71056529969b1d5c24ba9a2287ffcc01bcce58b0ef8aa98b1d"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "kernel\target\limine"
$verFile = Join-Path $dir "VERSION"
if ((Test-Path (Join-Path $dir "limine-binary\BOOTX64.EFI")) -and (Test-Path $verFile) -and ((Get-Content $verFile -Raw).Trim() -eq $version)) {
    Write-Host "limine $version already present"; exit 0
}
if (Test-Path $dir) { Remove-Item -Recurse -Force -Confirm:$false $dir }
New-Item -ItemType Directory -Force $dir | Out-Null
$tgz = Join-Path $dir "limine-binary.tar.gz"
Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/Limine-Bootloader/Limine/releases/download/$version/limine-binary.tar.gz" -OutFile $tgz
$got = (Get-FileHash -Algorithm SHA256 $tgz).Hash.ToLower()
if ($got -ne $sha256) { throw "limine-binary.tar.gz: SHA-256 mismatch ($got)" }
# Windows' own bsdtar: a GNU tar earlier on PATH (Git) reads "C:" as a host.
$tar = Join-Path $env:SystemRoot "System32\tar.exe"
if (-not (Test-Path $tar)) { $tar = "tar" }
& $tar -xzf $tgz -C $dir
if ($LASTEXITCODE -ne 0) { throw "tar failed" }
Set-Content -Path $verFile -Value $version -Encoding ascii
Write-Host "limine $version -> $(Join-Path $dir 'limine-binary')"
