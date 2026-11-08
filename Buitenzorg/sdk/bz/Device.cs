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
    static BzdpClient Client(Target t) => new(t.Host, t.Port, t.Code) { ReplyTimeout = TimeSpan.FromSeconds(2), Attempts = 8 };

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
                ["kill", var pid] when uint.TryParse(pid, out var p) => WithDevice(t => Kill(t, p)),
                ["prof", .. var rest] when rest.Length > 0 => WithDevice(t => Prof(t, rest)),
                ["gdb-check", .. var rest] => WithDevice(t => GdbCheck.Check(new BzdpDebugDevice(Client(t)), rest)),
                ["build-server", "--check", .. var rest] => WithDevice(t => BuildServer.Check(new BzdpDebugDevice(Client(t)), rest)),
                ["repo", "--check", .. var rest] => WithDevice(t => RepoCli.Check(new BzdpDebugDevice(Client(t)), rest)),
                ["dap", .. var rest] => DapCli.Run(rest, () => Load() is Target t ? new BzdpDebugDevice(Client(t)) : null),
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
