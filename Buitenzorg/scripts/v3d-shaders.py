#!/usr/bin/env python3
"""V3D 4.2 (Raspberry Pi 4) QPU shaders for the kernel's 3D pipeline.

Writes kernel/bzkernel/src/platform/virt/v3d_shaders.rs. The encoder follows
Mesa's src/broadcom/qpu/qpu_pack.c (MIT) for the subset of instructions used
here; the latency rules follow Mesa's qpu_schedule.c (an ldvary value is
readable in the next instruction, r5 one instruction later; SFU results two
instructions later); the shader I/O follows Mesa's v3d compiler:

* coordinate shader: VPM in = position (4); VPM out = clip x, y, z, w, then
  screen x, y as integers in 1/256 px (floor(pos * viewport scale / w));
* vertex shader: VPM in = position (4), colour (4), uv (2); VPM out = screen
  x, y, Zs, 1/Wc, then the varyings (colour in B, G, R, A order, u, v);
* fragment shader: payload rf0 = W; varyings = ldvary * W + r5; the "last
  thread switch" pair before the TLB writes (vfpack into tlb with the default
  config = 16-bit float RGBA, per pixel); a texturing variant samples the TMU
  (32-bit float output) and multiplies the colour by the texel.

Run: python scripts/v3d-shaders.py [--check PY_VIDEOCORE6_SRC]
(--check compares encodings of instructions both assemblers know.)
"""
import os
import struct
import sys

# ---------------------------------------------------------------- encoding

# muxes
R0, R1, R2, R3, R4, R5, A, B = range(8)
# magic waddrs
W_NOP, W_TLB, W_TLBU = 6, 7, 8
W_TMUS, W_TMUT = 33, 34

# signals (v41_sig_map)
SIG = {
    (): 0, ('thrsw',): 1, ('ldunif',): 2, ('ldtmu',): 4, ('ldvary',): 8,
    ('ldunifrf',): 12, ('smimm',): 15, ('wrtmuc',): 18, ('thrsw', 'wrtmuc'): 19,
}
WRITES_ADDR = {'ldtmu', 'ldvary', 'ldunifrf'}


class Ins:
    """One ALU instruction: add + mul ops, two raddrs, one signal."""

    def __init__(self, comment=''):
        self.add = (187, 0, 0, W_NOP, True)   # opcode, mux a, mux b, waddr, magic
        self.mul = (15, 0, 4, W_NOP, True)
        self.raddr_a = 0
        self.raddr_b = 0
        self.sig = ()
        self.sig_addr = None          # (waddr, magic)
        self.add_force_ma = False
        self.comment = comment

    def pack(self):
        op_a, ma_a, mb_a, wa, wa_magic = self.add
        op_m, ma_m, mb_m, wm, wm_magic = self.mul
        cond = 0
        if any(s in WRITES_ADDR for s in self.sig):
            addr, magic = self.sig_addr
            cond = addr | ((1 << 6) if magic else 0)
        sig = SIG[tuple(sorted(self.sig, key=lambda s: ('thrsw', 'smimm', 'ldvary', 'ldtmu', 'ldunif', 'ldunifrf', 'wrtmuc').index(s)))]
        v = 0
        v |= (op_m & 0x3F) << 58
        v |= (sig & 0x1F) << 53
        v |= (cond & 0x7F) << 46
        v |= (1 << 45) if wm_magic else 0
        v |= (1 << 44) if (wa_magic or self.add_force_ma) else 0
        v |= (wm & 0x3F) << 38
        v |= (wa & 0x3F) << 32
        v |= (op_a & 0xFF) << 24
        v |= (mb_m & 7) << 21
        v |= (ma_m & 7) << 18
        v |= (mb_a & 7) << 15
        v |= (ma_a & 7) << 12
        v |= (self.raddr_a & 0x3F) << 6
        v |= (self.raddr_b & 0x3F)
        return v


class Src:
    """A source: an accumulator, a regfile register or a small immediate."""

    def __init__(self, kind, n):
        self.kind, self.n = kind, n


def r(n):
    return Src('acc', n)


def rf(n):
    return Src('rf', n)


def imm(n):
    assert 0 <= n <= 15
    return Src('imm', n)


class Dst:
    def __init__(self, kind, n):
        self.kind, self.n = kind, n


def dr(n):
    return Dst('acc', n)


def drf(n):
    return Dst('rf', n)


def dmagic(n):
    return Dst('magic', n)


