"""Write assets/music/LAGU5.MID: "Kupu-kupu di Taman", an original short piece
(piano melody, string pad, bass, drums) for the MIDI synthesizer in
userland/clrhost/src/midi.rs. Standard MIDI File, format 1, 480 ticks per beat.

usage: python scripts/make-midi-song.py [out.mid]
"""
import struct
import sys

PPQ = 480


def vlq(n):
    out = [n & 0x7F]
    n >>= 7
    while n:
        out.insert(0, (n & 0x7F) | 0x80)
        n >>= 7
    return bytes(out)


def track(events):
    """events: (tick, bytes) -> an MTrk chunk with delta times."""
    data = b""
    last = 0
    for tick, msg in sorted(events, key=lambda e: e[0]):
        data += vlq(tick - last) + msg
        last = tick
    data += vlq(0) + b"\xff\x2f\x00"
    return b"MTrk" + struct.pack(">I", len(data)) + data


def notes(ch, seq, start=0, vel=96):
    """seq: (note or None, beats) pairs -> note on/off events."""
    ev, t = [], start
    for n, beats in seq:
        dur = int(beats * PPQ)
        if n is not None:
            ev.append((t, bytes([0x90 | ch, n, vel])))
            ev.append((t + dur - 20, bytes([0x80 | ch, n, 0])))
        t += dur
    return ev, t


# a pentatonic tune in C (C D E G A), two phrases, played twice
phrase_a = [(72, 1), (74, .5), (76, .5), (79, 1), (76, 1), (74, 1), (72, .5), (69, .5), (72, 2)]
phrase_b = [(76, 1), (79, .5), (81, .5), (79, 1), (76, 1), (74, .5), (76, .5), (74, .5), (72, .5), (72, 2)]
melody = (phrase_a + phrase_b) * 2
bars = 16
chords = [(48, 52, 55), (45, 48, 52), (41, 45, 48), (43, 47, 50)]  # C Am F G

tempo = b"\xff\x51\x03" + struct.pack(">I", 600000)[1:]  # 100 bpm
title = b"Kupu-kupu di Taman"
conductor = track([(0, tempo), (0, b"\xff\x03" + bytes([len(title)]) + title)])

mel, _ = notes(0, melody, vel=100)
piano = track([(0, b"\xc0\x00"), (0, b"\xb0\x07\x64")] + mel)

pad_ev = [(0, b"\xc1\x30"), (0, b"\xb1\x07\x46")]
bass_ev = [(0, b"\xc2\x21"), (0, b"\xb2\x07\x5a")]
for bar in range(bars):
    c = chords[bar % 4]
    t = bar * 4 * PPQ
    for n in c:
        pad_ev += [(t, bytes([0x91, n + 12, 60])), (t + 4 * PPQ - 30, bytes([0x81, n + 12, 0]))]
    for beat, n in enumerate([c[0], c[0], c[2], c[0] + 12]):
        bt = t + beat * PPQ
        bass_ev += [(bt, bytes([0x92, n - 12, 90])), (bt + PPQ - 40, bytes([0x82, n - 12, 0]))]
pad = track(pad_ev)
bass = track(bass_ev)

drum_ev = []
for bar in range(bars):
    t = bar * 4 * PPQ
    for beat in range(4):
        bt = t + beat * PPQ
        drum_ev += [(bt, b"\x99\x2a\x50"), (bt + 60, b"\x89\x2a\x00")]
        drum_ev += [(bt + PPQ // 2, b"\x99\x2a\x40"), (bt + PPQ // 2 + 60, b"\x89\x2a\x00")]
        if beat in (0, 2):
            drum_ev += [(bt, b"\x99\x24\x6e"), (bt + 100, b"\x89\x24\x00")]
        else:
            drum_ev += [(bt, b"\x99\x26\x5a"), (bt + 100, b"\x89\x26\x00")]
drums = track(drum_ev)

header = b"MThd" + struct.pack(">IHHH", 6, 1, 5, PPQ)
out = sys.argv[1] if len(sys.argv) > 1 else "assets/music/LAGU5.MID"
open(out, "wb").write(header + conductor + piano + pad + bass + drums)
print("wrote", out)
