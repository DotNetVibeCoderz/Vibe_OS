// Buitenzorg.Drawing (v0.9 "Serbuk") — a System.Drawing-style managed graphics
// library over the window syscall ABI. Freestanding (no GC): all types are
// value types, text buffers use stackalloc, no heap allocation.
//
// Shipped as a shared source file for now; becomes a NuGet-style SDK package
// once the runtime gains a full BCL.

using System.Runtime.InteropServices;

namespace Buitenzorg.Drawing
{
    // ARGB-ish color (0x00RRGGBB). Mirrors System.Drawing.Color's surface.
    public readonly struct Color
    {
        public readonly uint Value;
        public Color(uint value) { Value = value; }
        public static Color FromRgb(byte r, byte g, byte b)
            => new Color(((uint)r << 16) | ((uint)g << 8) | b);

        public static Color Black => new Color(0x000000);
        public static Color White => new Color(0xFFFFFF);
        public static Color Red => new Color(0xE0483B);
        public static Color Green => new Color(0x4FA33F);
        public static Color Blue => new Color(0x4A78E8);
        public static Color Yellow => new Color(0xE8B84B);
        public static Color Leaf => new Color(0x6FC14E);
        public static Color Soil => new Color(0x141C16);
        public static Color Text => new Color(0xC8E9B0);
    }

    public readonly struct Pen
    {
        public readonly Color Color;
        public Pen(Color color) { Color = color; }
    }

    public readonly struct Brush
    {
        public readonly Color Color;
        public Brush(Color color) { Color = color; }
    }

    public readonly struct Point { public readonly int X, Y; public Point(int x, int y) { X = x; Y = y; } }
    public readonly struct Size { public readonly int Width, Height; public Size(int w, int h) { Width = w; Height = h; } }
    public readonly struct Rectangle
    {
        public readonly int X, Y, Width, Height;
        public Rectangle(int x, int y, int w, int h) { X = x; Y = y; Width = w; Height = h; }
    }

    // A drawing surface bound to a window. System.Drawing.Graphics-style API.
    public unsafe struct Graphics
    {
        public readonly uint Window;
        public Graphics(uint window) { Window = window; }

        [DllImport("*")] static extern unsafe uint bz_win_create(byte* title, ulong len, ulong dims);
        [DllImport("*")] static extern unsafe ulong bz_win_cmd(uint window, DrawCmd* cmd);
        [DllImport("*")] static extern void bz_win_present(uint window);

        struct DrawCmd
        {
            public ulong Op;
            public int X, Y, W, H;
            public uint Color, Pad;
            public ulong TextPtr, TextLen;
        }
        const ulong OpFill = 0, OpText = 1, OpClear = 2, OpLine = 3, OpEllipse = 4, OpFillEllipse = 5, OpRect = 6;

        public static Graphics CreateWindow(string title, int w, int h)
        {
            byte* buf = stackalloc byte[64];
            int n = 0;
            fixed (char* tc = title)
                for (int i = 0; i < title.Length && n < 63; i++) buf[n++] = (byte)tc[i];
            uint win = bz_win_create(buf, (ulong)n, ((ulong)(uint)w << 32) | (uint)h);
            return new Graphics(win);
        }

        void Cmd(ulong op, int x, int y, int w, int h, uint color)
        {
            var cmd = new DrawCmd { Op = op, X = x, Y = y, W = w, H = h, Color = color };
            bz_win_cmd(Window, &cmd);
        }

        public void Clear(Color c) => Cmd(OpClear, 0, 0, 0, 0, c.Value);
        public void FillRectangle(Brush b, int x, int y, int w, int h) => Cmd(OpFill, x, y, w, h, b.Color.Value);
        public void FillRectangle(Brush b, Rectangle r) => FillRectangle(b, r.X, r.Y, r.Width, r.Height);
        public void DrawRectangle(Pen p, int x, int y, int w, int h) => Cmd(OpRect, x, y, w, h, p.Color.Value);
        public void DrawLine(Pen p, int x0, int y0, int x1, int y1) => Cmd(OpLine, x0, y0, x1 - x0, y1 - y0, p.Color.Value);
        public void DrawEllipse(Pen p, int x, int y, int w, int h) => Cmd(OpEllipse, x, y, w, h, p.Color.Value);
        public void FillEllipse(Brush b, int x, int y, int w, int h) => Cmd(OpFillEllipse, x, y, w, h, b.Color.Value);

        public void DrawString(string s, Color color, int x, int y)
        {
            byte* buf = stackalloc byte[256];
            int n = 0;
            fixed (char* sc = s)
                for (int i = 0; i < s.Length && n < 255; i++) buf[n++] = (byte)sc[i];
            var cmd = new DrawCmd { Op = OpText, X = x, Y = y, Color = color.Value, TextPtr = (ulong)buf, TextLen = (ulong)n };
            bz_win_cmd(Window, &cmd);
        }

        // Draw from a raw char buffer (no managed string / heap needed).
        public void DrawChars(char* s, int len, Color color, int x, int y)
        {
            byte* buf = stackalloc byte[512];
            int n = 0;
            for (int i = 0; i < len && n < 511; i++) buf[n++] = (byte)s[i];
            var cmd = new DrawCmd { Op = OpText, X = x, Y = y, Color = color.Value, TextPtr = (ulong)buf, TextLen = (ulong)n };
            bz_win_cmd(Window, &cmd);
        }

        public void Present() => bz_win_present(Window);
    }

    // Small helpers apps commonly need.
    public static class Sys
    {
        [DllImport("*")] static extern ulong bz_ticks();
        public static ulong Ticks() => bz_ticks();
        public static void Sleep(ulong ticks) { ulong u = bz_ticks() + ticks; while (bz_ticks() < u) { } }
    }
}
