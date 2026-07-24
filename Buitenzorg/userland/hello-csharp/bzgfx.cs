// Buitenzorg.Drawing (v0.16 "Panen") — a System.Drawing-complete 2D graphics
// library that renders CLIENT-SIDE into a managed pixel buffer (Bitmap) and
// blits the finished frame to a window with one syscall (the compositor model
// used by WPF/Avalonia). Built on the managed heap (v0.15), so it can allocate
// pixel buffers and use arbitrary rendering algorithms — unlike the old
// syscall-per-primitive Graphics in bzdraw.cs. No floats: integer math only.

using System;
using System.Runtime.InteropServices;

namespace Buitenzorg.Drawing
{
    /// <summary>32-bit ARGB color (0xAARRGGBB). System.Drawing.Color-style.</summary>
    public readonly struct Color
    {
        public readonly uint Argb;
        public Color(uint argb) { Argb = argb; }
        public static Color FromArgb(int a, int r, int g, int b)
            => new Color(((uint)(a & 255) << 24) | ((uint)(r & 255) << 16) | ((uint)(g & 255) << 8) | (uint)(b & 255));
        public static Color FromRgb(int r, int g, int b) => FromArgb(255, r, g, b);
        public int A => (int)((Argb >> 24) & 255);
        public int R => (int)((Argb >> 16) & 255);
        public int G => (int)((Argb >> 8) & 255);
        public int B => (int)(Argb & 255);
        public uint Rgb24 => Argb & 0xFF_FFFF;

        public static Color Black => FromRgb(0, 0, 0);
        public static Color White => FromRgb(255, 255, 255);
        public static Color Red => FromRgb(224, 72, 59);
        public static Color Green => FromRgb(79, 163, 63);
        public static Color Blue => FromRgb(74, 120, 232);
        public static Color Yellow => FromRgb(232, 184, 75);
        public static Color Orange => FromRgb(240, 140, 40);
        public static Color Cyan => FromRgb(60, 200, 200);
        public static Color Magenta => FromRgb(200, 60, 200);
        public static Color Gray => FromRgb(128, 128, 128);
        public static Color DarkGray => FromRgb(48, 48, 56);
        public static Color Transparent => new Color(0);
    }

    /// <summary>2D affine transform (like System.Drawing.Drawing2D.Matrix).
    /// Linear part a,b,c,d is fixed-point ×256; translation e,f is in pixels.</summary>
    public struct Matrix
    {
        public int A, B, C, D, E, F;
        public static Matrix Identity() { Matrix m; m.A = 256; m.B = 0; m.C = 0; m.D = 256; m.E = 0; m.F = 0; return m; }
        public void Apply(int x, int y, out int ox, out int oy)
        {
            ox = (A * x + C * y) / 256 + E;
            oy = (B * x + D * y) / 256 + F;
        }
        /// <summary>Returns this ∘ op (op applied first, then this).</summary>
        public Matrix Times(Matrix op)
        {
            Matrix r;
            r.A = (A * op.A + C * op.B) / 256;
            r.B = (B * op.A + D * op.B) / 256;
            r.C = (A * op.C + C * op.D) / 256;
            r.D = (B * op.C + D * op.D) / 256;
            r.E = (A * op.E + C * op.F) / 256 + E;
            r.F = (B * op.E + D * op.F) / 256 + F;
            return r;
        }
    }

    public readonly struct Point { public readonly int X, Y; public Point(int x, int y) { X = x; Y = y; } }
    public readonly struct Size { public readonly int Width, Height; public Size(int w, int h) { Width = w; Height = h; } }
    public readonly struct Rectangle
    {
        public readonly int X, Y, Width, Height;
        public Rectangle(int x, int y, int w, int h) { X = x; Y = y; Width = w; Height = h; }
        public int Right => X + Width;
        public int Bottom => Y + Height;
    }

    /// <summary>An ARGB pixel surface, like System.Drawing.Bitmap.</summary>
    public sealed class Bitmap
    {
        public readonly int Width, Height;
        public readonly uint[] Pixels; // ARGB
        public Bitmap(int w, int h) { Width = w; Height = h; Pixels = new uint[w * h]; }
        public void SetPixel(int x, int y, Color c) { if ((uint)x < (uint)Width && (uint)y < (uint)Height) Pixels[y * Width + x] = c.Argb; }
        public Color GetPixel(int x, int y) => new Color(Pixels[y * Width + x]);
        public void Clear(Color c) { uint v = c.Argb; for (int i = 0; i < Pixels.Length; i++) Pixels[i] = v; }
    }

    /// <summary>Renders shapes/text/images into a Bitmap, like Graphics.</summary>
    public sealed class Graphics
    {
        readonly int _w, _h;
        readonly uint[] _px;
        readonly Bitmap _b;
        public Graphics(Bitmap b) { _b = b; _w = b.Width; _h = b.Height; _px = b.Pixels; }

        static int Abs(int v) => v < 0 ? -v : v;
        static int Max(int a, int b) => a > b ? a : b;
        static int Min(int a, int b) => a < b ? a : b;
        static int ISqrt(long v)
        {
            if (v <= 0) return 0;
            long x = v, y = (x + 1) / 2;
            while (y < x) { x = y; y = (x + v / x) / 2; }
            return (int)x;
        }

        // Fixed-point (×256) sine/cosine via the Bhaskara approximation — no
        // floats needed. `deg` may be any integer.
        public static int SinFx(int deg)
        {
            deg = ((deg % 360) + 360) % 360;
            int sign = 1;
            if (deg >= 180) { deg -= 180; sign = -1; }
            int t = deg * (180 - deg);
            return sign * (256 * (4 * t) / (40500 - t));
        }
        public static int CosFx(int deg) => SinFx(deg + 90);

        // --- Current transform (like Graphics.Transform) ---------------------
        Matrix _t = Matrix.Identity();
        public void ResetTransform() => _t = Matrix.Identity();
        public void TranslateTransform(int tx, int ty)
        {
            Matrix op; op.A = 256; op.B = 0; op.C = 0; op.D = 256; op.E = tx; op.F = ty;
            _t = _t.Times(op);
        }
        /// <summary>Scale by pctX/100, pctY/100.</summary>
        public void ScaleTransform(int pctX, int pctY)
        {
            Matrix op; op.A = 256 * pctX / 100; op.B = 0; op.C = 0; op.D = 256 * pctY / 100; op.E = 0; op.F = 0;
            _t = _t.Times(op);
        }
        public void RotateTransform(int deg)
        {
            int co = CosFx(deg), si = SinFx(deg);
            Matrix op; op.A = co; op.B = si; op.C = -si; op.D = co; op.E = 0; op.F = 0;
            _t = _t.Times(op);
        }
        void TP(int x, int y, out int ox, out int oy) => _t.Apply(x, y, out ox, out oy);

