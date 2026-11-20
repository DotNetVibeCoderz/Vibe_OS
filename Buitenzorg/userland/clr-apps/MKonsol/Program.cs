// Konsol -- the standalone terminal app (Buitenzorg.UI): tabs, each with its
// own working folder and history; scrollback; ANSI colours; copy (drag over
// lines, Ctrl+C) and paste (Ctrl+V); two font sizes (F2); Tab completion of
// file names; jobs started with `run` listed by `jobs`, stopped by `kill %N`.
//
// Commands run in the system shell through SHELL_RUN, with the user's rights
// (pipes, redirection, scripts, coreutils, `bz` -- everything the kernel's
// Terminal has). Each line is sent as `cd "<tab folder>"; <line>; echo "@@ $?
// $PWD"`, so every tab keeps its folder and `$?` although the shell state
// behind SHELL_RUN is shared. A command runs on a background thread; it cannot
// be interrupted (the shell runs it to the end) -- programs started with `run`
// keep running on their own and are listed by `jobs`.

using System.Text;
using System.Threading;
using Buitenzorg.App;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;
using Window = Buitenzorg.UI.Window;

namespace MKonsol;

public struct Run
{
    public string Text;
    public int Color;
    public Run(string text, int color) { Text = text; Color = color; }
}

public sealed class TLine
{
    public readonly List<Run> Runs = new List<Run>();
    public string Plain
    {
        get { var sb = new StringBuilder(); foreach (var r in Runs) sb.Append(r.Text); return sb.ToString(); }
    }
}

public sealed class Job
{
    public int Number;
    public long Pid;
    public string Command = "";
}

public sealed class Tab
{
    public string Cwd = "/disk";
    public readonly List<TLine> Lines = new List<TLine>();
    public readonly List<string> History = new List<string>();
    public int HistoryAt = -1;
    public string Input = "";
    public int Caret;
    public int Scroll; // lines scrolled back from the bottom
    public bool Busy;
    public int LastStatus;
    public readonly List<Job> Jobs = new List<Job>();
    public int NextJob = 1;
    public string Title
    {
        get
        {
            string t = Cwd.TrimEnd('/');
            int s = t.LastIndexOf('/');
            return s >= 0 && s + 1 < t.Length ? t.Substring(s + 1) : t.Length == 0 ? "/" : t;
        }
    }
}

public sealed class KonsolView : Control
{
    public const int Background = 0x161B18, Foreground = 0xD8E2DA, PromptColor = 0x7CC4A4, ErrorColor = 0xE5735F, StripColor = 0x222A25;
    /// <summary>ANSI SGR 30..37 and 90..97 (the kernel Terminal's palette).</summary>
    public static readonly int[] Ansi =
    {
        0x555E58, 0xE5735F, 0x8BC97A, 0xE9C46A, 0x6FA8DC, 0xC792EA, 0x6CC4C4, 0xD8E2DA,
        0x7A847E, 0xFF8A75, 0xA9E59A, 0xFFDB7F, 0x8FC2F2, 0xDDB2F7, 0x8FE0E0, 0xFFFFFF,
    };
    public const int StripH = 28;

    readonly KonsolApp _app;
    public int Style; // 0 = 16 px, 2 = 24 px (F2)
    int _selA = -1, _selB = -1;
    bool _dragging;
    long _lastKey;
    bool _caretOn = true;
    DispatcherTimer? _blink;

    public KonsolView(KonsolApp app) { _app = app; Focusable = true; AcceptsTab = true; }

    int Gw => Fonts.GlyphWidth(Style);
    int Lh => Fonts.LineHeight(Style) + 2;
    Tab T => _app.Current;

    protected override Size MeasureOverride(Size a) => new Size(Math.Min(a.Width, 640), Math.Min(a.Height, 400));

    public int VisibleLines => Math.Max(1, (Bounds.Height - StripH - 8) / Lh);

