// Buitenzorg.Three engine parts: rigid-body physics and positional (3D) audio.
//
// Physics: spheres and boxes (axis-aligned, they do not rotate from contacts)
// under gravity, against static ground planes and static boxes and each
// other; restitution (bounce) and friction; fixed sub-steps; resting bodies
// fall asleep. Each body moves the Node it is attached to. Not a full
// solver: no rotation from collisions, no stacking-stability guarantees,
// no joints -- enough for falling fruit, bouncing balls, pushing crates.
//
// Audio3D: sounds placed in the scene; each is panned (equal power) and
// attenuated by its distance from the listener (usually the camera), mixed
// in C# and streamed into this program's mixer channel. Silent where the
// card cannot stream (Audio.CanStream false) -- the gains are still computed.

using System;
using System.Collections.Generic;
using Buitenzorg.Graphics3D;

namespace Buitenzorg.Three;

public enum BodyShape { Sphere, Box }

public sealed class RigidBody
{
    public Node Node;
    public BodyShape Shape;
    /// <summary>Sphere radius, or a box's half extents.</summary>
    public float Radius;
    public Vector3 HalfSize;
    public Vector3 Velocity;
    public float Mass = 1;
    /// <summary>0 = no bounce, 1 = perfectly elastic.</summary>
    public float Restitution = 0.4f;
    public float Friction = 0.4f;
    public bool Static;
    public bool Sleeping;
    /// <summary>Collisions this step (game logic: sounds, scores).</summary>
    public int Contacts;
    float _still;

    public RigidBody(Node node, BodyShape shape) { Node = node; Shape = shape; }

    public static RigidBody Sphere(Node node, float radius, float mass = 1) => new RigidBody(node, BodyShape.Sphere) { Radius = radius, Mass = mass };
    public static RigidBody Box(Node node, Vector3 halfSize, float mass = 1, bool isStatic = false) =>
        new RigidBody(node, BodyShape.Box) { HalfSize = halfSize, Mass = mass, Static = isStatic };

    public Vector3 Position { get => Node.Position; set => Node.Position = value; }
    public float InvMass => Static || Mass <= 0 ? 0 : 1 / Mass;

    public void Push(Vector3 impulse) { if (!Static) { Velocity = Velocity + impulse * InvMass; Sleeping = false; _still = 0; } }

    internal void Rest(float dt)
    {
        if (Velocity.Length() < 0.05f) { _still += dt; if (_still > 0.5f) { Sleeping = true; Velocity = Vector3.Zero; } }
        else _still = 0;
    }
    internal void Wake() { Sleeping = false; _still = 0; }
}

/// <summary>An infinite static plane: points p with dot(n, p) = d are on it.</summary>
public struct Plane
{
    public Vector3 Normal;
    public float D;
    public Plane(Vector3 normal, float d) { Normal = Vector3.Normalize(normal); D = d; }
    public static Plane Ground(float y = 0) => new Plane(new Vector3(0, 1, 0), y);
}

public sealed class PhysicsWorld
{
    public Vector3 Gravity = new Vector3(0, -9.8f, 0);
    public readonly List<RigidBody> Bodies = new List<RigidBody>();
    public readonly List<Plane> Planes = new List<Plane>();
    /// <summary>Sub-steps per Step call (stability for fast bodies).</summary>
    public int SubSteps = 4;
    /// <summary>Called for each new contact between two bodies (b may be null for a plane).</summary>
    public event Action<RigidBody, RigidBody?, float>? Collided;

    public RigidBody Add(RigidBody b) { Bodies.Add(b); return b; }

    public void Step(float dt)
    {
        float h = dt / Math.Max(1, SubSteps);
        foreach (var b in Bodies) b.Contacts = 0;
        for (int s = 0; s < SubSteps; s++)
        {
            foreach (var b in Bodies)
            {
                if (b.Static || b.Sleeping) continue;
                b.Velocity = b.Velocity + Gravity * h;
                b.Position = b.Position + b.Velocity * h;
            }
            foreach (var b in Bodies)
            {
                if (b.Static || b.Sleeping) continue;
                foreach (var p in Planes) PlaneContact(b, p);
            }
            for (int i = 0; i < Bodies.Count; i++)
                for (int j = i + 1; j < Bodies.Count; j++)
                {
                    var a = Bodies[i]; var b = Bodies[j];
                    if ((a.Static || a.Sleeping) && (b.Static || b.Sleeping)) continue;
                    PairContact(a, b);
                }
        }
        foreach (var b in Bodies) if (!b.Static && !b.Sleeping) b.Rest(dt);
    }

    float Support(RigidBody b, Vector3 n) =>
        b.Shape == BodyShape.Sphere ? b.Radius : MathF.Abs(n.X) * b.HalfSize.X + MathF.Abs(n.Y) * b.HalfSize.Y + MathF.Abs(n.Z) * b.HalfSize.Z;

