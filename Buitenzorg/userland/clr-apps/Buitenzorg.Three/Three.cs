// Buitenzorg.Three -- a scene graph in the style of ThreeNet / three.js on top
// of Buitenzorg.Graphics3D (/disk/BZTHREE.DLL): Scene, Node (position, Euler
// rotation, scale, children, LookAt), Geometry (box, plane, sphere, cylinder,
// cone, torus, OBJ), Material (Basic / Lambert / Phong, color map, opacity,
// wireframe), Light (directional, point, ambient), Camera (perspective,
// orthographic) and a Renderer that draws a scene through the kernel's
// rasterizer.
//
// Names follow ThreeNet (C:\experiment\VibeCoding\Vibe_Graphics\ThreeNet) so
// code reads the same: scene.AddMesh(geometry, material), scene.AddLight(...),
// scene.AddCamera(Camera.Perspective(...), position), node.LookAt(target),
// renderer.Render(scene). Angles are radians, colors 0xRRGGBB, the world is
// right-handed with +Y up, and cameras and lights face -Z.

using System;
using System.Collections.Generic;
using Buitenzorg.Graphics3D;

namespace Buitenzorg.Three;

public enum Shading { Basic, Lambert, Phong }

/// <summary>An image for Material.Map (0xAARRGGBB pixels; alpha 0 = cut out).</summary>
public sealed class Texture : IDisposable
{
    public int Width { get; }
    public int Height { get; }
    public int[] Pixels { get; }
    Texture3D? _gpu;

    public Texture(int width, int height, int[] pixels)
    {
        Width = width; Height = height; Pixels = pixels;
    }

    /// <summary>A checkerboard of `cells` x `cells` squares.</summary>
    public static Texture Checker(int size, int colorA, int colorB, int cells = 8)
    {
        var p = new int[size * size];
        int cell = Math.Max(1, size / cells);
        for (int y = 0; y < size; y++)
            for (int x = 0; x < size; x++)
                p[y * size + x] = unchecked((int)0xFF000000) | ((((x / cell) + (y / cell)) & 1) == 0 ? colorA : colorB);
        return new Texture(size, size, p);
    }

    /// <summary>Fine grid lines over a fill (for floors).</summary>
    public static Texture Grid(int size, int fill, int line, int cells = 8)
    {
        var p = new int[size * size];
        int cell = Math.Max(2, size / cells);
        for (int y = 0; y < size; y++)
            for (int x = 0; x < size; x++)
                p[y * size + x] = unchecked((int)0xFF000000) | (x % cell == 0 || y % cell == 0 ? line : fill);
        return new Texture(size, size, p);
    }

    /// <summary>Decode an uncompressed 24- or 32-bit BMP (null otherwise).</summary>
    public static Texture? FromBmp(byte[] b)
    {
        if (b.Length < 54 || b[0] != (byte)'B' || b[1] != (byte)'M') return null;
        int I32(int o) => b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24);
        int offset = I32(10), w = I32(18), h = I32(22), bpp = b[28] | (b[29] << 8);
        if (w <= 0 || w > 1024 || h == 0 || (bpp != 24 && bpp != 32)) return null;
        bool topDown = h < 0;
        if (topDown) h = -h;
        if (h > 1024) return null;
        int bytes = bpp / 8, stride = (w * bytes + 3) & ~3;
        if (offset + stride * (h - 1) + w * bytes > b.Length) return null;
        var p = new int[w * h];
        for (int y = 0; y < h; y++)
        {
            int row = offset + (topDown ? y : h - 1 - y) * stride;
            for (int x = 0; x < w; x++)
            {
                int s = row + x * bytes;
                p[y * w + x] = unchecked((int)0xFF000000) | (b[s + 2] << 16) | (b[s + 1] << 8) | b[s];
            }
        }
        return new Texture(w, h, p);
    }

    /// <summary>The kernel-side copy, made on first use.</summary>
    public Texture3D Gpu() => _gpu ??= new Texture3D(Width, Height, Pixels);

    public void Dispose() { _gpu?.Dispose(); _gpu = null; }
}

/// <summary>Vertices (8 floats each: position, normal, uv) and triangles.</summary>
public sealed class Geometry : IDisposable
{
    public float[] Vertices { get; private set; }
    public int[] Indices { get; private set; }
    /// <summary>Per-vertex colors (0xRRGGBB), or null for white.</summary>
    public int[]? Colors { get; private set; }
    public int VertexCount => Vertices.Length / 8;
    public int TriangleCount => Indices.Length / 3;
    /// <summary>Radius of a sphere around the origin that holds every vertex.</summary>
    public float BoundingRadius { get; private set; }
    Mesh3D? _gpu;

