// Buitenzorg Studio -- the script debugger (JavaScript, TypeScript, Python,
// C# script).
//
// A "record, then replay" debugger: F6 runs the file once in the kernel's
// interpreter with the debugger on (`script debug LANG FILE LINES|all`, see
// kernel/bzkernel/src/script.rs + shell.rs), which records the program's
// state at every breakpoint line -- or at every line when there are none.
// Studio then lets you walk through those stops in both directions (F10 /
// F11): the editor marks the line, the Debug panel shows the call stack, the
// locals, the globals and the output printed so far.

using Buitenzorg.App;
using Buitenzorg.UI;

namespace MStudio;

public sealed class Var
{
    public string Name { get; }
    public string Value { get; }
    public Var(string name, string value) { Name = name; Value = value; }
    public override string ToString() => Name + " = " + Value;
}

public sealed class Frame
{
    public string Function { get; }
    public int Line { get; }
    public Frame(string function, int line) { Function = function; Line = line; }
}

/// <summary>One recorded stop of a debugged run.</summary>
public sealed class Stop
{
    public int Index { get; }
    public int Line { get; }
    public int OutputLines { get; }
    public List<Frame> Stack { get; } = new List<Frame>();
    public List<Var> Locals { get; } = new List<Var>();
    public List<Var> Globals { get; } = new List<Var>();
    public Stop(int index, int line, int outputLines) { Index = index; Line = line; OutputLines = outputLines; }

    public string Function => Stack.Count > 0 ? Stack[0].Function : "<main>";

    public override string ToString()
    {
        var vars = Locals.Count > 0 ? Locals : Globals;
        string v = string.Join(", ", vars.Take(3).Select(x => x.ToString()));
        return "#" + (Index + 1) + "  baris " + Line + "  " + Function + (v.Length > 0 ? "  (" + v + ")" : "");
    }

    /// <summary>`@dbg LINE<TAB>OUT<TAB>STACK<TAB>LOCALS<TAB>GLOBALS` (shell.rs debug_script).</summary>
    public static Stop? Parse(int index, string record)
    {
        if (!record.StartsWith("@dbg ")) return null;
        var f = record.Substring(5).Split('\t');
        if (f.Length < 5 || !int.TryParse(f[0], out int line)) return null;
        int.TryParse(f[1], out int outLines);
        var s = new Stop(index, line, outLines);
        foreach (var fr in f[2].Split(',', StringSplitOptions.RemoveEmptyEntries))
        {
            int c = fr.LastIndexOf(':');
            if (c > 0 && int.TryParse(fr.Substring(c + 1), out int l)) s.Stack.Add(new Frame(fr.Substring(0, c), l));
        }
        Vars(f[3], s.Locals);
        Vars(f[4], s.Globals);
        return s;
    }

    static void Vars(string text, List<Var> into)
    {
        foreach (var kv in text.Split('\u001f', StringSplitOptions.RemoveEmptyEntries))
        {
            int eq = kv.IndexOf('=');
            if (eq > 0) into.Add(new Var(kv.Substring(0, eq), kv.Substring(eq + 1)));
        }
    }
}

public sealed partial class Studio
{
    public ObservableCollection<Stop> Stops { get; } = new ObservableCollection<Stop>();
    public int StopIndex { get; private set; } = -1;
    public string DebugText => _debugView.Text;
    string _debugFile = "";
    readonly List<string> _debugOutput = new List<string>();
    ListBox _stopList = null!;
    readonly TextBox _debugView = new TextBox { AcceptsReturn = true, IsReadOnly = true, BorderThickness = 0, CornerRadius = 0 };
    readonly TextBlock _debugSummary = new TextBlock { Margin = new Thickness(8, 4), TextTrimming = TextTrimming.CharacterEllipsis };

