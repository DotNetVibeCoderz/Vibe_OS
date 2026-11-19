// bz debug --remote: a console debugger on the PC for a program on a running
// device (bz-link), without VS Code.
//
//   bz debug --remote FILE.js|ts|py|csx [--break 5,8] [--commands "n;v;c;q"]
//       uploads the script, records its run on the device (`script debug ... all`,
//       a UI script runs in its window until it is closed) and replays the
//       stops: n(ext) / p(rev) / c(ontinue to a breakpoint) / b N / v(ars) /
//       bt / o(utput) / l(ist) / q(uit).
//   bz debug --remote APP|/disk/X.ELF [--commands "..."]
//       a native program under the device's debugger (`dbg run`): every
//       command goes to `dbg` (break NAME, step, regs, bt, mem ADDR [N], ...);
//       `c` / `cont` runs until the next stop (polling `dbg wait`), q = kill.
//
// `--commands` takes the commands from a list instead of the keyboard (tests);
// `bz debug --selftest` drives both modes against the simulated device of
// `bz dap --selftest`.

using System.Text;

static class RemoteDebug
{
    public static int Run(string[] args, Func<IDebugDevice?> connect)
    {
        if (args.Length == 1 && args[0] == "--selftest") return SelfTest();
        if (args.Length < 2 || args[0] != "--remote")
        {
            Console.Error.WriteLine("usage: bz debug --remote FILE.js|ts|py|csx|APP [--break L1,L2] [--commands \"n;v;q\"]");
            return 1;
        }
        string target = args[1];
        var breaks = new HashSet<int>();
        Queue<string>? scripted = null;
        for (int i = 2; i < args.Length; i++)
        {
            if (args[i] == "--break" && i + 1 < args.Length)
                foreach (var b in args[++i].Split(',')) { if (int.TryParse(b, out int n)) breaks.Add(n); }
            else if (args[i] == "--commands" && i + 1 < args.Length)
                scripted = new Queue<string>(args[++i].Split(';').Select(s => s.Trim()));
            else { Console.Error.WriteLine("bz debug: unknown option " + args[i]); return 1; }
        }
        var dev = connect();
        if (dev is null) { Console.Error.WriteLine("bz: no device (bz device connect HOST CODE)"); return 1; }
        try
        {
            Func<string?> read = scripted != null
                ? () => { if (scripted.Count == 0) return null; var c = scripted.Dequeue(); Console.WriteLine("(bz-dbg) " + c); return c; }
                : () => { Console.Write("(bz-dbg) "); return Console.ReadLine(); };
            string ext = Path.GetExtension(target).ToLowerInvariant();
            return ext is ".js" or ".ts" or ".py" or ".csx" && File.Exists(target)
                ? Script(dev, target, breaks, read, Console.Out)
                : Native(dev, target, read, Console.Out);
        }
        finally { (dev as IDisposable)?.Dispose(); }
    }

    // ---- scripts: record, then replay ------------------------------------------------

    sealed record Stop(int Line, int OutLines, string Stack, List<(string, string)> Locals, List<(string, string)> Globals);