    public Geometry(float[] vertices, int[] indices, int[]? colors = null)
    {
        Vertices = vertices; Indices = indices; Colors = colors;
        Measure();
    }

    void Measure()
    {
        float r2 = 0;
        for (int i = 0; i + 2 < Vertices.Length; i += 8)
        {
            float d = Vertices[i] * Vertices[i] + Vertices[i + 1] * Vertices[i + 1] + Vertices[i + 2] * Vertices[i + 2];
            if (d > r2) r2 = d;
        }
        BoundingRadius = MathF.Sqrt(r2);
    }

    /// <summary>Replace the data (the kernel copy is remade on the next draw).</summary>
    public void Update(float[] vertices, int[] indices, int[]? colors = null)
    {
        Vertices = vertices; Indices = indices; Colors = colors;
        Measure();
        _gpu?.Dispose(); _gpu = null;
    }

    /// <summary>Give every vertex one color (multiplied with the material color).</summary>
    public Geometry WithColor(int color)
    {
        var c = new int[VertexCount];
        for (int i = 0; i < c.Length; i++) c[i] = unchecked((int)0xFF000000) | color;
        Update(Vertices, Indices, c);
        return this;
    }

    public Mesh3D Gpu()
    {
        if (_gpu == null)
        {
            int[]? argb = null;
            if (Colors != null)
            {
                argb = new int[Colors.Length];
                for (int i = 0; i < argb.Length; i++) argb[i] = unchecked((int)0xFF000000) | Colors[i];
            }
            _gpu = new Mesh3D(Vertices, Indices, argb);
        }
        return _gpu;
    }

    public void Dispose() { _gpu?.Dispose(); _gpu = null; }

    /// <summary>Smooth normals: each vertex gets the average of its triangles' normals.</summary>
    public void ComputeNormals()
    {
        int n = VertexCount;
        var acc = new float[n * 3];
        for (int t = 0; t + 2 < Indices.Length; t += 3)
        {
            int a = Indices[t], b = Indices[t + 1], c = Indices[t + 2];
            var pa = P(a); var pb = P(b); var pc = P(c);
            var fn = Vector3.Cross(pb - pa, pc - pa);
            foreach (int k in new[] { a, b, c }) { acc[k * 3] += fn.X; acc[k * 3 + 1] += fn.Y; acc[k * 3 + 2] += fn.Z; }
        }
        for (int i = 0; i < n; i++)
        {
            var v = Vector3.Normalize(new Vector3(acc[i * 3], acc[i * 3 + 1], acc[i * 3 + 2]));
            Vertices[i * 8 + 3] = v.X; Vertices[i * 8 + 4] = v.Y; Vertices[i * 8 + 5] = v.Z;
        }
        _gpu?.Dispose(); _gpu = null;
    }

    Vector3 P(int i) => new Vector3(Vertices[i * 8], Vertices[i * 8 + 1], Vertices[i * 8 + 2]);

    // ---- builders ----------------------------------------------------------

    sealed class Builder
    {
        readonly List<float> _v = new List<float>();
        readonly List<int> _i = new List<int>();
        public int Count => _v.Count / 8;
        public int Add(float x, float y, float z, float nx, float ny, float nz, float u, float v)
        {
            _v.Add(x); _v.Add(y); _v.Add(z); _v.Add(nx); _v.Add(ny); _v.Add(nz); _v.Add(u); _v.Add(v);
            return Count - 1;
        }
        public void Tri(int a, int b, int c) { _i.Add(a); _i.Add(b); _i.Add(c); }
        public Geometry Build() => new Geometry(_v.ToArray(), _i.ToArray());
    }

