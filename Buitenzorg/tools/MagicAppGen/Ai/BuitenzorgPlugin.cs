using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading.Tasks;
using MagicAppGen.Models;
using MagicAppGen.Services;
using Microsoft.SemanticKernel;

namespace MagicAppGen.Ai;

/// <summary>Kernel functions that let "Jack" actually build apps for Buitenzorg
/// OS: the API/gotcha knowledge base, project scaffolding, file I/O, and the
/// build/run scripts. Registered on the SK kernel so the model can call them.</summary>
public sealed class BuitenzorgPlugin
{
    readonly Settings _settings;
    readonly Action<string> _log;

    public BuitenzorgPlugin(Settings settings, Action<string> log)
    {
        _settings = settings;
        _log = log;
    }

    string Root => string.IsNullOrWhiteSpace(_settings.BuitenzorgRoot)
        ? Directory.GetCurrentDirectory()
        : _settings.BuitenzorgRoot;

    [KernelFunction, Description(
        "Return the API reference and zerolib gotchas for a Buitenzorg library " +
        "so generated code compiles under bflat --stdlib:zero. library is one of: " +
        "drawing, ui, audio, bcl, syscalls, gotchas.")]
    public string GetApiReference([Description("drawing|ui|audio|bcl|syscalls|gotchas")] string library)
        => library.ToLowerInvariant() switch
        {
            "drawing" =>
                "namespace Buitenzorg.Drawing (bzgfx.cs). EXACT signatures (argument order matters):\n" +
                "  new Color(uint argb); Color.White/Black/Transparent\n" +
                "  new Bitmap(int w, int h)  // .Width .Height .Pixels (uint[] ARGB)\n" +
                "  new Graphics(Bitmap b); g.Clear(Color c)\n" +
                "  g.DrawLine(Color c, int x0, int y0, int x1, int y1[, int thick])\n" +
                "  g.FillRectangle(Color c, int x, int y, int w, int h)\n" +
                "  g.FillGradientV(int x, int y, int w, int h, Color top, Color bottom)\n" +
                "  g.FillRoundedRectangle(Color c, int x, int y, int w, int h, int rad)\n" +
                "  g.FillRoundedGradientV(int x, int y, int w, int h, int rad, Color top, Color bottom)\n" +
                "  g.DrawShadow(int x, int y, int w, int h, int rad, int spread, int alpha)\n" +
                "  g.FillCircleAA(Color c, int cx, int cy, int r); g.DrawCircle(Color c, int cx, int cy, int r)\n" +
                "  g.DrawString(Font f, string s, Color c, int x, int y)\n" +
                "  g.DrawChars(Font f, char[] s, int len, Color c, int x, int y)   // for dynamic text\n" +
                "  g.SinFx/CosFx(int deg) -> fixed point /256; Matrix, GraphicsPath, SetClip/ResetClip\n" +
                "  Font.Default(); Bmp.Load/Save (24-bit); Jpeg.Load(byte[]) (baseline)\n" +
                "  Window.Create(string title, int w, int h); win.Blit(Bitmap b[, int x, int y]); win.Present()",
            "ui" =>
                "REQUIRED usings for a Buitenzorg.UI app (Font/Color live in Buitenzorg.Drawing, Console in System):\n" +
                "  using System;              // Console.WriteLine for milestones/output\n" +
                "  using Buitenzorg.Drawing;  // Font, Color, Graphics, Bitmap\n" +
                "  using Buitenzorg.UI;       // UIHost, StackPanel, Button, TextBlock, ...\n" +
                "  Print milestones with Console.WriteLine(\"MILESTONE: X OK\") - NOT Con.WriteLine (Con only has\n" +
                "  Write(char[], int)). Color.White/Black/Transparent exist; Font.Default() (no `new Font()`).\n" +
                "namespace Buitenzorg.UI (bzui.cs): retained tree, children are a LINKED LIST (never object[]).\n" +
                "  UIElement fields: Width/Height (-1 = auto), DesiredW/DesiredH, X/Y/W/H, Visible, Background\n" +
                "  override void Measure(int availW, int availH) and void Render(Graphics g)\n" +
                "  Panels: StackPanel (.Spacing .Padding .Add(e)), Grid (.AddColumn/.AddRow(-1 = star), .Add(e,row,col)), Canvas, Border\n" +
                "          DockPanel (set child.Dock = 0/1/2/3 for Left/Top/Right/Bottom; .LastChildFill fills the rest)\n" +
                "          GroupBox(string title, Font) .SetContent(e) — a titled bordered frame\n" +
                "  Controls: new TextBlock(string, Font), new Button(string, Font) (.Tag int), new CheckBox(string, Font),\n" +
                "            ProgressBar, Slider, RadioButton+RadioGroup, ListBox, TextBox, Menu, ComboBox,\n" +
                "            TabControl, TreeView, ScrollViewer, DataGrid\n" +
                "  v0.16 additions (modelled on TinyCLR): \n" +
                "    new Image(Bitmap) .Stretch (0 none/1 fill/2 uniform) — draws a bitmap fit-to-box\n" +
                "    new Expander(string header, Font) .SetContent(e) .Expanded — collapsible section (click header toggles)\n" +
                "    new Gauge(Font) .Value/.Min/.Max — semicircular dial + numeric readout\n" +
                "    new Chart() .SetData(int[] vals, int n) .AsLine (bars default) — bar/line chart over a value series\n" +
                "    new Calendar(Font) .Year/.Month/.Day/.FirstDow — month grid, click selects a day\n" +
                "    new TextFlow(Font) .Append(string, Color) — word-wrapped multi-color rich text (no live numbers)\n" +
                "    new MessageBox(Font) .Show(title, msg); modal overlay, .Result (0=OK 1=Cancel) after a click, auto-closes\n" +
                "    Shapes (Stroke/Fill/Thickness, use Width/Height for size): RectShape (.CornerRadius),\n" +
                "      EllipseShape, LineShape (.Down), PolygonShape .SetPoints(int[] xs, int[] ys, int n) (coords relative to X,Y)\n" +
                "    UiText.Int(int, char[])->len and UiText.Int2(int, char[])->len format numbers into a char[] (no strings)\n" +
                "  new UIHost(string title, int w, int h); host.Root = e; host.Layout(); host.Render(Color clear);\n" +
                "  host.Present(); host.Mouse(int x, int y, bool down) routes hover/click/drag.\n" +
                "  BUTTON CLICKS: Button has NO event/callback (no OnClick, no delegate - a delegate would be a\n" +
                "    GC static and fault). It exposes `int Clicks` (bumped on each click) and `int Tag`. To react:\n" +
                "      int before = btn.Clicks;\n" +
                "      host.Mouse(btn.X + btn.W/2, btn.Y + btn.H/2, true); host.Mouse(..., false);\n" +
                "      if (btn.Clicks > before) { /* do the action; dispatch by btn.Tag if you have many */ }\n" +
                "  TextBlock.Text is a STRING and its ctor is TextBlock(string, Font) - you CANNOT pass a char[]\n" +
                "    and you cannot build a dynamic string (no concat/ToString). For a NUMBER that changes (a\n" +
                "    counter, a score, a clock), write a small custom UIElement whose Render calls\n" +
                "    g.DrawChars(font, buf, len, color, x, y) with a char[] you fill by hand - see CalcDisplay in\n" +
                "    calc.cs / Board2048 in game2048.cs. Do NOT try to change a TextBlock to show live numbers.\n" +
                "  NOT AVAILABLE under zerolib: Array.Reverse, Span/AsSpan/ToArray, LINQ on arrays, string concat.\n" +
                "    Reverse a char[] with a manual loop. Keep counters/state in LOCALS or instance fields, never\n" +
                "    a `static` reference field (static char[]/string/array = GC static = fault).",
            "audio" =>
                "namespace Buitenzorg.Audio (bzaudio.cs): AudioInfo Mixer.GetInfo() (.Present .SampleRate .Channels " +
                ".Bits .Volume); Mixer.SetVolume(int pct 0..100); Mixer.GetVolume(); Mixer.Mute(); " +
                "Mixer.Beep(int freqHz, int durationMs); Mixer.Play(short[] pcm) interleaved 16-bit stereo @48kHz; Tone.Square.",
            "bcl" =>
                "namespace Buitenzorg. Add BOTH bzbcl.cs and bzbcl2.cs to the source list to use it.\n" +
                "  bzbcl.cs: BzList<T>/BzStack<T>/BzQueue<T>/BzIntMap<V>/BzStrMap<V>/BzIntSet/\n" +
                "    BzRefList<T> (linked list for REFERENCE elements), BzSort, BzLinq (Where/Select/Sum/\n" +
                "    Count/Any/All/Max/Min/Contains/IndexOf/Reverse/First/Last/Take/Skip/Average/Aggregate,\n" +
                "    predicates are FUNCTION POINTERS), BzStringBuilder, BzMath, BzRandom, BzConvert,\n" +
                "    BzStr, BzHex, BzBase64, BzBitConverter, Con.Write(char[], len)\n" +
                "  bzbcl2.cs maps the remaining .NET namespaces (all output goes into caller-supplied\n" +
                "  char[]/byte[] buffers and returns the length — there are no managed strings):\n" +
                "    System.IO      BzPath.Combine/GetFileName/GetDirectoryName/GetExtension/HasExtension/Up\n" +
                "                   BzFile.ReadAllBytes(path, max, out byte[])/ReadAllChars/WriteAllBytes/\n" +
                "                     WriteAllChars/Exists   (writes need a writable mount: /ram, not /disk)\n" +
                "                   BzDir.GetEntries(path, max)/GetMounts/Count/Contains -> BzFileInfo list\n" +
                "                   BzMemoryStream Read/Write/ReadByte/WriteByte/Seek/ToArray\n" +
                "    System.Text    BzEncoding.Utf8GetBytes/Utf8GetChars/Utf8ByteCount/AsciiGetBytes/AsciiGetChars\n" +
                "    Regex          new BzRegex(pattern).IsMatch/Match(out end)/Replace/Split/MatchAt\n" +
                "                   supports literals . [abc] [^a-z] ranges ^ $ * + ? | (...) \\\\d \\\\w \\\\s;\n" +
                "                   NOT backreferences, lazy quantifiers, {n,m}, lookaround, captures\n" +
                "    Globalization  BzCulture.FormatInt/FormatIntAt/FormatGrouped/FormatFixed/FormatPercent/\n" +
                "                     FormatBytes/ToUpperInvariant/ToLowerInvariant/MonthAbbrev\n" +
                "                   BzDateTime.Now() (real CMOS clock) .Year/.Month/.Day/.Hour/.Minute/.Second,\n" +
                "                     IsValid/IsLeapYear/DaysInMonth/DayOfWeek/FormatDate/FormatTime/Format\n" +
                "    Diagnostics    BzStopwatch.StartNew/Start/Stop/Restart/ElapsedTicks (TSC ticks)\n" +
                "                   BzProcess.GetProcesses(max)/Count/FindByName/Kill -> BzProcessInfo list\n" +
                "                   BzDebug.Write/WriteLine/Assert(cond, msg)\n" +
                "    Management     BzSystemInfo.Query() -> UptimeTicks/TickHz/UptimeSeconds/HeapUsed/\n" +
                "                     HeapTotal/HeapPercent/TaskCount/MemTotalMib/Audio*\n" +
                "    Net/Sockets    BzIPAddress.Parse/Format, BzNetInfo.Query(),\n" +
                "                   BzSocket.CreateUdp/Bind(port)/SendTo(ip, port, data|string)/\n" +
                "                     Receive(buf, max)/ReceiveWithRetry/Close  (UDP only, LOOPBACK only)\n" +
                "    Net.Http       BzHttp.BuildGet/BuildPost/ParseStatus/GetHeader — MESSAGE LAYER ONLY,\n" +
                "                     the kernel has no TCP yet, so it cannot connect anywhere\n" +
                "    Tasks          BzTask.Run(&Method, arg) (body is a delegate*<ulong,void>, NOT a\n" +
                "                     delegate)/Wait/WhenAll/Yield, BzMutex(int* state) Lock/Unlock\n" +
                "    Timers         new BzTimer(intervalTicks) Start/Stop/Poll/Remaining/Count/AutoReset —\n" +
                "                     POLLED from the app loop; ring 3 has no signal delivery\n" +
                "    GC             BzGC.GetAllocatedBytes/GetTotalMemory/ChunkCount/AllocationCount;\n" +
                "                     Collect() returns FALSE — the heap is bump-only, nothing is reclaimed\n" +
                "    Pkg            BzPkg.List(max)/Count/Find/Search/IsInstalled/Install/Remove",
            "syscalls" =>
                "Shim wrappers (bzstart.rs): bz_win_create/bz_win_cmd/bz_win_present, bz_key_read, " +
                "bz_fs_list/bz_fs_read, bz_is_interactive, bz_mmap/mprotect, bz_thread_*, bz_audio_*, " +
                "bz_pkg_list/set. Call via [DllImport(\"*\")] static extern.",
            _ =>
                "zerolib gotchas: (1) NO static reference fields (GC statics read garbage) - keep state in " +
                "locals/instance fields. (2) NO method-group->delegate (caches in a GC static) - use function " +
                "pointers delegate*<...> + &Method. (3) NO storing a reference into an object[] element " +
                "(stelem.ref faults) - use linked lists / object fields. (4) NO new string()/ToString()/concat/" +
                "string == - build char[] and use Graphics.DrawChars, compare by reference or char loop. " +
                "(5) Entry: class with static void Main(). App ELF name must be <=8.3, uppercase.",
        };

