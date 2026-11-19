// glTF 2.0 models for Buitenzorg.Three: binary .glb files and .gltf JSON
// files (buffers embedded as data: URIs or in a .bin file next to them).
//
// Read: the default scene's node tree (translation / rotation quaternion /
// scale or a matrix), meshes with any number of triangle primitives
// (POSITION, NORMAL, TEXCOORD_0, COLOR_0, indices; float, unsigned byte /
// short / int components, byteStride), and materials' base color factor
// (alpha < 1 = transparent), double-sidedness and name. Not read: textures,
// skins, morph targets, animations, cameras, lights, sparse accessors,
// Draco / meshopt compression, primitive modes other than triangles.
//
// Written for the interpreter: the JSON is parsed from a char[] (indexing a
// long string is slow on RustCLR), floats are decoded from their bits by
// hand (BitConverter's float helpers are not in every BCL).

using System;
using System.Collections.Generic;
using Buitenzorg.Graphics3D;

namespace Buitenzorg.Three;

public static class Gltf
{
    /// <summary>Why the last load failed ("" after a success).</summary>
    public static string LastError { get; private set; } = "";

    /// <summary>Load a .glb / .gltf file into a node tree (null + <see cref="LastError"/> on failure).</summary>
    public static Node? LoadFile(string path)
    {
        var data = Buitenzorg.App.Files.ReadAllBytes(path);
        if (data == null) { LastError = "cannot read " + path; return null; }
        int slash = path.LastIndexOf('/');
        return Load(data, slash >= 0 ? path.Substring(0, slash + 1) : "");
    }

    /// <summary>Load a model from its bytes; `baseDir` (ending in '/') finds external .bin buffers.</summary>
    public static Node? Load(byte[] data, string baseDir = "")
    {
        try
        {
            LastError = "";
            byte[]? bin = null;
            char[] json;
            if (data.Length >= 12 && data[0] == (byte)'g' && data[1] == (byte)'l' && data[2] == (byte)'T' && data[3] == (byte)'F')
            {
                // GLB: header, then chunks (length, type, data); JSON first, BIN optional
                int off = 12;
                json = new char[0];
                while (off + 8 <= data.Length)
                {
                    int len = U32(data, off), type = U32(data, off + 4);
                    int start = off + 8;
                    if (start + len > data.Length) throw new FormatException("truncated GLB chunk");
                    if (type == 0x4E4F534A) json = Utf8Chars(data, start, len); // "JSON"
                    else if (type == 0x004E4942) // "BIN\0"
                    {
                        bin = new byte[len];
                        Array.Copy(data, start, bin, 0, len);
                    }
                    off = start + ((len + 3) & ~3);
                }
            }
            else json = Utf8Chars(data, 0, data.Length);
            var root = new Json(json).Value() as Dictionary<string, object?> ?? throw new FormatException("not a glTF JSON object");
            return new Reader(root, bin, baseDir).Build();
        }
        catch (Exception e)
        {
            LastError = e.Message;
            return null;
        }
    }

    static int U32(byte[] b, int o) => b[o] | b[o + 1] << 8 | b[o + 2] << 16 | b[o + 3] << 24;

    static char[] Utf8Chars(byte[] b, int start, int len)
    {
        // ASCII is the common case for glTF JSON; other characters (names) as '?'
        var c = new char[len];
        for (int i = 0; i < len; i++) { byte x = b[start + i]; c[i] = x < 0x80 ? (char)x : '?'; }
        return c;
    }

    // ---- a small JSON reader ------------------------------------------------

    sealed class Json
    {
        readonly char[] _s;
        int _i;
        public Json(char[] s) { _s = s; }

        void Ws() { while (_i < _s.Length && (_s[_i] == ' ' || _s[_i] == '\n' || _s[_i] == '\r' || _s[_i] == '\t')) _i++; }