    /// <summary>The Debug tab of the bottom panel.</summary>
    Control BuildDebugPanel()
    {
        _stopList = new ListBox { ItemsSource = Stops, BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0, Width = 300 };
        _stopList.SelectionChanged += l =>
        {
            if (_syncing || l.SelectedIndex < 0) return;
            ShowStop(l.SelectedIndex);
        };
        var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, Margin = new Thickness(4, 2) };
        bar.Children.Add(new Button(new TextBlock("< Sebelumnya (F11)"), _ => PrevStop()).WithClass("subtle"));
        bar.Children.Add(new Button(new TextBlock("Berikutnya (F10) >"), _ => NextStop()).WithClass("subtle"));
        bar.Children.Add(new Button(new TextBlock("Selesai"), _ => EndDebug()).WithClass("subtle"));
        var top = new DockPanel();
        top.Children.Add(bar.DockTo(Dock.Left));
        top.Children.Add(_debugSummary.WithClass("muted"));
        var panel = new DockPanel { Height = 176 }; // taller than the other tabs: stack + locals + globals
        panel.Children.Add(top.DockTo(Dock.Top));
        panel.Children.Add(_stopList.DockTo(Dock.Left));
        panel.Children.Add(_debugView);
        _debugSummary.Text = "F9: breakpoint di baris kursor (atau klik nomor baris), F6: debug";
        return panel;
    }

    /// <summary>The shell command that debugs a file, or null (JS / TS / Python / C# script only).</summary>
    public static string? DebugCommand(string path, IEnumerable<int> breakpoints)
    {
        string lang = Syntax.LangOf(path);
        if (lang != "js" && lang != "ts" && lang != "py" && lang != "csx") return null;
        var lines = breakpoints.OrderBy(x => x).ToList();
        return "script debug " + lang + " " + path + " " + (lines.Count == 0 ? "all" : string.Join(",", lines));
    }

    /// <summary>F9: breakpoint on the caret's line.</summary>
    public void ToggleBreakpoint() => Editor.ToggleBreakpoint(Editor.CaretLine);

    /// <summary>F6: run the current file under the debugger.</summary>
    public ShellResult? Debug()
    {
        if (Current == null) { Log("Tidak ada berkas untuk di-debug."); return null; }
        string? cmd = DebugCommand(Current.Path, Current.Breakpoints);
        if (cmd == null)
        {
            Log("Debugger skrip: JavaScript, TypeScript, Python dan C# script (" + Current.Name + " adalah " + Syntax.DisplayName(Current.Lang) + "; program native: 'dbg run' di Terminal).");
            return null;
        }
        SaveAll();
        Log("$ " + cmd);
        var r = Shell.Run(cmd);
        // a UI script runs as an app in its own window while it records; the
        // report comes when it ends (`@dbgui PID FILE`, then status polls)
        string? ui = r.Lines.FirstOrDefault(l => l.StartsWith("@dbgui "));
        if (ui != null) WatchUiDebug(Current.Path, ui);
        else LoadDebug(Current.Path, r.Lines);
        return r;
    }

    DispatcherTimer? _uiDebugTimer;
    /// <summary>The running UI app being recorded (pid), or 0.</summary>
    public long UiDebugPid { get; private set; }
    string _uiDebugStatus = "";
    string _uiDebugPath = "";

    void WatchUiDebug(string path, string record)
    {
        var f = record.Split(' ');
        if (f.Length < 3 || !long.TryParse(f[1], out long pid)) { Log(record); return; }
        EndDebug();
        UiDebugPid = pid;
        _uiDebugPath = path;
        _uiDebugStatus = "script debug status " + f[1] + " " + f[2];
        Log("aplikasi UI (pid " + pid + ") berjalan sambil merekam: pakai jendelanya, lalu tutup -> hentinya diputar ulang di sini");
        _debugSummary.Text = "Merekam aplikasi UI pid " + pid + " - tutup jendelanya untuk memutar ulang";
        Bottom.SelectedIndex = 3;
        _uiDebugTimer?.Stop();
        _uiDebugTimer = new DispatcherTimer(500);
        _uiDebugTimer.Tick += () => { if (PollUiDebug()) _uiDebugTimer?.Stop(); };
        _uiDebugTimer.Start();
    }

    /// <summary>Ask whether the recorded UI app has ended; true once its stops are loaded.</summary>
    public bool PollUiDebug()
    {
        if (UiDebugPid == 0) return true;
        var s = Shell.Run(_uiDebugStatus);
        if (s.Lines.FirstOrDefault()?.StartsWith("@dbgui running") == true) return false;
        UiDebugPid = 0;
        LoadDebug(_uiDebugPath, s.Lines);
        return true;
    }

    /// <summary>Take a debugged run's output: program lines to Output, stops to the Debug panel.</summary>
    public void LoadDebug(string path, IEnumerable<string> lines)
    {
        EndDebug();
        _debugFile = path;
        Problems.Clear();
        foreach (var l in lines)
        {
            var s = Stop.Parse(Stops.Count, l);
            if (s != null) { Stops.Add(s); continue; }
            Log(l);
            ParseProblem(path, l);
            // the program's own output (indented by the shell) for "output so far"
            if (l.StartsWith("  ") && !l.TrimStart().StartsWith("error:")) _debugOutput.Add(l.Substring(2));
        }
        MarkProblems();
        Bottom.SelectedIndex = 3;
        if (Stops.Count == 0)
        {
            _debugSummary.Text = "Tidak ada henti: program tidak melewati breakpoint mana pun";
            _debugView.Text = "";
            return;
        }
        ShowStop(0);
    }

    /// <summary>Show stop `i`: the editor's line marker and the state panel.</summary>
    public void ShowStop(int i)
    {
        if (i < 0 || i >= Stops.Count) return;
        StopIndex = i;
        var s = Stops[i];
        if (Current == null || Current.Path != _debugFile) OpenFile(_debugFile);
        Editor.ExecLine = s.Line;
        Editor.GoToLine(s.Line);
        _syncing = true;
        _stopList.SelectedIndex = i;
        _syncing = false;
        _debugSummary.Text = "Henti " + (i + 1) + " dari " + Stops.Count + "  -  baris " + s.Line + " (" + s.Function + ")";
        var t = new List<string> { "Tumpukan panggilan:" };
        foreach (var f in s.Stack) t.Add("  " + f.Function + "  baris " + f.Line);
        t.Add("Variabel lokal:" + (s.Locals.Count == 0 ? " -" : ""));
        foreach (var v in s.Locals) t.Add("  " + v);
        t.Add("Global:" + (s.Globals.Count == 0 ? " -" : ""));
        foreach (var v in s.Globals) t.Add("  " + v);
        int n = Math.Min(s.OutputLines, _debugOutput.Count);
        t.Add("Keluaran sampai di sini (" + n + " baris):");
        for (int k = Math.Max(0, n - 6); k < n; k++) t.Add("  " + _debugOutput[k]);
        _debugView.Text = string.Join("\n", t);
        UpdateStatus();
    }

    public void NextStop() { if (StopIndex + 1 < Stops.Count) ShowStop(StopIndex + 1); }
    public void PrevStop() { if (StopIndex > 0) ShowStop(StopIndex - 1); }

    public void EndDebug()
    {
        Stops.Clear();
        _debugOutput.Clear();
        StopIndex = -1;
        Editor.ExecLine = 0;
        _debugSummary.Text = "F9: breakpoint di baris kursor (atau klik nomor baris), F6: debug";
        _debugView.Text = "";
    }
}
