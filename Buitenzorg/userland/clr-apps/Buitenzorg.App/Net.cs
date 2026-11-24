// Buitenzorg.Net -- DNS, TCP, TLS and an HTTP/1.1 client for managed programs.
//
// On the device: DNS_RESOLVE and TCP_OP syscalls (kernel dns.rs / appnet.rs),
// TLS in the host (clrhost netapi.rs: rustls, Mozilla's root certificates --
// certificates ARE verified; Tls.AddRoot trusts one more). Off the device
// (`dotnet X.dll` on a PC) the same classes use .NET's sockets and SslStream,
// so programs and their self-tests run there too.
//
// Calls block the calling thread (they poll the kernel with short sleeps);
// a UI program should fetch on a background thread (see the Browser app).

namespace Buitenzorg.Net;

using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Collections.Generic;

internal static class NetNative
{
    [DllImport("buitenzorg")] internal static extern long DnsResolve(string name);
    [DllImport("buitenzorg")] internal static extern int DnsResolve6(string name, byte[] address);
    [DllImport("buitenzorg")] internal static extern long TcpConnect(long ip, int port);
    [DllImport("buitenzorg")] internal static extern long TcpConnect6(byte[] address, int port);
    [DllImport("buitenzorg")] internal static extern long TcpListen(int port);
    [DllImport("buitenzorg")] internal static extern long TcpAccept(long listener);
    [DllImport("buitenzorg")] internal static extern int TcpSend(long h, byte[] data, int offset, int count);
    [DllImport("buitenzorg")] internal static extern int TcpRecv(long h, byte[] buffer, int offset, int count);
    [DllImport("buitenzorg")] internal static extern int TcpState(long h);
    [DllImport("buitenzorg")] internal static extern int TcpClose(long h);
    [DllImport("buitenzorg")] internal static extern int TlsAddRoot(byte[] der);
    [DllImport("buitenzorg")] internal static extern long TlsOpen(long tcp, string host);
    [DllImport("buitenzorg")] internal static extern int TlsSend(long id, byte[] data, int offset, int count);
    [DllImport("buitenzorg")] internal static extern int TlsRecv(long id, byte[] buffer, int offset, int count);
    [DllImport("buitenzorg")] internal static extern int TlsState(long id);
    [DllImport("buitenzorg")] internal static extern string TlsError(long id);
    [DllImport("buitenzorg")] internal static extern string TlsVersion(long id);
    [DllImport("buitenzorg")] internal static extern int TlsClose(long id);

    static int _mode; // 0 unknown, 1 device, 2 host .NET
    internal static bool OnDevice
    {
        get
        {
            if (_mode == 0)
            {
                try { NetNative.TcpState(0); _mode = 1; }
                catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _mode = 2; }
            }
            return _mode == 1;
        }
    }
}

public sealed class NetException : Exception
{
    public NetException(string message) : base(message) { }
}

/// <summary>Host names to IPv4 addresses.</summary>
public static class Dns
{
    /// <summary>The address as a.b.c.d packed big-endian (a &lt;&lt; 24 ...), or -1.</summary>
    public static long Resolve(string host)
    {
        long lit = ParseIp(host);
        if (lit >= 0) return lit;
        if (NetNative.OnDevice) return NetNative.DnsResolve(host);
        return HostNet.Resolve(host);
    }

    /// <summary>The IPv6 address of `host` (DNS AAAA; a literal is parsed), or null.</summary>
    public static byte[]? Resolve6(string host)
    {
        if (host.StartsWith("[") && host.EndsWith("]")) host = host.Substring(1, host.Length - 2);
        if (host.IndexOf(':') >= 0) return TcpConnection.ParseIpv6(host);
        if (NetNative.OnDevice)
        {
            var a = new byte[16];
            return NetNative.DnsResolve6(host, a) == 0 ? a : null;
        }
        try
        {
            foreach (var ip in System.Net.Dns.GetHostAddresses(host))
                if (ip.AddressFamily == System.Net.Sockets.AddressFamily.InterNetworkV6) return ip.GetAddressBytes();
        }
        catch (Exception) { }
        return null;
    }

    public static long ParseIp(string s)
    {
        var parts = s.Split('.');
        if (parts.Length != 4) return -1;
        long v = 0;
        foreach (var p in parts)
        {
            if (p.Length == 0 || p.Length > 3) return -1;
            int n = 0;
            foreach (char c in p) { if (c < '0' || c > '9') return -1; n = n * 10 + (c - '0'); }
            if (n > 255) return -1;
            v = v << 8 | (uint)n;
        }
        return v;
    }

    public static string Format(long ip) =>
        ((ip >> 24) & 255) + "." + ((ip >> 16) & 255) + "." + ((ip >> 8) & 255) + "." + (ip & 255);
}

/// <summary>A byte stream: a TCP connection, or TLS over one.</summary>
public interface INetStream
{
    void Write(byte[] data, int offset, int count);
    /// <summary>Bytes read; 0 = the other side closed. Waits up to the timeout.</summary>
    int Read(byte[] buffer, int offset, int count);
    void Close();
    int TimeoutMs { get; set; }
}

public sealed class TcpConnection : INetStream
{
    internal long Handle;
    internal HostNet.Conn? Host;
    public int TimeoutMs { get; set; } = 15000;
    public string RemoteHost { get; private set; } = "";
    public long RemoteAddress { get; private set; }
    public int RemotePort { get; private set; }

    TcpConnection() { }