        public object? Value()
        {
            Ws();
            if (_i >= _s.Length) throw new FormatException("unexpected end of JSON");
            char c = _s[_i];
            if (c == '{')
            {
                _i++;
                var o = new Dictionary<string, object?>();
                Ws();
                if (_i < _s.Length && _s[_i] == '}') { _i++; return o; }
                while (true)
                {
                    Ws();
                    string k = Str();
                    Ws();
                    if (_i >= _s.Length || _s[_i] != ':') throw new FormatException("':' expected at " + _i);
                    _i++;
                    o[k] = Value();
                    Ws();
                    if (_i < _s.Length && _s[_i] == ',') { _i++; continue; }
                    if (_i < _s.Length && _s[_i] == '}') { _i++; return o; }
                    throw new FormatException("',' or '}' expected at " + _i);
                }
            }
            if (c == '[')
            {
                _i++;
                var a = new List<object?>();
                Ws();
                if (_i < _s.Length && _s[_i] == ']') { _i++; return a; }
                while (true)
                {
                    a.Add(Value());
                    Ws();
                    if (_i < _s.Length && _s[_i] == ',') { _i++; continue; }
                    if (_i < _s.Length && _s[_i] == ']') { _i++; return a; }
                    throw new FormatException("',' or ']' expected at " + _i);
                }
            }
            if (c == '"') return Str();
            if (c == 't') { _i += 4; return true; }
            if (c == 'f') { _i += 5; return false; }
            if (c == 'n') { _i += 4; return null; }
            return Num();
        }

        string Str()
        {
            if (_s[_i] != '"') throw new FormatException("string expected at " + _i);
            _i++;
            var sb = new System.Text.StringBuilder();
            while (_i < _s.Length && _s[_i] != '"')
            {
                char c = _s[_i++];
                if (c == '\\' && _i < _s.Length)
                {
                    char e = _s[_i++];
                    if (e == 'u') { _i += 4; sb.Append('?'); }
                    else sb.Append(e switch { 'n' => '\n', 't' => '\t', 'r' => '\r', 'b' => '\b', 'f' => '\f', _ => e });
                }
                else sb.Append(c);
            }
            _i++;
            return sb.ToString();
        }

        double Num()
        {
            double v = 0, frac = 0.1;
            bool neg = false, dot = false;
            if (_s[_i] == '-') { neg = true; _i++; }
            int start = _i;
            while (_i < _s.Length)
            {
                char c = _s[_i];
                if (c >= '0' && c <= '9') { if (dot) { v += (c - '0') * frac; frac *= 0.1; } else v = v * 10 + (c - '0'); _i++; }
                else if (c == '.') { dot = true; _i++; }
                else if (c == 'e' || c == 'E')
                {
                    _i++;
                    int es = 1, e = 0;
                    if (_i < _s.Length && (_s[_i] == '-' || _s[_i] == '+')) { es = _s[_i] == '-' ? -1 : 1; _i++; }
                    while (_i < _s.Length && _s[_i] >= '0' && _s[_i] <= '9') { e = e * 10 + (_s[_i] - '0'); _i++; }
                    v *= Math.Pow(10, es * e);
                    break;
                }
                else break;
            }
            if (_i == start) throw new FormatException("bad JSON value at " + _i);
            return neg ? -v : v;
        }
    }

    // ---- glTF -----------------------------------------------------------------

    sealed class Reader
    {
        readonly Dictionary<string, object?> _root;
        readonly byte[]? _glbBin;
        readonly string _baseDir;
        readonly List<byte[]?> _buffers = new List<byte[]?>();
        readonly Material?[] _materials;
        static double[]? _pow2;

        public Reader(Dictionary<string, object?> root, byte[]? glbBin, string baseDir)
        {
            _root = root; _glbBin = glbBin; _baseDir = baseDir;
            var mats = Arr(root, "materials");
            _materials = new Material?[mats.Count];
        }

        static List<object?> Arr(Dictionary<string, object?> o, string k) => o.TryGetValue(k, out var v) && v is List<object?> l ? l : new List<object?>();
        static Dictionary<string, object?>? Obj(object? v) => v as Dictionary<string, object?>;
        static int Int(Dictionary<string, object?> o, string k, int def = -1) => o.TryGetValue(k, out var v) && v is double d ? (int)d : def;
        static string Str(Dictionary<string, object?> o, string k) => o.TryGetValue(k, out var v) && v is string s ? s : "";
        static float[]? Floats(Dictionary<string, object?> o, string k)
        {
            if (!o.TryGetValue(k, out var v) || v is not List<object?> l) return null;
            var f = new float[l.Count];
            for (int i = 0; i < l.Count; i++) f[i] = l[i] is double d ? (float)d : 0;
            return f;
        }

