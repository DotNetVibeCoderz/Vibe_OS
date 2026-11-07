// `bz dap` -- a Debug Adapter Protocol server (VS Code and any other DAP
// client) for programs running on a Buitenzorg device, over bz-link (BZDP v1).
//
// Two kinds of program, picked by the launch configuration's `program`:
//
//  * Scripts (.js .ts .py .csx): the file is uploaded to the device and run
//    once under the kernel's recording debugger (`script debug LANG FILE all`,
//    kernel/bzkernel/src/shell.rs). Every statement start comes back as an
//    `@dbg` record (line, call stack, locals, globals, output so far), and the
//    adapter replays them: continue = next stop on a breakpoint line, next =
//    next stop at the same or a shallower call depth, step in = next stop,
//    step out = next stop at a shallower depth, and -- because it is a
//    recording -- step back and reverse continue too. At most 400 stops are
//    recorded per run (the kernel's limit).
//
//  * Native ELF programs (an app name such as `crashme`, or a local .ELF that
//    is uploaded first): the live debugger (`dbg run/break/cont/step/bt/regs`,
//    kernel/bzkernel/src/debug.rs). Breakpoints are function breakpoints
//    (symbol names, or a unique part of one; there is no line information);
//    step = one instruction, step out = run to the caller's return address,
//    the Registers scope can be edited, and the Debug Console passes its text
//    to `dbg` (e.g. `mem 0x400000 32`).
//
// The adapter speaks DAP on stdin/stdout. `bz dap --selftest` drives it with a
// simulated device; `bz dap --check` drives it against the connected device.

using System.Globalization;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using MagicAppGen.Services;

/// <summary>What the adapter needs from a device: run a shell line, write a file.</summary>
interface IDebugDevice
{
    (bool Failed, string Output) Exec(string line);
    void Upload(string remotePath, byte[] data);
}

sealed class BzdpDebugDevice : IDebugDevice, IDisposable
{
    readonly BzdpClient _c;
    readonly object _lock = new();
    public BzdpDebugDevice(BzdpClient c) { _c = c; _c.HelloAsync().GetAwaiter().GetResult(); }
    public (bool Failed, string Output) Exec(string line) { lock (_lock) return _c.ExecAsync(line).GetAwaiter().GetResult(); }
    public void Upload(string remotePath, byte[] data) { lock (_lock) _c.UploadAsync(remotePath, data).GetAwaiter().GetResult(); }
    public void Dispose() => _c.Dispose();
}

sealed class DapServer
{
    readonly Stream _in;
    readonly Stream _out;
    readonly Func<JsonObject, IDebugDevice> _connect;
    readonly object _write = new();
    int _seq = 1;

    // breakpoints as the client set them: source path -> lines; function names
    readonly Dictionary<string, int[]> _lineBps = new(StringComparer.OrdinalIgnoreCase);
    string[] _funcBps = Array.Empty<string>();

    Session? _session;
    bool _configured;
    bool _started;
    volatile bool _done;

    public DapServer(Stream input, Stream output, Func<JsonObject, IDebugDevice> connect)
    {
        _in = input;
        _out = output;
        _connect = connect;
    }

    // ---- framing ------------------------------------------------------------