    public static TcpConnection Connect(string host, int port, int timeoutMs = 10000)
    {
        if (host.StartsWith("[") && host.EndsWith("]")) host = host.Substring(1, host.Length - 2);
        var v6 = host.IndexOf(':') >= 0 ? ParseIpv6(host) : null;
        if (host.IndexOf(':') >= 0 && v6 == null) throw new NetException("alamat IPv6 " + host + " tidak sah");
        long ip = v6 != null ? 0 : Dns.Resolve(host);
        // a name with only an AAAA record: connect over IPv6
        if (ip < 0 && v6 == null) { v6 = Dns.Resolve6(host); if (v6 != null) ip = 0; }
        if (ip < 0) throw new NetException("alamat " + host + " tidak ditemukan (DNS)");
        var c = new TcpConnection { RemoteHost = host, RemoteAddress = ip, RemotePort = port };
        if (!NetNative.OnDevice) { c.Host = HostNet.Connect(v6 != null ? host : Dns.Format(ip), port, timeoutMs); return c; }
        c.Handle = v6 != null ? NetNative.TcpConnect6(v6, port) : NetNative.TcpConnect(ip, port);
        if (c.Handle <= 0) throw new NetException("tidak bisa membuka sambungan ke " + host);
        long until = Environment.TickCount64 + timeoutMs;
        while (true)
        {
            int st = NetNative.TcpState(c.Handle);
            if (st == 1 || st == 2) return c;
            if (st == 3) { NetNative.TcpClose(c.Handle); throw new NetException(host + ":" + port + " menolak sambungan"); }
            if (Environment.TickCount64 > until) { NetNative.TcpClose(c.Handle); throw new NetException(host + ":" + port + " tidak menjawab"); }
            Thread.Sleep(2);
        }
    }

    public void Write(byte[] data, int offset, int count)
    {
        if (Host != null) { Host.Write(data, offset, count); return; }
        long until = Environment.TickCount64 + TimeoutMs;
        while (count > 0)
        {
            int n = NetNative.TcpSend(Handle, data, offset, Math.Min(count, 65536));
            if (n < 0) throw new NetException("sambungan terputus saat mengirim");
            if (n == 0)
            {
                if (Environment.TickCount64 > until) throw new NetException("waktu habis saat mengirim");
                Thread.Sleep(2);
                continue;
            }
            offset += n; count -= n;
            until = Environment.TickCount64 + TimeoutMs;
        }
    }

    public void Write(string text) { var b = Encoding.UTF8.GetBytes(text); Write(b, 0, b.Length); }

    /// <summary>A textual IPv6 address ("fe80::1", "2001:db8::a:b") as 16 bytes, or null.</summary>
    public static byte[]? ParseIpv6(string s)
    {
        int pct = s.IndexOf('%');
        if (pct >= 0) s = s.Substring(0, pct); // zone id
        var head = new List<int>();
        var tail = new List<int>();
        var cur = head;
        bool gap = false;
        int i = 0, n = s.Length;
        if (n >= 2 && s[0] == ':' && s[1] == ':') { gap = true; cur = tail; i = 2; }
        while (i < n)
        {
            int v = 0, d = 0;
            while (i < n && d < 5)
            {
                char ch = s[i];
                int h = ch >= '0' && ch <= '9' ? ch - '0' : ch >= 'a' && ch <= 'f' ? ch - 'a' + 10 : ch >= 'A' && ch <= 'F' ? ch - 'A' + 10 : -1;
                if (h < 0) break;
                v = v * 16 + h; d++; i++;
            }
            if (d == 0 || d > 4) return null;
            cur.Add(v);
            if (i == n) break;
            if (s[i] != ':') return null;
            i++;
            if (i < n && s[i] == ':') { if (gap) return null; gap = true; cur = tail; i++; }
            else if (i == n) return null;
        }
        int total = head.Count + tail.Count;
        if (gap ? total > 7 : total != 8) return null;
        var b = new byte[16];
        for (int k = 0; k < head.Count; k++) { b[k * 2] = (byte)(head[k] >> 8); b[k * 2 + 1] = (byte)head[k]; }
        for (int k = 0; k < tail.Count; k++) { int at = 16 - (tail.Count - k) * 2; b[at] = (byte)(tail[k] >> 8); b[at + 1] = (byte)tail[k]; }
        return b;
    }

    public int Read(byte[] buffer, int offset, int count)
    {
        if (Host != null) return Host.Read(buffer, offset, count, TimeoutMs);
        long until = Environment.TickCount64 + TimeoutMs;
        while (true)
        {
            int n = NetNative.TcpRecv(Handle, buffer, offset, count);
            if (n > 0) return n;
            if (n < 0) return 0;
            int st = NetNative.TcpState(Handle);
            if (st >= 2) return 0;
            if (Environment.TickCount64 > until) throw new NetException("waktu habis menunggu jawaban");
            Thread.Sleep(2);
        }
    }

    /// <summary>What has arrived, without waiting (0 = nothing yet, -1 = closed).</summary>
    public int ReadAvailable(byte[] buffer, int offset, int count)
    {
        if (Host != null) return Host.ReadAvailable(buffer, offset, count);
        int n = NetNative.TcpRecv(Handle, buffer, offset, count);
        if (n != 0) return n;
        return NetNative.TcpState(Handle) >= 2 ? -1 : 0;
    }

    public void Close()
    {
        if (Host != null) { Host.Close(); return; }
        if (Handle > 0) NetNative.TcpClose(Handle);
        Handle = 0;
    }

    internal static TcpConnection FromHandle(long h) => new TcpConnection { Handle = h };
    internal static TcpConnection FromHost(HostNet.Conn c) => new TcpConnection { Host = c };
}

/// <summary>Accepts TCP connections on a port.</summary>
public sealed class TcpServer
{
    long _h;
    HostNet.Listener? _host;
    public int Port { get; }

    public TcpServer(int port)
    {
        Port = port;
        if (!NetNative.OnDevice) { _host = HostNet.Listen(port); return; }
        _h = NetNative.TcpListen(port);
        if (_h <= 0) throw new NetException("port " + port + " sudah dipakai");
    }

