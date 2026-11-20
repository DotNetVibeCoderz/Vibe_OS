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
    [DllImport("buitenzorg")] internal static extern int KeyEvent();
    [DllImport("buitenzorg")] internal static extern int KeyState(int code);
    [DllImport("buitenzorg")] internal static extern long TextMetrics(int style);
    [DllImport("buitenzorg")] internal static extern long FontGeneration();
    [DllImport("buitenzorg")] internal static extern long FontCount();
    [DllImport("buitenzorg")] internal static extern string FontInfo(int id);
    [DllImport("buitenzorg")] internal static extern long FontFind(string family, int bold);
    [DllImport("buitenzorg")] internal static extern long FontMetrics(int id, int px);
    [DllImport("buitenzorg")] internal static extern long FontMeasure(int id, int px, string text);
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

/// <summary>Raw key codes of press / release events (Window.RawKeys, games):
/// the PC scancode set-1 make code, | 0x100 for the 0xE0-prefixed keys.</summary>
public static class RawKey
{
    public const int Escape = 0x01, D1 = 0x02, D2 = 0x03, D3 = 0x04, D4 = 0x05, D5 = 0x06, D6 = 0x07, D7 = 0x08, D8 = 0x09, D9 = 0x0A, D0 = 0x0B;
    public const int Backspace = 0x0E, Tab = 0x0F, Enter = 0x1C, LeftCtrl = 0x1D, LeftShift = 0x2A, RightShift = 0x36, LeftAlt = 0x38, Space = 0x39;
    public const int Q = 0x10, W = 0x11, E = 0x12, R = 0x13, T = 0x14, Y = 0x15, U = 0x16, I = 0x17, O = 0x18, P = 0x19;
    public const int A = 0x1E, S = 0x1F, D = 0x20, F = 0x21, G = 0x22, H = 0x23, J = 0x24, K = 0x25, L = 0x26;
    public const int Z = 0x2C, X = 0x2D, C = 0x2E, V = 0x2F, B = 0x30, N = 0x31, M = 0x32;
    public const int F1 = 0x3B, F2 = 0x3C, F3 = 0x3D, F4 = 0x3E, F5 = 0x3F, F6 = 0x40, F7 = 0x41, F8 = 0x42, F9 = 0x43, F10 = 0x44, F11 = 0x57, F12 = 0x58;
    public const int Up = 0x148, Down = 0x150, Left = 0x14B, Right = 0x14D, Home = 0x147, End = 0x14F, PageUp = 0x149, PageDown = 0x151, Delete = 0x153, RightCtrl = 0x11D, RightAlt = 0x138;
    /// <summary>Gamepad buttons (USB HID gamepads; the kernel maps every pad to
    /// this standard layout: A/B/X/Y = bottom/right/left/top face buttons).</summary>
    public const int PadA = 0x200, PadB = 0x201, PadX = 0x202, PadY = 0x203, PadL = 0x204, PadR = 0x205,
        PadSelect = 0x206, PadStart = 0x207, PadUp = 0x210, PadDown = 0x211, PadLeft = 0x212, PadRight = 0x213;

    /// <summary>The raw key for a character a test types ('a' -> A, ' ' -> Space).</summary>
    public static int FromChar(char c)
    {
        string row1 = "qwertyuiop", row2 = "asdfghjkl", row3 = "zxcvbnm";
        c = char.ToLowerInvariant(c);
        int i = row1.IndexOf(c); if (i >= 0) return Q + i;
        i = row2.IndexOf(c); if (i >= 0) return A + i;
        i = row3.IndexOf(c); if (i >= 0) return Z + i;
        if (c >= '1' && c <= '9') return D1 + (c - '1');
        if (c == '0') return D0;
        return c switch { ' ' => Space, '\n' => Enter, (char)27 => Escape, _ => 0 };
    }
}

/// <summary>A raw key press or release (Control.OnKeyEvent).</summary>
public sealed class RawKeyEventArgs
{
    public int Code { get; init; }
    public bool Pressed { get; init; }
    public bool Handled { get; set; }
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
    // proportional measuring (the kernel's UI font): -1 = not asked yet,
    // 0 = grid only (off the device, or the bitmap font chosen), 1 = proportional
    static int _mode = -1;
    static long _gen = -1, _checked;
    static readonly Dictionary<string, int>[] _cache = { new Dictionary<string, int>(), new Dictionary<string, int>(), new Dictionary<string, int>() };

