// Buitenzorg.Net.WebSocket -- RFC 6455 over TcpConnection / TlsConnection:
// the client (ws:// and wss://) and the server side of one connection
// (WebSocket.Accept after a TcpServer accepted it). Text and binary messages,
// fragmented messages reassembled, ping answered with pong, close handshake.
// Works on the device and off it (the same TCP classes).

namespace Buitenzorg.Net;

using System.Text;

public sealed class WebSocketMessage
{
    public bool IsText;
    public byte[] Data = new byte[0];
    public string Text => Encoding.UTF8.GetString(Data);
}

public sealed class WebSocket
{
    readonly INetStream _s;
    readonly bool _client;
    readonly byte[] _rx = new byte[8192];
    int _rxStart, _rxEnd;
    readonly Random _rng = new Random();
    public bool IsOpen { get; private set; } = true;
    /// <summary>The close code the other side sent (0 = none yet).</summary>
    public int CloseCode { get; private set; }

    WebSocket(INetStream s, bool client) { _s = s; _client = client; }

    /// <summary>Connect to ws://host[:port]/path or wss://...</summary>
    public static WebSocket Connect(string url, int timeoutMs = 10000)
    {
        string u = url.Trim();
        bool secure = u.StartsWith("wss://");
        if (!secure && !u.StartsWith("ws://")) throw new NetException("bukan URL ws:// atau wss://");
        var parsed = Url.Parse((secure ? "https://" : "http://") + u.Substring(secure ? 6 : 5));
        if (parsed == null) throw new NetException("URL tidak sah: " + url);
        INetStream s = secure ? TlsConnection.Connect(parsed.Host, parsed.Port, timeoutMs) : TcpConnection.Connect(parsed.Host, parsed.Port, timeoutMs);
        s.TimeoutMs = timeoutMs;
        var key = new byte[16];
        new Random().NextBytes(key);
        string k64 = Convert.ToBase64String(key);
        string hostHeader = parsed.Host + ((secure && parsed.Port == 443) || (!secure && parsed.Port == 80) ? "" : ":" + parsed.Port);
        var req = "GET " + parsed.Path + " HTTP/1.1\r\nHost: " + hostHeader + "\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                + "Sec-WebSocket-Key: " + k64 + "\r\nSec-WebSocket-Version: 13\r\nUser-Agent: Buitenzorg\r\n\r\n";
        var rb = Encoding.ASCII.GetBytes(req);
        s.Write(rb, 0, rb.Length);
        var ws = new WebSocket(s, true);
        string head = ws.ReadHead();
        if (!head.StartsWith("HTTP/1.1 101"))
        {
            s.Close();
            int nl = head.IndexOf('\r');
            throw new NetException("server menolak WebSocket: " + (nl > 0 ? head.Substring(0, nl) : head));
        }
        if (HeaderValue(head, "Sec-WebSocket-Accept") != AcceptKey(k64))
        {
            s.Close();
            throw new NetException("Sec-WebSocket-Accept salah");
        }
        return ws;
    }

    /// <summary>Server side: answer the upgrade request `request` (the HTTP head
    /// already read from `c`). Null when it is not a WebSocket request.</summary>
    public static WebSocket? Accept(INetStream c, string request)
    {
        string key = HeaderValue(request, "Sec-WebSocket-Key");
        if (key.Length == 0 || HeaderValue(request, "Upgrade").ToLowerInvariant() != "websocket") return null;
        var resp = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + AcceptKey(key) + "\r\n\r\n";
        var b = Encoding.ASCII.GetBytes(resp);
        c.Write(b, 0, b.Length);
        return new WebSocket(c, false);
    }

    public void Send(string text) => SendFrame(1, Encoding.UTF8.GetBytes(text));
    public void Send(byte[] data) => SendFrame(2, data);
    public void Ping(byte[]? payload = null) => SendFrame(9, payload ?? new byte[0]);

