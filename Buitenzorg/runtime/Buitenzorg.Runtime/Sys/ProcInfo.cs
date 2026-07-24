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

/// <summary>
/// Mirror of the Rust <c>bz_abi::AudioInfo</c> (<c>#[repr(C)]</c>, 48 bytes):
/// audio-device status for the AUDIO_STAT syscall (v0.16 "Panen").
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public struct AudioInfo
{
    /// <summary>1 if a sound card was detected and initialized, else 0.</summary>
    public ulong Present;

    /// <summary>Output sample rate in Hz (48000 for AC'97 without VRA).</summary>
    public ulong SampleRate;

    /// <summary>Number of output channels (2 = stereo).</summary>
    public ulong Channels;

    /// <summary>Bits per sample (16).</summary>
    public ulong Bits;

    /// <summary>Current master volume, 0..=100 percent.</summary>
    public ulong Volume;

    /// <summary>1 if output is muted, else 0.</summary>
    public ulong Muted;
}

/// <summary>
/// Mirror of the Rust <c>bz_abi::PkgInfo</c> (<c>#[repr(C)]</c>, 48 bytes):
/// a package registry entry for the PKG_LIST syscall (v0.16 "Panen").
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public unsafe struct PkgInfo
{
    /// <summary>Null-padded ASCII package name (24 bytes).</summary>
    public fixed byte Name[24];

    /// <summary>Null-padded ASCII category label (16 bytes).</summary>
    public fixed byte Category[16];

    /// <summary>1 if the package is currently installed, else 0.</summary>
    public ulong Installed;
}

/// <summary>
/// Mirror of the Rust <c>bz_abi::FsEntry</c> (<c>#[repr(C)]</c>, 32 bytes):
/// a VFS directory entry for the FS_LIST syscall (v0.16 "Panen").
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public unsafe struct FsEntry
{
    /// <summary>Null-padded ASCII name (24 bytes).</summary>
    public fixed byte Name[24];

    /// <summary>1 if this entry is a directory/mount, else 0 (a file).</summary>
    public ulong IsDir;
}

/// <summary>
/// Mirror of the Rust <c>bz_abi::RtcTime</c> (<c>#[repr(C)]</c>, 48 bytes):
/// wall-clock date/time from the CMOS RTC for the CLOCK_RTC syscall.
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public struct RtcTime
{
    /// <summary>Full year (e.g. 2026).</summary>
    public ulong Year;

    /// <summary>Month, 1..=12.</summary>
    public ulong Month;

    /// <summary>Day of month, 1..=31.</summary>
    public ulong Day;

    /// <summary>Hour, 0..=23.</summary>
    public ulong Hour;

    /// <summary>Minute, 0..=59.</summary>
    public ulong Minute;

    /// <summary>Second, 0..=59.</summary>
    public ulong Second;
}

/// <summary>
/// Mirror of the Rust <c>bz_abi::NetDatagram</c> (<c>#[repr(C)]</c>, 16 bytes):
/// the header for the NET_SEND / NET_RECV syscalls. The payload follows it
/// immediately in the same buffer.
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public unsafe struct NetDatagram
{
    /// <summary>Peer IPv4 address, one octet per byte in network order (a.b.c.d).</summary>
    public fixed byte Addr[4];

    /// <summary>Peer port (host order).</summary>
    public uint Port;

    /// <summary>Payload length in bytes.</summary>
    public ulong Length;
}

/// <summary>
/// Mirror of the Rust <c>bz_abi::NetInfo</c> (<c>#[repr(C)]</c>, 48 bytes):
/// interface address, link state and counters for the NET_INFO syscall.
/// </summary>
[StructLayout(LayoutKind.Sequential, Pack = 8)]
public unsafe struct NetInfo
{
    /// <summary>Local IPv4 address in the first 4 bytes (a.b.c.d), then 4 zero bytes.</summary>
    public fixed byte Addr[8];

    /// <summary>1 if the stack is up, else 0.</summary>
    public ulong Up;

    /// <summary>Datagrams sent.</summary>
    public ulong TxDatagrams;

    /// <summary>Datagrams received and delivered to a socket.</summary>
    public ulong RxDatagrams;

    /// <summary>ICMP echo replies observed.</summary>
    public ulong IcmpReplies;

    /// <summary>ARP replies sent.</summary>
    public ulong ArpReplies;
}
