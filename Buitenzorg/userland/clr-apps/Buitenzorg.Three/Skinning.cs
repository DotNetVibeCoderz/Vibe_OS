// Skeletal animation for Buitenzorg.Three: skins (joints = ordinary Nodes in
// the scene + their inverse bind matrices), CPU vertex skinning with up to
// four weighted joints per vertex, and keyframed animation clips (translation,
// rotation as quaternions with slerp, scale; linear or step) played by an
// Animator. glTF skins and animations load into these (Gltf.cs).
//
// Skinning runs on the CPU every frame the skeleton moved: the kernel
// rasterizer takes finished vertices, so the skinned positions and normals go
// in through Geometry.Update.

using System;
using System.Collections.Generic;
using Buitenzorg.Graphics3D;

namespace Buitenzorg.Three;

/// <summary>A unit quaternion (x, y, z, w).</summary>
public struct Quat
{
    public float X, Y, Z, W;
    public Quat(float x, float y, float z, float w) { X = x; Y = y; Z = z; W = w; }
    public static Quat Identity => new Quat(0, 0, 0, 1);

    public static Quat operator *(Quat a, Quat b) => new Quat(
        a.W * b.X + a.X * b.W + a.Y * b.Z - a.Z * b.Y,
        a.W * b.Y - a.X * b.Z + a.Y * b.W + a.Z * b.X,
        a.W * b.Z + a.X * b.Y - a.Y * b.X + a.Z * b.W,
        a.W * b.W - a.X * b.X - a.Y * b.Y - a.Z * b.Z);

    public static Quat AxisAngle(Vector3 axis, float angle)
    {
        var a = Vector3.Normalize(axis);
        float s = MathF.Sin(angle / 2);
        return new Quat(a.X * s, a.Y * s, a.Z * s, MathF.Cos(angle / 2));
    }

    /// <summary>The rotation of Node's Euler angles (R = Ry * Rx * Rz).</summary>
    public static Quat FromEulerYXZ(Vector3 e) =>
        AxisAngle(Vector3.UnitY, e.Y) * AxisAngle(new Vector3(1, 0, 0), e.X) * AxisAngle(new Vector3(0, 0, 1), e.Z);

    /// <summary>As Node's Euler angles (R = Ry * Rx * Rz).</summary>
    public Vector3 ToEulerYXZ()
    {
        float x = X, y = Y, z = Z, w = W;
        float m13 = 2 * (x * z + w * y), m23 = 2 * (y * z - w * x), m33 = 1 - 2 * (x * x + y * y);
        float m21 = 2 * (x * y + w * z), m22 = 1 - 2 * (x * x + z * z), m11 = 1 - 2 * (y * y + z * z), m31 = 2 * (x * z - w * y);
        float ex = MathF.Asin(Math.Clamp(-m23, -1f, 1f));
        if (MathF.Abs(m23) < 0.99999f) return new Vector3(ex, MathF.Atan2(m13, m33), MathF.Atan2(m21, m22));
        return new Vector3(ex, MathF.Atan2(-m31, m11), 0);
    }

    /// <summary>Spherical interpolation along the shorter arc.</summary>
    public static Quat Slerp(Quat a, Quat b, float t)
    {
        float d = a.X * b.X + a.Y * b.Y + a.Z * b.Z + a.W * b.W;
        if (d < 0) { b = new Quat(-b.X, -b.Y, -b.Z, -b.W); d = -d; }
        float ka, kb;
        if (d > 0.9995f) { ka = 1 - t; kb = t; }
        else
        {
            float th = MathF.Acos(d), s = MathF.Sin(th);
            ka = MathF.Sin((1 - t) * th) / s;
            kb = MathF.Sin(t * th) / s;
        }
        var r = new Quat(a.X * ka + b.X * kb, a.Y * ka + b.Y * kb, a.Z * ka + b.Z * kb, a.W * ka + b.W * kb);
        float n = MathF.Sqrt(r.X * r.X + r.Y * r.Y + r.Z * r.Z + r.W * r.W);
        return n > 0 ? new Quat(r.X / n, r.Y / n, r.Z / n, r.W / n) : Identity;
    }
}

/// <summary>The joints of a skeleton (nodes in the scene) and their inverse bind matrices.</summary>
public sealed class Skin
{
    public readonly Node[] Joints;
    public readonly Matrix4[] InverseBind;

    public Skin(Node[] joints, Matrix4[] inverseBind)
    {
        if (joints.Length != inverseBind.Length) throw new ArgumentException("one inverse bind matrix per joint");
        Joints = joints; InverseBind = inverseBind;
    }