        // --- Clipping (like Graphics.SetClip / Clip) --------------------------
        bool _clip;
        int _cx0, _cy0, _cx1, _cy1;
        public void SetClip(int x, int y, int w, int h) { _clip = true; _cx0 = x; _cy0 = y; _cx1 = x + w; _cy1 = y + h; }
        public void ResetClip() => _clip = false;
        void ClipRect(ref int x0, ref int y0, ref int x1, ref int y1)
        {
            if (!_clip) return;
            if (x0 < _cx0) x0 = _cx0; if (y0 < _cy0) y0 = _cy0;
            if (x1 > _cx1) x1 = _cx1; if (y1 > _cy1) y1 = _cy1;
        }

        /// <summary>Alpha-blend color c over the pixel at (x,y).</summary>
        void Blend(int x, int y, Color c)
        {
            if ((uint)x >= (uint)_w || (uint)y >= (uint)_h) return;
            if (_clip && (x < _cx0 || x >= _cx1 || y < _cy0 || y >= _cy1)) return;
            int a = c.A;
            int idx = y * _w + x;
            if (a >= 255) { _px[idx] = c.Argb; return; }
            if (a <= 0) return;
            uint d = _px[idx];
            int dr = (int)((d >> 16) & 255), dg = (int)((d >> 8) & 255), db = (int)(d & 255);
            int rr = (c.R * a + dr * (255 - a)) / 255;
            int rg = (c.G * a + dg * (255 - a)) / 255;
            int rb = (c.B * a + db * (255 - a)) / 255;
            _px[idx] = 0xFF00_0000u | ((uint)rr << 16) | ((uint)rg << 8) | (uint)rb;
        }

        /// <summary>Blend `c` scaled by a coverage 0..256 (for anti-aliasing).</summary>
        void BlendCov(int x, int y, Color c, int cov)
        {
            if (cov <= 0) return;
            if (cov > 256) cov = 256;
            int a = c.A * cov / 256;
            Blend(x, y, new Color(((uint)(a & 255) << 24) | (c.Argb & 0x00FF_FFFF)));
        }

        // sqrt(v) in 8.8 fixed point (result = sqrt(v) * 256).
        static int ISqrtF(long v) => ISqrt(v * 65536);

        public void Clear(Color c) => _b.Clear(c);

        public void FillRectangle(Color c, int x, int y, int w, int h)
        {
            int x0 = Max(0, x), y0 = Max(0, y), x1 = Min(_w, x + w), y1 = Min(_h, y + h);
            ClipRect(ref x0, ref y0, ref x1, ref y1);
            if (c.A >= 255)
            {
                uint v = c.Argb;
                for (int yy = y0; yy < y1; yy++) { int row = yy * _w; for (int xx = x0; xx < x1; xx++) _px[row + xx] = v; }
            }
            else for (int yy = y0; yy < y1; yy++) for (int xx = x0; xx < x1; xx++) Blend(xx, yy, c);
        }
        public void FillRectangle(Color c, Rectangle r) => FillRectangle(c, r.X, r.Y, r.Width, r.Height);

        void HLine(int x0, int x1, int y, Color c) { if (x0 > x1) { int t = x0; x0 = x1; x1 = t; } for (int x = x0; x <= x1; x++) Blend(x, y, c); }
        void VLine(int y0, int y1, int x, Color c) { if (y0 > y1) { int t = y0; y0 = y1; y1 = t; } for (int y = y0; y <= y1; y++) Blend(x, y, c); }

        public void DrawRectangle(Color c, int x, int y, int w, int h, int thick)
        {
            for (int t = 0; t < thick; t++)
            {
                HLine(x, x + w - 1, y + t, c); HLine(x, x + w - 1, y + h - 1 - t, c);
                VLine(y, y + h - 1, x + t, c); VLine(y, y + h - 1, x + w - 1 - t, c);
            }
        }
        public void DrawRectangle(Color c, int x, int y, int w, int h) => DrawRectangle(c, x, y, w, h, 1);

        void DrawLineRaw(Color c, int x0, int y0, int x1, int y1)
        {
            int dx = Abs(x1 - x0), dy = -Abs(y1 - y0);
            int sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1, err = dx + dy;
            while (true)
            {
                Blend(x0, y0, c);
                if (x0 == x1 && y0 == y1) break;
                int e2 = 2 * err;
                if (e2 >= dy) { err += dy; x0 += sx; }
                if (e2 <= dx) { err += dx; y0 += sy; }
            }
        }
        public void DrawLine(Color c, int x0, int y0, int x1, int y1)
        {
            TP(x0, y0, out int ax, out int ay); TP(x1, y1, out int bx, out int by);
            DrawLineRaw(c, ax, ay, bx, by);
        }
        public void DrawLine(Color c, int x0, int y0, int x1, int y1, int thick)
        {
            TP(x0, y0, out int ax, out int ay); TP(x1, y1, out int bx, out int by);
            if (thick <= 1) { DrawLineRaw(c, ax, ay, bx, by); return; }
            int half = thick / 2;
            bool steep = Abs(by - ay) > Abs(bx - ax);
            for (int o = -half; o <= half; o++)
                if (steep) DrawLineRaw(c, ax + o, ay, bx + o, by);
                else DrawLineRaw(c, ax, ay + o, bx, by + o);
        }

        public void FillCircle(Color c, int cx, int cy, int r)
        {
            for (int y = -r; y <= r; y++)
            {
                int span = ISqrt((long)r * r - (long)y * y);
                HLine(cx - span, cx + span, cy + y, c);
            }
        }
        public void DrawCircle(Color c, int cx, int cy, int r)
        {
            int x = r, y = 0, err = 0;
            while (x >= y)
            {
                Blend(cx + x, cy + y, c); Blend(cx - x, cy + y, c);
                Blend(cx + x, cy - y, c); Blend(cx - x, cy - y, c);
                Blend(cx + y, cy + x, c); Blend(cx - y, cy + x, c);
                Blend(cx + y, cy - x, c); Blend(cx - y, cy - x, c);
                y++;
                if (err <= 0) err += 2 * y + 1;
                if (err > 0) { x--; err -= 2 * x + 1; }
            }
        }
        public void FillEllipse(Color c, int x, int y, int w, int h)
        {
            int rx = w / 2, ry = h / 2, cx = x + rx, cy = y + ry;
            if (rx <= 0 || ry <= 0) return;
            for (int yy = -ry; yy <= ry; yy++)
            {
                long inner = (long)rx * rx - ((long)rx * rx * yy * yy) / ((long)ry * ry);
                int span = ISqrt(inner);
                HLine(cx - span, cx + span, cy + yy, c);
            }
        }