    /// <summary>Face aliases of the kernel's font engine (FONT_OP): the UI font per style, the monospace font.</summary>
    public const int UiFont = 0xFFF0, MonoFont = 0xFFFE;

    /// <summary>Pixel size of the UI font for a style (the kernel's: 14 body / strong, 21 heading).</summary>
    public static int Px(int style) => style == 2 ? 21 : 14;

    /// <summary>Whether text in styles is drawn in a proportional font (a TrueType UI font
    /// on the device). Off the device, or with the bitmap font chosen, text sits on the
    /// monospace grid and <see cref="Measure"/> is characters x <see cref="GlyphWidth"/>.</summary>
    public static bool Proportional { get { Refresh(); return _mode == 1; } }

    /// <summary>Re-ask the kernel at most every half second whether fonts changed.</summary>
    static void Refresh()
    {
        if (_mode == 0 && _gen == -2) return; // no font engine (off the device)
        long now = Clock.Now;
        if (_mode >= 0 && now - _checked < 500) return;
        _checked = now;
        long g;
        try { g = Native.FontGeneration(); }
        catch (Exception) { _mode = 0; _gen = -2; return; }
        if (g == _gen && _mode >= 0) return;
        _gen = g;
        foreach (var c in _cache) c.Clear();
        long m = -1;
        try { m = Native.FontMeasure(UiFont, 14, "M"); } catch (Exception) { }
        _mode = m > 0 ? 1 : 0;
    }

    /// <summary>The font generation (changes when fonts or the UI font change); -2 off the device.</summary>
    public static long Generation { get { Refresh(); return _gen; } }

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

    /// <summary>Width of one cell of the monospace grid (8 px; 12 px for headings).</summary>
    public static int GlyphWidth(int style) { Load(style); return _w[style]; }
    public static int LineHeight(int style) { Load(style); return _h[style]; }

    /// <summary>Width of <paramref name="text"/> in a style's font, in pixels (kerned).</summary>
    public static int Measure(string text, int style)
    {
        if (text.Length == 0) return 0;
        style = style < 0 || style > 2 ? 0 : style;
        if (!Proportional) return text.Length * GlyphWidth(style);
        var cache = _cache[style];
        if (cache.TryGetValue(text, out int w)) return w;
        long m = -1;
        try { m = Native.FontMeasure(UiFont + style, Px(style), text); } catch (Exception) { }
        w = m >= 0 ? (int)m : text.Length * GlyphWidth(style);
        if (cache.Count > 1500) cache.Clear();
        cache[text] = w;
        return w;
    }

    /// <summary>Width of the first <paramref name="count"/> characters.</summary>
    public static int MeasurePrefix(string text, int count, int style)
    {
        if (count <= 0) return 0;
        if (count >= text.Length) return Measure(text, style);
        return Proportional ? Measure(text.Substring(0, count), style) : count * GlyphWidth(style);
    }

    /// <summary>How many leading characters fit in <paramref name="width"/> pixels.</summary>
    public static int Fit(string text, int width, int style)
    {
        if (width <= 0 || text.Length == 0) return 0;
        if (!Proportional) return Math.Min(text.Length, width / GlyphWidth(style));
        if (Measure(text, style) <= width) return text.Length;
        int lo = 0, hi = text.Length; // lo fits, hi does not
        while (hi - lo > 1)
        {
            int mid = (lo + hi) / 2;
            if (Measure(text.Substring(0, mid), style) <= width) lo = mid; else hi = mid;
        }
        return lo;
    }

    /// <summary>The character boundary nearest to <paramref name="x"/> pixels from the text's start.</summary>
    public static int IndexAt(string text, int x, int style)
    {
        if (x <= 0 || text.Length == 0) return 0;
        if (!Proportional) return Math.Min(text.Length, (x + GlyphWidth(style) / 2) / GlyphWidth(style));
        int n = Fit(text, x, style);
        if (n >= text.Length) return text.Length;
        int a = MeasurePrefix(text, n, style), b = MeasurePrefix(text, n + 1, style);
        return x - a <= b - x ? n : n + 1;
    }