    /// <summary>Text with ANSI SGR colour codes as coloured runs.</summary>
    public static List<Run> ParseAnsi(string s)
    {
        var runs = new List<Run>();
        int color = Foreground;
        var cur = new StringBuilder();
        for (int i = 0; i < s.Length; i++)
        {
            char ch = s[i];
            if (ch == (char)27 && i + 1 < s.Length && s[i + 1] == '[')
            {
                int end = i + 2;
                while (end < s.Length && (char.IsDigit(s[end]) || s[end] == ';')) end++;
                if (end < s.Length && s[end] == 'm')
                {
                    if (cur.Length > 0) { runs.Add(new Run(cur.ToString(), color)); cur.Clear(); }
                    foreach (var p in s.Substring(i + 2, end - i - 2).Split(';'))
                    {
                        int.TryParse(p, out int n);
                        if (n == 0 || n == 39) color = Foreground;
                        else if (n >= 30 && n <= 37) color = Ansi[n - 30];
                        else if (n >= 90 && n <= 97) color = Ansi[n - 90 + 8];
                    }
                }
                i = end;
                continue;
            }
            if (ch == '\t') { cur.Append("    "); continue; }
            if (ch == '\r') continue;
            cur.Append(ch);
        }
        if (cur.Length > 0 || runs.Count == 0) runs.Add(new Run(cur.ToString(), color));
        return runs;
    }

    // ---- input ----------------------------------------------------------------------

    protected override void OnKeyDown(KeyEventArgs e)
    {
        _lastKey = Clock.Now;
        _caretOn = true;
        _blink?.Start();
        if (_app.Key(e.Key)) { e.Handled = true; InvalidateVisual(); return; }
        base.OnKeyDown(e);
    }

    /// <summary>The content line under a y coordinate (Lines index; Lines.Count = the prompt).</summary>
    int LineAt(int y)
    {
        int total = T.Lines.Count + 1;
        int first = Math.Max(0, total - VisibleLines - T.Scroll);
        return Math.Max(0, Math.Min(total - 1, first + (y - Bounds.Y - StripH - 4) / Lh));
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        var r = Bounds;
        if (e.Y < r.Y + StripH)
        {
            // the tab strip: a tab, or "+"
            int x = r.X + 6;
            for (int i = 0; i < _app.Tabs.Count; i++)
            {
                int w = TabWidth(i);
                if (e.X >= x && e.X < x + w) { _app.Select(i); e.Handled = true; InvalidateVisual(); return; }
                x += w + 4;
            }
            if (e.X >= x && e.X < x + 24) { _app.NewTab(); e.Handled = true; InvalidateVisual(); }
            return;
        }
        _selA = _selB = LineAt(e.Y);
        _dragging = true;
        e.Handled = true;
        InvalidateVisual();
        base.OnPointerPressed(e);
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        if (_dragging && e.LeftButton)
        {
            int l = LineAt(e.Y);
            if (l != _selB) { _selB = l; InvalidateVisual(); }
            e.Handled = true;
        }
        base.OnPointerMoved(e);
    }

    protected override void OnPointerReleased(PointerEventArgs e)
    {
        _dragging = false;
        base.OnPointerReleased(e);
    }

    protected override void OnPointerWheelChanged(PointerEventArgs e)
    {
        _app.ScrollBy(e.WheelDelta * 3);
        e.Handled = true;
        InvalidateVisual();
        base.OnPointerWheelChanged(e);
    }

    public bool HasSelection => _selA >= 0;

    /// <summary>Select content lines a..b (tests; a drag does the same).</summary>
    public void SelectLines(int a, int b) { _selA = a; _selB = b; InvalidateVisual(); }
    public void ClearSelection() { _selA = _selB = -1; InvalidateVisual(); }

    /// <summary>The selected lines' text (the prompt line counts as its text).</summary>
    public string SelectedText()
    {
        if (_selA < 0) return "";
        int a = Math.Min(_selA, _selB), b = Math.Max(_selA, _selB);
        var sb = new StringBuilder();
        for (int i = a; i <= b; i++)
        {
            if (sb.Length > 0) sb.Append('\n');
            sb.Append(i < T.Lines.Count ? T.Lines[i].Plain : _app.PromptText() + T.Input);
        }
        return sb.ToString();
    }

    protected override void OnGotFocus()
    {
        base.OnGotFocus();
        _blink ??= new DispatcherTimer(530);
        _blink.Tick += Blink;
        _blink.Start();
        _lastKey = Clock.Now;
    }

