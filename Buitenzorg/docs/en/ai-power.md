# AI Subsystem & Power Management (v0.12) — English

Overview

The AI subsystem is implemented as a system service and includes simple local inference utilities suitable for offline demos and system integrations. Power management implements ACPI parsing and basic shutdown/reboot operations.

AI features

- LLM completion (small local model): a compact toy LLM for demonstration and local completion.
- Vision: simple edge detection (Sobel) and other small CV utilities.
- GenAI: procedural text-to-image generator for UI demos.
- Model manager: metadata-driven gallery (TinyLlama, phi-2, whisper, sd-turbo, plus built-in toy models). Model downloads and large-weight handling remain backlog items.

Power management

- ACPI parser: RSDP → RSDT/XSDT → FADT → PM1a_CNT_BLK, RESET_REG. Detects _S5 and shutdown states when available.
- Shutdown & restart: write SLP_TYPa|SLP_EN when ACPI supports it, otherwise fall back to QEMU/VirtualBox ports or soft reset methods.
- Sleep: light sleep implemented by halting CPU with blanked framebuffer until input arrives (full S3 suspend is backlog).

Next steps

Integrate production-scale LLM inference (GGUF/ONNX + GPU/NPU), model sandboxing & download, ACPI S3, and richer power UI and save/restore flows.