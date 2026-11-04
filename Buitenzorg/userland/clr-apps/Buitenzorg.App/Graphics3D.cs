// Buitenzorg.Graphics3D -- the low-level 3D API (G3): render contexts with a
// depth buffer, vertex/index buffers (Mesh3D), textures, matrices, lights.
// Every draw goes to the kernel's rasterizer through the GFX3D syscall (56):
// transform, lighting, clipping, rasterization and texturing run as native
// code there, so a managed app in the interpreter only builds the data and
// issues one call per mesh. The scene graph `Buitenzorg.Three` (BZTHREE.DLL)
// sits on top of this.
//
// Off the device (`dotnet App.dll`) the natives do not bind and a small
// managed rasterizer stands in (unlit, untextured, depth-tested), so apps and
// their self-tests run anywhere; Device3D.IsSimulated says which one draws.
//
// Conventions: right-handed, +Y up, the camera looks down -Z; matrices are
// row-major and multiply column vectors (translation in M03, M13, M23); clip
// space is OpenGL's; counter-clockwise triangles face the viewer.

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace Buitenzorg.Graphics3D;

internal static class G3dNative
{
    [DllImport("buitenzorg")] internal static extern int G3dCreate(int width, int height);
    [DllImport("buitenzorg")] internal static extern int G3dFree(int id);
    [DllImport("buitenzorg")] internal static extern int G3dClear(int context, int color, int bits);
    [DllImport("buitenzorg")] internal static extern int G3dMesh(float[] vertices, int[]? colors, int[] indices);
    [DllImport("buitenzorg")] internal static extern int G3dTexture(int width, int height, int[] pixels);
    [DllImport("buitenzorg")] internal static extern int G3dLights(int context, float[] block);
    [DllImport("buitenzorg")] internal static extern long G3dDraw(int context, int mesh, int texture, int flags, int color, float[] matrices);
    [DllImport("buitenzorg")] internal static extern int G3dPresent(int context, int window, int x, int y);
    [DllImport("buitenzorg")] internal static extern int G3dReadPixel(int context, int x, int y);
    [DllImport("buitenzorg")] internal static extern int G3dRead(int context, int[] pixels);
    [DllImport("buitenzorg")] internal static extern int G3dInfo(int context);

    static int _mode; // 0 unknown, 1 kernel, 2 simulated

    /// <summary>True when the natives are missing (off the device).</summary>
    internal static bool Simulated
    {
        get
        {
            if (_mode == 0)
            {
                try { G3dFree(0); _mode = 1; }
                catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _mode = 2; }
            }
            return _mode == 2;
        }
    }
}

/// <summary>A 3-component vector.</summary>
public struct Vector3
{
    public float X, Y, Z;
    public Vector3(float x, float y, float z) { X = x; Y = y; Z = z; }

    public static Vector3 Zero => new Vector3(0, 0, 0);
    public static Vector3 One => new Vector3(1, 1, 1);
    public static Vector3 UnitX => new Vector3(1, 0, 0);
    public static Vector3 UnitY => new Vector3(0, 1, 0);
    public static Vector3 UnitZ => new Vector3(0, 0, 1);

    public static Vector3 operator +(Vector3 a, Vector3 b) => new Vector3(a.X + b.X, a.Y + b.Y, a.Z + b.Z);
    public static Vector3 operator -(Vector3 a, Vector3 b) => new Vector3(a.X - b.X, a.Y - b.Y, a.Z - b.Z);
    public static Vector3 operator -(Vector3 a) => new Vector3(-a.X, -a.Y, -a.Z);
    public static Vector3 operator *(Vector3 a, float s) => new Vector3(a.X * s, a.Y * s, a.Z * s);
    public static Vector3 operator *(float s, Vector3 a) => new Vector3(a.X * s, a.Y * s, a.Z * s);

    public static float Dot(Vector3 a, Vector3 b) => a.X * b.X + a.Y * b.Y + a.Z * b.Z;
    public static Vector3 Cross(Vector3 a, Vector3 b) => new Vector3(a.Y * b.Z - a.Z * b.Y, a.Z * b.X - a.X * b.Z, a.X * b.Y - a.Y * b.X);
    public float Length() => MathF.Sqrt(X * X + Y * Y + Z * Z);
    public static float Distance(Vector3 a, Vector3 b) => (a - b).Length();
    public static Vector3 Lerp(Vector3 a, Vector3 b, float t) => a + (b - a) * t;

