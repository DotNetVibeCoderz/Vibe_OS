# ai/ — Layer 6 AI Subsystem

Tempat subsistem AI-native (requirements.md §6): LLM engine lokal, computer
vision, GenAI (image/audio/video), inference scheduler, Model Manager + galeri
Hugging Face, dan AI System API seragam.

**Status: belum dimulai.** Sesuai roadmap, subsistem ini dikerjakan di
**v0.12 "Nalar"**, *setelah* GPU compute API tersedia (v0.11 "Cahaya") —
AI bergantung pada compute API + fallback CPU (§20, mitigasi risiko).

Keputusan desain yang sudah dikunci oleh spesifikasi:
- Format model: GGUF / ONNX / safetensors; offline-first.
- Verifikasi checksum & lisensi sebelum menjalankan model; model di-sandbox.
- Akses app ke AI diatur permission broker (`ai.llm`, `ai.vision`, `ai.genai`
  di app.manifest — sudah didefinisikan di `sdk/manifest.schema.json`).