    /// <summary>The next connection, or null when none came within the wait.</summary>
    public TcpConnection? Accept(int waitMs = 0)
    {
        long until = Environment.TickCount64 + waitMs;
        while (true)
        {
            if (_host != null) { var c = _host.Accept(); if (c != null) return TcpConnection.FromHost(c); }
            else
            {
                long h = NetNative.TcpAccept(_h);
                if (h > 0) return TcpConnection.FromHandle(h);
            }
            if (Environment.TickCount64 >= until) return null;
            Thread.Sleep(2);
        }
    }

    public void Close()
    {
        if (_host != null) _host.Close();
        else if (_h > 0) NetNative.TcpClose(_h);
        _h = 0;
    }
}

/// <summary>TLS (HTTPS) over a TCP connection; the server's certificate is verified.</summary>
public sealed class TlsConnection : INetStream
{
    readonly TcpConnection _tcp;
    long _id;
    HostNet.Tls? _host;
    public int TimeoutMs { get; set; } = 15000;
    /// <summary>Protocol and cipher suite once open, e.g. "TLSv1_3 TLS13_AES_128_GCM_SHA256".</summary>
    public string Version { get; private set; } = "";

    TlsConnection(TcpConnection tcp) { _tcp = tcp; }

    /// <summary>Trust one more root certificate (DER bytes) for connections opened after this.</summary>
    public static bool AddRoot(byte[] der)
    {
        if (!NetNative.OnDevice) { HostNet.AddRoot(der); return true; }
        return NetNative.TlsAddRoot(der) == 0;
    }

    public static TlsConnection Connect(string host, int port = 443, int timeoutMs = 15000)
    {
        var tcp = TcpConnection.Connect(host, port, timeoutMs);
        var t = new TlsConnection(tcp) { TimeoutMs = timeoutMs };
        if (tcp.Host != null)
        {
            t._host = HostNet.StartTls(tcp.Host, host);
            t.Version = t._host.Version;
            return t;
        }
        t._id = NetNative.TlsOpen(tcp.Handle, host);
        if (t._id <= 0) { tcp.Close(); throw new NetException("TLS tidak bisa dimulai untuk " + host); }
        long until = Environment.TickCount64 + timeoutMs;
        while (true)
        {
            int st = NetNative.TlsState(t._id);
            if (st == 1) { t.Version = NetNative.TlsVersion(t._id); return t; }
            if (st >= 2)
            {
                string why = NetNative.TlsError(t._id);
                NetNative.TlsClose(t._id); tcp.Close();
                throw new NetException("TLS ke " + host + " gagal: " + (why.Length > 0 ? why : "ditutup"));
            }
            if (Environment.TickCount64 > until) { NetNative.TlsClose(t._id); tcp.Close(); throw new NetException("TLS ke " + host + ": waktu habis"); }
            Thread.Sleep(2);
        }
    }

    public void Write(byte[] data, int offset, int count)
    {
        if (_host != null) { _host.Write(data, offset, count); return; }
        if (NetNative.TlsSend(_id, data, offset, count) < 0) throw new NetException("TLS: " + NetNative.TlsError(_id));
    }

    public int Read(byte[] buffer, int offset, int count)
    {
        if (_host != null) return _host.Read(buffer, offset, count);
        long until = Environment.TickCount64 + TimeoutMs;
        while (true)
        {
            int n = NetNative.TlsRecv(_id, buffer, offset, count);
            if (n > 0) return n;
            if (n < 0) return 0;
            if (Environment.TickCount64 > until) throw new NetException("waktu habis menunggu jawaban");
            Thread.Sleep(2);
        }
    }

    public void Close()
    {
        if (_host != null) _host.Close();
        else if (_id > 0) NetNative.TlsClose(_id);
        _id = 0;
        _tcp.Close();
    }
}

/// <summary>A parsed http:// or https:// address.</summary>
public sealed class Url
{
    public string Scheme = "http", Host = "", Path = "/";
    public int Port = 80;
    public bool Secure => Scheme == "https";

    public static Url? Parse(string text)
    {
        string s = text.Trim();
        var u = new Url();
        int se = s.IndexOf("://");
        if (se > 0) { u.Scheme = s.Substring(0, se).ToLowerInvariant(); s = s.Substring(se + 3); }
        if (u.Scheme != "http" && u.Scheme != "https") return null;
        u.Port = u.Secure ? 443 : 80;
        int slash = s.IndexOf('/');
        string hostPort = slash >= 0 ? s.Substring(0, slash) : s;
        u.Path = slash >= 0 ? s.Substring(slash) : "/";
        int hash = u.Path.IndexOf('#');
        if (hash >= 0) u.Path = u.Path.Substring(0, hash);
        int colon = hostPort.LastIndexOf(':');
        if (colon > 0)
        {
            if (!int.TryParse(hostPort.Substring(colon + 1), out u.Port) || u.Port <= 0 || u.Port > 65535) return null;
            hostPort = hostPort.Substring(0, colon);
        }
        u.Host = hostPort.ToLowerInvariant();
        return u.Host.Length == 0 ? null : u;
    }

    /// <summary>Resolve a link found on this page.</summary>
    public Url? Resolve(string link)
    {
        link = link.Trim();
        if (link.StartsWith("http://") || link.StartsWith("https://")) return Parse(link);
        if (link.StartsWith("//")) return Parse(Scheme + ":" + link);
        var u = new Url { Scheme = Scheme, Host = Host, Port = Port };
        if (link.StartsWith("/")) u.Path = link;
        else if (link.StartsWith("#")) u.Path = Path;
        else
        {
            string dir = Path;
            int q = dir.IndexOf('?');
            if (q >= 0) dir = dir.Substring(0, q);
            dir = dir.Substring(0, dir.LastIndexOf('/') + 1);
            u.Path = Normalize(dir + link);
        }
        int hash = u.Path.IndexOf('#');
        if (hash >= 0) u.Path = u.Path.Substring(0, hash);
        if (u.Path.Length == 0) u.Path = "/";
        return u;
    }

