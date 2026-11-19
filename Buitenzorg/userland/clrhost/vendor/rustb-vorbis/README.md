# rustb-vorbis

[![crates.io](https://img.shields.io/crates/v/rustb-vorbis.svg)](https://crates.io/crates/rustb-vorbis)
[![docs.rs](https://docs.rs/rustb-vorbis/badge.svg)](https://docs.rs/rustb-vorbis)

A pure-Rust Ogg Vorbis decoder, ported from Sean Barrett's
[stb_vorbis](https://github.com/nothings/stb/blob/master/stb_vorbis.c) (version 1.22).

The goal of this crate is simple: decode Ogg Vorbis exactly the way stb_vorbis does, in safe
Rust, with nothing else to install.

- **Bit-identical output.** The port follows stb_vorbis function by function and keeps its
  floating-point operations in the same order, so the samples you get are the same ones the C
  library produces, both as floats and as 16-bit integers.
- **No dependencies.** The crate uses only the Rust standard library.
- **No `unsafe` code.** The crate is built with `#![forbid(unsafe_code)]`, so a damaged or
  hostile file cannot make it read or write outside its buffers.
- **All of stb_vorbis's interfaces.** Decode a file held in memory, or one on disk that is read
  as it is needed, both with seeking. Or feed the decoder a stream piece by piece as the bytes
  arrive.
- **Fast.** On the same machine it decodes in less time than stb_vorbis built by gcc or clang
  with full optimisation, roughly a tenth to a quarter less, and the output is still the same
  to the bit. See [Speed](#speed).

## Contents

- [Getting started](#getting-started)
- [Is this the right decoder for you?](#is-this-the-right-decoder-for-you)
- [API overview](#api-overview)
- [Matching a particular build of stb_vorbis](#matching-a-particular-build-of-stb_vorbis)
- [Behaviour on damaged files](#behaviour-on-damaged-files)
- [How the port is verified](#how-the-port-is-verified)
- [Speed](#speed)
- [Limitations](#limitations)
- [Credits and licence](#credits-and-licence)

## Getting started

The crate is on [crates.io](https://crates.io/crates/rustb-vorbis). Add it to your project
with `cargo add rustb-vorbis`, or put it in your `Cargo.toml`:

```toml
[dependencies]
rustb-vorbis = "0.1"
```

The API documentation is on [docs.rs](https://docs.rs/rustb-vorbis).

To decode a whole file in one call:

```rust
let file = std::fs::read("sound.ogg")?;
let (channels, sample_rate, samples) = rustb_vorbis::decode_memory(&file)?;
// `samples` holds interleaved 16-bit samples.
```

To decode a little at a time, which is what you want for streaming playback or long files:

```rust
use rustb_vorbis::Vorbis;

let file = std::fs::read("sound.ogg")?;
let mut vorbis = Vorbis::open(&file)?;
let info = vorbis.info();
println!(
    "{} channels at {} Hz, {} samples long",
    info.channels,
    info.sample_rate,
    vorbis.stream_length_in_samples()
);

let mut buffer = vec![0i16; 4096 * info.channels];
loop {
    let frames = vorbis.get_samples_short_interleaved(info.channels, &mut buffer);
    if frames == 0 {
        break; // end of the stream
    }
    // buffer[..frames * info.channels] is ready to play or write out
}
```

Seeking works on sample positions:

```rust
vorbis.seek(44_100)?; // the next sample read is sample 44,100
```

### Reading a file from disk

From version 0.1.2, the decoder can also read the file itself, a few kilobytes at a time, so
a long file takes no more memory than a short one:

```rust
use rustb_vorbis::Vorbis;

let mut vorbis = Vorbis::open_filename("music.ogg")?;
// From here on it works as above: info, get_samples_short_interleaved, seek and the rest.
```

`rustb_vorbis::decode_filename("sound.ogg")` decodes a whole file from disk in one call.

If the sound sits inside a larger file, such as a game's archive, open that file, seek to
where the sound starts and pass it to `Vorbis::open_file_section` along with the sound's
length. `Vorbis::open_file` does the same for a sound that runs to the end of the file. Both
accept anything that implements `Read + Seek`, and you can pass `&mut file` if you would
like the file back when the decoder is done with it.

### Decoding a stream as it arrives

If the file is still arriving, for example over a network, `VorbisPush` (available from
version 0.1.1) decodes it piece by piece. You hand it the bytes you have, it tells you how many it used, and you hand it the
rest, plus whatever has arrived since, on the next call:

```rust
use rustb_vorbis::{VorbisError, VorbisPush};

// `receive_more` stands for however your bytes arrive. Here it appends them to the buffer
// and returns false once the stream has ended.
let mut bytes: Vec<u8> = Vec::new(); // received, not yet used

// The headers come first. Keep adding data until there is enough to read them.
let mut vorbis = loop {
    match VorbisPush::open(&bytes) {
        Ok((vorbis, used)) => {
            bytes.drain(..used);
            break vorbis;
        }
        Err(VorbisError::NeedMoreData) if receive_more(&mut bytes) => {}
        Err(error) => return Err(error.into()),
    }
};

// Then the audio, one frame at a time.
loop {
    let step = vorbis.decode_frame(&bytes);
    if step.bytes_used == 0 {
        // Not enough for a frame yet.
        if !receive_more(&mut bytes) {
            break; // the end of the stream
        }
        continue;
    }
    bytes.drain(..step.bytes_used);
    if step.samples > 0 {
        let left: &[f32] = vorbis.frame(0);
        // `step.samples` samples for each channel are ready to play
    }
}
```

If you jump to another place in the stream, or lose some data, call `vorbis.flush()`. The
decoder then finds the next page it can decode from by itself.

## Is this the right decoder for you?

Rust already has good Vorbis decoders written in Rust, notably
[lewton](https://crates.io/crates/lewton) and
[Symphonia](https://crates.io/crates/symphonia). If you would like a decoder that is part of a
wider audio library, with other formats and containers alongside it, one of those is probably
the better choice.

rustb-vorbis is aimed at a narrower need:

- You are moving a C or C++ project that uses stb_vorbis to Rust, and its audio output must
  not change.
- You would like a small, self-contained decoder with no dependencies at all.

The crate was written for [Noolith](https://github.com/OctoKeights/Noolith), a project whose
C++ code decoded its sounds with stb_vorbis and whose Rust rewrite needed to produce exactly
the same audio.

## API overview

The crate covers the whole of stb_vorbis's API. Function names follow the originals with the
`stb_vorbis_` prefix removed, so existing knowledge of the C library carries over.

`Vorbis` is the "pull" API, for a file held in memory or read from disk:

| stb_vorbis | rustb-vorbis |
| --- | --- |
| `stb_vorbis_open_memory` | `Vorbis::open` |
| `stb_vorbis_open_filename` | `Vorbis::open_filename` |
| `stb_vorbis_open_file`, `stb_vorbis_open_file_section` | `Vorbis::open_file`, `Vorbis::open_file_section` |
| `stb_vorbis_get_info`, `stb_vorbis_get_comment` | `info`, `vendor`, `comments` |
| `stb_vorbis_get_error` | `take_error` |
| `stb_vorbis_get_sample_offset`, `stb_vorbis_get_file_offset` | `sample_offset`, `file_offset` |
| `stb_vorbis_get_frame_float` | `get_frame_float`, then `frame(channel)` |
| `stb_vorbis_get_frame_short`, `..._short_interleaved` | `get_frame_short`, `get_frame_short_interleaved` |
| `stb_vorbis_get_samples_float`, `..._float_interleaved` | `get_samples_float`, `get_samples_float_interleaved` |
| `stb_vorbis_get_samples_short`, `..._short_interleaved` | `get_samples_short`, `get_samples_short_interleaved` |
| `stb_vorbis_seek`, `stb_vorbis_seek_frame`, `stb_vorbis_seek_start` | `seek`, `seek_frame`, `seek_start` |
| `stb_vorbis_stream_length_in_samples`, `..._in_seconds` | `stream_length_in_samples`, `stream_length_in_seconds` |
| `stb_vorbis_decode_memory`, `stb_vorbis_decode_filename` | `decode_memory`, `decode_filename` |
| `stb_vorbis_close` | dropping the decoder |

`VorbisPush` is the "push" API, for a file that arrives in pieces:

| stb_vorbis | rustb-vorbis |
| --- | --- |
| `stb_vorbis_open_pushdata` | `VorbisPush::open` |
| `stb_vorbis_decode_frame_pushdata` | `decode_frame`, then `frame(channel)` |
| `stb_vorbis_flush_pushdata` | `flush` |
| `stb_vorbis_get_info`, `stb_vorbis_get_comment` | `info`, `vendor`, `comments` |
| `stb_vorbis_get_error`, `stb_vorbis_get_sample_offset` | `take_error`, `sample_offset` |

In C, the two APIs share one type and calling a pull function on a push decoder is an error
at run time. Here they are separate types, so that mistake cannot be made.

Errors are reported as a `VorbisError`, which carries the same numeric codes as stb_vorbis's
`STBVorbisError` (available through `VorbisError::code`).

## Matching a particular build of stb_vorbis

One detail is worth knowing if you need bit-exact output. stb_vorbis builds its lookup tables
from π, and where it gets π from depends on the compiler:

- Where `<math.h>` defines `M_PI` (gcc and clang on Linux, macOS and MinGW), it uses that
  `double`.
- Where it does not (Microsoft's compiler, unless `_USE_MATH_DEFINES` is set), it falls back
  to a `float` constant of its own.
- With that `float`, and the file compiled as C++, some of the cosines and sines are computed
  by the single-precision functions, because of C++ overloading.

The resulting audio is the same to the ear. The samples differ only in their last bits, and
occasionally a 16-bit sample differs by one. Since "the same as stb_vorbis" can therefore mean
three slightly different things, the crate lets you choose:

| You want to match | Use |
| --- | --- |
| stb_vorbis built by gcc or clang | `Vorbis::open` / `decode_memory` (the default, `Pi::Double`) |
| stb_vorbis built as C by Microsoft's compiler | `Vorbis::open_with_pi(data, Pi::Float)` |
| stb_vorbis built as C++ by Microsoft's compiler | `Vorbis::open_with_pi(data, Pi::FloatCpp)` |

Every other way of opening a decoder has a `_with_pi` form that takes the same choice
(`Vorbis::open_filename_with_pi`, `decode_memory_with_pi`, `VorbisPush::open_with_pi` and so
on). One note on `Pi::FloatCpp`: the single-precision cosine and sine come from the system's
maths library, so it matches a C++ build made on the same system it runs on.

If you only need correct audio and are not comparing against a C build, the default is the
one to use.

## Behaviour on damaged files

For valid Vorbis files, the port and stb_vorbis agree exactly. For damaged files, the port
follows stb_vorbis as far as stb_vorbis stays within its own memory, and in practice it almost
always returns the same samples, positions and error codes. The known differences, which
apply to every way of decoding, are:

- **Out-of-bounds access.** Where stb_vorbis would read or write past a buffer, the port stops
  or reads zeros.
- **Assertions.** Where stb_vorbis asserts (for example, on out-of-order pages while seeking),
  the seek returns an error.
- **Overlong comments.** If a comment's length runs past the end of the comment header,
  stb_vorbis allocates the claimed length and pads it with bytes of 255. The port keeps only
  the bytes that are really there.
- **Overlong codebooks.** If a codebook claims more values than the setup header holds,
  stb_vorbis allocates the claimed size, keeps reading, and fails later. The port fails
  straight away, with the same error stb_vorbis would end on.
- **Failures without an error code.** stb_vorbis can occasionally fail to open a file without
  setting an error. The port reports `UnexpectedEof` in that case.
- **Not-a-number samples.** A file can contain values so extreme that the decoded samples are
  NaN. The sign of a NaN, and so whether it becomes -32768 or 32767 as a 16-bit sample,
  depends on the compiler. This is true of the C library as well.
- **Mono from multi-channel sound.** Asking `get_samples_short_interleaved` for one channel of
  a sound that has more is not supported by stb_vorbis (it asserts, or overruns the buffer if
  assertions are off). The port returns the same mono mix that `get_samples_short` gives.
- **Very large files.** Files larger than 2 GB are refused, because stb_vorbis takes the
  length as an `int`.

## How the port is verified

The repository includes a tool that checks the port against the C library directly.
`compare/dump.c` runs stb_vorbis over a set of files and prints the results of every function
it has. For a file in memory, that is each way of reading samples, down-mixing to one, two
and three channels, ten seeks to a sample and ten to a frame, and the decoder's position and
error state after each step. For a file on disk, each sound is opened by name, from a place
inside a larger file, and as a section said to be half as long as it is; each of those is
read to the end, sought in five places and read frame by frame. For the push API, the file
is fed in pieces of several sizes, down to a single byte at a time for small files, and the
decoder is then moved to four other places in the file to find its way again.
`examples/dump.rs` does the same with this crate, and `compare/compare.sh` builds both, runs
them over a folder of `.ogg` files and confirms that the output matches byte for byte:

```
compare/compare.sh <folder containing stb_vorbis.c> <folder of .ogg files> [double|float|float-cpp]
```

### Results so far

These comparisons were run on Linux (x86-64) against stb_vorbis built with gcc.

| Test set | Result |
| --- | --- |
| 23,622 sounds from three commercial games (S.T.A.L.K.E.R.: Shadow of Chernobyl, Clear Sky and Call of Pripyat; all mono) | Identical in the `double` build, from memory, from disk and through the push API: 2,350,638 lines of results, with no differences. The same sounds match in the `float-cpp` build through Noolith's own comparison against its C++ code. |
| 142 synthetic sounds encoded with ffmpeg (1 to 8 channels, 8 to 96 kHz, every libvorbis quality from -1 to 10, fixed bitrates, ffmpeg's native Vorbis encoder, silence, a sound shorter than one frame, and sounds long enough to exercise seeking), and the 8 sounds in `tests/data` | Identical in all three builds, all three ways. |
| 3,600 randomly damaged copies of small sounds (changed bytes, truncated files, dropped or overwritten blocks) | 3,598 identical, all three ways. stb_vorbis itself crashed on the other two. 2,213 of these files still open and decode. |
| Every single-bit change to the comment and setup headers of four small sounds (146,664 files, of which 64,267 still open) | 146,433 identical, all three ways. stb_vorbis crashed, hung or needed more than 20 seconds on 117. The remaining 114 are the differences described above: 71 in comment bytes, 40 in the sign of NaN samples, and 3 where stb_vorbis fails without an error code. |

These are the results for version 0.1.2, that is, with the file functions and after the work
on speed. The port did not panic on any of these files, and none of the differences is
specific to one way of decoding.

### What has not been verified

In the interest of being upfront about the gaps:

- For the `float` and `float-cpp` builds, gcc was given stb_vorbis's own constant in place of
  `M_PI`. The port has not yet been compared against a build made by Microsoft's compiler
  itself.
- The `float` build was last run over the 23,622 game sounds for version 0.1.0 (the pull API
  from memory: 968,502 lines, no differences). Since then it has been checked on the
  synthetic sounds only. The three builds differ in the numbers in their tables, not in the
  code that uses them.
- No encoder available for testing writes residue type 0 or codebooks with accumulating values
  (`sequence_p`), so those paths are exercised only by damaged files, where a changed bit
  switches them on and both decoders agree on the resulting noise.
- No test file contains a codebook of lookup type 2, so that code has been checked only by
  reading it against the C source.
- The scripts that generated the damaged files are not part of this repository.

### Running the tests

`cargo test` needs no C compiler. It decodes the sounds in `tests/data` and compares the
results with what stb_vorbis printed for them, which is stored in `tests/data/expected-*.txt`.
The sounds are synthetic and were made for these tests; `tests/data/README.md` describes how.

## Speed

From version 0.1.2 the port is faster than the C library it was made from. These are the
times to decode six minutes of sound (three one-minute files, two in stereo and one in mono,
twice each), and the number of instructions the processor executed to decode the three
files once in each of the three ways:

| Decoder | 16-bit samples | Float frames | 16-bit samples, from disk | Instructions |
| --- | --- | --- | --- | --- |
| rustb-vorbis 0.1.2 | 0.222 s | 0.225 s | 0.230 s | 5.37 billion |
| stb_vorbis, clang 18 `-O3` | 0.270 s | 0.246 s | 0.280 s | 7.07 billion |
| stb_vorbis, clang 18 `-O2` | 0.274 s | 0.262 s | 0.304 s | 7.17 billion |
| stb_vorbis, gcc 13 `-O3` | 0.301 s | 0.272 s | 0.312 s | 7.04 billion |
| stb_vorbis, gcc 13 `-O2` | 0.337 s | 0.298 s | 0.348 s | 7.65 billion |
| rustb-vorbis 0.1.1 | 0.495 s | 0.457 s | 0.490 s | 13.36 billion |

That is about 1,600 times as fast as the sound plays, 9 to 18% less time than the fastest
build of the C library here, and about twice the speed of version 0.1.1.

The gain does not come from `unsafe` code or from doing the arithmetic differently: every
floating-point operation is the one stb_vorbis does, in the same order, which is what keeps
the output identical. It comes from the code around the arithmetic:

- The inverse MDCT and the other per-sample loops work on slices that the compiler can see
  are in range, so nothing is checked sample by sample.
- 16-bit samples are converted several at a time.
- The residue, where most of a file's bits go, has a tight loop for its usual case, and the
  passes that a file does not use are skipped.
- A Huffman codeword and its length are found with one table lookup where stb_vorbis uses
  two, and the bit reader takes its bytes several at a time.
- Decoding a packet allocates no memory.

Some caution is due. The times are the best of 50 runs on one machine (a 2.1 GHz Xeon
virtual machine running Linux, with rustc 1.97), and they vary by several percent from run
to run; the instruction counts, from valgrind's callgrind, are exact but are not time. Other
processors, compilers and files will give other numbers, and Microsoft's compiler has not
been measured. You can measure on your own machine with the two programs the table was made
with, which decode the same files in the same ways:

```
cc -O3 -I <folder containing stb_vorbis.c> compare/bench.c -lm -o bench
./bench 2 <file.ogg>...
cargo run --release --example bench -- 2 <file.ogg>...
```

## Limitations

- **Inherited from stb_vorbis.** The port shares the original's limits: floor type 0 (found
  only in very old files, from before 2004) is not supported, only the first logical stream
  in a file is decoded, and page checksums are checked when seeking but not during normal
  decoding.
- **A quirk of reading from disk.** stb_vorbis reads an Ogg page that holds nothing (no
  segments) when the file is in memory, but takes it for the end of the file when it reads
  from disk. The port does the same in both cases. Encoders do not normally write such
  pages.

## Credits and licence

All of the decoding logic is the work of Sean Barrett and the stb_vorbis contributors listed
in the [original source](https://github.com/nothings/stb/blob/master/stb_vorbis.c). This crate
is a translation of that work into Rust.

Like stb_vorbis, rustb-vorbis is available under either the MIT licence or the Unlicense
(public domain), whichever you prefer. See [`LICENSE`](LICENSE) for the full text.

Bug reports and contributions are welcome. If you find a file that this crate decodes
differently from stb_vorbis, please open an issue: that is exactly the kind of bug this
project exists to rule out.