    public static JsonObject? ReadMessage(Stream s)
    {
        int length = -1;
        var header = new StringBuilder();
        while (true)
        {
            int b = s.ReadByte();
            if (b < 0) return null;
            if (b == '\n')
            {
                string line = header.ToString().TrimEnd('\r');
                header.Clear();
                if (line.Length == 0)
                {
                    if (length < 0) continue;
                    break;
                }
                if (line.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase))
                    length = int.Parse(line["Content-Length:".Length..].Trim(), CultureInfo.InvariantCulture);
                continue;
            }
            header.Append((char)b);
        }
        var body = new byte[length];
        int got = 0;
        while (got < length)
        {
            int n = s.Read(body, got, length - got);
            if (n <= 0) return null;
            got += n;
        }
        return JsonNode.Parse(body) as JsonObject;
    }

    public static void WriteMessage(Stream s, JsonObject msg)
    {
        var body = Encoding.UTF8.GetBytes(msg.ToJsonString());
        var head = Encoding.ASCII.GetBytes($"Content-Length: {body.Length}\r\n\r\n");
        s.Write(head);
        s.Write(body);
        s.Flush();
    }

    void Send(JsonObject msg)
    {
        lock (_write)
        {
            msg["seq"] = _seq++;
            WriteMessage(_out, msg);
        }
    }

    internal void Event(string name, JsonObject? body = null) =>
        Send(new JsonObject { ["type"] = "event", ["event"] = name, ["body"] = body ?? new JsonObject() });

    void Respond(JsonObject req, JsonObject? body = null, string? error = null)
    {
        var r = new JsonObject
        {
            ["type"] = "response",
            ["request_seq"] = req["seq"]?.GetValue<int>() ?? 0,
            ["command"] = req["command"]?.GetValue<string>() ?? "",
            ["success"] = error == null,
        };
        if (error != null)
        {
            r["message"] = error;
            r["body"] = new JsonObject { ["error"] = new JsonObject { ["id"] = 1, ["format"] = error, ["showUser"] = true } };
        }
        else r["body"] = body ?? new JsonObject();
        Send(r);
    }

    internal void Output(string text, string category = "stdout") =>
        Event("output", new JsonObject { ["category"] = category, ["output"] = text.EndsWith('\n') ? text : text + "\n" });

    internal void Stopped(string reason, string? text = null)
    {
        var b = new JsonObject { ["reason"] = reason, ["threadId"] = 1, ["allThreadsStopped"] = true };
        if (text != null) b["text"] = text;
        Event("stopped", b);
    }

    internal void Finished(int exitCode)
    {
        if (_done) return;
        _done = true;
        Event("exited", new JsonObject { ["exitCode"] = exitCode });
        Event("terminated");
    }

    // ---- the request loop -----------------------------------------------------

    public void Run()
    {
        while (true)
        {
            var msg = ReadMessage(_in);
            if (msg == null) break;
            if (msg["type"]?.GetValue<string>() != "request") continue;
            string cmd = msg["command"]?.GetValue<string>() ?? "";
            var args = msg["arguments"] as JsonObject ?? new JsonObject();
            try
            {
                if (!Handle(cmd, msg, args)) break;
            }
            catch (Exception e) when (e is not OutOfMemoryException)
            {
                Respond(msg, error: e is BzdpTimeoutException or BzdpException
                    ? "perangkat tidak menjawab: " + e.Message
                    : e.Message);
            }
        }
        _session?.Dispose();
    }

    /// <summary>One request; false = the session is over.</summary>
    bool Handle(string cmd, JsonObject req, JsonObject a)
    {
        switch (cmd)
        {
            case "initialize":
                Respond(req, new JsonObject
                {
                    ["supportsConfigurationDoneRequest"] = true,
                    ["supportsFunctionBreakpoints"] = true,
                    ["supportsStepBack"] = true,
                    ["supportsSetVariable"] = true,
                    ["supportsEvaluateForHovers"] = true,
                    ["supportsTerminateRequest"] = true,
                    ["supportsRestartRequest"] = false,
                });
                Event("initialized");
                return true;

            case "launch":
                _session = Session.Create(this, a, _connect);
                Respond(req);
                MaybeStart();
                return true;

            case "attach":
                Respond(req, error: "bz dap: hanya 'launch' (program dijalankan di perangkat oleh adapter)");
                return true;

            case "setBreakpoints":
            {
                string path = Norm(a["source"]?["path"]?.GetValue<string>() ?? "");
                var lines = (a["breakpoints"] as JsonArray ?? new JsonArray())
                    .Select(b => b?["line"]?.GetValue<int>() ?? 0).Where(l => l > 0).ToArray();
                _lineBps[path] = lines;
                var res = new JsonArray();
                foreach (int l in lines)
                {
                    var (ok, msg) = _session?.LineBreakpoint(path, l) ?? (true, null);
                    var bp = new JsonObject { ["verified"] = ok, ["line"] = l };
                    if (msg != null) bp["message"] = msg;
                    res.Add(bp);
                }
                Respond(req, new JsonObject { ["breakpoints"] = res });
                return true;
            }

            case "setFunctionBreakpoints":
            {
                _funcBps = (a["breakpoints"] as JsonArray ?? new JsonArray())
                    .Select(b => b?["name"]?.GetValue<string>() ?? "").Where(n => n.Length > 0).ToArray();
                var res = new JsonArray();
                foreach (var (ok, msg) in _session?.FunctionBreakpoints(_funcBps, _started) ?? _funcBps.Select(_ => (true, (string?)null)))
                {
                    var bp = new JsonObject { ["verified"] = ok };
                    if (msg != null) bp["message"] = msg;
                    res.Add(bp);
                }
                Respond(req, new JsonObject { ["breakpoints"] = res });
                return true;
            }

            case "setExceptionBreakpoints":
                Respond(req, new JsonObject { ["breakpoints"] = new JsonArray() });
                return true;

            case "configurationDone":
                _configured = true;
                Respond(req);
                MaybeStart();
                return true;

            case "threads":
                Respond(req, new JsonObject
                {
                    ["threads"] = new JsonArray { new JsonObject { ["id"] = 1, ["name"] = _session?.Name ?? "program" } },
                });
                return true;

            case "stackTrace":
            {
                var frames = Need().StackTrace();
                Respond(req, new JsonObject { ["stackFrames"] = frames, ["totalFrames"] = frames.Count });
                return true;
            }

            case "scopes":
                Respond(req, new JsonObject { ["scopes"] = Need().Scopes(a["frameId"]?.GetValue<int>() ?? 0) });
                return true;

            case "variables":
                Respond(req, new JsonObject { ["variables"] = Need().Variables(a["variablesReference"]?.GetValue<int>() ?? 0) });
                return true;

            case "setVariable":
            {
                var v = Need().SetVariable(a["variablesReference"]?.GetValue<int>() ?? 0,
                    a["name"]?.GetValue<string>() ?? "", a["value"]?.GetValue<string>() ?? "");
                Respond(req, new JsonObject { ["value"] = v });
                return true;
            }

            case "evaluate":
            {
                var (text, err) = Need().Evaluate(a["expression"]?.GetValue<string>() ?? "", a["context"]?.GetValue<string>() ?? "repl");
                if (err != null) Respond(req, error: err);
                else Respond(req, new JsonObject { ["result"] = text, ["variablesReference"] = 0 });
                return true;
            }

            case "continue":
                Respond(req, new JsonObject { ["allThreadsContinued"] = true });
                Need().Resume(Motion.Continue);
                return true;
            case "next":
                Respond(req);
                Need().Resume(Motion.Over);
                return true;
            case "stepIn":
                Respond(req);
                Need().Resume(Motion.In);
                return true;
            case "stepOut":
                Respond(req);
                Need().Resume(Motion.Out);
                return true;
            case "stepBack":
                Respond(req);
                Need().Resume(Motion.Back);
                return true;
            case "reverseContinue":
                Respond(req);
                Need().Resume(Motion.ReverseContinue);
                return true;

            case "pause":
                Respond(req, error: "jeda tidak didukung: program berhenti di breakpoint");
                return true;

            case "terminate":
                _session?.Terminate();
                Respond(req);
                Finished(_session?.ExitCode ?? 0);
                return true;

            case "disconnect":
                _session?.Terminate();
                Respond(req);
                return false;

            default:
                Respond(req, error: $"bz dap: perintah '{cmd}' tidak didukung");
                return true;
        }
    }

    Session Need() => _session ?? throw new InvalidOperationException("belum ada program (launch)");

    void MaybeStart()
    {
        if (_started || !_configured || _session == null) return;
        _started = true;
        _session.Start(_lineBps.TryGetValue(Norm(_session.LocalPath ?? ""), out var l) ? l : Array.Empty<int>(), _funcBps);
    }

    internal int[] LinesFor(string path) => _lineBps.TryGetValue(Norm(path), out var l) ? l : Array.Empty<int>();

    internal static string Norm(string p)
    {
        if (p.Length == 0) return p;
        try { return Path.GetFullPath(p); } catch { return p; }
    }
}

