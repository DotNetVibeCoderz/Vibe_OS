# Download the Raspberry Pi 4 boot files (pinned release + SHA-256) into
# kernel\target\rpi4-uefi: the Pi firmware (start4.elf, fixup4.dat), its
# Device Trees and overlays, and the community UEFI firmware (RPI_EFI.fd).
# bzimage then also builds dist\buitenzorg-rpi4.img, the SD card image.
# Nothing here is committed: these are third-party binaries, see Readme.md in
# that directory.
$ErrorActionPreference = "Stop"
$version = "v1.53"
$sha256 = "ca9973e2a7a546b3df871cfb7382e656829114b6dfa424f40dc67cc90a217d88"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "kernel\target\rpi4-uefi"
$verFile = Join-Path $dir "VERSION"
if ((Test-Path (Join-Path $dir "start4.elf")) -and (Test-Path $verFile) -and ((Get-Content $verFile -Raw).Trim() -eq $version)) {
    Write-Host "rpi4 firmware $version already present"; exit 0
}
if (Test-Path $dir) { Remove-Item -Recurse -Force -Confirm:$false $dir }
New-Item -ItemType Directory -Force $dir | Out-Null
$zip = Join-Path $dir "RPi4_UEFI_Firmware_$version.zip"
Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/pftf/RPi4/releases/download/$version/RPi4_UEFI_Firmware_$version.zip" -OutFile $zip
$got = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
if ($got -ne $sha256) { throw "$zip: SHA-256 mismatch ($got)" }
Expand-Archive -Path $zip -DestinationPath $dir -Force
Set-Content -Path $verFile -Value $version -Encoding ascii
Write-Host "rpi4 firmware $version -> $dir"