    /// <summary>A box centred on the origin.</summary>
    public static Geometry Box(float width = 1, float height = 1, float depth = 1)
    {
        float x = width / 2, y = height / 2, z = depth / 2;
        var b = new Builder();
        void Face(Vector3 n, Vector3 u, Vector3 v, float hu, float hv, float d)
        {
            // corners: centre n*d -/+ u*hu -/+ v*hv, counter-clockwise seen from outside
            var c = n * d;
            int s = b.Count;
            var p0 = c - u * hu - v * hv; var p1 = c + u * hu - v * hv; var p2 = c + u * hu + v * hv; var p3 = c - u * hu + v * hv;
            b.Add(p0.X, p0.Y, p0.Z, n.X, n.Y, n.Z, 0, 1);
            b.Add(p1.X, p1.Y, p1.Z, n.X, n.Y, n.Z, 1, 1);
            b.Add(p2.X, p2.Y, p2.Z, n.X, n.Y, n.Z, 1, 0);
            b.Add(p3.X, p3.Y, p3.Z, n.X, n.Y, n.Z, 0, 0);
            b.Tri(s, s + 1, s + 2); b.Tri(s, s + 2, s + 3);
        }
        Face(Vector3.UnitZ, Vector3.UnitX, Vector3.UnitY, x, y, z);
        Face(-Vector3.UnitZ, -Vector3.UnitX, Vector3.UnitY, x, y, z);
        Face(Vector3.UnitX, -Vector3.UnitZ, Vector3.UnitY, z, y, x);
        Face(-Vector3.UnitX, Vector3.UnitZ, Vector3.UnitY, z, y, x);
        Face(Vector3.UnitY, Vector3.UnitX, -Vector3.UnitZ, x, z, y);
        Face(-Vector3.UnitY, Vector3.UnitX, Vector3.UnitZ, x, z, y);
        return b.Build();
    }

    /// <summary>A flat rectangle in the XY plane facing +Z (rotate it -90 degrees about X
    /// for a floor); the texture repeats `repeat` times across it.</summary>
    public static Geometry Plane(float width = 1, float height = 1, int segments = 1, float repeat = 1)
    {
        var b = new Builder();
        int n = Math.Max(1, segments);
        for (int j = 0; j <= n; j++)
            for (int i = 0; i <= n; i++)
            {
                float u = (float)i / n, v = (float)j / n;
                b.Add((u - 0.5f) * width, (0.5f - v) * height, 0, 0, 0, 1, u * repeat, v * repeat);
            }
        for (int j = 0; j < n; j++)
            for (int i = 0; i < n; i++)
            {
                int a = j * (n + 1) + i, c = a + n + 1;
                b.Tri(c, c + 1, a + 1); b.Tri(c, a + 1, a);
            }
        return b.Build();
    }

    /// <summary>A UV sphere.</summary>
    public static Geometry Sphere(float radius = 0.5f, int widthSegments = 24, int heightSegments = 16)
    {
        int ws = Math.Max(3, widthSegments), hs = Math.Max(2, heightSegments);
        var b = new Builder();
        for (int y = 0; y <= hs; y++)
        {
            float v = (float)y / hs;
            double th = v * Math.PI;
            for (int x = 0; x <= ws; x++)
            {
                float u = (float)x / ws;
                double ph = u * 2 * Math.PI;
                float nx = (float)(-Math.Cos(ph) * Math.Sin(th)), ny = (float)Math.Cos(th), nz = (float)(Math.Sin(ph) * Math.Sin(th));
                b.Add(nx * radius, ny * radius, nz * radius, nx, ny, nz, u, v);
            }
        }
        for (int y = 0; y < hs; y++)
            for (int x = 0; x < ws; x++)
            {
                int a = y * (ws + 1) + x + 1, bb = y * (ws + 1) + x, c = (y + 1) * (ws + 1) + x, d = (y + 1) * (ws + 1) + x + 1;
                if (y != 0) b.Tri(a, bb, d);
                if (y != hs - 1) b.Tri(bb, c, d);
            }
        return b.Build();
    }

