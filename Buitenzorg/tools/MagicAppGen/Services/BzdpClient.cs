using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.IO.Hashing;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicAppGen.Services;

/// <summary>BZDP v1 operation codes. Replies carry <c>op | 0x80</c>.</summary>
public enum BzdpOp : byte
{
    Hello = 1, Begin = 2, Data = 3, End = 4, Run = 5, Log = 6, Kill = 7, List = 8,
    Read = 9, Exec = 10, Output = 11,
}

/// <summary>The status byte that starts every reply payload.</summary>
public enum BzdpStatus : byte
{
    Ok = 0, BadCode = 1, BadRequest = 2, IoError = 3, NotFound = 4, Busy = 5,
}

/// <summary>A device answered, but not with "ok".</summary>
public sealed class BzdpException : Exception
{
    public BzdpStatus Status { get; }
    public BzdpException(BzdpStatus status, string message) : base(message) => Status = status;
}

/// <summary>No reply after every retry.</summary>
public sealed class BzdpTimeoutException : Exception
{
    public BzdpTimeoutException(string message) : base(message) { }
}

/// <summary>What HELLO tells us about the device.</summary>
public sealed record BzdpDeviceInfo(string Text, string Arch, string Hostname, int MaxChunk);

/// <summary>Upload progress: bytes acknowledged by the device out of the total.</summary>
public readonly record struct BzdpProgress(string RemotePath, long Sent, long Total);

/// <summary>
/// Client of the Buitenzorg Deploy Protocol v1: UDP, little-endian,
/// stop-and-wait. Each request is one datagram; the client waits up to
/// <see cref="ReplyTimeout"/> for a reply with the same sequence number and
/// resends it up to <see cref="Attempts"/> times in total.
///
/// <code>
/// datagram = "BZDP" | u8 version=1 | u8 op | u32 seq | u16 code | payload
/// reply    = same header with op|0x80, code 0; payload starts with u8 status
/// </code>
/// </summary>
public sealed class BzdpClient : IDisposable
{
    public const int DefaultPort = 7070;
    public const byte Version = 1;
    public const int HeaderSize = 12;
    static readonly byte[] Magic = "BZDP"u8.ToArray();

    readonly UdpClient? _udp;
    readonly IPEndPoint? _device;
    /// <summary>Stream transports (TCP, serial): opens the byte stream; null = UDP.</summary>
    readonly Func<Stream>? _open;
    Stream? _stream;
    readonly SemaphoreSlim _gate = new(1, 1);
    uint _seq;

    public string Host { get; }
    public int Port { get; }
    public ushort Code { get; }
    public TimeSpan ReplyTimeout { get; init; } = TimeSpan.FromMilliseconds(500);
    public int Attempts { get; init; } = 6;
    /// <summary>Payload size per DATA, until HELLO reports the device's own limit.</summary>
    public int MaxChunk { get; private set; } = 1024;
    /// <summary>Diagnostic sink (retries, resends); may be null.</summary>
    public Action<string>? Trace { get; init; }

    /// <summary>A device over UDP (`host`), or over TCP when `host` is
    /// `tcp://ADDRESS` -- the same packets, framed (see <see cref="Frame"/>).</summary>
    public BzdpClient(string host, int port = DefaultPort, int code = 0)
    {
        if (code is < 0 or > 9999) throw new ArgumentOutOfRangeException(nameof(code), "pairing code is 0000-9999");
        Host = host;
        Port = port;
        Code = (ushort)code;
        _seq = (uint)Random.Shared.Next(1, int.MaxValue);
        if (host.StartsWith("tcp://", StringComparison.OrdinalIgnoreCase))
        {
            string h = host[6..];
            _open = () =>
            {
                var c = new TcpClient { NoDelay = true };
                c.Connect(h, port);
                return c.GetStream();
            };
            return;
        }
        var addr = IPAddress.TryParse(host, out var ip)
            ? ip
            : Array.Find(Dns.GetHostAddresses(host), a => a.AddressFamily == AddressFamily.InterNetwork)
              ?? throw new ArgumentException($"cannot resolve '{host}' to an IPv4 address");
        _device = new IPEndPoint(addr, port);
        _udp = new UdpClient(AddressFamily.InterNetwork);
        _udp.Client.Bind(new IPEndPoint(IPAddress.Any, 0));
        // Windows reports an ICMP "port unreachable" for an earlier datagram as an
        // exception on the next receive; switch that off so a stale error cannot
        // eat a retry (SIO_UDP_CONNRESET = 0x9800000C).
        if (OperatingSystem.IsWindows())
            try { _udp.Client.IOControl(unchecked((int)0x9800000C), new byte[] { 0 }, null); } catch { }
    }