    static int Script(IDebugDevice dev, string local, HashSet<int> breaks, Func<string?> read, TextWriter o)
    {
        string text = File.ReadAllText(local);
        string[] src = text.Replace("\r", "").Split('\n');
        string lang = Path.GetExtension(local).ToLowerInvariant() switch { ".js" => "js", ".ts" => "ts", ".py" => "py", _ => "csx" };
        string remote = Session.RemoteName(local);
        dev.Upload(remote, Encoding.UTF8.GetBytes(text));
        o.WriteLine($"bz debug: {Path.GetFileName(local)} -> {remote}, merekam jalannya di perangkat ...");
        var (_, output) = dev.Exec($"script debug {lang} {remote} all");
        var ui = output.Split('\n').FirstOrDefault(l => l.StartsWith("@dbgui "))?.Trim().Split(' ');
        if (ui is { Length: >= 3 })
        {
            o.WriteLine($"bz debug: aplikasi UI berjalan di jendelanya (pid {ui[1]}); tutup jendelanya untuk memutar ulang");
            while ((output = dev.Exec($"script debug status {ui[1]} {ui[2]}").Output).StartsWith("@dbgui running")) Thread.Sleep(500);
        }
        var stops = new List<Stop>();
        var printed = new List<string>();
        string? error = null;
        foreach (var raw in output.Split('\n'))
        {
            string l = raw.TrimEnd('\r');
            if (l.StartsWith("@dbg "))
            {
                var f = l[5..].Split('\t');
                if (f.Length >= 5 && int.TryParse(f[0], out int line))
                {
                    int.TryParse(f[1], out int outLines);
                    stops.Add(new Stop(line, outLines, f[2], Vars(f[3]), Vars(f[4])));
                }
            }
            else if (l.StartsWith("  error: ")) error = l[9..];
            else if (l.StartsWith("  ")) printed.Add(l[2..]);
            else if (l.StartsWith("error") || l.StartsWith("script:")) error ??= l;
        }
        o.WriteLine($"bz debug: {stops.Count} henti direkam" + (error != null ? $", galat: {error}" : "") + " -- n p c b N v bt o l q");
        if (stops.Count == 0) return error == null ? 0 : 1;
        int at = 0;
        void Show()
        {
            var s = stops[at];
            string code = s.Line - 1 < src.Length && s.Line >= 1 ? src[s.Line - 1].Trim() : "";
            o.WriteLine($"[{at + 1}/{stops.Count}] baris {s.Line}{(breaks.Contains(s.Line) ? " (breakpoint)" : "")}: {code}");
        }
        at = 0;
        // start at the first breakpoint when there is one
        if (breaks.Count > 0) { int k = stops.FindIndex(s => breaks.Contains(s.Line)); if (k >= 0) at = k; }
        Show();
        while (read() is string cmd)
        {
            var w = cmd.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            switch (w.Length == 0 ? "n" : w[0])
            {
                case "n" or "next" or "s" or "step":
                    if (at + 1 < stops.Count) { at++; Show(); } else o.WriteLine("akhir rekaman" + (error != null ? " -- galat: " + error : ""));
                    break;
                case "p" or "prev" or "back":
                    if (at > 0) { at--; Show(); } else o.WriteLine("awal rekaman");
                    break;
                case "c" or "cont" or "continue":
                    {
                        int k = stops.FindIndex(at + 1, s => breaks.Contains(s.Line));
                        if (k < 0) { at = stops.Count - 1; Show(); o.WriteLine("tidak ada breakpoint lagi: akhir rekaman" + (error != null ? " -- galat: " + error : "")); }
                        else { at = k; Show(); }
                        break;
                    }
                case "b" or "break" when w.Length > 1 && int.TryParse(w[1], out int bl):
                    if (!breaks.Remove(bl)) breaks.Add(bl);
                    o.WriteLine($"breakpoint baris {bl} {(breaks.Contains(bl) ? "dipasang" : "dilepas")}" + (stops.Any(s => s.Line == bl) ? "" : " (baris ini tidak pernah dieksekusi)"));
                    break;
                case "v" or "vars":
                    {
                        var s = stops[at];
                        foreach (var (n, v) in s.Locals) o.WriteLine($"  lokal  {n} = {v}");
                        foreach (var (n, v) in s.Globals) o.WriteLine($"  global {n} = {v}");
                        if (s.Locals.Count + s.Globals.Count == 0) o.WriteLine("  (belum ada variabel)");
                        break;
                    }
                case "bt" or "where":
                    foreach (var fr in stops[at].Stack.Split(',', StringSplitOptions.RemoveEmptyEntries)) o.WriteLine("  " + fr);
                    break;
                case "o" or "out":
                    foreach (var l in printed.Take(stops[at].OutLines)) o.WriteLine("  > " + l);
                    break;
                case "l" or "list":
                    {
                        int line = stops[at].Line;
                        for (int k = Math.Max(1, line - 3); k <= Math.Min(src.Length, line + 3); k++)
                            o.WriteLine($"{(k == line ? "->" : "  ")}{k,4}  {src[k - 1]}");
                        break;
                    }
                case "q" or "quit" or "exit":
                    return 0;
                default:
                    o.WriteLine("perintah: n(ext) p(rev) c(ontinue) b BARIS v(ars) bt o(utput) l(ist) q(uit)");
                    break;
            }
        }
        return 0;
    }

