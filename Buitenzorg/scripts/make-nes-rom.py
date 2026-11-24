#!/usr/bin/env python3
"""Builds assets/nes/KEBUN.NES: "Lebah Kebun", an original NES homebrew game
for Buitenzorg's NES emulator (MNes). A bee (d-pad) flies over a garden and
drinks from flowers; each flower reached adds a point and chirps on pulse
channel 1, and a new flower opens elsewhere. NROM-128 (mapper 0), 16 KiB
PRG + 8 KiB CHR, every tile drawn below.

The 6502 code is assembled by the small two-pass assembler in this file
(only the instructions and addressing modes the game uses).

    python scripts/make-nes-rom.py            # writes assets/nes/KEBUN.NES
"""
import os
import sys

# ---- a small 6502 assembler -------------------------------------------------

OPS = {
    # mnemonic: {mode: opcode}
    'ADC': {'imm': 0x69, 'zp': 0x65, 'abs': 0x6D, 'zpx': 0x75},
    'AND': {'imm': 0x29, 'zp': 0x25},
    'ASL': {'acc': 0x0A, 'zp': 0x06},
    'BCC': {'rel': 0x90}, 'BCS': {'rel': 0xB0}, 'BEQ': {'rel': 0xF0}, 'BNE': {'rel': 0xD0},
    'BMI': {'rel': 0x30}, 'BPL': {'rel': 0x10},
    'BIT': {'abs': 0x2C, 'zp': 0x24},
    'CLC': {'imp': 0x18}, 'CLD': {'imp': 0xD8}, 'SEC': {'imp': 0x38}, 'SEI': {'imp': 0x78},
    'CMP': {'imm': 0xC9, 'zp': 0xC5, 'abs': 0xCD},
    'CPX': {'imm': 0xE0}, 'CPY': {'imm': 0xC0},
    'DEC': {'zp': 0xC6}, 'DEX': {'imp': 0xCA}, 'DEY': {'imp': 0x88},
    'EOR': {'imm': 0x49},
    'INC': {'zp': 0xE6}, 'INX': {'imp': 0xE8}, 'INY': {'imp': 0xC8},
    'JMP': {'abs': 0x4C}, 'JSR': {'abs': 0x20}, 'RTS': {'imp': 0x60}, 'RTI': {'imp': 0x40},
    'LDA': {'imm': 0xA9, 'zp': 0xA5, 'abs': 0xAD, 'abx': 0xBD, 'zpx': 0xB5, 'izy': 0xB1},
    'LDX': {'imm': 0xA2, 'zp': 0xA6}, 'LDY': {'imm': 0xA0, 'zp': 0xA4},
    'LSR': {'acc': 0x4A},
    'ORA': {'imm': 0x09, 'zp': 0x05},
    'PHA': {'imp': 0x48}, 'PLA': {'imp': 0x68}, 'TAX': {'imp': 0xAA}, 'TAY': {'imp': 0xA8},
    'TXA': {'imp': 0x8A}, 'TYA': {'imp': 0x98}, 'TXS': {'imp': 0x9A},
    'ROL': {'zp': 0x26},
    'SBC': {'imm': 0xE9, 'zp': 0xE5},
    'STA': {'zp': 0x85, 'abs': 0x8D, 'abx': 0x9D, 'zpx': 0x95, 'izy': 0x91},
    'STX': {'zp': 0x86, 'abs': 0x8E}, 'STY': {'zp': 0x84, 'abs': 0x8C},
}
SIZE = {'imp': 1, 'acc': 1, 'imm': 2, 'zp': 2, 'zpx': 2, 'izy': 2, 'rel': 2, 'abs': 3, 'abx': 3}