    static string Normalize(string path)
    {
        string query = "";
        int q = path.IndexOf('?');
        if (q >= 0) { query = path.Substring(q); path = path.Substring(0, q); }
        var outParts = new List<string>();
        var parts = path.Split('/');
        for (int i = 0; i < parts.Length; i++)
        {
            string p = parts[i];
            if (p == "..") { if (outParts.Count > 1) outParts.RemoveAt(outParts.Count - 1); }
            else if (p != "." && (p.Length > 0 || i == 0 || i == parts.Length - 1)) outParts.Add(p);
        }
        return string.Join("/", outParts) + query;
    }

    public override string ToString() =>
        Scheme + "://" + Host + ((Secure && Port == 443) || (!Secure && Port == 80) ? "" : ":" + Port) + Path;
}

public sealed class HttpResponse
{
    public int Status;
    public string Reason = "";
    public readonly List<KeyValuePair<string, string>> Headers = new List<KeyValuePair<string, string>>();
    public byte[] Body = Array.Empty<byte>();
    /// <summary>The address the body came from (after redirects).</summary>
    public string Url = "";
    /// <summary>"TLSv1_3 ..." for HTTPS, "" for plain HTTP.</summary>
    public string Tls = "";
    /// <summary>Download: bytes now in the file, and whether it was stopped early.</summary>
    public long Saved;
    public bool Cancelled;
    /// <summary>Download: the transfer continued an earlier partial file (206).</summary>
    public bool Resumed;

    public string? Header(string name)
    {
        foreach (var kv in Headers) if (string.Equals(kv.Key, name, StringComparison.OrdinalIgnoreCase)) return kv.Value;
        return null;
    }

    public string ContentType => Header("Content-Type") ?? "";
    public string Text => Encoding.UTF8.GetString(Body);
    public bool Ok => Status >= 200 && Status < 300;
}

/// <summary>HTTP/1.1 (one request per connection), http and https, redirects, chunked bodies.</summary>
/// <summary>HTTP cookies (an RFC 6265 subset): Set-Cookie from responses, a Cookie
/// header for requests. Domain (host-only when absent; a domain that does not
/// cover the host is refused), Path (default: the request's directory),
/// Expires / Max-Age (a past date deletes), Secure (https only). HttpOnly and
/// SameSite are accepted and ignored -- there is no script access to cookies.
/// `Save`/`Load` keep the persistent ones as text (one tab-separated line each).</summary>
public sealed class CookieJar
{
    sealed class Cookie
    {
        public string Name = "", Value = "", Domain = "", Path = "/";
        public bool HostOnly, Secure;
        /// <summary>Unix seconds; 0 = a session cookie.</summary>
        public long Expires;
    }

    readonly List<Cookie> _cookies = new List<Cookie>();
    readonly object _lock = new object();

    /// <summary>The jar the browser and downloads share.</summary>
    public static readonly CookieJar Shared = new CookieJar();

    /// <summary>Unix seconds now (replaceable for tests).</summary>
    public Func<long> Now = UnixNow;

    public int Count { get { lock (_lock) { Expire(); return _cookies.Count; } } }

    public void Clear() { lock (_lock) _cookies.Clear(); }

    static long UnixNow()
    {
        DateTime t;
        try { t = DateTime.UtcNow; } catch (Exception) { t = DateTime.Now; }
        return Seconds(t.Year, t.Month, t.Day, t.Hour, t.Minute, t.Second);
    }

    /// <summary>Unix seconds of a UTC calendar time (Hinnant's days_from_civil).</summary>
    static long Seconds(int y, int m, int d, int hh, int mm, int ss)
    {
        y -= m <= 2 ? 1 : 0;
        long era = (y >= 0 ? y : y - 399) / 400;
        long yoe = y - era * 400;
        long doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
        long doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        long days = era * 146097 + doe - 719468;
        return days * 86400 + hh * 3600 + mm * 60 + ss;
    }

    static readonly string[] Months = { "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec" };

    /// <summary>An Expires date ("Wed, 21 Oct 2015 07:28:00 GMT" and the usual variants); -1 = unreadable.</summary>
    public static long ParseDate(string s)
    {
        int day = -1, month = -1, year = -1, h = -1, mi = 0, se = 0;
        foreach (var raw in s.Replace(',', ' ').Replace('-', ' ').Split(' '))
        {
            string w = raw.Trim().ToLowerInvariant();
            if (w.Length == 0) continue;
            if (w.Contains(':'))
            {
                var p = w.Split(':');
                if (p.Length >= 2 && int.TryParse(p[0], out h) && int.TryParse(p[1], out mi)) { if (p.Length > 2) int.TryParse(p[2], out se); }
                continue;
            }
            if (w.Length >= 3)
            {
                bool isMonth = false;
                for (int i = 0; i < 12; i++) if (w.StartsWith(Months[i])) { month = i + 1; isMonth = true; break; }
                if (isMonth) continue;
            }
            if (int.TryParse(w, out int n))
            {
                if (day < 0 && n >= 1 && n <= 31 && w.Length <= 2) day = n;
                else if (year < 0) year = n < 70 ? 2000 + n : n < 100 ? 1900 + n : n;
            }
        }
        if (day < 0 || month < 0 || year < 1601 || h < 0) return -1;
        return Seconds(year, month, day, h, mi, se);
    }

    static bool DomainMatches(string host, string domain) =>
        host == domain || (host.EndsWith("." + domain) && !IsIp(host));

    static bool IsIp(string host)
    {
        if (host.Contains(':')) return true;
        foreach (char c in host) if (c != '.' && (c < '0' || c > '9')) return false;
        return true;
    }

    static string PathOnly(string path)
    {
        int q = path.IndexOf('?');
        return q >= 0 ? path.Substring(0, q) : path;
    }

