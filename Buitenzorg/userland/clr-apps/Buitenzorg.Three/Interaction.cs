// Buitenzorg.Three -- interaction: rays from the camera, raycasting (picking)
// against meshes, and per-node update callbacks for animation and game logic.
// Names follow ThreeNet (Scene.Raycast, CreateCameraRay).

using System;
using System.Collections.Generic;
using Buitenzorg.Graphics3D;

namespace Buitenzorg.Three;

/// <summary>A ray: origin and unit direction (world space).</summary>
public struct Ray
{
    public Vector3 Origin, Direction;
    public Ray(Vector3 origin, Vector3 direction) { Origin = origin; Direction = Vector3.Normalize(direction); }
    public Vector3 At(float distance) => Origin + Direction * distance;
}

/// <summary>What a raycast hit.</summary>
public sealed class RaycastHit
{
    public Node Node = null!;
    public float Distance;
    public Vector3 Point;
    public int Triangle;
    public override string ToString() => Node + " @ " + Distance.ToString();
}

public sealed partial class Node
{
    /// <summary>Called by Scene.Update(dt) for this node: animation, behaviour, game logic.</summary>
    public event Action<Node, float>? Updating;

    /// <summary>False: raycasts pass through this node (helpers, effects).</summary>
    public bool Pickable = true;

    internal void RunUpdate(float dt)
    {
        Updating?.Invoke(this, dt);
        // a handler may add or remove children: walk a copy
        if (Children.Count == 0) return;
        var copy = Children.ToArray();
        for (int i = 0; i < copy.Length; i++) copy[i].RunUpdate(dt);
    }

    /// <summary>Spin around the Y axis at `radiansPerSecond` (a ready-made behaviour).</summary>
    public Node Spin(float radiansPerSecond)
    {
        Updating += (n, dt) => n.EulerAngles = new Vector3(n.EulerAngles.X, n.EulerAngles.Y + radiansPerSecond * dt, n.EulerAngles.Z);
        return this;
    }
}

public sealed partial class Scene
{
    /// <summary>Seconds of scene time (advanced by Update).</summary>
    public float Time { get; private set; }

    /// <summary>Advance animations and behaviours by `dt` seconds.</summary>
    public void Update(float dt)
    {
        Time += dt;
        Root.RunUpdate(dt);
    }

    /// <summary>All hits along `ray`, nearest first (visible, pickable meshes; front and back faces).</summary>
    public List<RaycastHit> Raycast(Ray ray, float maxDistance = float.MaxValue, int maxHits = 16)
    {
        UpdateWorldTransforms();
        var hits = new List<RaycastHit>();
        Collect(Root, ray, maxDistance, hits);
        hits.Sort((a, b) => a.Distance < b.Distance ? -1 : a.Distance > b.Distance ? 1 : 0);
        if (hits.Count > maxHits) hits.RemoveRange(maxHits, hits.Count - maxHits);
        return hits;
    }

    /// <summary>The nearest hit, or null.</summary>
    public RaycastHit? RaycastFirst(Ray ray, float maxDistance = float.MaxValue)
    {
        var h = Raycast(ray, maxDistance, 1);
        return h.Count > 0 ? h[0] : null;
    }

    static void Collect(Node n, Ray ray, float max, List<RaycastHit> hits)
    {
        if (!n.Visible) return;
        if (n.Pickable && n.Geometry != null && n.Geometry.TriangleCount > 0)
        {
            var hit = Intersect(n, ray, max);
            if (hit != null) hits.Add(hit);
        }
        for (int i = 0; i < n.Children.Count; i++) Collect(n.Children[i], ray, max, hits);
    }

    /// <summary>Ray against one mesh: bounding sphere first, then every triangle (Moller-Trumbore) in the node's space.</summary>
    static RaycastHit? Intersect(Node n, Ray ray, float max)
    {
        var g = n.Geometry!;
        var inv = n.WorldMatrix.InvertAffine();
        var o = inv.TransformPoint(ray.Origin);
        var d = inv.TransformDirection(ray.Direction);
        // bounding sphere around the local origin
        float b = Vector3.Dot(o, d), dd = Vector3.Dot(d, d), c = Vector3.Dot(o, o) - g.BoundingRadius * g.BoundingRadius;
        if (c > 0 && b > 0) return null;
        if (b * b - dd * c < 0) return null;
        var v = g.Vertices;
        var idx = g.Indices;
        float best = float.MaxValue;
        int bestTri = -1;
        for (int t = 0; t + 2 < idx.Length; t += 3)
        {
            int ia = idx[t] * 8, ib = idx[t + 1] * 8, ic = idx[t + 2] * 8;
            var p0 = new Vector3(v[ia], v[ia + 1], v[ia + 2]);
            var e1 = new Vector3(v[ib], v[ib + 1], v[ib + 2]) - p0;
            var e2 = new Vector3(v[ic], v[ic + 1], v[ic + 2]) - p0;
            var pv = Vector3.Cross(d, e2);
            float det = Vector3.Dot(e1, pv);
            if (det > -1e-9f && det < 1e-9f) continue;
            float inv_det = 1 / det;
            var tv = o - p0;
            float u = Vector3.Dot(tv, pv) * inv_det;
            if (u < 0 || u > 1) continue;
            var qv = Vector3.Cross(tv, e1);
            float w = Vector3.Dot(d, qv) * inv_det;
            if (w < 0 || u + w > 1) continue;
            float dist = Vector3.Dot(e2, qv) * inv_det; // in units of the local direction
            if (dist > 0 && dist < best) { best = dist; bestTri = t / 3; }
        }
        if (bestTri < 0) return null;
        var world = n.WorldMatrix.TransformPoint(o + d * best);
        float wd = Vector3.Distance(world, ray.Origin);
        if (wd > max) return null;
        return new RaycastHit { Node = n, Distance = wd, Point = world, Triangle = bestTri };
    }
}

public sealed partial class Renderer
{
    /// <summary>The ray from `camera` through picture pixel (x, y) -- for picking with the mouse.</summary>
    public Ray CreateCameraRay(Scene scene, Node camera, int x, int y)
    {
        scene.UpdateWorldTransforms();
        float ndcX = (x + 0.5f) / Width * 2 - 1, ndcY = 1 - (y + 0.5f) / Height * 2;
        var cam = camera.Camera ?? Camera.Perspective(1f);
        var world = camera.WorldMatrix;
        if (cam.IsOrthographic)
        {
            float a = cam.AspectRatio > 0 ? cam.AspectRatio : (float)Width / Height;
            var local = new Vector3(ndcX * cam.Height / 2 * a, ndcY * cam.Height / 2, 0);
            return new Ray(world.TransformPoint(local), world.TransformDirection(new Vector3(0, 0, -1)));
        }
        float aspect = cam.AspectRatio > 0 ? cam.AspectRatio : (float)Width / Height;
        float t = MathF.Tan(cam.FieldOfView / 2);
        var dir = new Vector3(ndcX * t * aspect, ndcY * t, -1);
        return new Ray(camera.WorldPosition, world.TransformDirection(dir));
    }

    /// <summary>The nearest object under picture pixel (x, y), or null.</summary>
    public RaycastHit? Pick(Scene scene, Node camera, int x, int y) =>
        scene.RaycastFirst(CreateCameraRay(scene, camera, x, y));
}
