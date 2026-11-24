# Fetch the Raspberry Pi 4's Wi-Fi firmware (Cypress CYW43455) into
# kernel\target\wifi, each file checked against a pinned SHA-256 (see
# fetch-wifi-firmware.sh). Not committed: Cypress licence.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "kernel\target\wifi"
New-Item -ItemType Directory -Force $dir | Out-Null
$base = "https://raw.githubusercontent.com/RPi-Distro/firmware-nonfree/c91cd2804cf7463aab913e7247c176049f16bbd6/debian/config/brcm80211"
function Get-Pinned($url, $name, $sha) {
    $file = Join-Path $dir $name
    if ((Test-Path $file) -and (Get-FileHash $file -Algorithm SHA256).Hash -eq $sha.ToUpper()) { return }
    Write-Host "fetch-wifi-firmware: downloading $url"
    Invoke-WebRequest -Uri $url -OutFile "$file.part" -UseBasicParsing
    if ((Get-FileHash "$file.part" -Algorithm SHA256).Hash -ne $sha.ToUpper()) {
        Remove-Item "$file.part"; throw "fetch-wifi-firmware: checksum mismatch for $name"
    }
    Move-Item -Force "$file.part" $file
}
Get-Pinned "$base/cypress/cyfmac43455-sdio-standard.bin" "FW.BIN" "d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3"
Get-Pinned "$base/cypress/cyfmac43455-sdio.clm_blob" "FW.CLM" "9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1"
Get-Pinned "$base/brcm/brcmfmac43455-sdio.txt" "FW.TXT" "ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d"
# Intel Wireless 8265 (laptops, platform/pc/iwlwifi.rs): linux-firmware tag 20241110.
Get-Pinned "https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/plain/iwlwifi-8265-36.ucode?h=20241110" "IWL8265.UCO" "d7d84c9b6a3b1976acb97a2bec7102e24a6b5d794110c0dcbd107ab5d9877e5f"
Write-Host "fetch-wifi-firmware: $dir ready"