def assemble(src, org):
    lines = []
    for raw in src.splitlines():
        line = raw.split(';')[0].strip()
        if line:
            lines.append(line)
    labels = {}
    consts = {}

    def value(tok, pc, final):
        tok = tok.strip()
        lo = hi = False
        if tok.startswith('<'):
            lo, tok = True, tok[1:]
        elif tok.startswith('>'):
            hi, tok = True, tok[1:]
        if '+' in tok and not tok.startswith('$'):
            a, b = tok.split('+', 1)
            v = value(a, pc, final) + value(b, pc, final)
        elif tok.startswith('$'):
            v = int(tok[1:], 16)
        elif tok.startswith('%'):
            v = int(tok[1:], 2)
        elif tok.isdigit():
            v = int(tok)
        elif tok in consts:
            v = consts[tok]
        elif tok in labels:
            v = labels[tok]
        elif final:
            raise SystemExit('unknown symbol ' + tok)
        else:
            v = 0xFFFF
        if lo:
            v &= 0xFF
        if hi:
            v = (v >> 8) & 0xFF
        return v

    def mode_of(mn, arg, pc, final):
        if arg == '':
            return ('acc' if 'acc' in OPS[mn] and 'imp' not in OPS[mn] else 'imp'), None
        if arg == 'A':
            return 'acc', None
        if arg.startswith('#'):
            return 'imm', value(arg[1:], pc, final)
        if 'rel' in OPS[mn]:
            return 'rel', value(arg, pc, final)
        if arg.startswith('(') and arg.endswith('),Y'):
            return 'izy', value(arg[1:-3], pc, final)
        idx = None
        if arg.endswith(',X'):
            idx, arg = 'x', arg[:-2]
        v = value(arg, pc, final)
        small = v < 0x100 and not arg.startswith('!')
        if idx == 'x':
            return ('zpx' if small and 'zpx' in OPS[mn] else 'abx'), v
        return ('zp' if small and 'zp' in OPS[mn] else 'abs'), v

    for final in (False, True):
        pc = org
        out = bytearray()
        for line in lines:
            if line.endswith(':'):
                labels[line[:-1]] = pc
                continue
            if '=' in line and not line.startswith('.'):
                k, v = line.split('=')
                consts[k.strip()] = value(v, pc, True)
                continue
            if line.startswith('.db'):
                for t in line[3:].split(','):
                    out.append(value(t, pc, final) & 0xFF)
                    pc += 1
                continue
            if line.startswith('.dw'):
                for t in line[3:].split(','):
                    v = value(t, pc, final)
                    out += bytes([v & 0xFF, v >> 8 & 0xFF])
                    pc += 2
                continue
            if line.startswith('.pad'):
                target = value(line[4:], pc, True)
                out += bytes(target - pc)
                pc = target
                continue
            parts = line.split(None, 1)
            mn = parts[0].upper()
            arg = parts[1].replace(' ', '') if len(parts) > 1 else ''
            if mn not in OPS:
                raise SystemExit('unknown instruction ' + line)
            m, v = mode_of(mn, arg, pc, final)
            if m not in OPS[mn]:
                raise SystemExit('mode %s not available for %s' % (m, line))
            out.append(OPS[mn][m])
            if m == 'rel':
                d = v - (pc + 2)
                if final and not -128 <= d <= 127:
                    raise SystemExit('branch out of range: ' + line)
                out.append(d & 0xFF)
            elif SIZE[m] == 2:
                out.append(v & 0xFF)
            elif SIZE[m] == 3:
                out += bytes([v & 0xFF, v >> 8 & 0xFF])
            pc += SIZE[m]
    return bytes(out), labels


# ---- the game -----------------------------------------------------------------