        /// <summary>Even-odd scanline polygon fill (respects the transform).</summary>
        public void FillPolygon(Color c, int[] xs, int[] ys, int n)
        {
            int[] tx = new int[n]; int[] ty = new int[n];
            for (int i = 0; i < n; i++) { TP(xs[i], ys[i], out int ox, out int oy); tx[i] = ox; ty[i] = oy; }
            FillPolyRaw(c, tx, ty, n);
        }
        void FillPolyRaw(Color c, int[] xs, int[] ys, int n)
        {
            int minY = ys[0], maxY = ys[0];
            for (int i = 1; i < n; i++) { if (ys[i] < minY) minY = ys[i]; if (ys[i] > maxY) maxY = ys[i]; }
            if (minY < 0) minY = 0;
            if (maxY >= _h) maxY = _h - 1;
            int[] xi = new int[n];
            for (int y = minY; y <= maxY; y++)
            {
                int cnt = 0, j = n - 1;
                for (int i = 0; i < n; i++)
                {
                    if ((ys[i] <= y && ys[j] > y) || (ys[j] <= y && ys[i] > y))
                        xi[cnt++] = xs[i] + (y - ys[i]) * (xs[j] - xs[i]) / (ys[j] - ys[i]);
                    j = i;
                }
                for (int a = 0; a < cnt - 1; a++) for (int b = a + 1; b < cnt; b++) if (xi[b] < xi[a]) { int t = xi[a]; xi[a] = xi[b]; xi[b] = t; }
                for (int k = 0; k + 1 < cnt; k += 2) HLine(xi[k], xi[k + 1], y, c);
            }
        }
        public void DrawPolygon(Color c, int[] xs, int[] ys, int n)
        {
            for (int i = 0; i < n; i++) DrawLine(c, xs[i], ys[i], xs[(i + 1) % n], ys[(i + 1) % n]);
        }

        /// <summary>Fill a GraphicsPath (respects the transform).</summary>
        public void FillPath(Color c, GraphicsPath p)
        {
            int n = p.Count;
            if (n < 3) return;
            int[] tx = new int[n]; int[] ty = new int[n];
            for (int i = 0; i < n; i++) { TP(p.PX(i), p.PY(i), out int ox, out int oy); tx[i] = ox; ty[i] = oy; }
            FillPolyRaw(c, tx, ty, n);
        }
        public void DrawPath(Color c, GraphicsPath p, bool close)
        {
            int n = p.Count;
            for (int i = 0; i + 1 < n; i++) DrawLine(c, p.PX(i), p.PY(i), p.PX(i + 1), p.PY(i + 1));
            if (close && n > 1) DrawLine(c, p.PX(n - 1), p.PY(n - 1), p.PX(0), p.PY(0));
        }

        /// <summary>Vertical linear gradient fill.</summary>
        public void FillGradientV(int x, int y, int w, int h, Color top, Color bottom)
        {
            int y0 = Max(0, y), y1 = Min(_h, y + h), x0 = Max(0, x), x1 = Min(_w, x + w);
            ClipRect(ref x0, ref y0, ref x1, ref y1);
            for (int yy = y0; yy < y1; yy++)
            {
                int t = h > 1 ? (yy - y) * 255 / (h - 1) : 0;
                if (t < 0) t = 0; if (t > 255) t = 255;
                int r = (top.R * (255 - t) + bottom.R * t) / 255;
                int g = (top.G * (255 - t) + bottom.G * t) / 255;
                int b = (top.B * (255 - t) + bottom.B * t) / 255;
                uint v = 0xFF00_0000u | ((uint)r << 16) | ((uint)g << 8) | (uint)b;
                int row = yy * _w;
                for (int xx = x0; xx < x1; xx++) _px[row + xx] = v;
            }
        }

        /// <summary>Horizontal linear gradient fill.</summary>
        public void FillGradientH(int x, int y, int w, int h, Color left, Color right)
        {
            int y0 = Max(0, y), y1 = Min(_h, y + h), x0 = Max(0, x), x1 = Min(_w, x + w);
            ClipRect(ref x0, ref y0, ref x1, ref y1);
            for (int xx = x0; xx < x1; xx++)
            {
                int t = w > 1 ? (xx - x) * 255 / (w - 1) : 0;
                if (t < 0) t = 0; if (t > 255) t = 255;
                int r = (left.R * (255 - t) + right.R * t) / 255;
                int g = (left.G * (255 - t) + right.G * t) / 255;
                int b = (left.B * (255 - t) + right.B * t) / 255;
                uint v = 0xFF00_0000u | ((uint)r << 16) | ((uint)g << 8) | (uint)b;
                for (int yy = y0; yy < y1; yy++) _px[yy * _w + xx] = v;
            }
        }
        /// <summary>Linear gradient, horizontal or vertical.</summary>
        public void FillGradient(int x, int y, int w, int h, Color a, Color b, bool horizontal)
        { if (horizontal) FillGradientH(x, y, w, h, a, b); else FillGradientV(x, y, w, h, a, b); }

        /// <summary>Filled rectangle with anti-aliased rounded corners.</summary>
        public void FillRoundedRectangle(Color c, int x, int y, int w, int h, int rad)
        {
            if (w <= 0 || h <= 0) return;
            if (rad < 0) rad = 0; int m = Min(w, h) / 2; if (rad > m) rad = m;
            for (int j = 0; j < h; j++)
            {
                int yy = y + j, inset = 0, frac = 0, vd = -1;
                if (j < rad) vd = rad - j; else if (j >= h - rad) vd = rad - (h - 1 - j);
                if (vd >= 0) { int spF = ISqrtF((long)rad * rad - (long)vd * vd); inset = rad - (spF >> 8); frac = spF & 255; }
                HLine(x + inset, x + w - 1 - inset, yy, c);
                if (vd >= 0 && frac > 0) { BlendCov(x + inset - 1, yy, c, frac); BlendCov(x + w - inset, yy, c, frac); }
            }
        }

