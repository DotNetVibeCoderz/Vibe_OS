# Komputasi GPU dan LLM lokal

Buitenzorg menjalankan model bahasa dalam berkas GGUF (format llama.cpp yang
dipakai di Hugging Face) langsung di perangkat. Perkalian matriksnya
dikerjakan GPU bila kernel punya backend compute.

## Menjalankan model

```
llm                                  # model di setiap image, /disk/STORIES.GGU
llm Once upon a time --steps 100     # prompt, sampai 100 token baru
llm /disk/STORY15M.GGU A dragon --temp 0.8
llm --cpu ...                        # jangan pakai GPU
llm --gpu ...                        # beri tahu alasannya bila GPU tidak dipakai
run /disk/STORIES.GGU ...            # sama saja: berkas .ggu / .gguf adalah program
```

Program mencetak teks selagi dibuat, lalu
`N tokens in M ms (T tok/s, GPU V3D 4.2 QPU | CPU)`.

`/disk/STORIES.GGU` adalah stories260K, model TinyStories 260 ribu parameter
dari llama2.c (lisensi MIT, `assets/models/README.md`). Model llama yang lebih
besar juga jalan, misalnya `stories15M-q4_0.gguf` (19 MB) dari
`huggingface.co/ggml-org/models` (`tinyllamas/`). Unduh lewat Unduhan dan beri
nama 8.3 berakhiran `.GGU`.

## Apa yang dikerjakan mesinnya

`userland/clrhost/src/llm.rs` (bagian dari `CLRHOST.ELF`, no_std):

- **GGUF v2/v3:** metadata, tabel tensor, dan bobot **F32, F16, Q8_0 dan
  Q4_0**. Blok terkuantisasi dibuka ke f32 saat model dimuat, jadi model
  Q4_0 memakai RAM kira-kira 8 kali ukuran berkasnya.
- **Hanya llama:** RMSNorm, rotary embedding pada pasangan bersebelahan,
  grouped-query attention dengan KV cache (sampai 512 posisi), feed-forward
  SwiGLU, dan matriks keluaran yang terikat atau terpisah.
- **Tokenizer:** gaya SentencePiece dari kosakata GGUF (penggabungan menurut
  skor, `▁` untuk spasi, byte fallback `<0xNN>`).
- **Sampling:** greedy (`--temp 0`, bawaan) atau temperatur.

Mesin ini sudah dibandingkan dengan implementasi numpy dari matematika yang
sama. stories260K (F32) dan stories15M-q4_0 sama-sama menghasilkan token dan
teks yang identik.

## Komputasi GPU: `COMPUTE_OP`

Syscall 72 (`docs/abi.md`) memberi program buffer GPU dan **GEMV**
(`y = W x`, f32, W per baris). `kernel/bzkernel/src/compute.rs` berisi lapisan
generiknya:

- buffer dimiliki per proses dan dibebaskan saat proses keluar;
- backend berupa tabel fungsi (`compute::Native`);
- backend **referensi CPU kernel** (jenis 0) selalu ada, jadi jalur syscall
  berfungsi di semua mesin. Program memakainya hanya untuk menguji jalur itu,
  karena menghitung di program sendiri lebih cepat.

Backend GPU baru dipasang setelah `compute::install_native` menjalankan uji
GEMV-nya: matriks kecil yang hasilnya harus persis, dua kali, lalu satu
pengukuran 1024x1024. Bila gagal, backend tidak dipakai. Shell `compute`
menampilkan backend, jumlah GEMV dan waktunya; `compute test` mengulang uji.

**V3D (Raspberry Pi 4).** `platform/virt/v3d_compute.rs` menjalankan GEMV
lewat compute-shader dispatcher (CSD) di 8 QPU: satu job 8 batch x 16 lane,
dan setiap thread menentukan barisnya dari nomor QPU-nya.

- Kernelnya ditulis dengan assembler py-videocore6
  (`scripts/v3d-compute.py` -> `v3d_compute_kernels.rs`).
- Setiap thread mengerjakan 16 baris sekaligus dengan 8 load TMU berjalan
  bersamaan. Butterfly rotate menjumlahkan lane, lalu satu store TMU menulis
  16 hasil.
- Jumlah baris harus kelipatan 128 dan kolom kelipatan 64. Pemanggil
  menambal dengan nol; LLM melakukannya saat mengunggah bobot.
- Cache: L2T di-flush dan cache slice diinvalidasi sebelum job. Sesudahnya,
  TMU write combiner dan L2T di-clean.
- Job yang macet atau fault me-reset V3D, sama seperti backend 3D.

**Di Pi ini belum diuji.** Uji mandiri melindungi pemasangannya: bila gagal,
GEMV tetap di CPU dan `dmesg compute` menjelaskan sebabnya.

**Cara LLM memakainya.** Dengan backend selain referensi, `llm` mengunggah
setiap matriks sekali (Q/K/V digabung jadi satu, gate/up jadi satu).
Setiap token lalu butuh satu GEMV per matriks per lapisan plus matriks
keluaran; attention, norm dan sampling tetap di CPU.

## Untuk pembuat aplikasi

