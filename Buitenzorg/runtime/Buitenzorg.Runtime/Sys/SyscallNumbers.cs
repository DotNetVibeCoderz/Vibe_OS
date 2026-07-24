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

    /// <summary>Map anonymous user pages (v0.15 "Matang" PAL): a0=size, a1=prot. Returns base VA.</summary>
    public const ulong Mmap = 13;

    /// <summary>Change protection: a0=addr, a1=size, a2=prot. Returns 0.</summary>
    public const ulong Mprotect = 14;

    /// <summary>Unmap a user range: a0=addr, a1=size. Returns 0.</summary>
    public const ulong Munmap = 15;

    /// <summary>Spawn a ring-3 thread: a0=entry rip, a1=arg, a2=stack top. Returns thread id.</summary>
    public const ulong ThreadCreate = 16;

    /// <summary>Wait for a thread to finish: a0=thread id. Returns 0.</summary>
    public const ulong ThreadJoin = 17;

    /// <summary>Terminate the calling thread: a0=exit code. Does not return.</summary>
    public const ulong ThreadExit = 18;

    /// <summary>Futex wait: if *a0 == a1, block until woken. Returns 0.</summary>
    public const ulong FutexWait = 19;

    /// <summary>Futex wake: wake up to a1 threads blocked on a0. Returns count woken.</summary>
    public const ulong FutexWake = 20;

    /// <summary>Return the calling thread's id (pthread_self foundation).</summary>
    public const ulong ThreadSelf = 21;

    /// <summary>Monotonic high-resolution counter (CPU timestamp counter).</summary>
    public const ulong ClockMono = 22;

    /// <summary>Fill an <see cref="AudioInfo"/> at a0. Returns 0 on success.</summary>
    public const ulong AudioStat = 23;

    /// <summary>Set the master output volume: a0 = 0..=100 percent. Returns 0.</summary>
    public const ulong AudioSetVolume = 24;

    /// <summary>Play a generated sine tone: a0 = frequency Hz, a1 = duration ms.</summary>
    public const ulong AudioTone = 25;

    /// <summary>Play 16-bit stereo PCM: a0 = samples ptr, a1 = length bytes.</summary>
    public const ulong AudioPlay = 26;

    /// <summary>Fill an array of <see cref="PkgInfo"/>: a0 = buffer, a1 = max. Returns count.</summary>
    public const ulong PkgList = 27;

    /// <summary>Install/remove a package: a0 = name ptr, a1 = len, a2 = action (1=install).</summary>
    public const ulong PkgSet = 28;

    /// <summary>List a VFS directory: a0 = NUL-terminated path, a1 = FsEntry[] ptr, a2 = max. Returns count.</summary>
    public const ulong FsList = 29;

    /// <summary>Read a file's bytes: a0 = NUL-terminated path, a1 = out buffer, a2 = max bytes. Returns bytes read.</summary>
    public const ulong FsRead = 30;

    /// <summary>Returns 1 in an interactive session (desktop up), 0 during headless boot-demo runs. No arguments.</summary>
    public const ulong IsInteractive = 31;

    /// <summary>Write a file's bytes: a0 = NUL-terminated path, a1 = source buffer, a2 = byte count. Returns bytes written.</summary>
    public const ulong FsWrite = 32;

    /// <summary>Read the CMOS real-time clock: a0 = out ptr to an <see cref="RtcTime"/>. Returns 0 on success.</summary>
    public const ulong ClockRtc = 33;

    /// <summary>Create a socket: a0 = kind (see <see cref="SockKind"/>). Returns a handle (>= 1), or 0 on failure.</summary>
    public const ulong NetSocket = 34;

    /// <summary>Bind a socket to a local port: a0 = handle, a1 = port. Returns 0 on success.</summary>
    public const ulong NetBind = 35;

    /// <summary>Send a datagram: a0 = handle, a1 = <see cref="NetDatagram"/> header + payload, a2 = payload length.</summary>
    public const ulong NetSend = 36;

    /// <summary>Receive a datagram (non-blocking): a0 = handle, a1 = header + room for payload, a2 = max bytes.</summary>
    public const ulong NetRecv = 37;

    /// <summary>Close a socket: a0 = handle. Returns 0 on success.</summary>
    public const ulong NetClose = 38;

    /// <summary>Interface info and counters: a0 = out ptr to a <see cref="NetInfo"/>. Returns 0 on success.</summary>
    public const ulong NetInfo = 39;

    /// <summary>Exclusive upper bound of the v1 table.</summary>
    public const ulong Count = 40;
}

/// <summary>Socket kinds for <see cref="SyscallNumbers.NetSocket"/>. Only UDP is implemented;
/// <see cref="Stream"/> is reserved for the TCP work that System.Net.Http needs.</summary>
public static class SockKind
{
    /// <summary>Connectionless datagrams (UDP).</summary>
    public const ulong Dgram = 0;

    /// <summary>Reserved for TCP; not implemented yet.</summary>
    public const ulong Stream = 1;
}

/// <summary>Protection flags for <see cref="SyscallNumbers.Mmap"/> / <see cref="SyscallNumbers.Mprotect"/>.</summary>
public static class MmapProt
{
    /// <summary>No access (reserve only).</summary>
    public const ulong None = 0;
    /// <summary>Readable.</summary>
    public const ulong Read = 1;
    /// <summary>Writable.</summary>
    public const ulong Write = 2;
    /// <summary>Executable.</summary>
    public const ulong Exec = 4;
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