        public Node Build()
        {
            var top = new Node("gltf");
            var scenes = Arr(_root, "scenes");
            var nodes = Arr(_root, "nodes");
            int si = Int(_root, "scene", 0);
            List<object?> roots;
            if (si >= 0 && si < scenes.Count && Obj(scenes[si]) is { } scene) roots = Arr(scene, "nodes");
            else
            {
                // no scene: every node that is nobody's child
                var child = new bool[nodes.Count];
                foreach (var n in nodes) if (Obj(n) is { } no) foreach (var c in Arr(no, "children")) if (c is double d && d >= 0 && d < child.Length) child[(int)d] = true;
                roots = new List<object?>();
                for (int i = 0; i < nodes.Count; i++) if (!child[i]) roots.Add((double)i);
            }
            foreach (var r in roots) if (r is double d) top.Add(MakeNode((int)d, nodes, 0));
            return top;
        }

        Node MakeNode(int index, List<object?> nodes, int depth)
        {
            if (index < 0 || index >= nodes.Count || depth > 64 || Obj(nodes[index]) is not { } o) throw new FormatException("bad node " + index);
            var n = new Node(Str(o, "name"));
            var m = Floats(o, "matrix");
            if (m != null && m.Length == 16) Decompose(m, n);
            else
            {
                var t = Floats(o, "translation");
                if (t != null && t.Length == 3) n.Position = new Vector3(t[0], t[1], t[2]);
                var r = Floats(o, "rotation");
                if (r != null && r.Length == 4) n.EulerAngles = EulerYXZ(r[0], r[1], r[2], r[3]);
                var s = Floats(o, "scale");
                if (s != null && s.Length == 3) n.Scale = new Vector3(s[0], s[1], s[2]);
            }
            int mesh = Int(o, "mesh");
            if (mesh >= 0) AddMesh(n, mesh);
            foreach (var c in Arr(o, "children")) if (c is double d) n.Add(MakeNode((int)d, nodes, depth + 1));
            return n;
        }

        /// <summary>A unit quaternion as Node's Euler angles (R = Ry * Rx * Rz).</summary>
        static Vector3 EulerYXZ(float x, float y, float z, float w)
        {
            float m13 = 2 * (x * z + w * y), m23 = 2 * (y * z - w * x), m33 = 1 - 2 * (x * x + y * y);
            float m21 = 2 * (x * y + w * z), m22 = 1 - 2 * (x * x + z * z), m11 = 1 - 2 * (y * y + z * z), m31 = 2 * (x * z - w * y);
            return EulerFrom(m11, m13, m21, m22, m23, m31, m33);
        }

        static Vector3 EulerFrom(float m11, float m13, float m21, float m22, float m23, float m31, float m33)
        {
            float ex = MathF.Asin(Math.Clamp(-m23, -1f, 1f));
            if (MathF.Abs(m23) < 0.99999f) return new Vector3(ex, MathF.Atan2(m13, m33), MathF.Atan2(m21, m22));
            return new Vector3(ex, MathF.Atan2(-m31, m11), 0);
        }

        static void Decompose(float[] m, Node n)
        {
            // column-major: column c = m[4c .. 4c+3]
            n.Position = new Vector3(m[12], m[13], m[14]);
            float sx = MathF.Sqrt(m[0] * m[0] + m[1] * m[1] + m[2] * m[2]);
            float sy = MathF.Sqrt(m[4] * m[4] + m[5] * m[5] + m[6] * m[6]);
            float sz = MathF.Sqrt(m[8] * m[8] + m[9] * m[9] + m[10] * m[10]);
            n.Scale = new Vector3(sx, sy, sz);
            if (sx == 0 || sy == 0 || sz == 0) return;
            // rotation entries r(row, col) = m[col * 4 + row] / scale(col)
            n.EulerAngles = EulerFrom(m[0] / sx, m[8] / sz, m[1] / sx, m[5] / sy, m[9] / sz, m[2] / sx, m[10] / sz);
        }

        void AddMesh(Node n, int mi)
        {
            var meshes = Arr(_root, "meshes");
            if (mi >= meshes.Count || Obj(meshes[mi]) is not { } mesh) throw new FormatException("bad mesh " + mi);
            var prims = Arr(mesh, "primitives");
            for (int p = 0; p < prims.Count; p++)
            {
                if (Obj(prims[p]) is not { } prim) continue;
                if (Int(prim, "mode", 4) != 4) continue; // triangles only
                var (g, mat) = Primitive(prim);
                if (g == null) continue;
                if (prims.Count == 1 && n.Geometry == null) { n.Geometry = g; n.Material = mat; }
                else n.Add(new Node(n.Name + "#" + p) { Geometry = g, Material = mat });
            }
        }