    protected override void OnLostFocus()
    {
        base.OnLostFocus();
        if (_blink != null) { _blink.Tick -= Blink; _blink.Stop(); }
        InvalidateVisual();
    }

    // an idle caret stops blinking (each blink recomposes the desktop)
    void Blink()
    {
        if (Clock.Now - _lastKey > 10000) { _caretOn = true; _blink?.Stop(); }
        else _caretOn = !_caretOn;
        InvalidateVisual();
    }

    // ---- drawing --------------------------------------------------------------------

    int TabWidth(int i) => Math.Min(160, Fonts.Measure((i + 1) + ": " + _app.Tabs[i].Title, 0) + 20);

    public override void Render(DrawingContext dc)
    {
        var r = Bounds;
        dc.FillRect(r, Background);
        // tab strip
        dc.FillRect(new Rect(r.X, r.Y, r.Width, StripH), StripColor);
        int x = r.X + 6;
        for (int i = 0; i < _app.Tabs.Count; i++)
        {
            int w = TabWidth(i);
            bool cur = i == _app.CurrentIndex;
            dc.FillRoundRect(new Rect(x, r.Y + 4, w, StripH - 6), 6, cur ? 0x2F3A33 : StripColor);
            string label = (i + 1) + ": " + _app.Tabs[i].Title + (_app.Tabs[i].Busy ? " *" : "");
            dc.Text(x + 10, r.Y + 8, label, cur ? Foreground : 0x8A968E, 0);
            x += w + 4;
        }
        dc.Text(x + 6, r.Y + 8, "+", 0x8A968E, 1);
        // lines + prompt, bottom-aligned
        int gw = Gw, lh = Lh;
        int total = T.Lines.Count + 1;
        int vis = VisibleLines;
        int first = Math.Max(0, total - vis - T.Scroll);
        int y = r.Y + StripH + 4;
        int a = Math.Min(_selA, _selB), b = Math.Max(_selA, _selB);
        dc.PushClip(new Rect(r.X, r.Y + StripH, r.Width, r.Height - StripH));
        for (int i = first; i < total && i < first + vis; i++, y += lh)
        {
            if (_selA >= 0 && i >= a && i <= b) dc.FillRect(new Rect(r.X + 2, y - 1, r.Width - 4, lh), 0x33463A);
            int cx = r.X + 8;
            if (i < T.Lines.Count)
            {
                foreach (var run in T.Lines[i].Runs)
                {
                    if (run.Text.Length > 0) dc.MonoText(cx, y, run.Text, run.Color, Style);
                    cx += run.Text.Length * gw;
                }
            }
            else
            {
                string p = _app.PromptText();
                dc.MonoText(cx, y, p, PromptColor, Style);
                cx += p.Length * gw;
                if (T.Busy) dc.MonoText(cx, y, "(berjalan ...)", 0x8A968E, Style);
                else
                {
                    dc.MonoText(cx, y, T.Input, Foreground, Style);
                    if (IsFocused && _caretOn) dc.FillRect(new Rect(cx + T.Caret * gw, y, 2, lh - 2), PromptColor);
                }
            }
        }
        dc.PopClip();
        if (T.Scroll > 0)
            dc.Text(r.X + r.Width - 8 - Fonts.Measure("-- " + T.Scroll + " baris di bawah --", 0), r.Y + r.Height - 20, "-- " + T.Scroll + " baris di bawah --", 0x8A968E, 0);
    }
}

public sealed class KonsolApp
{
    public const int MaxLines = 2000;
    public Window W { get; }
    public KonsolView View { get; }
    public readonly List<Tab> Tabs = new List<Tab>();
    public int CurrentIndex { get; private set; }
    public Tab Current => Tabs[CurrentIndex];
    /// <summary>Tests: run commands on the calling thread.</summary>
    public bool Synchronous;
    /// <summary>What runs a command line (the system shell; tests replace it).</summary>
    public Func<string, ShellResult> Runner = Shell.Run;
    readonly List<string> _sharedHistory = new List<string>();