GAME = r'''
PPUCTRL = $2000
PPUMASK = $2001
PPUSTATUS = $2002
OAMADDR = $2003
PPUSCROLL = $2005
PPUADDR = $2006
PPUDATA = $2007
OAMDMA = $4014
JOY1 = $4016

frame = $00
buttons = $01
beex = $02
beey = $03
flx = $04
fly = $05
ones = $06
tens = $07
dirty = $08
seed = $09
row = $0A
col = $0B
tmp = $0C
catches = $0D
ptr = $0E

reset:
    SEI
    CLD
    LDX #$40
    STX $4017
    LDX #$FF
    TXS
    INX
    STX PPUCTRL
    STX PPUMASK
    STX $4010
vb1:
    BIT PPUSTATUS
    BPL vb1
    LDA #0
    TAX
clear:
    STA $0000,X
    STA $0300,X
    STA $0400,X
    STA $0500,X
    STA $0600,X
    STA $0700,X
    INX
    BNE clear
    LDA #$FF
hide:
    STA $0200,X
    INX
    BNE hide
vb2:
    BIT PPUSTATUS
    BPL vb2

    ; palettes
    LDA PPUSTATUS
    LDA #$3F
    STA PPUADDR
    LDA #$00
    STA PPUADDR
    LDX #0
pal:
    LDA palettes,X
    STA PPUDATA
    INX
    CPX #32
    BNE pal

    ; the garden: 30 rows of 32 tiles at $2000
    LDA #$20
    STA PPUADDR
    LDA #$00
    STA PPUADDR
    LDA #0
    STA row
rowloop:
    LDA #0
    STA col
colloop:
    LDA row
    CMP #4
    BCS ground
    LDA #0          ; sky: the backdrop shows through
    JMP put
ground:
    LDA row
    ASL A
    ADC row         ; row * 3
    ADC col
    AND #7
    BNE grass
    LDA #2          ; a flower in the grass
    JMP put
grass:
    LDA #1
put:
    STA PPUDATA
    INC col
    LDA col
    CMP #32
    BNE colloop
    INC row
    LDA row
    CMP #30
    BNE rowloop
    ; attribute table: palette 0 everywhere
    LDX #64
    LDA #0
attr:
    STA PPUDATA
    DEX
    BNE attr

    ; start
    LDA #100
    STA beex
    LDA #120
    STA beey
    LDA #140
    STA flx
    LDA #120
    STA fly
    LDA #$5A
    STA seed
    LDA #1
    STA dirty
    STA $4015       ; pulse 1 on
    LDA #$80
    STA PPUCTRL     ; NMI on
    LDA #$1E
    STA PPUMASK     ; background + sprites, left column too

main:
    LDA frame
wait:
    CMP frame
    BEQ wait
    JSR readpad
    JSR move
    JSR catch
    JSR sprites
    JMP main

readpad:
    LDA #1
    STA JOY1
    LDA #0
    STA JOY1
    LDX #8
rp:
    LDA JOY1
    LSR A
    ROL buttons     ; A ends in bit 7, Right in bit 0
    DEX
    BNE rp
    RTS

move:
    LDA buttons
    AND #%00000001
    BEQ noright
    LDA beex
    CMP #240
    BCS noright
    INC beex
noright:
    LDA buttons
    AND #%00000010
    BEQ noleft
    LDA beex
    CMP #9
    BCC noleft
    DEC beex
noleft:
    LDA buttons
    AND #%00000100
    BEQ nodown
    LDA beey
    CMP #216
    BCS nodown
    INC beey
nodown:
    LDA buttons
    AND #%00001000
    BEQ noup
    LDA beey
    CMP #41
    BCC noup
    DEC beey
noup:
    RTS

catch:
    LDA beex
    SEC
    SBC flx
    BCS cx
    EOR #$FF
    CLC
    ADC #1
cx:
    CMP #10
    BCS nocatch
    LDA beey
    SEC
    SBC fly
    BCS cy
    EOR #$FF
    CLC
    ADC #1
cy:
    CMP #10
    BCS nocatch
    ; a catch: score, chirp, a new flower
    INC catches
    INC ones
    LDA ones
    CMP #10
    BNE scored
    LDA #0
    STA ones
    INC tens
    LDA tens
    CMP #10
    BNE scored
    LDA #0
    STA tens
scored:
    LDA #1
    STA dirty
    LDA #%10011111  ; duty 50%, constant volume 15
    STA $4000
    LDA #0
    STA $4001
    LDA #$A9        ; ~660 Hz
    STA $4002
    LDA #%00000000  ; length index 0 = 10 half frames (~80 ms), timer high 0
    STA $4003
    JSR random
    AND #$BF
    CLC
    ADC #24
    STA flx
    JSR random
    AND #$7F
    CLC
    ADC #64
    STA fly
nocatch:
    RTS

random:
    LDA seed
    ASL A
    BCC nofb
    EOR #$1D
nofb:
    STA seed
    RTS

sprites:
    ; sprite 0: the bee, sprite 1: the flower to reach
    LDA beey
    SEC
    SBC #1
    STA $0200
    LDA #$20
    STA $0201
    LDA #0
    STA $0202
    LDA beex
    STA $0203
    LDA fly
    SEC
    SBC #1
    STA $0204
    LDA #$21
    STA $0205
    LDA #1
    STA $0206
    LDA flx
    STA $0207
    RTS

nmi:
    PHA
    TXA
    PHA
    LDA #0
    STA OAMADDR
    LDA #$02
    STA OAMDMA
    LDA dirty
    BEQ noscore
    LDA PPUSTATUS
    LDA #$20
    STA PPUADDR
    LDA #$42
    STA PPUADDR
    LDA tens
    CLC
    ADC #$10
    STA PPUDATA
    LDA ones
    CLC
    ADC #$10
    STA PPUDATA
    LDA #0
    STA dirty
noscore:
    LDA #0
    STA PPUSCROLL
    STA PPUSCROLL
    LDA #$80
    STA PPUCTRL
    INC frame
    PLA
    TAX
    PLA
irq:
    RTI

palettes:
    ; background: sky backdrop; grass green, petal pink, pollen yellow
    .db $21,$1A,$25,$28, $21,$1A,$25,$28, $21,$1A,$25,$28, $21,$1A,$25,$28
    ; sprites: bee yellow / black / white; flower red / yellow / green
    .db $21,$28,$0F,$30, $21,$16,$28,$1A, $21,$28,$0F,$30, $21,$28,$0F,$30

    .pad $FFFA
    .dw nmi, reset, irq
'''


