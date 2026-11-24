#!/usr/bin/env python3
"""Draw the app icons that have no picture yet (assets/icons/<launch>.jpg).

The first icons were AI-generated; these are drawn by this program instead, in
the same style: a full-bleed vertical gradient, one glossy object in the middle
with a soft shadow. bzimage rounds the corners and scales them to 44 / 18 px
(build_icon_pack), so draw big and simple: shapes read at 44 px, details do not.

    python scripts/make-icons.py            # every icon below (overwrites)
    python scripts/make-icons.py mular mbata

Needs Pillow. Output: 256 x 256 baseline JPEG (the decoder in bzimage reads
baseline only), drawn at 4x and downscaled for smooth edges.
"""

import math
import os
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

S = 1024  # drawing size (4x the output)
OUT = 256
HERE = os.path.dirname(os.path.abspath(__file__))
ICONS = os.path.join(HERE, "..", "assets", "icons")


def hexc(v):
    return ((v >> 16) & 255, (v >> 8) & 255, v & 255)


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def lighten(c, t):
    return mix(c, (255, 255, 255), t)


def darken(c, t):
    return mix(c, (0, 0, 0), t)


def background(top, bottom):
    """Vertical gradient with a soft light spot top-left (like the AI icons)."""
    top, bottom = hexc(top), hexc(bottom)
    img = Image.new("RGB", (S, S))
    px = img.load()
    for y in range(S):
        row = mix(top, bottom, y / (S - 1))
        for x in range(S):
            d = math.hypot(x - S * 0.3, y - S * 0.25) / S
            px[x, y] = lighten(row, max(0.0, 0.18 - d * 0.25))
    return img


def new_mask():
    return Image.new("L", (S, S), 0)


def gradient(c_top, c_bottom, y0=0, y1=S):
    g = Image.new("RGB", (S, S))
    d = ImageDraw.Draw(g)
    for y in range(S):
        t = min(1.0, max(0.0, (y - y0) / max(1, y1 - y0)))
        d.line([(0, y), (S, y)], fill=mix(c_top, c_bottom, t))
    return g


def bbox_y(mask):
    b = mask.getbbox()
    return (b[1], b[3]) if b else (0, S)


