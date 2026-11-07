// `bz dap` entry points and the adapter's tests.
//
//   bz dap               DAP on stdin/stdout (what VS Code starts)
//   bz dap --selftest    a full DAP session against a simulated device
//   bz dap --check [--native APP FUNC CALLER]
//                        the same session against the connected device
//                        (bz device connect / BZ_DEVICE + BZ_CODE)

using System.IO.Pipes;
using System.Text.Json.Nodes;
using MagicAppGen.Services;

static class DapCli
{
    // The test program. Its stops are the interpreter's statement starts; the
    // simulated device below answers with what the kernel records for it.
    public const string TestScript =
        "function sq(n) {\n" +           // 1
        "  let r = n * n;\n" +           // 2
        "  return r;\n" +                // 3
        "}\n" +                          // 4
        "let total = 0;\n" +             // 5
        "for (let i = 1; i <= 2; i = i + 1) {\n" + // 6
        "  total = total + sq(i);\n" +   // 7
        "  print(\"i=\" + i);\n" +       // 8
        "}\n" +                          // 9
        "print(\"total=\" + total);\n";  // 10

    public static int Run(string[] rest, Func<IDebugDevice?> connect)
    {
        switch (rest)
        {
            case []:
            {
                var server = new DapServer(Console.OpenStandardInput(), Console.OpenStandardOutput(), a => Connect(a, connect));
                server.Run();
                return 0;
            }
            case ["--selftest"]:
                return Check(() => new FakeDevice(), native: ("crashme", "Crash__Level2", "Crash__Level1"), label: "simulated device");
            case ["--check"]:
                return Check(() => connect() ?? throw new InvalidOperationException("no device (bz device connect)"), native: null, label: "device");
            case ["--check", "--native", var app, var func, var caller]:
                return Check(() => connect() ?? throw new InvalidOperationException("no device (bz device connect)"), native: (app, func, caller), label: "device");
            default:
                Console.Error.WriteLine("usage: bz dap [--selftest | --check [--native APP FUNC CALLER]]");
                return 1;
        }
    }

    /// <summary>The launch configuration may name a device ("device": "host:port", "code": 1234).</summary>
    static IDebugDevice Connect(JsonObject a, Func<IDebugDevice?> fallback)
    {
        string? dev = a["device"]?.GetValue<string>();
        if (!string.IsNullOrWhiteSpace(dev))
        {
            int colon = dev.LastIndexOf(':');
            string host = dev;
            int port = BzdpClient.DefaultPort;
            if (colon > 0 && int.TryParse(dev[(colon + 1)..], out var p)) { host = dev[..colon]; port = p; }
            int code = a["code"]?.GetValue<int>() ?? 0;
            return new BzdpDebugDevice(new BzdpClient(host, port, code) { ReplyTimeout = TimeSpan.FromSeconds(2), Attempts = 8 });
        }
        return fallback() ?? throw new InvalidOperationException(
            "tidak ada perangkat: 'bz device connect <host[:port]> <kode>', atau isi \"device\" dan \"code\" di launch.json");
    }

    // ---- a DAP client over in-process pipes ---------------------------------------

    sealed class Client : IDisposable
    {
        readonly AnonymousPipeServerStream _toServer = new(PipeDirection.Out);
        readonly AnonymousPipeServerStream _fromServer = new(PipeDirection.In);
        readonly Thread _thread;
        readonly List<JsonObject> _inbox = new();
        readonly Thread _reader;
        int _seq = 1;
        public readonly List<string> Output = new();

        public Client(Func<JsonObject, IDebugDevice> connect)
        {
            var sin = new AnonymousPipeClientStream(PipeDirection.In, _toServer.ClientSafePipeHandle);
            var sout = new AnonymousPipeClientStream(PipeDirection.Out, _fromServer.ClientSafePipeHandle);
            var server = new DapServer(sin, sout, connect);
            _thread = new Thread(() => { server.Run(); sout.Dispose(); }) { IsBackground = true };
            _thread.Start();
            _reader = new Thread(() =>
            {
                while (DapServer.ReadMessage(_fromServer) is JsonObject m)
                    lock (_inbox)
                    {
                        if (m["event"]?.GetValue<string>() == "output")
                            Output.Add(m["body"]!["output"]!.GetValue<string>().TrimEnd('\n'));
                        _inbox.Add(m);
                        Monitor.PulseAll(_inbox);
                    }
            }) { IsBackground = true };
            _reader.Start();
        }

