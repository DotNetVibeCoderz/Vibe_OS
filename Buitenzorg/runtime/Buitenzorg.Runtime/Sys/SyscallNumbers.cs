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

    /// <summary>Start a program as its own process: a0 = path (NUL-terminated app name or VFS path), a1 = arguments (NUL-terminated, 0 = none). Returns the pid.</summary>
    public const ulong ProcSpawn = 40;

    /// <summary>Block until process a0 = pid exits. Returns its exit code (INVAL if unknown).</summary>
    public const ulong ProcWait = 41;

    /// <summary>Copy the caller's argument string into a0 = buffer (max a1 bytes). Returns its full length.</summary>
    public const ulong ProcArgs = 42;

    /// <summary>FUTEX_WAIT with a time limit: block while the u32 at a0 holds a1, at most a2 nanoseconds (0 = no limit). Returns 0 when woken / changed, 1 on timeout.</summary>
    public const ulong FutexWaitTimeout = 43;

    /// <summary>Monotonic nanoseconds since boot. No arguments.</summary>
    public const ulong ClockNanos = 44;

    /// <summary>Next pointer event of a window the caller owns (a0 = window id); 0 = none, else packed: x bits 0-15, y 16-31, buttons 32-39, kind 56-63 (1 move, 2 down, 3 up, 4 leave).</summary>
    public const ulong WinEvent = 45;

    /// <summary>Metrics of a text style (a0 = 0 body, 1 strong, 2 heading): (glyph width &lt;&lt; 32) | line height.</summary>
    public const ulong TextMetrics = 46;

    /// <summary>Board info + built-in sensors (Raspberry Pi 4): a0 = <see cref="HwQuery"/> code, a1 = argument. NOSYS without board I/O.</summary>
    public const ulong HwBoard = 47;

    /// <summary>GPIO: a0 = <see cref="HwGpio"/> operation, a1 = pin, a2 = value.</summary>
    public const ulong HwGpio = 48;

    /// <summary>I2C transfer: a0 = ptr to a <see cref="HwTransfer"/>. Returns a <see cref="HwStatus"/> code.</summary>
    public const ulong HwI2c = 49;

    /// <summary>SPI full-duplex transfer: a0 = ptr to a <see cref="HwTransfer"/>. Returns a <see cref="HwStatus"/> code.</summary>
    public const ulong HwSpi = 50;

    /// <summary>Serial port: a0 = <see cref="HwUart"/> operation, a1 = ptr to a <see cref="HwTransfer"/>. Returns bytes written / read.</summary>
    public const ulong HwUart = 51;

    /// <summary>Built-in LEDs: a0 = 0 activity / 1 power, a1 = 1 on / 0 off.</summary>
    public const ulong HwLed = 52;

    /// <summary>Desktop settings: a0 = <see cref="SettingOp"/>, a1 = <see cref="SettingKey"/> (| index &lt;&lt; 16 for names), a2 = value or 64-byte buffer.</summary>
    public const ulong Setting = 53;

    /// <summary>Exclusive upper bound of the v1 table.</summary>
    public const ulong Count = 54;
}

/// <summary>Operations of <see cref="SyscallNumbers.Setting"/>.</summary>
public static class SettingOp
{
    public const ulong Get = 0;
    public const ulong Set = 1;
    public const ulong Count = 2;
    public const ulong Name = 3;
    public const ulong Description = 4;
}

/// <summary>Keys of <see cref="SyscallNumbers.Setting"/>.</summary>
public static class SettingKey
{
    public const ulong Theme = 0;
    public const ulong Wallpaper = 1;
    public const ulong Screensaver = 2;
    public const ulong ScreensaverDelay = 3;
    public const ulong Animations = 4;
    public const ulong LargeCursor = 5;
}

/// <summary>Queries of <see cref="SyscallNumbers.HwBoard"/>.</summary>
public static class HwQuery
{
    public const ulong Board = 0;
    public const ulong Temperature = 1;
    public const ulong TemperatureMax = 2;
    public const ulong Clock = 3;
    public const ulong Voltage = 4;
    public const ulong Throttled = 5;
    public const ulong Revision = 6;
    public const ulong Serial = 7;
    public const ulong Memory = 8;
    public const ulong GpioCount = 9;
}

/// <summary>Operations of <see cref="SyscallNumbers.HwGpio"/>.</summary>
public static class HwGpio
{
    public const ulong Mode = 0;
    public const ulong Write = 1;
    public const ulong Read = 2;
    public const ulong Pull = 3;
    public const ulong GetMode = 4;
}

/// <summary>Operations of <see cref="SyscallNumbers.HwUart"/>.</summary>
public static class HwUart
{
    public const ulong Open = 0;
    public const ulong Write = 1;
    public const ulong Read = 2;
    public const ulong Close = 3;
}

/// <summary>Results of <see cref="SyscallNumbers.HwI2c"/> / <see cref="SyscallNumbers.HwSpi"/>.</summary>
public static class HwStatus
{
    public const ulong Ok = 0;
    public const ulong Nack = 1;
    public const ulong ClockStretch = 2;
    public const ulong Timeout = 3;
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