    /// <summary>A device over any byte stream (a serial port): `open` connects.</summary>
    public BzdpClient(string name, Func<Stream> open, int code)
    {
        if (code is < 0 or > 9999) throw new ArgumentOutOfRangeException(nameof(code), "pairing code is 0000-9999");
        Host = name;
        Port = 0;
        Code = (ushort)code;
        _open = open;
        _seq = (uint)Random.Shared.Next(1, int.MaxValue);
    }

    public void Dispose() { _udp?.Dispose(); _stream?.Dispose(); _gate.Dispose(); }

    /// <summary>Stream framing (TCP and serial): "BZ" | length u16 | packet | CRC-32 of the packet.</summary>
    public static byte[] Frame(ReadOnlySpan<byte> packet)
    {
        var f = new byte[4 + packet.Length + 4];
        f[0] = (byte)'B'; f[1] = (byte)'Z';
        BinaryPrimitives.WriteUInt16LittleEndian(f.AsSpan(2), (ushort)packet.Length);
        packet.CopyTo(f.AsSpan(4));
        BinaryPrimitives.WriteUInt32LittleEndian(f.AsSpan(4 + packet.Length), System.IO.Hashing.Crc32.HashToUInt32(packet));
        return f;
    }

    /// <summary>Read one frame from `s` (skipping noise before "BZ"); null when the
    /// stream ended. A frame with a wrong CRC is dropped (returns an empty array).</summary>
    public static async Task<byte[]?> ReadFrameAsync(Stream s, CancellationToken ct)
    {
        var one = new byte[1];
        async Task<bool> Byte() => await s.ReadAsync(one, ct) == 1;
        int prev = -1;
        while (true)
        {
            if (!await Byte()) return null;
            if (prev == 'B' && one[0] == 'Z') break;
            prev = one[0];
        }
        var head = new byte[2];
        await s.ReadExactlyAsync(head, ct);
        int len = BinaryPrimitives.ReadUInt16LittleEndian(head);
        var body = new byte[len + 4];
        await s.ReadExactlyAsync(body, ct);
        uint crc = BinaryPrimitives.ReadUInt32LittleEndian(body.AsSpan(len));
        if (crc != System.IO.Hashing.Crc32.HashToUInt32(body.AsSpan(0, len))) return Array.Empty<byte>();
        return body[..len];
    }

    // ---- operations ---------------------------------------------------------

    public async Task<BzdpDeviceInfo> HelloAsync(CancellationToken ct = default)
    {
        var data = await RequestAsync(BzdpOp.Hello, ReadOnlyMemory<byte>.Empty, ct);
        var text = Encoding.UTF8.GetString(data.Span);
        var info = ParseHello(text);
        if (info.MaxChunk > 0) MaxChunk = Math.Min(info.MaxChunk, 60_000);
        return info;
    }

    /// <summary>Parse "Buitenzorg OS &lt;arch&gt; &lt;hostname&gt;; max_chunk=&lt;n&gt;".</summary>
    public static BzdpDeviceInfo ParseHello(string text)
    {
        string head = text, arch = "?", host = "?";
        int maxChunk = 1024;
        int semi = text.IndexOf(';');
        if (semi >= 0)
        {
            head = text[..semi];
            foreach (var part in text[(semi + 1)..].Split(';', StringSplitOptions.TrimEntries))
                if (part.StartsWith("max_chunk=", StringComparison.Ordinal) &&
                    int.TryParse(part["max_chunk=".Length..], out var n) && n > 0)
                    maxChunk = n;
        }
        var words = head.Trim().Split(' ', StringSplitOptions.RemoveEmptyEntries);
        // "Buitenzorg" "OS" arch hostname
        if (words.Length >= 3) arch = words[2];
        if (words.Length >= 4) host = string.Join(' ', words, 3, words.Length - 3);
        return new BzdpDeviceInfo(text, arch, host, maxChunk);
    }