    /// <summary>A cylinder along Y (radiusTop 0 makes a cone).</summary>
    public static Geometry Cylinder(float radiusTop = 0.5f, float radiusBottom = 0.5f, float height = 1, int segments = 24, bool capped = true)
    {
        int n = Math.Max(3, segments);
        var b = new Builder();
        float h2 = height / 2, slope = (radiusBottom - radiusTop) / height;
        for (int i = 0; i <= n; i++)
        {
            double a = (double)i / n * 2 * Math.PI;
            float s = (float)Math.Sin(a), c = (float)Math.Cos(a);
            var nrm = Vector3.Normalize(new Vector3(s, slope, c));
            b.Add(radiusTop * s, h2, radiusTop * c, nrm.X, nrm.Y, nrm.Z, (float)i / n, 0);
            b.Add(radiusBottom * s, -h2, radiusBottom * c, nrm.X, nrm.Y, nrm.Z, (float)i / n, 1);
        }
        for (int i = 0; i < n; i++)
        {
            int top = i * 2, bot = top + 1, top2 = top + 2, bot2 = top + 3;
            b.Tri(top, bot, top2); b.Tri(bot, bot2, top2);
        }
        if (capped)
        {
            void Cap(float y, float r, float ny)
            {
                if (r <= 0) return;
                int centre = b.Add(0, y, 0, 0, ny, 0, 0.5f, 0.5f);
                int first = b.Count;
                for (int i = 0; i <= n; i++)
                {
                    double a = (double)i / n * 2 * Math.PI;
                    float s = (float)Math.Sin(a), c = (float)Math.Cos(a);
                    b.Add(r * s, y, r * c, 0, ny, 0, 0.5f + 0.5f * s, 0.5f - 0.5f * c);
                }
                for (int i = 0; i < n; i++)
                    if (ny > 0) b.Tri(centre, first + i, first + i + 1);
                    else b.Tri(centre, first + i + 1, first + i);
            }
            Cap(h2, radiusTop, 1);
            Cap(-h2, radiusBottom, -1);
        }
        return b.Build();
    }

    public static Geometry Cone(float radius = 0.5f, float height = 1, int segments = 24) => Cylinder(0, radius, height, segments);

    /// <summary>A torus around the Z axis (rotate it to lay it flat).</summary>
    public static Geometry Torus(float radius = 0.5f, float tube = 0.2f, int radialSegments = 12, int tubularSegments = 32)
    {
        int rs = Math.Max(3, radialSegments), ts = Math.Max(3, tubularSegments);
        var b = new Builder();
        for (int j = 0; j <= rs; j++)
            for (int i = 0; i <= ts; i++)
            {
                double u = (double)i / ts * 2 * Math.PI, v = (double)j / rs * 2 * Math.PI;
                float cx = (float)(radius * Math.Cos(u)), cy = (float)(radius * Math.Sin(u));
                float x = (float)((radius + tube * Math.Cos(v)) * Math.Cos(u));
                float y = (float)((radius + tube * Math.Cos(v)) * Math.Sin(u));
                float z = (float)(tube * Math.Sin(v));
                var nrm = Vector3.Normalize(new Vector3(x - cx, y - cy, z));
                b.Add(x, y, z, nrm.X, nrm.Y, nrm.Z, (float)i / ts, (float)j / rs);
            }
        for (int j = 1; j <= rs; j++)
            for (int i = 1; i <= ts; i++)
            {
                int a = (ts + 1) * j + i - 1, bb = (ts + 1) * (j - 1) + i - 1, c = (ts + 1) * (j - 1) + i, d = (ts + 1) * j + i;
                b.Tri(a, bb, d); b.Tri(bb, c, d);
            }
        return b.Build();
    }

