# Dokumentasi

Indeks dokumentasi lengkap **Buitenzorg OS**. Dokumen tersedia dalam **English**
dan **Bahasa Indonesia**; spec teknis dan tracker perencanaan di root repo dalam
**Bahasa Indonesia** (ditandai *ID* di bawah).

[English](README.md) · **Bahasa Indonesia** · ← Kembali ke [README proyek](../README.id.md).

## Mulai di sini

| Dokumen | Isi |
|---|---|
| [**Tutorial**](tutorial.id.md) | Panduan berurutan nol→app: build → keliling desktop → shell → bikin app → debug/profil → keluar QEMU. **Baca ini dulu.** |
| [Getting Started](getting-started.id.md) | Prasyarat, quickstart satu perintah, alur harian, dan troubleshooting. |
| [App Pertama](first-app.id.md) | Dua cara membuat app (SDK & native), katalog library bawaan, dan aturan zerolib yang wajib diketahui tiap penulis app. |

## Jalankan & pasang

| Dokumen | Isi |
|---|---|
| [Jalankan di VM](run-in-vm.id.md) | Konversi image lalu jalankan di VMware Player, VirtualBox, atau Hyper-V. |
| [Pasang di Hardware](install-hardware.id.md) | Tulis image ke stik USB dan boot mesin fisik (BIOS/UEFI), dengan tabel kompatibilitas yang jujur. |
| [Debugging & Profiling](debugging.id.md) | Attach GDB ke kernel yang berjalan, dan profiler zona berbasis TSC. |

## Referensi teknis

| Dokumen | Isi |
|---|---|
| [Syscall ABI](abi.id.md) | Tabel syscall v1, struct lintas-batas, model keamanan pointer, dan aturan evolusi. |
| [C# di Ring 3](csharp-userland.id.md) | Cara C# berjalan di user-space: pipeline bflat, ELF loader, dan shim `bzstart`. |
| [App Framework](app-framework.id.md) | Model aplikasi, manifest, SDK, dan window syscall. |
| [System Services](system-services.id.md) | VFS, service/init manager, async I/O, dan jaringan. |
| [Desktop Environment](desktop-environment.id.md) | Compositor, window manager, tema, workspace, dan shell. |
| [Graphics & Window System](window-system.id.md) | Stack rendering dan manajemen window. |
| [Subsistem AI & Power](ai-power.id.md) | Subsistem LLM / CV / GenAI lokal, Model Manager, dan power management. |

## Perencanaan & riwayat (root repo)

| Dokumen | Isi |
|---|---|
| [PLAN.md](../PLAN.md) *(ID)* | Roadmap produk, per-versi (v0.1 → v1.x). |
| [Progress.md](../Progress.md) *(ID)* | Tracker checklist per-fitur (selesai / sebagian / belum). |
| [CHANGELOG.md](../CHANGELOG.md) *(EN)* | Riwayat rilis per codename versi. |
| [requirements.md](../requirements.md) *(ID)* | Spec teknis penuh; §17 adalah checklist pengembangan. |
| [CONTRIBUTING.md](../CONTRIBUTING.md) *(EN)* | Standar koding dan alur kontribusi. |

---

*Buitenzorg OS dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