    public KonsolApp(bool headless)
    {
        W = new Window("Konsol", 760, 460, headless);
        View = new KonsolView(this);
        W.Content = View;
        W.UnhandledKey += e => { if (Key(e.Key)) { e.Handled = true; View.InvalidateVisual(); } };
        // the kernel Terminal's history as a start
        foreach (var l in Files.ReadAllText("/disk/HISTORY.TXT").Split('\n'))
            if (l.Trim().Length > 0) _sharedHistory.Add(l.Trim());
        NewTab();
        Application.Current?.Apply(W);
        W.UpdateLayout();
        View.Focus();
    }

    public void NewTab()
    {
        var t = new Tab();
        t.History.AddRange(_sharedHistory);
        if (Tabs.Count > 0) t.Cwd = Current.Cwd;
        Tabs.Add(t);
        CurrentIndex = Tabs.Count - 1;
        if (Tabs.Count == 1) Print("Konsol -- shell Buitenzorg. Ctrl+T tab baru, Ctrl+W tutup, Ctrl+N/P pindah tab, F2 ukuran huruf, 'jobs'.", 0x8A968E);
    }

    public void Select(int i) { if (i >= 0 && i < Tabs.Count) CurrentIndex = i; }

    public void CloseTab()
    {
        if (Tabs.Count == 1) { Current.Lines.Clear(); return; }
        Tabs.RemoveAt(CurrentIndex);
        if (CurrentIndex >= Tabs.Count) CurrentIndex = Tabs.Count - 1;
    }

    public string PromptText() => Current.Cwd + " $ ";

    public void ScrollBy(int lines)
    {
        int max = Math.Max(0, Current.Lines.Count + 1 - View.VisibleLines);
        Current.Scroll = Math.Max(0, Math.Min(max, Current.Scroll + lines));
    }

    void Add(Tab t, TLine l)
    {
        t.Lines.Add(l);
        if (t.Lines.Count > MaxLines) t.Lines.RemoveRange(0, t.Lines.Count - MaxLines);
    }

    public void Print(string text, int color = -1)
    {
        foreach (var l in text.Split('\n'))
        {
            var line = new TLine();
            if (color >= 0) line.Runs.Add(new Run(l, color));
            else line.Runs.AddRange(KonsolView.ParseAnsi(l));
            Add(Current, line);
        }
    }

    /// <summary>A key for the terminal; false = not ours.</summary>
    public bool Key(char k)
    {
        var t = Current;
        switch (k)
        {
            case (char)20: NewTab(); return true;                       // Ctrl+T
            case (char)23: CloseTab(); return true;                     // Ctrl+W
            case (char)14: Select((CurrentIndex + 1) % Tabs.Count); return true;               // Ctrl+N
            case (char)16: Select((CurrentIndex + Tabs.Count - 1) % Tabs.Count); return true;  // Ctrl+P
            case (char)12: t.Lines.Clear(); t.Scroll = 0; return true;  // Ctrl+L
            case Keys.F2: View.Style = View.Style == 0 ? 2 : 0; return true;
            case Keys.PageUp: ScrollBy(View.VisibleLines - 1); return true;
            case Keys.PageDown: ScrollBy(-(View.VisibleLines - 1)); return true;
        }
        if (k == Keys.CtrlC)
        {
            if (View.HasSelection) { Clipboard.SetText(View.SelectedText()); View.ClearSelection(); }
            else if (!t.Busy) { Print(PromptText() + t.Input + "^C", 0x8A968E); t.Input = ""; t.Caret = 0; }
            return true;
        }
        if (t.Busy) return k != Keys.Tab; // typed while a command runs: dropped
        if (k == Keys.CtrlV)
        {
            string p = Clipboard.GetText().Replace("\r", "").Replace("\n", "; ").TrimEnd(' ', ';');
            t.Input = t.Input.Insert(t.Caret, p);
            t.Caret += p.Length;
            return true;
        }
        t.Scroll = 0;
        View.ClearSelection();
        switch (k)
        {
            case Keys.Enter: Submit(); return true;
            case Keys.Backspace: case Keys.Rubout:
                if (t.Caret > 0) { t.Input = t.Input.Remove(t.Caret - 1, 1); t.Caret--; }
                return true;
            case Keys.Delete: if (t.Caret < t.Input.Length) t.Input = t.Input.Remove(t.Caret, 1); return true;
            case Keys.Left: if (t.Caret > 0) t.Caret--; return true;
            case Keys.Right: if (t.Caret < t.Input.Length) t.Caret++; return true;
            case Keys.Home: case Keys.CtrlA: t.Caret = 0; return true;
            case Keys.End: case (char)5: t.Caret = t.Input.Length; return true;
            case (char)21: t.Input = t.Input.Substring(t.Caret); t.Caret = 0; return true; // Ctrl+U
            case Keys.Up: History(-1); return true;
            case Keys.Down: History(1); return true;
            case Keys.Tab: Complete(); return true;
        }
        if (Keys.IsPrintable(k))
        {
            t.Input = t.Input.Insert(t.Caret, k.ToString());
            t.Caret++;
            return true;
        }
        return false;
    }

