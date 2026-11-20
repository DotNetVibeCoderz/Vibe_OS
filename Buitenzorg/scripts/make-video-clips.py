#!/usr/bin/env python3
"""Write assets/video/kebun.web (VP8 + Opus in WebM) and assets/video/senja.mp4
(Motion JPEG + MP3 in MP4): two original 5-second clips made from ffmpeg's
generated sources (a Mandelbrot zoom, moving gradients) and a synthesized
chord. ffmpeg comes from $FFMPEG, else imageio-ffmpeg's binary, else PATH (it
needs libvpx, libopus and libmp3lame). bzimage copies both onto /disk, where
the Video app opens them through Buitenzorg.Media.Movie.
"""
import os, shutil, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'assets', 'video')


def ffmpeg():
    if os.environ.get('FFMPEG'):
        return os.environ['FFMPEG']
    try:
        import imageio_ffmpeg
        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        return shutil.which('ffmpeg') or 'ffmpeg'


CHORD = ('aevalsrc=0.25*sin(2*PI*220*t)*(0.6+0.4*sin(2*PI*0.5*t))+0.15*sin(2*PI*330*t)'
         '+0.1*sin(2*PI*440*t+sin(2*PI*3*t)):s=48000:c=stereo')

CLIPS = [
    ('kebun.web', ['-f', 'lavfi', '-i', 'mandelbrot=s=320x180:rate=15:inner=convergence:outer=normalized_iteration_count',
                   '-f', 'lavfi', '-i', CHORD, '-t', '5', '-c:v', 'libvpx', '-b:v', '220k', '-auto-alt-ref', '0',
                   '-c:a', 'libopus', '-b:a', '32k', '-metadata', 'title=Kebun Fraktal', '-shortest', '-f', 'webm']),
    ('senja.mp4', ['-f', 'lavfi', '-i', 'gradients=s=320x180:speed=0.03:c0=0x1F4D2E:c1=0xF2B544:c2=0x7CC4A4:c3=0xE5735F:nb_colors=4:rate=10',
                   '-f', 'lavfi', '-i', CHORD, '-t', '5', '-c:v', 'mjpeg', '-q:v', '6', '-pix_fmt', 'yuvj420p',
                   '-c:a', 'libmp3lame', '-b:a', '64k', '-ar', '44100', '-metadata', 'title=Senja di Kebun', '-shortest']),
]

os.makedirs(OUT, exist_ok=True)
for name, args in CLIPS:
    path = os.path.join(OUT, name)
    subprocess.run([ffmpeg(), '-hide_banner', '-loglevel', 'error', '-y'] + args + [path], check=True)
    print('wrote', path, os.path.getsize(path), 'bytes')
sys.exit(0)
