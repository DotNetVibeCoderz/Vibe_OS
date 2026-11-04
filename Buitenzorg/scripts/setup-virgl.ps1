# G3: prepare a QEMU that can run the 3D API on a host GPU (virtio-gpu-gl,
# virglrenderer). The Windows QEMU build ships virglrenderer, but on many
# machines its OpenGL context cannot be created ("Unable to create OpenGL
# context >= 3.0"). This makes kernel\target\qemu-gl: a copy of
# qemu-system-x86_64.exe and its DLLs next to Mesa's EGL / OpenGL (llvmpipe,
# pinned release + SHA-256), which works with `-display egl-headless`. The
# installed QEMU is not modified.
#
#   .\scripts\setup-virgl.ps1
#   GALLIUM_DRIVER=llvmpipe kernel\target\qemu-gl\qemu-system-x86_64.exe -L "<qemu>\share" `
#       -device virtio-gpu-gl-pci -display egl-headless ...
# smoke-test.ps1 runs its [virgl] step when this folder exists.
$ErrorActionPreference = "Stop"
$version = "26.2.6"
$sha256 = "da0f71d9181b672c1918f3c342f11118c687f6059a46b16502845160733cc388"
$root = Split-Path -Parent $PSScriptRoot
$qemuDir = if ($env:QEMU) { Split-Path -Parent $env:QEMU } else { "C:\Program Files\qemu" }
$dst = Join-Path $root "kernel\target\qemu-gl"
$verFile = Join-Path $dst "MESA_VERSION"
if ((Test-Path (Join-Path $dst "opengl32.dll")) -and (Test-Path $verFile) -and ((Get-Content $verFile -Raw).Trim() -eq $version)) {
    Write-Host "qemu-gl (Mesa $version) already present: $dst"; exit 0
}
$sevenZip = @("C:\Program Files\7-Zip\7z.exe", "7z") | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue } | Select-Object -First 1
if (-not $sevenZip) { throw "7-Zip is needed to unpack Mesa (winget install 7zip.7zip)" }
if (Test-Path $dst) { Remove-Item -Recurse -Force -Confirm:$false $dst }
New-Item -ItemType Directory -Force $dst | Out-Null
$tmp = Join-Path $root "kernel\target\mesa-dl"
New-Item -ItemType Directory -Force $tmp | Out-Null
$archive = Join-Path $tmp "mesa3d-$version-release-mingw.7z"
Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/pal1000/mesa-dist-win/releases/download/$version/mesa3d-$version-release-mingw.7z" -OutFile $archive
$got = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLower()
if ($got -ne $sha256) { throw "Mesa archive: SHA-256 mismatch ($got)" }
& $sevenZip x -y "-o$tmp\x" $archive | Out-Null
if ($LASTEXITCODE -ne 0) { throw "7-Zip failed" }
Copy-Item (Join-Path $qemuDir "qemu-system-x86_64.exe") $dst
Get-ChildItem $qemuDir -Filter *.dll | Copy-Item -Destination $dst
foreach ($f in "opengl32.dll", "libgallium_wgl.dll", "libEGL.dll", "libGLESv2.dll") {
    Copy-Item (Join-Path "$tmp\x\x64" $f) $dst -Force
}
Set-Content -Path $verFile -Value $version -Encoding ascii
Remove-Item -Recurse -Force -Confirm:$false $tmp
Write-Host "qemu-gl ready: $dst (QEMU from $qemuDir + Mesa $version llvmpipe)"