    void History(int d)
    {
        var t = Current;
        if (t.History.Count == 0) return;
        int at = t.HistoryAt < 0 ? t.History.Count : t.HistoryAt;
        at = Math.Max(0, Math.Min(t.History.Count, at + d));
        t.HistoryAt = at;
        t.Input = at == t.History.Count ? "" : t.History[at];
        t.Caret = t.Input.Length;
    }

    /// <summary>Tab: complete the last word as a file name (in its folder, or the tab's).</summary>
    void Complete()
    {
        var t = Current;
        string before = t.Input.Substring(0, t.Caret);
        int sp = before.LastIndexOf(' ') + 1;
        string word = before.Substring(sp);
        int slash = word.LastIndexOf('/');
        string dir = slash >= 0 ? (word.StartsWith("/") ? (slash == 0 ? "/" : word.Substring(0, slash)) : t.Cwd + "/" + word.Substring(0, slash)) : t.Cwd;
        string stem = word.Substring(slash + 1).ToUpperInvariant();
        var hits = new List<string>();
        try { foreach (var f in Files.List(dir)) if (f.Name.ToUpperInvariant().StartsWith(stem)) hits.Add(f.Name + (f.IsDirectory ? "/" : "")); }
        catch (Exception) { }
        if (hits.Count == 1)
        {
            string add = hits[0].Substring(stem.Length);
            t.Input = t.Input.Insert(t.Caret, add);
            t.Caret += add.Length;
        }
        else if (hits.Count > 1)
        {
            Print(PromptText() + t.Input, KonsolView.PromptColor);
            Print(string.Join("  ", hits), 0x8A968E);
        }
    }

    /// <summary>Run the input line (Enter).</summary>
    public void Submit()
    {
        var t = Current;
        string line = t.Input.Trim();
        Print(PromptText() + t.Input, KonsolView.PromptColor);
        t.Input = "";
        t.Caret = 0;
        t.HistoryAt = -1;
        if (line.Length == 0) return;
        if (t.History.Count == 0 || t.History[t.History.Count - 1] != line) t.History.Add(line);
        if (Builtin(t, line)) return;
        string wrapped = "cd \"" + t.Cwd + "\"; " + line + "; echo \"@@ $? $PWD\"";
        t.Busy = true;
        if (Synchronous) { Finish(t, Runner(wrapped)); return; }
        new Thread(() =>
        {
            ShellResult r;
            try { r = Runner(wrapped); } catch (Exception e) { r = new ShellResult(false, "konsol: " + e.Message + "\n"); }
            Dispatcher.Post(() => { Finish(t, r); View.InvalidateVisual(); });
        }).Start();
    }

    /// <summary>Konsol's own commands: clear, exit, jobs, kill %N.</summary>
    bool Builtin(Tab t, string line)
    {
        var w = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        switch (w[0])
        {
            case "clear": case "cls": t.Lines.Clear(); return true;
            case "exit": CloseTab(); return true;
            case "jobs":
                if (t.Jobs.Count == 0) Print("(tidak ada job -- program yang dimulai dengan 'run' tercatat di sini)", 0x8A968E);
                var alive = new HashSet<long>();
                try { foreach (var p in Processes.List()) alive.Add(p.Pid); } catch (Exception) { }
                foreach (var j in t.Jobs)
                    Print("[" + j.Number + "] pid " + j.Pid + "  " + (alive.Contains(j.Pid) ? "berjalan" : "selesai ") + "  " + j.Command);
                return true;
            case "kill" when w.Length == 2 && w[1].StartsWith("%") && int.TryParse(w[1].Substring(1), out int n):
                var job = t.Jobs.Find(j => j.Number == n);
                if (job == null) { Print("kill: tidak ada job %" + n, KonsolView.ErrorColor); return true; }
                var r = Runner("kill " + job.Pid);
                foreach (var l in r.Lines) Print(l);
                return true;
        }
        return false;
    }