    /// <summary>The next message (pings are answered on the way); null when the connection closed.</summary>
    public WebSocketMessage? Receive()
    {
        var acc = new List<byte>();
        int kind = 0;
        while (IsOpen)
        {
            var (fin, op, payload) = ReadFrame();
            if (op < 0) { IsOpen = false; return null; }
            if (op == 9) { SendFrame(10, payload); continue; }   // ping -> pong
            if (op == 10) continue;                               // pong
            if (op == 8)
            {
                CloseCode = payload.Length >= 2 ? payload[0] << 8 | payload[1] : 1005;
                if (IsOpen) { try { SendFrame(8, payload.Length >= 2 ? new[] { payload[0], payload[1] } : new byte[0]); } catch (Exception) { } }
                IsOpen = false;
                _s.Close();
                return null;
            }
            if (op != 0) kind = op;
            if (fin && acc.Count == 0) return new WebSocketMessage { IsText = kind == 1, Data = payload };   // one frame: no copy
            acc.AddRange(payload);
            if (fin) return new WebSocketMessage { IsText = kind == 1, Data = acc.ToArray() };
        }
        return null;
    }

    /// <summary>Start the close handshake (and wait for the answer).</summary>
    public void Close(int code = 1000)
    {
        if (!IsOpen) return;
        try
        {
            SendFrame(8, new[] { (byte)(code >> 8), (byte)code });
            for (int i = 0; i < 8 && IsOpen; i++)
            {
                var (_, op, payload) = ReadFrame();
                if (op < 0 || op == 8) { CloseCode = payload.Length >= 2 ? payload[0] << 8 | payload[1] : code; break; }
            }
        }
        catch (Exception) { }
        IsOpen = false;
        _s.Close();
    }

    // ---- frames -------------------------------------------------------------

    void SendFrame(int op, byte[] payload)
    {
        int n = payload.Length;
        var h = new List<byte> { (byte)(0x80 | op) };
        byte maskBit = (byte)(_client ? 0x80 : 0);
        if (n < 126) h.Add((byte)(maskBit | n));
        else if (n < 65536) { h.Add((byte)(maskBit | 126)); h.Add((byte)(n >> 8)); h.Add((byte)n); }
        else
        {
            h.Add((byte)(maskBit | 127));
            for (int i = 7; i >= 0; i--) h.Add(i >= 4 ? (byte)0 : (byte)(n >> (8 * i)));
        }
        var frame = new byte[h.Count + (_client ? 4 : 0) + n];
        for (int i = 0; i < h.Count; i++) frame[i] = h[i];
        int p = h.Count;
        if (_client)
        {
            var mask = new byte[4];
            _rng.NextBytes(mask);
            for (int i = 0; i < 4; i++) frame[p + i] = mask[i];
            p += 4;
            for (int i = 0; i < n; i++) frame[p + i] = (byte)(payload[i] ^ mask[i & 3]);
        }
        else if (n > 0) Array.Copy(payload, 0, frame, p, n);
        _s.Write(frame, 0, frame.Length);
    }

    /// <summary>(final fragment, opcode -- -1 at end of stream, payload)</summary>
    (bool, int, byte[]) ReadFrame()
    {
        var hd = new byte[2];
        if (!ReadExact(hd, 2)) return (true, -1, new byte[0]);
        bool fin = (hd[0] & 0x80) != 0;
        int op = hd[0] & 0x0F;
        bool masked = (hd[1] & 0x80) != 0;
        long len = hd[1] & 0x7F;
        if (len == 126)
        {
            var e = new byte[2];
            if (!ReadExact(e, 2)) return (true, -1, new byte[0]);
            len = e[0] << 8 | e[1];
        }
        else if (len == 127)
        {
            var e = new byte[8];
            if (!ReadExact(e, 8)) return (true, -1, new byte[0]);
            len = 0;
            for (int i = 0; i < 8; i++) len = len << 8 | e[i];
        }
        if (len > 64L * 1024 * 1024) throw new NetException("pesan WebSocket terlalu besar");
        var mask = new byte[4];
        if (masked && !ReadExact(mask, 4)) return (true, -1, new byte[0]);
        var payload = new byte[len];
        if (len > 0 && !ReadExact(payload, (int)len)) return (true, -1, new byte[0]);
        if (masked) for (int i = 0; i < payload.Length; i++) payload[i] ^= mask[i & 3];
        return (fin, op, payload);
    }