    /// <summary>A Wavefront OBJ model (v / vt / vn / f, polygons as fans, negative
    /// indices). Without normals in the file they are computed.</summary>
    public static Geometry FromObj(string text)
    {
        var pos = new List<Vector3>();
        var uv = new List<float>();
        var nrm = new List<Vector3>();
        var b = new Builder();
        var seen = new Dictionary<long, int>();
        bool hasNormals = false;
        var face = new List<int>();
        int i = 0, len = text.Length;
        var tok = new List<string>();
        while (i < len)
        {
            // one line into tokens
            tok.Clear();
            while (i < len && text[i] != '\n')
            {
                while (i < len && (text[i] == ' ' || text[i] == '\t' || text[i] == '\r')) i++;
                int s = i;
                while (i < len && text[i] != ' ' && text[i] != '\t' && text[i] != '\r' && text[i] != '\n') i++;
                if (i > s) tok.Add(text.Substring(s, i - s));
            }
            i++;
            if (tok.Count == 0) continue;
            string k = tok[0];
            if (k == "v" && tok.Count >= 4) pos.Add(new Vector3(Num(tok[1]), Num(tok[2]), Num(tok[3])));
            else if (k == "vt" && tok.Count >= 3) { uv.Add(Num(tok[1])); uv.Add(1 - Num(tok[2])); }
            else if (k == "vn" && tok.Count >= 4) nrm.Add(new Vector3(Num(tok[1]), Num(tok[2]), Num(tok[3])));
            else if (k == "f" && tok.Count >= 4)
            {
                face.Clear();
                for (int t = 1; t < tok.Count; t++)
                {
                    string f = tok[t];
                    int s1 = f.IndexOf('/');
                    int vi = Idx(s1 < 0 ? f : f.Substring(0, s1), pos.Count), ti = -1, ni = -1;
                    if (s1 >= 0)
                    {
                        int s2 = f.IndexOf('/', s1 + 1);
                        string tPart = s2 < 0 ? f.Substring(s1 + 1) : f.Substring(s1 + 1, s2 - s1 - 1);
                        if (tPart.Length > 0) ti = Idx(tPart, uv.Count / 2);
                        if (s2 >= 0 && s2 + 1 < f.Length) ni = Idx(f.Substring(s2 + 1), nrm.Count);
                    }
                    if (vi < 0) { face.Clear(); break; }
                    long key = ((long)vi << 40) | ((long)(ti + 1) << 20) | (long)(ni + 1);
                    if (!seen.TryGetValue(key, out int id))
                    {
                        var p = pos[vi];
                        var n = ni >= 0 ? nrm[ni] : Vector3.Zero;
                        if (ni >= 0) hasNormals = true;
                        id = b.Add(p.X, p.Y, p.Z, n.X, n.Y, n.Z, ti >= 0 ? uv[ti * 2] : 0, ti >= 0 ? uv[ti * 2 + 1] : 0);
                        seen[key] = id;
                    }
                    face.Add(id);
                }
                for (int t = 1; t + 1 < face.Count; t++) b.Tri(face[0], face[t], face[t + 1]);
            }
        }
        var g = b.Build();
        if (!hasNormals) g.ComputeNormals();
        return g;
    }

    static int Idx(string s, int count)
    {
        int n = 0, sign = 1, k = 0;
        if (s.Length > 0 && s[0] == '-') { sign = -1; k = 1; }
        for (; k < s.Length; k++) { char c = s[k]; if (c < '0' || c > '9') return -1; n = n * 10 + (c - '0'); }
        if (n == 0) return -1;
        int r = sign > 0 ? n - 1 : count - n;
        return r >= 0 && r < count ? r : -1;
    }

    /// <summary>A decimal number ("-1.5", "2e-3") without culture rules.</summary>
    internal static float Num(string s)
    {
        double v = 0, frac = 0.1; int k = 0; bool neg = false, dot = false;
        if (k < s.Length && (s[k] == '-' || s[k] == '+')) { neg = s[k] == '-'; k++; }
        for (; k < s.Length; k++)
        {
            char c = s[k];
            if (c >= '0' && c <= '9') { if (dot) { v += (c - '0') * frac; frac *= 0.1; } else v = v * 10 + (c - '0'); }
            else if (c == '.') dot = true;
            else if (c == 'e' || c == 'E')
            {
                int e = 0, es = 1; k++;
                if (k < s.Length && (s[k] == '-' || s[k] == '+')) { es = s[k] == '-' ? -1 : 1; k++; }
                for (; k < s.Length && s[k] >= '0' && s[k] <= '9'; k++) e = e * 10 + (s[k] - '0');
                v *= Math.Pow(10, es * e);
                break;
            }
            else break;
        }
        return (float)(neg ? -v : v);
    }
}

/// <summary>How a surface looks.</summary>
public sealed class Material
{
    public int Color = 0xFFFFFF;
    public float Opacity = 1;
    public Shading Shading = Shading.Lambert;
    public int Shininess = 32;
    /// <summary>Strength of the Phong highlight, 0..1.</summary>
    public float Specular = 0.5f;
    public Texture? Map;
    public bool Wireframe, DoubleSided, Transparent;
    public bool DepthTest = true, DepthWrite = true;
    public string Name = "";

    public static Material Basic(int color) => new Material { Color = color, Shading = Shading.Basic };
    public static Material Lambert(int color) => new Material { Color = color, Shading = Shading.Lambert };
    public static Material Phong(int color, int shininess = 32, float specular = 0.5f) => new Material { Color = color, Shading = Shading.Phong, Shininess = shininess, Specular = specular };

