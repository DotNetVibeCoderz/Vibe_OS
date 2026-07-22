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
        Assert.Equal(13ul, SyscallNumbers.Count);
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
