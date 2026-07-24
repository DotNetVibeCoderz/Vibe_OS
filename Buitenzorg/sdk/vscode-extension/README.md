# Buitenzorg SDK — VS Code Extension

Membangun, menjalankan, dan men-debug app Buitenzorg OS langsung dari VS Code.
Semua perintah men-shell-out ke skrip repo (`scripts/…`) dan CLI `bz`, memakai
pipeline yang sama dengan yang membangun app suite bawaan.

## Perintah (Command Palette → "Buitenzorg:")

| Perintah | Fungsi | Status |
|----------|--------|--------|
| **New Project (pick template)** | scaffold via `bz new <template> <name>` dengan **pemilih template** (desktop-csharp, console-csharp, js-app, ts-app, python-app) | ✅ berfungsi |
| **Build & Run in QEMU** | build app C# + image kernel, lalu boot di QEMU (loop dev) | ✅ berfungsi |
| **Deploy (build image + smoke test)** | `build.ps1` + `smoke-test.ps1` (verifikasi 4 media) | ✅ berfungsi |
| **Debug Kernel (QEMU + GDB server)** | boot QEMU ditahan dengan server GDB (`-s -S`) di `:1234` untuk di-attach | ✅ GDB kernel-level |
| **Validate app.manifest** | `bz manifest validate` | ✅ berfungsi |

## Membangun & memasang ekstensi

```bash
cd sdk/vscode-extension
npm install
npm run compile              # tsc -> out/extension.js
```
Lalu tekan **F5** di VS Code (Extension Development Host) untuk mencobanya, atau
paket-kan dengan `vsce package` untuk menghasilkan `.vsix` dan
`code --install-extension buitenzorg-sdk-*.vsix`.

## Debugging — status & rencana

- **Sekarang:** *Debug Kernel* meluncurkan QEMU dengan **GDB server** (`-s -S`);
  attach dari terminal: `gdb` → `target remote :1234`. Simbol kernel di
  `kernel/target/x86_64-unknown-none/release/bzkernel`. Konfigurasi debug tipe
  `buitenzorg` (di `.vscode/launch.json` template) memicu alur ini.
- **Menyusul (DAP penuh):** breakpoint tingkat-app yang memetakan baris C# ke
  eksekusi ring-3 (debug adapter khusus). Ini pekerjaan berikutnya — lihat
  `PLAN.md` (Developer Experience) & `requirements.md` §13.

## Catatan

- Ekstensi memanggil `pwsh` di Windows dan `bash` di Linux/macOS untuk skrip.
- Ekstensi memakai folder workspace teratas sebagai root repo.
- Untuk panduan lengkap alur app, lihat [`../../docs/first-app.md`](../../docs/first-app.md).