    internal DrawFlags Flags
    {
        get
        {
            var f = Shading == Shading.Basic ? DrawFlags.Unlit : Shading == Shading.Lambert ? DrawFlags.Lambert : DrawFlags.Phong;
            if (DoubleSided) f |= DrawFlags.DoubleSided;
            if (Wireframe) f |= DrawFlags.Wireframe;
            if (!DepthTest) f |= DrawFlags.NoDepthTest;
            if (!DepthWrite || Transparent) f |= DrawFlags.NoDepthWrite;
            if (Transparent) f |= DrawFlags.Blend;
            return f;
        }
    }
}

public enum LightType { Directional, Point, Ambient }

/// <summary>A light; directional lights shine along their node's -Z (use LookAt),
/// point lights from the node's position.</summary>
public sealed class Light
{
    public LightType Type;
    public int Color = 0xFFFFFF;
    public float Intensity = 1;
    /// <summary>Point lights: distance where the light has faded out (0 = never).</summary>
    public float Range;
    public bool Enabled = true;

    public static Light Directional(int color = 0xFFFFFF, float intensity = 1) => new Light { Type = LightType.Directional, Color = color, Intensity = intensity };
    public static Light Point(int color = 0xFFFFFF, float intensity = 1, float range = 0) => new Light { Type = LightType.Point, Color = color, Intensity = intensity, Range = range };
    public static Light Ambient(int color = 0xFFFFFF, float intensity = 0.3f) => new Light { Type = LightType.Ambient, Color = color, Intensity = intensity };
}

/// <summary>A camera; it looks along its node's -Z.</summary>
public sealed class Camera
{
    public bool IsOrthographic;
    /// <summary>Vertical field of view in radians (perspective).</summary>
    public float FieldOfView = 1f;
    /// <summary>Visible height in world units (orthographic).</summary>
    public float Height = 5;
    public float Near = 0.1f, Far = 100f;
    /// <summary>Width / height; 0 = the renderer's.</summary>
    public float AspectRatio;

    public static Camera Perspective(float fieldOfView, float near = 0.1f, float far = 100f) => new Camera { FieldOfView = fieldOfView, Near = near, Far = far };
    public static Camera Orthographic(float height, float near = 0.1f, float far = 100f) => new Camera { IsOrthographic = true, Height = height, Near = near, Far = far };

    public Matrix4 Projection(float aspect)
    {
        float a = AspectRatio > 0 ? AspectRatio : aspect;
        return IsOrthographic ? Matrix4.Orthographic(Height, a, Near, Far) : Matrix4.Perspective(FieldOfView, a, Near, Far);
    }
}

/// <summary>An object in the scene graph.</summary>
public sealed partial class Node
{
    public string Name;
    public Vector3 Position = Vector3.Zero;
    /// <summary>Rotation as Euler angles (radians), applied Y, then X, then Z.</summary>
    public Vector3 EulerAngles = Vector3.Zero;
    public Vector3 Scale = Vector3.One;
    public bool Visible = true;
    public Geometry? Geometry;
    public Material? Material;
    public Light? Light;
    public Camera? Camera;
    public object? Tag;
    public Node? Parent { get; private set; }
    public List<Node> Children { get; } = new List<Node>();
    /// <summary>Set by Scene.UpdateWorldTransforms (every render).</summary>
    public Matrix4 WorldMatrix = Matrix4.Identity;

    public Node(string name = "") { Name = name; }

    public Matrix4 LocalMatrix => Matrix4.Compose(Position, EulerAngles, Scale);
    public Vector3 WorldPosition => WorldMatrix.Translation3;
    /// <summary>The direction the node faces (its -Z) in world space.</summary>
    public Vector3 WorldForward => Vector3.Normalize(WorldMatrix.TransformDirection(new Vector3(0, 0, -1)));

    public Node Add(Node child)
    {
        child.Parent?.Children.Remove(child);
        child.Parent = this;
        Children.Add(child);
        return child;
    }

    public Node CreateChild(string name = "") => Add(new Node(name));

    public void Remove() { Parent?.Children.Remove(this); Parent = null; }

    public void Translate(Vector3 d) => Position = Position + d;

    /// <summary>Turn so -Z points at `target` (a point in the parent's space).</summary>
    public void LookAt(Vector3 target)
    {
        var f = Vector3.Normalize(target - Position);
        if (f.Length() < 0.5f) return;
        float pitch = MathF.Asin(f.Y < -1 ? -1 : f.Y > 1 ? 1 : f.Y);
        float yaw = MathF.Atan2(-f.X, -f.Z);
        EulerAngles = new Vector3(pitch, yaw, 0);
    }