    public static Vector3 Normalize(Vector3 v)
    {
        float l = v.Length();
        return l > 1e-12f ? v * (1f / l) : Zero;
    }

    public override string ToString() => "(" + X + ", " + Y + ", " + Z + ")";
}

/// <summary>A row-major 4x4 matrix for column vectors (translation in M03, M13, M23).</summary>
public struct Matrix4
{
    public float M00, M01, M02, M03;
    public float M10, M11, M12, M13;
    public float M20, M21, M22, M23;
    public float M30, M31, M32, M33;

    public static Matrix4 Identity => new Matrix4 { M00 = 1, M11 = 1, M22 = 1, M33 = 1 };

    public static Matrix4 Translation(Vector3 t)
    {
        var m = Identity;
        m.M03 = t.X; m.M13 = t.Y; m.M23 = t.Z;
        return m;
    }

    public static Matrix4 Scaling(Vector3 s) => new Matrix4 { M00 = s.X, M11 = s.Y, M22 = s.Z, M33 = 1 };

    public static Matrix4 RotationX(float a)
    {
        float c = MathF.Cos(a), s = MathF.Sin(a);
        return new Matrix4 { M00 = 1, M11 = c, M12 = -s, M21 = s, M22 = c, M33 = 1 };
    }

    public static Matrix4 RotationY(float a)
    {
        float c = MathF.Cos(a), s = MathF.Sin(a);
        return new Matrix4 { M00 = c, M02 = s, M11 = 1, M20 = -s, M22 = c, M33 = 1 };
    }

    public static Matrix4 RotationZ(float a)
    {
        float c = MathF.Cos(a), s = MathF.Sin(a);
        return new Matrix4 { M00 = c, M01 = -s, M10 = s, M11 = c, M22 = 1, M33 = 1 };
    }

    /// <summary>Euler angles applied in the order Y, X, Z (three.js / ThreeNet "YXZ").</summary>
    public static Matrix4 RotationYXZ(Vector3 euler) => RotationY(euler.Y) * RotationX(euler.X) * RotationZ(euler.Z);

    /// <summary>Translation * rotation (YXZ Euler) * scale.</summary>
    public static Matrix4 Compose(Vector3 position, Vector3 euler, Vector3 scale) =>
        Translation(position) * RotationYXZ(euler) * Scaling(scale);

    /// <summary>OpenGL-style perspective projection; fovY in radians.</summary>
    public static Matrix4 Perspective(float fovY, float aspect, float near, float far)
    {
        float f = 1f / MathF.Tan(fovY / 2);
        return new Matrix4
        {
            M00 = f / aspect,
            M11 = f,
            M22 = (far + near) / (near - far),
            M23 = 2 * far * near / (near - far),
            M32 = -1,
        };
    }

    /// <summary>Orthographic projection showing `height` units vertically.</summary>
    public static Matrix4 Orthographic(float height, float aspect, float near, float far)
    {
        float h = height / 2, w = h * aspect;
        return new Matrix4 { M00 = 1 / w, M11 = 1 / h, M22 = -2 / (far - near), M23 = -(far + near) / (far - near), M33 = 1 };
    }

    /// <summary>A view matrix: the camera at `eye` looking at `target`.</summary>
    public static Matrix4 LookAt(Vector3 eye, Vector3 target, Vector3 up)
    {
        var z = Vector3.Normalize(eye - target);
        var x = Vector3.Normalize(Vector3.Cross(up, z));
        if (x.Length() < 0.5f) x = Vector3.Normalize(Vector3.Cross(Vector3.UnitZ, z));
        var y = Vector3.Cross(z, x);
        return new Matrix4
        {
            M00 = x.X, M01 = x.Y, M02 = x.Z, M03 = -Vector3.Dot(x, eye),
            M10 = y.X, M11 = y.Y, M12 = y.Z, M13 = -Vector3.Dot(y, eye),
            M20 = z.X, M21 = z.Y, M22 = z.Z, M23 = -Vector3.Dot(z, eye),
            M33 = 1,
        };
    }