        void Plot(Color c, int cx, int cy, int px, int py, int quad)
        {
            if (quad == 0) Blend(cx + px, cy + py, c);
            else if (quad == 1) Blend(cx + px, cy - py, c);
            else if (quad == 2) Blend(cx - px, cy - py, c);
            else Blend(cx - px, cy + py, c);
        }
        void ArcCorner(Color c, int cx, int cy, int r, int quad)
        {
            int x = r, y = 0, err = 0;
            while (x >= y)
            {
                Plot(c, cx, cy, x, y, quad); Plot(c, cx, cy, y, x, quad);
                y++; if (err <= 0) err += 2 * y + 1; if (err > 0) { x--; err -= 2 * x + 1; }
            }
        }
        /// <summary>Outline of a rounded rectangle (1px).</summary>
        public void DrawRoundedRectangle(Color c, int x, int y, int w, int h, int rad)
        {
            if (rad < 0) rad = 0; int m = Min(w, h) / 2; if (rad > m) rad = m;
            HLine(x + rad, x + w - 1 - rad, y, c); HLine(x + rad, x + w - 1 - rad, y + h - 1, c);
            VLine(y + rad, y + h - 1 - rad, x, c); VLine(y + rad, y + h - 1 - rad, x + w - 1, c);
            ArcCorner(c, x + w - 1 - rad, y + h - 1 - rad, rad, 0); // BR
            ArcCorner(c, x + w - 1 - rad, y + rad, rad, 1);         // TR
            ArcCorner(c, x + rad, y + rad, rad, 2);                 // TL
            ArcCorner(c, x + rad, y + h - 1 - rad, rad, 3);         // BL
        }

        /// <summary>Rounded rectangle filled with a vertical gradient (nice for buttons).</summary>
        public void FillRoundedGradientV(int x, int y, int w, int h, int rad, Color top, Color bottom)
        {
            if (w <= 0 || h <= 0) return;
            if (rad < 0) rad = 0; int m = Min(w, h) / 2; if (rad > m) rad = m;
            for (int j = 0; j < h; j++)
            {
                int yy = y + j, t = h > 1 ? j * 255 / (h - 1) : 0;
                int r = (top.R * (255 - t) + bottom.R * t) / 255;
                int g = (top.G * (255 - t) + bottom.G * t) / 255;
                int b = (top.B * (255 - t) + bottom.B * t) / 255;
                Color c = Color.FromRgb(r, g, b);
                int inset = 0, frac = 0, vd = -1;
                if (j < rad) vd = rad - j; else if (j >= h - rad) vd = rad - (h - 1 - j);
                if (vd >= 0) { int spF = ISqrtF((long)rad * rad - (long)vd * vd); inset = rad - (spF >> 8); frac = spF & 255; }
                HLine(x + inset, x + w - 1 - inset, yy, c);
                if (vd >= 0 && frac > 0) { BlendCov(x + inset - 1, yy, c, frac); BlendCov(x + w - inset, yy, c, frac); }
            }
        }

        /// <summary>Soft drop shadow: expanding translucent rounded layers (draw before the shape).</summary>
        public void DrawShadow(int x, int y, int w, int h, int rad, int spread, int alpha)
        {
            if (spread < 1) spread = 1;
            int a = alpha / spread; if (a < 1) a = 1;
            Color sc = new Color((uint)(a & 255) << 24); // translucent black
            for (int i = spread; i >= 1; i--) FillRoundedRectangle(sc, x - i, y - i, w + 2 * i, h + 2 * i, rad + i);
        }

        /// <summary>Anti-aliased filled circle (smooth left/right edges).</summary>
        public void FillCircleAA(Color c, int cx, int cy, int r)
        {
            if (r <= 0) return;
            for (int dy = -r; dy <= r; dy++)
            {
                long inner = (long)r * r - (long)dy * dy;
                if (inner < 0) continue;
                int spF = ISqrtF(inner), spI = spF >> 8, frac = spF & 255, yy = cy + dy;
                HLine(cx - spI, cx + spI, yy, c);
                if (frac > 0) { BlendCov(cx - spI - 1, yy, c, frac); BlendCov(cx + spI + 1, yy, c, frac); }
            }
        }

        /// <summary>Blit another bitmap (with per-pixel alpha) at (dx,dy).</summary>
        public void DrawImage(Bitmap img, int dx, int dy)
        {
            for (int y = 0; y < img.Height; y++)
            {
                int py = dy + y;
                for (int x = 0; x < img.Width; x++)
                    Blend(dx + x, py, new Color(img.Pixels[y * img.Width + x]));
            }
        }

        /// <summary>Blit a bitmap scaled to (dw,dh) via nearest-neighbor.</summary>
        public void DrawImageScaled(Bitmap img, int dx, int dy, int dw, int dh)
        {
            if (dw <= 0 || dh <= 0) return;
            for (int y = 0; y < dh; y++)
            {
                int sy = y * img.Height / dh;
                for (int x = 0; x < dw; x++)
                {
                    int sx = x * img.Width / dw;
                    Blend(dx + x, dy + y, new Color(img.Pixels[sy * img.Width + sx]));
                }
            }
        }

        // Hatch styles for FillHatch.
        public const int HatchHorizontal = 0, HatchVertical = 1, HatchCross = 2,
                         HatchForward = 3, HatchBackward = 4, HatchDots = 5;

        /// <summary>Fill a rectangle with a hatch pattern (like HatchBrush).</summary>
        public void FillHatch(Color c, int x, int y, int w, int h, int style, int spacing)
        {
            if (spacing < 1) spacing = 1;
            for (int yy = y; yy < y + h; yy++)
            {
                for (int xx = x; xx < x + w; xx++)
                {
                    bool on;
                    switch (style)
                    {
                        case HatchVertical: on = (xx % spacing) == 0; break;
                        case HatchCross: on = (xx % spacing) == 0 || (yy % spacing) == 0; break;
                        case HatchForward: on = ((xx + yy) % spacing) == 0; break;
                        case HatchBackward: on = ((xx - yy) % spacing == 0) || ((xx - yy + spacing) % spacing == 0); break;
                        case HatchDots: on = (xx % spacing) == 0 && (yy % spacing) == 0; break;
                        default: on = (yy % spacing) == 0; break; // horizontal
                    }
                    if (on) Blend(xx, yy, c);
                }
            }
        }