    /// <summary>A skin bound in the joints' current pose: each inverse bind matrix
    /// is the inverse of its joint's world matrix now (call after placing the skeleton).</summary>
    public static Skin BindPose(Scene scene, Node[] joints)
    {
        scene.UpdateWorldTransforms();
        var inv = new Matrix4[joints.Length];
        for (int i = 0; i < joints.Length; i++) inv[i] = joints[i].WorldMatrix.InvertAffine();
        return new Skin(joints, inv);
    }
}

/// <summary>A mesh deformed by a skin: bind-pose vertices + up to four joints and weights per vertex.</summary>
public sealed class SkinnedMesh
{
    public readonly Node Owner;
    public readonly Skin Skin;
    readonly float[] _bind;
    readonly int[] _joints;
    readonly float[] _weights;
    float[] _out;
    Matrix4[] _m;
    /// <summary>Frames skinned so far.</summary>
    public int Updates { get; private set; }

    /// <summary>`joints` / `weights`: four per vertex (weights are normalised here).</summary>
    public SkinnedMesh(Node owner, Skin skin, int[] joints, float[] weights)
    {
        var g = owner.Geometry ?? throw new ArgumentException("the node has no geometry");
        int n = g.VertexCount;
        if (joints.Length != n * 4 || weights.Length != n * 4) throw new ArgumentException("four joints and weights per vertex");
        Owner = owner; Skin = skin;
        _bind = new float[g.Vertices.Length];
        Array.Copy(g.Vertices, _bind, _bind.Length);
        _out = new float[_bind.Length];
        _joints = joints;
        _weights = new float[weights.Length];
        for (int v = 0; v < n; v++)
        {
            float s = 0;
            for (int k = 0; k < 4; k++)
            {
                int j = joints[v * 4 + k];
                if (j < 0 || j >= skin.Joints.Length) throw new ArgumentException("joint index out of range");
                s += weights[v * 4 + k];
            }
            for (int k = 0; k < 4; k++) _weights[v * 4 + k] = s > 0 ? weights[v * 4 + k] / s : (k == 0 ? 1 : 0);
        }
        _m = new Matrix4[skin.Joints.Length];
        owner.Skinned = this;
    }

    /// <summary>Deform the vertices for the joints' current world matrices (needs up-to-date WorldMatrix values).</summary>
    public void Apply()
    {
        var inv = Owner.WorldMatrix.InvertAffine();
        for (int j = 0; j < _m.Length; j++) _m[j] = inv * Skin.Joints[j].WorldMatrix * Skin.InverseBind[j];
        int n = _bind.Length / 8;
        for (int v = 0; v < n; v++)
        {
            int o = v * 8;
            float px = 0, py = 0, pz = 0, nx = 0, ny = 0, nz = 0;
            for (int k = 0; k < 4; k++)
            {
                float w = _weights[v * 4 + k];
                if (w == 0) continue;
                Matrix4 m = _m[_joints[v * 4 + k]];
                float x = _bind[o], y = _bind[o + 1], z = _bind[o + 2];
                px += w * (m.M00 * x + m.M01 * y + m.M02 * z + m.M03);
                py += w * (m.M10 * x + m.M11 * y + m.M12 * z + m.M13);
                pz += w * (m.M20 * x + m.M21 * y + m.M22 * z + m.M23);
                float a = _bind[o + 3], b = _bind[o + 4], c = _bind[o + 5];
                nx += w * (m.M00 * a + m.M01 * b + m.M02 * c);
                ny += w * (m.M10 * a + m.M11 * b + m.M12 * c);
                nz += w * (m.M20 * a + m.M21 * b + m.M22 * c);
            }
            float l = MathF.Sqrt(nx * nx + ny * ny + nz * nz);
            if (l > 0) { nx /= l; ny /= l; nz /= l; }
            _out[o] = px; _out[o + 1] = py; _out[o + 2] = pz;
            _out[o + 3] = nx; _out[o + 4] = ny; _out[o + 5] = nz;
            _out[o + 6] = _bind[o + 6]; _out[o + 7] = _bind[o + 7];
        }
        var g = Owner.Geometry!;
        // a fresh array each time: the geometry keeps the reference
        var copy = new float[_out.Length];
        Array.Copy(_out, copy, copy.Length);
        g.Update(copy, g.Indices, g.Colors);
        Updates++;
    }

    /// <summary>A skinned vertex position (in the owner's space) from the last Apply.</summary>
    public Vector3 VertexPosition(int i) => new Vector3(_out[i * 8], _out[i * 8 + 1], _out[i * 8 + 2]);
}

public enum AnimationPath { Translation, Rotation, Scale }

