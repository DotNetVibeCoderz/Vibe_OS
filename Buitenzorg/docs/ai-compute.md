# GPU compute and the local LLM

Buitenzorg runs language models in GGUF files (the llama.cpp format used on
Hugging Face) on the device itself, and does their matrix products on the GPU
when the kernel has a compute backend.

## Running a model

```
llm                                  # the model on every image, /disk/STORIES.GGU
llm Once upon a time --steps 100     # a prompt, up to 100 new tokens
llm /disk/STORY15M.GGU A dragon --temp 0.8
llm --cpu ...                        # never use the GPU
llm --gpu ...                        # say why when the GPU is not used
run /disk/STORIES.GGU ...            # the same thing: a .ggu / .gguf file is a program
```

The program prints the text as it is generated and then
`N tokens in M ms (T tok/s, GPU V3D 4.2 QPU | CPU)`.

`/disk/STORIES.GGU` is stories260K, the 260 000-parameter TinyStories model
from llama2.c (MIT licence, `assets/models/README.md`). Larger llama models
work too, for example `stories15M-q4_0.gguf` (19 MB) from
`huggingface.co/ggml-org/models` (`tinyllamas/`). Download them with Unduhan
and give them an 8.3 name ending in `.GGU`.

## What the engine does

`userland/clrhost/src/llm.rs` (part of `CLRHOST.ELF`, no_std):

- **GGUF v2/v3:** metadata, the tensor table, and **F32, F16, Q8_0 and Q4_0**
  weights. Quantised blocks are expanded to f32 when the model loads, so a
  Q4_0 model takes about 8 times its file size in RAM.
- **llama only:** RMSNorm, rotary embedding on adjacent pairs, grouped-query
  attention with a KV cache (up to 512 positions), a SwiGLU feed-forward,
  and a tied or separate output matrix.
- **Tokenizer:** SentencePiece style from the GGUF vocabulary (score-ordered
  merges, `▁` for spaces, byte fallback `<0xNN>`).
- **Sampling:** greedy (`--temp 0`, the default) or temperature.

It was checked against a numpy implementation of the same maths. Both
stories260K (F32) and stories15M-q4_0 produce identical tokens and text.

## GPU compute: `COMPUTE_OP`

Syscall 72 (`docs/abi.md`) gives programs GPU buffers and a **GEMV**
(`y = W x`, f32, W row-major). `kernel/bzkernel/src/compute.rs` holds the
generic layer:

- buffers are owned per process and freed when the process exits;
- a backend is a function table (`compute::Native`);
- the **kernel CPU reference** backend (kind 0) is always there, so the
  syscall path works on every machine. Programs use it only to test the path,
  because computing in the program itself is faster.

A GPU backend is installed only after `compute::install_native` runs its GEMV
self-test: an exact small matrix twice, then a 1024x1024 timing run. On
failure the backend is not used. Shell `compute` shows the backend, the GEMV
count and the time; `compute test` runs the self-test again.

**V3D (Raspberry Pi 4).** `platform/virt/v3d_compute.rs` dispatches the GEMV
through the compute-shader dispatcher (CSD) on the 8 QPUs: one job of
8 batches x 16 lanes, every thread finding its rows from its QPU number.

- The kernel is written with py-videocore6's assembler
  (`scripts/v3d-compute.py` -> `v3d_compute_kernels.rs`).
- Each thread works on 16 rows at a time, with 8 TMU loads in flight. A
  rotate butterfly sums the lanes, and one TMU store writes the 16 results.
- Rows must be a multiple of 128 and columns a multiple of 64. Callers pad
  with zeros: the LLM does this when it uploads its weights.
- Caches: L2T is flushed and the slice caches invalidated before the job.
  After it, the TMU write combiner and L2T are cleaned.
- A job that hangs or faults resets the V3D, as in the 3D backend.

**On the Pi this is untested so far.** The self-test protects the install:
if it fails, GEMV stays on the CPU and `dmesg compute` says why.

**How the LLM uses it.** With a backend other than the reference, `llm`
uploads every matrix once (Q/K/V fused into one, gate/up into one). Each
token then costs one GEMV per matrix and layer plus the output matrix, and
attention, norms and sampling stay on the CPU.

## For app authors

The model and the vision functions are available to applications, not only to
the shell. Everything runs on the machine; the model file is loaded once per
process and shared by every caller in it.