        public JsonObject Request(string command, JsonObject? args = null)
        {
            int seq = _seq++;
            DapServer.WriteMessage(_toServer, new JsonObject { ["seq"] = seq, ["type"] = "request", ["command"] = command, ["arguments"] = args ?? new JsonObject() });
            var r = Wait(m => m["type"]?.GetValue<string>() == "response" && m["request_seq"]?.GetValue<int>() == seq, command);
            if (!r["success"]!.GetValue<bool>()) throw new InvalidOperationException($"{command}: {r["message"]}");
            return r["body"] as JsonObject ?? new JsonObject();
        }

        public JsonObject Event(string name) => Wait(m => m["event"]?.GetValue<string>() == name, name)["body"] as JsonObject ?? new JsonObject();

        /// <summary>The next matching message (consumed), waiting up to 60 s.</summary>
        JsonObject Wait(Func<JsonObject, bool> match, string what)
        {
            var until = DateTime.UtcNow.AddSeconds(60);
            lock (_inbox)
            {
                while (true)
                {
                    int i = _inbox.FindIndex(m => match(m));
                    if (i >= 0) { var m = _inbox[i]; _inbox.RemoveAt(i); return m; }
                    var left = until - DateTime.UtcNow;
                    if (left <= TimeSpan.Zero) throw new TimeoutException("no '" + what + "' from the adapter");
                    Monitor.Wait(_inbox, left);
                }
            }
        }

        public void Dispose() { _toServer.Dispose(); _thread.Join(5000); }
    }

    // ---- the session the tests drive ------------------------------------------------

    static int Check(Func<IDebugDevice> device, (string App, string Func, string Caller)? native, string label)
    {
        int checks = 0, failed = 0;
        void Expect(bool ok, string what)
        {
            checks++;
            if (!ok) failed++;
            Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what);
        }