enum Motion { Continue, Over, In, Out, Back, ReverseContinue }

abstract class Session : IDisposable
{
    protected readonly DapServer Dap;
    protected readonly IDebugDevice Dev;
    public string Name { get; protected init; } = "program";
    public string? LocalPath { get; protected init; }
    public int ExitCode { get; protected set; }

    protected Session(DapServer dap, IDebugDevice dev) { Dap = dap; Dev = dev; }

    public static Session Create(DapServer dap, JsonObject a, Func<JsonObject, IDebugDevice> connect)
    {
        string program = a["program"]?.GetValue<string>() ?? throw new InvalidOperationException("launch: 'program' wajib diisi");
        string ext = Path.GetExtension(program).ToLowerInvariant();
        string? mode = a["mode"]?.GetValue<string>();
        bool script = mode == "script" || (mode != "native" && ext is ".js" or ".ts" or ".py" or ".csx");
        var dev = connect(a);
        return script ? new ScriptSession(dap, dev, a, program) : new NativeSession(dap, dev, a, program);
    }

    public abstract void Start(int[] lines, string[] functions);
    public virtual (bool, string?) LineBreakpoint(string path, int line) => (true, null);
    public virtual IEnumerable<(bool, string?)> FunctionBreakpoints(string[] names, bool live) =>
        names.Select(_ => (false, (string?)"breakpoint fungsi hanya untuk program native"));
    public abstract JsonArray StackTrace();
    public abstract JsonArray Scopes(int frame);
    public abstract JsonArray Variables(int reference);
    public virtual string SetVariable(int reference, string name, string value) =>
        throw new InvalidOperationException("nilai tidak bisa diubah: ini rekaman eksekusi");
    public abstract (string Text, string? Error) Evaluate(string expr, string context);
    public abstract void Resume(Motion how);
    public abstract void Terminate();
    public virtual void Dispose() => (Dev as IDisposable)?.Dispose();

