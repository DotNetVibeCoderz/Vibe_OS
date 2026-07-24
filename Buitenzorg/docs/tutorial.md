# Tutorial: Dari Nol sampai Bikin App di Buitenzorg OS

Panduan berurutan dari **membangun & mem-boot OS** sampai **menulis app sendiri**
dan **men-debug/profil kernel**. Ikuti dari atas, atau lompat ke bagian yang
Anda butuhkan. Setiap bagian menautkan dokumen mendalam kalau ingin detail.

> Dibuat oleh **Gravicode Studios**, dipimpin oleh **Kang Fadhil**.
> Prasyarat & troubleshooting lengkap: [getting-started.md](getting-started.md).

**Peta perjalanan:**
1. [Build & boot](#1-build--boot-5-menit) — jalankan OS di QEMU
2. [Keliling desktop](#2-keliling-desktop) — Start menu, ikon, app suite
3. [Shell](#3-shell-terminal) — perintah, tema, workspace, polyglot
4. [Bikin app pertama](#4-bikin-app-pertama-c) — dari template ke `run`
5. [Pakai library](#5-pakai-library-bawaan) — Drawing/UI/Audio/Bcl
6. [Debug & profil](#6-debug--profil-kernel) — GDB + profiler
7. [Bawa keluar QEMU](#7-bawa-keluar-qemu) — VM & USB hardware
8. [Langkah berikut](#8-langkah-berikut)

---

## 1. Build & boot (5 menit)

Cara tercepat — satu skrip memasang semua dependency lalu boot:

```powershell
# Windows
.\scripts\quickstart.ps1
```
```bash
# Linux / macOS
./scripts/quickstart.sh
```

Atau manual, kalau dependency sudah ada (Rust nightly, .NET SDK, QEMU, bflat):

```powershell
.\scripts\build.ps1          # → dist\buitenzorg-{bios,uefi}.img
.\scripts\run-qemu.ps1       # boot dengan tampilan + serial
```

Butuh ~1 menit untuk boot ke `BUITENZORG READY` (kernel menjalankan puluhan
demo milestone di jalan). Log kernel muncul di serial **dan** di framebuffer;
setelah desktop render, ia menutupi teks boot.

**Verifikasi tanpa tampilan** (yang dipakai CI):

```powershell
.\scripts\smoke-test.ps1     # boot headless, assert semua marker MILESTONE
```

➡️ Detail setup, daftar dependency, troubleshooting: **[getting-started.md](getting-started.md)**.

---

## 2. Keliling desktop

Setelah `READY`, desktop hidup (mouse & keyboard live di QEMU):

- **Tombol Start** (kiri-bawah, hijau) → **start menu**: daftar app + aksi power.
- **Ikon desktop** (kiri-atas) → klik-ganda meluncurkan app.
- **Taskbar**: tombol window berjalan + **tray** (nama tema + **jam RTC live** +
  pip workspace).
- **App suite bawaan** (8): Kalkulator, Text Editor, 2048, Jam, File Manager,
  Piano, Image Viewer, App Store.

➡️ Konsep desktop (kompositor, window manager, tema, workspace):
**[desktop-environment.md](desktop-environment.md)** ·
**[window-system.md](window-system.md)**.

---

## 3. Shell (terminal)

Buka Terminal dari desktop. Coba:

```
help                 # daftar perintah
ls /disk             # isi disk (app suite ada di sini)
cat /ram/DAHAN.TXT   # baca file
theme cycle          # ganti antar 8 tema (live)
ws 2                 # pindah ke workspace 2
run calc             # jalankan Kalkulator
run editor           # Editor — interaktif: ketik, Ctrl+S simpan
prof self            # profil recompose desktop (lihat serial log)
ask halo dunia       # LLM lokal melengkapi teks
bz model list        # galeri model gaya Hugging Face
vm create nanovm     # buat VM (guest: NanoOS)
vm start nanovm      # boot guest OS mini di VMM software
vm list              # daftar VM + status
```

**Polyglot** — jalankan JS/TS/Python di interpreter in-kernel:

```
js                   # demo JavaScript bawaan
py main.py           # jalankan file Python dari VFS
script ts main.ts    # TypeScript (transpile lalu interpret)
```

➡️ Layanan sistem (VFS, service manager, async I/O, jaringan):
**[system-services.md](system-services.md)** · AI & power:
**[ai-power.md](ai-power.md)**.

---

## 4. Bikin app pertama (C#)

Dua jalur. **Jalur cepat — pakai template SDK:**

```powershell
dotnet run --project sdk\bz -- new console-csharp MyApp
```

**Jalur native — tambah app C# ring-3** ke build (seperti app suite):

1. Tulis `userland/hello-csharp/myapp.cs` (kelas dengan `static void Main`).
2. Daftarkan di `scripts/build-hello-csharp.ps1` **dan** `.sh` (daftar program).
3. Embed di `kernel/bzimage/build.rs` (`("myapp.elf", "myapp.elf")`).
4. Beri nama peluncuran di `kernel/bzkernel/src/app.rs` (`"myapp" => Some("MYAPP.ELF")`).
5. Build ulang app + image, lalu `run myapp` di shell.

Contoh minimal yang mencetak milestone:

```csharp
using System;
class Program {
    static void Main() {
        Console.WriteLine("Halo dari app saya!");
        Console.WriteLine("MILESTONE: MYAPP OK");
    }
}
```

➡️ Panduan lengkap kedua jalur + katalog contoh: **[first-app.md](first-app.md)**.

> ⚠️ **Aturan zerolib (wajib dibaca).** App freestanding: heap **jalan**
> (`new`, array, generic), tapi **tanpa** static reference field, **tanpa**
> method-group→delegate (pakai function pointer), **tanpa** simpan referensi ke
> elemen `object[]` (pakai linked list), **tanpa** `new string()`/`ToString()`/
> concat (pakai `char[]` + `Graphics.DrawChars`). Rincian: [first-app.md](first-app.md).

---

## 5. Pakai library bawaan

App C# punya empat library (tambahkan file sumbernya ke daftar build):

| Library | File | Untuk |
|---------|------|-------|
| **Buitenzorg.Drawing** | `bzgfx.cs` | grafik: Graphics/Bitmap/transform/Font, BMP+JPEG |
| **Buitenzorg.UI** | `bzui.cs` | toolkit retained: Button/Grid/ListBox/… (butuh Drawing) |
| **Buitenzorg.Audio** | `bzaudio.cs` | mixer + tone/PCM (AC'97) |
| **Buitenzorg.Bcl** | `bzbcl.cs` + `bzbcl2.cs` | koleksi/LINQ + System.IO/Text/Regex/Net/Tasks/… |

Contoh — window UI + baca file + waktu nyata:

```csharp
using Buitenzorg;                 // BCL
using Buitenzorg.UI;

var host = new UIHost("Demo", 320, 200);
var root = new StackPanel { Padding = 12 };
root.Add(new TextBlock("Halo Buitenzorg", Font.Default()));
host.Root = root; host.Layout();
host.Render(new Buitenzorg.Drawing.Color(0xFF1C2028)); host.Present();

// System.Globalization + System.IO dari Buitenzorg.Bcl:
var now = BzDateTime.Now();                    // jam CMOS nyata
byte[] data; BzFile.ReadAllBytes("/disk/PHOTO.BMP", 400*1024, out data);
```

➡️ Katalog API tiap library + contoh app nyata: **[first-app.md](first-app.md)**
(lihat app suite `calc.cs`, `clock.cs`, `store.cs`, `imgview.cs` sebagai contoh).

---

## 6. Debug & profil kernel

**GDB attach** — boot QEMU ditahan lalu step di ring 0:

```powershell
.\scripts\debug-kernel.ps1        # Linux/macOS: ./scripts/debug-kernel.sh
```
```gdb
(gdb) bz-break-main               # break di kernel_main
(gdb) continue
(gdb) bt
```

**Profiler** — ukur di mana siklus dihabiskan (di shell OS):

```
prof self                         # profil recompose desktop, laporan ke serial
```

➡️ Alur lengkap + helper GDB + cara menambah zona profiler:
**[debugging.md](debugging.md)**.

---

## 7. Bawa keluar QEMU

**VM (VMware / VirtualBox):**

```powershell
.\scripts\make-vm-images.ps1      # → .vmdk (VMware) + .vdi (VirtualBox)
```
➡️ **[run-in-vm.md](run-in-vm.md)**.

**Komputer fisik (boot USB):**

```powershell
.\scripts\flash-usb.ps1 -List     # lihat disk USB
.\scripts\flash-usb.ps1 -DiskNumber <N> -Firmware uefi
```
➡️ **[install-hardware.md](install-hardware.md)** — pilih firmware, boot menu,
tabel kompatibilitas. *(Boot hardware masih eksperimental.)*

---

## 8. Langkah berikut

- **Roadmap & status:** [PLAN.md](../PLAN.md) · [Progress.md](../Progress.md) ·
  [CHANGELOG.md](../CHANGELOG.md).
- **Kontrak ABI syscall** (kalau menambah syscall): [abi.md](abi.md).
- **Runtime C# ↔ kernel** (interop, ELF loader): [csharp-userland.md](csharp-userland.md).
- **App framework & SDK:** [app-framework.md](app-framework.md).
- **Desain teknis penuh (spec):** [requirements.md](../requirements.md).
- **Berkontribusi:** [CONTRIBUTING.md](../CONTRIBUTING.md).
- **MagicAppGen** — generate app Buitenzorg dari prompt dengan bantuan LLM:
  `tools/MagicAppGen/README.md`.

Selamat ngoprek — *zonder zorg, tanpa kekhawatiran.* 🌱