    static List<(string, string)> Vars(string text)
    {
        var r = new List<(string, string)>();
        foreach (var kv in text.Split('\u001f', StringSplitOptions.RemoveEmptyEntries))
        {
            int eq = kv.IndexOf('=');
            if (eq > 0) r.Add((kv[..eq], kv[(eq + 1)..]));
        }
        return r;
    }

    // ---- native programs: the device's dbg ------------------------------------------

    static int Native(IDebugDevice dev, string app, Func<string?> read, TextWriter o)
    {
        var (failed, first) = dev.Exec("dbg run " + app);
        o.Write(first);
        if (failed) return 1;
        o.WriteLine("bz debug: perintah dikirim ke 'dbg' di perangkat (break NAMA, step, regs, bt, mem ALAMAT [N], cont, kill); q = selesai");
        while (read() is string cmd)
        {
            string c = cmd.Trim();
            if (c is "q" or "quit" or "exit") { o.Write(dev.Exec("dbg kill").Output); return 0; }
            if (c is "c" or "cont" or "continue")
            {
                var (_, outp) = dev.Exec("dbg cont");
                // the program runs on: wait for its next stop (or its end)
                for (int i = 0; i < 300 && outp.Contains("is running"); i++) outp = dev.Exec("dbg wait 2000").Output;
                o.Write(outp);
                if (outp.Contains("exited")) return 0;
                continue;
            }
            o.Write(dev.Exec("dbg " + (c.Length == 0 ? "step" : c)).Output);
        }
        return 0;
    }

    // ---- self-test against the simulated device --------------------------------------

    static int SelfTest()
    {
        int checks = 0, failed = 0;
        void That(bool ok, string what) { checks++; if (!ok) { failed++; Console.WriteLine("bz debug: FAIL " + what); } }

        var file = Path.Combine(Path.GetTempPath(), "bzdbgtest.js");
        File.WriteAllText(file, DapCli.TestScript);
        var dev = new DapCli.FakeDevice();
        Queue<string> Q(string s) => new(s.Split(';'));
        string RunScript(string cmds, HashSet<int> breaks)
        {
            var q = Q(cmds);
            var sw = new StringWriter();
            Script(dev, file, breaks, () => q.Count > 0 ? q.Dequeue() : null, sw);
            return sw.ToString();
        }
        string a = RunScript("n;n;v;l;q", new HashSet<int>());
        That(a.Contains("11 henti direkam") && a.Contains("[1/11] baris 5") && a.Contains("[3/11] baris 7"), "script: recorded stops, n moves forward");
        That(a.Contains("global total = 0") && a.Contains("->   7    total = total + sq(i);"), "script: vars and the listing at the stop");
        string b = RunScript("bt;c;v;c;p;o;q", new HashSet<int> { 3 });
        That(b.Contains("[5/11] baris 3 (breakpoint)") && b.Contains("sq:3"), "script: starts at the first breakpoint, bt shows the call");
        That(b.Contains("[9/11] baris 3 (breakpoint)") && b.Contains("lokal  r = 4"), "script: c runs to the next breakpoint, its locals");
        That(b.Contains("tidak ada breakpoint lagi") && b.Contains("[10/11] baris 8") && b.Contains("  > i=1"), "script: c past the last breakpoint ends, p goes back, o shows the output");

        var sw2 = new StringWriter();
        var q2 = Q("break Crash__Level2;c;bt;regs;q");
        Native(dev, "crashme", () => q2.Count > 0 ? q2.Dequeue() : null, sw2);
        string n = sw2.ToString();
        That(n.Contains("stopped (entry)") && n.Contains("breakpoint 1 at 0x401230"), "native: dbg run + break");
        That(n.Contains("stopped (breakpoint) at 0x401230") && n.Contains("Crash__Level1+0x14") && n.Contains("rip 0x0000000000401230") && n.Contains("killed"),
             "native: cont waits for the stop, bt, regs, q kills");
        File.Delete(file);
        Console.WriteLine($"bz debug: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }
}