    public static Matrix4 operator *(Matrix4 a, Matrix4 b)
    {
        Matrix4 r;
        r.M00 = a.M00 * b.M00 + a.M01 * b.M10 + a.M02 * b.M20 + a.M03 * b.M30;
        r.M01 = a.M00 * b.M01 + a.M01 * b.M11 + a.M02 * b.M21 + a.M03 * b.M31;
        r.M02 = a.M00 * b.M02 + a.M01 * b.M12 + a.M02 * b.M22 + a.M03 * b.M32;
        r.M03 = a.M00 * b.M03 + a.M01 * b.M13 + a.M02 * b.M23 + a.M03 * b.M33;
        r.M10 = a.M10 * b.M00 + a.M11 * b.M10 + a.M12 * b.M20 + a.M13 * b.M30;
        r.M11 = a.M10 * b.M01 + a.M11 * b.M11 + a.M12 * b.M21 + a.M13 * b.M31;
        r.M12 = a.M10 * b.M02 + a.M11 * b.M12 + a.M12 * b.M22 + a.M13 * b.M32;
        r.M13 = a.M10 * b.M03 + a.M11 * b.M13 + a.M12 * b.M23 + a.M13 * b.M33;
        r.M20 = a.M20 * b.M00 + a.M21 * b.M10 + a.M22 * b.M20 + a.M23 * b.M30;
        r.M21 = a.M20 * b.M01 + a.M21 * b.M11 + a.M22 * b.M21 + a.M23 * b.M31;
        r.M22 = a.M20 * b.M02 + a.M21 * b.M12 + a.M22 * b.M22 + a.M23 * b.M32;
        r.M23 = a.M20 * b.M03 + a.M21 * b.M13 + a.M22 * b.M23 + a.M23 * b.M33;
        r.M30 = a.M30 * b.M00 + a.M31 * b.M10 + a.M32 * b.M20 + a.M33 * b.M30;
        r.M31 = a.M30 * b.M01 + a.M31 * b.M11 + a.M32 * b.M21 + a.M33 * b.M31;
        r.M32 = a.M30 * b.M02 + a.M31 * b.M12 + a.M32 * b.M22 + a.M33 * b.M32;
        r.M33 = a.M30 * b.M03 + a.M31 * b.M13 + a.M32 * b.M23 + a.M33 * b.M33;
        return r;
    }

    /// <summary>The point p transformed (with the perspective divide).</summary>
    public Vector3 TransformPoint(Vector3 p)
    {
        float x = M00 * p.X + M01 * p.Y + M02 * p.Z + M03;
        float y = M10 * p.X + M11 * p.Y + M12 * p.Z + M13;
        float z = M20 * p.X + M21 * p.Y + M22 * p.Z + M23;
        float w = M30 * p.X + M31 * p.Y + M32 * p.Z + M33;
        return w != 0 && w != 1 ? new Vector3(x / w, y / w, z / w) : new Vector3(x, y, z);
    }

    /// <summary>The direction d transformed (no translation).</summary>
    public Vector3 TransformDirection(Vector3 d) =>
        new Vector3(M00 * d.X + M01 * d.Y + M02 * d.Z, M10 * d.X + M11 * d.Y + M12 * d.Z, M20 * d.X + M21 * d.Y + M22 * d.Z);

    public Vector3 Translation3 => new Vector3(M03, M13, M23);