/// <summary>Keyframes of one property of one node: 3 values per key (translation, scale) or 4 (rotation quaternion x y z w).</summary>
public sealed class AnimationChannel
{
    public readonly Node Target;
    public readonly AnimationPath Path;
    public readonly float[] Times;
    public readonly float[] Values;
    public readonly bool Step;

    public AnimationChannel(Node target, AnimationPath path, float[] times, float[] values, bool step = false)
    {
        int w = path == AnimationPath.Rotation ? 4 : 3;
        if (times.Length == 0 || values.Length != times.Length * w) throw new ArgumentException("one value per key");
        Target = target; Path = path; Times = times; Values = values; Step = step;
    }

    /// <summary>A rotation channel from Euler-angle keys (Node's YXZ convention).</summary>
    public static AnimationChannel RotationEuler(Node target, float[] times, Vector3[] keys)
    {
        var v = new float[keys.Length * 4];
        for (int i = 0; i < keys.Length; i++)
        {
            var q = Quat.FromEulerYXZ(keys[i]);
            v[i * 4] = q.X; v[i * 4 + 1] = q.Y; v[i * 4 + 2] = q.Z; v[i * 4 + 3] = q.W;
        }
        return new AnimationChannel(target, AnimationPath.Rotation, times, v);
    }

    public float Duration => Times[Times.Length - 1];

    public void Sample(float t)
    {
        int n = Times.Length, i = 0;
        while (i + 1 < n && Times[i + 1] <= t) i++;
        int j = i + 1 < n ? i + 1 : i;
        float f = 0;
        if (j != i && !Step && t > Times[i]) f = Math.Clamp((t - Times[i]) / (Times[j] - Times[i]), 0f, 1f);
        if (t < Times[0]) { i = j = 0; f = 0; }
        if (Path == AnimationPath.Rotation)
        {
            var a = new Quat(Values[i * 4], Values[i * 4 + 1], Values[i * 4 + 2], Values[i * 4 + 3]);
            var b = new Quat(Values[j * 4], Values[j * 4 + 1], Values[j * 4 + 2], Values[j * 4 + 3]);
            Target.EulerAngles = Quat.Slerp(a, b, f).ToEulerYXZ();
            return;
        }
        var v = new Vector3(Values[i * 3] + (Values[j * 3] - Values[i * 3]) * f,
            Values[i * 3 + 1] + (Values[j * 3 + 1] - Values[i * 3 + 1]) * f,
            Values[i * 3 + 2] + (Values[j * 3 + 2] - Values[i * 3 + 2]) * f);
        if (Path == AnimationPath.Translation) Target.Position = v; else Target.Scale = v;
    }
}

/// <summary>A named set of channels played together.</summary>
public sealed class AnimationClip
{
    public string Name;
    public readonly List<AnimationChannel> Channels = new List<AnimationChannel>();
    public AnimationClip(string name = "") { Name = name; }

    public float Duration
    {
        get { float d = 0; foreach (var c in Channels) if (c.Duration > d) d = c.Duration; return d; }
    }

    public void Sample(float t) { foreach (var c in Channels) c.Sample(t); }
}

/// <summary>Plays one clip: advanced by Scene.Update through the node it is attached to.</summary>
public sealed class Animator
{
    public AnimationClip Clip;
    public float Time;
    public float Speed = 1;
    public bool Loop = true;
    public bool Playing = true;

    public Animator(AnimationClip clip) { Clip = clip; }

    /// <summary>Drive this animator from `host`'s update (Scene.Update).</summary>
    public Animator AttachTo(Node host)
    {
        host.Updating += (_, dt) => Advance(dt);
        return this;
    }

    public void Advance(float dt)
    {
        if (!Playing) return;
        float d = Clip.Duration;
        Time += dt * Speed;
        if (d > 0)
        {
            if (Loop) { Time %= d; if (Time < 0) Time += d; }
            else if (Time > d) { Time = d; Playing = false; }
        }
        Clip.Sample(Time);
    }
}

public sealed partial class Node
{
    /// <summary>The skin deforming this node's geometry, if any.</summary>
    public SkinnedMesh? Skinned;
}

public sealed partial class Scene
{
    /// <summary>Animation clips loaded with the scene's models (glTF animations).</summary>
    public readonly List<AnimationClip> Animations = new List<AnimationClip>();

    /// <summary>Deform every skinned mesh for the current pose (Renderer.Render calls this after the world transforms).</summary>
    public int ApplySkins()
    {
        int n = 0;
        Root.Traverse(node => { if (node.Skinned != null && node.Visible) { node.Skinned.Apply(); n++; } });
        return n;
    }
}