    /// <summary>Upload one file: BEGIN, DATA until the device has every byte, END with the CRC-32.</summary>
    public async Task UploadAsync(string remotePath, byte[] content, IProgress<BzdpProgress>? progress = null,
                                  CancellationToken ct = default)
    {
        var path = Encoding.UTF8.GetBytes(remotePath);
        if (path.Length > ushort.MaxValue) throw new ArgumentException("path too long", nameof(remotePath));
        var begin = new byte[4 + 2 + path.Length];
        BinaryPrimitives.WriteUInt32LittleEndian(begin, (uint)content.Length);
        BinaryPrimitives.WriteUInt16LittleEndian(begin.AsSpan(4), (ushort)path.Length);
        path.CopyTo(begin, 6);
        await RequestAsync(BzdpOp.Begin, begin, ct);
        progress?.Report(new BzdpProgress(remotePath, 0, content.Length));

        uint offset = 0;
        int stalls = 0;
        while (offset < content.Length)
        {
            int n = Math.Min(MaxChunk, content.Length - (int)offset);
            var msg = new byte[4 + n];
            BinaryPrimitives.WriteUInt32LittleEndian(msg, offset);
            Buffer.BlockCopy(content, (int)offset, msg, 4, n);
            var reply = await RequestAsync(BzdpOp.Data, msg, ct);
            if (reply.Length < 4) throw new BzdpException(BzdpStatus.BadRequest, "DATA reply without next_offset");
            uint next = BinaryPrimitives.ReadUInt32LittleEndian(reply.Span);
            if (next > content.Length) throw new BzdpException(BzdpStatus.BadRequest, $"device reports offset {next} past the end ({content.Length})");
            if (next <= offset)
            {
                // The device did not take this chunk (e.g. an earlier one went missing):
                // resend from where it says it is, but never loop for ever.
                if (++stalls > Attempts * 4) throw new BzdpException(BzdpStatus.IoError, $"upload stuck at byte {next}");
                Trace?.Invoke($"[bzdp] device at {next}, resending from there");
            }
            else stalls = 0;
            offset = next;
            progress?.Report(new BzdpProgress(remotePath, offset, content.Length));
        }

        // CRC-32 for damaged transfers, SHA-256 for a file that is not the one sent
        // (a device older than the SHA check reads only the first four bytes)
        var end = new byte[4 + 32];
        BinaryPrimitives.WriteUInt32LittleEndian(end, Crc32.HashToUInt32(content));
        System.Security.Cryptography.SHA256.HashData(content).CopyTo(end, 4);
        await RequestAsync(BzdpOp.End, end, ct);
    }

    /// <summary>Run a shell command line on the device. Returns the pid, or 0 when it ran synchronously.</summary>
    public async Task<uint> RunAsync(string commandLine, CancellationToken ct = default)
    {
        var reply = await RequestAsync(BzdpOp.Run, Encoding.UTF8.GetBytes(commandLine), ct);
        return reply.Length >= 4 ? BinaryPrimitives.ReadUInt32LittleEndian(reply.Span) : 0;
    }

    /// <summary>Read device console output from <paramref name="cursor"/>. Returns the next cursor and the text.</summary>
    public async Task<(uint Next, string Text)> LogAsync(uint cursor, CancellationToken ct = default)
    {
        var msg = new byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(msg, cursor);
        var reply = await RequestAsync(BzdpOp.Log, msg, ct);
        if (reply.Length < 4) throw new BzdpException(BzdpStatus.BadRequest, "LOG reply without next_cursor");
        uint next = BinaryPrimitives.ReadUInt32LittleEndian(reply.Span);
        return (next, Encoding.UTF8.GetString(reply.Span[4..]));
    }

    public async Task KillAsync(uint pid, CancellationToken ct = default)
    {
        var msg = new byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(msg, pid);
        await RequestAsync(BzdpOp.Kill, msg, ct);
    }

    public async Task<IReadOnlyList<string>> ListAsync(string path, CancellationToken ct = default)
    {
        var reply = await RequestAsync(BzdpOp.List, Encoding.UTF8.GetBytes(path), ct);
        return Encoding.UTF8.GetString(reply.Span).Split('\n', StringSplitOptions.RemoveEmptyEntries);
    }