    /// <summary>The inverse of an affine matrix (rotation, uniform or not scale, translation).</summary>
    public Matrix4 InvertAffine()
    {
        float a = M00, b = M01, c = M02, d = M10, e = M11, f = M12, g = M20, h = M21, i = M22;
        float A = e * i - f * h, B = -(d * i - f * g), C = d * h - e * g;
        float det = a * A + b * B + c * C;
        if (MathF.Abs(det) < 1e-20f) return Identity;
        float k = 1 / det;
        var r = new Matrix4
        {
            M00 = A * k, M01 = -(b * i - c * h) * k, M02 = (b * f - c * e) * k,
            M10 = B * k, M11 = (a * i - c * g) * k, M12 = -(a * f - c * d) * k,
            M20 = C * k, M21 = -(a * h - b * g) * k, M22 = (a * e - b * d) * k,
            M33 = 1,
        };
        r.M03 = -(r.M00 * M03 + r.M01 * M13 + r.M02 * M23);
        r.M13 = -(r.M10 * M03 + r.M11 * M13 + r.M12 * M23);
        r.M23 = -(r.M20 * M03 + r.M21 * M13 + r.M22 * M23);
        return r;
    }

    internal void CopyTo(float[] d, int o)
    {
        d[o] = M00; d[o + 1] = M01; d[o + 2] = M02; d[o + 3] = M03;
        d[o + 4] = M10; d[o + 5] = M11; d[o + 6] = M12; d[o + 7] = M13;
        d[o + 8] = M20; d[o + 9] = M21; d[o + 10] = M22; d[o + 11] = M23;
        d[o + 12] = M30; d[o + 13] = M31; d[o + 14] = M32; d[o + 15] = M33;
    }
}

/// <summary>How a draw is shaded and blended (bz-abi gfx3d_flags).</summary>
[Flags]
public enum DrawFlags
{
    Unlit = 0,
    Lambert = 1,
    Phong = 2,
    DoubleSided = 4,
    Wireframe = 8,
    NoDepthTest = 16,
    NoDepthWrite = 32,
    Blend = 64,
}

/// <summary>One light for <see cref="Lighting"/>.</summary>
public struct LightSource
{
    public bool IsPoint;
    /// <summary>Directional: the direction the light travels. Point: its position.</summary>
    public Vector3 Vector;
    /// <summary>0xRRGGBB.</summary>
    public int Color;
    public float Intensity;
    /// <summary>Point lights: the distance where the light fades out (0 = never).</summary>
    public float Range;

    public static LightSource Directional(Vector3 direction, int color = 0xFFFFFF, float intensity = 1f) =>
        new LightSource { Vector = direction, Color = color, Intensity = intensity };

    public static LightSource Point(Vector3 position, int color = 0xFFFFFF, float intensity = 1f, float range = 0) =>
        new LightSource { IsPoint = true, Vector = position, Color = color, Intensity = intensity, Range = range };
}

/// <summary>Ambient light, up to 8 lights, fog and the eye position of a context.</summary>
public sealed class Lighting
{
    public const int MaxLights = 8;
    public int AmbientColor = 0xFFFFFF;
    public float AmbientIntensity = 0.3f;
    public Vector3 Eye = new Vector3(0, 0, 10);
    public int FogColor;
    /// <summary>Fog is off unless FogEnd &gt; FogStart (distances from the eye).</summary>
    public float FogStart, FogEnd;
    public readonly List<LightSource> Lights = new List<LightSource>();

    static float Ch(int c, int shift) => ((c >> shift) & 0xFF) / 255f;

    internal float[] ToBlock()
    {
        int n = Math.Min(Lights.Count, MaxLights);
        var b = new float[12 + 8 * n];
        b[0] = Ch(AmbientColor, 16) * AmbientIntensity; b[1] = Ch(AmbientColor, 8) * AmbientIntensity; b[2] = Ch(AmbientColor, 0) * AmbientIntensity;
        b[3] = Eye.X; b[4] = Eye.Y; b[5] = Eye.Z;
        b[6] = Ch(FogColor, 16); b[7] = Ch(FogColor, 8); b[8] = Ch(FogColor, 0);
        b[9] = FogStart; b[10] = FogEnd; b[11] = n;
        for (int i = 0; i < n; i++)
        {
            var l = Lights[i];
            int o = 12 + 8 * i;
            b[o] = l.IsPoint ? 1 : 0;
            b[o + 1] = l.Vector.X; b[o + 2] = l.Vector.Y; b[o + 3] = l.Vector.Z;
            b[o + 4] = Ch(l.Color, 16) * l.Intensity; b[o + 5] = Ch(l.Color, 8) * l.Intensity; b[o + 6] = Ch(l.Color, 0) * l.Intensity;
            b[o + 7] = l.Range;
        }
        return b;
    }
}