    public void Traverse(Action<Node> visit)
    {
        visit(this);
        for (int i = 0; i < Children.Count; i++) Children[i].Traverse(visit);
    }

    internal void UpdateWorld(Matrix4 parent)
    {
        WorldMatrix = parent * LocalMatrix;
        for (int i = 0; i < Children.Count; i++) Children[i].UpdateWorld(WorldMatrix);
    }

    public override string ToString() => Name.Length > 0 ? Name : "Node";
}

/// <summary>A scene: the node tree plus background, ambient light and fog.</summary>
public sealed partial class Scene
{
    public Node Root { get; } = new Node("root");
    public int Background = 0x101820;
    public int AmbientColor = 0xFFFFFF;
    public float AmbientIntensity = 0.25f;
    public int FogColor = 0x101820;
    /// <summary>Linear fog between these distances from the camera (off unless FogEnd &gt; FogStart).</summary>
    public float FogStart, FogEnd;
    public Node? ActiveCamera;

    public Node CreateNode(Node? parent = null, string name = "") => (parent ?? Root).Add(new Node(name));

    public Node AddMesh(Geometry geometry, Material material, Node? parent = null, string name = "")
    {
        var n = CreateNode(parent, name);
        n.Geometry = geometry; n.Material = material;
        return n;
    }

    public Node AddLight(Light light, Node? parent = null, string name = "")
    {
        var n = CreateNode(parent, name);
        n.Light = light;
        return n;
    }

    public Node AddCamera(Camera camera, Vector3 position, Node? parent = null, bool makeActive = true)
    {
        var n = CreateNode(parent, "camera");
        n.Camera = camera; n.Position = position;
        if (makeActive) ActiveCamera = n;
        return n;
    }

    public Node? FindByName(string name)
    {
        Node? hit = null;
        Root.Traverse(n => { if (hit == null && n.Name == name) hit = n; });
        return hit;
    }

    public int NodeCount { get { int c = 0; Root.Traverse(_ => c++); return c; } }

    public void UpdateWorldTransforms() => Root.UpdateWorld(Matrix4.Identity);
}

/// <summary>What the last frame cost.</summary>
public struct RenderStats
{
    public int DrawCalls, Triangles, Pixels, Lights;
    public long Milliseconds;
    public override string ToString() => DrawCalls + " objek, " + Triangles + " segitiga, " + Pixels + " piksel, " + Milliseconds + " ms";
}

/// <summary>Draws a scene into a Device3D (the kernel rasterizer).</summary>
public sealed partial class Renderer : IDisposable
{
    public Device3D Device { get; private set; }
    public int Width => Device.Width;
    public int Height => Device.Height;
    public RenderStats Stats;
    readonly Lighting _light = new Lighting();
    readonly List<Node> _draw = new List<Node>();
    readonly List<Node> _blend = new List<Node>();
    readonly List<float> _depth = new List<float>();

    public Renderer(int width, int height) { Device = new Device3D(width, height); }

    /// <summary>Recreate the render target at another size.</summary>
    public void Resize(int width, int height)
    {
        if (width == Width && height == Height) return;
        Device.Dispose();
        Device = new Device3D(width, height);
    }

    static (float r, float g, float b) Rgb(int c, float k) => (((c >> 16) & 0xFF) / 255f * k, ((c >> 8) & 0xFF) / 255f * k, (c & 0xFF) / 255f * k);

