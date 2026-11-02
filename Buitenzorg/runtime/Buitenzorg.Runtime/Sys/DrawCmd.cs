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
    /// <summary>Filled rounded rectangle; radius in the reserved field.</summary>
    public const ulong FillRoundRect = 8;
    /// <summary>Rounded rectangle outline; radius in the reserved field.</summary>
    public const ulong RoundRect = 9;
    /// <summary>Vertical gradient: color (top) to the reserved field (bottom).</summary>
    public const ulong GradientV = 10;
    /// <summary>Rounded vertical gradient: color top, reserved field bottom, radius in TextLen.</summary>
    public const ulong FillRoundGradientV = 11;
    /// <summary>Text in a style: reserved field 0 body, 1 strong, 2 heading.</summary>
    public const ulong DrawTextStyled = 12;
    /// <summary>Translucent rounded rectangle: reserved field = alpha &lt;&lt; 16 | radius.</summary>
    public const ulong BlendRoundRect = 13;
}