    bool ReadExact(byte[] dst, int count)
    {
        int got = 0;
        while (got < count)
        {
            if (_rxStart == _rxEnd)
            {
                _rxStart = 0;
                _rxEnd = _s.Read(_rx, 0, _rx.Length);
                if (_rxEnd <= 0) { _rxEnd = 0; return false; }
            }
            int n = Math.Min(count - got, _rxEnd - _rxStart);
            Array.Copy(_rx, _rxStart, dst, got, n);
            _rxStart += n;
            got += n;
        }
        return true;
    }

    /// <summary>The HTTP head of the server's answer (bytes after it stay buffered).</summary>
    string ReadHead()
    {
        var sb = new StringBuilder();
        var one = new byte[1];
        while (sb.Length < 16384)
        {
            if (!ReadExact(one, 1)) break;
            sb.Append((char)one[0]);
            int L = sb.Length;
            if (L >= 4 && sb[L - 4] == '\r' && sb[L - 3] == '\n' && sb[L - 2] == '\r' && sb[L - 1] == '\n') break;
        }
        return sb.ToString();
    }

    static string HeaderValue(string head, string name)
    {
        foreach (var line in head.Split('\n'))
        {
            int c = line.IndexOf(':');
            if (c > 0 && line.Substring(0, c).Trim().ToLowerInvariant() == name.ToLowerInvariant()) return line.Substring(c + 1).Trim();
        }
        return "";
    }

    /// <summary>Sec-WebSocket-Accept for a key: base64(SHA-1(key + GUID)).</summary>
    public static string AcceptKey(string key) =>
        Convert.ToBase64String(Sha1(Encoding.ASCII.GetBytes(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")));

    /// <summary>SHA-1 (only for the handshake; RustBCL has no System.Security.Cryptography).</summary>
    public static byte[] Sha1(byte[] data)
    {
        uint h0 = 0x67452301, h1 = 0xEFCDAB89, h2 = 0x98BADCFE, h3 = 0x10325476, h4 = 0xC3D2E1F0;
        long bitLen = (long)data.Length * 8;
        int padded = ((data.Length + 8) / 64 + 1) * 64;
        var m = new byte[padded];
        Array.Copy(data, m, data.Length);
        m[data.Length] = 0x80;
        for (int i = 0; i < 8; i++) m[padded - 1 - i] = (byte)(bitLen >> (8 * i));
        var w = new uint[80];
        for (int chunk = 0; chunk < padded; chunk += 64)
        {
            for (int i = 0; i < 16; i++)
                w[i] = (uint)(m[chunk + 4 * i] << 24 | m[chunk + 4 * i + 1] << 16 | m[chunk + 4 * i + 2] << 8 | m[chunk + 4 * i + 3]);
            for (int i = 16; i < 80; i++) { uint x = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]; w[i] = x << 1 | x >> 31; }
            uint a = h0, b = h1, c = h2, d = h3, e = h4;
            for (int i = 0; i < 80; i++)
            {
                uint f, k;
                if (i < 20) { f = (b & c) | (~b & d); k = 0x5A827999; }
                else if (i < 40) { f = b ^ c ^ d; k = 0x6ED9EBA1; }
                else if (i < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8F1BBCDC; }
                else { f = b ^ c ^ d; k = 0xCA62C1D6; }
                uint t = (a << 5 | a >> 27) + f + e + k + w[i];
                e = d; d = c; c = b << 30 | b >> 2; b = a; a = t;
            }
            h0 += a; h1 += b; h2 += c; h3 += d; h4 += e;
        }
        var outp = new byte[20];
        uint[] hs = { h0, h1, h2, h3, h4 };
        for (int i = 0; i < 5; i++) { outp[4 * i] = (byte)(hs[i] >> 24); outp[4 * i + 1] = (byte)(hs[i] >> 16); outp[4 * i + 2] = (byte)(hs[i] >> 8); outp[4 * i + 3] = (byte)hs[i]; }
        return outp;
    }
}
