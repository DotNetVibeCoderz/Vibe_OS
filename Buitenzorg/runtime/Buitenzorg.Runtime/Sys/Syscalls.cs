using System.Runtime.InteropServices;
using System.Text;

namespace Buitenzorg.Runtime.Sys;

/// <summary>
/// Uniform entry point for Buitenzorg syscalls from managed code.
/// On a real Buitenzorg target this dispatches into the kernel's FFI shim
/// (<c>bzsys</c>, C ABI — requirements.md §4); on a development host it falls
/// back to <see cref="HostSyscalls"/> so apps and tests run anywhere.
/// </summary>
public interface ISyscallBackend
{
    ulong Syscall(ulong number, ulong a0, ulong a1, ulong a2);
}

/// <summary>
/// P/Invoke binding to the kernel FFI shim. Only usable when running on
/// Buitenzorg OS itself (the <c>bzsys</c> native library is provided by the
/// managed-runtime bring-up, roadmap v0.4 "Tunas").
/// </summary>
public sealed partial class NativeSyscalls : ISyscallBackend
{
    [LibraryImport("bzsys", EntryPoint = "bz_syscall")]
    private static partial ulong BzSyscall(ulong number, ulong a0, ulong a1, ulong a2);

    public ulong Syscall(ulong number, ulong a0, ulong a1, ulong a2)
        => BzSyscall(number, a0, a1, a2);
}

/// <summary>
/// Host simulation backend: implements the v1 syscall table on top of the
/// host OS so Buitenzorg apps can be developed and tested before the managed
/// runtime runs on bare metal.
/// </summary>
public sealed class HostSyscalls : ISyscallBackend
{
    private readonly long _bootTimestamp = Environment.TickCount64;

    public unsafe ulong Syscall(ulong number, ulong a0, ulong a1, ulong a2) => number switch
    {
        SyscallNumbers.AbiVersion => Abi.Version,
        SyscallNumbers.DebugWrite => DebugWrite(a0, a1),
        SyscallNumbers.Exit => ExitHost(a0),
        SyscallNumbers.Yield => YieldHost(),
        // PIT default rate on target is ~18.2 Hz; emulate the same unit.
        SyscallNumbers.Ticks => (ulong)((Environment.TickCount64 - _bootTimestamp) * 182 / 10000),
        SyscallNumbers.FbInfo => SyscallErrors.Invalid, // no framebuffer on host
        _ => SyscallErrors.NoSys,
    };

    private static unsafe ulong DebugWrite(ulong ptr, ulong len)
    {
        if (ptr == 0 || len == 0)
        {
            return SyscallErrors.Invalid;
        }
        var span = new ReadOnlySpan<byte>((void*)ptr, checked((int)len));
        Console.Out.Write(Encoding.UTF8.GetString(span));
        return len;
    }

    private static ulong ExitHost(ulong code)
    {
        Environment.Exit((int)code);
        return 0;
    }

    private static ulong YieldHost()
    {
        Thread.Yield();
        return 0;
    }
}

/// <summary>
/// Static facade apps use. Selects the native backend when running on
/// Buitenzorg OS, the host simulation otherwise.
/// </summary>
public static class BzSys
{
    private static ISyscallBackend _backend = CreateDefaultBackend();

    /// <summary>True when running on Buitenzorg OS itself.</summary>
    public static bool IsOnBuitenzorg =>
        OperatingSystem.IsOSPlatform("buitenzorg");

    public static ISyscallBackend Backend
    {
        get => _backend;
        set => _backend = value ?? throw new ArgumentNullException(nameof(value));
    }

    private static ISyscallBackend CreateDefaultBackend()
        => IsOnBuitenzorg ? new NativeSyscalls() : new HostSyscalls();

    public static ulong Call(ulong number, ulong a0 = 0, ulong a1 = 0, ulong a2 = 0)
        => _backend.Syscall(number, a0, a1, a2);

    /// <summary>ABI version reported by the kernel (or host simulation).</summary>
    public static ulong AbiVersion() => Call(SyscallNumbers.AbiVersion);

    /// <summary>Monotonic timer ticks since boot.</summary>
    public static ulong Ticks() => Call(SyscallNumbers.Ticks);

    /// <summary>Write UTF-8 text to the kernel debug console.</summary>
    public static unsafe ulong DebugWrite(string text)
    {
        var bytes = Encoding.UTF8.GetBytes(text);
        fixed (byte* p = bytes) // GC-aware pinning (requirements.md §4 rule 5)
        {
            return Call(SyscallNumbers.DebugWrite, (ulong)p, (ulong)bytes.Length);
        }
    }

    /// <summary>Create an app window; returns its id.</summary>
    public static unsafe uint WinCreate(string title, int width, int height)
    {
        var bytes = Encoding.UTF8.GetBytes(title);
        ulong dims = ((ulong)(uint)width << 32) | (uint)height;
        fixed (byte* p = bytes)
        {
            return (uint)Call(SyscallNumbers.WinCreate, (ulong)p, (ulong)bytes.Length, dims);
        }
    }

    /// <summary>Submit a draw command to a window.</summary>
    public static unsafe void WinCmd(uint window, ref DrawCmd cmd)
    {
        fixed (DrawCmd* p = &cmd)
        {
            Call(SyscallNumbers.WinCmd, window, (ulong)p);
        }
    }

    /// <summary>Recompose the desktop so a window's canvas becomes visible.</summary>
    public static void WinPresent(uint window) => Call(SyscallNumbers.WinPresent, window);

    /// <summary>Read one keyboard character; 0 when none is available.</summary>
    public static char KeyRead() => (char)Call(SyscallNumbers.KeyRead);
}
