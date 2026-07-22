using System.Runtime.InteropServices;

namespace Buitenzorg.Runtime.Sys;

/// <summary>
/// Mirror of the Rust <c>bz_abi::FramebufferInfo</c> (<c>#[repr(C)]</c>, 7 × u64,
/// 56 bytes). Field order and sizes must match exactly.
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public struct FramebufferInfo
{
    /// <summary>Physical address of the framebuffer start.</summary>
    public ulong Address;

    /// <summary>Total framebuffer size in bytes.</summary>
    public ulong Size;

    /// <summary>Visible width in pixels.</summary>
    public ulong Width;

    /// <summary>Visible height in pixels.</summary>
    public ulong Height;

    /// <summary>Bytes per row, may exceed Width × BytesPerPixel.</summary>
    public ulong Stride;

    /// <summary>Bytes per pixel.</summary>
    public ulong BytesPerPixel;

    /// <summary>One of the <see cref="Sys.PixelFormat"/> constants.</summary>
    public ulong PixelFormat;
}

/// <summary>Pixel format constants, mirror of <c>bz_abi::pixel_format</c>.</summary>
public static class PixelFormat
{
    public const ulong Rgb = 0;
    public const ulong Bgr = 1;
    public const ulong Gray = 2;
    public const ulong Unknown = 255;
}