    static bool PathMatches(string reqPath, string cookiePath)
    {
        if (reqPath == cookiePath) return true;
        if (!reqPath.StartsWith(cookiePath)) return false;
        return cookiePath.EndsWith("/") || reqPath[cookiePath.Length] == '/';
    }

    void Expire()
    {
        long now = Now();
        for (int i = _cookies.Count - 1; i >= 0; i--)
            if (_cookies[i].Expires != 0 && _cookies[i].Expires <= now) _cookies.RemoveAt(i);
    }

    /// <summary>Take one Set-Cookie header of a response from `u`. False = refused.</summary>
    public bool Store(Url u, string header)
    {
        var parts = header.Split(';');
        string first = parts[0];
        int eq = first.IndexOf('=');
        if (eq <= 0) return false;
        var c = new Cookie { Name = first.Substring(0, eq).Trim(), Value = first.Substring(eq + 1).Trim() };
        if (c.Name.Length == 0) return false;
        if (c.Value.Length >= 2 && c.Value[0] == '"' && c.Value[c.Value.Length - 1] == '"') c.Value = c.Value.Substring(1, c.Value.Length - 2);
        string domain = "", path = "";
        long expires = 0;
        bool maxAge = false;
        for (int i = 1; i < parts.Length; i++)
        {
            string a = parts[i].Trim();
            int e = a.IndexOf('=');
            string k = (e >= 0 ? a.Substring(0, e) : a).Trim().ToLowerInvariant();
            string v = e >= 0 ? a.Substring(e + 1).Trim() : "";
            if (k == "domain") domain = v.TrimStart('.').ToLowerInvariant();
            else if (k == "path") path = v;
            else if (k == "secure") c.Secure = true;
            else if (k == "max-age")
            {
                if (long.TryParse(v, out long s)) { expires = s <= 0 ? 1 : Now() + s; maxAge = true; }
            }
            else if (k == "expires" && !maxAge)
            {
                long t = ParseDate(v);
                if (t >= 0) expires = t <= 0 ? 1 : t;
            }
        }
        string host = u.Host.ToLowerInvariant();
        if (domain.Length > 0)
        {
            if (!DomainMatches(host, domain) || !domain.Contains('.') && domain != host) return false;
            c.Domain = domain;
        }
        else { c.Domain = host; c.HostOnly = true; }
        if (path.Length == 0 || path[0] != '/')
        {
            string rp = PathOnly(u.Path);
            int slash = rp.LastIndexOf('/');
            path = slash <= 0 ? "/" : rp.Substring(0, slash);
        }
        c.Path = path;
        c.Expires = expires;
        if (c.Secure && !u.Secure) return false;
        lock (_lock)
        {
            for (int i = 0; i < _cookies.Count; i++)
            {
                var o = _cookies[i];
                if (o.Name == c.Name && o.Domain == c.Domain && o.Path == c.Path) { _cookies.RemoveAt(i); break; }
            }
            if (c.Expires == 0 || c.Expires > Now()) _cookies.Add(c);
            if (_cookies.Count > 500) _cookies.RemoveAt(0);
        }
        return true;
    }

    /// <summary>Every Set-Cookie header of a response.</summary>
    public void StoreAll(Url u, HttpResponse r)
    {
        foreach (var kv in r.Headers)
            if (string.Equals(kv.Key, "Set-Cookie", StringComparison.OrdinalIgnoreCase)) Store(u, kv.Value);
    }

    /// <summary>The Cookie header value for a request to `u` ("" = none); longer paths first.</summary>
    public string Header(Url u)
    {
        string host = u.Host.ToLowerInvariant(), path = PathOnly(u.Path);
        var hits = new List<Cookie>();
        lock (_lock)
        {
            Expire();
            foreach (var c in _cookies)
            {
                if (c.Secure && !u.Secure) continue;
                if (c.HostOnly ? host != c.Domain : !DomainMatches(host, c.Domain)) continue;
                if (!PathMatches(path, c.Path)) continue;
                int at = 0;
                while (at < hits.Count && hits[at].Path.Length >= c.Path.Length) at++;
                hits.Insert(at, c);
            }
        }
        var sb = new StringBuilder();
        foreach (var c in hits)
        {
            if (sb.Length > 0) sb.Append("; ");
            sb.Append(c.Name).Append('=').Append(c.Value);
        }
        return sb.ToString();
    }

    /// <summary>The persistent cookies as text (session cookies are not kept).</summary>
    public string Save()
    {
        var sb = new StringBuilder();
        lock (_lock)
        {
            Expire();
            foreach (var c in _cookies)
            {
                if (c.Expires == 0) continue;
                sb.Append(c.Domain).Append('\t').Append(c.HostOnly ? "1" : "0").Append('\t').Append(c.Path).Append('\t')
                  .Append(c.Secure ? "1" : "0").Append('\t').Append(c.Expires).Append('\t').Append(c.Name).Append('\t').Append(c.Value).Append('\n');
            }
        }
        return sb.ToString();
    }

    /// <summary>Add cookies written by Save (expired lines are skipped).</summary>
    public int Load(string text)
    {
        int n = 0;
        foreach (var line in text.Split('\n'))
        {
            var f = line.Split('\t');
            if (f.Length < 7 || !long.TryParse(f[4], out long exp)) continue;
            var c = new Cookie { Domain = f[0], HostOnly = f[1] == "1", Path = f[2], Secure = f[3] == "1", Expires = exp, Name = f[5], Value = f[6].TrimEnd('\r') };
            if (c.Expires <= Now()) continue;
            lock (_lock) _cookies.Add(c);
            n++;
        }
        return n;
    }
}

public sealed class HttpClient
{
    public string UserAgent = "Buitenzorg/1.0 (Peramban)";
    public int TimeoutMs = 20000;
    public int MaxRedirects = 5;
    public int MaxBody = 8 << 20;
    /// <summary>Cookies sent and stored with every request (null = none).</summary>
    public CookieJar? Cookies;
    /// <summary>Download: bytes gathered before each write to the file.</summary>
    public int DownloadPiece = 256 * 1024;