        /// <summary>Draw a string with the given font (respects transform origin +
        /// clip). Text pixels are axis-aligned from the transformed origin.</summary>
        public void DrawString(Font f, string s, Color c, int x, int y)
        {
            TP(x, y, out int ox, out int oy);
            int cx = ox;
            for (int i = 0; i < s.Length; i++)
            {
                char ch = s[i];
                for (int r = 0; r < 8; r++)
                {
                    byte bits = f.Row(ch, r);
                    if (bits == 0) continue;
                    for (int col = 0; col < 8; col++)
                        if (((bits >> (7 - col)) & 1) != 0) Blend(cx + col, oy + r, c);
                }
                cx += f.CharW;
            }
        }

        /// <summary>Draw `len` chars from a buffer (no managed string needed —
        /// useful for dynamic numeric text under zerolib).</summary>
        public void DrawChars(Font f, char[] s, int len, Color c, int x, int y)
        {
            TP(x, y, out int ox, out int oy);
            int cx = ox;
            for (int i = 0; i < len; i++)
            {
                char ch = s[i];
                for (int r = 0; r < 8; r++)
                {
                    byte bits = f.Row(ch, r);
                    if (bits == 0) continue;
                    for (int col = 0; col < 8; col++)
                        if (((bits >> (7 - col)) & 1) != 0) Blend(cx + col, oy + r, c);
                }
                cx += f.CharW;
            }
        }
    }

    /// <summary>A sequence of points forming a figure, like GraphicsPath.</summary>
    public sealed class GraphicsPath
    {
        int[] _xs, _ys;
        int _n;
        public GraphicsPath() { _xs = new int[8]; _ys = new int[8]; _n = 0; }
        void Add(int x, int y)
        {
            if (_n == _xs.Length)
            {
                int[] nx = new int[_n * 2]; int[] ny = new int[_n * 2];
                for (int i = 0; i < _n; i++) { nx[i] = _xs[i]; ny[i] = _ys[i]; }
                _xs = nx; _ys = ny;
            }
            _xs[_n] = x; _ys[_n] = y; _n++;
        }
        public void MoveTo(int x, int y) => Add(x, y);
        public void LineTo(int x, int y) => Add(x, y);
        public void AddRectangle(int x, int y, int w, int h)
        {
            Add(x, y); Add(x + w, y); Add(x + w, y + h); Add(x, y + h);
        }
        public void AddEllipse(int x, int y, int w, int h, int segments)
        {
            int rx = w / 2, ry = h / 2, cx = x + rx, cy = y + ry;
            for (int i = 0; i < segments; i++)
            {
                int d = i * 360 / segments;
                Add(cx + rx * Graphics.CosFx(d) / 256, cy + ry * Graphics.SinFx(d) / 256);
            }
        }
        public int Count => _n;
        public int PX(int i) => _xs[i];
        public int PY(int i) => _ys[i];
    }

    /// <summary>24-bit BMP encode/decode (like Image.Save/FromStream for BMP).</summary>
    public static class Bmp
    {
        static void PutI32(byte[] o, int i, int v) { o[i] = (byte)v; o[i + 1] = (byte)(v >> 8); o[i + 2] = (byte)(v >> 16); o[i + 3] = (byte)(v >> 24); }
        static int GetI32(byte[] o, int i) => o[i] | (o[i + 1] << 8) | (o[i + 2] << 16) | (o[i + 3] << 24);

        public static byte[] Save(Bitmap b)
        {
            int w = b.Width, h = b.Height;
            int rowBytes = ((w * 3 + 3) / 4) * 4;
            int dataSize = rowBytes * h;
            byte[] o = new byte[54 + dataSize];
            o[0] = (byte)'B'; o[1] = (byte)'M';
            PutI32(o, 2, 54 + dataSize);
            PutI32(o, 10, 54);
            PutI32(o, 14, 40);
            PutI32(o, 18, w);
            PutI32(o, 22, h);
            o[26] = 1; o[28] = 24;
            PutI32(o, 34, dataSize);
            for (int y = 0; y < h; y++)
            {
                int srcY = h - 1 - y; // BMP is bottom-up
                int off = 54 + y * rowBytes;
                for (int x = 0; x < w; x++)
                {
                    uint p = b.Pixels[srcY * w + x];
                    o[off + x * 3] = (byte)(p & 255);         // B
                    o[off + x * 3 + 1] = (byte)((p >> 8) & 255);   // G
                    o[off + x * 3 + 2] = (byte)((p >> 16) & 255);  // R
                }
            }
            return o;
        }

        public static Bitmap Load(byte[] o)
        {
            int w = GetI32(o, 18), h = GetI32(o, 22), dataOff = GetI32(o, 10);
            int rowBytes = ((w * 3 + 3) / 4) * 4;
            Bitmap b = new Bitmap(w, h);
            for (int y = 0; y < h; y++)
            {
                int dstY = h - 1 - y;
                int off = dataOff + y * rowBytes;
                for (int x = 0; x < w; x++)
                {
                    int bl = o[off + x * 3], g = o[off + x * 3 + 1], r = o[off + x * 3 + 2];
                    b.Pixels[dstY * w + x] = 0xFF00_0000u | ((uint)r << 16) | ((uint)g << 8) | (uint)bl;
                }
            }
            return b;
        }
    }

    /// <summary>Baseline JPEG decoder (JFIF, SOF0 sequential DCT only). Handles
    /// grayscale and YCbCr (4:4:4 / 4:2:2 / 4:2:0 chroma subsampling), Huffman
    /// entropy coding, restart markers, and an integer (float-free) inverse DCT.
    /// Progressive JPEG (SOF2), arithmetic coding, and CMYK are not supported —
    /// <see cref="Load"/> returns null for those. Enough to open typical camera
    /// / web baseline .jpg files in the Image Viewer or as wallpaper.
    ///
    /// All state is flat value-type arrays: zerolib faults on storing a reference
    /// into an object-array element (stelem.ref), so no jagged arrays / arrays of
    /// class instances (Huffman + quant tables + per-component planes are packed
    /// into single int[]/byte[] buffers with per-table offsets).</summary>
    public sealed unsafe class Jpeg
    {
        // Fundamental cosines cos(k*pi/16) * 8192, k = 0..8 (last = 0).
        readonly int[] _cos = { 8192, 8035, 7568, 6811, 5793, 4551, 3135, 1598, 0 };
        // Natural (row-major) order each zig-zag index maps to.
        readonly int[] _zig = {
            0,1,8,16,9,2,3,10,17,24,32,25,18,11,4,5,12,19,26,33,40,48,41,34,27,20,13,6,7,14,21,28,
            35,42,49,56,57,50,43,36,29,22,15,23,30,37,44,51,58,59,52,45,38,31,39,46,53,60,61,54,47,55,62,63 };