    /// <summary>Download a file from the device (READ, chunk by chunk).</summary>
    public async Task<byte[]> ReadAsync(string remotePath, IProgress<BzdpProgress>? progress = null,
                                        CancellationToken ct = default)
    {
        var path = Encoding.UTF8.GetBytes(remotePath);
        var buf = new List<byte>();
        long total = -1;
        while (total < 0 || buf.Count < total)
        {
            var msg = new byte[4 + path.Length];
            BinaryPrimitives.WriteUInt32LittleEndian(msg, (uint)buf.Count);
            path.CopyTo(msg, 4);
            var reply = await RequestAsync(BzdpOp.Read, msg, ct);
            if (reply.Length < 4) throw new BzdpException(BzdpStatus.BadRequest, "READ reply without a size");
            total = BinaryPrimitives.ReadUInt32LittleEndian(reply.Span);
            var chunk = reply.Span[4..];
            if (chunk.Length == 0 && buf.Count < total) throw new BzdpException(BzdpStatus.IoError, $"READ stopped at byte {buf.Count} of {total}");
            buf.AddRange(chunk.ToArray());
            progress?.Report(new BzdpProgress(remotePath, buf.Count, total));
        }
        return buf.ToArray();
    }

    /// <summary>Run a shell command line on the device and return what it printed
    /// (EXEC, then OUTPUT for the rest) and whether the command failed.</summary>
    public async Task<(bool Failed, string Output)> ExecAsync(string commandLine, CancellationToken ct = default)
    {
        var reply = await RequestAsync(BzdpOp.Exec, Encoding.UTF8.GetBytes(commandLine), ct);
        if (reply.Length < 5) throw new BzdpException(BzdpStatus.BadRequest, "EXEC reply too short");
        bool failed = reply.Span[0] != 0;
        int total = (int)BinaryPrimitives.ReadUInt32LittleEndian(reply.Span[1..]);
        var buf = new List<byte>(reply.Span[5..].ToArray());
        while (buf.Count < total)
        {
            var msg = new byte[4];
            BinaryPrimitives.WriteUInt32LittleEndian(msg, (uint)buf.Count);
            var more = await RequestAsync(BzdpOp.Output, msg, ct);
            if (more.Length <= 4) break;
            buf.AddRange(more.Span[4..].ToArray());
        }
        return (failed, Encoding.UTF8.GetString(buf.ToArray()));
    }

    // ---- transport ----------------------------------------------------------

    public static byte[] Encode(BzdpOp op, uint seq, ushort code, ReadOnlySpan<byte> payload, bool reply = false)
    {
        var d = new byte[HeaderSize + payload.Length];
        Magic.CopyTo(d, 0);
        d[4] = Version;
        d[5] = (byte)((byte)op | (reply ? 0x80 : 0));
        BinaryPrimitives.WriteUInt32LittleEndian(d.AsSpan(6), seq);
        BinaryPrimitives.WriteUInt16LittleEndian(d.AsSpan(10), code);
        payload.CopyTo(d.AsSpan(HeaderSize));
        return d;
    }

    /// <summary>Decode a header; false for anything that is not a BZDP v1 datagram.</summary>
    public static bool TryDecode(ReadOnlySpan<byte> d, out byte op, out uint seq, out ushort code)
    {
        op = 0; seq = 0; code = 0;
        if (d.Length < HeaderSize || !d[..4].SequenceEqual(Magic) || d[4] != Version) return false;
        op = d[5];
        seq = BinaryPrimitives.ReadUInt32LittleEndian(d[6..]);
        code = BinaryPrimitives.ReadUInt16LittleEndian(d[10..]);
        return true;
    }

