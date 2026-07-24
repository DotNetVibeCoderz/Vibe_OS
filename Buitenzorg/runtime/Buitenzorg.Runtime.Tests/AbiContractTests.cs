using System.Runtime.InteropServices;
using Buitenzorg.Runtime.Sys;

namespace Buitenzorg.Runtime.Tests;

/// <summary>
/// Contract tests for the Rust ↔ C# ABI mirror. These mirror the tests in
/// <c>kernel/abi/src/lib.rs</c>; if either side changes, both fail together.
/// </summary>
public class AbiContractTests
{
    [Fact]
    public void SyscallNumbersAreStable()
    {
        Assert.Equal(0ul, SyscallNumbers.AbiVersion);
        Assert.Equal(1ul, SyscallNumbers.DebugWrite);
        Assert.Equal(2ul, SyscallNumbers.Exit);
        Assert.Equal(3ul, SyscallNumbers.Yield);
        Assert.Equal(4ul, SyscallNumbers.Ticks);
        Assert.Equal(5ul, SyscallNumbers.FbInfo);
        Assert.Equal(6ul, SyscallNumbers.WinCreate);
        Assert.Equal(7ul, SyscallNumbers.WinCmd);
        Assert.Equal(8ul, SyscallNumbers.WinPresent);
        Assert.Equal(9ul, SyscallNumbers.KeyRead);
        Assert.Equal(10ul, SyscallNumbers.ProcList);
        Assert.Equal(11ul, SyscallNumbers.ProcKill);
        Assert.Equal(12ul, SyscallNumbers.SysStatCall);
        Assert.Equal(13ul, SyscallNumbers.Mmap);
        Assert.Equal(14ul, SyscallNumbers.Mprotect);
        Assert.Equal(15ul, SyscallNumbers.Munmap);
        Assert.Equal(16ul, SyscallNumbers.ThreadCreate);
        Assert.Equal(17ul, SyscallNumbers.ThreadJoin);
        Assert.Equal(18ul, SyscallNumbers.ThreadExit);
        Assert.Equal(19ul, SyscallNumbers.FutexWait);
        Assert.Equal(20ul, SyscallNumbers.FutexWake);
        Assert.Equal(21ul, SyscallNumbers.ThreadSelf);
        Assert.Equal(22ul, SyscallNumbers.ClockMono);
        Assert.Equal(23ul, SyscallNumbers.AudioStat);
        Assert.Equal(24ul, SyscallNumbers.AudioSetVolume);
        Assert.Equal(25ul, SyscallNumbers.AudioTone);
        Assert.Equal(26ul, SyscallNumbers.AudioPlay);
        Assert.Equal(27ul, SyscallNumbers.PkgList);
        Assert.Equal(28ul, SyscallNumbers.PkgSet);
        Assert.Equal(29ul, SyscallNumbers.FsList);
        Assert.Equal(30ul, SyscallNumbers.FsRead);
        Assert.Equal(31ul, SyscallNumbers.IsInteractive);
        Assert.Equal(32ul, SyscallNumbers.FsWrite);
        Assert.Equal(33ul, SyscallNumbers.ClockRtc);
        Assert.Equal(34ul, SyscallNumbers.NetSocket);
        Assert.Equal(35ul, SyscallNumbers.NetBind);
        Assert.Equal(36ul, SyscallNumbers.NetSend);
        Assert.Equal(37ul, SyscallNumbers.NetRecv);
        Assert.Equal(38ul, SyscallNumbers.NetClose);
        Assert.Equal(39ul, SyscallNumbers.NetInfo);
        Assert.Equal(40ul, SyscallNumbers.Count);
    }

