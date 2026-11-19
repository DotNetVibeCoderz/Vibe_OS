# opus-pure (vendored)

[opus-pure](https://crates.io/crates/opus-pure) 0.2.3, a pure-Rust Opus codec
(RFC 6716 / 7845), BSD-3-Clause (see `LICENSE`). Vendored for `CLRHOST.ELF`,
which has no `std`, by `scripts`-free patching (see the Buitenzorg history):

- `#![no_std]` + `alloc`; float methods through `num-traits` (libm);
- the `ogg` module (std::io; the CLR host reads Ogg itself, `src/ogg.rs`) and
  the `parallel` module (threaded encoding) are left out;
- x86 SIMD paths (runtime AVX/SSE detection) are compiled out -- the x86
  build of the CLR host is soft-float; the scalar code runs. ARM64 NEON stays;
- `LazyLock` -> `spin::Lazy`; `std::` items that are in `core` -> `core::`.

Measured against libopus (ffmpeg -c:a libopus): SNR 103 dB stereo, 105 dB
mono, ~58 us per 20 ms packet on the PC. (Tried first: `mousiki`, 19 dB;
`opus-decoder`, accurate but ~50x slower.)