    /// <summary>Draw `scene` as seen by `camera` (default: scene.ActiveCamera, else one at (0, 0, 5)).</summary>
    public RenderStats Render(Scene scene, Node? camera = null)
    {
        long t0 = Environment.TickCount64;
        scene.UpdateWorldTransforms();
        var cam = camera ?? scene.ActiveCamera;
        Matrix4 view, proj;
        Vector3 eye;
        float aspect = (float)Width / Height;
        if (cam != null && cam.Camera != null)
        {
            view = cam.WorldMatrix.InvertAffine();
            proj = cam.Camera.Projection(aspect);
            eye = cam.WorldPosition;
        }
        else
        {
            eye = new Vector3(0, 0, 5);
            view = Matrix4.LookAt(eye, Vector3.Zero, Vector3.UnitY);
            proj = Matrix4.Perspective(1f, aspect, 0.1f, 100f);
        }
        var vp = proj * view;

        // lights: ambient sums, then up to 8 directional / point lights
        var amb = Rgb(scene.AmbientColor, scene.AmbientIntensity);
        _light.Lights.Clear();
        _draw.Clear(); _blend.Clear(); _depth.Clear();
        Collect(scene.Root, ref amb);
        float m = MathF.Max(amb.r, MathF.Max(amb.g, amb.b));
        if (m > 0)
        {
            _light.AmbientIntensity = m;
            _light.AmbientColor = ((int)(amb.r / m * 255) << 16) | ((int)(amb.g / m * 255) << 8) | (int)(amb.b / m * 255);
        }
        else _light.AmbientIntensity = 0;
        _light.Eye = eye;
        _light.FogColor = scene.FogColor; _light.FogStart = scene.FogStart; _light.FogEnd = scene.FogEnd;

        Device.Clear(scene.Background);
        Device.SetLighting(_light);
        var st = new RenderStats { Lights = _light.Lights.Count };
        foreach (var n in _draw) Draw(n, vp, ref st);
        // transparent objects last, far to near
        for (int i = 0; i < _blend.Count; i++) _depth.Add(Vector3.Distance(_blend[i].WorldPosition, eye));
        for (int i = 1; i < _blend.Count; i++)
            for (int j = i; j > 0 && _depth[j] > _depth[j - 1]; j--)
            {
                (_blend[j], _blend[j - 1]) = (_blend[j - 1], _blend[j]);
                (_depth[j], _depth[j - 1]) = (_depth[j - 1], _depth[j]);
            }
        foreach (var n in _blend) Draw(n, vp, ref st);
        st.Milliseconds = Environment.TickCount64 - t0;
        Stats = st;
        return st;
    }

    void Collect(Node n, ref (float r, float g, float b) amb)
    {
        if (!n.Visible) return;
        var l = n.Light;
        if (l != null && l.Enabled)
        {
            if (l.Type == LightType.Ambient)
            {
                var a = Rgb(l.Color, l.Intensity);
                amb = (amb.r + a.r, amb.g + a.g, amb.b + a.b);
            }
            else if (_light.Lights.Count < Lighting.MaxLights)
            {
                _light.Lights.Add(l.Type == LightType.Point
                    ? LightSource.Point(n.WorldPosition, l.Color, l.Intensity, l.Range)
                    : LightSource.Directional(n.WorldForward, l.Color, l.Intensity));
            }
        }
        if (n.Geometry != null && n.Material != null && n.Geometry.TriangleCount > 0)
        {
            if (n.Material.Transparent) _blend.Add(n); else _draw.Add(n);
        }
        for (int i = 0; i < n.Children.Count; i++) Collect(n.Children[i], ref amb);
    }

    void Draw(Node n, Matrix4 vp, ref RenderStats st)
    {
        var mat = n.Material!;
        var s = Device.Draw(n.Geometry!.Gpu(), mat.Map?.Gpu(), n.WorldMatrix, vp, mat.Flags, mat.Color, mat.Opacity, mat.Shininess, mat.Specular);
        st.DrawCalls++;
        st.Triangles += s.Triangles;
        st.Pixels += s.Pixels;
    }

    /// <summary>Copy the picture into a window at (x, y); Window.Present shows it.</summary>
    public bool PresentTo(int windowId, int x, int y) => Device.PresentTo(windowId, x, y);

    public int ReadPixel(int x, int y) => Device.ReadPixel(x, y);

    /// <summary>Where a world point lands on the picture (pixels), and whether it is in front of the camera.</summary>
    public (int x, int y, bool visible) Project(Scene scene, Vector3 world, Node? camera = null)
    {
        scene.UpdateWorldTransforms();
        var cam = camera ?? scene.ActiveCamera;
        if (cam?.Camera == null) return (0, 0, false);
        var vp = cam.Camera.Projection((float)Width / Height) * cam.WorldMatrix.InvertAffine();
        float x = vp.M00 * world.X + vp.M01 * world.Y + vp.M02 * world.Z + vp.M03;
        float y = vp.M10 * world.X + vp.M11 * world.Y + vp.M12 * world.Z + vp.M13;
        float w = vp.M30 * world.X + vp.M31 * world.Y + vp.M32 * world.Z + vp.M33;
        if (w <= 0) return (0, 0, false);
        return ((int)((x / w + 1) * 0.5f * Width), (int)((1 - y / w) * 0.5f * Height), true);
    }

    public void Dispose() => Device.Dispose();
}