        string dir = Path.Combine(Path.GetTempPath(), "bzdap-" + Environment.ProcessId);
        Directory.CreateDirectory(dir);
        string file = Path.Combine(dir, "DAPTEST.JS");
        File.WriteAllText(file, TestScript);
        try
        {
            Console.WriteLine($"bz dap: script session ({label})");
            using (var c = new Client(_ => device()))
            {
                var caps = c.Request("initialize", new JsonObject { ["adapterID"] = "buitenzorg" });
                Expect(caps["supportsStepBack"]?.GetValue<bool>() == true, "initialize: step back supported");
                c.Event("initialized");
                c.Request("launch", new JsonObject { ["program"] = file });
                var bps = c.Request("setBreakpoints", new JsonObject
                {
                    ["source"] = new JsonObject { ["path"] = file },
                    ["breakpoints"] = new JsonArray { new JsonObject { ["line"] = 3 }, new JsonObject { ["line"] = 4 } },
                });
                var verified = (bps["breakpoints"] as JsonArray)!.Select(b => b!["verified"]!.GetValue<bool>()).ToArray();
                Expect(verified is [true, false], "setBreakpoints: line 3 verified, line 4 (a brace) not");
                c.Request("configurationDone");

                (int Line, string Fn, int Frames) Where()
                {
                    var st = c.Request("stackTrace", new JsonObject { ["threadId"] = 1 });
                    var f = (st["stackFrames"] as JsonArray)!;
                    return (f[0]!["line"]!.GetValue<int>(), f[0]!["name"]!.GetValue<string>(), f.Count);
                }
                Dictionary<string, string> Vars(string scope)
                {
                    var sc = (c.Request("scopes", new JsonObject { ["frameId"] = 0 })["scopes"] as JsonArray)!;
                    var s = sc.FirstOrDefault(x => x!["name"]!.GetValue<string>() == scope);
                    if (s == null) return new();
                    var vs = (c.Request("variables", new JsonObject { ["variablesReference"] = s["variablesReference"]!.GetValue<int>() })["variables"] as JsonArray)!;
                    return vs.ToDictionary(v => v!["name"]!.GetValue<string>(), v => v!["value"]!.GetValue<string>());
                }

                var ev = c.Event("stopped");
                var w = Where();
                var loc = Vars("Lokal");
                Expect(ev["reason"]?.GetValue<string>() == "breakpoint" && w.Line == 3 && w.Fn == "sq" && w.Frames == 2,
                    $"stopped at the breakpoint: line {w.Line} in {w.Fn}, {w.Frames} frames");
                Expect(loc.GetValueOrDefault("n") == "1" && loc.GetValueOrDefault("r") == "1", $"locals n=1 r=1 ({string.Join(" ", loc.Select(kv => kv.Key + "=" + kv.Value))})");

                c.Request("continue", new JsonObject { ["threadId"] = 1 });
                c.Event("stopped");
                w = Where();
                loc = Vars("Lokal");
                Expect(w.Line == 3 && loc.GetValueOrDefault("n") == "2" && loc.GetValueOrDefault("r") == "4", $"continue: line 3 again with n=2 r=4");
                Expect(c.Output.Contains("i=1"), "output so far: i=1");

                c.Request("stepBack", new JsonObject { ["threadId"] = 1 });
                c.Event("stopped");
                w = Where();
                Expect(w.Line == 2 && w.Fn == "sq", $"step back: line {w.Line} in {w.Fn}");

                c.Request("stepOut", new JsonObject { ["threadId"] = 1 });
                c.Event("stopped");
                w = Where();
                var glob = Vars("Global");
                Expect(w.Line == 8 && w.Frames == 1 && glob.GetValueOrDefault("total") == "5", $"step out: line {w.Line}, total={glob.GetValueOrDefault("total")}");

                var hover = c.Request("evaluate", new JsonObject { ["expression"] = "total", ["context"] = "hover", ["frameId"] = 0 });
                Expect(hover["result"]?.GetValue<string>() == "5", "evaluate total = 5");

                c.Request("next", new JsonObject { ["threadId"] = 1 });
                c.Event("stopped");
                w = Where();
                Expect(w.Line == 10, $"next: line {w.Line} (after the loop)");

                c.Request("continue", new JsonObject { ["threadId"] = 1 });
                var ex = c.Event("exited");
                c.Event("terminated");
                Expect(ex["exitCode"]?.GetValue<int>() == 0 && c.Output.Contains("total=5"), "continue: the program ends (exit 0, printed total=5)");
                c.Request("disconnect");
            }

            if (native is var (app, func, caller))
            {
                Console.WriteLine($"bz dap: native session, {app} ({label})");
                var probe = device();
                bool present = probe.Exec("ls /disk").Output.ToUpperInvariant().Contains(app.ToUpperInvariant() + ".ELF");
                (probe as IDisposable)?.Dispose();
                if (!present)
                    Console.WriteLine($"  skipped: /disk/{app.ToUpperInvariant()}.ELF is not on the device");
                else
                {
                    using var c = new Client(_ => device());
                    c.Request("initialize", new JsonObject { ["adapterID"] = "buitenzorg" });
                    c.Event("initialized");
                    c.Request("launch", new JsonObject { ["program"] = app });
                    var fb = c.Request("setFunctionBreakpoints", new JsonObject { ["breakpoints"] = new JsonArray { new JsonObject { ["name"] = func } } });
                    Expect((fb["breakpoints"] as JsonArray)![0]!["verified"]!.GetValue<bool>(), $"function breakpoint {func} verified");
                    c.Request("configurationDone");
                    var ev = c.Event("stopped");
                    var st = (c.Request("stackTrace", new JsonObject { ["threadId"] = 1 })["stackFrames"] as JsonArray)!;
                    string names = string.Join(" | ", st.Select(f => f!["name"]!.GetValue<string>()));
                    Expect(ev["reason"]?.GetValue<string>() == "function breakpoint" && st.Count > 1 && st[0]!["name"]!.GetValue<string>().Contains(func) && names.Contains(caller),
                        $"stopped in {func}, called from {caller} ({names})");
                    var regs = (c.Request("variables", new JsonObject { ["variablesReference"] = 3 })["variables"] as JsonArray)!;
                    Expect(regs.Any(r => r!["name"]!.GetValue<string>() is "rsp" or "sp"), $"registers ({regs.Count})");
                    c.Request("next", new JsonObject { ["threadId"] = 1 });
                    ev = c.Event("stopped");
                    Expect(ev["reason"]?.GetValue<string>() == "step", "next: one instruction");
                    var mem = c.Request("evaluate", new JsonObject { ["expression"] = "bt", ["context"] = "repl" });
                    Expect(mem["result"]?.GetValue<string>().Contains(caller) == true, "debug console: 'bt' goes to dbg");
                    c.Request("disconnect");
                }
            }
        }
        catch (Exception e)
        {
            failed++;
            Console.WriteLine("  FAIL " + e.Message);
        }
        finally
        {
            try { Directory.Delete(dir, true); } catch { }
        }
        Console.WriteLine($"bz dap: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }

    // ---- a simulated device -----------------------------------------------------------

    /// <summary>Answers like the kernel does for <see cref="TestScript"/> and for
    /// `dbg` on crashme (kernel/bzkernel/src/shell.rs debug_lines, debug.rs).</summary>
    sealed class FakeDevice : IDebugDevice
    {
        readonly Dictionary<string, byte[]> _files = new();
        bool _running;

        public void Upload(string remotePath, byte[] data) => _files[remotePath] = data;

        public (bool, string) Exec(string line)
        {
            if (line.StartsWith("script debug js "))
            {
                string path = line.Split(' ')[3];
                if (!_files.ContainsKey(path)) return (true, "error: no such file\n");
                const char U = '\u001f';
                string[] recs =
                {
                    "5\t0\t<main>:5\t\t",
                    $"6\t0\t<main>:6\t\ttotal=0",
                    $"7\t0\t<main>:7\t\ttotal=0",
                    $"2\t0\tsq:2,<main>:7\tn=1\ttotal=0",
                    $"3\t0\tsq:3,<main>:7\tn=1{U}r=1\ttotal=0",
                    $"8\t0\t<main>:8\t\ttotal=1",
                    $"7\t1\t<main>:7\t\ttotal=1",
                    $"2\t1\tsq:2,<main>:7\tn=2\ttotal=1",
                    $"3\t1\tsq:3,<main>:7\tn=2{U}r=4\ttotal=1",
                    $"8\t1\t<main>:8\t\ttotal=5",
                    $"10\t2\t<main>:10\t\ttotal=5",
                };
                return (false, "[js] (60 langkah, 11 henti)\n  i=1\n  i=2\n  total=5\n" + string.Concat(recs.Select(r => "@dbg " + r + "\n")));
            }
            return line switch
            {
                "ls /disk" => (false, "CRASHME.ELF\n"),
                "dbg run crashme" => (false, "pid 7 stopped (entry) at 0x401000  _start\n"),
                "dbg break Crash__Level2" => (false, "breakpoint 1 at 0x401230  Crash__Level2\n"),
                "dbg cont" => Cont(),
                "dbg wait 2000" when _running => (false, "pid 7 stopped (breakpoint) at 0x401230  Crash__Level2\n"),
                "dbg bt" => (false, "  #0  0x401230  Crash__Level2\n  #1  0x401290  Crash__Level1+0x14\n  #2  0x4012f0  Crash__Main+0x9\n"),
                "dbg regs" => (false, "   rax 0x0000000000000001     rbx 0x0000000000000000  \n   rsp 0x000000007000ffe0     rip 0x0000000000401230  \n"),
                "dbg step" => Step(),
                "dbg kill" => (false, "pid 7 killed\n"),
                _ => (true, "dbg: unexpected request '" + line + "'\n"),
            };
        }

        // the first stop comes later than one request: the adapter polls with `dbg wait`
        (bool, string) Cont()
        {
            _running = true;
            return (false, "pid 7 is running (not stopped within 3000 ms; 'dbg' shows its state)\n");
        }

        static (bool, string) Step() => (false, "pid 7 stopped (step) at 0x401234  Crash__Level2+0x4\n");
    }
}