class Prog:
    def __init__(self, name):
        self.name = name
        self.code = []

    def _ins(self, comment):
        i = Ins(comment)
        self.code.append(i)
        return i

    @staticmethod
    def _bind(ins, srcs):
        """Muxes for up to two sources, sharing raddr_a / raddr_b."""
        muxes = []
        used_a = used_b = None
        for s in srcs:
            if s.kind == 'acc':
                muxes.append(s.n)
            elif s.kind == 'imm':
                assert used_b in (None, ('imm', s.n)), 'raddr_b busy'
                used_b = ('imm', s.n)
                ins.raddr_b = s.n
                if 'smimm' not in ins.sig:
                    ins.sig = ins.sig + ('smimm',)
                muxes.append(B)
            else:
                if used_a in (None, ('rf', s.n)):
                    used_a = ('rf', s.n)
                    ins.raddr_a = s.n
                    muxes.append(A)
                else:
                    assert used_b in (None, ('rf', s.n)), 'both raddrs busy'
                    used_b = ('rf', s.n)
                    ins.raddr_b = s.n
                    muxes.append(B)
        return muxes

    @staticmethod
    def _dst(d):
        if d.kind == 'acc':
            return d.n, True
        if d.kind == 'magic':
            return d.n, True
        return d.n, False

    # --- add ALU
    def add_op(self, opcode, dst, srcs, comment='', mux_b_fixed=None, mux_a_fixed=None):
        i = self._ins(comment)
        m = self._bind(i, srcs)
        ma = m[0] if len(m) > 0 else (mux_a_fixed or 0)
        mb = m[1] if len(m) > 1 else (mux_b_fixed if mux_b_fixed is not None else 0)
        if mux_a_fixed is not None and len(m) == 0:
            ma = mux_a_fixed
        w, magic = self._dst(dst) if dst else (W_NOP, True)
        i.add = (opcode, ma, mb, w, magic)
        return i

    def fadd(self, dst, a, b, comment=''):
        i = self._ins(comment)
        m = self._bind(i, [a, b])
        ma, mb = m
        # FADD (not FADDNF): operands ordered so that a_unpack*8 + mux_a <= b's
        if ma > mb:
            ma, mb = mb, ma
        w, magic = self._dst(dst)
        i.add = (0 | (1 << 2) | 1, ma, mb, w, magic)
        return i

    def vfpack(self, dst, a, b, comment=''):
        i = self._ins(comment)
        m = self._bind(i, [a, b])
        w, magic = self._dst(dst)
        i.add = (53, m[0], m[1], w, magic)
        return i

    def recip(self, dst, a, comment=''):
        return self.add_op(186, dst, [a], comment, mux_b_fixed=5)

    def ffloor(self, dst, a, comment=''):
        return self.add_op(246, dst, [a], comment, mux_b_fixed=0)

    def ftoiz(self, dst, a, comment=''):
        return self.add_op(245, dst, [a], comment, mux_b_fixed=7)

    def ldvpmv_in(self, dst, idx, comment=''):
        return self.add_op(188, dst, [idx], comment, mux_b_fixed=0)

    def stvpmv(self, idx, val, comment=''):
        i = self._ins(comment)
        m = self._bind(i, [idx, val])
        i.add = (248, m[0], m[1], 0, False)      # waddr 0 = STVPMV, never magic
        return i

    def vpmwt(self, comment=''):
        i = self._ins(comment)
        i.add = (187, 6, 2, W_NOP, True)
        return i

    # --- mul ALU
    def fmul(self, dst, a, b, comment=''):
        i = self._ins(comment)
        m = self._bind(i, [a, b])
        w, magic = self._dst(dst)
        i.mul = (16 | (1 << 2) | 1, m[0], m[1], w, magic)
        return i

    def mov(self, dst, a, comment=''):
        i = self._ins(comment)
        m = self._bind(i, [a])
        w, magic = self._dst(dst)
        i.mul = (15, m[0], 7, w, magic)
        return i

    # --- signals on a nop
    def nop(self, comment='', sig=(), sig_addr=None):
        i = self._ins(comment)
        i.sig = tuple(sig)
        i.sig_addr = sig_addr
        return i

    def ldunifrf(self, n, comment=''):
        return self.nop(comment, ('ldunifrf',), (n, False))

    def ldvary(self, n, comment=''):
        return self.nop(comment, ('ldvary',), (n, False))

    def ldtmu(self, n, comment=''):
        return self.nop(comment, ('ldtmu',), (n, False))

    def wrtmuc(self, comment=''):
        return self.nop(comment, ('wrtmuc',))

    def thrsw(self, comment=''):
        return self.nop(comment, ('thrsw',))

    def end(self):
        self.thrsw('thread end')
        self.nop('delay slot')
        self.nop('delay slot')

    def words(self):
        return [i.pack() for i in self.code]


# ---------------------------------------------------------------- shaders