def tile(rows):
    """8 strings of 8 chars '0'..'3' -> 16 bytes of CHR."""
    lo = bytearray()
    hi = bytearray()
    for r in rows:
        a = b = 0
        for ch in r:
            c = int(ch)
            a = a << 1 | (c & 1)
            b = b << 1 | (c >> 1)
        lo.append(a)
        hi.append(b)
    return bytes(lo + hi)


BLANK = ['00000000'] * 8
GRASS = ['11111111', '11121111', '11111111', '11111112', '11111111', '12111111', '11111111', '11111111']
FLOWER = ['11222111', '12232211', '12333211', '11232211', '11222111', '11111111', '11101111', '11111111']
DIGITS = {
    0: ['00333000', '03000300', '03000300', '03000300', '03000300', '03000300', '00333000', '00000000'],
    1: ['00030000', '00330000', '00030000', '00030000', '00030000', '00030000', '00333000', '00000000'],
    2: ['00333000', '03000300', '00000300', '00003000', '00030000', '00300000', '03333300', '00000000'],
    3: ['00333000', '03000300', '00000300', '00033000', '00000300', '03000300', '00333000', '00000000'],
    4: ['00003000', '00033000', '00303000', '03003000', '03333300', '00003000', '00003000', '00000000'],
    5: ['03333300', '03000000', '03333000', '00000300', '00000300', '03000300', '00333000', '00000000'],
    6: ['00333000', '03000000', '03000000', '03333000', '03000300', '03000300', '00333000', '00000000'],
    7: ['03333300', '00000300', '00003000', '00030000', '00030000', '00030000', '00030000', '00000000'],
    8: ['00333000', '03000300', '03000300', '00333000', '03000300', '03000300', '00333000', '00000000'],
    9: ['00333000', '03000300', '03000300', '00333300', '00000300', '00000300', '00333000', '00000000'],
}
BEE = ['00033000', '03303300', '33333330', '12121210', '21212120', '12121210', '01212100', '00000000']
BLOOM = ['00010000', '01121100', '11232110', '01121100', '00131000', '00030000', '00333000', '00030000']


def chr_rom():
    t = [BLANK] * 512
    t[1] = GRASS
    t[2] = FLOWER
    for d, rows in DIGITS.items():
        t[0x10 + d] = rows
    t[0x20] = BEE
    t[0x21] = BLOOM
    data = b''.join(tile(r) for r in t[:256]) + bytes(4096)
    assert len(data) == 8192
    return data


def build():
    prg, labels = assemble(GAME, 0xC000)
    assert len(prg) == 0x4000, len(prg)
    header = b'NES\x1a' + bytes([1, 1, 0x00, 0x00]) + bytes(8)
    return header + prg + chr_rom(), labels


if __name__ == '__main__':
    rom, labels = build()
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = os.path.join(root, 'assets', 'nes', 'KEBUN.NES')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, 'wb') as f:
        f.write(rom)
    print('%s: %d bytes, reset at $%04X, nmi at $%04X' % (out, len(rom), labels['reset'], labels['nmi']))