    /// <summary>A face of the kernel's font engine by family (and weight); -1 when it is not installed.</summary>
    public static int Find(string family, bool bold = false)
    {
        try { long id = Native.FontFind(family, bold ? 1 : 0); return id > 0 ? (int)id : -1; }
        catch (Exception) { return -1; }
    }

    /// <summary>Width of text in face <paramref name="id"/> at <paramref name="px"/>; a guess off the device.</summary>
    public static int MeasureIn(int id, int px, string text)
    {
        if (text.Length == 0) return 0;
        long m = -1;
        try { m = Native.FontMeasure(id, px, text); } catch (Exception) { }
        return m >= 0 ? (int)m : text.Length * px * 11 / 20;
    }

    /// <summary>(ascent, descent, line height) of face <paramref name="id"/> at <paramref name="px"/>.</summary>
    public static (int Ascent, int Descent, int Line) MetricsOf(int id, int px)
    {
        long m = -1;
        try { m = Native.FontMetrics(id, px); } catch (Exception) { }
        if (m < 0) return (px * 4 / 5, px / 5, px * 6 / 5);
        return ((int)(m & 0xFFFF), (int)((m >> 16) & 0xFFFF), (int)(m >> 32));
    }

    /// <summary>Installed faces as (id, family, bold, monospace, file); empty off the device.</summary>
    public static List<(int Id, string Family, bool Bold, bool Mono, string File)> Installed()
    {
        var list = new List<(int, string, bool, bool, string)>();
        long n = 0;
        try { n = Native.FontCount(); } catch (Exception) { return list; }
        for (int id = 1; id <= n; id++)
        {
            string s = Native.FontInfo(id);
            if (s.Length == 0) continue;
            var f = s.Split('\t');
            if (f.Length < 4) continue;
            list.Add((id, f[0], f[1] == "Bold", f[2] == "1", f[3]));
        }
        return list;
    }
}

/// <summary>Keys as KEY_READ delivers them (navigation keys are private-use characters).</summary>
public static class Keys
{
    public const char Up = '', Down = '', Left = '', Right = '';
    public const char Home = '', End = '', Delete = '', PageUp = '', PageDown = '';
    public const char Enter = '\n', Tab = '\t', Backspace = '\b', Escape = (char)27, Space = ' ';
    public const char Rubout = (char)127;
    /// <summary>F1..F12 (U+F709..U+F714, keyboard::nav_char).</summary>
    public const char F1 = '', F2 = '', F3 = '', F4 = '', F5 = '', F6 = '',
        F7 = '', F8 = '', F9 = '', F10 = '', F11 = '', F12 = '';

    /// <summary>Shift + a navigation key (Up .. PageDown, Delete) arrives 0x20 higher
    /// (U+F720..U+F728, keyboard.rs SHIFT_OFFSET): extend a text selection.</summary>
    public const char ShiftUp = (char)0xF720, ShiftDown = (char)0xF721, ShiftLeft = (char)0xF722, ShiftRight = (char)0xF723,
        ShiftHome = (char)0xF724, ShiftEnd = (char)0xF725;
    /// <summary>Ctrl + a letter arrives as its control character (Ctrl+A = 1 ...).</summary>
    public const char CtrlA = (char)1, CtrlC = (char)3, CtrlV = (char)22, CtrlX = (char)24, CtrlZ = (char)26;

    public static bool IsShifted(char c) => c >= (char)0xF720 && c <= (char)0xF728;
    /// <summary>The plain key of a shifted navigation key (others unchanged).</summary>
    public static char Unshift(char c) => IsShifted(c) ? (char)(c - 0x20) : c;
    public static bool IsNavigation(char c) => c >= '' && c <= '';
    public static bool IsPrintable(char c) => c >= ' ' && c != Rubout && !IsNavigation(c);
}

/// <summary>Monotonic milliseconds.</summary>
public static class Clock
{
    public static long Now => Environment.TickCount64;
}
