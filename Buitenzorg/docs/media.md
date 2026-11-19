# Sound and media

Every program can play sound at the same time: the kernel keeps one stream
per program, mixes them, and feeds the sound card from a ring that never
stops while there is something to play. **Musik** plays the WAV files on the
disk; two original pieces come with every image.

![Musik playing "Embun Pagi di Kebun Raya" (QEMU, AC'97)](img/musik.png)

## Musik

`run mmusik`, or **Start > Multimedia > Musik**. The list shows the WAV,
FLAC (`.FLA` on the FAT disk, `.FLAC`) and MP3 files of `/disk` and the scratch folder (`/ram`); click one, or Space to play /
pause; Sebelumnya / Berikutnya; click the progress bar to jump; the slider
sets *this program's* volume in the mixer (the master volume stays in
Pengaturan). The two songs on every image -- *Embun Pagi di Kebun Raya* and
*Hujan Sore di Bogor* (`LAGU1.WAV`, `LAGU2.WAV`) -- are synthesized when the
image is built (`synth_song` in `kernel/bzimage/build.rs`: bell tones on a
five-note scale over a slow gong bass); a third, *Tetes Embun* (`LAGU3.FLA`,
`assets/music`), is lossless FLAC. FLAC is decoded to PCM by the host when a
song is loaded (`userland/clrhost/src/flac.rs`: fixed and LPC predictors,
Rice residuals, stereo decorrelation, 8 to 24 bits -- output bit-exact with
ffmpeg's), the title comes from its `TITLE=` comment. MP3 (and MP1 / MP2) goes
through the `nanomp3` crate, a safe Rust port of minimp3 (within one step of
ffmpeg's output; ID3 tags skipped, encoder delay and padding trimmed), the
title from the ID3v2 `TIT2` frame; `LAGU4.MP3` (*Gerimis*) is on every image.
MIDI files (`.MID`) are played by a small synthesizer in the host
(`userland/clrhost/src/midi.rs`): format 0 and 1, tempo changes, 48 voices
with a timbre per General MIDI instrument family (piano, organ, guitar,
bass, strings, brass / reed, flute, synth) and an envelope, channel 10 as
drums (kick, snare, hi-hat, toms), rendered at 22 kHz mono; the title is the
first track name. *Kupu-kupu di Taman* (`LAGU5.MID`, made by
`scripts/make-midi-song.py`) is on every image -- MIDI is also what games
can ship as music: a few KB per song.

## Gambar (pictures)

`run mimage`, or **Start > Multimedia > Gambar**. It lists the pictures of
`/disk` and the scratch folder -- **BMP, PNG, JPEG (baseline and progressive)
and GIF** -- and shows the chosen one fitted to the window. Zoom with
Perkecil / Pas / 1:1 / Perbesar, `+` `-` `0` `1` or the wheel; drag (or
W A S D) to pan a zoomed picture; Left / Right or `<` `>` for the previous /
next one; **Slide show** (or Space) moves on every 3 s. Animated GIFs play.
Three samples come with every image: `KEBUN.JPG`, `ANGGREK.PNG` (with
transparency, shown over a checkerboard) and `DAUN.GIF` (12 frames).

![Gambar showing ANGGREK.PNG (QEMU)](img/gambar.png)

## Video

![Video playing *Fajar di Kebun Raya*](img/video.png)

`run mvideo`, or **Start > Multimedia > Video**: plays AVI files with
Motion-JPEG pictures and PCM sound (`/disk/*.AVI`); Space plays / pauses,
click the bar to jump, **Dari awal** restarts. The picture follows the
sound's position (what the mixer has really played), so they stay in step.
Every image carries an original 12-second clip, *Fajar di Kebun Raya*
(`FAJAR.AVI`: 320x180, 10 fps, 16 kHz sound), synthesized when the image is
built (`synth_video` in `kernel/bzimage/build.rs`). Frames are decoded and
scaled by the host's JPEG decoder (`Picture`). Not supported: other codecs
(H.264, VP9 ...), MP4/MKV containers, compressed sound.

## For programs

```csharp
using Buitenzorg.Media;

var player = new MediaPlayer();
player.Load("/disk/LAGU1.WAV");     // RIFF/WAVE PCM: 8-bit or 16-bit, mono or stereo, any rate
player.Play();
var timer = new DispatcherTimer(100);
timer.Tick += () => player.Pump();  // keeps ~0.6 s queued in the mixer
timer.Start();
// player.Pause() / Stop() / Seek(seconds), .Position, .Duration, .Title (LIST/INFO INAM), .Level, Ended
```

Lower level, `Buitenzorg.App.Audio`:

| | |
|---|---|
| `Audio.Write(data, offset, count, Audio.Format(rate, channels, bits))` | queue PCM in this program's stream; returns the bytes taken (fewer when ~2 s are queued) |
| `Audio.QueuedMs`, `Audio.CanStream` | what is waiting; whether the card streams |
| `Audio.AppVolume(pid, volume)` | a program's level in the mix (pid 0 = this one; -1 = read) |
| `Audio.Beep(hz, ms)`, `Audio.Volume` | a tone; the master volume |

Pictures, `Buitenzorg.Media.Picture`:

```csharp
var pic = Picture.Load("/disk/KEBUN.JPG");   // BMP, PNG, JPEG, GIF; null if not a picture
// pic.Width, pic.Height, pic.Format ("JPEG"), pic.FrameCount (> 1 for an animated GIF)
var px = new int[320 * 180];
int delay = pic.Render(frame: 0, px, 320, 180, sx: 0, sy: 0, sw: pic.Width, sh: pic.Height);
dc.Blit(x, y, 320, 180, px);                 // delay = the frame's time in ms (0 = still)
```

Decoding and scaling run in the host (`userland/clrhost/src/imgdecode.rs`,
natives `ImageInfo` / `ImageDecodeScaled`; the decoded picture is cached),
so the interpreter never loops over pixels. Off the device only BMP decodes.

Syscalls: **`AUDIO_WRITE` (62)** -- samples, length, format word `rate |
channels << 20 | bits << 24` (0 = 48 kHz stereo 16-bit) -- and
**`AUDIO_APP_VOLUME` (63)**. In the Terminal, `mixer` lists the streams
(`mixer PID VOLUME` sets one).

## How it works

`kernel/bzkernel/src/mixer.rs` converts each write to 48 kHz stereo 16-bit
(linear interpolation for other rates), keeps up to two seconds per program,
and sums the streams at their volumes with saturation. The driver's pump
(`audio::stream_pump`, called by `AUDIO_WRITE` and the desktop loop) fills
the ring: on AC'97 32 DMA buffers (680 ms) whose last-valid index moves ahead
of the playing one, on Intel HD Audio the 16 cyclic pages (340 ms) behind the
link position. A whole ring of silence stops the card; the next write starts
it again. One-shot sounds (`Audio.Beep`, the AUDIO_PLAY syscall) join the mix
as the system's stream while music plays. Boot checks: `MILESTONE: MIXER OK`
(conversion, volume, saturation -- every architecture) and `MILESTONE: AUDIO
STREAM OK` (a second of tone through the ring on the PC's card); Musik's
self-test plays through `AUDIO_WRITE` in the boot's managed suite.

## Limits

- Streaming works on **PC** sound cards (AC'97, Intel HD Audio -- tested on
  the Dell Latitude 7290's Realtek codec for one-shot sounds; streaming was
  verified under QEMU's HDA and AC'97). On ARM64 / RISC-V `virt` and the
  Raspberry Pi the mixer is there but no card streams yet (`Audio.CanStream`
  is false; the Pi still plays one-shot sounds on its jack and HDMI).
- Pictures: no EXIF rotation, thumbnails or editing; GIF frames are decoded
  up front (fine for small animations).
- WAV, FLAC, MP3 and MIDI: no Ogg Vorbis / Opus decoder yet; the MIDI synth
  has no SoundFont, reverb or stereo panning; a song is decoded
  whole when it is loaded (a few seconds for a long MP3 under QEMU); no microphone; the level meter
  shows peaks, not a spectrum.

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*