    public HttpResponse Get(string url) => Send("GET", url, null, null);

    public HttpResponse Post(string url, string contentType, byte[] body) => Send("POST", url, contentType, body);

    public HttpResponse Send(string method, string url, string? contentType, byte[]? body)
    {
        var u = Url.Parse(url) ?? throw new NetException("alamat tidak sah: " + url);
        for (int hop = 0; ; hop++)
        {
            var r = Once(method, u, contentType, body);
            if (r.Status >= 300 && r.Status < 400 && r.Header("Location") is string loc && hop < MaxRedirects)
            {
                var next = u.Resolve(loc) ?? throw new NetException("pengalihan ke alamat tidak sah: " + loc);
                if (r.Status == 303 || ((r.Status == 301 || r.Status == 302) && method == "POST")) { method = "GET"; body = null; contentType = null; }
                u = next;
                continue;
            }
            return r;
        }
    }

    HttpResponse Once(string method, Url u, string? contentType, byte[]? body)
    {
        var s = Open(method, u, contentType, body, "", out string tls);
        try
        {
            var r = ReadResponse(s, method == "HEAD");
            Cookies?.StoreAll(u, r);
            r.Url = u.ToString();
            r.Tls = tls;
            return r;
        }
        finally { s.Close(); }
    }

    /// <summary>Connect and send the request (extra = more header lines, each ending in CRLF).</summary>
    INetStream Open(string method, Url u, string? contentType, byte[]? body, string extra, out string tls)
    {
        INetStream s;
        tls = "";
        if (u.Secure) { var t = TlsConnection.Connect(u.Host, u.Port, TimeoutMs); tls = t.Version; s = t; }
        else s = TcpConnection.Connect(u.Host, u.Port, TimeoutMs);
        s.TimeoutMs = TimeoutMs;
        try
        {
            var sb = new StringBuilder();
            sb.Append(method).Append(' ').Append(u.Path).Append(" HTTP/1.1\r\n");
            sb.Append("Host: ").Append(u.Host);
            if ((u.Secure && u.Port != 443) || (!u.Secure && u.Port != 80)) sb.Append(':').Append(u.Port);
            sb.Append("\r\nUser-Agent: ").Append(UserAgent);
            sb.Append("\r\nAccept: text/html,text/plain,image/*;q=0.8,*/*;q=0.5\r\nAccept-Encoding: identity\r\nConnection: close\r\n");
            sb.Append(extra);
            string cookie = Cookies?.Header(u) ?? "";
            if (cookie.Length > 0) sb.Append("Cookie: ").Append(cookie).Append("\r\n");
            if (body != null)
            {
                if (contentType != null) sb.Append("Content-Type: ").Append(contentType).Append("\r\n");
                sb.Append("Content-Length: ").Append(body.Length).Append("\r\n");
            }
            sb.Append("\r\n");
            var head = Encoding.UTF8.GetBytes(sb.ToString());
            s.Write(head, 0, head.Length);
            if (body != null && body.Length > 0) s.Write(body, 0, body.Length);
            return s;
        }
        catch (Exception) { s.Close(); throw; }
    }

    /// <summary>Save `url` to the file `path` without holding it in memory: the body
    /// is appended in 256 KiB pieces as it arrives. With `resume` an existing
    /// partial file is continued with a Range request (a server that ignores it
    /// sends everything again, and the file starts over). `progress(saved, total)`
    /// is called after every piece (total -1 = unknown); `cancel` returning true
    /// stops the transfer, keeping what arrived (Cancelled = true) -- a later call
    /// with resume continues it. Redirects are followed.</summary>
    public HttpResponse Download(string url, string path, Action<long, long>? progress = null, Func<bool>? cancel = null, bool resume = true)
    {
        var u = Url.Parse(url) ?? throw new NetException("alamat tidak sah: " + url);
        long have = resume ? Math.Max(0L, Buitenzorg.App.Files.Size(path)) : 0;
        for (int hop = 0; ; hop++)
        {
            var s = Open("GET", u, null, null, have > 0 ? "Range: bytes=" + have + "-\r\n" : "", out string tls);
            try
            {
                var r = ReadHead(s, out var rest);
                Cookies?.StoreAll(u, r);
                r.Url = u.ToString();
                r.Tls = tls;
                if (r.Status >= 300 && r.Status < 400 && r.Header("Location") is string loc && hop < MaxRedirects)
                {
                    u = u.Resolve(loc) ?? throw new NetException("pengalihan ke alamat tidak sah: " + loc);
                    continue;
                }
                if (r.Status == 416 && have > 0) { r.Saved = have; r.Resumed = true; return r; }   // already complete
                if (!r.Ok) return r;
                bool append = r.Status == 206 && have > 0;
                if (!append)
                {
                    have = 0;
                    if (!Buitenzorg.App.Files.WriteAllBytes(path, new byte[0])) throw new NetException("tidak bisa menulis " + path);
                }
                r.Resumed = append;
                long length = long.TryParse(r.Header("Content-Length"), out var cl) ? cl : -1;
                long total = length >= 0 ? have + length : -1;
                bool chunked = (r.Header("Transfer-Encoding") ?? "").ToLowerInvariant().Contains("chunked");
                var buf = new byte[32768];
                long got = rest.Count;            // body bytes received (raw, before de-chunking)
                var pending = rest;               // not yet written
                long saved = have;
                while (true)
                {
                    bool end = (!chunked && length >= 0 && got >= length) || (chunked && pending.EndsWith("0\r\n\r\n"));
                    if (!chunked && (pending.Count >= DownloadPiece || (end && pending.Count > 0)))
                    {
                        // write what arrived in pieces of DownloadPiece bytes: progress
                        // after each, and a cancel takes effect between them
                        int have_now = (int)Math.Min(pending.Count, length >= 0 ? have + length - saved : pending.Count);
                        int off = 0;
                        while (off < have_now && (end || have_now - off >= DownloadPiece))
                        {
                            int n = Math.Min(DownloadPiece, have_now - off);
                            if (!Buitenzorg.App.Files.AppendAllBytes(path, pending.Data, off, n)) throw new NetException("menulis " + path + " gagal (disk penuh?)");
                            saved += n;
                            off += n;
                            progress?.Invoke(saved, total);
                            if (off < have_now && cancel != null && cancel()) { r.Cancelled = true; break; }
                        }
                        if (r.Cancelled) break;   // the rest is fetched again when it resumes
                        // keep what is left (less than a piece) for the next round
                        int left = pending.Count - off;
                        if (left > 0) Array.Copy(pending.Data, off, pending.Data, 0, left);
                        pending.Count = Math.Max(0, left);
                        if (end) pending.Count = 0;
                    }
                    if (end) break;
                    if (cancel != null && cancel())
                    {
                        if (!chunked && pending.Count > 0 && Buitenzorg.App.Files.AppendAllBytes(path, pending.Data, 0, pending.Count)) saved += pending.Count;
                        r.Cancelled = true;
                        break;
                    }
                    int k = s.Read(buf, 0, buf.Length);
                    if (k <= 0)
                    {
                        if (!chunked && pending.Count > 0 && Buitenzorg.App.Files.AppendAllBytes(path, pending.Data, 0, pending.Count)) { saved += pending.Count; pending.Count = 0; }
                        if (length >= 0 && saved < have + length) r.Cancelled = true;   // cut off: resumable
                        break;
                    }
                    pending.Add(buf, 0, k);
                    got += k;
                    if (chunked && pending.Count > MaxBody) throw new NetException("jawaban chunked terlalu besar (> " + (MaxBody >> 20) + " MB)");
                }
                if (chunked && !r.Cancelled)
                {
                    var body = Dechunk(pending.Data, pending.Count);
                    if (!Buitenzorg.App.Files.AppendAllBytes(path, body, 0, body.Length)) throw new NetException("menulis " + path + " gagal");
                    saved += body.Length;
                    progress?.Invoke(saved, saved);
                }
                r.Saved = saved;
                return r;
            }
            finally { s.Close(); }
        }
    }