    /// <summary>A device path for a local file: /disk + its 8.3 name.</summary>
    public static string RemoteName(string local)
    {
        string name = Path.GetFileName(local).ToUpperInvariant();
        if (Device.Is83(name)) return "/disk/" + name;
        string stem = new string(Path.GetFileNameWithoutExtension(name).Where(char.IsAsciiLetterOrDigit).Take(8).ToArray());
        string ext = Path.GetExtension(name).TrimStart('.');
        if (stem.Length == 0) stem = "PROGRAM";
        if (ext.Length > 3) throw new InvalidOperationException($"{name}: ekstensi lebih dari 3 huruf tidak muat di FAT 8.3 (pakai 'remotePath')");
        return "/disk/" + stem + (ext.Length > 0 ? "." + ext : "");
    }
}

// ---- scripts: replay a recorded run ---------------------------------------------

sealed class ScriptSession : Session
{
    sealed record Stop(int Line, int OutLines, List<(string Fn, int Line)> Stack, List<(string, string)> Locals, List<(string, string)> Globals);

    readonly List<Stop> _stops = new();
    readonly List<string> _output = new();
    string? _error;
    int _at = -1;
    int _emitted;
    bool _errorShown;
    readonly bool _stopOnEntry;
    public const int MaxStops = 400;

    public ScriptSession(DapServer dap, IDebugDevice dev, JsonObject a, string program) : base(dap, dev)
    {
        LocalPath = DapServer.Norm(program);
        Name = Path.GetFileName(program);
        _stopOnEntry = a["stopOnEntry"]?.GetValue<bool>() ?? false;
        if (!File.Exists(LocalPath)) throw new InvalidOperationException("berkas tidak ada: " + LocalPath);
        string text = File.ReadAllText(LocalPath);
        string ext = Path.GetExtension(LocalPath).ToLowerInvariant();
        string lang = ext switch { ".js" => "js", ".ts" => "ts", ".py" => "py", _ => "csx" };
        if (text.Contains("window(") || text.Contains("Ui.Window("))
            throw new InvalidOperationException("skrip UI (yang membuka jendela) belum bisa di-debug; jalankan dengan 'bz deploy'");
        string remote = a["remotePath"]?.GetValue<string>() ?? RemoteName(LocalPath);
        Dap.Output($"bz dap: {Name} -> {remote}", "console");
        Dev.Upload(remote, Encoding.UTF8.GetBytes(text));
        var (_, output) = Dev.Exec($"script debug {lang} {remote} all");
        Parse(output);
        Dap.Output($"bz dap: {_stops.Count} henti direkam" + (_stops.Count >= MaxStops ? $" (batas {MaxStops}: sisa program berjalan tanpa rekaman)" : ""), "console");
    }

