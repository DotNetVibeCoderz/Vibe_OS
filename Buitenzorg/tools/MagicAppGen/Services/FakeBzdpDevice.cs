using System;
using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.IO.Hashing;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicAppGen.Services;

/// <summary>
/// A loopback stand-in for the device side of BZDP v1, so the client can be
/// exercised without Buitenzorg OS: it keeps uploaded files in memory, appends
/// everything "run" to a console ring buffer, and can drop a share of the
/// datagrams to exercise the client's retries. Used by <c>--fake-device</c>
/// and <c>--bzdp-selftest</c>.
/// </summary>
public sealed class FakeBzdpDevice : IDisposable
{
    readonly UdpClient _udp;
    readonly ushort _code;
    readonly CancellationTokenSource _stop = new();
    readonly object _lock = new();
    readonly StringBuilder _console = new();
    readonly Random _rng = new(7);
    Upload? _upload;
    uint _nextPid = 100;

    sealed class Upload
    {
        public required string Path;
        public required byte[] Buffer;
        public uint Received;
    }

    public ConcurrentDictionary<string, byte[]> Files { get; } = new(StringComparer.OrdinalIgnoreCase);
    public ConcurrentDictionary<uint, string> Running { get; } = new();
    public int Port { get; }
    public int MaxChunk { get; init; } = 1024;
    /// <summary>Fraction (0..1) of incoming requests silently ignored.</summary>
    public double DropRate { get; init; }
    public string Arch { get; init; } = "x86_64";
    public string Hostname { get; init; } = "fake-device";
    public int Dropped;

    public FakeBzdpDevice(int port = 0, int code = 0)
    {
        _code = (ushort)code;
        _udp = new UdpClient(new IPEndPoint(IPAddress.Loopback, port));
        Port = ((IPEndPoint)_udp.Client.LocalEndPoint!).Port;
        Files["/disk/BATANG.TXT"] = "Batang\n"u8.ToArray();
        Files["/disk/HELLO.ELF"] = new byte[64];
        Print("Buitenzorg OS (fake device) siap.\n");
    }

    public Task RunAsync() => Task.Run(Loop);

    public void Dispose() { _stop.Cancel(); _udp.Dispose(); }

    void Print(string s) { lock (_lock) _console.Append(s); }

    async Task Loop()
    {
        while (!_stop.IsCancellationRequested)
        {
            UdpReceiveResult r;
            try { r = await _udp.ReceiveAsync(_stop.Token); }
            catch (OperationCanceledException) { return; }
            catch (ObjectDisposedException) { return; }
            catch (SocketException) { continue; }
            if (!BzdpClient.TryDecode(r.Buffer, out var op, out var seq, out var code) || (op & 0x80) != 0) continue;
            bool drop;
            lock (_lock) drop = DropRate > 0 && _rng.NextDouble() < DropRate;
            if (drop) { Interlocked.Increment(ref Dropped); continue; }

            byte[] reply;
            if (code != _code)
                reply = new[] { (byte)BzdpStatus.BadCode };
            else
            {
                try { reply = Handle((BzdpOp)op, r.Buffer.AsSpan(BzdpClient.HeaderSize)); }
                catch { reply = new[] { (byte)BzdpStatus.BadRequest }; }
            }
            var d = BzdpClient.Encode((BzdpOp)op, seq, 0, reply, reply: true);
            try { await _udp.SendAsync(d, r.RemoteEndPoint); } catch { }
        }
    }

    static byte[] Ok(ReadOnlySpan<byte> data = default)
    {
        var b = new byte[1 + data.Length];
        data.CopyTo(b.AsSpan(1));
        return b;
    }

    static byte[] OkU32(uint v, ReadOnlySpan<byte> tail = default)
    {
        var b = new byte[1 + 4 + tail.Length];
        BinaryPrimitives.WriteUInt32LittleEndian(b.AsSpan(1), v);
        tail.CopyTo(b.AsSpan(5));
        return b;
    }

    static byte[] Status(BzdpStatus s) => new[] { (byte)s };