    /// <summary>A finished command: its output, its status, the tab's new folder.</summary>
    public void Finish(Tab t, ShellResult r)
    {
        t.Busy = false;
        var lines = new List<string>(r.Lines);
        while (lines.Count > 0 && lines[lines.Count - 1].Length == 0) lines.RemoveAt(lines.Count - 1);
        int status = r.Ok ? 0 : 1;
        int marker = -1;
        for (int i = lines.Count - 1; i >= 0; i--) if (lines[i].StartsWith("@@ ")) { marker = i; break; }
        if (marker >= 0)
        {
            string m = lines[marker].Substring(3);
            int sp = m.IndexOf(' ');
            if (int.TryParse(sp < 0 ? m : m.Substring(0, sp), out int st)) status = st;
            if (sp > 0 && m.Substring(sp + 1).StartsWith("/")) t.Cwd = m.Substring(sp + 1).Trim();
            lines.RemoveAt(marker);
        }
        var prev = Current;
        int idx = Tabs.IndexOf(t);
        if (idx >= 0) CurrentIndex = idx;
        foreach (var l in lines)
        {
            Print(l);
            // `run X`: a job of this tab
            int p = l.IndexOf("(pid ");
            if (p >= 0 && l.EndsWith(")") && long.TryParse(l.Substring(p + 5, l.Length - p - 6), out long pid))
                t.Jobs.Add(new Job { Number = t.NextJob++, Pid = pid, Command = l });
        }
        if (status != 0) Print("[status " + status + "]", KonsolView.ErrorColor);
        t.LastStatus = status;
        int back = Tabs.IndexOf(prev);
        if (back >= 0) CurrentIndex = back;
    }

    // ---- self-test ------------------------------------------------------------------