/// <summary>What one draw produced.</summary>
public struct DrawStats
{
    public int Triangles, Pixels;
    public override string ToString() => Triangles + " segitiga, " + Pixels + " piksel";
}

/// <summary>A vertex + index buffer. Vertices: 8 floats each (x, y, z, nx, ny, nz, u, v).</summary>
public sealed class Mesh3D : IDisposable
{
    internal int Id;
    public int VertexCount { get; }
    public int TriangleCount { get; }
    // kept for the simulated rasterizer
    internal readonly float[] Vertices;
    internal readonly int[]? Colors;
    internal readonly int[] Indices;

    public Mesh3D(float[] vertices, int[] indices, int[]? colors = null)
    {
        if (vertices.Length < 8 || vertices.Length % 8 != 0) throw new ArgumentException("8 floats per vertex", nameof(vertices));
        if (indices.Length % 3 != 0) throw new ArgumentException("whole triangles only", nameof(indices));
        VertexCount = vertices.Length / 8;
        TriangleCount = indices.Length / 3;
        Vertices = vertices; Colors = colors; Indices = indices;
        if (!G3dNative.Simulated)
        {
            Id = G3dNative.G3dMesh(vertices, colors, indices);
            if (Id <= 0) throw new InvalidOperationException("Buitenzorg.Graphics3D: the mesh was refused (too big, or the 3D memory budget is used up)");
        }
    }

    public void Dispose()
    {
        if (Id > 0) G3dNative.G3dFree(Id);
        Id = 0;
    }
}

/// <summary>A texture of 0xAARRGGBB pixels (alpha 0 = cut out).</summary>
public sealed class Texture3D : IDisposable
{
    internal int Id;
    public int Width { get; }
    public int Height { get; }

    public Texture3D(int width, int height, int[] pixels)
    {
        if (width <= 0 || height <= 0 || width > 1024 || height > 1024 || pixels.Length < width * height)
            throw new ArgumentException("texture size 1..1024 and w*h pixels");
        Width = width; Height = height;
        if (!G3dNative.Simulated)
        {
            Id = G3dNative.G3dTexture(width, height, pixels);
            if (Id <= 0) throw new InvalidOperationException("Buitenzorg.Graphics3D: the texture was refused (3D memory budget)");
        }
    }

    public void Dispose()
    {
        if (Id > 0) G3dNative.G3dFree(Id);
        Id = 0;
    }
}

/// <summary>A render context: a color buffer and a depth buffer the kernel draws into.</summary>
public sealed class Device3D : IDisposable
{
    int _id;
    readonly float[] _mats = new float[32];
    readonly SimRaster? _sim;

    public int Width { get; }
    public int Height { get; }
    /// <summary>True off the device: a managed stand-in rasterizer draws (unlit, untextured).</summary>
    public bool IsSimulated => _sim != null;

    /// <summary>True when the host GPU draws (virtio-gpu 3D / virgl), false for the kernel's software rasterizer.</summary>
    public bool IsGpu => _sim == null && G3dNative.G3dInfo(_id) == 1;

    /// <summary>Who draws: "GPU (virgl)", "kernel rasterizer" or "simulated".</summary>
    public string Backend => _sim != null ? "simulated" : IsGpu ? "GPU (virgl)" : "kernel rasterizer";
    public DrawStats LastFrame;

    public Device3D(int width, int height)
    {
        if (width <= 0 || height <= 0 || width > 1024 || height > 1024) throw new ArgumentException("size 1..1024");
        Width = width; Height = height;
        if (G3dNative.Simulated) _sim = new SimRaster(width, height);
        else
        {
            _id = G3dNative.G3dCreate(width, height);
            if (_id <= 0) throw new InvalidOperationException("Buitenzorg.Graphics3D: no render context (3D memory budget used up?)");
        }
    }

    /// <summary>Fill the color buffer with 0xRRGGBB and reset the depth buffer.</summary>
    public void Clear(int color, bool depth = true)
    {
        LastFrame = default;
        if (_sim != null) { _sim.Clear(color, depth); return; }
        G3dNative.G3dClear(_id, color, 1 | (depth ? 2 : 0));
    }

