using System.Runtime.InteropServices;

namespace Buitenzorg.Runtime.Sys;

/// <summary>
/// Mirror of the Rust <c>bz_abi::ProcInfo</c> (<c>#[repr(C)]</c>, 64 bytes):
/// a process/task descriptor for the PROC_LIST syscall (v0.9 "Serbuk").
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public unsafe struct ProcInfo
{
    /// <summary>Task/process id.</summary>
    public ulong Pid;

    /// <summary>State: one of <see cref="ProcState"/>.</summary>
    public ulong State;

    /// <summary>Accumulated CPU time in timer ticks.</summary>
    public ulong CpuTicks;

    /// <summary>Kind: 0 = kernel task, 1 = user app.</summary>
    public ulong Kind;

    /// <summary>Null-padded ASCII name (32 bytes).</summary>
    public fixed byte Name[32];
}

/// <summary>States for <see cref="ProcInfo.State"/>.</summary>
public static class ProcState
{
    public const ulong Runnable = 0;
    public const ulong Running = 1;
    public const ulong Finished = 2;
}

/// <summary>
/// Mirror of the Rust <c>bz_abi::SysStat</c> (<c>#[repr(C)]</c>, 48 bytes):
/// system resource statistics for the SYS_STAT syscall.
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public struct SysStat
{
    public ulong UptimeTicks;
    public ulong TickHz;
    public ulong HeapUsed;
    public ulong HeapTotal;
    public ulong TaskCount;
    public ulong MemTotalMiB;
}
