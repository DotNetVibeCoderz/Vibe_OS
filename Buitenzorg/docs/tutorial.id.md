# Tutorial: Dari Nol sampai App Pertama

Panduan berurutan dari **membangun & mem-boot OS** sampai **menulis app sendiri**
dan **men-debug/profil kernel**. Ikuti dari atas ke bawah, atau lompat ke bagian
yang Anda butuhkan — tiap bagian menautkan dokumen lebih dalam untuk detailnya.

> Prasyarat & troubleshooting lengkap: [Getting Started](getting-started.id.md).

[English](tutorial.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

**Peta perjalanan:**
1. [Build & boot](#1-build--boot-5-menit) — jalankan OS di QEMU
2. [Keliling desktop](#2-keliling-desktop) — start menu, ikon, suite app
3. [Shell](#3-shell-terminal) — perintah, tema, workspace, polyglot
4. [App pertama](#4-app-pertama-c) — dari template ke `run`
5. [Pakai library](#5-pakai-library-bawaan) — Drawing / UI / Audio / Bcl
6. [Debug & profil](#6-debug--profil-kernel) — GDB + profiler
7. [Keluar QEMU](#7-keluar-qemu) — VM & USB hardware
8. [Langkah berikutnya](#8-langkah-berikutnya)

---

## 1. Build & boot (5 menit)

Jalur tercepat — satu skrip memasang dependensi lalu boot:

```powershell
.\scripts\quickstart.ps1     # Linux/macOS: ./scripts/quickstart.sh
```

Atau manual, kalau dependensi sudah terpasang (Rust nightly, .NET SDK, QEMU,
bflat):

```powershell
.\scripts\build.ps1          # → dist\buitenzorg-{bios,uefi}.img
.\scripts\run-qemu.ps1       # boot dengan tampilan + serial
```

Butuh sekitar semenit untuk mencapai `BUITENZORG READY` (kernel menjalankan
puluhan demo milestone di jalan). Log kernel tampil di serial **dan** di
framebuffer; setelah desktop dirender, ia menutupi teks boot.

**Verifikasi tanpa tampilan** (yang dipakai CI):

```powershell
.\scripts\smoke-test.ps1     # boot headless, assert tiap marker MILESTONE
```

➡️ Detail setup, daftar dependensi, troubleshooting: **[Getting Started](getting-started.id.md)**.

## 2. Keliling desktop

Setelah `READY`, desktop hidup (mouse & keyboard berfungsi di QEMU):

- **Tombol Start** (kiri-bawah, hijau) → **start menu**: daftar app + aksi power.
- **Ikon desktop** (kiri-atas) → klik-ganda untuk meluncurkan app.
- **Taskbar**: tombol window berjalan + **tray** (nama tema + **jam RTC live** +
  pip workspace).
- **Suite bawaan** (8 app): Kalkulator, Text Editor, 2048, Jam, File Manager,
  Piano, Image Viewer, App Store.

![Desktop Buitenzorg](img/desktop-shell.png)

➡️ Konsep desktop (compositor, window manager, tema, workspace):
**[Desktop Environment](desktop-environment.id.md)** · **[Window System](window-system.id.md)**.

## 3. Shell (terminal)

Buka Terminal dari desktop. Coba:

```
help                 # daftar perintah
ls /disk             # isi disk (suite app ada di sini)
cat /ram/DAHAN.TXT   # baca file
theme cycle          # gilir 8 tema (live)
ws 2                 # pindah ke workspace 2
run calc             # luncurkan Kalkulator
run editor           # Editor — interaktif: ketik, Ctrl+S untuk simpan
prof self            # profil recompose desktop (laporan di serial)
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
script ts main.ts    # TypeScript (di-transpile, lalu diinterpretasi)
```

➡️ Layanan sistem (VFS, service manager, async I/O, jaringan):
**[System Services](system-services.id.md)** · AI & power: **[AI & Power](ai-power.id.md)**.

## 4. App pertama (C#)

Dua jalur. **Jalur cepat — pakai template SDK:**

```powershell
dotnet run --project sdk\bz -- new console-csharp MyApp
```

**Jalur native — tambah app C# ring-3** ke build (seperti suite app):

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

➡️ Panduan lengkap kedua jalur + katalog contoh: **[App Pertama](first-app.id.md)**.

> ⚠️ **Aturan zerolib (wajib dibaca).** App freestanding: heap **berfungsi**
> (`new`, array, generic), tapi **tanpa** static reference field, **tanpa**
> konversi method-group→delegate (pakai function pointer), **tanpa** menyimpan
> referensi ke elemen `object[]` (pakai linked list), dan **tanpa**
> `new string()` / `ToString()` / concat (pakai `char[]` + `Graphics.DrawChars`).
> Detail: [first-app.id.md](first-app.id.md).

## 5. Pakai library bawaan

App C# punya empat library (tambahkan file sumbernya ke build):

| Library | File | Untuk |
|---|---|---|
| **Buitenzorg.Drawing** | `bzgfx.cs` | grafik: Graphics/Bitmap/transform/Font, BMP + JPEG |
| **Buitenzorg.UI** | `bzui.cs` | toolkit retained-mode: Button/Grid/ListBox/… (butuh Drawing) |
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

➡️ Katalog API per-library + contoh app nyata: **[App Pertama](first-app.id.md)**
(lihat suite app `calc.cs`, `clock.cs`, `store.cs`, `imgview.cs`).

## 6. Debug & profil kernel

**Attach GDB** — boot QEMU ditahan, lalu step di ring 0:

```powershell
.\scripts\debug-kernel.ps1        # Linux/macOS: ./scripts/debug-kernel.sh
```
```gdb
(gdb) bz-break-main               # break di kernel_main
(gdb) continue
(gdb) bt
```

**Profiler** — ukur ke mana siklus dihabiskan (di shell OS):

```
prof self                         # profil recompose desktop; laporan di serial
```

➡️ Alur lengkap + helper GDB + cara menambah zona profiler:
**[Debugging & Profiling](debugging.id.md)**.

## 7. Keluar QEMU

**VM (VMware / VirtualBox / Hyper-V):**

```powershell
.\scripts\make-vm-images.ps1      # → .vmdk + .vdi + .vhdx
```
➡️ **[Jalankan di VM](run-in-vm.id.md)**.

**Mesin fisik (boot dari USB):**

```powershell
.\scripts\flash-usb.ps1 -List     # daftar disk USB
.\scripts\flash-usb.ps1 -DiskNumber <N> -Firmware uefi
```
➡️ **[Pasang di Hardware](install-hardware.id.md)** — pilihan firmware, boot menu,
tabel kompatibilitas. *(Boot hardware masih eksperimental.)*

## 8. Langkah berikutnya

- **Roadmap & status:** [PLAN.md](../PLAN.md) · [Progress.md](../Progress.md) · [CHANGELOG.md](../CHANGELOG.md).
- **Kontrak syscall ABI** (kalau menambah syscall): [Syscall ABI](abi.id.md).
- **C# ↔ kernel** (interop, ELF loader): [C# di Ring 3](csharp-userland.id.md).
- **App framework & SDK:** [App Framework](app-framework.id.md).
- **Spec teknis penuh:** [requirements.md](../requirements.md) *(ID)*.
- **Berkontribusi:** [CONTRIBUTING.md](../CONTRIBUTING.md) *(EN)*.
- **MagicAppGen** — generate app Buitenzorg dari prompt dengan bantuan LLM:
  `tools/MagicAppGen/README.md`.

Selamat ngoprek — *zonder zorg, tanpa kekhawatiran.* 🌱

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