    public void SetLighting(Lighting l)
    {
        if (_sim == null) G3dNative.G3dLights(_id, l.ToBlock());
    }

    /// <summary>Draw a mesh. color = 0xRRGGBB, opacity 0..1 (with DrawFlags.Blend);
    /// shininess and specular (0..1) apply to Phong.</summary>
    public DrawStats Draw(Mesh3D mesh, Texture3D? texture, Matrix4 model, Matrix4 viewProjection,
        DrawFlags flags = DrawFlags.Lambert, int color = 0xFFFFFF, float opacity = 1f, int shininess = 32, float specular = 0.5f)
    {
        int a = (int)(Math.Clamp(opacity, 0f, 1f) * 255);
        int argb = (a << 24) | (color & 0xFFFFFF);
        int f = (int)flags | (Math.Clamp(shininess, 1, 255) << 8) | (Math.Clamp((int)(specular * 255), 1, 255) << 16);
        DrawStats s;
        if (_sim != null) s = _sim.Draw(mesh, model, viewProjection, flags, argb);
        else
        {
            model.CopyTo(_mats, 0);
            viewProjection.CopyTo(_mats, 16);
            long r = G3dNative.G3dDraw(_id, mesh.Id, texture?.Id ?? 0, f, argb, _mats);
            s = r < 0 ? default : new DrawStats { Triangles = (int)(r & 0xFFFFFFFF), Pixels = (int)(r >> 32) };
        }
        LastFrame.Triangles += s.Triangles;
        LastFrame.Pixels += s.Pixels;
        return s;
    }

    /// <summary>Copy the picture into a window (client coordinates); Window.Present shows it.</summary>
    public bool PresentTo(int windowId, int x, int y)
    {
        if (_sim != null || windowId == 0) return false;
        return G3dNative.G3dPresent(_id, windowId, x, y) == 0;
    }

    public int ReadPixel(int x, int y)
    {
        if (_sim != null) return _sim.Pixel(x, y);
        return G3dNative.G3dReadPixel(_id, x, y);
    }

    /// <summary>The whole picture, 0x00RRGGBB, row by row.</summary>
    public int[] ReadPixels()
    {
        var p = new int[Width * Height];
        if (_sim != null) Array.Copy(_sim.Color, p, p.Length);
        else G3dNative.G3dRead(_id, p);
        return p;
    }

    public void Dispose()
    {
        if (_id > 0) G3dNative.G3dFree(_id);
        _id = 0;
    }
}

/// <summary>The off-device stand-in: unlit, untextured triangles with a depth buffer.</summary>
internal sealed class SimRaster
{
    readonly int _w, _h;
    internal readonly int[] Color;
    readonly float[] _depth;

    internal SimRaster(int w, int h) { _w = w; _h = h; Color = new int[w * h]; _depth = new float[w * h]; Clear(0, true); }

    internal void Clear(int color, bool depth)
    {
        for (int i = 0; i < Color.Length; i++) Color[i] = color & 0xFFFFFF;
        if (depth) for (int i = 0; i < _depth.Length; i++) _depth[i] = float.MaxValue;
    }

    static float Min3(float a, float b, float c) => MathF.Min(a, MathF.Min(b, c));
    static float Max3(float a, float b, float c) => MathF.Max(a, MathF.Max(b, c));

    int Line(float x0, float y0, float x1, float y1, int col)
    {
        float dx = x1 - x0, dy = y1 - y0;
        int steps = (int)MathF.Max(MathF.Abs(dx), MathF.Abs(dy)) + 1, n = 0;
        for (int k = 0; k <= steps && k < 4096; k++)
        {
            int x = (int)(x0 + dx * k / steps), y = (int)(y0 + dy * k / steps);
            if (x < 0 || y < 0 || x >= _w || y >= _h) continue;
            Color[y * _w + x] = col & 0xFFFFFF;
            n++;
        }
        return n;
    }

    internal int Pixel(int x, int y) => x < 0 || y < 0 || x >= _w || y >= _h ? -1 : Color[y * _w + x];

