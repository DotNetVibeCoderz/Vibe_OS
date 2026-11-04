# 3D graphics (G3)

Buitenzorg apps draw 3D scenes through two managed libraries on top of one
kernel API:

| Layer | Where | What |
|---|---|---|
| `Buitenzorg.Three` | `/disk/BZTHREE.DLL` | Scene graph in the style of [ThreeNet](../../../Vibe_Graphics/ThreeNet) / three.js: `Scene`, `Node`, `Geometry`, `Material`, `Light`, `Camera`, `Renderer`, raycasting |
| `Buitenzorg.Graphics3D` | in `/disk/BZAPP.DLL` | Low level: `Device3D` (render context), `Mesh3D`, `Texture3D`, `Matrix4`, `Vector3`, `Lighting` |
| `GFX3D` syscall (56) | kernel `gfx3d.rs` | Vertex stage (transform, lighting, fog) + one of two backends |

The kernel picks the backend per render context:

* **Host GPU (virgl)** when the machine has a virtio GPU with 3D (QEMU
  `virtio-gpu-gl`): rasterization, depth, texturing and blending run on the
  host's OpenGL through virglrenderer (`virtio/virgl.rs`).
* **Software rasterizer** everywhere else -- every architecture, the
  Raspberry Pi included (its V3D is detected and identified, but rendering on
  it is not implemented yet).

The results are the same; `Device3D.Backend` says which one draws.

## A scene in a few lines

```csharp
using Buitenzorg.Three;
using Vector3 = Buitenzorg.Graphics3D.Vector3;

var scene = new Scene { Background = 0x101820 };
var box = scene.AddMesh(Geometry.Box(1, 1, 1), Material.Phong(0xE3615A, 48));
box.Spin(1.2f);                                   // a behaviour, run by scene.Update(dt)
var sun = scene.AddLight(Light.Directional(0xFFFFFF, 0.8f));
sun.Position = new Vector3(3, 5, 4);
sun.LookAt(Vector3.Zero);                         // lights and cameras face -Z
var camera = scene.AddCamera(Camera.Perspective(0.9f), new Vector3(0, 1.5f, 4));
camera.LookAt(Vector3.Zero);

var renderer = new Renderer(480, 320);
scene.Update(0.016f);
renderer.Render(scene, camera);
renderer.PresentTo(window.Id, 0, 0);              // then window.Present()
```

In a `Buitenzorg.UI` app, draw from a control's `Render` with
`dc.Present3D(renderer.Device, Bounds.X, Bounds.Y)` -- see
`userland/clr-apps/MRuang` (Ruang 3D) and `userland/clr-apps/MBuah`
(Panen Buah 3D, a small game).

## What `Buitenzorg.Three` has

* **Geometry**: `Box`, `Plane` (XY, facing +Z; rotate -90 degrees about X for
  a floor), `Sphere`, `Cylinder`, `Cone`, `Torus`, `FromObj(text)` (Wavefront
  OBJ: v / vt / vn / f, fans, negative indices; normals computed when the
  file has none), `ComputeNormals`, `WithColor`, `Update`.
* **Material**: `Basic` (unlit), `Lambert`, `Phong` (`Shininess`,
  `Specular`), `Map` (a `Texture`), `Opacity` + `Transparent`, `Wireframe`,
  `DoubleSided`, `DepthTest` / `DepthWrite`.
* **Texture**: `Checker`, `Grid`, `FromBmp(bytes)`, or any `int[]` of
  0xAARRGGBB (alpha 0 cuts the texel out).
* **Light**: `Directional` (along the node's -Z), `Point` (`Range` fade),
  `Ambient`; up to 8 directional/point lights per frame.
* **Camera**: `Perspective(fovY, near, far)`, `Orthographic(height, ...)`.
* **Scene**: `Background`, `AmbientColor/Intensity`, linear fog
  (`FogColor`, `FogStart`, `FogEnd`), `FindByName`, `Update(dt)`,
  `Raycast(ray)` / `RaycastFirst`.
* **Node**: `Position`, `EulerAngles` (Y, X, Z order), `Scale`, `Visible`,
  `Pickable`, `LookAt`, `Add` / `Remove`, `Updating` event, `Spin`.
* **Renderer**: `Render` (opaque objects, then transparent ones far to near),
  `PresentTo`, `ReadPixel`, `Project(scene, point, camera)`,
  `CreateCameraRay(scene, camera, x, y)`, `Pick(scene, camera, x, y)`.

Conventions: right-handed, +Y up, radians, colors 0xRRGGBB; matrices are
row-major and multiply column vectors; counter-clockwise triangles face the
viewer.

## Trying it on a host GPU (virgl)

```powershell
.\scripts\setup-virgl.ps1     # once: kernel\target\qemu-gl = QEMU + Mesa EGL (llvmpipe)
$env:GALLIUM_DRIVER = "llvmpipe"
kernel\target\qemu-gl\qemu-system-x86_64.exe -L "C:\Program Files\qemu\share" `
    -drive format=raw,file=dist\buitenzorg-bios.img -m 1G -smp 2 `
    -device virtio-gpu-gl-pci -display egl-headless -serial stdio
```

The boot log then shows `[virgl] 3D context up ...` and
`MILESTONE: GFX3D VIRGL OK`; `run mruang` draws on the GPU. The shell
command `gfx3d` shows the backend and resource use; `gfx3d sw` / `gfx3d gpu`
switch new contexts between software and GPU. `smoke-test.ps1` runs the
`[virgl]` step when `kernel\target\qemu-gl` exists.

## Limits

* Lighting is per vertex (Gouraud), not per pixel; textures are sampled
  nearest-neighbour with wrap-around.
* The kernel copies the picture into the window (`PRESENT`); with virgl the
  picture is read back from the GPU once per present.
* All 3D resources of all processes together are capped at 8 MiB of kernel
  memory (`gfx3d::BUDGET`).
* Raspberry Pi 4: V3D is powered on and identified (`MILESTONE: RPI4 V3D OK`
  on a board), rendering stays in software.
