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
