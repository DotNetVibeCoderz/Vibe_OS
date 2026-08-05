# Pasang & Boot di Hardware Nyata

Selain QEMU dan VM (VMware/VirtualBox/Hyper-V — lihat [run-in-vm.id.md](run-in-vm.id.md)),
Buitenzorg OS bisa **boot dari USB di mesin fisik**. Build menghasilkan dua disk
image mentah yang bisa langsung ditulis ke stik USB:

| Berkas | Firmware | Skema disk |
|---|---|---|
| `dist/buitenzorg-bios.img` | Legacy BIOS / CSM | MBR |
| `dist/buitenzorg-uefi.img` | UEFI | GPT + ESP (FAT) |

[English](install-hardware.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

> ⚠️ **Status:** jalur boot USB ini disiapkan lengkap (skrip + verifikasi
> baca-ulang), tetapi **belum diverifikasi di mesin fisik** — sejauh ini
> Buitenzorg teruji otomatis di QEMU (4 media) dan di VMware/VirtualBox.
> Perlakukan boot hardware sebagai **eksperimen**: pakai mesin dan stik USB yang
> boleh Anda korbankan, dan baca [Kompatibilitas &
> batasan](#-kompatibilitas--batasan) di bawah.

---

## 0. Prasyarat

1. **Build OS lebih dulu:**
   ```powershell
   .\scripts\build.ps1        # Windows   (Linux/macOS: ./scripts/build.sh)
   ```
   Setelahnya `dist\buitenzorg-bios.img` dan `dist\buitenzorg-uefi.img` ada.
2. **Stik USB** yang isinya boleh dihapus (image kecil, ~5 MB — USB berapa pun cukup).
3. **Pilih firmware** sesuai target:
   - Mesin lama, atau opsi *Legacy*/*CSM* di BIOS → pakai **bios**.
   - Mesin UEFI modern → pakai **uefi**, dan **matikan Secure Boot** (bootloader
     Buitenzorg belum ditandatangani).

## 1. Tulis image ke USB (skrip)

**Skrip menghapus seluruh disk target.** Ada pengaman berlapis: hanya disk
**USB/removable** yang ditawarkan, target dipilih **eksplisit** (bukan ditebak),
disk **sistem/boot ditolak mentah**, ukuran + model ditampilkan dengan konfirmasi
ketik, dan hasil tulis **diverifikasi baca-ulang**.

### Windows (PowerShell **sebagai Administrator**)

```powershell
# 1) Lihat kandidat disk USB (aman, read-only):
.\scripts\flash-usb.ps1 -List

# 2) Tulis (interaktif — meminta nomor disk + konfirmasi ketik ERASE):
.\scripts\flash-usb.ps1

# atau langsung, mis. physical disk 2, firmware UEFI:
.\scripts\flash-usb.ps1 -DiskNumber 2 -Firmware uefi
```

Menulis butuh akses disk mentah → **wajib PowerShell elevated**; skrip berhenti
kalau tidak. Disk non-USB ditolak kecuali `-Force` (jaring pengaman terakhir).

### Linux / macOS

```bash
# 1) Lihat kandidat perangkat:
./scripts/flash-usb.sh --list

# 2) Tulis (butuh sudo untuk akses perangkat mentah):
sudo ./scripts/flash-usb.sh /dev/sdX            # Linux, image BIOS
sudo ./scripts/flash-usb.sh /dev/sdX --uefi     # image UEFI
sudo ./scripts/flash-usb.sh /dev/rdiskN         # macOS: pakai node RAW rdiskN
```

Target harus **seluruh disk** (`/dev/sdb`), bukan partisi (`/dev/sdb1`). Skrip
menolak disk root/sistem dan meng-unmount partisi yang ter-mount lebih dulu.

## 2. Tulis image ke USB (alat GUI — disarankan untuk pemula)

Kalau ragu dengan skrip, pakai alat flash yang sudah dikenal — mereka juga
menampilkan daftar disk dengan aman:

- **[balenaEtcher](https://etcher.balena.io/)** (Windows/macOS/Linux) — pilih
  `buitenzorg-bios.img` **atau** `buitenzorg-uefi.img`, pilih USB, *Flash*.
- **[Rufus](https://rufus.ie/)** (Windows) — *Boot selection* → pilih image, pilih
  mode **DD image** saat ditanya.
- **`dd`** manual (Linux/macOS), tanpa skrip:
  ```bash
  sudo dd if=dist/buitenzorg-bios.img of=/dev/sdX bs=4M conv=fsync status=progress
  sync
  ```

> Image ini bootable apa adanya (bukan ISO hybrid, tapi disk MBR/GPT penuh), jadi
> mode **DD/raw** — bukan mode "ISO" — yang benar di Rufus.

## 3. Boot mesin dari USB

1. Colok USB, nyalakan / restart.
2. Masuk **boot menu** (biasanya `F12`, `F10`, `F9`, `Esc`, atau `F2` → Boot
   order — tergantung vendor).
3. Pilih entri USB:
   - image **uefi** muncul sebagai **"UEFI: <nama USB>"**,
   - image **bios** muncul sebagai USB biasa (non-UEFI).
4. Buitenzorg boot: logo ASCII → log kernel di layar → desktop.

Kalau USB tak muncul di boot menu: pastikan **Secure Boot OFF**, dan untuk image
bios aktifkan **Legacy/CSM**; untuk image uefi pastikan mode **UEFI** aktif.

## 4. Verifikasi di mesin fisik (mohon dilaporkan)

Karena boot hardware belum tervalidasi, catat hal-hal ini bila Anda mencobanya —
sangat membantu untuk menandainya "teruji":

- **Boot & framebuffer:** logo muncul? resolusi & warna benar? Bootloader meminta
  framebuffer linear; sebagian GPU/monitor bisa memberi mode lain.
- **Input PS/2 vs USB:** driver keyboard/mouse Buitenzorg saat ini **PS/2**.
  Laptop/PC modern sering hanya punya **USB HID** (driver USB belum ada) → input
  bisa mati walau boot sukses. Port PS/2 atau adaptor, atau opsi BIOS **"USB
  Legacy / emulation"** (menyajikan HID sebagai PS/2), membantu.
- **Penyimpanan:** driver disk hanya **IDE/SATA PIO (ATA)**. Suite app ada di
  `/disk` yang butuh ini; AHCI/NVMe native belum ada, jadi di mesin NVMe-only
  `/disk` mungkin tak terbaca (kernel tetap boot).
- **Timer & interrupt:** PIT + PIC 8259 legacy (bukan APIC). Umumnya masih
  didukung lewat legacy emulation di chipset modern.
- **ACPI shutdown:** `shutdown` di shell memakai ACPI (fallback port QEMU/VBox
  tak berlaku di hardware) — laporkan apakah benar-benar mematikan.

Serial log tidak otomatis tersedia di hardware seperti di QEMU; andalkan layar
(semua log kernel juga dicetak ke framebuffer console).

## 🧭 Kompatibilitas & batasan

| Area | Dukungan sekarang | Catatan |
|---|---|---|
| Firmware | UEFI **dan** legacy BIOS | Secure Boot harus OFF (unsigned) |
| CPU | x86-64 | ARM64/RISC-V = v1.x "Rimba" |
| Grafis | framebuffer linear dari bootloader | tanpa driver GPU/akselerasi |
| Keyboard/Mouse | **PS/2** | USB HID belum; pakai USB-legacy BIOS |
| Storage | IDE/SATA **PIO (ATA)** | AHCI/NVMe/USB-MSD native belum |
| Interrupt/timer | PIC 8259 + PIT | APIC belum (blok SMP) |
| Jaringan | loopback saja | driver NIC (e1000) belum |
| SMP | single-core | multi-core belum |

Semua item "belum" ada di backlog **[PLAN.md](../PLAN.md)** (Utang Teknis) dan
dilacak di **[Progress.md](../Progress.md)**.

## 🆘 Pemecahan masalah

- **USB tak muncul di boot menu** → Secure Boot OFF; cocokkan firmware
  (bios↔Legacy/CSM, uefi↔UEFI); coba tulis image **satunya**; coba port USB lain
  (kadang hanya port tertentu bootable).
- **Layar hitam setelah pilih USB** → biasanya mismatch firmware (menulis image
  bios lalu boot mode UEFI, atau sebaliknya) — tulis image yang cocok.
- **Boot tapi keyboard mati** → lihat catatan PS/2 vs USB di atas; aktifkan **USB
  Legacy** di BIOS.
- **`flash-usb.ps1` bilang butuh Administrator** → buka PowerShell via klik-kanan
  → *Run as administrator*.
- **`flash-usb.ps1` menolak disk** → itu bukan disk USB/removable (atau disk
  sistem). Pastikan nomor disk benar via `-List`; ini pengaman yang disengaja.
- **Verifikasi GAGAL** setelah menulis → **jangan** boot dari USB itu; ulangi
  penulisan (stik bisa aus/palsu — coba stik lain).
- **Kembalikan USB jadi normal** → format ulang: Windows `diskpart` → `clean` →
  buat partisi baru; Linux/macOS `sudo wipefs -a /dev/sdX`, lalu buat partisi.

## Ringkasan perintah

```powershell
# Windows
.\scripts\build.ps1
.\scripts\flash-usb.ps1 -List
.\scripts\flash-usb.ps1 -DiskNumber <N> -Firmware uefi   # atau -Firmware bios
```
```bash
# Linux / macOS
./scripts/build.sh
./scripts/flash-usb.sh --list
sudo ./scripts/flash-usb.sh /dev/sdX --uefi              # atau --bios
```

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