def paint(img, mask, color, shadow=True, gloss=0.35, rim=True, flat=False):
    """Composite a shape (mask) filled with a vertical gradient of `color`,
    with a drop shadow, a top highlight and a thin darker rim."""
    c = hexc(color) if isinstance(color, int) else color
    y0, y1 = bbox_y(mask)
    if shadow:
        sh = mask.filter(ImageFilter.GaussianBlur(S // 40))
        sh = ImageChops.offset(sh, 0, S // 45)
        dark = Image.new("RGB", (S, S), (0, 0, 0))
        img.paste(dark, (0, 0), sh.point(lambda v: int(v * 0.42)))
    fill = c if flat else None
    if fill is None:
        fill_img = gradient(lighten(c, 0.22), darken(c, 0.18), y0, y1)
    else:
        fill_img = Image.new("RGB", (S, S), fill)
    img.paste(fill_img, (0, 0), mask)
    if rim:
        edge = ImageChops.subtract(mask, mask.filter(ImageFilter.MinFilter(9)))
        img.paste(Image.new("RGB", (S, S), darken(c, 0.35)), (0, 0), edge.point(lambda v: int(v * 0.6)))
    if gloss > 0:
        # a highlight on the upper part of the shape
        hl = new_mask()
        top = y0 + (y1 - y0) * 0.55
        hd = ImageDraw.Draw(hl)
        for y in range(int(y0), int(top)):
            hd.line([(0, y), (S, y)], fill=int(255 * gloss * (1 - (y - y0) / max(1, top - y0))))
        hl = ImageChops.multiply(hl, mask.filter(ImageFilter.MinFilter(13)))
        img.paste(Image.new("RGB", (S, S), (255, 255, 255)), (0, 0), hl)


def shape(fn):
    m = new_mask()
    fn(ImageDraw.Draw(m))
    return m


def save(img, name):
    out = img.resize((OUT, OUT), Image.LANCZOS)
    path = os.path.join(ICONS, name + ".jpg")
    out.save(path, "JPEG", quality=92, progressive=False, optimize=True)
    print("wrote", os.path.relpath(path))


def star4(d, cx, cy, r, fill=255):
    """A four-pointed sparkle."""
    pts = []
    for i in range(8):
        a = math.pi / 4 * i - math.pi / 2
        rr = r if i % 2 == 0 else r * 0.28
        pts.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
    d.polygon(pts, fill=fill)


# ---- the icons ------------------------------------------------------------------------


def mmusik():
    img = background(0xB99AE8, 0x5B3C9E)
    note = shape(lambda d: (
        d.ellipse([250, 640, 470, 800], fill=255),
        d.ellipse([560, 580, 780, 740], fill=255),
        d.rectangle([420, 250, 470, 720], fill=255),
        d.rectangle([730, 190, 780, 660], fill=255),
        d.polygon([(420, 250), (780, 190), (780, 300), (420, 360)], fill=255),
    ))
    paint(img, note, 0xFFF4D6)
    save(img, "mmusik")


def mvideo():
    img = background(0x8FB8E8, 0x2E5C9E)
    screen = shape(lambda d: d.rounded_rectangle([180, 260, 844, 760], radius=70, fill=255))
    paint(img, screen, 0x1E2B3A, gloss=0.18)
    play = shape(lambda d: d.polygon([(430, 380), (430, 640), (650, 510)], fill=255))
    paint(img, play, 0xFFFFFF, shadow=False, gloss=0.0, rim=False)
    holes = shape(lambda d: [d.rounded_rectangle([220 + i * 120, 290, 280 + i * 120, 320], radius=10, fill=255) for i in range(6)]
                  + [d.rounded_rectangle([220 + i * 120, 700, 280 + i * 120, 730], radius=10, fill=255) for i in range(6)])
    paint(img, holes, 0x8FB8E8, shadow=False, gloss=0.0, rim=False, flat=True)
    save(img, "mvideo")


def mrekam():
    img = background(0xF29A86, 0xB23A2B)
    stand = shape(lambda d: (
        d.arc([300, 330, 724, 760], start=0, end=180, fill=255, width=46),
        d.rectangle([489, 750, 535, 860], fill=255),
        d.rounded_rectangle([370, 840, 654, 886], radius=23, fill=255),
    ))
    paint(img, stand, 0xE8E2DA, gloss=0.2)
    mic = shape(lambda d: d.rounded_rectangle([392, 150, 632, 650], radius=120, fill=255))
    paint(img, mic, 0x3A3F47)
    grille = shape(lambda d: [d.line([(420, y), (604, y)], fill=255, width=14) for y in range(230, 600, 56)])
    paint(img, grille, 0x6B7380, shadow=False, gloss=0.0, rim=False, flat=True)
    dot = shape(lambda d: d.ellipse([700, 140, 860, 300], fill=255))
    paint(img, dot, 0xFF4D4D)
    save(img, "mrekam")


def mular():
    img = background(0x9BD86A, 0x2F7A3A)
    body = new_mask()
    d = ImageDraw.Draw(body)
    pts = []
    for i in range(60):
        t = i / 59
        x = 230 + t * 560
        y = 600 + math.sin(t * math.pi * 2.2) * 150
        pts.append((x, y))
    d.line(pts, fill=255, width=150, joint="curve")
    for p in (pts[0], pts[-1]):
        d.ellipse([p[0] - 75, p[1] - 75, p[0] + 75, p[1] + 75], fill=255)
    hx, hy = pts[-1]
    d.ellipse([hx - 105, hy - 100, hx + 115, hy + 95], fill=255)
    paint(img, body, 0x2E9E5A)
    eye = shape(lambda d: d.ellipse([hx + 5, hy - 60, hx + 70, hy + 5], fill=255))
    paint(img, eye, 0xFFFFFF, shadow=False, gloss=0, rim=False, flat=True)
    pupil = shape(lambda d: d.ellipse([hx + 30, hy - 40, hx + 62, hy - 8], fill=255))
    paint(img, pupil, 0x111111, shadow=False, gloss=0, rim=False, flat=True)
    apple = shape(lambda d: d.ellipse([170, 160, 400, 380], fill=255))
    paint(img, apple, 0xE0352B)
    leaf = shape(lambda d: d.ellipse([290, 110, 390, 180], fill=255))
    paint(img, leaf, 0x5FB85A, shadow=False, gloss=0.2)
    save(img, "mular")


def mranjau():
    img = background(0xC9A27A, 0x6B4A2E)
    tiles = shape(lambda d: [d.rounded_rectangle([170 + c * 230, 170 + r * 230, 380 + c * 230, 380 + r * 230], radius=36, fill=255)
                             for r in range(3) for c in range(3) if (r, c) != (1, 1)])
    paint(img, tiles, 0x8FC46A, gloss=0.3)
    hole = shape(lambda d: d.rounded_rectangle([400, 400, 610, 610], radius=36, fill=255))
    paint(img, hole, 0xE8D3B0, shadow=False, gloss=0.0)
    mine = new_mask()
    md = ImageDraw.Draw(mine)
    md.ellipse([435, 435, 575, 575], fill=255)
    for i in range(8):
        a = i * math.pi / 4
        md.line([(505 + 40 * math.cos(a), 505 + 40 * math.sin(a)), (505 + 100 * math.cos(a), 505 + 100 * math.sin(a))], fill=255, width=26)
    paint(img, mine, 0x2A2A2A, gloss=0.35)
    flag = shape(lambda d: (d.rectangle([660, 190, 682, 360], fill=255), d.polygon([(682, 195), (790, 240), (682, 285)], fill=255)))
    paint(img, flag, 0xE0352B, shadow=False, gloss=0.2)
    save(img, "mranjau")


def mbata():
    img = background(0xF2B661, 0xB0581E)
    bricks = new_mask()
    d = ImageDraw.Draw(bricks)
    for r in range(4):
        off = 0 if r % 2 == 0 else -95
        for c in range(5):
            x0 = 150 + off + c * 190
            x0, x1 = max(150, x0), min(874, x0 + 175)
            if x1 - x0 > 40:
                d.rounded_rectangle([x0, 150 + r * 95, x1, 230 + r * 95], radius=16, fill=255)
    paint(img, bricks, 0xC8442B, gloss=0.3)
    ball = shape(lambda d: d.ellipse([560, 600, 660, 700], fill=255))
    paint(img, ball, 0xFFF4D6)
    paddle = shape(lambda d: d.rounded_rectangle([330, 790, 700, 850], radius=30, fill=255))
    paint(img, paddle, 0x3E7FD0)
    save(img, "mbata")


def mbalok():
    img = background(0x6BB5EE, 0x1F5A9E)
    cells = [(0, 3, 0xF3C969), (1, 3, 0xF3C969), (2, 3, 0x7CC4A4), (3, 3, 0x7CC4A4),
             (0, 2, 0xF3C969), (2, 2, 0x7CC4A4), (3, 2, 0xE8735F), (3, 1, 0xE8735F),
             (1, 0, 0xC792EA), (2, 0, 0xC792EA), (1, -1, 0xC792EA)]
    step, size = 150, 138
    x0, y0 = 512 - (4 * step - (step - size)) // 2, 512 + step - (5 * step - (step - size)) // 2
    for c, r, col in cells:
        m = shape(lambda d, c=c, r=r: d.rounded_rectangle([x0 + c * step, y0 + r * step, x0 + c * step + size, y0 + r * step + size], radius=24, fill=255))
        paint(img, m, col, gloss=0.4)
    save(img, "mbalok")


def mkartu():
    img = background(0x3F9A62, 0x175234)
    back = new_mask()
    ImageDraw.Draw(back).rounded_rectangle([180, 230, 560, 760], radius=40, fill=255)
    back = back.rotate(14, center=(370, 495), resample=Image.BICUBIC)
    paint(img, back, 0xC8352B, gloss=0.25)
    front = new_mask()
    ImageDraw.Draw(front).rounded_rectangle([430, 200, 830, 760], radius=40, fill=255)
    front = front.rotate(-8, center=(630, 480), resample=Image.BICUBIC)
    paint(img, front, 0xFBF7EE, gloss=0.15)
    heart = new_mask()
    hd = ImageDraw.Draw(heart)
    hd.ellipse([520, 360, 640, 480], fill=255)
    hd.ellipse([620, 360, 740, 480], fill=255)
    hd.polygon([(527, 450), (733, 450), (630, 600)], fill=255)
    heart = heart.rotate(-8, center=(630, 480), resample=Image.BICUBIC)
    paint(img, heart, 0xD63A30, shadow=False, gloss=0.3)
    save(img, "mkartu")


def mkonsol():
    img = background(0x2C5E46, 0x0D2419)
    win = shape(lambda d: d.rounded_rectangle([150, 220, 874, 800], radius=60, fill=255))
    paint(img, win, 0x14201A, gloss=0.12)
    bar = shape(lambda d: d.rounded_rectangle([150, 220, 874, 320], radius=60, fill=255))
    bar = ImageChops.multiply(bar, shape(lambda d: d.rectangle([0, 0, S, 300], fill=255)))
    paint(img, bar, 0x2F4A3C, shadow=False, gloss=0.0, rim=False)
    for i, col in enumerate((0xE8735F, 0xF3C969, 0x7CC4A4)):
        dot = shape(lambda d, i=i: d.ellipse([200 + i * 60, 248, 240 + i * 60, 288], fill=255))
        paint(img, dot, col, shadow=False, gloss=0, rim=False, flat=True)
    chevron = shape(lambda d: d.line([(240, 420), (360, 510), (240, 600)], fill=255, width=52, joint="curve"))
    paint(img, chevron, 0x7CC4A4, shadow=False, gloss=0, rim=False, flat=True)
    under = shape(lambda d: d.rounded_rectangle([410, 570, 600, 616], radius=12, fill=255))
    paint(img, under, 0xF0FFE0, shadow=False, gloss=0, rim=False, flat=True)
    lines = shape(lambda d: [d.rounded_rectangle([240, 680 + i * 50, 240 + w, 708 + i * 50], radius=12, fill=255) for i, w in enumerate((420, 260))])
    paint(img, lines, 0x4F7A63, shadow=False, gloss=0, rim=False, flat=True)
    save(img, "mkonsol")


def munduh():
    img = background(0x64C3C9, 0x1C6E86)
    tray = shape(lambda d: (
        d.rounded_rectangle([200, 640, 824, 820], radius=50, fill=255),
    ))
    paint(img, tray, 0xEAF6F4, gloss=0.3)
    slot = shape(lambda d: d.rounded_rectangle([280, 660, 744, 700], radius=20, fill=255))
    paint(img, slot, 0x9FBFC0, shadow=False, gloss=0, rim=False, flat=True)
    arrow = shape(lambda d: (d.rounded_rectangle([432, 150, 592, 470], radius=30, fill=255),
                             d.polygon([(300, 430), (724, 430), (512, 660)], fill=255)))
    paint(img, arrow, 0xF3C969)
    save(img, "munduh")


def mbaca():
    img = background(0xE6C891, 0x9C6B32)
    cover = shape(lambda d: d.rounded_rectangle([150, 300, 874, 790], radius=50, fill=255))
    paint(img, cover, 0x7A3E2A, gloss=0.2)
    left = shape(lambda d: d.polygon([(190, 280), (500, 330), (500, 760), (190, 720)], fill=255))
    paint(img, left, 0xFBF5E6, shadow=False, gloss=0.1)
    right = shape(lambda d: d.polygon([(524, 330), (834, 280), (834, 720), (524, 760)], fill=255))
    paint(img, right, 0xF3EBD6, shadow=False, gloss=0.1)
    text = shape(lambda d: [d.rounded_rectangle([240, 400 + i * 70, 450, 422 + i * 70], radius=10, fill=255) for i in range(4)]
                 + [d.rounded_rectangle([574, 400 + i * 70, 784, 422 + i * 70], radius=10, fill=255) for i in range(4)])
    paint(img, text, 0xC9B999, shadow=False, gloss=0, rim=False, flat=True)
    ribbon = shape(lambda d: d.polygon([(700, 290), (760, 290), (760, 520), (730, 480), (700, 520)], fill=255))
    paint(img, ribbon, 0x3F9A62, shadow=False, gloss=0.2)
    save(img, "mbaca")


def mlabirin():
    img = background(0x86C97A, 0x2B6B3A)
    floor = shape(lambda d: d.rounded_rectangle([170, 170, 854, 854], radius=50, fill=255))
    paint(img, floor, 0xD9C79A, gloss=0.15)
    walls = new_mask()
    d = ImageDraw.Draw(walls)
    w = 46
    segs = [((170, 170), (854, 170)), ((170, 854), (854, 854)), ((170, 170), (170, 680)), ((854, 340), (854, 854)),
            ((340, 170), (340, 510)), ((340, 680), (680, 680)), ((510, 340), (510, 680)), ((510, 340), (680, 340)),
            ((680, 510), (854, 510)), ((170, 510), (340, 510))]
    for a, b in segs:
        d.line([a, b], fill=255, width=w)
        for p in (a, b):
            d.ellipse([p[0] - w / 2, p[1] - w / 2, p[0] + w / 2, p[1] + w / 2], fill=255)
    paint(img, walls, 0x2F7A3A, gloss=0.3)
    flower = new_mask()
    fd = ImageDraw.Draw(flower)
    cx, cy = 595, 595
    for i in range(5):
        a = i * 2 * math.pi / 5 - math.pi / 2
        fd.ellipse([cx + 34 * math.cos(a) - 30, cy + 34 * math.sin(a) - 30, cx + 34 * math.cos(a) + 30, cy + 34 * math.sin(a) + 30], fill=255)
    paint(img, flower, 0xF3C969)
    centre = shape(lambda d: d.ellipse([cx - 20, cy - 20, cx + 20, cy + 20], fill=255))
    paint(img, centre, 0xC8642B, shadow=False, gloss=0, rim=False, flat=True)
    save(img, "mlabirin")


def mchip8():
    img = background(0xA8E6C6, 0x3B8C6A)
    pins = new_mask()
    d = ImageDraw.Draw(pins)
    for i in range(5):
        p = 300 + i * 106
        d.rounded_rectangle([p - 22, 150, p + 22, 260], radius=10, fill=255)
        d.rounded_rectangle([p - 22, 764, p + 22, 874], radius=10, fill=255)
        d.rounded_rectangle([150, p - 22, 260, p + 22], radius=10, fill=255)
        d.rounded_rectangle([764, p - 22, 874, p + 22], radius=10, fill=255)
    paint(img, pins, 0xD8DDE0, gloss=0.3)
    chip = shape(lambda d: d.rounded_rectangle([230, 230, 794, 794], radius=60, fill=255))
    paint(img, chip, 0x1F2A26, gloss=0.2)
    eight = ["0111110", "1100011", "1100011", "0111110", "1100011", "1100011", "0111110"]
    px = new_mask()
    pd = ImageDraw.Draw(px)
    cell = 52
    ox, oy = 512 - 3.5 * cell, 512 - 3.5 * cell
    for r, row in enumerate(eight):
        for c, ch in enumerate(row):
            if ch == "1":
                pd.rectangle([ox + c * cell + 4, oy + r * cell + 4, ox + (c + 1) * cell - 4, oy + (r + 1) * cell - 4], fill=255)
    paint(img, px, 0x9BE0B8, shadow=False, gloss=0, rim=False, flat=True)
    save(img, "mchip8")


def mlompat():
    img = background(0x9FD6F0, 0x3E8FC4)
    water = shape(lambda d: d.rectangle([0, 760, S, S], fill=255))
    paint(img, water, 0x2E78B0, shadow=False, gloss=0.0, rim=False)
    pad = shape(lambda d: d.pieslice([170, 640, 854, 880], start=200, end=520, fill=255))
    paint(img, pad, 0x3F9A3A, gloss=0.2)
    body = shape(lambda d: (d.ellipse([290, 380, 734, 760], fill=255), d.ellipse([300, 300, 470, 470], fill=255), d.ellipse([554, 300, 724, 470], fill=255)))
    paint(img, body, 0x7CCB4F)
    for cx in (385, 639):
        eye = shape(lambda d, cx=cx: d.ellipse([cx - 55, 330, cx + 55, 440], fill=255))
        paint(img, eye, 0xFFFFFF, shadow=False, gloss=0, rim=False, flat=True)
        pupil = shape(lambda d, cx=cx: d.ellipse([cx - 25, 360, cx + 25, 410], fill=255))
        paint(img, pupil, 0x111111, shadow=False, gloss=0, rim=False, flat=True)
    smile = shape(lambda d: d.arc([400, 480, 624, 640], start=20, end=160, fill=255, width=22))
    paint(img, smile, 0x2B5A20, shadow=False, gloss=0, rim=False, flat=True)
    drop = shape(lambda d: (d.ellipse([760, 170, 860, 270], fill=255), d.polygon([(765, 210), (855, 210), (810, 110)], fill=255)))
    paint(img, drop, 0xDDF3FF)
    save(img, "mlompat")


def masisten():
    img = background(0x7FD8C0, 0x1E6A64)
    bubble = shape(lambda d: (d.rounded_rectangle([150, 220, 830, 700], radius=150, fill=255),
                              d.polygon([(270, 640), (420, 690), (230, 840)], fill=255)))
    paint(img, bubble, 0xF4FFF9, gloss=0.25)
    leaf = new_mask()
    ld = ImageDraw.Draw(leaf)
    ld.ellipse([300, 360, 660, 560], fill=255)
    leaf = leaf.rotate(28, center=(480, 460), resample=Image.BICUBIC)
    paint(img, leaf, 0x3F9A62, shadow=False, gloss=0.3)
    vein = new_mask()
    ImageDraw.Draw(vein).line([(330, 545), (630, 375)], fill=255, width=16)
    paint(img, vein, 0xBFE8C8, shadow=False, gloss=0, rim=False, flat=True)
    sparks = shape(lambda d: (star4(d, 760, 230, 150), star4(d, 880, 420, 70), star4(d, 650, 110, 55)))
    paint(img, sparks, 0xF3C969, gloss=0.3)
    save(img, "masisten")


def doom():
    """An original picture (no logo or art of the game): a burning portal in a crosshair."""
    img = background(0x6E3A34, 0x1C0D0E)
    arch = shape(lambda d: (d.ellipse([222, 150, 802, 730], fill=255), d.rectangle([222, 440, 802, 880], fill=255)))
    paint(img, arch, 0x8C8580, gloss=0.2)
    glow = shape(lambda d: (d.ellipse([312, 240, 712, 640], fill=255), d.rectangle([312, 440, 712, 880], fill=255)))
    paint(img, glow, 0xF2762E, shadow=False, gloss=0.4)
    sight = shape(lambda d: (
        d.ellipse([332, 350, 692, 710], outline=255, width=50),
        d.rectangle([487, 290, 537, 440], fill=255),
        d.rectangle([487, 620, 537, 770], fill=255),
        d.rectangle([272, 505, 422, 555], fill=255),
        d.rectangle([602, 505, 752, 555], fill=255),
    ))
    paint(img, sight, 0xFFF1C9, shadow=True, gloss=0.2)
    save(img, "doom")


def mnes():
    """A generic game pad with a bee-yellow pair of buttons (no console's own artwork)."""
    img = background(0xF0C2A8, 0xA8503C)
    body = shape(lambda d: d.rounded_rectangle([130, 330, 894, 730], radius=150, fill=255))
    paint(img, body, 0xE9E4DA, gloss=0.3)
    cross = shape(lambda d: (d.rounded_rectangle([250, 480, 450, 560], radius=14, fill=255),
                             d.rounded_rectangle([310, 420, 390, 620], radius=14, fill=255)))
    paint(img, cross, 0x2A2A2E, shadow=False, gloss=0.15, rim=False)
    for cx, cy in ((640, 560), (770, 480)):
        btn = shape(lambda d, cx=cx, cy=cy: d.ellipse([cx - 55, cy - 55, cx + 55, cy + 55], fill=255))
        paint(img, btn, 0xF3C969, shadow=True, gloss=0.35)
    for x in (470, 560):
        pill = shape(lambda d, x=x: d.rounded_rectangle([x - 40, 620, x + 40, 650], radius=15, fill=255))
        paint(img, pill, 0x6A6A70, shadow=False, gloss=0, rim=False, flat=True)
    save(img, "mnes")


def mjaring():
    img = background(0x8FB8E8, 0x2D5C8E)
    for r, w in ((330, 70), (230, 70), (130, 70)):
        arc = shape(lambda d, r=r, w=w: d.arc([512 - r, 600 - r, 512 + r, 600 + r], start=225, end=315, fill=255, width=w))
        paint(img, arc, 0xEAF6F4, gloss=0.25)
    dot = shape(lambda d: d.ellipse([462, 580, 562, 680], fill=255))
    paint(img, dot, 0xF3C969)
    save(img, "mjaring")


ALL = [mjaring, mmusik, mvideo, mrekam, mular, mranjau, mbata, mbalok, mkartu, mkonsol, munduh, mbaca, mlabirin, mchip8, mlompat, masisten, doom, mnes]

if __name__ == "__main__":
    want = set(sys.argv[1:])
    for fn in ALL:
        if not want or fn.__name__ in want:
            fn()