def coordinate_shader():
    p = Prog('COORD')
    for k in range(4):
        p.ldvpmv_in(drf(1 + k), imm(k), f'rf{1 + k} = position[{k}]')
    p.ldunifrf(5, 'rf5 = viewport x scale (half width * 256)')
    p.ldunifrf(6, 'rf6 = viewport y scale (-half height * 256)')
    p.recip(drf(7), rf(4), 'rf7 = 1 / Wc')
    p.fmul(dr(0), rf(1), rf(5), 'r0 = Xc * xscale')
    p.fmul(dr(1), rf(2), rf(6), 'r1 = Yc * yscale')
    p.fmul(dr(0), r(0), rf(7), 'r0 *= 1 / Wc')
    p.fmul(dr(1), r(1), rf(7), 'r1 *= 1 / Wc')
    p.ffloor(dr(0), r(0))
    p.ffloor(dr(1), r(1))
    p.ftoiz(dr(0), r(0), 'screen x in 1/256 px')
    p.ftoiz(dr(1), r(1), 'screen y in 1/256 px')
    for k in range(4):
        p.stvpmv(imm(k), rf(1 + k), f'out[{k}] = clip position[{k}]')
    p.stvpmv(imm(4), r(0), 'out[4] = screen x')
    p.stvpmv(imm(5), r(1), 'out[5] = screen y')
    p.vpmwt('GFXH-1684: VPM writes done before the end')
    p.end()
    return p


def vertex_shader():
    p = Prog('VERTEX')
    for k in range(10):
        p.ldvpmv_in(drf(k), imm(k), f'rf{k} = input[{k}] (x y z w r g b a u v)')
    p.ldunifrf(10, 'x scale')
    p.ldunifrf(11, 'y scale')
    p.ldunifrf(12, 'z scale')
    p.ldunifrf(13, 'z offset')
    p.recip(drf(14), rf(3), 'rf14 = 1 / Wc')
    p.fmul(dr(0), rf(0), rf(10))
    p.fmul(dr(1), rf(1), rf(11))
    p.fmul(dr(2), rf(2), rf(12))
    p.fmul(dr(0), r(0), rf(14))
    p.fmul(dr(1), r(1), rf(14))
    p.fmul(dr(2), r(2), rf(14))
    p.ffloor(dr(0), r(0))
    p.ffloor(dr(1), r(1))
    p.fadd(dr(2), r(2), rf(13), 'Zs = Zc * zscale / Wc + zoffset')
    p.ftoiz(dr(0), r(0))
    p.ftoiz(dr(1), r(1))
    p.stvpmv(imm(0), r(0), 'out[0] = screen x')
    p.stvpmv(imm(1), r(1), 'out[1] = screen y')
    p.stvpmv(imm(2), r(2), 'out[2] = Zs')
    p.stvpmv(imm(3), rf(14), 'out[3] = 1 / Wc')
    # varyings: colour as B, G, R, A (the TLB's first channel lands in byte 0,
    # so pixels come out as 0xAARRGGBB), then u, v
    for k, src in enumerate([6, 5, 4, 7, 8, 9]):
        p.stvpmv(imm(4 + k), rf(src), f'varying {k}')
    p.vpmwt('GFXH-1684')
    p.end()
    return p


def interpolate(p, n):
    """Six varyings into rf3..rf8: ldvary * W + C (C arrives in r5)."""
    for k in range(n):
        d = 3 + k
        p.ldvary(d, f'varying {k}')
        p.nop()
        p.fmul(dr(0), rf(d), rf(0), 'r0 = vary * W')
        p.fadd(drf(d), r(0), r(5), f'rf{d} = r0 + C')


def tlb_end(p):
    p.thrsw('last thread switch (pair)')
    p.thrsw('last thread switch (pair)')
    p.nop('delay slot')
    p.nop('delay slot')
    p.vfpack(dmagic(W_TLB), rf(3), rf(4), 'tlb <- f16 (first channel, second)')
    p.vfpack(dmagic(W_TLB), rf(5), rf(6), 'tlb <- f16 (third, alpha)')
    p.end()


def fragment_shader():
    p = Prog('FRAGMENT')
    interpolate(p, 6)
    tlb_end(p)
    return p


def fragment_shader_textured():
    p = Prog('FRAGMENT_TEX')
    interpolate(p, 6)
    p.mov(dmagic(W_TMUT), rf(8), 'tmut <- v')
    p.wrtmuc('TMU config P0 (uniform: texture state | 4 words)')
    p.wrtmuc('TMU config P1 (uniform: sampler state | 32-bit output)')
    p.mov(dmagic(W_TMUS), rf(7), 'tmus <- u (starts the lookup)')
    # Mesa (ntq_flush_tmu) always switches threads between starting a lookup
    # and reading its result
    p.thrsw('wait for the TMU (thread switch)')
    p.nop('delay slot')
    p.nop('delay slot')
    for k in range(4):
        p.ldtmu(9 + k, f'texel channel {k}')
    p.nop()
    for k in range(4):
        p.fmul(drf(3 + k), rf(3 + k), rf(9 + k), 'colour *= texel')
    tlb_end(p)
    return p