    /// <summary>Off the device the system shell is a simulation: a tiny one that
    /// knows `cd`, `echo` (with $? and $PWD) and `false` answers instead.</summary>
    static ShellResult FakeShell(string wrapped)
    {
        string cwd = "/disk";
        int status = 0;
        var o = new StringBuilder();
        foreach (var raw in wrapped.Replace("; ", "\u0001").Split('\u0001'))
        {
            string c = raw.Trim();
            if (c.StartsWith("cd ")) { string d = c.Substring(3).Trim('"'); if (d.StartsWith("/")) { cwd = d; status = 0; } else status = 1; }
            else if (c == "false") status = 1;
            else if (c.StartsWith("echo ")) { o.Append(c.Substring(5).Trim('"').Replace("$?", status.ToString()).Replace("$PWD", cwd)).Append('\n'); status = 0; }
            else if (c.StartsWith("run ")) { o.Append("app '" + c.Substring(4) + "' berjalan (pid 4242)\n"); status = 0; }
            else { o.Append("sh: " + c + ": tidak dikenal\n"); status = 1; }
        }
        return new ShellResult(true, o.ToString());
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Konsol");
        var app = new KonsolApp(true) { Synchronous = true };
        bool device = Shell.Run("echo $((40+2))").Output.Trim() == "42";
        if (!device) app.Runner = FakeShell;
        string All()
        {
            var sb = new StringBuilder();
            foreach (var l in app.Current.Lines) sb.Append(l.Plain).Append('\n');
            return sb.ToString();
        }

        app.W.SimulateKeys("echo halo konsol\n");
        c.That(All().Contains("halo konsol") && !All().Contains("@@ "), "a command runs, the status marker is hidden");
        app.W.SimulateKeys("false\n");
        c.That(app.Current.LastStatus != 0 && All().Contains("[status 1]"), "a failed command shows its status");

        // tabs keep their own folder
        string dir = Files.ScratchDirectory + "/KONSOLT";
        if (Files.GetInfo(dir) == null) Files.CreateDirectory(dir);
        app.W.SimulateKeys("cd " + dir + "\n");
        c.That(app.Current.Cwd == dir && app.PromptText().StartsWith(dir), "cd changes this tab's folder (" + app.Current.Cwd + ")");
        app.W.SimulateKeys(((char)20).ToString()); // Ctrl+T
        app.W.SimulateKeys("cd /disk\n");
        c.That(app.Tabs.Count == 2 && app.Current.Cwd == "/disk" && app.Tabs[0].Cwd == dir, "a second tab with its own folder");
        if (device)
        {
            app.W.SimulateKeys("echo $PWD | wc -c\n");
            c.That(All().Contains("6"), "pipes run in the tab's folder (/disk)");
        }
        app.W.SimulateKeys(((char)16).ToString()); // Ctrl+P
        c.That(app.CurrentIndex == 0 && app.Current.Cwd == dir, "Ctrl+P back to the first tab, its folder kept");

        // history, editing
        app.W.SimulateKeys(Keys.Up.ToString());
        c.That(app.Current.Input.StartsWith("cd "), "Up recalls the last command (" + app.Current.Input + ")");
        app.W.SimulateKeys(((char)21).ToString() + "eXcho" + Keys.Home + Keys.Right + Keys.Delete + Keys.End + " ok");
        c.That(app.Current.Input == "echo ok", "line editing: caret, delete (" + app.Current.Input + ")");
        app.W.SimulateKeys("\n");

        // ANSI colours
        var runs = KonsolView.ParseAnsi("\u001b[31mmerah\u001b[0m biasa \u001b[92mhijau");
        c.That(runs.Count == 3 && runs[0].Text == "merah" && runs[0].Color == KonsolView.Ansi[1] && runs[1].Color == KonsolView.Foreground && runs[2].Color == KonsolView.Ansi[10],
            "ANSI colour codes become coloured runs");

        // copy a line, paste it into the input
        int idx = app.Current.Lines.FindIndex(l => l.Plain == "ok");
        app.View.SelectLines(idx, idx);
        app.W.SimulateKeys(Keys.CtrlC.ToString());
        c.That(Clipboard.GetText() == "ok", "selected lines copied (Ctrl+C)");
        app.W.SimulateKeys(Keys.CtrlV.ToString());
        c.That(app.Current.Input == "ok", "paste into the input (Ctrl+V)");
        app.W.SimulateKeys(((char)21).ToString());

        // jobs: a `run` line is recorded
        app.Finish(app.Current, new ShellResult(true, "app 'jam' berjalan (pid 4242)\n@@ 0 " + dir + "\n"));
        app.W.SimulateKeys("jobs\n");
        c.That(app.Current.Jobs.Count == 1 && app.Current.Jobs[0].Pid == 4242 && All().Contains("[1] pid 4242"), "jobs lists programs started with run");

        // font size, scrollback, clear
        int lh = Fonts.LineHeight(0);
        app.W.SimulateKeys(Keys.F2.ToString());
        c.That(app.View.Style == 2 && Fonts.LineHeight(2) > lh, "F2: larger font");
        app.W.SimulateKeys(Keys.F2.ToString());
        for (int i = 0; i < 60; i++) app.Print("baris " + i);
        app.W.SimulateKeys(Keys.PageUp.ToString());
        c.That(app.Current.Scroll > 0, "PageUp scrolls back (" + app.Current.Scroll + " lines)");
        app.W.SimulateKeys("clear\n");
        c.That(app.Current.Lines.Count == 0 && app.Current.Scroll == 0, "clear empties the tab");
        app.W.SimulateKeys(((char)23).ToString()); // Ctrl+W
        c.That(app.Tabs.Count == 1, "Ctrl+W closes the tab");
        app.W.RenderFrame();
        Files.Delete(dir);
        return c;
    }

    public static (Window, KonsolApp) Build(bool headless = false)
    {
        var a = new KonsolApp(headless);
        return (a.W, a);
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = KonsolApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = KonsolApp.Build();
        app.Run(w);
        return 0;
    }
}
