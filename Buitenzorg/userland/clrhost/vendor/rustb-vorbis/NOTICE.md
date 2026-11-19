# rustb-vorbis (vendored)

[rustb-vorbis](https://crates.io/crates/rustb-vorbis) 0.1.2, a pure-Rust port of
Sean Barrett's stb_vorbis, MIT OR Unlicense (see `LICENSE`). Vendored for
`CLRHOST.ELF`, which has no `std`, with a small patch
(`scripts`-free, by hand):

- `#![no_std]` + `alloc` imports in every module;
- the file reader goes through a local `Stream` trait (read + seek) instead of
  `std::io::{Read, Seek}`; opening a file by its path (`open_filename*`,
  `open_file`, `decode_filename*`) is removed;
- `core::fmt::Display` instead of `std::fmt`, no `std::error::Error` impl;
- `f32`/`f64` `sin`/`cos`/`exp`/`ln`/`powf`/`floor` through `libm`.

The decoding code itself is unchanged.