    void PlaneContact(RigidBody b, Plane p)
    {
        float dist = Vector3.Dot(p.Normal, b.Position) - p.D - Support(b, p.Normal);
        if (dist >= 0) return;
        b.Position = b.Position - p.Normal * dist;
        float vn = Vector3.Dot(b.Velocity, p.Normal);
        if (vn < 0)
        {
            Resolve(b, null, p.Normal, vn, b.Restitution, b.Friction);
            if (-vn > 0.3f) Collided?.Invoke(b, null, -vn);
        }
    }

    void PairContact(RigidBody a, RigidBody b)
    {
        // contact normal n points from a to b; depth > 0 = overlap
        Vector3 n; float depth;
        if (a.Shape == BodyShape.Sphere && b.Shape == BodyShape.Sphere)
        {
            var d = b.Position - a.Position;
            float len = d.Length(), r = a.Radius + b.Radius;
            if (len >= r) return;
            n = len > 1e-5f ? d * (1 / len) : new Vector3(0, 1, 0);
            depth = r - len;
        }
        else if (a.Shape == BodyShape.Box && b.Shape == BodyShape.Box)
        {
            var d = b.Position - a.Position;
            float ox = a.HalfSize.X + b.HalfSize.X - MathF.Abs(d.X), oy = a.HalfSize.Y + b.HalfSize.Y - MathF.Abs(d.Y), oz = a.HalfSize.Z + b.HalfSize.Z - MathF.Abs(d.Z);
            if (ox <= 0 || oy <= 0 || oz <= 0) return;
            if (ox < oy && ox < oz) { n = new Vector3(MathF.Sign(d.X) == 0 ? 1 : MathF.Sign(d.X), 0, 0); depth = ox; }
            else if (oy < oz) { n = new Vector3(0, MathF.Sign(d.Y) == 0 ? 1 : MathF.Sign(d.Y), 0); depth = oy; }
            else { n = new Vector3(0, 0, MathF.Sign(d.Z) == 0 ? 1 : MathF.Sign(d.Z)); depth = oz; }
        }
        else
        {
            // sphere against box: the closest point of the box to the centre
            bool sphereFirst = a.Shape == BodyShape.Sphere;
            var s = sphereFirst ? a : b; var bx = sphereFirst ? b : a;
            var rel = s.Position - bx.Position;
            var cl = new Vector3(Math.Clamp(rel.X, -bx.HalfSize.X, bx.HalfSize.X), Math.Clamp(rel.Y, -bx.HalfSize.Y, bx.HalfSize.Y), Math.Clamp(rel.Z, -bx.HalfSize.Z, bx.HalfSize.Z));
            var d = rel - cl;
            float len = d.Length();
            if (len >= s.Radius) return;
            Vector3 outward; // from the box towards the sphere
            if (len > 1e-5f) { outward = d * (1 / len); depth = s.Radius - len; }
            else
            {
                // centre inside the box: push out along the shallowest axis
                float px = bx.HalfSize.X - MathF.Abs(rel.X), py = bx.HalfSize.Y - MathF.Abs(rel.Y), pz = bx.HalfSize.Z - MathF.Abs(rel.Z);
                if (px < py && px < pz) { outward = new Vector3(rel.X < 0 ? -1 : 1, 0, 0); depth = px + s.Radius; }
                else if (py < pz) { outward = new Vector3(0, rel.Y < 0 ? -1 : 1, 0); depth = py + s.Radius; }
                else { outward = new Vector3(0, 0, rel.Z < 0 ? -1 : 1); depth = pz + s.Radius; }
            }
            n = sphereFirst ? outward * -1 : outward;
        }
        float ia = a.InvMass, ib = b.InvMass, sum = ia + ib;
        if (sum <= 0) return;
        a.Position = a.Position - n * (depth * ia / sum);
        b.Position = b.Position + n * (depth * ib / sum);
        float vn = Vector3.Dot(b.Velocity - a.Velocity, n);
        if (vn >= 0) return;
        a.Wake(); b.Wake();
        a.Contacts++; b.Contacts++;
        float e = MathF.Min(a.Restitution, b.Restitution);
        float jn = -(1 + e) * vn / sum;
        a.Velocity = a.Velocity - n * (jn * ia);
        b.Velocity = b.Velocity + n * (jn * ib);
        // friction along the sliding direction
        var rv = b.Velocity - a.Velocity;
        var tangent = rv - n * Vector3.Dot(rv, n);
        float tl = tangent.Length();
        if (tl > 1e-4f)
        {
            tangent = tangent * (1 / tl);
            float jt = MathF.Min(tl / sum, jn * MathF.Max(a.Friction, b.Friction));
            a.Velocity = a.Velocity + tangent * (jt * ia);
            b.Velocity = b.Velocity - tangent * (jt * ib);
        }
        if (-vn > 0.3f) Collided?.Invoke(a, b, -vn);
    }

