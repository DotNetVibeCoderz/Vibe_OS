# Dokumentasi Buitenzorg OS

Indeks seluruh dokumen. Dibuat oleh **Gravicode Studios**, dipimpin oleh
**Kang Fadhil**.

## Mulai di sini

| Dokumen | Isi |
|---------|-----|
| [tutorial.md](tutorial.md) | **Tutorial berurutan**: build → keliling desktop → shell → bikin app → debug/profil → bawa keluar QEMU |
| [getting-started.md](getting-started.md) | Setup ramah-pemula: prasyarat, quickstart, alur harian, troubleshooting |
| [first-app.md](first-app.md) | Bikin app pertama (SDK + native) + katalog API library bawaan + aturan zerolib |

## Menjalankan & memasang

| Dokumen | Isi |
|---------|-----|
| [run-in-vm.md](run-in-vm.md) | Jalankan image di VMware Player & VirtualBox |
| [install-hardware.md](install-hardware.md) | Tulis image ke USB & boot di komputer fisik (BIOS/UEFI) |
| [debugging.md](debugging.md) | Debug kernel dengan GDB + profiler zona (TSC) |

## Referensi teknis

| Dokumen | Isi |
|---------|-----|
| [abi.md](abi.md) | Tabel syscall ABI v1, struct lintas-batas, model keamanan pointer, aturan evolusi |
| [csharp-userland.md](csharp-userland.md) | Runtime C# ↔ kernel: interop, ELF loader, shim `bzstart` |
| [app-framework.md](app-framework.md) | App framework, manifest, SDK, window syscall |
| [system-services.md](system-services.md) | VFS, service/init manager, async I/O, jaringan |
| [desktop-environment.md](desktop-environment.md) | Kompositor, window manager, tema, workspace, shell |
| [window-system.md](window-system.md) | Sistem grafik & window (v0.6 "Daun") |
| [ai-power.md](ai-power.md) | Subsistem AI (LLM/CV/GenAI + Model Manager) & power management |

## Perencanaan & riwayat (di root repo)

| Dokumen | Isi |
|---------|-----|
| [../PLAN.md](../PLAN.md) | Roadmap produk per-versi (v0.1 → v1.x) |
| [../Progress.md](../Progress.md) | Tracking checklist fitur (sudah/sebagian/belum) |
| [../CHANGELOG.md](../CHANGELOG.md) | Riwayat rilis per codename versi |
| [../requirements.md](../requirements.md) | Desain teknis penuh (spec, Bahasa Indonesia); §17 checklist |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | Standar koding & alur kontribusi |