PROGRAMS = [coordinate_shader, vertex_shader, fragment_shader, fragment_shader_textured]


# ---------------------------------------------------------------- check

def check_against_pyvc6(path):
    """Encode instructions py-videocore6 (MIT assembler, also V3D 4.2) knows and compare."""
    sys.path.insert(0, path)
    from _videocore6 import assembler as a  # noqa

    def theirs(src):
        @a.qpu
        def k(asm):
            exec(src, globals())
        return a.assemble(k)

    cases = [
        ('recip rf7, rf4', lambda p: p.recip(drf(7), rf(4)), None),
        ('ldvpmv_in rf1, 0', lambda p: p.ldvpmv_in(drf(1), imm(0)), None),
        ('nop', lambda p: p.nop(), 'nop()'),
        ('thrsw', lambda p: p.thrsw(), 'nop(sig=thrsw)'),
        ('fmul r0, rf1, rf5', lambda p: p.fmul(dr(0), rf(1), rf(5)), 'nop().fmul(r0, rf[1], rf[5])'),
        ('fmul rf3, rf3, rf9', lambda p: p.fmul(drf(3), rf(3), rf(9)), 'nop().fmul(rf[3], rf[3], rf[9])'),
        ('fadd rf3, r0, r5', lambda p: p.fadd(drf(3), r(0), r(5)), 'fadd(rf[3], r0, r5)'),
        ('fadd r2, r2, rf13', lambda p: p.fadd(dr(2), r(2), rf(13)), 'fadd(r2, r2, rf[13])'),
        ('ldunifrf rf5', lambda p: p.ldunifrf(5), 'nop(sig=ldunifrf(rf[5]))'),
        ('ldtmu rf9', lambda p: p.ldtmu(9), 'nop(sig=ldtmu(rf[9]))'),
        ('ldvary rf3', lambda p: p.ldvary(3), 'nop(sig=ldvary(rf[3]))'),
        ('wrtmuc', lambda p: p.wrtmuc(), 'nop(sig=wrtmuc)'),
        ('vfpack tlb, rf3, rf4', lambda p: p.vfpack(dmagic(W_TLB), rf(3), rf(4)), 'vfpack(tlb, rf[3], rf[4])'),
        ('mov tmus, rf7', lambda p: p.mov(dmagic(W_TMUS), rf(7)), 'nop().mov(tmus, rf[7])'),
        ('mov tmut, rf8', lambda p: p.mov(dmagic(W_TMUT), rf(8)), 'nop().mov(tmut, rf[8])'),
        ('ffloor r0, r0', lambda p: p.ffloor(dr(0), r(0)), 'ffloor(r0, r0)'),
        ('ftoiz r0, r0', lambda p: p.ftoiz(dr(0), r(0)), 'ftoiz(r0, r0)'),
        ('vpmwt', lambda p: p.vpmwt(), 'vpmwt(null)'),
        ('stvpmv 4, r0', lambda p: p.stvpmv(imm(4), r(0)), 'stvpmv(rf[0], 4, r0)'),
        ('stvpmv 1, rf2', lambda p: p.stvpmv(imm(1), rf(2)), 'stvpmv(rf[0], 1, rf[2])'),
    ]
    bad = 0
    for name, mine_f, ref_src in cases:
        p = Prog('t')
        mine_f(p)
        mine = p.words()[0]
        if ref_src is None:
            print(f'---- {name:24} {mine:016x} (no reference)')
            continue
        try:
            ref = theirs(ref_src)[0]
        except Exception as e:  # noqa
            print(f'????  {name:24} {mine:016x} reference failed: {e}')
            continue
        ok = mine == ref
        bad += not ok
        print(f'{"ok  " if ok else "DIFF"} {name:24} {mine:016x} {"" if ok else "ref %016x" % ref}')
    return bad


def main():
    if '--check' in sys.argv:
        path = sys.argv[sys.argv.index('--check') + 1]
        sys.exit(1 if check_against_pyvc6(path) else 0)
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = os.path.join(root, 'kernel', 'bzkernel', 'src', 'platform', 'virt', 'v3d_shaders.rs')
    lines = [
        '//! V3D 4.2 QPU shaders (Raspberry Pi 4), generated by scripts/v3d-shaders.py',
        '//! -- edit the script, not this file. See the script for the conventions.',
        '',
    ]
    for f in PROGRAMS:
        p = f()
        lines.append(f'/// {len(p.code)} instructions.')
        lines.append(f'pub const {p.name}: [u64; {len(p.code)}] = [')
        for i in p.code:
            c = f' // {i.comment}' if i.comment else ''
            lines.append(f'    0x{i.pack():016x},{c}')
        lines.append('];')
        lines.append('')
    open(out, 'w', newline='\n').write('\n'.join(lines))
    print('wrote', out)


if __name__ == '__main__':
    main()
