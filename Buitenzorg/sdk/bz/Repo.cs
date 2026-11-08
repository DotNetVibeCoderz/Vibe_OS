// `bz repo` on the PC: a package repository for Buitenzorg devices
// (kernel/bzkernel/src/repo.rs is the client).
//
//   bz repo index DIR                 write DIR/INDEX.TXT from the packages in DIR
//   bz repo serve DIR [--port 8080] [--local]
//                                     index, then serve DIR over HTTP
//   bz repo --selftest                package -> index -> HTTP round trip on the PC
//   bz repo --check [--port N] [--host-ip IP]
//                                     a whole session against the connected device:
//                                     update, install with a dependency, upgrade,
//                                     a tampered package refused
//
// On the device: `bz repo http://<PC>:8080`, `bz update`, `bz install NAME`,
// `bz upgrade`. Under QEMU user networking the PC is 10.0.2.2.

using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;

static class RepoCli
{
    public const string IndexName = "INDEX.TXT";

    public sealed record Pkg(string Name, string Version, string Title, string Depends);

    /// <summary>A .bzpkg archive (format v1, see bzpkg.rs).</summary>
    public static byte[] Build(string manifest, IEnumerable<(string Name, byte[] Data)> files)
    {
        var list = files.ToList();
        using var ms = new MemoryStream();
        using (var w = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: true))
        {
            w.Write("BZPK"u8.ToArray());
            w.Write((ushort)1);
            w.Write((ushort)list.Count);
            var mb = Encoding.UTF8.GetBytes(manifest);
            w.Write((uint)mb.Length);
            w.Write(mb);
            foreach (var (name, data) in list)
            {
                var fname = Encoding.ASCII.GetBytes(name.ToUpperInvariant());
                w.Write((byte)fname.Length);
                w.Write(fname);
                w.Write((uint)data.Length);
                w.Write(SHA256.HashData(data));
                w.Write(data);
            }
        }
        var body = ms.ToArray();
        return body.Concat(SHA256.HashData(body)).ToArray();
    }

    /// <summary>The manifest fields of a .bzpkg, or null when it is not one.</summary>
    public static Pkg? ReadManifest(byte[] b)
    {
        if (b.Length < 44 || b[0] != 'B' || b[1] != 'Z' || b[2] != 'P' || b[3] != 'K') return null;
        int mlen = (int)BitConverter.ToUInt32(b, 8);
        if (12 + mlen > b.Length) return null;
        var kv = Encoding.UTF8.GetString(b, 12, mlen).Split('\n')
            .Select(l => l.Split('=', 2)).Where(p => p.Length == 2)
            .GroupBy(p => p[0].Trim()).ToDictionary(g => g.Key, g => g.First()[1].Trim());
        string name = kv.GetValueOrDefault("name", "");
        if (name.Length == 0) return null;
        return new Pkg(name, kv.GetValueOrDefault("version", "0"), kv.GetValueOrDefault("title", name), kv.GetValueOrDefault("depends", ""));
    }

    /// <summary>Write DIR/INDEX.TXT; the packages it lists.</summary>
    public static List<Pkg> WriteIndex(string dir)
    {
        var sb = new StringBuilder("BZREPO 1\n");
        var pkgs = new List<Pkg>();
        foreach (var f in Directory.GetFiles(dir).OrderBy(f => f, StringComparer.Ordinal))
        {
            string ext = Path.GetExtension(f).ToLowerInvariant();
            if (ext is not (".bzpkg" or ".bzp")) continue;
            var bytes = File.ReadAllBytes(f);
            var p = ReadManifest(bytes);
            if (p == null) { Console.Error.WriteLine($"bz repo: {Path.GetFileName(f)} is not a .bzpkg, skipped"); continue; }
            string deps = string.IsNullOrWhiteSpace(p.Depends) ? "-" : string.Join(',', p.Depends.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries));
            sb.Append($"{p.Name}\t{p.Version}\t{Path.GetFileName(f)}\t{Convert.ToHexStringLower(SHA256.HashData(bytes))}\t{deps}\t{p.Title.Replace('\t', ' ')}\n");
            pkgs.Add(p);
        }
        File.WriteAllText(Path.Combine(dir, IndexName), sb.ToString());
        return pkgs;
    }

    /// <summary>A minimal HTTP/1.0 file server for DIR (GET only, no subfolders).</summary>
    public sealed class Server : IDisposable
    {
        readonly TcpListener _listener;
        readonly string _dir;
        readonly Thread _thread;
        volatile bool _stop;
        public int Port { get; }
        public Action<string>? Log { get; init; }

        public Server(string dir, int port, bool localOnly)
        {
            _dir = Path.GetFullPath(dir);
            _listener = new TcpListener(localOnly ? IPAddress.Loopback : IPAddress.Any, port);
            _listener.Start();
            Port = ((IPEndPoint)_listener.LocalEndpoint).Port;
            _thread = new Thread(Loop) { IsBackground = true };
            _thread.Start();
        }

        void Loop()
        {
            while (!_stop)
            {
                TcpClient c;
                try { c = _listener.AcceptTcpClient(); }
                catch { return; }
                ThreadPool.QueueUserWorkItem(_ => Serve(c));
            }
        }

        void Serve(TcpClient c)
        {
            using (c)
            {
                try
                {
                    c.ReceiveTimeout = 10000;
                    var s = c.GetStream();
                    var head = new List<byte>();
                    while (head.Count < 8192)
                    {
                        int b = s.ReadByte();
                        if (b < 0) return;
                        head.Add((byte)b);
                        int n = head.Count;
                        if (n >= 4 && head[n - 4] == '\r' && head[n - 3] == '\n' && head[n - 2] == '\r' && head[n - 1] == '\n') break;
                    }
                    var line = Encoding.ASCII.GetString(head.ToArray()).Split("\r\n")[0].Split(' ');
                    string name = line.Length >= 2 ? Uri.UnescapeDataString(line[1].TrimStart('/')) : "";
                    string path = Path.Combine(_dir, name);
                    byte[] body;
                    string status;
                    // only plain file names inside the folder
                    if (line[0] != "GET" || name.Length == 0 || name.Contains('/') || name.Contains('\\') || name.Contains("..") || !File.Exists(path))
                    {
                        status = "404 Not Found";
                        body = Encoding.ASCII.GetBytes("not found\n");
                    }
                    else
                    {
                        status = "200 OK";
                        body = File.ReadAllBytes(path);
                    }
                    Log?.Invoke($"{((IPEndPoint)c.Client.RemoteEndPoint!).Address} GET /{name} -> {status} ({body.Length} bytes)");
                    var h = Encoding.ASCII.GetBytes($"HTTP/1.0 {status}\r\nContent-Length: {body.Length}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n");
                    s.Write(h);
                    s.Write(body);
                    s.Flush();
                }
                catch (IOException) { }
                catch (SocketException) { }
            }
        }

        public void Dispose() { _stop = true; _listener.Stop(); }
    }

    public static int Run(string[] args)
    {
        switch (args)
        {
            case ["index", var dir]:
            {
                if (!Directory.Exists(dir)) return Fail($"not a folder: {dir}");
                var pkgs = WriteIndex(dir);
                Console.WriteLine($"{Path.Combine(dir, IndexName)}: {pkgs.Count} package(s)");
                foreach (var p in pkgs) Console.WriteLine($"  {p.Name} {p.Version}" + (p.Depends.Length > 0 ? $" (depends {p.Depends})" : ""));
                return 0;
            }
            case ["serve", var dir, .. var rest]:
            {
                if (!Directory.Exists(dir)) return Fail($"not a folder: {dir}");
                int port = 8080;
                bool local = rest.Contains("--local");
                int i = Array.IndexOf(rest, "--port");
                if (i >= 0 && i + 1 < rest.Length && !int.TryParse(rest[i + 1], out port)) return Fail("--port N");
                var pkgs = WriteIndex(dir);
                using var server = new Server(dir, port, local) { Log = Console.WriteLine };
                Console.WriteLine($"bz repo: {pkgs.Count} package(s) from {Path.GetFullPath(dir)} on http://{(local ? "127.0.0.1" : "0.0.0.0")}:{server.Port}/ -- Ctrl+C stops");
                Console.WriteLine($"on the device: bz repo http://<this PC's IP>:{server.Port}   (QEMU user-net: http://10.0.2.2:{server.Port}), then bz update");
                Thread.Sleep(Timeout.Infinite);
                return 0;
            }
            case ["publish", var file, .. var rest]:
            {
                int i = Array.IndexOf(rest, "--repo");
                string dir = i >= 0 && i + 1 < rest.Length ? rest[i + 1] : "repo";
                var (ok, msg) = Publish(file, dir, rest.Contains("--force"));
                Console.WriteLine(msg);
                return ok ? 0 : 1;
            }
            case ["--selftest"]:
                return SelfTest();
            default:
                return Fail("usage: bz repo index DIR | bz repo serve DIR [--port N] [--local] | bz repo --selftest | bz repo --check [--port N] [--host-ip IP]");
        }
    }

    static int Fail(string m) { Console.Error.WriteLine("bz: " + m); return 1; }

    /// <summary>`bz publish PKG [--repo DIR] [--force]`: copy a package into a
    /// repository folder and rewrite its index (a running `bz repo serve` of
    /// that folder serves it at once). The same name + version again is refused
    /// unless --force; a newer version keeps the older file next to it.</summary>
    public static (bool Ok, string Message) Publish(string file, string dir, bool force)
    {
        if (!File.Exists(file)) return (false, $"bz publish: not found: {file}");
        var bytes = File.ReadAllBytes(file);
        var p = ReadManifest(bytes);
        if (p == null) return (false, $"bz publish: {file} is not a .bzpkg");
        Directory.CreateDirectory(dir);
        foreach (var f in Directory.GetFiles(dir))
        {
            string ext = Path.GetExtension(f).ToLowerInvariant();
            if (ext is not (".bzpkg" or ".bzp")) continue;
            var q = ReadManifest(File.ReadAllBytes(f));
            if (q != null && q.Name == p.Name && q.Version == p.Version && !force)
                return (false, $"bz publish: {p.Name} {p.Version} is already in {dir} ({Path.GetFileName(f)}); bump the version or pass --force");
        }
        // one file per version: NAME-VERSION.bzpkg (the device fetches it by this name)
        string safeVer = new string(p.Version.Select(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '-' ? c : '_').ToArray());
        string target = Path.Combine(dir, $"{p.Name}-{safeVer}.bzpkg");
        File.WriteAllBytes(target, bytes);
        var pkgs = WriteIndex(dir);
        return (true, $"published {p.Name} {p.Version} -> {target}; {Path.Combine(dir, IndexName)} lists {pkgs.Count} package file(s)");
    }

    static byte[] ScriptPackage(string name, string version, string file, string script, string depends = "") =>
        Build($"name={name}\nversion={version}\ntitle={name} (uji repo)\nkind=console\ncategory=LAINNYA\nentry={file}\n" + (depends.Length > 0 ? $"depends={depends}\n" : ""),
              new[] { (file, Encoding.ASCII.GetBytes(script)) });

    /// <summary>On the PC only: packages -> index -> served -> fetched back.</summary>
    static int SelfTest()
    {
        int checks = 0, failed = 0;
        void Expect(bool ok, string what) { checks++; if (!ok) failed++; Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what); }
        string dir = Path.Combine(Path.GetTempPath(), "bzrepo-" + Environment.ProcessId);
        Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllBytes(Path.Combine(dir, "RLIB.BZP"), ScriptPackage("repolib", "1.0", "RLIB.SH", "echo repolib\n"));
            File.WriteAllBytes(Path.Combine(dir, "RAPP.BZP"), ScriptPackage("repoapp", "1.10", "RAPP.SH", "echo repoapp\n", "repolib"));
            File.WriteAllText(Path.Combine(dir, "NOTES.TXT"), "not a package");
            var pkgs = WriteIndex(dir);
            Expect(pkgs.Count == 2, "index: 2 packages (a text file skipped)");
            using var server = new Server(dir, 0, localOnly: true);
            using var http = new HttpClient { BaseAddress = new Uri($"http://127.0.0.1:{server.Port}/") };
            var index = http.GetStringAsync(IndexName).GetAwaiter().GetResult();
            var lines = index.Split('\n', StringSplitOptions.RemoveEmptyEntries);
            Expect(lines[0] == "BZREPO 1" && lines.Length == 3, "INDEX.TXT served: header + 2 entries");
            var app = lines.Select(l => l.Split('\t')).First(f => f[0] == "repoapp");
            Expect(app[1] == "1.10" && app[4] == "repolib" && app[2] == "RAPP.BZP", "repoapp 1.10 depends on repolib");
            var bytes = http.GetByteArrayAsync(app[2]).GetAwaiter().GetResult();
            Expect(Convert.ToHexStringLower(SHA256.HashData(bytes)) == app[3], "the package's SHA-256 matches the index");
            var miss = http.GetAsync("../secret").GetAwaiter().GetResult();
            var none = http.GetAsync("NOPE.BZP").GetAwaiter().GetResult();
            Expect(miss.StatusCode == HttpStatusCode.NotFound && none.StatusCode == HttpStatusCode.NotFound, "outside the folder / missing -> 404");

            // bz publish: a new version next to the old one; the same version again refused
            string pkgFile = Path.Combine(dir, "..", "bzpub-" + Environment.ProcessId + ".bzpkg");
            File.WriteAllBytes(pkgFile, ScriptPackage("repoapp", "1.11", "RAPP.SH", "echo repoapp 1.11\n", "repolib"));
            var (ok1, _) = Publish(pkgFile, dir, force: false);
            var (ok2, again) = Publish(pkgFile, dir, force: false);
            File.Delete(pkgFile);
            var idx = File.ReadAllLines(Path.Combine(dir, IndexName));
            Expect(ok1 && idx.Count(l => l.StartsWith("repoapp\t")) == 2 && idx.Any(l => l.StartsWith("repoapp\t1.11\trepoapp-1.11.bzpkg")),
                   "bz publish repoapp 1.11: next to 1.10, both in the index");
            Expect(!ok2 && again.Contains("already"), "publishing the same version again is refused");
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        finally { try { Directory.Delete(dir, true); } catch { } }
        Console.WriteLine($"bz repo: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }

    /// <summary>A whole session against the connected device.</summary>
    public static int Check(IDebugDevice dev, string[] args)
    {
        int port = 8089;
        string hostIp = "10.0.2.2";
        for (int i = 0; i + 1 < args.Length; i++)
        {
            if (args[i] == "--port") int.TryParse(args[i + 1], out port);
            if (args[i] == "--host-ip") hostIp = args[i + 1];
        }
        int checks = 0, failed = 0;
        void Expect(bool ok, string what, string got) { checks++; if (!ok) failed++; Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what + (ok ? "" : "\n         got: " + got.Trim().Replace("\n", "\n              "))); }
        string Run(string line) => dev.Exec(line).Output;
        string dir = Path.Combine(Path.GetTempPath(), "bzrepo-check-" + Environment.ProcessId);
        Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllBytes(Path.Combine(dir, "RLIB.BZP"), ScriptPackage("repolib", "1.0", "RLIB.SH", "echo repolib-satu\n"));
            File.WriteAllBytes(Path.Combine(dir, "RAPP.BZP"), ScriptPackage("repoapp", "1.0", "RAPP.SH", "echo repoapp-v1\n", "repolib"));
            File.WriteAllBytes(Path.Combine(dir, "RBAD.BZP"), ScriptPackage("repobad", "1.0", "RBAD.SH", "echo bad\n"));
            WriteIndex(dir);
            // a tampered package: the bytes no longer match the index
            File.WriteAllBytes(Path.Combine(dir, "RBAD.BZP"), ScriptPackage("repobad", "1.0", "RBAD.SH", "echo evil\n"));
            using var server = new Server(dir, port, localOnly: true);
            Console.WriteLine($"bz repo: serving {dir} on 127.0.0.1:{server.Port} (the device sees http://{hostIp}:{server.Port})");
            foreach (var p in new[] { "repoapp", "repolib", "repobad" }) Run("bz remove " + p);

            var r = Run($"bz repo http://{hostIp}:{server.Port}");
            Expect(r.Contains("bz update"), "bz repo URL", r);
            r = Run("bz update");
            Expect(r.Contains("3 paket"), "bz update: 3 packages in the index", r);
            r = Run("bz search repo");
            Expect(r.Contains("[ ] repoapp") && r.Contains("butuh repolib"), "bz search lists the repo with dependencies", r);
            r = Run("bz install repoapp");
            Expect(r.Contains("dependensi: repolib") && r.Contains("terpasang: repolib") && r.Contains("terpasang: repoapp"), "bz install repoapp: repolib first (dependency), then repoapp", r);
            r = Run("repoapp");
            Expect(r.Contains("repoapp-v1"), "the installed package runs", r);

            File.WriteAllBytes(Path.Combine(dir, "RAPP.BZP"), ScriptPackage("repoapp", "1.1", "RAPP.SH", "echo repoapp-v2\n", "repolib"));
            WriteIndex(dir);
            File.WriteAllBytes(Path.Combine(dir, "RBAD.BZP"), ScriptPackage("repobad", "1.0", "RBAD.SH", "echo evil\n").Concat(new byte[] { 1 }).ToArray());
            r = Run("bz update");
            Expect(r.Contains("repoapp 1.0 -> 1.1"), "bz update sees repoapp 1.1", r);
            r = Run("bz upgrade");
            Expect(r.Contains("diperbarui: repoapp 1.0 -> 1.1"), "bz upgrade installs it", r);
            r = Run("repoapp");
            Expect(r.Contains("repoapp-v2"), "the new version runs", r);
            r = Run("bz install repobad");
            Expect(r.Contains("SHA-256 tidak cocok") || r.Contains("checksum"), "a package that does not match the index is refused", r);
            r = Run("bz rollback repoapp");
            Expect(r.Contains("dikembalikan: repoapp 1.1 -> 1.0"), "bz rollback repoapp: back to 1.0", r);
            r = Run("repoapp");
            Expect(r.Contains("repoapp-v1"), "the old version runs again", r);
            r = Run("bz rollback repoapp");
            Expect(r.Contains("1.0 -> 1.1"), "a second rollback goes forward to 1.1", r);
            r = Run("bz install repolib");
            Expect(r.Contains("sudah terpasang"), "installing an installed package is a no-op", r);
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        finally
        {
            foreach (var p in new[] { "repoapp", "repolib", "repobad" }) { try { dev.Exec("bz remove " + p); } catch { } }
            try { dev.Exec("bz repo off"); } catch { }
            try { Directory.Delete(dir, true); } catch { }
        }
        Console.WriteLine($"bz repo: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }
}