    byte[] Handle(BzdpOp op, ReadOnlySpan<byte> p)
    {
        lock (_lock)
        {
            switch (op)
            {
                case BzdpOp.Hello:
                    return Ok(Encoding.UTF8.GetBytes($"Buitenzorg OS {Arch} {Hostname}; max_chunk={MaxChunk}"));
                case BzdpOp.Begin:
                {
                    uint size = BinaryPrimitives.ReadUInt32LittleEndian(p);
                    int len = BinaryPrimitives.ReadUInt16LittleEndian(p[4..]);
                    var path = Encoding.UTF8.GetString(p.Slice(6, len));
                    if (!path.StartsWith('/') || size > 64 * 1024 * 1024) return Status(BzdpStatus.BadRequest);
                    _upload = new Upload { Path = path, Buffer = new byte[size] };
                    return Ok();
                }
                case BzdpOp.Data:
                {
                    if (_upload is null) return Status(BzdpStatus.BadRequest);
                    uint off = BinaryPrimitives.ReadUInt32LittleEndian(p);
                    var bytes = p[4..];
                    if (bytes.Length > MaxChunk) return Status(BzdpStatus.BadRequest);
                    // Accept only the contiguous next chunk (or a resend of the last one).
                    if (off == _upload.Received && off + bytes.Length <= _upload.Buffer.Length)
                    {
                        bytes.CopyTo(_upload.Buffer.AsSpan((int)off));
                        _upload.Received += (uint)bytes.Length;
                    }
                    return OkU32(_upload.Received);
                }
                case BzdpOp.End:
                {
                    if (_upload is null) return Status(BzdpStatus.BadRequest);
                    uint crc = BinaryPrimitives.ReadUInt32LittleEndian(p);
                    var u = _upload;
                    if (u.Received != u.Buffer.Length) return Status(BzdpStatus.IoError);
                    if (Crc32.HashToUInt32(u.Buffer) != crc) return Status(BzdpStatus.IoError);
                    Files[u.Path] = u.Buffer;
                    _upload = null;
                    _console.Append($"[deploy] {u.Path} ({u.Buffer.Length} byte) ditulis\n");
                    return Ok();
                }
                case BzdpOp.Run:
                {
                    var cmd = Encoding.UTF8.GetString(p).Trim();
                    var words = cmd.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (words.Length == 0) return Status(BzdpStatus.BadRequest);
                    _console.Append($"$ {cmd}\n");
                    if (words[0] is "script" or "py" or "js" or "ts")
                    {
                        var file = words[^1];
                        if (!Files.ContainsKey(file)) return Status(BzdpStatus.NotFound);
                        _console.Append($"[script] {file}: {Files[file].Length} byte dijalankan\n");
                        return OkU32(0);
                    }
                    if (words[0] == "run" && words.Length >= 2)
                    {
                        var name = words[1];
                        string upper = name.ToUpperInvariant();
                        bool found = Files.ContainsKey(name) || Files.ContainsKey($"/disk/{upper}.ELF") || Files.ContainsKey($"/disk/{upper}.DLL");
                        if (!found) return Status(BzdpStatus.NotFound);
                        uint pid = ++_nextPid;
                        Running[pid] = name;
                        _console.Append($"[app] {name} berjalan sebagai pid {pid}\n{name}: halo dari perangkat palsu\n");
                        return OkU32(pid);
                    }
                    _console.Append($"{words[0]}: perintah dijalankan\n");
                    return OkU32(0);
                }
                case BzdpOp.Log:
                {
                    uint cursor = BinaryPrimitives.ReadUInt32LittleEndian(p);
                    var all = Encoding.UTF8.GetBytes(_console.ToString());
                    if (cursor > all.Length) cursor = (uint)all.Length;
                    int n = Math.Min(1000, all.Length - (int)cursor);
                    return OkU32(cursor + (uint)n, all.AsSpan((int)cursor, n));
                }
                case BzdpOp.Kill:
                {
                    uint pid = BinaryPrimitives.ReadUInt32LittleEndian(p);
                    if (!Running.TryRemove(pid, out var name)) return Status(BzdpStatus.NotFound);
                    _console.Append($"[app] {name} (pid {pid}) dihentikan\n");
                    return Ok();
                }
                case BzdpOp.List:
                {
                    var dir = Encoding.UTF8.GetString(p).TrimEnd('/') + "/";
                    var names = Files.Keys.Where(k => k.StartsWith(dir, StringComparison.OrdinalIgnoreCase))
                                          .Select(k => k[dir.Length..]).OrderBy(k => k);
                    return Ok(Encoding.UTF8.GetBytes(string.Join('\n', names)));
                }
                default:
                    return Status(BzdpStatus.BadRequest);
            }
        }
    }
}
