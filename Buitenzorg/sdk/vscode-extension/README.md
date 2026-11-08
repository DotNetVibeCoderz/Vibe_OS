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
| **F5 / Run and Debug** (tipe `buitenzorg`) | debug skrip / program native di perangkat lewat `bz dap` | ✅ DAP |
| **Connect to Device (bz-link)** | `bz device connect` (UDP, `tcp://`, `serial://COM5`) | ✅ |
| **Deploy to Device** | `bz deploy` berkas aktif (atau folder proyek) ke perangkat yang hidup, lalu jalankan | ✅ |
| **Run Command on Device** | `bz run "..."` -- keluarannya di panel Output | ✅ |
| **Show Device Logs** | `bz logs -f` (log sistem) atau `bz logs APP -f` (satu program) | ✅ |
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

## Debugging

- **App di perangkat (DAP, tipe `buitenzorg`):** debug adapter-nya `bz dap`
  (`sdk/bz/Dap.cs`) yang bicara ke perangkat Buitenzorg yang hidup lewat
  bz-link. Buka berkas `.js/.ts/.py/.csx`, pasang breakpoint, tekan **F5**:
  continue, step over/in/out, **step back**, call stack, variabel, hover.
  Program native (`"program": "crashme"`): breakpoint fungsi, step instruksi,
  register, Debug Console → `dbg`. Panduan + batasan:
  [`docs/debugging.md`](../../docs/debugging.md#-vs-code-breakpoints-on-a-running-device-bz-dap).
  Ekstensi mencari `dist/bz/bz.dll`, lalu `sdk/bz/bin/Debug/net10.0/bz.dll`,
  lalu setelan `buitenzorg.bzPath`, lalu `bz` di PATH.
- **Kernel:** *Debug Kernel* meluncurkan QEMU dengan **GDB server** (`-s -S`);
  attach dari terminal: `gdb` → `target remote :1234`. Simbol kernel di
  `kernel/target/x86_64-unknown-none/release/bzkernel`.

## Catatan

- Ekstensi memanggil `pwsh` di Windows dan `bash` di Linux/macOS untuk skrip.
- Ekstensi memakai folder workspace teratas sebagai root repo.
- Untuk panduan lengkap alur app, lihat [`../../docs/first-app.md`](../../docs/first-app.md).
