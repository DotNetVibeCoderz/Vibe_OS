"""Write assets/music/LAGU6.OGG ("Angin Sore", Ogg Vorbis) and
assets/music/LAGU7.OPU ("Embun Pagi", Ogg Opus): two short original pieces
for Musik, synthesized here (plucked-string melody over soft chords) and
encoded with ffmpeg (libvorbis / libopus).

usage: python scripts/make-ogg-songs.py   (ffmpeg: $FFMPEG, else imageio-ffmpeg's, else PATH)
"""
import math
import os
import shutil
import struct
import subprocess
import tempfile
import wave

RATE = 22050


def ffmpeg():
    if os.environ.get('FFMPEG'):
        return os.environ['FFMPEG']
    try:
        import imageio_ffmpeg
        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        return shutil.which('ffmpeg')


def note(n):
    return 440.0 * 2 ** ((n - 69) / 12)


def render(melody, chords, bpm, seconds):
    beat = 60.0 / bpm
    out = [0.0] * int(seconds * RATE)

    def add(freq, start, dur, vol, decay, harm):
        a = int(start * RATE)
        for i in range(int(dur * RATE)):
            if a + i >= len(out):
                break
            t = i / RATE
            env = math.exp(-t * decay) * min(1.0, t * 200)
            v = sum(math.sin(2 * math.pi * freq * k * t) / k ** harm for k in (1, 2, 3))
            out[a + i] += vol * env * v

    t = 0.0
    for n, beats in melody:
        if n is not None:
            add(note(n), t, beats * beat + 0.6, 0.30, 2.2, 1.6)
        t += beats * beat
    t = 0.0
    for c in chords:
        for n in c:
            add(note(n), t, 4 * beat, 0.10, 0.5, 2.5)
        add(note(c[0] - 12), t, 4 * beat, 0.16, 0.9, 2.0)
        t += 4 * beat
    peak = max(abs(x) for x in out) or 1.0
    fade = int(1.5 * RATE)
    for i in range(fade):
        out[-1 - i] *= i / fade
    return [int(x / peak * 0.85 * 32767) for x in out]


def write(path, samples):
    with wave.open(path, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b''.join(struct.pack('<h', s) for s in samples))


songs = [
    ('LAGU6.OGG', 'Angin Sore', ['-c:a', 'libvorbis', '-q:a', '3'],
     [(67, 1), (69, .5), (71, .5), (74, 1), (71, 1), (69, 1), (67, 1), (64, 2),
      (67, 1), (69, 1), (71, .5), (69, .5), (67, 1), (62, 1), (64, 1), (67, 3)],
     [(55, 59, 62), (52, 55, 59), (48, 52, 55), (50, 54, 57)] * 2, 96, 22),
    ('LAGU7.OPU', 'Embun Pagi', ['-c:a', 'libopus', '-b:a', '48k'],
     [(72, .5), (74, .5), (76, 1), (79, 1), (76, .5), (74, .5), (72, 2), (69, 1), (72, 1),
      (74, .5), (76, .5), (74, 1), (72, 1), (69, 1), (72, 3)],
     [(48, 52, 55), (45, 48, 52), (41, 45, 48), (43, 47, 50)] * 2, 104, 20),
]
here = os.path.dirname(os.path.abspath(__file__))
dest = os.path.join(here, '..', 'assets', 'music')
ff = ffmpeg()
for name, title, codec, melody, chords, bpm, secs in songs:
    tmp = os.path.join(tempfile.gettempdir(), name + '.wav')
    write(tmp, render(melody, chords, bpm, secs))
    out = os.path.join(dest, name)
    fmt = ['-f', 'ogg'] if name.endswith('.OPU') else []
    subprocess.run([ff, '-hide_banner', '-loglevel', 'error', '-y', '-i', tmp, *codec, '-metadata', 'title=' + title, *fmt, out], check=True)
    os.remove(tmp)
    print('wrote', out, os.path.getsize(out), 'bytes')
