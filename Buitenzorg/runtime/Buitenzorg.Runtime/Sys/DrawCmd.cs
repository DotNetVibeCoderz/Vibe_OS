using System.Runtime.InteropServices;

namespace Buitenzorg.Runtime.Sys;

/// <summary>
/// Mirror of the Rust <c>bz_abi::DrawCmd</c> (<c>#[repr(C)]</c>, 48 bytes):
/// the drawing operation payload for the WIN_CMD syscall (v0.8 UI ABI).
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public struct DrawCmd
{
    /// <summary>One of the <see cref="DrawOp"/> constants.</summary>
    public ulong Op;

    /// <summary>X coordinate in window-client pixels.</summary>
    public int X;

    /// <summary>Y coordinate in window-client pixels.</summary>
    public int Y;

    /// <summary>Width in pixels (fill_rect).</summary>
    public int W;

    /// <summary>Height in pixels (fill_rect).</summary>
    public int H;

    /// <summary>Color as 0x00RRGGBB.</summary>
    public uint Color;

    /// <summary>Reserved; keeps 8-byte alignment for the pointer below.</summary>
    public uint Pad;

    /// <summary>UTF-8 text pointer (draw_text), else 0.</summary>
    public ulong TextPtr;

    /// <summary>UTF-8 text length in bytes (draw_text), else 0.</summary>
    public ulong TextLen;
}

/// <summary>Operations for <see cref="DrawCmd.Op"/>.</summary>
public static class DrawOp
{
    public const ulong FillRect = 0;
    public const ulong DrawText = 1;
    public const ulong Clear = 2;
    public const ulong Line = 3;
    public const ulong Ellipse = 4;
    public const ulong FillEllipse = 5;
    public const ulong Rect = 6;
    public const ulong Blit = 7;
}