    void Parse(string text)
    {
        foreach (var raw in text.Split('\n'))
        {
            string l = raw.TrimEnd('\r');
            if (l.StartsWith("@dbg "))
            {
                var f = l[5..].Split('\t');
                if (f.Length < 5 || !int.TryParse(f[0], out int line)) continue;
                int.TryParse(f[1], out int outLines);
                var stack = new List<(string, int)>();
                foreach (var fr in f[2].Split(',', StringSplitOptions.RemoveEmptyEntries))
                {
                    int c = fr.LastIndexOf(':');
                    if (c > 0 && int.TryParse(fr[(c + 1)..], out int fl)) stack.Add((fr[..c], fl));
                }
                if (stack.Count == 0) stack.Add(("<main>", line));
                _stops.Add(new Stop(line, outLines, stack, Vars(f[3]), Vars(f[4])));
            }
            else if (l.StartsWith("  error: ")) _error = l[9..];
            else if (l.StartsWith("  ")) _output.Add(l[2..]);
            else if (l.StartsWith("error") || l.StartsWith("script:")) _error ??= l; // the shell refused (no file, parse error)
        }
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

    HashSet<int> _breaks = new();

    public override (bool, string?) LineBreakpoint(string path, int line)
    {
        if (!string.Equals(path, LocalPath, StringComparison.OrdinalIgnoreCase)) return (false, "bukan berkas yang di-debug");
        _breaks = Dap.LinesFor(LocalPath!).ToHashSet();
        if (_stops.Count == 0) return (true, null);
        return _stops.Any(s => s.Line == line) ? (true, null) : (false, "baris ini tidak dieksekusi dalam rekaman");
    }

    public override void Start(int[] lines, string[] functions)
    {
        _breaks = lines.ToHashSet();
        if (_stops.Count == 0)
        {
            Finish();
            return;
        }
        if (_stopOnEntry) { Show(0, "entry"); return; }
        Resume(Motion.Continue, fromStart: true);
    }

    public override void Resume(Motion how) => Resume(how, fromStart: false);

    void Resume(Motion how, bool fromStart)
    {
        int cur = fromStart ? -1 : _at;
        int depth = cur >= 0 ? _stops[cur].Stack.Count : int.MaxValue;
        int next = -1;
        string reason = "step";
        switch (how)
        {
            case Motion.Continue:
                next = Find(cur + 1, 1, s => _breaks.Contains(s.Line));
                reason = "breakpoint";
                if (next < 0 && _error != null && !_errorShown)
                {
                    _errorShown = true;
                    Show(_stops.Count - 1, "exception", _error);
                    return;
                }
                break;
            case Motion.In: next = cur + 1 < _stops.Count ? cur + 1 : -1; break;
            case Motion.Over: next = Find(cur + 1, 1, s => s.Stack.Count <= depth); break;
            case Motion.Out: next = Find(cur + 1, 1, s => s.Stack.Count < depth); break;
            case Motion.Back:
                Show(Math.Max(0, cur - 1), "step");
                return;
            case Motion.ReverseContinue:
                int prev = Find(cur - 1, -1, s => _breaks.Contains(s.Line));
                Show(prev < 0 ? 0 : prev, prev < 0 ? "entry" : "breakpoint");
                return;
        }
        if (next < 0) Finish();
        else Show(next, reason);
    }

    int Find(int from, int dir, Func<Stop, bool> ok)
    {
        for (int i = from; i >= 0 && i < _stops.Count; i += dir)
            if (ok(_stops[i])) return i;
        return -1;
    }

    void Show(int i, string reason, string? text = null)
    {
        _at = i;
        Emit(_stops[i].OutLines);
        Dap.Stopped(reason, text);
    }

    void Emit(int upTo)
    {
        for (; _emitted < Math.Min(upTo, _output.Count); _emitted++) Dap.Output(_output[_emitted]);
    }

    void Finish()
    {
        Emit(int.MaxValue);
        if (_error != null) Dap.Output("error: " + _error, "stderr");
        ExitCode = _error != null ? 1 : 0;
        Dap.Finished(ExitCode);
    }

    Stop Cur => _at >= 0 ? _stops[_at] : throw new InvalidOperationException("program tidak sedang berhenti");

    public override JsonArray StackTrace()
    {
        var s = Cur;
        var res = new JsonArray();
        for (int k = 0; k < s.Stack.Count; k++)
            res.Add(new JsonObject
            {
                ["id"] = k,
                ["name"] = s.Stack[k].Fn,
                ["line"] = s.Stack[k].Line,
                ["column"] = 1,
                ["source"] = new JsonObject { ["name"] = Name, ["path"] = LocalPath },
            });
        return res;
    }

    public override JsonArray Scopes(int frame)
    {
        var res = new JsonArray();
        // the recording keeps the innermost function's locals and the globals
        if (frame == 0 && Cur.Stack.Count > 1)
            res.Add(new JsonObject { ["name"] = "Lokal", ["variablesReference"] = 1, ["expensive"] = false, ["presentationHint"] = "locals" });
        res.Add(new JsonObject { ["name"] = "Global", ["variablesReference"] = 2, ["expensive"] = false });
        return res;
    }

    public override JsonArray Variables(int reference)
    {
        var list = reference == 1 ? Cur.Locals : reference == 2 ? Cur.Globals : new List<(string, string)>();
        var res = new JsonArray();
        foreach (var (n, v) in list) res.Add(new JsonObject { ["name"] = n, ["value"] = v, ["variablesReference"] = 0 });
        return res;
    }

    public override (string, string?) Evaluate(string expr, string context)
    {
        string name = expr.Trim();
        if (_at < 0) return ("", "program tidak sedang berhenti");
        foreach (var (n, v) in Cur.Locals.Concat(Cur.Globals))
            if (n == name) return (v, null);
        return ("", $"'{name}' tidak ada di rekaman henti ini (hanya nama variabel)");
    }

    public override void Terminate() { }
}

// ---- native programs: the live debugger -------------------------------------------

sealed class NativeSession : Session
{
    static readonly Regex StopRx = new(@"pid (\d+) stopped \((\w+)\) at (0x[0-9a-f]+)\s*(.*)", RegexOptions.Compiled);
    static readonly Regex ExitRx = new(@"pid (\d+) exited with code (0x[0-9a-f]+)", RegexOptions.Compiled);
    static readonly Regex FrameRx = new(@"#(\d+)\s+(0x[0-9a-f]+)\s+(.*)", RegexOptions.Compiled);
    static readonly Regex RegRx = new(@"(\S+)\s+(0x[0-9a-f]+)", RegexOptions.Compiled);

