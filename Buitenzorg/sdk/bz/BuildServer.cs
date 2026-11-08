// `bz build-server`: compile C# for Buitenzorg devices on a PC
// (kernel/bzkernel/src/rbuild.rs is the client: `bz buildsrv URL`, then
// `bz build DIR [--run]` on the device, or F5 in Studio on a managed C#
// project).
//
//   bz build-server [--port 8090] [--local] [--refs DIR]
//   bz build-server --selftest           a good and a broken project, on the PC
//   bz build-server --check [--port N] [--host-ip IP]
//                                        a whole session against the connected device
//
// A request (POST /build) carries the project's .CS files; the server writes
// them with a csproj (net10.0, references Buitenzorg.App / .UI / .Three from
// userland/clr-apps/dist or --refs), runs `dotnet build`, and answers with the
// assembly (200) or the compiler errors, one per line, as
// `error: PROGRAM.CS baris 12: CS1002: ; expected` (400).

using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.RegularExpressions;

static class BuildServer
{
    static readonly object BuildLock = new();

    public sealed record Result(bool Ok, byte[] Assembly, string Errors);

    /// <summary>userland/clr-apps/dist (the Buitenzorg.* assemblies), found upwards from here.</summary>
    public static string? FindRefs()
    {
        foreach (var start in new[] { Directory.GetCurrentDirectory(), AppContext.BaseDirectory })
            for (var d = new DirectoryInfo(start); d != null; d = d.Parent)
            {
                var c = Path.Combine(d.FullName, "userland", "clr-apps", "dist");
                if (File.Exists(Path.Combine(c, "BZAPP.DLL"))) return c;
            }
        return null;
    }