        int Cos16(int i)
        {
            i &= 31;
            if (i <= 8) return _cos[i];
            if (i <= 16) return -_cos[16 - i];
            if (i <= 24) return -_cos[i - 16];
            return _cos[32 - i];
        }

        // Quant tables: 4 * 64 ints, indexed [tq*64 + k] (k in zig-zag order).
        readonly int[] _quant = new int[4 * 64];
        // Huffman tables: index ti = tc*4 + th (tc: 0=DC 1=AC). Canonical-code
        // decode via min/max/valptr per code length (1..16) + a flat vals buffer.
        readonly int[] _hMin = new int[8 * 17];
        readonly int[] _hMax = new int[8 * 17];   // -1 = no codes of that length
        readonly int[] _hValPtr = new int[8 * 17];
        readonly byte[] _hVals = new byte[8 * 256];

        // --- entropy bit reader (handles 0xFF00 stuffing + restart markers) ---
        byte[] _d; int _p, _end;
        int _bitBuf, _bitCnt; bool _marker;
        readonly int[] _idctTmp = new int[64];

        void ResetBits() { _bitBuf = 0; _bitCnt = 0; _marker = false; }

        int NextBit()
        {
            if (_bitCnt == 0)
            {
                if (_p >= _end) { _marker = true; return 0; }
                int b = _d[_p++];
                if (b == 0xFF)
                {
                    int b2 = _p < _end ? _d[_p] : 0;
                    if (b2 == 0) { _p++; }                 // stuffed 0xFF00 -> 0xFF
                    else { _marker = true; _p--; return 0; } // real marker: stop
                }
                _bitBuf = b; _bitCnt = 8;
            }
            _bitCnt--;
            return (_bitBuf >> _bitCnt) & 1;
        }

        int Receive(int n) { int v = 0; for (int i = 0; i < n; i++) v = (v << 1) | NextBit(); return v; }
        // Extend a received value to signed per the JPEG magnitude category.
        static int Extend(int v, int n) => v < (1 << (n - 1)) ? v - (1 << n) + 1 : v;

        int DecodeHuff(int ti)
        {
            int b = ti * 17, code = 0;
            for (int len = 1; len <= 16; len++)
            {
                code = (code << 1) | NextBit();
                if (_marker) return 0;
                if (_hMax[b + len] >= 0 && code <= _hMax[b + len])
                    return _hVals[ti * 256 + _hValPtr[b + len] + code - _hMin[b + len]];
            }
            return 0;
        }

        // 8x8 integer inverse DCT: coeff (natural order) -> pixels (level-shifted
        // by the caller). Separable: 1D IDCT over rows then columns.
        void Idct(int[] blk, int[] outp)
        {
            int[] tmp = _idctTmp;
            for (int y = 0; y < 8; y++)
                for (int x = 0; x < 8; x++)
                {
                    int acc = 0;
                    for (int u = 0; u < 8; u++)
                        acc += (u == 0 ? 5793 : Cos16((2 * x + 1) * u)) * blk[y * 8 + u];
                    tmp[y * 8 + x] = (acc + (1 << 10)) >> 11;
                }
            for (int x = 0; x < 8; x++)
                for (int y = 0; y < 8; y++)
                {
                    int acc = 0;
                    for (int v = 0; v < 8; v++)
                        acc += (v == 0 ? 5793 : Cos16((2 * y + 1) * v)) * tmp[v * 8 + x];
                    outp[y * 8 + x] = (acc + (1 << 16)) >> 17;
                }
        }

        static int Clamp(int v) => v < 0 ? 0 : (v > 255 ? 255 : v);
        static int Rd16(byte[] o, int i) => (o[i] << 8) | o[i + 1];

        // Component descriptors as parallel flat arrays (no struct[] needed, and
        // avoids any reference-array store): [c] over up to 4 components.
        readonly int[] _cId = new int[4], _cH = new int[4], _cV = new int[4];
        readonly int[] _cTq = new int[4], _cTd = new int[4], _cTa = new int[4], _cDc = new int[4];

        public static Bitmap Load(byte[] o)
        {
            if (o == null || o.Length < 4 || o[0] != 0xFF || o[1] != 0xD8) return null; // SOI
            return new Jpeg().Decode(o);
        }

        Bitmap Decode(byte[] o)
        {
            for (int i = 0; i < 8 * 17; i++) _hMax[i] = -1;
            int p = 2, w = 0, h = 0, ncomp = 0, restart = 0;
            bool progressive = false;
            while (p + 4 <= o.Length)
            {
                if (o[p] != 0xFF) { p++; continue; }
                int m = o[p + 1]; p += 2;
                if (m == 0xD9) break;                       // EOI
                if (m == 0x01 || (m >= 0xD0 && m <= 0xD7)) continue; // standalone
                int len = Rd16(o, p); int seg = p + 2; p += len;
                if (m == 0xDB) // DQT
                {
                    int q = seg;
                    while (q < seg + len - 2)
                    {
                        int pq = o[q] >> 4, tq = o[q] & 15; q++;
                        if (tq < 4)
                            for (int i = 0; i < 64; i++) { _quant[tq * 64 + i] = pq == 0 ? o[q] : Rd16(o, q); q += pq == 0 ? 1 : 2; }
                        else q += pq == 0 ? 64 : 128;
                    }
                }
                else if (m == 0xC0 || m == 0xC1) // SOF0/1 baseline
                {
                    h = Rd16(o, seg + 1); w = Rd16(o, seg + 3); ncomp = o[seg + 5];
                    for (int i = 0; i < ncomp && i < 4; i++)
                    {
                        int b = seg + 6 + i * 3;
                        _cId[i] = o[b]; _cH[i] = o[b + 1] >> 4; _cV[i] = o[b + 1] & 15; _cTq[i] = o[b + 2];
                    }
                }
                else if (m == 0xC2) { progressive = true; }
                else if (m == 0xC4) // DHT
                {
                    int q = seg;
                    while (q < seg + len - 2)
                    {
                        int tc = o[q] >> 4, th = o[q] & 15; q++;
                        int ti = tc * 4 + th, bb = ti * 17;
                        int code = 0, k = 0, total = 0;
                        int cq = q; q += 16;               // counts, then values
                        for (int lenc = 1; lenc <= 16; lenc++)
                        {
                            int c = o[cq + lenc - 1];
                            if (c == 0) { _hMax[bb + lenc] = -1; }
                            else { _hValPtr[bb + lenc] = k; _hMin[bb + lenc] = code; code += c; k += c; _hMax[bb + lenc] = code - 1; }
                            code <<= 1; total += c;
                        }
                        for (int i = 0; i < total && i < 256; i++) _hVals[ti * 256 + i] = o[q++];
                    }
                }
                else if (m == 0xDD) { restart = Rd16(o, seg); } // DRI
                else if (m == 0xDA) // SOS
                {
                    if (progressive || ncomp == 0) return null;
                    int ns = o[seg]; int q = seg + 1;
                    for (int i = 0; i < ns; i++)
                    {
                        int cid = o[q]; int sel = o[q + 1]; q += 2;
                        for (int c = 0; c < ncomp; c++)
                            if (_cId[c] == cid) { _cTd[c] = sel >> 4; _cTa[c] = sel & 15; }
                    }
                    return DecodeScan(o, q + 3, w, h, ncomp, restart); // skip Ss/Se/Ah:Al
                }
            }
            return null;
        }