    readonly bool _stopOnEntry;
    readonly HashSet<string> _set = new();
    public uint Pid { get; }
    volatile bool _running;
    volatile bool _over;

    public NativeSession(DapServer dap, IDebugDevice dev, JsonObject a, string program) : base(dap, dev)
    {
        _stopOnEntry = a["stopOnEntry"]?.GetValue<bool>() ?? false;
        string target = program;
        if (File.Exists(program))
        {
            target = a["remotePath"]?.GetValue<string>() ?? RemoteName(program);
            Dap.Output($"bz dap: {Path.GetFileName(program)} -> {target}", "console");
            Dev.Upload(target, File.ReadAllBytes(program));
        }
        Name = Path.GetFileName(target);
        string args = string.Join(' ', (a["args"] as JsonArray ?? new JsonArray()).Select(x => x?.GetValue<string>() ?? ""));
        var (_, o) = Dev.Exec($"dbg run {target} {args}".TrimEnd());
        var m = StopRx.Match(o);
        if (!m.Success) throw new InvalidOperationException(o.Trim());
        Pid = uint.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture);
        Dap.Output($"bz dap: pid {Pid} berhenti di awal program ({m.Groups[4].Value.Trim()})", "console");
    }

    public override (bool, string?) LineBreakpoint(string path, int line) =>
        (false, "program native tidak punya informasi baris: pakai breakpoint fungsi (nama simbol)");

    public override IEnumerable<(bool, string?)> FunctionBreakpoints(string[] names, bool live)
    {
        var res = new List<(bool, string?)>();
        foreach (var gone in _set.Where(n => !names.Contains(n)).ToList())
        {
            Dev.Exec("dbg delete " + gone);
            _set.Remove(gone);
        }
        foreach (var n in names)
        {
            if (_set.Contains(n)) { res.Add((true, null)); continue; }
            if (_running) { res.Add((false, "program sedang berjalan")); continue; }
            var (_, o) = Dev.Exec("dbg break " + n);
            bool ok = o.Contains("breakpoint") && !o.StartsWith("dbg:");
            if (ok) _set.Add(n);
            res.Add((ok, o.Trim()));
        }
        return res;
    }

    public override void Start(int[] lines, string[] functions)
    {
        FunctionBreakpoints(functions, false).ToList();
        if (_stopOnEntry) Dap.Stopped("entry");
        else Resume(Motion.Continue);
    }