Model dan fungsi penglihatan bisa dipakai aplikasi, tidak hanya dari shell.
Semuanya berjalan di mesin ini; berkas model dimuat sekali per proses dan
dipakai bersama oleh semua pemanggil di proses itu.

**C# (managed): `Buitenzorg.AI`**, di `BZAPP.DLL` (`userland/clr-apps/Buitenzorg.App/Ai.cs`):

```csharp
using Buitenzorg.AI;

var m = LanguageModel.Load();               // /disk/STORIES.GGU; null -> LanguageModel.LastError
string text = m.Generate("Once upon a time", maxTokens: 48);      // suhu 0 = selalu sama
var g = m.Begin("The cat", maxTokens: 64, temperature: 0.8);
string? piece = g.Next();                    // panggil dari DispatcherTimer; null = selesai
foreach (var p in m.Stream("A dog", 20)) { } // bentuk IEnumerable
Console.WriteLine($"{m.Backend}: {m.LastTokensPerSecond:0} token/detik, {m.Parameters} parameter");

int[] edges = Vision.Edges(argb, w, h);      // juga Grayscale, Histogram(argb, bins), Sharpness
```

`LanguageModel.Load(path, Accelerator.Cpu | Gpu | Auto)` memilih backend
(`Gpu` = backend GPU `COMPUTE_OP` walaupun tidak terpilih otomatis). Di luar
perangkat (`dotnet X.dll --selftest`) model simulasi yang deterministik
menjawab, jadi uji mandiri aplikasi jalan di PC. Sisi host:
`userland/clrhost/src/aiapi.rs` (native `AiLoad`/`AiBegin`/`AiNext`/...; kerja
panjang berjalan di dalam `interp.blocking`, jadi thread managed lain tetap
jalan).

**Skrip UI (JavaScript / TypeScript / Python / C# script).** Skrip yang
membuka jendela (`window(...)`, dijalankan dengan `run BERKAS`) punya:

| fungsi | |
|---|---|
| `aiload([berkas])` | buka model (bawaan `/disk/STORIES.GGU`); bilangan negatif bila gagal |
| `aistart(awal, token[, suhu*1000])` + `ainext()` | bertahap: satu `ainext()` per frame, `""` = selesai |
| `aigenerate(awal, token[, suhu*1000])` | seluruh lanjutan sekaligus |
| `aibackend()`, `aiinfo(n)`, `aispeed()` | nama backend; 0 lapisan, 4 kosakata, 5 konteks, 6 parameter, 7 di GPU; token/detik putaran terakhir |

Di C# script namanya `Ai.Load()`, `Ai.Start(...)`, `Ai.Next()`,
`Ai.Generate(...)`, `Ai.Backend()`, `Ai.Info(n)`, `Ai.Speed()`. Skrip konsol
(`script js BERKAS`) berjalan di interpreter kernel dan tidak memilikinya.

**Template dan aplikasi contoh:**

- **Asisten AI** (`run masisten`, menu Start Kantor; `userland/clr-apps/MAsisten`):
  penulis cerita yang menampilkan jawaban model kata demi kata beserta
  kecepatan dan backend-nya, plus tab penglihatan (tepi, abu-abu, histogram
  kecerahan, ketajaman) atas foto di `/disk`. Uji mandirinya (13 pemeriksaan:
  teks bertahap = `Generate`, lanjutan greedy yang sudah diketahui, jumlah
  token, Sobel/histogram) ikut di rangkaian managed saat boot.
- **Studio:** *Penulis Cerita AI* (JS), *Pelengkap Kalimat AI* (Python), *Cerita AI* (C# script), kelompok "AI".
- **MagicAppGen:** `managed-ai` (jendela C#) dan `js-ai` (skrip UI); Jack
  mengenal API-nya (`GetApiReference("ai")`). Run di MagicAppGen menjalankan
  skrip yang memanggil `window(` dengan `run`.
- **VS Code / `bz new`:** `ai-csharp`, `ai-js`. `bz new` kini mengisi
  `__APPNAME__`, `__NAME83__` dan `__BZROOT__` di isi dan nama berkas template,
  sehingga template managed bisa merujuk pustaka di checkout Anda.

## Uji mandiri

- `MILESTONE: COMPUTE OK (reference: ...; GPU backend: ...)`: setiap kernel,
  boot self-test.
- `MILESTONE: COMPUTE GPU OK (...)`: bila backend GPU lulus (Pi 4).
- `MILESTONE: LLM OK (...)`: `CLRHOST.ELF /disk/STORIES.GGU --selftest`.
  1. Tokenizer dan 24 token greedy harus sama dengan referensi numpy
     (", there was a little girl named Lily.").
  2. Generasi yang sama diulang dengan semua perkalian matriks lewat
     `COMPUTE_OP` (GPU, atau referensi kernel), dan tokennya harus identik.

## Belum

- Backend compute untuk Intel Gen9 (GPGPU walker di render engine) dan virgl
  (compute shader GL 4.3).
- GEMV langsung pada bobot terkuantisasi (Q4_0 / Q8_0 di GPU).
- Pemrosesan prompt per batch.
- API managed `Buitenzorg.AI` di atas mesin ini; `ask` masih memakai model
  bigram mainan di `ai.rs`.
