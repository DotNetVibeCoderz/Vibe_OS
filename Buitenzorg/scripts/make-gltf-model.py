#!/usr/bin/env python3
"""Write assets/models/pohon.glb: a small tree as a glTF 2.0 binary file.

It exercises what Buitenzorg.Three's loader (Gltf.cs) reads: a node tree with
translation, a rotation quaternion and scale, two meshes (one with two
primitives), float positions, normals on one mesh only (the other is computed),
per-vertex COLOR_0, unsigned-short and unsigned-byte indices, materials with a
base color factor (one translucent, one double-sided). Run it again after
editing; bzimage copies the file to /disk/POHON.GLB.
"""
import json, math, os, struct

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, 'assets', 'models', 'pohon.glb')

buf = bytearray()
views, accessors = [], []


def add_view(data, target=None):
    while len(buf) % 4:
        buf.append(0)
    views.append({'buffer': 0, 'byteOffset': len(buf), 'byteLength': len(data), **({'target': target} if target else {})})
    buf.extend(data)
    return len(views) - 1


def add_floats(rows, typ):
    n = len(rows[0])
    data = b''.join(struct.pack('<%df' % n, *r) for r in rows)
    v = add_view(data, 34962)
    acc = {'bufferView': v, 'componentType': 5126, 'count': len(rows), 'type': typ}
    if typ == 'VEC3':
        acc['min'] = [min(r[i] for r in rows) for i in range(3)]
        acc['max'] = [max(r[i] for r in rows) for i in range(3)]
    accessors.append(acc)
    return len(accessors) - 1


def add_indices(idx, comp):
    fmt, ct = ('<%dH', 5123) if comp == 'u16' else ('<%dB', 5121)
    v = add_view(struct.pack(fmt % len(idx), *idx), 34963)
    accessors.append({'bufferView': v, 'componentType': ct, 'count': len(idx), 'type': 'SCALAR'})
    return len(accessors) - 1


# trunk: a box 0.3 x 1.0 x 0.3 standing on y = 0, with normals
pos, nrm, col, idx = [], [], [], []
w, h = 0.15, 1.0
faces = [((1, 0, 0), (0, 0, -1), (0, 1, 0)), ((-1, 0, 0), (0, 0, 1), (0, 1, 0)), ((0, 0, 1), (1, 0, 0), (0, 1, 0)),
         ((0, 0, -1), (-1, 0, 0), (0, 1, 0)), ((0, 1, 0), (1, 0, 0), (0, 0, -1)), ((0, -1, 0), (1, 0, 0), (0, 0, 1))]
for n, u, v in faces:
    s = len(pos)
    for cu, cv in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
        p = [n[i] * w + u[i] * w * cu + v[i] * (h / 2 if v[1] else w) * cv for i in range(3)]
        p[1] += h / 2
        pos.append(p)
        nrm.append(list(n))
        shade = 0.55 + 0.1 * cv
        col.append([0.45 * shade / 0.55, 0.30 * shade / 0.55, 0.16 * shade / 0.55])
    idx += [s, s + 1, s + 2, s, s + 2, s + 3]
trunk = {'attributes': {'POSITION': add_floats(pos, 'VEC3'), 'NORMAL': add_floats(nrm, 'VEC3'), 'COLOR_0': add_floats(col, 'VEC3')},
         'indices': add_indices(idx, 'u16'), 'material': 0}

# crown: two stacked 8-sided cones (no normals: the loader computes them)
def cone(y0, r, height, top_color, base_color):
    pos, col, idx = [[0, y0 + height, 0]], [top_color], []
    for k in range(8):
        a = 2 * math.pi * k / 8
        pos.append([r * math.cos(a), y0, -r * math.sin(a)])
        col.append(base_color)
    for k in range(8):
        idx += [0, 1 + k, 1 + (k + 1) % 8]
    c = len(pos)
    pos.append([0, y0, 0])
    col.append(base_color)
    for k in range(8):
        idx += [c, 1 + (k + 1) % 8, 1 + k]
    return pos, col, idx

lower = cone(0.0, 0.7, 0.9, [0.36, 0.72, 0.35], [0.16, 0.42, 0.20])
upper = cone(0.55, 0.5, 0.8, [0.56, 0.82, 0.45], [0.24, 0.55, 0.26])
crown_prims = []
for (p, c, i) in (lower, upper):
    crown_prims.append({'attributes': {'POSITION': add_floats(p, 'VEC3'), 'COLOR_0': add_floats(c, 'VEC3')},
                        'indices': add_indices(i, 'u8'), 'material': 1})

# a translucent pond under the tree (double-sided plane)
pond = {'attributes': {'POSITION': add_floats([[-1, 0, -1], [1, 0, -1], [1, 0, 1], [-1, 0, 1]], 'VEC3')},
        'indices': add_indices([0, 2, 1, 0, 3, 2], 'u8'), 'material': 2}

half = math.radians(20) / 2  # crown turned 20 degrees about Y
gltf = {
    'asset': {'version': '2.0', 'generator': 'Buitenzorg make-gltf-model.py'},
    'scene': 0,
    'scenes': [{'nodes': [0]}],
    'nodes': [
        {'name': 'pohon', 'children': [1, 2, 3], 'scale': [1.2, 1.2, 1.2]},
        {'name': 'batang', 'mesh': 0},
        {'name': 'tajuk', 'mesh': 1, 'translation': [0, 0.9, 0], 'rotation': [0, math.sin(half), 0, math.cos(half)]},
        {'name': 'kolam', 'mesh': 2, 'translation': [0, 0.01, 0], 'scale': [1.3, 1, 1.3]},
    ],
    'meshes': [{'name': 'batang', 'primitives': [trunk]}, {'name': 'tajuk', 'primitives': crown_prims}, {'name': 'kolam', 'primitives': [pond]}],
    'materials': [
        {'name': 'kayu', 'pbrMetallicRoughness': {'baseColorFactor': [1, 1, 1, 1]}},
        {'name': 'daun', 'pbrMetallicRoughness': {'baseColorFactor': [1, 1, 1, 1]}},
        {'name': 'air', 'pbrMetallicRoughness': {'baseColorFactor': [0.35, 0.62, 0.86, 0.6]}, 'alphaMode': 'BLEND', 'doubleSided': True},
    ],
    'accessors': accessors,
    'bufferViews': views,
    'buffers': [{'byteLength': len(buf)}],
}
js = json.dumps(gltf, separators=(',', ':')).encode()
js += b' ' * ((4 - len(js) % 4) % 4)
while len(buf) % 4:
    buf.append(0)
total = 12 + 8 + len(js) + 8 + len(buf)
out = struct.pack('<4sII', b'glTF', 2, total) + struct.pack('<I4s', len(js), b'JSON') + js + struct.pack('<I4s', len(buf), b'BIN\0') + bytes(buf)
os.makedirs(os.path.dirname(OUT), exist_ok=True)
open(OUT, 'wb').write(out)
print('wrote', OUT, len(out), 'bytes')