    public override void Resume(Motion how)
    {
        switch (how)
        {
            case Motion.Back:
            case Motion.ReverseContinue:
                Dap.Output("bz dap: mundur hanya untuk skrip (rekaman); program native berjalan maju", "console");
                Dap.Stopped("step");
                return;
            case Motion.Over:
            case Motion.In:
                Status(Dev.Exec("dbg step").Output, "step");
                return;
            case Motion.Out:
            {
                var frames = Dev.Exec("dbg bt").Output.Split('\n').Select(l => FrameRx.Match(l)).Where(m => m.Success).ToList();
                if (frames.Count < 2) { Dap.Output("bz dap: pemanggil tidak diketahui", "console"); Dap.Stopped("step"); return; }
                string ret = frames[1].Groups[2].Value;
                bool had = Dev.Exec("dbg break " + ret).Output.Contains("already");
                RunUntilStop(() => Dev.Exec("dbg cont").Output, "step");
                if (!had && !_over) Dev.Exec("dbg delete " + ret);
                return;
            }
            default:
                // the program may run longer than one request: wait on a worker
                _running = true;
                Task.Run(() =>
                {
                    try { RunUntilStop(() => Dev.Exec("dbg cont").Output, null); }
                    catch (Exception e) { Dap.Output("bz dap: " + e.Message, "stderr"); Dap.Finished(1); }
                    finally { _running = false; }
                });
                return;
        }
    }

    void RunUntilStop(Func<string> first, string? reason)
    {
        string o = first();
        while (!_over && o.Contains("is running"))
            o = Dev.Exec("dbg wait 2000").Output;
        if (!_over) Status(o, reason);
    }

    void Status(string o, string? reasonOverride)
    {
        var m = StopRx.Match(o);
        if (m.Success)
        {
            string why = m.Groups[2].Value;
            string reason = reasonOverride ?? why switch
            {
                "breakpoint" => "function breakpoint",
                "fault" => "exception",
                "entry" => "entry",
                _ => "step",
            };
            if (why == "fault") reason = "exception";
            Dap.Stopped(reason, why == "fault" ? "program fault di " + m.Groups[4].Value.Trim() : null);
            return;
        }
        var e = ExitRx.Match(o);
        _over = true;
        if (e.Success) ExitCode = Convert.ToInt32(e.Groups[2].Value, 16);
        Dap.Output("bz dap: " + o.Trim(), "console");
        Dap.Finished(ExitCode);
    }

    public override JsonArray StackTrace()
    {
        var res = new JsonArray();
        foreach (var l in Dev.Exec("dbg bt").Output.Split('\n'))
        {
            var m = FrameRx.Match(l);
            if (!m.Success) continue;
            res.Add(new JsonObject
            {
                ["id"] = int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture),
                ["name"] = m.Groups[3].Value.Trim() + "  " + m.Groups[2].Value,
                ["line"] = 0,
                ["column"] = 0,
                ["instructionPointerReference"] = m.Groups[2].Value,
                ["presentationHint"] = "normal",
            });
        }
        return res;
    }

    public override JsonArray Scopes(int frame) => new()
    {
        new JsonObject { ["name"] = "Register", ["variablesReference"] = 3, ["expensive"] = false, ["presentationHint"] = "registers" },
    };

    public override JsonArray Variables(int reference)
    {
        var res = new JsonArray();
        if (reference != 3) return res;
        foreach (var l in Dev.Exec("dbg regs").Output.Split('\n'))
            foreach (Match m in RegRx.Matches(l))
                res.Add(new JsonObject { ["name"] = m.Groups[1].Value, ["value"] = m.Groups[2].Value, ["variablesReference"] = 0 });
        return res;
    }

    public override string SetVariable(int reference, string name, string value)
    {
        var (_, o) = Dev.Exec($"dbg set {name} {value.Trim()}");
        if (o.StartsWith("dbg:")) throw new InvalidOperationException(o.Trim());
        int eq = o.IndexOf('=');
        return eq > 0 ? o[(eq + 1)..].Trim() : value;
    }

    public override (string, string?) Evaluate(string expr, string context)
    {
        string e = expr.Trim();
        if (e.Length == 0) return ("", null);
        if (context == "hover") return ("", "tidak ada nilai untuk hover di program native");
        if (e.StartsWith("dbg ")) e = e[4..];
        return (Dev.Exec("dbg " + e).Output.TrimEnd(), null);
    }

    public override void Terminate()
    {
        if (_over) return;
        _over = true;
        // a running program cannot take a debugger request: end it from the shell
        Dev.Exec(_running ? $"kill {Pid}" : "dbg kill");
    }
}