    /// <summary>Send one request and wait for its reply; returns the reply data after the status byte.</summary>
    async Task<ReadOnlyMemory<byte>> RequestAsync(BzdpOp op, ReadOnlyMemory<byte> payload, CancellationToken ct)
    {
        await _gate.WaitAsync(ct);
        try
        {
            uint seq = unchecked(++_seq);
            var datagram = Encode(op, seq, Code, payload.Span);
            if (_open != null) return await StreamRequestAsync(op, seq, datagram, ct);
            for (int attempt = 1; attempt <= Attempts; attempt++)
            {
                await _udp!.SendAsync(datagram, _device!, ct);
                var deadline = DateTime.UtcNow + ReplyTimeout;
                while (true)
                {
                    var left = deadline - DateTime.UtcNow;
                    if (left <= TimeSpan.Zero) break;
                    using var wait = CancellationTokenSource.CreateLinkedTokenSource(ct);
                    wait.CancelAfter(left);
                    UdpReceiveResult r;
                    try { r = await _udp.ReceiveAsync(wait.Token); }
                    catch (OperationCanceledException) when (!ct.IsCancellationRequested) { break; }
                    catch (SocketException) { break; } // e.g. ICMP port unreachable on Windows: treat as no reply
                    if (!TryDecode(r.Buffer, out var rop, out var rseq, out _)) continue;
                    if (rseq != seq || rop != ((byte)op | 0x80)) continue; // a late reply to an earlier request
                    if (r.Buffer.Length < HeaderSize + 1) throw new BzdpException(BzdpStatus.BadRequest, $"{op}: reply without a status byte");
                    var status = (BzdpStatus)r.Buffer[HeaderSize];
                    if (status != BzdpStatus.Ok) throw new BzdpException(status, Describe(op, status));
                    return r.Buffer.AsMemory(HeaderSize + 1);
                }
                if (attempt < Attempts) Trace?.Invoke($"[bzdp] {op} seq {seq}: no reply, retry {attempt}/{Attempts - 1}");
            }
            throw new BzdpTimeoutException(
                $"device not reachable at {Host}:{Port} ({op} unanswered after {Attempts} tries): " +
                $"is `deploy on` enabled and port {Port} forwarded?");
        }
        finally { _gate.Release(); }
    }

    /// <summary>One request over the stream transport: a lost connection is
    /// reopened and the request resent (the device answers a repeated seq from
    /// its saved reply, so nothing runs twice).</summary>
    async Task<ReadOnlyMemory<byte>> StreamRequestAsync(BzdpOp op, uint seq, byte[] packet, CancellationToken ct)
    {
        var frame = Frame(packet);
        for (int attempt = 1; attempt <= Math.Max(2, Attempts / 2); attempt++)
        {
            try
            {
                _stream ??= _open!();
                await _stream.WriteAsync(frame, ct);
                await _stream.FlushAsync(ct);
                var deadline = DateTime.UtcNow + ReplyTimeout * 4;
                while (DateTime.UtcNow < deadline)
                {
                    using var wait = CancellationTokenSource.CreateLinkedTokenSource(ct);
                    wait.CancelAfter(deadline - DateTime.UtcNow);
                    var r = await ReadFrameAsync(_stream, wait.Token) ?? throw new IOException("connection closed");
                    if (!TryDecode(r, out var rop, out var rseq, out _) || rseq != seq || rop != ((byte)op | 0x80)) continue;
                    if (r.Length < HeaderSize + 1) throw new BzdpException(BzdpStatus.BadRequest, $"{op}: reply without a status byte");
                    var status = (BzdpStatus)r[HeaderSize];
                    if (status != BzdpStatus.Ok) throw new BzdpException(status, Describe(op, status));
                    return r.AsMemory(HeaderSize + 1);
                }
            }
            catch (Exception e) when (e is IOException or SocketException or OperationCanceledException or TimeoutException && !ct.IsCancellationRequested)
            {
                Trace?.Invoke($"[bzdp] {op} seq {seq}: {e.Message}, reconnect {attempt}");
            }
            _stream?.Dispose();
            _stream = null;
        }
        throw new BzdpTimeoutException($"device not reachable at {Host}{(Port > 0 ? ":" + Port : "")} ({op} unanswered): is `deploy on` enabled?");
    }

    public static string Describe(BzdpOp op, BzdpStatus s) => s switch
    {
        BzdpStatus.BadCode => "the device rejected the pairing code (check the code shown by `deploy on`)",
        BzdpStatus.BadRequest => $"the device rejected the {op} request as malformed",
        BzdpStatus.IoError => op == BzdpOp.End
            ? "the device could not write the file (CRC mismatch or a full/read-only disk)"
            : $"I/O error on the device during {op}",
        BzdpStatus.NotFound => op == BzdpOp.Run ? "the device could not find that program" : $"{op}: not found on the device",
        BzdpStatus.Busy => "the device is busy; try again in a moment",
        _ => $"{op}: status {(byte)s}",
    };
}