    /// <summary>A growable byte buffer (appends with Array.Copy, not per byte).</summary>
    sealed class Bytes
    {
        public byte[] Data = new byte[32768];
        public int Count;
        public void Add(byte[] src, int off, int n)
        {
            if (Count + n > Data.Length)
            {
                var d = new byte[Math.Max(Data.Length * 2, Count + n)];
                Array.Copy(Data, 0, d, 0, Count);
                Data = d;
            }
            Array.Copy(src, off, Data, Count, n);
            Count += n;
        }
        public bool EndsWith(string tail)
        {
            if (Count < tail.Length) return false;
            for (int i = 0; i < tail.Length; i++) if (Data[Count - tail.Length + i] != tail[i]) return false;
            return true;
        }
    }

    HttpResponse ReadResponse(INetStream s, bool headOnly)
    {
        var r = ReadHead(s, out var rest);
        if (headOnly || r.Status == 204 || r.Status == 304 || (r.Status >= 100 && r.Status < 200)) return r;
        var buf = new byte[32768];

        bool chunked = (r.Header("Transfer-Encoding") ?? "").ToLowerInvariant().Contains("chunked");
        long length = long.TryParse(r.Header("Content-Length"), out var cl) ? cl : -1;
        // we asked for "Connection: close": the body ends at its length, the
        // final chunk, or when the server closes
        while (true)
        {
            if (!chunked && length >= 0 && rest.Count >= length) break;
            if (chunked && rest.EndsWith("0\r\n\r\n")) break;
            int n = s.Read(buf, 0, buf.Length);
            if (n <= 0) break;
            rest.Add(buf, 0, n);
            if (rest.Count > MaxBody) throw new NetException("halaman terlalu besar (> " + (MaxBody >> 20) + " MB)");
        }
        if (chunked) r.Body = Dechunk(rest.Data, rest.Count);
        else
        {
            int len = length >= 0 && rest.Count > length ? (int)length : rest.Count;
            r.Body = new byte[len];
            Array.Copy(rest.Data, 0, r.Body, 0, len);
        }
        return r;
    }

    /// <summary>The status line and headers; `rest` = body bytes that came with them.</summary>
    HttpResponse ReadHead(INetStream s, out Bytes rest)
    {
        var data = new Bytes();
        var buf = new byte[32768];
        int headEnd = -1, scanned = 0;
        while (headEnd < 0)
        {
            int n = s.Read(buf, 0, buf.Length);
            if (n <= 0) break;
            data.Add(buf, 0, n);
            for (int i = Math.Max(3, scanned); i < data.Count; i++)
                if (data.Data[i] == '\n' && data.Data[i - 1] == '\r' && data.Data[i - 2] == '\n' && data.Data[i - 3] == '\r') { headEnd = i - 3; break; }
            scanned = data.Count;
            if (data.Count > 65536 && headEnd < 0) throw new NetException("kepala jawaban terlalu panjang");
        }
        if (headEnd < 0) throw new NetException("server menutup sambungan tanpa jawaban HTTP");
        string head = Encoding.ASCII.GetString(data.Data, 0, headEnd);
        var lines = head.Split('\n');
        var r = new HttpResponse();
        string first = lines[0].TrimEnd('\r');
        int sp1 = first.IndexOf(' ');
        int sp2 = sp1 < 0 ? -1 : first.IndexOf(' ', sp1 + 1);
        string code = sp1 < 0 ? "" : sp2 < 0 ? first.Substring(sp1 + 1) : first.Substring(sp1 + 1, sp2 - sp1 - 1);
        if (!first.StartsWith("HTTP/") || !int.TryParse(code, out r.Status)) throw new NetException("bukan jawaban HTTP: " + first);
        r.Reason = sp2 < 0 ? "" : first.Substring(sp2 + 1);
        for (int i = 1; i < lines.Length; i++)
        {
            string line = lines[i].TrimEnd('\r');
            int c = line.IndexOf(':');
            if (c > 0) r.Headers.Add(new KeyValuePair<string, string>(line.Substring(0, c).Trim(), line.Substring(c + 1).Trim()));
        }
        rest = new Bytes();
        rest.Add(data.Data, headEnd + 4, data.Count - headEnd - 4);
        return r;
    }

