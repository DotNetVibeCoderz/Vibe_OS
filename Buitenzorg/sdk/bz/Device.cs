// bz-link: the host side of the device commands (`bz device / ls / push /
// pull / run / shell / logs / kill / deploy`) over BZDP v1 (UDP 7070), the
// protocol of kernel/bzkernel/src/deploy.rs. The client itself is shared with
// the Magic App Generator (tools/MagicAppGen/Services/BzdpClient.cs).
//
// On the device: `deploy on [code]` in the Terminal starts the service and
// shows the pairing code. Under QEMU forward the port:
//   -netdev user,id=n0,hostfwd=udp::7070-:7070 -device e1000,netdev=n0

using System.Text.Json;
using MagicAppGen.Services;

static class Device
{
    sealed record Target(string Host, int Port, int Code);

    static string ConfigPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".bz", "device.json");

    /// <summary>The device from BZ_DEVICE (host[:port]) + BZ_CODE, else ~/.bz/device.json.</summary>
    static Target? Load()
    {
        var env = Environment.GetEnvironmentVariable("BZ_DEVICE");
        if (!string.IsNullOrWhiteSpace(env))
        {
            var (h, p) = SplitHost(env);
            int.TryParse(Environment.GetEnvironmentVariable("BZ_CODE"), out var c);
            return new Target(h, p, c);
        }
        if (!File.Exists(ConfigPath)) return null;
        try { return JsonSerializer.Deserialize<Target>(File.ReadAllText(ConfigPath)); }
        catch { return null; }
    }

    static (string Host, int Port) SplitHost(string s)
    {
        int colon = s.LastIndexOf(':');
        if (colon > 0 && int.TryParse(s[(colon + 1)..], out var port)) return (s[..colon], port);
        return (s, BzdpClient.DefaultPort);
    }

    // generous: the device serves requests from its desktop loop, and the first READ
    // of a large file reads all of it from disk before answering
    // Transports: "HOST" = UDP, "tcp://HOST" = TCP, "serial://COM5[@115200]" = a
    // serial port (the device's COM2; the same frames as TCP).
    static BzdpClient Client(Target t)
    {
        if (t.Host.StartsWith("serial://", StringComparison.OrdinalIgnoreCase))
        {
            var spec = t.Host[9..].Split('@');
            int baud = spec.Length > 1 && int.TryParse(spec[1], out var b) ? b : 115200;
            return new BzdpClient(t.Host, () =>
            {
                var sp = new System.IO.Ports.SerialPort(spec[0], baud, System.IO.Ports.Parity.None, 8, System.IO.Ports.StopBits.One)
                { ReadTimeout = System.Threading.Timeout.Infinite, WriteTimeout = 5000 };
                sp.Open();
                return sp.BaseStream;
            }, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(2), Attempts = 8 };
        }
        return new(t.Host, t.Port, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(2), Attempts = 8 };
    }

    /// <summary>A client that waits up to a minute for an answer.</summary>
    static BzdpClient PatientClient(Target t) =>
        t.Host.Contains("://") ? Client(t) : new BzdpClient(t.Host, t.Port, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(2), Attempts = 30 };

    static int Fail(string m) { Console.Error.WriteLine("bz: " + m); return 1; }

    /// <summary>Entry for every device command; false = not one of them.</summary>
    public static int? Run(string[] args)
    {
        try
        {
            return args switch
            {
                ["device", "connect", var host, var code] => Connect(host, code),
                ["device", "forget"] => Forget(),
                ["device"] or ["device", "info"] => WithDevice(Info),
                ["device", ..] => Fail("usage: bz device [connect <host[:port]> <code> | info | forget]"),
                ["ls"] => WithDevice(t => Ls(t, "/disk")),
                ["ls", var path] => WithDevice(t => Ls(t, path)),
                ["push", var local, var remote] => WithDevice(t => Push(t, local, remote)),
                ["pull", var remote] => WithDevice(t => Pull(t, remote, Path.GetFileName(remote))),
                ["pull", var remote, var local] => WithDevice(t => Pull(t, remote, local)),
                ["run", .. var rest] when rest.Length > 0 => WithDevice(t => Exec(t, string.Join(' ', rest))),
                ["shell"] => WithDevice(Shell),
                ["logs"] => WithDevice(t => Logs(t, follow: false)),
                ["logs", "-f"] or ["logs", "--follow"] => WithDevice(t => Logs(t, follow: true)),
                ["logs", var app] => WithDevice(t => AppLogs(t, app, follow: false)),
                ["logs", var app, "-f" or "--follow"] => WithDevice(t => AppLogs(t, app, follow: true)),
                ["logs", "-f" or "--follow", var fapp] => WithDevice(t => AppLogs(t, fapp, follow: true)),
                ["pi", "update", var kernel, .. var rest] => WithDevice(t => PiUpdate(t, kernel, rest)),
                ["pi", ..] => Fail("usage: bz pi update <kernel8.img> [--expect TEXT]... [--run CMD]... [--no-promote] [--wait SECONDS]"),
                ["kill", var pid] when uint.TryParse(pid, out var p) => WithDevice(t => Kill(t, p)),
                ["prof", .. var rest] when rest.Length > 0 => WithDevice(t => Prof(t, rest)),
                ["gdb-check", .. var rest] => WithDevice(t => GdbCheck.Check(new BzdpDebugDevice(Client(t)), rest)),
                // `bz build` holds the device's shell until the PC has compiled (20 s+ on a busy machine)
                ["build-server", "--check", .. var rest] => WithDevice(t => BuildServer.Check(new BzdpDebugDevice(PatientClient(t)), rest)),
                ["repo", "--check", .. var rest] => WithDevice(t => RepoCli.Check(new BzdpDebugDevice(Client(t)), rest)),
                ["dap", .. var rest] => DapCli.Run(rest, () => Load() is Target t ? new BzdpDebugDevice(Client(t)) : null),
                ["debug", .. var rest] => RemoteDebug.Run(rest, () => Load() is Target t ? new BzdpDebugDevice(Client(t)) : null),
                _ => null,
            };
        }
        catch (BzdpTimeoutException e) { return Fail(e.Message); }
        catch (BzdpException e) { return Fail(e.Message); }
    }

    static int WithDevice(Func<Target, int> f)
    {
        var t = Load();
        if (t is null)
            return Fail("no device: run 'bz device connect <host[:port]> <code>' (the code is shown by 'deploy on' on the device), or set BZ_DEVICE / BZ_CODE");
        return f(t);
    }

    static int Connect(string hostPort, string codeText)
    {
        if (!int.TryParse(codeText, out var code) || code is < 0 or > 9999)
            return Fail("the pairing code is 4 digits (shown by 'deploy on' on the device)");
        var (host, port) = SplitHost(hostPort);
        var t = new Target(host, port, code);
        using var c = Client(t);
        var info = c.HelloAsync().GetAwaiter().GetResult();
        Directory.CreateDirectory(Path.GetDirectoryName(ConfigPath)!);
        File.WriteAllText(ConfigPath, JsonSerializer.Serialize(t));
        Console.WriteLine($"connected: {info.Text}");
        Console.WriteLine($"saved to {ConfigPath}");
        return 0;
    }

    static int Forget()
    {
        if (File.Exists(ConfigPath)) File.Delete(ConfigPath);
        Console.WriteLine("device forgotten");
        return 0;
    }

    static int Info(Target t)
    {
        using var c = Client(t);
        var info = c.HelloAsync().GetAwaiter().GetResult();
        Console.WriteLine($"{t.Host}:{t.Port}  {info.Text}");
        return 0;
    }

    static int Ls(Target t, string path)
    {
        using var c = Client(t);
        foreach (var n in c.ListAsync(path).GetAwaiter().GetResult()) Console.WriteLine(n);
        return 0;
    }

    /// <summary>A file name the device's FAT accepts as it is (8.3, upper case).</summary>
    public static bool Is83(string name)
    {
        var parts = name.Split('.');
        bool ok(string p, int max) => p.Length is > 0 && p.Length <= max && p.All(ch => char.IsAsciiLetterOrDigit(ch) || ch is '_' or '-');
        return parts.Length switch { 1 => ok(parts[0], 8), 2 => ok(parts[0], 8) && ok(parts[1], 3), _ => false };
    }

    /// <summary>The remote path for `remote`: a directory (ends with / or names a mount) gets the local file's name.</summary>
    static string RemotePath(string local, string remote)
    {
        string name = Path.GetFileName(local).ToUpperInvariant();
        if (remote.EndsWith('/') || remote is "/disk" or "/ram") return remote.TrimEnd('/') + "/" + name;
        return remote;
    }

    static int Push(Target t, string local, string remote)
    {
        if (!File.Exists(local)) return Fail($"file not found: {local}");
        if (!remote.StartsWith('/')) return Fail("the remote path must be absolute (/disk/... or /ram/...)");
        remote = RemotePath(local, remote);
        if (!Is83(Path.GetFileName(remote))) return Fail($"{Path.GetFileName(remote)} is not an 8.3 name (FAT on the device)");
        Upload(t, local, remote);
        return 0;
    }

    static void Upload(Target t, string local, string remote)
    {
        var data = File.ReadAllBytes(local);
        using var c = Client(t);
        c.HelloAsync().GetAwaiter().GetResult(); // learn max_chunk
        var sw = System.Diagnostics.Stopwatch.StartNew();
        c.UploadAsync(remote, data).GetAwaiter().GetResult();
        Console.WriteLine($"{local} -> {remote} ({data.Length} bytes, {sw.ElapsedMilliseconds} ms)");
    }

    static int Pull(Target t, string remote, string local)
    {
        using var c = Client(t);
        var data = c.ReadAsync(remote).GetAwaiter().GetResult();
        File.WriteAllBytes(local, data);
        Console.WriteLine($"{remote} -> {local} ({data.Length} bytes)");
        return 0;
    }

    static int Exec(Target t, string line)
    {
        using var c = Client(t);
        var (failed, output) = c.ExecAsync(line).GetAwaiter().GetResult();
        Console.Write(output);
        return failed ? 1 : 0;
    }

    static int Shell(Target t)
    {
        using var c = Client(t);
        var info = c.HelloAsync().GetAwaiter().GetResult();
        Console.WriteLine($"{info.Text} -- each line runs in the device's shell; 'exit' leaves");
        while (true)
        {
            Console.Write("bz> ");
            var line = Console.ReadLine();
            if (line is null || line.Trim() is "exit" or "quit") return 0;
            if (line.Trim().Length == 0) continue;
            try
            {
                var (failed, output) = c.ExecAsync(line).GetAwaiter().GetResult();
                Console.Write(output);
                if (failed) Console.WriteLine("(failed)");
            }
            catch (BzdpException e) { Console.WriteLine("bz: " + e.Message); }
        }
    }

    /// <summary>`bz prof APP [args] [--seconds N]` / `bz prof --pid N [--seconds N]`:
    /// profile a program on the device (appprof.rs) and print its report when it
    /// ends, or after N seconds.</summary>
    static int Prof(Target t, string[] args)
    {
        int seconds = 0;
        var rest = new List<string>();
        for (int i = 0; i < args.Length; i++)
        {
            if (args[i] == "--seconds" && i + 1 < args.Length && int.TryParse(args[i + 1], out var s)) { seconds = s; i++; }
            else rest.Add(args[i]);
        }
        string start = rest is ["--pid", var pid] ? "prof pid " + pid : "prof app " + string.Join(' ', rest);
        using var c = Client(t);
        var (_, output) = c.ExecAsync(start).GetAwaiter().GetResult();
        Console.Write(output);
        if (!output.StartsWith("prof: memprofil")) return 1; // the shell guesses "failed" for any "prof: ..." line
        var sw = System.Diagnostics.Stopwatch.StartNew();
        while (true)
        {
            Thread.Sleep(1000);
            var (_, show) = c.ExecAsync("prof show").GetAwaiter().GetResult();
            bool ended = show.StartsWith("prof: tidak sedang");
            if (ended || (seconds > 0 && sw.Elapsed.TotalSeconds >= seconds))
            {
                var (_, report) = c.ExecAsync(ended ? "prof last" : "prof stop").GetAwaiter().GetResult();
                Console.Write(report);
                return 0;
            }
        }
    }

    /// <summary>`bz logs APP [-f]`: what one program printed (applog.rs on the
    /// device); -f follows it until it ends.</summary>
    static int AppLogs(Target t, string app, bool follow)
    {
        using var c = Client(t);
        if (!follow)
        {
            var (failed, output) = c.ExecAsync("logs " + app).GetAwaiter().GetResult();
            Console.Write(output);
            return output.StartsWith("logs: tidak ada") ? 1 : 0;
        }
        ulong since = 0;
        while (true)
        {
            var (_, output) = c.ExecAsync($"logs {app} --since {since}").GetAwaiter().GetResult();
            var lines = output.Split('\n');
            if (!lines[0].StartsWith("@next "))
            {
                Console.Write(output); // no such program (yet): keep waiting for it
                Thread.Sleep(1000);
                continue;
            }
            var head = lines[0].Split(' ', 3);
            since = ulong.Parse(head[1]);
            foreach (var l in lines.Skip(1).Where((l, i) => i < lines.Length - 2 || l.Length > 0)) Console.WriteLine(l);
            if (head.Length > 2 && head[2].StartsWith("exit"))
            {
                Console.WriteLine($"({app}: {head[2].Trim()})");
                return 0;
            }
            Thread.Sleep(1000);
        }
    }

    static int Logs(Target t, bool follow)
    {
        using var c = Client(t);
        uint cursor = 0;
        while (true)
        {
            var (next, text) = c.LogAsync(cursor).GetAwaiter().GetResult();
            Console.Write(text);
            bool more = next != cursor && text.Length > 0;
            cursor = next;
            if (!more)
            {
                if (!follow) return 0;
                Thread.Sleep(300);
            }
        }
    }

    /// <summary>
    /// `bz pi update KERNEL`: try a new kernel on a Raspberry Pi without
    /// touching its SD card. Upload it as /disk/KERNTRY.IMG, `pi tryboot` (one
    /// boot with it, guarded by the PM watchdog), wait until the board answers
    /// again, check it (`--expect` text in its log, `--run` commands that must
    /// succeed) and `pi promote` it. A kernel that fails a check is not kept:
    /// the board is restarted into the old one. A kernel that hangs is reset by
    /// the watchdog into the old one.
    /// </summary>
    static int PiUpdate(Target t, string kernel, string[] args)
    {
        if (!File.Exists(kernel)) return Fail($"file not found: {kernel}");
        var expects = new List<string>();
        var runs = new List<string>();
        bool promote = true;
        int waitS = 180;
        for (int i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--expect" when i + 1 < args.Length: expects.Add(args[++i]); break;
                case "--run" when i + 1 < args.Length: runs.Add(args[++i]); break;
                case "--no-promote": promote = false; break;
                case "--wait" when i + 1 < args.Length && int.TryParse(args[i + 1], out var w): waitS = w; i++; break;
                default: return Fail($"pi update: unknown option {args[i]}");
            }
        }
        string Pi(BzdpClient c) => c.ExecAsync("pi").GetAwaiter().GetResult().Output;
        using (var c = Client(t))
        {
            var before = Pi(c);
            if (!before.Contains("boot:")) return Fail("the device has no 'pi' command (not a Raspberry Pi kernel?):\n" + before);
            Console.WriteLine("device: " + before.Split('\n')[0]);
        }
        Upload(t, kernel, "/disk/KERNTRY.IMG");
        // `pi tryboot` restarts the board before it can answer: send it once,
        // never resend (a resent request would reach the trial kernel)
        using (var once = new BzdpClient(t.Host, t.Port, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(3), Attempts = 1 })
        {
            try
            {
                var (failed, output) = once.ExecAsync("pi tryboot").GetAwaiter().GetResult();
                if (failed || output.Contains("pi tryboot:")) return Fail(output.Trim());
            }
            catch (BzdpTimeoutException) { /* restarting */ }
        }
        Console.WriteLine($"restarting into the trial kernel; waiting up to {waitS} s for the board ...");
        var sw = System.Diagnostics.Stopwatch.StartNew();
        Thread.Sleep(5000);
        string state = "";
        while (sw.Elapsed.TotalSeconds < waitS)
        {
            try
            {
                using var c = new BzdpClient(t.Host, t.Port, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(2), Attempts = 1 };
                state = Pi(c);
                if (state.Contains("boot:")) break;
            }
            catch (Exception e) when (e is BzdpException or BzdpTimeoutException or System.Net.Sockets.SocketException) { }
            Thread.Sleep(3000);
        }
        if (!state.Contains("boot:")) return Fail($"the board did not answer within {waitS} s (the watchdog returns it to the old kernel)");
        if (!state.Contains("TRIAL"))
            return Fail("the board came back with the OLD kernel: the trial kernel hung or was reset by the watchdog\n" + state);
        Console.WriteLine($"trial kernel up after {sw.Elapsed.TotalSeconds:F0} s");
        int failedChecks = 0;
        using (var c = Client(t))
        {
            var log = new System.Text.StringBuilder();
            uint cursor = 0;
            while (true)
            {
                var (next, text) = c.LogAsync(cursor).GetAwaiter().GetResult();
                log.Append(text);
                if (next == cursor || text.Length == 0) break;
                cursor = next;
            }
            foreach (var e in expects)
            {
                bool ok = log.ToString().Contains(e);
                if (!ok) failedChecks++;
                Console.WriteLine((ok ? "  ok   log has: " : "  FAIL log lacks: ") + e);
            }
            foreach (var r in runs)
            {
                var (failed, output) = c.ExecAsync(r).GetAwaiter().GetResult();
                if (failed) failedChecks++;
                Console.WriteLine((failed ? "  FAIL " : "  ok   ") + r + (failed ? "\n         " + output.Trim().Replace("\n", "\n         ") : ""));
            }
            if (failedChecks > 0 || !promote)
            {
                Console.WriteLine(failedChecks > 0 ? $"{failedChecks} check(s) failed: restarting into the old kernel" : "--no-promote: restarting into the old kernel");
                using var once = new BzdpClient(t.Host, t.Port, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(3), Attempts = 1 };
                try { once.ExecAsync("restart").GetAwaiter().GetResult(); } catch (Exception e) when (e is BzdpException or BzdpTimeoutException) { }
                return failedChecks > 0 ? 1 : 0;
            }
            var (pf, po) = c.ExecAsync("pi promote").GetAwaiter().GetResult();
            Console.Write(po);
            return pf || !po.Contains("KERNEL8.IMG is now") ? 1 : 0;
        }
    }

    static int Kill(Target t, uint pid)
    {
        using var c = Client(t);
        c.KillAsync(pid).GetAwaiter().GetResult();
        Console.WriteLine($"killed {pid}");
        return 0;
    }

    /// <summary>The `name=` of a .bzpkg whose manifest names an `entry=` (else null).
    /// Format v1: "BZPK", u16 version, u16 files, u32 manifest length, manifest.</summary>
    static string? PackageName(string path)
    {
        var b = File.ReadAllBytes(path);
        if (b.Length < 12 || b[0] != 'B' || b[1] != 'Z' || b[2] != 'P' || b[3] != 'K') return null;
        int len = BitConverter.ToInt32(b, 8);
        if (len <= 0 || 12 + len > b.Length) return null;
        string name = "", entry = "";
        foreach (var line in System.Text.Encoding.UTF8.GetString(b, 12, len).Split('\n'))
        {
            if (line.StartsWith("name=")) name = line[5..].Trim();
            if (line.StartsWith("entry=")) entry = line[6..].Trim();
        }
        return name.Length > 0 && entry.Length > 0 ? name : null;
    }

    /// <summary>The 8.3 /disk name a local file is deployed as.</summary>
    static string? RemoteName(string local, string? asName)
    {
        string name = (asName ?? Path.GetFileName(local)).ToUpperInvariant();
        if (Path.GetExtension(name) is ".BZPKG") name = Path.GetFileNameWithoutExtension(name) + ".BZP";
        if (Is83(name)) return name;
        string stem = new(Path.GetFileNameWithoutExtension(name).Where(char.IsAsciiLetterOrDigit).Take(8).ToArray());
        string e3 = new(Path.GetExtension(name).TrimStart('.').Take(3).ToArray());
        name = e3.Length > 0 ? $"{stem}.{e3}" : stem;
        return Is83(name) ? name : null;
    }

    static bool IsScript(string name) => Path.GetExtension(name) is ".JS" or ".TS" or ".PY" or ".CSX";

    /// <summary>`bz deploy X --watch`: deploy, then deploy again on every save of `watched`
    /// (a file or a project folder; `rebuild` repacks a project first). A UI script runs with
    /// `--watch` on the device and reloads in its window; a program or package is stopped and
    /// started again. The device log streams meanwhile; Ctrl+C ends it.</summary>
    public static int DeployWatch(string local, string? asName, string watched, Func<bool>? rebuild, int seconds = 0)
    {
        var t = Load();
        if (t is null) return WithDevice(_ => 0);
        string? name = RemoteName(local, asName);
        if (name is null) return Fail($"cannot make an 8.3 name of {Path.GetFileName(local)} (use --as NAME.EXT)");
        string remote = "/disk/" + name;
        string ext = Path.GetExtension(name);
        DateTime Stamp() => Directory.Exists(watched)
            ? Directory.GetFiles(watched, "*", SearchOption.AllDirectories).Select(File.GetLastWriteTimeUtc).DefaultIfEmpty().Max()
            : File.GetLastWriteTimeUtc(watched);
        try
        {
            using var c = Client(t);
            uint cursor = c.LogAsync(uint.MaxValue).GetAwaiter().GetResult().Next;
            long pid = 0;
            void Start()
            {
                string line = ext switch
                {
                    ".BZP" => "bz install " + remote,
                    ".SH" => "sh " + remote,
                    _ when IsScript(name) => "run " + remote + " --watch",
                    _ => "run " + remote,
                };
                var (failed, output) = c.ExecAsync(line).GetAwaiter().GetResult();
                Console.Write(output);
                if (!failed && ext == ".BZP" && PackageName(local) is string pkg)
                    (failed, output) = c.ExecAsync(pkg).GetAwaiter().GetResult();
                if (ext == ".BZP") Console.Write(output);
                var m = System.Text.RegularExpressions.Regex.Match(output, @"\(pid (\d+)\)");
                pid = m.Success ? long.Parse(m.Groups[1].Value) : 0;
            }
            Upload(t, local, remote);
            Start();
            var last = Stamp();
            Console.WriteLine($"-- watching {watched}: every save is deployed again (Ctrl+C ends) --");
            var until = seconds > 0 ? DateTime.UtcNow.AddSeconds(seconds) : DateTime.MaxValue;
            while (DateTime.UtcNow < until)
            {
                var (next, text) = c.LogAsync(cursor).GetAwaiter().GetResult();
                Console.Write(text);
                cursor = next;
                Thread.Sleep(400);
                var now = Stamp();
                if (now == last) continue;
                last = now;
                Thread.Sleep(200); // let the editor finish writing
                if (rebuild != null && !rebuild()) { Console.WriteLine("-- rebuild failed; waiting for the next save --"); continue; }
                Upload(t, local, remote);
                if (IsScript(name) && pid != 0) { Console.WriteLine($"-- {name} reloads in its window (pid {pid}) --"); continue; }
                if (pid != 0) c.ExecAsync("kill " + pid).GetAwaiter().GetResult();
                Start();
            }
            return 0;
        }
        catch (BzdpTimeoutException e) { return Fail(e.Message); }
        catch (BzdpException e) { return Fail(e.Message); }
    }

    /// <summary>Upload a file to /disk and start it: a package is installed, a program or
    /// script is run. Then the device log is followed for `followSeconds`.</summary>
    public static int Deploy(string local, string? asName, bool run, int followSeconds)
    {
        var t = Load();
        if (t is null) return WithDevice(_ => 0);
        try
        {
            string name = (asName ?? Path.GetFileName(local)).ToUpperInvariant();
            string ext = Path.GetExtension(name);
            if (ext is ".BZPKG") name = Path.GetFileNameWithoutExtension(name) + ".BZP";
            if (!Is83(name))
            {
                // shorten: first 8 letters/digits of the stem + the extension's first 3
                string stem = new(Path.GetFileNameWithoutExtension(name).Where(char.IsAsciiLetterOrDigit).Take(8).ToArray());
                string e3 = new(Path.GetExtension(name).TrimStart('.').Take(3).ToArray());
                name = e3.Length > 0 ? $"{stem}.{e3}" : stem;
                if (!Is83(name)) return Fail($"cannot make an 8.3 name of {Path.GetFileName(local)} (use --as NAME.EXT)");
            }
            string remote = "/disk/" + name;
            using var c = Client(t);
            uint cursor = c.LogAsync(uint.MaxValue).GetAwaiter().GetResult().Next; // only what follows
            Upload(t, local, remote);
            if (!run) return 0;
            string ext2 = Path.GetExtension(name);
            string line = ext2 switch
            {
                ".BZP" => "bz install " + remote,
                ".SH" => "sh " + remote,
                _ => "run " + remote,
            };
            var (failed, output) = c.ExecAsync(line).GetAwaiter().GetResult();
            Console.Write(output);
            if (failed) return 1;
            // an installed package with an entry is started by its name
            if (ext2 == ".BZP" && PackageName(local) is string pkg)
            {
                var (f2, out2) = c.ExecAsync(pkg).GetAwaiter().GetResult();
                Console.Write(out2);
                if (f2) return 1;
            }
            if (followSeconds <= 0) return 0;
            Console.WriteLine($"-- device log for {followSeconds} s (bz logs -f to keep following) --");
            var until = DateTime.UtcNow.AddSeconds(followSeconds);
            while (DateTime.UtcNow < until)
            {
                var (next, text) = c.LogAsync(cursor).GetAwaiter().GetResult();
                Console.Write(text);
                if (next == cursor) Thread.Sleep(300);
                cursor = next;
            }
            return 0;
        }
        catch (BzdpTimeoutException e) { return Fail(e.Message); }
        catch (BzdpException e) { return Fail(e.Message); }
    }
}
