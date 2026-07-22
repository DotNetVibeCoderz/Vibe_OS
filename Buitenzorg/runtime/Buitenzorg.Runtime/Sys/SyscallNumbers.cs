namespace Buitenzorg.Runtime.Sys;

/// <summary>
/// Buitenzorg OS syscall ABI v1 — mirror of the Rust crate <c>kernel/abi</c>
/// (<c>bz-abi</c>). The Rust side is the source of truth; keep both in sync.
/// Numbers are stable once released: append-only, never renumber.
/// </summary>
public static class SyscallNumbers
{
    /// <summary>Query the ABI version implemented by the kernel.</summary>
    public const ulong AbiVersion = 0;

    /// <summary>Write bytes to the kernel debug console. a0 = ptr, a1 = len.</summary>
    public const ulong DebugWrite = 1;

    /// <summary>Terminate the calling task. a0 = exit code.</summary>
    public const ulong Exit = 2;

    /// <summary>Cooperatively yield the CPU.</summary>
    public const ulong Yield = 3;

    /// <summary>Monotonic timer ticks since boot.</summary>
    public const ulong Ticks = 4;

    /// <summary>Fill a <see cref="FramebufferInfo"/> pointed to by a0.</summary>
    public const ulong FbInfo = 5;

    /// <summary>Create a window: a0=title ptr, a1=title len, a2=(w&lt;&lt;32)|h. Returns id.</summary>
    public const ulong WinCreate = 6;

    /// <summary>Execute a <see cref="DrawCmd"/>: a0=window id, a1=ptr to DrawCmd.</summary>
    public const ulong WinCmd = 7;

    /// <summary>Recompose the desktop so the window's canvas becomes visible. a0=id.</summary>
    public const ulong WinPresent = 8;

    /// <summary>Pop one keyboard character (Unicode scalar); 0 when empty.</summary>
    public const ulong KeyRead = 9;

    /// <summary>Fill an array of <see cref="ProcInfo"/>: a0=buffer, a1=max count. Returns count.</summary>
    public const ulong ProcList = 10;

    /// <summary>Terminate a task/process by id: a0=pid. Returns 0 on success.</summary>
    public const ulong ProcKill = 11;

    /// <summary>Fill a <see cref="SysStat"/> pointed to by a0.</summary>
    public const ulong SysStatCall = 12;

    /// <summary>Exclusive upper bound of the v1 table.</summary>
    public const ulong Count = 13;
}

/// <summary>Error results returned in the high range of a syscall result.</summary>
public static class SyscallErrors
{
    /// <summary>Unknown syscall number.</summary>
    public const ulong NoSys = ulong.MaxValue;

    /// <summary>An argument was invalid.</summary>
    public const ulong Invalid = ulong.MaxValue - 1;
}

/// <summary>The ABI major version this library implements.</summary>
public static class Abi
{
    public const ulong Version = 1;
}
