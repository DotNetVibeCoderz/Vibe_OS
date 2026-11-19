#!/usr/bin/env python3
"""A small CHIP-8 assembler for Buitenzorg's own ROMs (assets/chip8/*.asm).

Syntax, one statement per line, `;` starts a comment:
    label:                  a label (code or data)
    name = 12               a constant (decimal, 0x.. hex, 0b.. binary)
    cls / ret / jp ADDR / jp v0, ADDR / call ADDR
    se vX, NN|vY  sne vX, NN|vY  ld vX, NN|vY  add vX, NN|vY
    or/and/xor/sub/subn/shr/shl vX, vY
    ld i, ADDR   ld vX, dt   ld dt, vX   ld st, vX   ld vX, k
    ld f, vX   ld b, vX   ld [i], vX   ld vX, [i]   add i, vX
    rnd vX, NN   drw vX, vY, N   skp vX   sknp vX
    db 0b10000001, 0x3C, ...   (data bytes)
Usage: python scripts/chip8asm.py assets/chip8/X.asm assets/chip8/X.CH8
"""
import re
import sys

def num(tok, syms):
    tok = tok.strip()
    if tok in syms:
        return syms[tok]
    if tok.lower().startswith('0x'):
        return int(tok, 16)
    if tok.lower().startswith('0b'):
        return int(tok, 2)
    return int(tok)

def reg(tok):
    m = re.fullmatch(r'v([0-9a-fA-F])', tok.strip())
    return int(m.group(1), 16) if m else None

def assemble(src):
    lines = []
    for raw in src.splitlines():
        line = raw.split(';', 1)[0].strip()
        if line:
            lines.append(line)
    syms = {}
    # pass 1: addresses
    pc = 0x200
    for line in lines:
        while True:
            m = re.match(r'([A-Za-z_][\w]*):\s*(.*)', line)
            if not m:
                break
            syms[m.group(1)] = pc
            line = m.group(2)
        if not line:
            continue
        m = re.fullmatch(r'([A-Za-z_]\w*)\s*=\s*(.+)', line)
        if m:
            syms[m.group(1)] = num(m.group(2), syms)
            continue
        op = line.split(None, 1)[0].lower()
        if op == 'db':
            pc += len(line.split(None, 1)[1].split(','))
        else:
            pc += 2
    out = bytearray()
    for line in lines:
        while True:
            m = re.match(r'([A-Za-z_][\w]*):\s*(.*)', line)
            if not m:
                break
            line = m.group(2)
        if not line or re.fullmatch(r'[A-Za-z_]\w*\s*=\s*.+', line):
            continue
        parts = line.split(None, 1)
        op = parts[0].lower()
        args = [a.strip() for a in parts[1].split(',')] if len(parts) > 1 else []
        if op == 'db':
            out += bytes(num(a, syms) & 0xFF for a in args)
            continue
        w = encode(op, args, syms)
        out += bytes([w >> 8, w & 0xFF])
    return bytes(out)

def encode(op, a, syms):
    x = reg(a[0]) if a else None
    y = reg(a[1]) if len(a) > 1 else None
    if op == 'cls': return 0x00E0
    if op == 'ret': return 0x00EE
    if op == 'jp':
        if len(a) == 2: return 0xB000 | num(a[1], syms)
        return 0x1000 | num(a[0], syms)
    if op == 'call': return 0x2000 | num(a[0], syms)
    if op == 'se': return (0x5000 | x << 8 | y << 4) if y is not None else (0x3000 | x << 8 | num(a[1], syms) & 0xFF)
    if op == 'sne': return (0x9000 | x << 8 | y << 4) if y is not None else (0x4000 | x << 8 | num(a[1], syms) & 0xFF)
    if op == 'add':
        if a[0].lower() == 'i': return 0xF01E | reg(a[1]) << 8
        return (0x8004 | x << 8 | y << 4) if y is not None else (0x7000 | x << 8 | num(a[1], syms) & 0xFF)
    alu = {'or': 1, 'and': 2, 'xor': 3, 'sub': 5, 'shr': 6, 'subn': 7, 'shl': 0xE}
    if op in alu: return 0x8000 | x << 8 | (y or 0) << 4 | alu[op]
    if op == 'rnd': return 0xC000 | x << 8 | num(a[1], syms) & 0xFF
    if op == 'drw': return 0xD000 | x << 8 | y << 4 | num(a[2], syms) & 0xF
    if op == 'skp': return 0xE09E | x << 8
    if op == 'sknp': return 0xE0A1 | x << 8
    if op == 'ld':
        d, s = a[0].lower(), a[1].lower()
        if d == 'i': return 0xA000 | num(a[1], syms) & 0xFFF
        if d == 'dt': return 0xF015 | reg(s) << 8
        if d == 'st': return 0xF018 | reg(s) << 8
        if d == 'f': return 0xF029 | reg(s) << 8
        if d == 'b': return 0xF033 | reg(s) << 8
        if d == '[i]': return 0xF055 | reg(s) << 8
        if s == '[i]': return 0xF065 | x << 8
        if s == 'dt': return 0xF007 | x << 8
        if s == 'k': return 0xF00A | x << 8
        if y is not None: return 0x8000 | x << 8 | y << 4
        return 0x6000 | x << 8 | num(a[1], syms) & 0xFF
    raise SystemExit('unknown instruction: ' + op + ' ' + ','.join(a))

if __name__ == '__main__':
    data = assemble(open(sys.argv[1], encoding='utf-8').read())
    open(sys.argv[2], 'wb').write(data)
    print(sys.argv[2], len(data), 'bytes')