    /// <summary>
    /// ABI freeze gate (v1.0), mirroring <c>abi_v1_is_frozen</c> on the Rust side.
    /// The v1 table is frozen: numbers are append-only and struct layouts may
    /// never change. Any renumbering or field-width change fails here before it
    /// can reach a released image.
    /// </summary>
    [Fact]
    public void AbiV1IsFrozen()
    {
        Assert.Equal(1ul, Abi.Version);
        Assert.Equal(40ul, SyscallNumbers.Count);

        Assert.Equal(56, Marshal.SizeOf<FramebufferInfo>());
        Assert.Equal(48, Marshal.SizeOf<DrawCmd>());
        Assert.Equal(64, Marshal.SizeOf<ProcInfo>());
        Assert.Equal(48, Marshal.SizeOf<SysStat>());
        Assert.Equal(48, Marshal.SizeOf<AudioInfo>());
        Assert.Equal(48, Marshal.SizeOf<PkgInfo>());
        Assert.Equal(32, Marshal.SizeOf<FsEntry>());
        Assert.Equal(48, Marshal.SizeOf<RtcTime>());
        Assert.Equal(16, Marshal.SizeOf<NetDatagram>());
        Assert.Equal(48, Marshal.SizeOf<NetInfo>());

        Assert.Equal(ulong.MaxValue, SyscallErrors.NoSys);
        Assert.Equal(ulong.MaxValue - 1, SyscallErrors.Invalid);
    }

    [Fact]
    public void SocketKindsAreStable()
    {
        Assert.Equal(0ul, SockKind.Dgram);
        Assert.Equal(1ul, SockKind.Stream);
    }

    [Fact]
    public void FramebufferInfoMatchesReprC()
    {
        // Rust: #[repr(C)] struct of 7 × u64 = 56 bytes.
        Assert.Equal(56, Marshal.SizeOf<FramebufferInfo>());
    }

    [Fact]
    public void DrawCmdMatchesReprC()
    {
        // Rust: #[repr(C)] op:u64 + 4×i32 + 2×u32 + 2×u64 = 48 bytes.
        Assert.Equal(48, Marshal.SizeOf<DrawCmd>());
    }

    [Fact]
    public void ProcInfoAndSysStatMatchReprC()
    {
        Assert.Equal(64, Marshal.SizeOf<ProcInfo>()); // 4×u64 + 32
        Assert.Equal(48, Marshal.SizeOf<SysStat>()); // 6×u64
        Assert.Equal(48, Marshal.SizeOf<AudioInfo>()); // 6×u64
        Assert.Equal(48, Marshal.SizeOf<PkgInfo>()); // 24 + 16 + u64
        Assert.Equal(32, Marshal.SizeOf<FsEntry>()); // 24 + u64
    }

    [Fact]
    public void BclStructsMatchReprC()
    {
        Assert.Equal(48, Marshal.SizeOf<RtcTime>()); // 6×u64
        Assert.Equal(16, Marshal.SizeOf<NetDatagram>()); // 4 + u32 + u64
        Assert.Equal(48, Marshal.SizeOf<NetInfo>()); // 8 + 5×u64
        // Field offsets the syscall layer hard-codes.
        Assert.Equal(0, (int)Marshal.OffsetOf<NetDatagram>(nameof(NetDatagram.Port)) - 4);
        Assert.Equal(8, (int)Marshal.OffsetOf<NetDatagram>(nameof(NetDatagram.Length)));
        Assert.Equal(8, (int)Marshal.OffsetOf<NetInfo>(nameof(NetInfo.Up)));
    }

    [Fact]
    public void HostBackendImplementsV1Table()
    {
        var host = new HostSyscalls();
        Assert.Equal(Abi.Version, host.Syscall(SyscallNumbers.AbiVersion, 0, 0, 0));
        Assert.Equal(SyscallErrors.NoSys, host.Syscall(999, 0, 0, 0));
        Assert.Equal(SyscallErrors.Invalid, host.Syscall(SyscallNumbers.DebugWrite, 0, 0, 0));
    }

    [Fact]
    public void DebugWriteRoundTrips()
    {
        var original = Console.Out;
        try
        {
            using var writer = new StringWriter();
            Console.SetOut(writer);
            var written = BzSys.DebugWrite("halo Bogor");
            Assert.Equal((ulong)"halo Bogor".Length, written);
            Assert.Equal("halo Bogor", writer.ToString());
        }
        finally
        {
            Console.SetOut(original);
        }
    }
}