        (Geometry?, Material) Primitive(Dictionary<string, object?> prim)
        {
            var attrs = Obj(prim.TryGetValue("attributes", out var a) ? a : null) ?? throw new FormatException("primitive without attributes");
            int posAcc = Int(attrs, "POSITION");
            if (posAcc < 0) return (null, Material.Lambert(0xFFFFFF));
            var pos = ReadFloats(posAcc, out int pc, out int vcount);
            if (pc < 3) throw new FormatException("POSITION is not a VEC3");
            float[]? nrm = null, uv = null, col = null;
            int nc = 0, uc = 0, cc = 0;
            if (Int(attrs, "NORMAL") is int na and >= 0) nrm = ReadFloats(na, out nc, out _);
            if (Int(attrs, "TEXCOORD_0") is int ta and >= 0) uv = ReadFloats(ta, out uc, out _);
            if (Int(attrs, "COLOR_0") is int ca and >= 0) col = ReadFloats(ca, out cc, out _);
            var v = new float[vcount * 8];
            for (int i = 0; i < vcount; i++)
            {
                int o = i * 8;
                v[o] = pos[i * pc]; v[o + 1] = pos[i * pc + 1]; v[o + 2] = pos[i * pc + 2];
                if (nrm != null && nc >= 3 && i * nc + 2 < nrm.Length) { v[o + 3] = nrm[i * nc]; v[o + 4] = nrm[i * nc + 1]; v[o + 5] = nrm[i * nc + 2]; }
                if (uv != null && uc >= 2 && i * uc + 1 < uv.Length) { v[o + 6] = uv[i * uc]; v[o + 7] = uv[i * uc + 1]; }
            }
            int[] idx;
            int ia = Int(prim, "indices");
            if (ia >= 0)
            {
                var f = ReadFloats(ia, out _, out int ic);
                idx = new int[ic - ic % 3];
                for (int i = 0; i < idx.Length; i++)
                {
                    int k = (int)f[i];
                    if (k < 0 || k >= vcount) throw new FormatException("index out of range");
                    idx[i] = k;
                }
            }
            else
            {
                idx = new int[vcount - vcount % 3];
                for (int i = 0; i < idx.Length; i++) idx[i] = i;
            }
            int[]? colors = null;
            if (col != null && cc >= 3)
            {
                colors = new int[vcount];
                for (int i = 0; i < vcount && i * cc + 2 < col.Length; i++)
                    colors[i] = unchecked((int)0xFF000000) | Byte(col[i * cc]) << 16 | Byte(col[i * cc + 1]) << 8 | Byte(col[i * cc + 2]);
            }
            var g = new Geometry(v, idx, colors);
            if (nrm == null) g.ComputeNormals();
            return (g, MaterialOf(Int(prim, "material")));
        }

        static int Byte(float f) => (int)(Math.Clamp(f, 0f, 1f) * 255 + 0.5f);

        Material MaterialOf(int mi)
        {
            if (mi < 0 || mi >= _materials.Length) return Material.Lambert(0xFFFFFF);
            if (_materials[mi] != null) return _materials[mi]!;
            var mats = Arr(_root, "materials");
            var mo = Obj(mats[mi]) ?? new Dictionary<string, object?>();
            int color = 0xFFFFFF;
            float alpha = 1;
            if (Obj(mo.TryGetValue("pbrMetallicRoughness", out var pbr) ? pbr : null) is { } p && Floats(p, "baseColorFactor") is { Length: 4 } f)
            {
                color = Byte(f[0]) << 16 | Byte(f[1]) << 8 | Byte(f[2]);
                alpha = f[3];
            }
            var m = Material.Lambert(color);
            if (alpha < 0.999f || Str(mo, "alphaMode") == "BLEND") { m.Opacity = alpha; m.Transparent = true; }
            if (mo.TryGetValue("doubleSided", out var ds) && ds is bool b && b) m.DoubleSided = true;
            _materials[mi] = m;
            return m;
        }

        byte[] Buffer(int bi)
        {
            while (_buffers.Count <= bi) _buffers.Add(null);
            if (_buffers[bi] != null) return _buffers[bi]!;
            var bufs = Arr(_root, "buffers");
            if (bi >= bufs.Count || Obj(bufs[bi]) is not { } bo) throw new FormatException("bad buffer " + bi);
            string uri = Str(bo, "uri");
            byte[]? data;
            if (uri.Length == 0) data = _glbBin ?? throw new FormatException("GLB without a BIN chunk");
            else if (uri.StartsWith("data:"))
            {
                int comma = uri.IndexOf(',');
                if (comma < 0 || !uri.Substring(0, comma).EndsWith(";base64")) throw new FormatException("unsupported data URI");
                data = Base64(uri, comma + 1);
            }
            else data = Buitenzorg.App.Files.ReadAllBytes(_baseDir + uri) ?? throw new FormatException("cannot read buffer " + uri);
            _buffers[bi] = data;
            return data;
        }