    static void Resolve(RigidBody b, RigidBody? _, Vector3 n, float vn, float e, float mu)
    {
        b.Contacts++;
        var v = b.Velocity - n * ((1 + (-vn > 0.5f ? e : 0)) * vn); // tiny bounces die out: resting contact
        var tangent = v - n * Vector3.Dot(v, n);
        float tl = tangent.Length();
        float drop = mu * (1 + e) * -vn;
        if (tl > 0) v = v - tangent * (MathF.Min(drop, tl) / tl);
        b.Velocity = v;
    }
}

/// <summary>A sound placed in the scene.</summary>
public sealed class Sound3D
{
    internal short[] Samples = Array.Empty<short>();
    internal int Rate;
    internal double Pos;
    public Vector3 Position;
    public Node? Follow;
    public bool Loop;
    public float Volume = 1;
    public bool Done { get; internal set; }
    /// <summary>The gains of the last mix (tests, meters).</summary>
    public float GainLeft { get; internal set; }
    public float GainRight { get; internal set; }
}

public sealed class Audio3D
{
    const int OutRate = 22050;
    readonly List<Sound3D> _voices = new List<Sound3D>();
    /// <summary>The listener: position and facing (its -Z), usually the camera node.</summary>
    public Node? Listener;
    /// <summary>Full volume within this distance, then 1/distance.</summary>
    public float ReferenceDistance = 2;
    public float MaxDistance = 40;
    public int Playing => _voices.Count;

    /// <summary>Start a mono sound (16-bit samples at `rate`) at a point.</summary>
    public Sound3D Play(short[] mono, int rate, Vector3 at, bool loop = false, float volume = 1)
    {
        var s = new Sound3D { Samples = mono, Rate = rate, Position = at, Loop = loop, Volume = volume };
        _voices.Add(s);
        return s;
    }

    /// <summary>A short synthesized tone (game effects without files).</summary>
    public static short[] Tone(int hz, int ms, int rate = OutRate, float decay = 6)
    {
        int n = rate * ms / 1000;
        var o = new short[n];
        for (int i = 0; i < n; i++)
        {
            float t = i / (float)rate;
            o[i] = (short)(MathF.Sin(2 * MathF.PI * hz * t) * MathF.Exp(-decay * t) * 12000);
        }
        return o;
    }

    /// <summary>Left / right gains of a source at `p` for the current listener.</summary>
    public (float, float) Gains(Vector3 p, float volume = 1)
    {
        var lp = Listener?.WorldPosition ?? Vector3.Zero;
        var m = Listener?.WorldMatrix ?? Matrix4.Identity;
        var right = Vector3.Normalize(m.TransformDirection(new Vector3(1, 0, 0)));
        var d = p - lp;
        float dist = d.Length();
        float att = dist <= ReferenceDistance ? 1 : dist >= MaxDistance ? 0 : ReferenceDistance / dist;
        float pan = dist > 1e-4f ? Math.Clamp(Vector3.Dot(d * (1 / dist), right), -1f, 1f) : 0; // -1 left .. 1 right
        float ang = (pan + 1) * MathF.PI / 4; // equal-power pan
        return (MathF.Cos(ang) * att * volume, MathF.Sin(ang) * att * volume);
    }

    /// <summary>Mix ahead while less than ~120 ms is queued; call every frame.</summary>
    public void Update()
    {
        if (_voices.Count == 0) return;
        int queued = Buitenzorg.App.Audio.QueuedMs;
        if (queued > 120) return;
        int frames = OutRate * 60 / 1000;
        var mix = new int[frames * 2];
        for (int v = _voices.Count - 1; v >= 0; v--)
        {
            var s = _voices[v];
            if (s.Follow != null) s.Position = s.Follow.WorldPosition;
            var (gl, gr) = Gains(s.Position, s.Volume);
            s.GainLeft = gl; s.GainRight = gr;
            double stepv = s.Rate / (double)OutRate;
            for (int i = 0; i < frames; i++)
            {
                int k = (int)s.Pos;
                if (k >= s.Samples.Length)
                {
                    if (s.Loop && s.Samples.Length > 0) { s.Pos = 0; k = 0; }
                    else { s.Done = true; break; }
                }
                int x = s.Samples[k];
                mix[i * 2] += (int)(x * gl);
                mix[i * 2 + 1] += (int)(x * gr);
                s.Pos += stepv;
            }
            if (s.Done) _voices.RemoveAt(v);
        }
        var bytes = new byte[frames * 4];
        for (int i = 0; i < frames * 2; i++)
        {
            int x = Math.Clamp(mix[i], -32768, 32767);
            bytes[i * 2] = (byte)x;
            bytes[i * 2 + 1] = (byte)(x >> 8);
        }
        Buitenzorg.App.Audio.Write(bytes, 0, bytes.Length, Buitenzorg.App.Audio.Format(OutRate, 2, 16));
    }

    public void StopAll() => _voices.Clear();
}