    internal DrawStats Draw(Mesh3D m, Matrix4 model, Matrix4 vp, DrawFlags flags, int argb)
    {
        var mvp = vp * model;
        int n = m.VertexCount;
        var sx = new float[n]; var sy = new float[n]; var sz = new float[n]; var ok = new bool[n];
        for (int i = 0; i < n; i++)
        {
            float x = m.Vertices[i * 8], y = m.Vertices[i * 8 + 1], z = m.Vertices[i * 8 + 2];
            float cx = mvp.M00 * x + mvp.M01 * y + mvp.M02 * z + mvp.M03;
            float cy = mvp.M10 * x + mvp.M11 * y + mvp.M12 * z + mvp.M13;
            float cz = mvp.M20 * x + mvp.M21 * y + mvp.M22 * z + mvp.M23;
            float cw = mvp.M30 * x + mvp.M31 * y + mvp.M32 * z + mvp.M33;
            ok[i] = cw > 1e-4f && cz >= -cw;
            if (!ok[i]) continue;
            sx[i] = (cx / cw + 1) * 0.5f * _w;
            sy[i] = (1 - cy / cw) * 0.5f * _h;
            sz[i] = cz / cw;
        }
        var st = new DrawStats();
        for (int t = 0; t + 2 < m.Indices.Length; t += 3)
        {
            int a = m.Indices[t], b = m.Indices[t + 1], c = m.Indices[t + 2];
            if (!ok[a] || !ok[b] || !ok[c]) continue; // no clipping in the stand-in
            float area = (sx[b] - sx[a]) * (sy[c] - sy[a]) - (sx[c] - sx[a]) * (sy[b] - sy[a]);
            if (area == 0 || (area > 0 && (flags & DrawFlags.DoubleSided) == 0)) continue;
            int col = argb;
            if (m.Colors != null && a < m.Colors.Length)
            {
                int vc = m.Colors[a];
                col = (((((col >> 16) & 0xFF) * ((vc >> 16) & 0xFF)) / 255) << 16) | (((((col >> 8) & 0xFF) * ((vc >> 8) & 0xFF)) / 255) << 8) | (((col & 0xFF) * (vc & 0xFF)) / 255);
            }
            if ((flags & DrawFlags.Wireframe) != 0)
            {
                int px = Line(sx[a], sy[a], sx[b], sy[b], col) + Line(sx[b], sy[b], sx[c], sy[c], col) + Line(sx[c], sy[c], sx[a], sy[a], col);
                st.Pixels += px;
                if (px > 0) st.Triangles++;
                continue;
            }
            int x0 = Math.Max(0, (int)MathF.Floor(Min3(sx[a], sx[b], sx[c]))), x1 = Math.Min(_w - 1, (int)MathF.Ceiling(Max3(sx[a], sx[b], sx[c])));
            int y0 = Math.Max(0, (int)MathF.Floor(Min3(sy[a], sy[b], sy[c]))), y1 = Math.Min(_h - 1, (int)MathF.Ceiling(Max3(sy[a], sy[b], sy[c])));
            bool any = false;
            for (int y = y0; y <= y1; y++)
                for (int x = x0; x <= x1; x++)
                {
                    float px = x + 0.5f, py = y + 0.5f;
                    float w0 = ((sx[c] - sx[b]) * (py - sy[b]) - (sy[c] - sy[b]) * (px - sx[b])) / area;
                    float w1 = ((sx[a] - sx[c]) * (py - sy[c]) - (sy[a] - sy[c]) * (px - sx[c])) / area;
                    float w2 = 1 - w0 - w1;
                    if (w0 < 0 || w1 < 0 || w2 < 0) continue;
                    float z = w0 * sz[a] + w1 * sz[b] + w2 * sz[c];
                    int i = y * _w + x;
                    if ((flags & DrawFlags.NoDepthTest) == 0 && z >= _depth[i]) continue;
                    Color[i] = col & 0xFFFFFF;
                    if ((flags & DrawFlags.NoDepthWrite) == 0) _depth[i] = z;
                    st.Pixels++;
                    any = true;
                }
            if (any) st.Triangles++;
        }
        return st;
    }
}