    static int HexVal(byte b) => b >= '0' && b <= '9' ? b - '0' : b >= 'a' && b <= 'f' ? b - 'a' + 10 : b >= 'A' && b <= 'F' ? b - 'A' + 10 : -1;

    public static byte[] Dechunk(byte[] d) => Dechunk(d, d.Length);

    public static byte[] Dechunk(byte[] d, int count)
    {
        var o = new Bytes();
        int p = 0;
        while (p < count)
        {
            int eol = -1; // a loop: RustBCL has no Array.IndexOf(byte[], byte, int, int)
            for (int i = p; i < count; i++) if (d[i] == '\n') { eol = i; break; }
            if (eol < 0) break;
            int size = 0;
            for (int i = p; i < eol; i++) { int h = HexVal(d[i]); if (h < 0) break; size = size * 16 + h; }
            if (size == 0) break;
            int start = eol + 1;
            int n = Math.Min(size, count - start);
            if (n <= 0) break;
            o.Add(d, start, n);
            p = start + size + 2;
        }
        var r = new byte[o.Count];
        Array.Copy(o.Data, 0, r, 0, o.Count);
        return r;
    }
}

/// <summary>Off the device: the same operations on .NET's sockets (never used on Buitenzorg).</summary>
internal static class HostNet
{
    internal sealed class Conn
    {
        internal System.Net.Sockets.TcpClient Client = null!;
        internal System.IO.Stream Stream = null!;
        internal void Write(byte[] d, int o, int c) => Stream.Write(d, o, c);
        internal int Read(byte[] b, int o, int c, int timeout) { Client.ReceiveTimeout = timeout; try { return Stream.Read(b, o, c); } catch (System.IO.IOException) { throw new NetException("waktu habis menunggu jawaban"); } }
        internal int ReadAvailable(byte[] b, int o, int c) => Client.Available > 0 ? Stream.Read(b, o, Math.Min(c, Client.Available)) : (Client.Client.Poll(0, System.Net.Sockets.SelectMode.SelectRead) ? -1 : 0);
        internal void Close() => Client.Close();
    }

    internal sealed class Tls
    {
        internal System.Net.Security.SslStream Ssl = null!;
        internal string Version = "";
        internal void Write(byte[] d, int o, int c) => Ssl.Write(d, o, c);
        internal int Read(byte[] b, int o, int c) => Ssl.Read(b, o, c);
        internal void Close() => Ssl.Close();
    }

    internal sealed class Listener
    {
        internal System.Net.Sockets.TcpListener L = null!;
        internal Conn? Accept()
        {
            if (!L.Pending()) return null;
            var c = L.AcceptTcpClient();
            return new Conn { Client = c, Stream = c.GetStream() };
        }
        internal void Close() => L.Stop();
    }

    static readonly List<byte[]> _roots = new List<byte[]>();

    internal static long Resolve(string host)
    {
        try
        {
            foreach (var a in System.Net.Dns.GetHostAddresses(host))
                if (a.AddressFamily == System.Net.Sockets.AddressFamily.InterNetwork) return Dns.ParseIp(a.ToString());
        }
        catch (Exception) { }
        return -1;
    }

    internal static Conn Connect(string ip, int port, int timeout)
    {
        var c = new System.Net.Sockets.TcpClient();
        try
        {
            if (!c.ConnectAsync(ip, port).Wait(timeout)) throw new NetException(ip + ":" + port + " tidak menjawab");
        }
        catch (AggregateException) { throw new NetException(ip + ":" + port + " menolak sambungan"); }
        return new Conn { Client = c, Stream = c.GetStream() };
    }

    internal static Listener Listen(int port)
    {
        var l = new System.Net.Sockets.TcpListener(System.Net.IPAddress.Loopback, port);
        l.Start();
        return new Listener { L = l };
    }

    internal static void AddRoot(byte[] der) => _roots.Add(der);

    internal static Tls StartTls(Conn c, string host)
    {
        var ssl = new System.Net.Security.SslStream(c.Stream, false, (_, cert, chain, errors) =>
        {
            if (errors == System.Net.Security.SslPolicyErrors.None) return true;
            if (cert == null || _roots.Count == 0) return false;
            // a root a program added: build the chain against it alone
            var ch = new System.Security.Cryptography.X509Certificates.X509Chain();
            ch.ChainPolicy.TrustMode = System.Security.Cryptography.X509Certificates.X509ChainTrustMode.CustomRootTrust;
            ch.ChainPolicy.RevocationMode = System.Security.Cryptography.X509Certificates.X509RevocationMode.NoCheck;
            foreach (var r in _roots) ch.ChainPolicy.CustomTrustStore.Add(System.Security.Cryptography.X509Certificates.X509CertificateLoader.LoadCertificate(r));
            return ch.Build(new System.Security.Cryptography.X509Certificates.X509Certificate2(cert)) && (errors & System.Net.Security.SslPolicyErrors.RemoteCertificateNameMismatch) == 0;
        });
        try { ssl.AuthenticateAsClient(host); }
        catch (Exception e) { throw new NetException("TLS ke " + host + " gagal: " + e.Message); }
        return new Tls { Ssl = ssl, Version = ssl.SslProtocol + " " + ssl.NegotiatedCipherSuite };
    }
}
