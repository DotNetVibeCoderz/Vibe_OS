// Buitenzorg.UI -- geometry, enums, the host entry points and text metrics.

using System.Runtime.InteropServices;

namespace Buitenzorg.UI;

/// <summary>Native entry points of CLRHOST.ELF ("pinvoke:buitenzorg!...").</summary>
internal static class Native
{
    [DllImport("buitenzorg")] internal static extern int WinCreateEx(string title, int width, int height, int flags);
    [DllImport("buitenzorg")] internal static extern int WinDraw(int window, int op, int x, int y, int w, int h, int color, string text);
    [DllImport("buitenzorg")] internal static extern int WinDrawEx(int window, int op, int x, int y, int w, int h, int color, int param, int param2, string text);
    [DllImport("buitenzorg")] internal static extern int WinBlit(int window, int x, int y, int w, int h, int[] pixels);
    [DllImport("buitenzorg")] internal static extern int WinPresent(int window);
    [DllImport("buitenzorg")] internal static extern long WinEvent(int window);
    [DllImport("buitenzorg")] internal static extern int KeyRead();
    [DllImport("buitenzorg")] internal static extern long TextMetrics(int style);
    [DllImport("buitenzorg")] internal static extern int IsInteractive();
}

/// <summary>Space around an element (left, top, right, bottom).</summary>
public struct Thickness
{
    public int Left, Top, Right, Bottom;
    public Thickness(int all) { Left = Top = Right = Bottom = all; }
    public Thickness(int horizontal, int vertical) { Left = Right = horizontal; Top = Bottom = vertical; }
    public Thickness(int left, int top, int right, int bottom) { Left = left; Top = top; Right = right; Bottom = bottom; }
    public int Horizontal => Left + Right;
    public int Vertical => Top + Bottom;
    public static implicit operator Thickness(int all) => new Thickness(all);
}

public struct Size
{
    public int Width, Height;
    public Size(int w, int h) { Width = w; Height = h; }
    public const int Infinite = 1 << 28;
    public static Size Unbounded => new Size(Infinite, Infinite);
}

public struct Point
{
    public int X, Y;
    public Point(int x, int y) { X = x; Y = y; }
}

/// <summary>A rectangle in window (client) pixels.</summary>
public struct Rect
{
    public int X, Y, Width, Height;
    public Rect(int x, int y, int w, int h) { X = x; Y = y; Width = w; Height = h; }
    public int Right => X + Width;
    public int Bottom => Y + Height;
    public bool Contains(int px, int py) => px >= X && py >= Y && px < X + Width && py < Y + Height;
    public Rect Deflate(Thickness t) =>
        new Rect(X + t.Left, Y + t.Top, Math.Max(0, Width - t.Horizontal), Math.Max(0, Height - t.Vertical));
    public Rect Inflate(int d) => new Rect(X - d, Y - d, Width + 2 * d, Height + 2 * d);
    public Rect Intersect(Rect o)
    {
        int x = Math.Max(X, o.X), y = Math.Max(Y, o.Y);
        int r = Math.Min(Right, o.Right), b = Math.Min(Bottom, o.Bottom);
        return r <= x || b <= y ? new Rect(x, y, 0, 0) : new Rect(x, y, r - x, b - y);
    }
    public bool Covers(Rect o) => o.X >= X && o.Y >= Y && o.Right <= Right && o.Bottom <= Bottom;
    public bool IsEmpty => Width <= 0 || Height <= 0;
    public Point Center => new Point(X + Width / 2, Y + Height / 2);
}

public enum HorizontalAlignment { Stretch, Left, Center, Right }
public enum VerticalAlignment { Stretch, Top, Center, Bottom }
public enum Orientation { Vertical, Horizontal }
public enum Dock { Left, Top, Right, Bottom }
public enum FontWeight { Normal, Bold }
public enum TextAlignment { Left, Center, Right }
public enum TextWrapping { NoWrap, Wrap }
public enum TextTrimming { None, CharacterEllipsis }
public enum Stretch { None, Uniform, Fill }
public enum PlacementMode { Bottom, Top, Right, Left, Pointer, Center }
public enum SplitViewDisplayMode { Inline, Overlay, CompactInline, CompactOverlay }
public enum ThemeVariant { Dark, Light }
public enum NotificationType { Information, Success, Warning, Error }

/// <summary>Colors are 0xRRGGBB ints; <see cref="Transparent"/> means "draw nothing".</summary>
public static class Color
{
    public const int Transparent = -1;

    public static int Rgb(int r, int g, int b) => ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);

    /// <summary>Linear mix of two colors, t in 0..256.</summary>
    public static int Mix(int a, int b, int t)
    {
        if (a < 0) return b;
        if (b < 0) return a;
        int ra = (a >> 16) & 0xFF, ga = (a >> 8) & 0xFF, ba = a & 0xFF;
        int rb = (b >> 16) & 0xFF, gb = (b >> 8) & 0xFF, bb = b & 0xFF;
        return Rgb(ra + (rb - ra) * t / 256, ga + (gb - ga) * t / 256, ba + (bb - ba) * t / 256);
    }

    public static int Lighten(int c, int amount) => Mix(c, 0xFFFFFF, amount);
    public static int Darken(int c, int amount) => Mix(c, 0x000000, amount);

    /// <summary>Parse "#RRGGBB" (or "RRGGBB").</summary>
    public static int Parse(string s)
    {
        int v = 0;
        foreach (char ch in s)
        {
            int d = ch >= '0' && ch <= '9' ? ch - '0' : ch >= 'a' && ch <= 'f' ? ch - 'a' + 10 : ch >= 'A' && ch <= 'F' ? ch - 'A' + 10 : -1;
            if (d >= 0) v = (v << 4) | d;
        }
        return v & 0xFFFFFF;
    }
}

/// <summary>Font metrics of the kernel's monospaced bitmap fonts (three styles).</summary>
public static class Fonts
{
    static readonly int[] _w = new int[3];
    static readonly int[] _h = new int[3];

    /// <summary>Style index: 0 body (16 px), 1 strong (bold 16 px), 2 heading (bold 24 px).</summary>
    public static int StyleOf(FontWeight weight, int size) => size >= 20 ? 2 : weight == FontWeight.Bold ? 1 : 0;

    static void Load(int style)
    {
        if (_w[style] != 0) return;
        long m = 0;
        try { m = Native.TextMetrics(style); } catch (Exception) { /* off-device: defaults */ }
        _w[style] = (int)(m >> 32);
        _h[style] = (int)(m & 0xFFFFFFFF);
        if (_w[style] <= 0) { _w[style] = style == 2 ? 12 : 8; _h[style] = style == 2 ? 24 : 16; }
    }

    public static int GlyphWidth(int style) { Load(style); return _w[style]; }
    public static int LineHeight(int style) { Load(style); return _h[style]; }
    public static int Measure(string text, int style) => text.Length * GlyphWidth(style);
}

/// <summary>Keys as KEY_READ delivers them (navigation keys are private-use characters).</summary>
public static class Keys
{
    public const char Up = '', Down = '', Left = '', Right = '';
    public const char Home = '', End = '', Delete = '', PageUp = '', PageDown = '';
    public const char Enter = '\n', Tab = '\t', Backspace = '\b', Escape = (char)27, Space = ' ';
    public const char Rubout = (char)127;

    public static bool IsNavigation(char c) => c >= '' && c <= '';
    public static bool IsPrintable(char c) => c >= ' ' && c != Rubout && !IsNavigation(c);
}

/// <summary>Monotonic milliseconds.</summary>
public static class Clock
{
    public static long Now => Environment.TickCount64;
}