    [KernelFunction, Description("List the available Buitenzorg app project templates (id, language, what it contains).")]
    public string ListTemplates() =>
        string.Join("; ", ProjectTemplates.All.Select(t => $"{t.Id} [{t.Language}] — {t.Summary}"));

    [KernelFunction, Description("Return the full starter source of one template, so it can be shown or extended.")]
    public string GetTemplateSource([Description("template id from ListTemplates")] string template)
    {
        var t = ProjectTemplates.Find(template);
        if (t is null) return $"error: unknown template '{template}'";
        return string.Join("\n\n", t.Files.Select(f => $"--- {f.Key} ---\n{f.Value}"));
    }

    [KernelFunction, Description("Create a new project folder from a template (real starter files). Returns the main source file's path.")]
    public string ScaffoldProject(
        [Description("project folder path")] string path,
        [Description("template id from ListTemplates")] string template,
        [Description("app name used in the generated files")] string appName = "MyApp")
    {
        try
        {
            var t = ProjectTemplates.Find(template);
            if (t is null) return $"error: unknown template '{template}'. Call ListTemplates first.";
            var file = ProjectTemplates.Scaffold(t, path, appName);
            _log($"[buitenzorg] scaffolded '{t.Title}' at {path}");
            return file;
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description("Write text content to a file (creating folders). Returns ok or an error.")]
    public string WriteFile([Description("file path")] string path, [Description("full file content")] string content)
    {
        try
        {
            var dir = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(dir)) Directory.CreateDirectory(dir);
            File.WriteAllText(path, content);
            _log($"[buitenzorg] wrote {path} ({content.Length} chars)");
            return "ok";
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description("Read a text file and return its content.")]
    public string ReadFile([Description("file path")] string path)
    {
        try { return File.ReadAllText(path); }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description(
        "Compile a generated ring-3 C# app with the real bflat --stdlib:zero toolchain to check it " +
        "actually builds. Pass the path to the app's .cs file (e.g. the main.cs you wrote). The needed " +
        "Buitenzorg library sources are added automatically from its `using` directives " +
        "(Buitenzorg.Drawing->bzgfx.cs, Buitenzorg.UI->bzui.cs, Buitenzorg.Audio->bzaudio.cs, " +
        "Buitenzorg->bzbcl.cs+bzbcl2.cs). Returns 'OK: compiles' or the compiler errors. ALWAYS call " +
        "this after writing an app, and if it returns errors, fix them and call it again until it says OK.")]
    public string CompileCheck([Description("path to the app's main .cs file")] string csFile)
    {
        try
        {
            if (!File.Exists(csFile)) return $"error: {csFile} not found";
            var bflat = Path.Combine(Root, "tools", "bflat", "bflat.exe");
            if (!File.Exists(bflat))
                return "error: bflat not found under <repo>/tools/bflat (set Buitenzorg.Root in Settings)";
            var userland = Path.Combine(Root, "userland", "hello-csharp");

            // The library sources this app pulls in, detected from its usings.
            var src = File.ReadAllText(csFile);
            var sources = new System.Collections.Generic.List<string> { csFile };
            void Need(string lib, params string[] files)
            {
                if (src.Contains(lib))
                    foreach (var f in files)
                    {
                        var p = Path.Combine(userland, f);
                        if (File.Exists(p) && !sources.Contains(p)) sources.Add(p);
                    }
            }
            Need("Buitenzorg.Drawing", "bzgfx.cs");
            Need("Buitenzorg.UI", "bzui.cs", "bzgfx.cs");
            Need("Buitenzorg.Audio", "bzaudio.cs");
            Need("using Buitenzorg;", "bzbcl.cs", "bzbcl2.cs");

            // Full paths, and a unique obj in the temp dir (not the project) so
            // repeated checks never collide.
            var full = sources.Select(Path.GetFullPath).ToList();
            var obj = Path.Combine(Path.GetTempPath(), $"bzcc_{Guid.NewGuid():N}.o");
            // NB: the `build` subcommand is required (bflat build <files> ...);
            // without it bflat rejects every argument as "unrecognized".
            var args = "build " + string.Join(' ', full.Select(s => $"\"{s}\"")) +
                       " --stdlib:zero --os:linux --arch:x64 -c -Os --no-debug-info " +
                       $"--no-reflection --no-stacktrace-data -o \"{obj}\"";
            _log($"[buitenzorg] compile-check: bflat {full.Count} source(s)");

            var psi = new ProcessStartInfo
            {
                FileName = bflat,
                Arguments = args,
                WorkingDirectory = userland,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            // Drain both pipes concurrently, then wait, so a full pipe buffer
            // can never deadlock the child.
            using var proc = Process.Start(psi)!;
            var outTask = proc.StandardOutput.ReadToEndAsync();
            var errTask = proc.StandardError.ReadToEndAsync();
            bool exited = proc.WaitForExit(120_000);
            var diag = ((errTask.GetAwaiter().GetResult() ?? "") + "\n" +
                        (outTask.GetAwaiter().GetResult() ?? "")).Trim();
            int exit = exited ? proc.ExitCode : -1;
            try { if (File.Exists(obj)) File.Delete(obj); } catch { }

            if (exit == 0)
            {
                _log("[buitenzorg] compile-check: OK");
                return "OK: compiles with bflat --stdlib:zero";
            }
            var tail = diag.Length > 2000 ? diag[^2000..] : diag;
            _log($"[buitenzorg] compile-check: FAILED (exit {exit}): {tail.Replace('\n', ' ')}");
            if (string.IsNullOrWhiteSpace(tail))
                tail = exited ? $"bflat exited {exit} with no diagnostics" : "bflat timed out";
            return "COMPILE ERRORS (fix these and call CompileCheck again):\n" + tail;
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description(
        "Compile AND DEPLOY a ring-3 C# app so the OS can run it. Builds <csFile> (+ the Buitenzorg " +
        "library sources detected from its usings) with bflat --stdlib:zero, links it with the bzstart " +
        "shim into a static ELF, and installs it as userland/hello-csharp/userapp.elf — which the kernel " +
        "image embeds as /disk/USERAPP.ELF. After this succeeds, call BuildApp() to rebuild the image, then " +
        "in the Buitenzorg shell launch it with `run myapp`. Returns 'OK: deployed' or the build errors. " +
        "Call CompileCheck first; only Deploy once it compiles.")]
    public string DeployApp([Description("path to the app's main .cs file")] string csFile)
    {
        try
        {
            if (!File.Exists(csFile)) return $"error: {csFile} not found";
            var bflat = Path.Combine(Root, "tools", "bflat", OperatingSystem.IsWindows() ? "bflat.exe" : "bflat");
            if (!File.Exists(bflat)) return "error: bflat not found under <repo>/tools/bflat (set Buitenzorg.Root in Settings)";
            var userland = Path.Combine(Root, "userland", "hello-csharp");
            var lld = FindRustLld();
            if (lld is null) return "error: rust-lld not found (install the rust nightly toolchain)";

            // Ensure the freestanding startup/PAL shim object exists.
            var bzstartO = Path.Combine(userland, "bzstart.o");
            if (!File.Exists(bzstartO))
            {
                var (rc, ro) = RunTool("rustc", "+nightly --edition 2021 --crate-type staticlib --emit obj " +
                    $"--target x86_64-unknown-none -C panic=abort -C opt-level=2 -o \"{bzstartO}\" \"{Path.Combine(userland, "bzstart.rs")}\"",
                    userland);
                if (rc != 0) return "error building bzstart.o:\n" + ro;
            }

            // Library sources this app pulls in (same detection as CompileCheck).
            var src = File.ReadAllText(csFile);
            var sources = new System.Collections.Generic.List<string> { Path.GetFullPath(csFile) };
            void Need(string lib, params string[] files)
            {
                if (src.Contains(lib))
                    foreach (var f in files)
                    {
                        var p = Path.Combine(userland, f);
                        if (File.Exists(p) && !sources.Contains(Path.GetFullPath(p))) sources.Add(Path.GetFullPath(p));
                    }
            }
            Need("Buitenzorg.Drawing", "bzgfx.cs");
            Need("Buitenzorg.UI", "bzui.cs", "bzgfx.cs");
            Need("Buitenzorg.Audio", "bzaudio.cs");
            Need("using Buitenzorg;", "bzbcl.cs", "bzbcl2.cs");

            var obj = Path.Combine(userland, "userapp.o");
            var elf = Path.Combine(userland, "userapp.elf");
            var cargs = "build " + string.Join(' ', sources.Select(s => $"\"{s}\"")) +
                        " --stdlib:zero --os:linux --arch:x64 -c -Os --no-debug-info " +
                        $"--no-reflection --no-stacktrace-data -o \"{obj}\"";
            _log($"[buitenzorg] deploy: bflat {sources.Count} source(s)");
            var (cc, co) = RunTool(bflat, cargs, userland);
            if (cc != 0) return "COMPILE ERRORS (fix, then CompileCheck + DeployApp again):\n" + Tail(co);

            var largs = $"-flavor gnu -o \"{elf}\" -T \"{Path.Combine(userland, "user.ld")}\" " +
                        $"--static --no-dynamic-linker -e _start \"{obj}\" \"{bzstartO}\"";
            _log("[buitenzorg] deploy: rust-lld link -> userapp.elf");
            var (lc, lo) = RunTool(lld, largs, userland);
            if (lc != 0) return "LINK ERRORS:\n" + Tail(lo);

            _log("[buitenzorg] deploy: OK (userapp.elf)");
            return "OK: deployed userapp.elf. Next: call BuildApp() to rebuild the image, then in the OS shell run: run myapp";
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    static string Tail(string s) => s.Length > 2000 ? s[^2000..] : s;

    // Locate rust-lld inside the installed rustup toolchains (glob, cross-OS).
    static string? FindRustLld()
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var toolchains = Path.Combine(home, ".rustup", "toolchains");
        if (!Directory.Exists(toolchains)) return null;
        var name = OperatingSystem.IsWindows() ? "rust-lld.exe" : "rust-lld";
        foreach (var tc in Directory.GetDirectories(toolchains))
        {
            try
            {
                var hit = Directory.GetFiles(tc, name, SearchOption.AllDirectories).FirstOrDefault();
                if (hit != null) return hit;
            }
            catch { }
        }
        return null;
    }

    // Run a build tool, draining both pipes so a full buffer can't deadlock.
    (int code, string output) RunTool(string file, string args, string cwd)
    {
        var psi = new ProcessStartInfo
        {
            FileName = file,
            Arguments = args,
            WorkingDirectory = cwd,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        using var proc = Process.Start(psi)!;
        var outTask = proc.StandardOutput.ReadToEndAsync();
        var errTask = proc.StandardError.ReadToEndAsync();
        bool exited = proc.WaitForExit(180_000);
        var diag = ((errTask.GetAwaiter().GetResult() ?? "") + "\n" +
                    (outTask.GetAwaiter().GetResult() ?? "")).Trim();
        return (exited ? proc.ExitCode : -1, diag);
    }

    [KernelFunction, Description("Build the Buitenzorg OS image (runs scripts/build.ps1 in the repo root). Returns the tail of the build output.")]
    public async Task<string> BuildApp()
        => await RunScript("scripts/build.ps1", "");

    [KernelFunction, Description("Boot the built Buitenzorg image headlessly and check milestone markers (scripts/smoke-test.ps1). Returns the tail of the output.")]
    public async Task<string> RunApp()
        => await RunScript("scripts/smoke-test.ps1", "");

    async Task<string> RunScript(string relScript, string args)
    {
        var script = Path.Combine(Root, relScript.Replace('/', Path.DirectorySeparatorChar));
        if (!File.Exists(script)) return $"error: {script} not found (set Buitenzorg.Root in Settings)";
        _log($"[buitenzorg] running {relScript} ...");
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = OperatingSystem.IsWindows() ? "powershell" : "pwsh",
                Arguments = $"-NoProfile -ExecutionPolicy Bypass -File \"{script}\" {args}",
                WorkingDirectory = Root,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            using var proc = Process.Start(psi)!;
            var sb = new StringBuilder();
            proc.OutputDataReceived += (_, e) => { if (e.Data != null) { sb.AppendLine(e.Data); _log(e.Data); } };
            proc.ErrorDataReceived += (_, e) => { if (e.Data != null) { sb.AppendLine(e.Data); _log(e.Data); } };
            proc.BeginOutputReadLine();
            proc.BeginErrorReadLine();
            await proc.WaitForExitAsync();
            var outp = sb.ToString();
            return outp.Length > 1500 ? outp[^1500..] : outp;
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }
}
