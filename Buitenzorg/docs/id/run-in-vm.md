# Menjalankan Buitenzorg OS di VMware Player, VirtualBox & Hyper-V

Selain QEMU, image Buitenzorg bisa dijalankan di **VMware Workstation Player**,
**Oracle VirtualBox**, dan **Microsoft Hyper-V**. Build menghasilkan disk mentah (`dist/buitenzorg-bios.img`,
MBR/BIOS); skrip di bawah mengonversinya ke format masing-masing hypervisor.

Prasyarat: sudah build OS (`.\scripts\build.ps1` / `./scripts/build.sh` atau
quickstart). Konversi memakai **`qemu-img`** (bagian dari QEMU).

---

## 🔧 Buat image VM (satu perintah)

**Windows:**
```powershell
.\scripts\make-vm-images.ps1
```
**Linux / macOS:**
```bash
./scripts/make-vm-images.sh
```

Menghasilkan di `dist/`:

| Berkas | Untuk |
|--------|-------|
| `buitenzorg.vmdk` | VMware (Player / Workstation / Fusion) |
| `buitenzorg.vdi`  | Oracle VirtualBox |
| `buitenzorg.vhdx` | Microsoft Hyper-V (Generation 1 / BIOS) |
| `Buitenzorg.vmx`  | konfigurasi VM VMware siap-buka |

Jika `VBoxManage` ada di PATH, skrip juga otomatis **mendaftarkan VM
VirtualBox** bernama `Buitenzorg`.

> Konversi manual (kalau perlu):
> ```
> qemu-img convert -f raw -O vmdk dist/buitenzorg-bios.img dist/buitenzorg.vmdk
> qemu-img convert -f raw -O vdi  dist/buitenzorg-bios.img dist/buitenzorg.vdi
> ```

---

## 🟦 VMware Workstation Player

1. Jalankan `make-vm-images` (menghasilkan `dist/Buitenzorg.vmx` + `.vmdk`).
2. VMware Player → **Open a Virtual Machine** → pilih `dist/Buitenzorg.vmx`.
3. Klik **Play**.

VM dikonfigurasi: firmware **BIOS**, RAM **512 MB**, disk `.vmdk` sebagai IDE,
kartu suara (ES1371). Serial di-log ke `buitenzorg-serial.log` di folder VM.

> Membuat manual: New VM → *I will install the OS later* → Guest OS **Other** →
> hapus disk default, **Add → Hard Disk → Use an existing disk** →
> `buitenzorg.vmdk` (IDE) → pastikan firmware **BIOS** (bukan UEFI).

---

## 🟧 Oracle VirtualBox

**Otomatis** (jika `VBoxManage` di PATH, `make-vm-images` sudah melakukannya):
```
VBoxManage startvm Buitenzorg
```

**Manual (GUI):**
1. **New** → Name `Buitenzorg`, Type **Other**, Version **Other/Unknown**.
2. Memory **512 MB**.
3. **Use an existing virtual hard disk file** → pilih `dist/buitenzorg.vdi`.
4. Setelah dibuat: **Settings → System → Motherboard** → pastikan **BIOS**
   (nonaktifkan *Enable EFI*). Opsional **Audio** → **ICH AC97**.
5. **Start**.

**Manual (CLI):**
```
VBoxManage createvm --name Buitenzorg --ostype Other --register
VBoxManage modifyvm Buitenzorg --memory 512 --firmware bios --audiocontroller ac97
VBoxManage storagectl Buitenzorg --name IDE --add ide
VBoxManage storageattach Buitenzorg --storagectl IDE --port 0 --device 0 --type hdd --medium dist/buitenzorg.vdi
VBoxManage startvm Buitenzorg
```

---

## 🟦 Microsoft Hyper-V

Buitenzorg boot lewat **MBR di disk IDE**, jadi pakai **Generation 1** (BIOS).
Generation 2 itu UEFI + SCSI + Secure Boot, dan bootloader Buitenzorg belum
ditandatangani — jangan pakai Gen 2.

**Otomatis** — dari **PowerShell elevated** dengan Hyper-V aktif:
```powershell
.\scripts\make-vm-images.ps1        # menghasilkan dist\buitenzorg.vhdx
.\scripts\make-hyperv-vm.ps1        # buat VM Gen-1 "Buitenzorg" dari VHDX
.\scripts\make-hyperv-vm.ps1 -Start # buat lalu langsung nyalakan
```
Skrip mendeteksi Hyper-V; kalau belum aktif ia mencetak langkah manual (bukan
gagal). Opsi: `-Name`, `-MemoryMB`, `-Switch`, `-Force` (ganti VM yang ada).

**Manual (Hyper-V Manager):**
1. **New → Virtual Machine…**
2. **Generation 1** (WAJIB — BIOS/MBR).
3. Memory **512 MB**.
4. **Connect Virtual Hard Disk → Use an existing virtual hard disk** →
   `dist\buitenzorg.vhdx`.
5. **Finish**, lalu **Start** + **Connect**.

**Manual (PowerShell):**
```powershell
New-VM -Name Buitenzorg -Generation 1 -MemoryStartupBytes 512MB -VHDPath dist\buitenzorg.vhdx
Set-VMProcessor -VMName Buitenzorg -Count 1
Start-VM Buitenzorg
vmconnect localhost Buitenzorg
```

> **Catatan VHDX:** `make-vm-images` membuat VHDX **dinamis 64 MiB** (whole-MiB,
> supaya Hyper-V menerimanya — konversi `-O vhdx` telanjang menghasilkan disk
> 5,47 MiB ganjil yang bisa ditolak, dan qemu build ini tak bisa `resize` vhdx).
> Ruang lebih tak terpakai — MBR hanya menjelaskan sektor yang dibutuhkan OS.
> Jaringan opsional (stack OS baru loopback), jadi VM tanpa NIC tetap boot.

---

## ❓ Catatan & troubleshooting

- **Harus BIOS, bukan UEFI** — image `-bios.img` boot lewat MBR. (Untuk UEFI di
  QEMU ada `buitenzorg-uefi.img`; VMware/VirtualBox contoh di atas memakai jalur
  BIOS yang paling portabel.)
- **Layar hitam beberapa detik** — kernel merender ke framebuffer; tunggu
  desktop muncul. Log lengkap ada di serial (file log VMware/VBox).
- **Tidak boot / "no bootable medium"** — pastikan disk terpasang sebagai
  **IDE** dan firmware **BIOS**.
- **Audio** — driver AC'97; pilih kontroler **AC97/ICH AC97** di VM (opsional).
  Hyper-V tak punya emulasi audio — normal, audio mati di Hyper-V.
- **Hyper-V VM tak boot / "boot failure"** — pastikan **Generation 1** (bukan
  Gen 2); Gen 2 (UEFI) tak akan boot disk MBR ini.
- **Hyper-V "The system cannot find the file"/format ditolak** — pakai VHDX yang
  dibuat `make-vm-images` (64 MiB whole-MiB), bukan hasil `qemu-img -O vhdx`
  telanjang (5,47 MiB, bisa ditolak Hyper-V).
- **Re-generate** — setelah rebuild OS, jalankan `make-vm-images` lagi untuk
  memperbarui `.vmdk`/`.vdi`/`.vhdx`.

---

Mau boot di **komputer fisik** (bukan VM)? Lihat
**[install-hardware.md](install-hardware.md)** — tulis image ke USB dengan
`scripts/flash-usb.ps1` / `.sh`.