    /// <summary>Parse a BZSRC 1 request: (name, files).</summary>
    public static (string Name, List<(string File, byte[] Data)> Files) ParseRequest(byte[] b)
    {
        int p = 0;
        string Line()
        {
            int e = Array.IndexOf(b, (byte)'\n', p);
            if (e < 0) throw new InvalidDataException("truncated request");
            var s = Encoding.UTF8.GetString(b, p, e - p).TrimEnd('\r');
            p = e + 1;
            return s;
        }
        if (Line() != "BZSRC 1") throw new InvalidDataException("not a BZSRC 1 request");
        var nameLine = Line();
        if (!nameLine.StartsWith("name ")) throw new InvalidDataException("no name");
        string name = new string(nameLine[5..].Where(char.IsAsciiLetterOrDigit).Take(32).ToArray());
        if (name.Length == 0 || char.IsDigit(name[0])) name = "P" + name;
        var files = new List<(string, byte[])>();
        while (p < b.Length)
        {
            var l = Line();
            if (l.Length == 0) continue;
            var f = l.Split(' ');
            if (f.Length != 3 || f[0] != "file" || !int.TryParse(f[2], out int len) || len < 0 || p + len > b.Length)
                throw new InvalidDataException("bad file header: " + l);
            string file = Path.GetFileName(f[1]);
            if (!file.EndsWith(".CS", StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("not a .CS file: " + file);
            files.Add((file.ToUpperInvariant(), b[p..(p + len)]));
            p += len + 1;
        }
        if (files.Count == 0) throw new InvalidDataException("no files");
        return (name, files);
    }

    static readonly Regex ErrorRx = new(@"^(?<file>.*?)\((?<line>\d+),(?<col>\d+)\): error (?<code>\w+): (?<msg>.*?)(?: \[[^\]]*\])?$", RegexOptions.Compiled);

    /// <summary>Build one project with dotnet.</summary>
    public static Result Build(string name, List<(string File, byte[] Data)> files, string refs)
    {
        lock (BuildLock)
        {
            string dir = Path.Combine(Path.GetTempPath(), "bzbuild", name + "-" + Guid.NewGuid().ToString("N")[..8]);
            Directory.CreateDirectory(dir);
            try
            {
                foreach (var (f, d) in files) File.WriteAllBytes(Path.Combine(dir, f), d);
                string Ref(string asm, string file) => File.Exists(Path.Combine(refs, file))
                    ? $"    <Reference Include=\"{asm}\"><HintPath>{Path.Combine(refs, file)}</HintPath></Reference>\n" : "";
                File.WriteAllText(Path.Combine(dir, name + ".csproj"),
                    "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    <TargetFramework>net10.0</TargetFramework>\n" +
                    "    <Nullable>enable</Nullable>\n    <ImplicitUsings>enable</ImplicitUsings>\n" +
                    $"    <AssemblyName>{name}</AssemblyName>\n    <RootNamespace>{name}</RootNamespace>\n  </PropertyGroup>\n  <ItemGroup>\n" +
                    Ref("Buitenzorg.App", "BZAPP.DLL") + Ref("Buitenzorg.UI", "BZUI.DLL") + Ref("Buitenzorg.Three", "BZTHREE.DLL") +
                    "  </ItemGroup>\n</Project>\n");
                var psi = new ProcessStartInfo("dotnet", "build -c Release -nologo -v q -clp:NoSummary")
                {
                    WorkingDirectory = dir,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                };
                psi.Environment["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1";
                psi.Environment["DOTNET_NOLOGO"] = "1";
                using var proc = Process.Start(psi)!;
                var stdout = proc.StandardOutput.ReadToEndAsync();
                var stderr = proc.StandardError.ReadToEndAsync();
                if (!proc.WaitForExit(240_000)) { try { proc.Kill(true); } catch { } return new Result(false, Array.Empty<byte>(), "error: dotnet build: waktu habis (240 s)"); }
                string output = stdout.Result + stderr.Result;
                string dll = Path.Combine(dir, "bin", "Release", "net10.0", name + ".dll");
                if (proc.ExitCode == 0 && File.Exists(dll)) return new Result(true, File.ReadAllBytes(dll), "");
                var errors = output.Split('\n').Select(l => ErrorRx.Match(l.Trim()))
                    .Where(m => m.Success)
                    .Select(m => $"error: {Path.GetFileName(m.Groups["file"].Value).ToUpperInvariant()} baris {m.Groups["line"].Value}: {m.Groups["code"].Value}: {m.Groups["msg"].Value}")
                    .Distinct().ToList();
                if (errors.Count == 0) errors.AddRange(output.Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0).TakeLast(12).Select(l => "error: " + l));
                return new Result(false, Array.Empty<byte>(), string.Join("\n", errors));
            }
            finally
            {
                try { Directory.Delete(dir, true); } catch { }
            }
        }
    }

    /// <summary>The HTTP side: POST /build only.</summary>
    public sealed class Server : IDisposable
    {
        readonly TcpListener _listener;
        readonly string _refs;
        volatile bool _stop;
        public int Port { get; }
        public Action<string>? Log { get; init; }

        public Server(int port, bool localOnly, string refs)
        {
            _refs = refs;
            _listener = new TcpListener(localOnly ? IPAddress.Loopback : IPAddress.Any, port);
            _listener.Start();
            Port = ((IPEndPoint)_listener.LocalEndpoint).Port;
            new Thread(() =>
            {
                while (!_stop)
                {
                    TcpClient c;
                    try { c = _listener.AcceptTcpClient(); } catch { return; }
                    ThreadPool.QueueUserWorkItem(_ => Serve(c));
                }
            }) { IsBackground = true }.Start();
        }

        void Serve(TcpClient c)
        {
            using (c)
            {
                try
                {
                    c.ReceiveTimeout = 30000;
                    var s = c.GetStream();
                    var head = new List<byte>();
                    while (head.Count < 16384)
                    {
                        int b = s.ReadByte();
                        if (b < 0) return;
                        head.Add((byte)b);
                        int n = head.Count;
                        if (n >= 4 && head[n - 4] == '\r' && head[n - 3] == '\n' && head[n - 2] == '\r' && head[n - 1] == '\n') break;
                    }
                    var lines = Encoding.ASCII.GetString(head.ToArray()).Split("\r\n");
                    var req = lines[0].Split(' ');
                    int len = lines.Select(l => l.Split(':', 2)).Where(p => p.Length == 2 && p[0].Trim().Equals("Content-Length", StringComparison.OrdinalIgnoreCase))
                        .Select(p => int.TryParse(p[1].Trim(), out var v) ? v : 0).FirstOrDefault();
                    if (len < 0 || len > 8 << 20) len = 0;
                    var body = new byte[len];
                    int got = 0;
                    while (got < len) { int r = s.Read(body, got, len - got); if (r <= 0) break; got += r; }
                    string status;
                    byte[] answer;
                    if (req.Length < 2 || req[0] != "POST" || req[1].TrimStart('/') != "build")
                    {
                        status = "404 Not Found";
                        answer = Encoding.UTF8.GetBytes("bz build-server: POST /build\n");
                    }
                    else
                    {
                        try
                        {
                            var (name, files) = ParseRequest(body);
                            var sw = Stopwatch.StartNew();
                            var r = Build(name, files, _refs);
                            status = r.Ok ? "200 OK" : "400 Bad Request";
                            answer = r.Ok ? r.Assembly : Encoding.UTF8.GetBytes(r.Errors + "\n");
                            Log?.Invoke($"{((IPEndPoint)c.Client.RemoteEndPoint!).Address} build {name} ({files.Count} file(s)) -> {(r.Ok ? answer.Length + " bytes" : "errors")} in {sw.ElapsedMilliseconds} ms");
                        }
                        catch (InvalidDataException e)
                        {
                            status = "400 Bad Request";
                            answer = Encoding.UTF8.GetBytes("error: " + e.Message + "\n");
                        }
                    }
                    s.Write(Encoding.ASCII.GetBytes($"HTTP/1.0 {status}\r\nContent-Length: {answer.Length}\r\nConnection: close\r\n\r\n"));
                    s.Write(answer);
                    s.Flush();
                }
                catch (IOException) { }
                catch (SocketException) { }
            }
        }

        public void Dispose() { _stop = true; _listener.Stop(); }
    }

    public const string GoodProgram =
        "using Buitenzorg.App;\n\n" +
        "var nums = new List<int> { 5, 3, 8, 1 };\n" +
        "nums.Sort();\n" +
        "string text = \"RBUILD \" + string.Join(\",\", nums) + \" \" + nums.Where(n => n > 2).Sum();\n" +
        "Console.WriteLine(text);\n" +
        "Files.WriteAllText(Files.ScratchDirectory + \"/RBUILD.TXT\", text);\n";

    public const string BadProgram =
        "Console.WriteLine(\"halo\");\n" +
        "int x = 1\n" +
        "Console.WriteLine(x);\n";

    static byte[] Request(string name, params (string File, string Text)[] files)
    {
        var ms = new MemoryStream();
        void W(string s) => ms.Write(Encoding.UTF8.GetBytes(s));
        W($"BZSRC 1\nname {name}\n");
        foreach (var (f, t) in files)
        {
            var d = Encoding.UTF8.GetBytes(t);
            W($"file {f} {d.Length}\n");
            ms.Write(d);
            W("\n");
        }
        return ms.ToArray();
    }

    public static int Run(string[] args)
    {
        int port = 8090;
        bool local = args.Contains("--local");
        string? refs = null;
        for (int i = 0; i + 1 < args.Length; i++)
        {
            if (args[i] == "--port" && !int.TryParse(args[i + 1], out port)) return Fail("--port N");
            if (args[i] == "--refs") refs = args[i + 1];
        }
        refs ??= FindRefs();
        if (refs == null || !File.Exists(Path.Combine(refs, "BZAPP.DLL")))
            return Fail("Buitenzorg.App not found: run from the Buitenzorg repository, or pass --refs <userland/clr-apps/dist>");
        if (args.Contains("--selftest")) return SelfTest(refs);
        using var server = new Server(port, local, refs) { Log = Console.WriteLine };
        Console.WriteLine($"bz build-server: http://{(local ? "127.0.0.1" : "0.0.0.0")}:{server.Port}/build (references from {refs}) -- Ctrl+C stops");
        Console.WriteLine($"on the device: bz buildsrv http://<this PC's IP>:{server.Port}   (QEMU user-net: http://10.0.2.2:{server.Port}), then bz build DIR --run");
        Thread.Sleep(Timeout.Infinite);
        return 0;
    }

    static int Fail(string m) { Console.Error.WriteLine("bz: " + m); return 1; }

    /// <summary>On the PC: a good and a broken project through the HTTP server.</summary>
    static int SelfTest(string refs)
    {
        int checks = 0, failed = 0;
        void Expect(bool ok, string what) { checks++; if (!ok) failed++; Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what); }
        try
        {
            using var server = new Server(0, true, refs);
            using var http = new HttpClient { BaseAddress = new Uri($"http://127.0.0.1:{server.Port}/"), Timeout = TimeSpan.FromMinutes(5) };
            var good = http.PostAsync("build", new ByteArrayContent(Request("RBTEST", ("PROGRAM.CS", GoodProgram)))).GetAwaiter().GetResult();
            var dll = good.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult();
            Expect(good.StatusCode == HttpStatusCode.OK && dll.Length > 1000 && dll[0] == 'M' && dll[1] == 'Z', $"a good project builds: {dll.Length} bytes of PE assembly");
            var bad = http.PostAsync("build", new ByteArrayContent(Request("RBBAD", ("PROGRAM.CS", BadProgram)))).GetAwaiter().GetResult();
            var text = bad.Content.ReadAsStringAsync().GetAwaiter().GetResult();
            Expect(bad.StatusCode == HttpStatusCode.BadRequest && text.Contains("error: PROGRAM.CS baris 2: CS"), "a broken one answers with its line: (" + text.Trim().Split('\n')[0] + ")");
            var junk = http.PostAsync("build", new ByteArrayContent(Encoding.ASCII.GetBytes("hello"))).GetAwaiter().GetResult();
            Expect(junk.StatusCode == HttpStatusCode.BadRequest, "a request that is not BZSRC is refused");
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        Console.WriteLine($"bz build-server: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }

    /// <summary>A whole session against the connected device.</summary>
    public static int Check(IDebugDevice dev, string[] args)
    {
        int port = 8090;
        string hostIp = "10.0.2.2";
        for (int i = 0; i + 1 < args.Length; i++)
        {
            if (args[i] == "--port") int.TryParse(args[i + 1], out port);
            if (args[i] == "--host-ip") hostIp = args[i + 1];
        }
        int checks = 0, failed = 0;
        void Expect(bool ok, string what, string got) { checks++; if (!ok) failed++; Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what + (ok ? "" : "\n         got: " + got.Trim().Replace("\n", "\n              "))); }
        string Run(string line) => dev.Exec(line).Output;
        var refs = FindRefs();
        if (refs == null) { Console.WriteLine("bz build-server: Buitenzorg.App not found (run from the repository)"); return 1; }
        try
        {
            using var server = new Server(port, true, refs) { Log = l => Console.WriteLine("  [server] " + l) };
            Run("mkdir /disk/RBTEST");
            Run("mkdir /disk/RBBAD");
            dev.Upload("/disk/RBTEST/PROGRAM.CS", Encoding.UTF8.GetBytes(GoodProgram));
            dev.Upload("/disk/RBBAD/PROGRAM.CS", Encoding.UTF8.GetBytes(BadProgram));
            var r = Run($"bz buildsrv http://{hostIp}:{server.Port}");
            Expect(r.Contains("buildsrv: http://"), "bz buildsrv URL", r);
            r = Run("bz build /disk/RBTEST --run");
            Expect(r.Contains("/disk/RBTEST/RBTEST.DLL") && r.Contains("berjalan (pid"), "bz build /disk/RBTEST --run: compiled on the PC, started on the device", r);
            string outText = "";
            for (int i = 0; i < 60 && !outText.Contains("RBUILD"); i++)
            {
                Thread.Sleep(1000);
                outText = Run("cat /ram/RBUILD.TXT") + Run("cat /disk/RBUILD.TXT");
            }
            Expect(outText.Contains("RBUILD 1,3,5,8 16"), "the assembly ran in RustCLR (LINQ, List.Sort): RBUILD 1,3,5,8 16", outText);
            r = Run("bz build /disk/RBBAD");
            Expect(r.Contains("error: PROGRAM.CS baris 2: CS"), "a compile error comes back with its line", r);
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        finally
        {
            try { Run("bz buildsrv off"); } catch { }
        }
        Console.WriteLine($"bz build-server: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }
}
