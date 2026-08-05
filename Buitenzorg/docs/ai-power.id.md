# Subsistem AI & Power Management (v0.12 "Nalar")

[English](ai-power.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

![Subsistem AI v0.12 "Nalar" — galeri model + power CLI](img/desktop-nalar.png)

## Subsistem AI (`ai.rs`, `model.rs`)

AI sebagai layanan sistem (requirements.md §6), diperkecil agar berjalan offline
di kernel/CPU (LLM produksi butuh model GGUF/ONNX + GPU/NPU — backlog).

- **AI System API** (`ai.rs`):
  - `llm_complete(prompt, n)` — **model bigram char-level nyata**: dilatih dari
    korpus kecil (statistik karakter→karakter), lalu meng-generate teks. LLM lokal
    sungguhan, hanya skala mainan.
  - `vision_edges(gray, w, h, thr)` — deteksi tepi **Sobel** (computer vision).
  - `genai_image(prompt, w, h)` — text-to-image prosedural (hash prompt → pola).
  - Shell: `ask <prompt>`.
- **Model Manager** (`model.rs`): galeri gaya **Hugging Face** dengan metadata
  (task, ukuran, VRAM, lisensi, format): TinyLlama, phi-2, whisper, trocr,
  sd-turbo, plus model bawaan `buitenzorg/nalar-bigram`. `bz model list/pull/info`
  ("pull" mendaftarkan model sebagai tersedia offline; download bobot nyata =
  backlog).
- Verifikasi: `ai::selftest` menjalankan ketiganya → `AI OK`.

## Power Management (`power.rs`)

Shutdown / Restart / Sleep, via ACPI dengan fallback VM.

- **Parser ACPI**: RSDP (dari `BootInfo.rsdp_addr` bootloader) → RSDT/XSDT → FADT →
  `PM1a_CNT_BLK`, `RESET_REG`; scan DSDT untuk paket `\_S5` (SLP_TYPa).
- **Shutdown** — tulis `SLP_TYPa | SLP_EN` ke PM1a_CNT; fallback port QEMU
  (0x604/0xB004) & VirtualBox (0x4004). **Teruji: QEMU power off (exit 0).**
- **Restart** — ACPI reset register (bila ada); fallback pulse reset
  keyboard-controller (`0x64 ← 0xFE`); triple-fault (null IDT + int3) upaya akhir.
- **Sleep** — *light sleep*: blank framebuffer + `hlt` sampai ada input
  mouse/keyboard (ACPI S3 suspend-to-RAM = backlog).
- Shell: `shutdown`, `restart`/`reboot`, `sleep`; `bz power off|restart|sleep`;
  `bz power` menampilkan status ACPI.

> Untuk memverifikasi shutdown tanpa merusak boot normal: tambah `power::shutdown()`
> sementara di akhir `nalar_demo`, boot, pastikan QEMU keluar sendiri, lalu hapus
> (jangan ditinggal — mematikan boot normal & smoke test).

## Berikutnya

LLM skala produksi (GGUF/ONNX di GPU/NPU) + inference scheduler, CV/GenAI
audio-video, download model nyata + sandbox, ACPI S3, power-menu GUI +
konfirmasi, dan flush/save-state sebelum perubahan power.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*