        Bitmap DecodeScan(byte[] o, int start, int w, int h, int ncomp, int restart)
        {
            int hmax = 1, vmax = 1;
            for (int c = 0; c < ncomp; c++) { if (_cH[c] > hmax) hmax = _cH[c]; if (_cV[c] > vmax) vmax = _cV[c]; }
            int mcusX = (w + hmax * 8 - 1) / (hmax * 8), mcusY = (h + vmax * 8 - 1) / (vmax * 8);

            // One flat plane buffer for all components, at their own resolutions.
            int[] pw = new int[4], ph = new int[4], pbase = new int[4];
            int total = 0;
            for (int c = 0; c < ncomp; c++)
            {
                pw[c] = mcusX * _cH[c] * 8; ph[c] = mcusY * _cV[c] * 8; pbase[c] = total; total += pw[c] * ph[c];
            }
            int[] plane = new int[total];

            _d = o; _p = start; _end = o.Length; ResetBits();
            for (int c = 0; c < ncomp; c++) _cDc[c] = 0;
            int[] blk = new int[64]; int[] px = new int[64];
            int mcuCount = 0;

            for (int my = 0; my < mcusY; my++)
                for (int mx = 0; mx < mcusX; mx++)
                {
                    if (restart > 0 && mcuCount > 0 && mcuCount % restart == 0)
                    {
                        _bitCnt = 0;
                        while (_p + 1 < _end && !(o[_p] == 0xFF && o[_p + 1] >= 0xD0 && o[_p + 1] <= 0xD7)) _p++;
                        if (_p + 1 < _end) _p += 2;
                        ResetBits();
                        for (int c = 0; c < ncomp; c++) _cDc[c] = 0;
                    }
                    for (int c = 0; c < ncomp; c++)
                    {
                        int qb = _cTq[c] * 64, tiDc = _cTd[c], tiAc = 4 + _cTa[c];
                        for (int by = 0; by < _cV[c]; by++)
                            for (int bx = 0; bx < _cH[c]; bx++)
                            {
                                for (int i = 0; i < 64; i++) blk[i] = 0;
                                int t = DecodeHuff(tiDc);
                                int diff = t == 0 ? 0 : Extend(Receive(t), t);
                                _cDc[c] += diff;
                                blk[0] = _cDc[c] * _quant[qb];
                                int k = 1;
                                while (k < 64)
                                {
                                    int rs = DecodeHuff(tiAc);
                                    int r = rs >> 4, s = rs & 15;
                                    if (s == 0) { if (r != 15) break; k += 16; continue; }
                                    k += r; if (k >= 64) break;
                                    blk[_zig[k]] = Extend(Receive(s), s) * _quant[qb + k];
                                    k++;
                                }
                                Idct(blk, px);
                                int ox = (mx * _cH[c] + bx) * 8, oy = (my * _cV[c] + by) * 8;
                                for (int yy = 0; yy < 8; yy++)
                                    for (int xx = 0; xx < 8; xx++)
                                        plane[pbase[c] + (oy + yy) * pw[c] + ox + xx] = Clamp(px[yy * 8 + xx] + 128);
                            }
                    }
                    mcuCount++;
                }

            Bitmap bmp = new Bitmap(w, h);
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                {
                    int Y = plane[pbase[0] + (y * _cV[0] / vmax) * pw[0] + (x * _cH[0] / hmax)];
                    uint argb;
                    if (ncomp == 1) argb = 0xFF000000u | ((uint)Y << 16) | ((uint)Y << 8) | (uint)Y;
                    else
                    {
                        int cb = plane[pbase[1] + (y * _cV[1] / vmax) * pw[1] + (x * _cH[1] / hmax)] - 128;
                        int cr = plane[pbase[2] + (y * _cV[2] / vmax) * pw[2] + (x * _cH[2] / hmax)] - 128;
                        int r = Clamp(Y + ((91881 * cr) >> 16));
                        int g = Clamp(Y - ((22554 * cb + 46802 * cr) >> 16));
                        int b = Clamp(Y + ((116130 * cb) >> 16));
                        argb = 0xFF000000u | ((uint)r << 16) | ((uint)g << 8) | (uint)b;
                    }
                    bmp.Pixels[y * w + x] = argb;
                }
            return bmp;
        }
    }

    /// <summary>An 8×8 bitmap font (a usable ASCII subset). The glyph table is
    /// an instance field built in the constructor from readable ASCII-art, so it
    /// avoids zerolib's unsupported static reference fields.</summary>
    public sealed class Font
    {
        readonly byte[] _g; // 128 * 8
        public readonly int CharW, CharH;
        Font(byte[] g) { _g = g; CharW = 8; CharH = 8; }

        public byte Row(char ch, int r)
        {
            int c = ch;
            if (c >= 'a' && c <= 'z') c -= 32; // map lowercase to uppercase glyphs
            if (c < 0 || c >= 128) return 0;
            return _g[c * 8 + r];
        }
        public int Measure(string s) => s.Length * CharW;

