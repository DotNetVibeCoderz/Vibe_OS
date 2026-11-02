// Buitenzorg.App -- the operating-system API for managed C# apps (RustCLR).
//
// A managed app is an ordinary `dotnet build` assembly that references this
// library; CLRHOST.ELF loads both from /disk and implements every
// [DllImport("buitenzorg")] below on top of the Buitenzorg syscalls. The host
// does the marshalling (a managed string becomes UTF-8 bytes, an int[] a
// pixel buffer), because a pointer in RustCLR is a reference into the managed
// heap, not an address the kernel could read.

using System.Runtime.InteropServices;

namespace Buitenzorg.App;

/// <summary>The host's native entry points (CLRHOST.ELF, "pinvoke:buitenzorg!...").</summary>
internal static class Native
{
    [DllImport("buitenzorg")] internal static extern int WinCreate(string title, int width, int height);
    [DllImport("buitenzorg")] internal static extern int WinDraw(int window, int op, int x, int y, int w, int h, int color, string text);
    [DllImport("buitenzorg")] internal static extern int WinBlit(int window, int x, int y, int w, int h, int[] pixels);
    [DllImport("buitenzorg")] internal static extern int WinPresent(int window);
    [DllImport("buitenzorg")] internal static extern int KeyRead();
    [DllImport("buitenzorg")] internal static extern int ScreenWidth();
    [DllImport("buitenzorg")] internal static extern int ScreenHeight();
    [DllImport("buitenzorg")] internal static extern int IsInteractive();
    [DllImport("buitenzorg")] internal static extern long Spawn(string path, string args);
    [DllImport("buitenzorg")] internal static extern long Wait(long pid);
}

/// <summary>Colors as 0xRRGGBB.</summary>
public static class Colors
{
    public const int Black = 0x000000;
    public const int White = 0xFFFFFF;
    public const int Red = 0xE5484D;
    public const int Green = 0x46A758;
    public const int Blue = 0x3E63DD;
    public const int Yellow = 0xF5D90A;
    public const int Gray = 0x8B8D98;
    public const int Leaf = 0x7CC4A4;
    public const int Forest = 0x12301F;

    public static int Rgb(int r, int g, int b) => ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);
}

/// <summary>A window on the Buitenzorg desktop. Draw into it, then <see cref="Present"/>.</summary>
public sealed class Window
{
    // DrawCmd ops (bz-abi draw_op).
    const int FillRectOp = 0, TextOp = 1, ClearOp = 2, LineOp = 3, EllipseOp = 4, FillEllipseOp = 5, RectOp = 6;

    public int Id { get; }
    public int Width { get; }
    public int Height { get; }

    public Window(string title, int width, int height)
    {
        Id = Native.WinCreate(title, width, height);
        if (Id == 0)
            throw new InvalidOperationException("Buitenzorg: the window could not be created (no desktop?)");
        Width = width;
        Height = height;
    }

    void Draw(int op, int x, int y, int w, int h, int color, string text = "")
    {
        if (Native.WinDraw(Id, op, x, y, w, h, color, text) != 0)
            throw new InvalidOperationException("Buitenzorg: draw command refused");
    }

    public void Clear(int color) => Draw(ClearOp, 0, 0, 0, 0, color);
    public void FillRect(int x, int y, int w, int h, int color) => Draw(FillRectOp, x, y, w, h, color);
    public void DrawRect(int x, int y, int w, int h, int color) => Draw(RectOp, x, y, w, h, color);
    public void DrawLine(int x0, int y0, int x1, int y1, int color) => Draw(LineOp, x0, y0, x1 - x0, y1 - y0, color);
    public void DrawEllipse(int x, int y, int w, int h, int color) => Draw(EllipseOp, x, y, w, h, color);
    public void FillEllipse(int x, int y, int w, int h, int color) => Draw(FillEllipseOp, x, y, w, h, color);
    public void DrawText(int x, int y, string text, int color) => Draw(TextOp, x, y, 0, 0, color, text);

    /// <summary>Copy a w*h block of 0xAARRGGBB pixels into the window at (x, y).</summary>
    public void Blit(int x, int y, int w, int h, int[] pixels)
    {
        if (pixels.Length < w * h)
            throw new ArgumentException("pixel buffer smaller than w*h", nameof(pixels));
        if (Native.WinBlit(Id, x, y, w, h, pixels) != 0)
            throw new InvalidOperationException("Buitenzorg: blit refused");
    }

    /// <summary>Recompose the desktop so what was drawn becomes visible.</summary>
    public void Present() => Native.WinPresent(Id);
}

/// <summary>The keyboard (keys reach the app whose window has the focus).</summary>
public static class Keyboard
{
    public const char Escape = (char)27;

    /// <summary>One key, or '\0' when none is waiting.</summary>
    public static char Read() => (char)Native.KeyRead();
}

/// <summary>The display.</summary>
public static class Screen
{
    public static int Width => Native.ScreenWidth();
    public static int Height => Native.ScreenHeight();
}

/// <summary>The session the app runs in.</summary>
public static class Session
{
    /// <summary>
    /// True once the desktop is live (an app started from the shell or the
    /// Start menu); false while the boot self-tests run, when an app should
    /// draw, report and exit rather than wait for a keyboard.
    /// </summary>
    public static bool IsInteractive => Native.IsInteractive() != 0;
}

/// <summary>Other programs.</summary>
public static class Programs
{
    /// <summary>Start a program (an app name, or a /disk path to an ELF or a .DLL); its pid.</summary>
    public static long Start(string path, string args = "")
    {
        long pid = Native.Spawn(path, args);
        if (pid <= 0)
            throw new InvalidOperationException("Buitenzorg: could not start " + path);
        return pid;
    }

    /// <summary>Wait for a program to exit; its exit code.</summary>
    public static long WaitForExit(long pid) => Native.Wait(pid);
}