**C# (managed): `Buitenzorg.AI`**, in `BZAPP.DLL` (`userland/clr-apps/Buitenzorg.App/Ai.cs`):

```csharp
using Buitenzorg.AI;

var m = LanguageModel.Load();               // /disk/STORIES.GGU; null -> LanguageModel.LastError
string text = m.Generate("Once upon a time", maxTokens: 48);      // temperature 0 = always the same
var g = m.Begin("The cat", maxTokens: 64, temperature: 0.8);
string? piece = g.Next();                    // call from a DispatcherTimer; null = done
foreach (var p in m.Stream("A dog", 20)) { } // the IEnumerable form
Console.WriteLine($"{m.Backend}: {m.LastTokensPerSecond:0} tokens/s, {m.Parameters} parameters");

int[] edges = Vision.Edges(argb, w, h);      // also Grayscale, Histogram(argb, bins), Sharpness
```

`LanguageModel.Load(path, Accelerator.Cpu | Gpu | Auto)` picks the backend
(`Gpu` = the `COMPUTE_OP` GPU backend even when it was not chosen
automatically). Off the device (`dotnet X.dll --selftest`) a deterministic
simulated model answers, so app self-tests run on a PC. The host side is
`userland/clrhost/src/aiapi.rs` (natives `AiLoad`/`AiBegin`/`AiNext`/...;
long work runs inside `interp.blocking`, so other managed threads keep going).

**UI scripts (JavaScript / TypeScript / Python / C# script).** A script that
opens a window (`window(...)`, started with `run FILE`) has:

| function | |
|---|---|
| `aiload([file])` | open a model (default `/disk/STORIES.GGU`); a negative number on failure |
| `aistart(prompt, tokens[, temperature*1000])` + `ainext()` | stream: one `ainext()` per frame, `""` = done |
| `aigenerate(prompt, tokens[, temperature*1000])` | the whole continuation at once |
| `aibackend()`, `aiinfo(n)`, `aispeed()` | backend name; 0 layers, 4 vocabulary, 5 context, 6 parameters, 7 on GPU; tokens/s of the last run |

In C# script they are `Ai.Load()`, `Ai.Start(...)`, `Ai.Next()`,
`Ai.Generate(...)`, `Ai.Backend()`, `Ai.Info(n)`, `Ai.Speed()`. Console
scripts (`script js FILE`) run in the kernel's interpreter and do not have
them.

**Templates and a sample app:**

- **Asisten AI** (`run masisten`, Start menu Kantor; `userland/clr-apps/MAsisten`):
  a story writer that streams the model's answer with its speed and backend,
  plus a vision tab (edges, grayscale, brightness histogram, sharpness) on the
  photos on `/disk`. Its self-test (13 checks: streamed text = `Generate`, the
  known greedy continuation, token counts, Sobel/histogram) runs in the boot's
  managed suite.
- **Studio:** *Penulis Cerita AI* (JS), *Pelengkap Kalimat AI* (Python), *Cerita AI* (C# script), group "AI".
- **MagicAppGen:** `managed-ai` (C# window) and `js-ai` (UI script); Jack
  knows the API (`GetApiReference("ai")`). MagicAppGen's Run starts any
  script that calls `window(` with `run`.
- **VS Code / `bz new`:** `ai-csharp`, `ai-js`. `bz new` now fills
  `__APPNAME__`, `__NAME83__` and `__BZROOT__` in a template's files and
  file names, which is what lets a managed template reference the libraries of
  your checkout.

## Self-tests

- `MILESTONE: COMPUTE OK (reference: ...; GPU backend: ...)`: every kernel,
  self-test boot.
- `MILESTONE: COMPUTE GPU OK (...)`: when a GPU backend passed (Pi 4).
- `MILESTONE: LLM OK (...)`: `CLRHOST.ELF /disk/STORIES.GGU --selftest`.
  1. The tokenizer and 24 greedy tokens must match the numpy reference
     (", there was a little girl named Lily.").
  2. The same generation runs again with every matrix product through
     `COMPUTE_OP` (a GPU, else the kernel reference), and the tokens must be
     identical.

## Not done yet

- Compute backends for Intel Gen9 (GPGPU walker on the render engine) and
  virgl (GL 4.3 compute shaders).
- GEMV directly on quantised weights (Q4_0 / Q8_0 on the GPU).
- Batched prompt processing.
- A managed `Buitenzorg.AI` API over the engine; `ask` still uses the toy
  bigram model in `ai.rs`.