        static void G(byte[] g, char ch, string r0, string r1, string r2, string r3, string r4, string r5, string r6)
        {
            int b = (int)ch * 8;
            g[b] = Bits(r0); g[b + 1] = Bits(r1); g[b + 2] = Bits(r2); g[b + 3] = Bits(r3);
            g[b + 4] = Bits(r4); g[b + 5] = Bits(r5); g[b + 6] = Bits(r6); g[b + 7] = 0;
        }
        static byte Bits(string s)
        {
            int v = 0;
            for (int i = 0; i < s.Length && i < 8; i++) if (s[i] == '#') v |= 1 << (7 - i);
            return (byte)v;
        }

        public static Font Default()
        {
            byte[] g = new byte[128 * 8];
            G(g, '0', ".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###.");
            G(g, '1', "..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###.");
            G(g, '2', ".###.", "#...#", "....#", "..##.", ".#...", "#....", "#####");
            G(g, '3', "####.", "....#", "....#", ".###.", "....#", "....#", "####.");
            G(g, '4', "...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#.");
            G(g, '5', "#####", "#....", "####.", "....#", "....#", "#...#", ".###.");
            G(g, '6', ".###.", "#....", "#....", "####.", "#...#", "#...#", ".###.");
            G(g, '7', "#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#...");
            G(g, '8', ".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###.");
            G(g, '9', ".###.", "#...#", "#...#", ".####", "....#", "....#", ".###.");
            G(g, 'A', ".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#");
            G(g, 'B', "####.", "#...#", "#...#", "####.", "#...#", "#...#", "####.");
            G(g, 'C', ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.");
            G(g, 'D', "###..", "#..#.", "#...#", "#...#", "#...#", "#..#.", "###..");
            G(g, 'E', "#####", "#....", "#....", "####.", "#....", "#....", "#####");
            G(g, 'F', "#####", "#....", "#....", "####.", "#....", "#....", "#....");
            G(g, 'G', ".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".###.");
            G(g, 'H', "#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#");
            G(g, 'I', ".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###.");
            G(g, 'J', "..###", "...#.", "...#.", "...#.", "#..#.", "#..#.", ".##..");
            G(g, 'K', "#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#");
            G(g, 'L', "#....", "#....", "#....", "#....", "#....", "#....", "#####");
            G(g, 'M', "#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#");
            G(g, 'N', "#...#", "##..#", "#.#.#", "#.#.#", "#..##", "#...#", "#...#");
            G(g, 'O', ".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.");
            G(g, 'P', "####.", "#...#", "#...#", "####.", "#....", "#....", "#....");
            G(g, 'Q', ".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#");
            G(g, 'R', "####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#");
            G(g, 'S', ".####", "#....", "#....", ".###.", "....#", "....#", "####.");
            G(g, 'T', "#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#..");
            G(g, 'U', "#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###.");
            G(g, 'V', "#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#..");
            G(g, 'W', "#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#");
            G(g, 'X', "#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#");
            G(g, 'Y', "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#..");
            G(g, 'Z', "#####", "....#", "...#.", "..#..", ".#...", "#....", "#####");
            G(g, ' ', ".....", ".....", ".....", ".....", ".....", ".....", ".....");
            G(g, '.', ".....", ".....", ".....", ".....", ".....", ".##..", ".##..");
            G(g, ',', ".....", ".....", ".....", ".....", ".##..", ".##..", ".#...");
            G(g, ':', ".....", ".##..", ".##..", ".....", ".##..", ".##..", ".....");
            G(g, '-', ".....", ".....", ".....", "#####", ".....", ".....", ".....");
            G(g, '+', ".....", "..#..", "..#..", "#####", "..#..", "..#..", ".....");
            G(g, '=', ".....", ".....", "#####", ".....", "#####", ".....", ".....");
            G(g, '!', "..#..", "..#..", "..#..", "..#..", "..#..", ".....", "..#..");
            G(g, '?', ".###.", "#...#", "...#.", "..#..", "..#..", ".....", "..#..");
            G(g, '/', "....#", "...#.", "..#..", "..#..", ".#...", "#....", "#....");
            G(g, '(', "..##.", ".#...", "#....", "#....", "#....", ".#...", "..##.");
            G(g, ')', ".##..", "...#.", "....#", "....#", "....#", "...#.", ".##..");
            G(g, '%', "##..#", "##.#.", "..#..", ".#...", "#..##", "..#.#", "..#.#");
            return new Font(g);
        }
    }

    /// <summary>A window; blit a finished Bitmap and draw text on top.</summary>
    public unsafe struct Window
    {
        public readonly uint Handle;
        Window(uint h) { Handle = h; }

        [DllImport("*")] static extern unsafe uint bz_win_create(byte* title, ulong len, ulong dims);
        [DllImport("*")] static extern unsafe ulong bz_win_cmd(uint window, DrawCmd* cmd);
        [DllImport("*")] static extern void bz_win_present(uint window);

        struct DrawCmd { public ulong Op; public int X, Y, W, H; public uint Color, Pad; public ulong TextPtr, TextLen; }
        const ulong OpText = 1, OpBlit = 7;

        public static Window Create(string title, int w, int h)
        {
            byte* buf = stackalloc byte[64];
            int n = 0;
            fixed (char* tc = title) for (int i = 0; i < title.Length && n < 63; i++) buf[n++] = (byte)tc[i];
            uint win = bz_win_create(buf, (ulong)n, ((ulong)(uint)w << 32) | (uint)h);
            return new Window(win);
        }

        public void Blit(Bitmap b, int x, int y)
        {
            fixed (uint* p = b.Pixels)
            {
                var cmd = new DrawCmd { Op = OpBlit, X = x, Y = y, W = b.Width, H = b.Height, TextPtr = (ulong)p, TextLen = (ulong)(b.Width * b.Height * 4) };
                bz_win_cmd(Handle, &cmd);
            }
        }
        public void Blit(Bitmap b) => Blit(b, 0, 0);

        public void DrawText(int x, int y, Color c, string s)
        {
            byte* buf = stackalloc byte[256];
            int n = 0;
            fixed (char* sc = s) for (int i = 0; i < s.Length && n < 255; i++) buf[n++] = (byte)sc[i];
            var cmd = new DrawCmd { Op = OpText, X = x, Y = y, Color = c.Rgb24, TextPtr = (ulong)buf, TextLen = (ulong)n };
            bz_win_cmd(Handle, &cmd);
        }

        public void Present() => bz_win_present(Handle);
    }
}