        static byte[] Base64(string s, int from)
        {
            var chars = s.ToCharArray();
            var o = new List<byte>(chars.Length * 3 / 4);
            int acc = 0, bits = 0;
            for (int i = from; i < chars.Length; i++)
            {
                char c = chars[i];
                int v = c >= 'A' && c <= 'Z' ? c - 'A' : c >= 'a' && c <= 'z' ? c - 'a' + 26 : c >= '0' && c <= '9' ? c - '0' + 52 : c == '+' ? 62 : c == '/' ? 63 : -1;
                if (v < 0) continue;
                acc = acc << 6 | v; bits += 6;
                if (bits >= 8) { bits -= 8; o.Add((byte)(acc >> bits)); acc &= (1 << bits) - 1; }
            }
            return o.ToArray();
        }

        /// <summary>An accessor's values as floats (indices too), `comps` per element.</summary>
        float[] ReadFloats(int ai, out int comps, out int count)
        {
            var accs = Arr(_root, "accessors");
            if (ai >= accs.Count || Obj(accs[ai]) is not { } acc) throw new FormatException("bad accessor " + ai);
            count = Int(acc, "count", 0);
            comps = Str(acc, "type") switch { "SCALAR" => 1, "VEC2" => 2, "VEC3" => 3, "VEC4" => 4, "MAT4" => 16, _ => throw new FormatException("accessor type") };
            int ct = Int(acc, "componentType");
            bool norm = acc.TryGetValue("normalized", out var nv) && nv is bool nb && nb;
            int csize = ct switch { 5120 or 5121 => 1, 5122 or 5123 => 2, 5125 or 5126 => 4, _ => throw new FormatException("component type " + ct) };
            var outv = new float[count * comps];
            int bvi = Int(acc, "bufferView");
            if (bvi < 0) return outv; // all zeros
            var views = Arr(_root, "bufferViews");
            if (bvi >= views.Count || Obj(views[bvi]) is not { } bv) throw new FormatException("bad bufferView " + bvi);
            var buf = Buffer(Int(bv, "buffer", 0));
            int start = Int(bv, "byteOffset", 0) + Int(acc, "byteOffset", 0);
            int stride = Int(bv, "byteStride", 0);
            if (stride <= 0) stride = csize * comps;
            if (count > 0 && start + (count - 1) * stride + csize * comps > buf.Length) throw new FormatException("accessor " + ai + " runs past its buffer");
            for (int i = 0; i < count; i++)
            {
                int p = start + i * stride;
                for (int k = 0; k < comps; k++, p += csize)
                {
                    float f = ct switch
                    {
                        5126 => F32(buf[p] | buf[p + 1] << 8 | buf[p + 2] << 16 | buf[p + 3] << 24),
                        5125 => (float)(uint)(buf[p] | buf[p + 1] << 8 | buf[p + 2] << 16 | buf[p + 3] << 24),
                        5123 => norm ? (buf[p] | buf[p + 1] << 8) / 65535f : buf[p] | buf[p + 1] << 8,
                        5122 => norm ? Math.Max(-1f, (short)(buf[p] | buf[p + 1] << 8) / 32767f) : (short)(buf[p] | buf[p + 1] << 8),
                        5121 => norm ? buf[p] / 255f : buf[p],
                        _ => norm ? Math.Max(-1f, (sbyte)buf[p] / 127f) : (sbyte)buf[p],
                    };
                    outv[i * comps + k] = f;
                }
            }
            return outv;
        }

        /// <summary>An IEEE 754 single from its bits.</summary>
        static float F32(int bits)
        {
            if (_pow2 == null)
            {
                _pow2 = new double[300];
                double v = Math.Pow(2, -150);
                for (int i = 0; i < 300; i++) { _pow2[i] = v; v *= 2; }
            }
            int e = (bits >> 23) & 0xFF, mant = bits & 0x7FFFFF;
            if (e == 0xFF) return 0; // inf / NaN: not meaningful in a mesh
            double m = e == 0 ? mant : (mant | 0x800000);
            // value = m * 2^(e - 150), or m * 2^(-149) for denormals
            double r = m * _pow2[(e == 0 ? 1 : e) - 150 + 150];
            return (float)(bits < 0 ? -r : r);
        }
    }
}
