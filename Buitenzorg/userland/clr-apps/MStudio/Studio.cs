// Buitenzorg Studio -- an IDE inside the OS: a project explorer, a code editor
// with tabs, syntax colors and find/replace, Run (F5) for JavaScript,
// TypeScript, Python and shell scripts through the kernel's interpreter, an
// Output panel, a Problems panel (parse errors with their line) and a
// terminal. Projects live in /disk/PROYEK (or the scratch folder) and start
// from many templates (Templates.cs). Buitenzorg.UI app on RustCLR.

using Buitenzorg.App;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;
using Window = Buitenzorg.UI.Window;

namespace MStudio;

public sealed class Doc
{
    public string Path { get; }
    public string Text { get; set; }
    public bool Dirty { get; set; }
    public int Line { get; set; } = 1;
    public string Lang { get; }
    public Doc(string path, string text) { Path = path; Text = text; Lang = Syntax.LangOf(path); }
    public string Name => Path.Substring(Path.LastIndexOf('/') + 1);
}

public sealed class Entry
{
    public string Name { get; }
    public string Path { get; }
    public bool IsDir { get; }
    public Entry(string name, string path, bool dir) { Name = name; Path = path; IsDir = dir; }
    public override string ToString() => IsDir ? Name + "/" : Name;
}

public sealed class Problem
{
    public string File { get; }
    public int Line { get; }
    public string Message { get; }
    public Problem(string file, int line, string message) { File = file; Line = line; Message = message; }
    public override string ToString() => File.Substring(File.LastIndexOf('/') + 1) + (Line > 0 ? ":" + Line : "") + "  " + Message;
}

public sealed class Studio
{
    public Window W { get; }
    public CodeEditor Editor { get; } = new CodeEditor();
    public string Root { get; }
    public string Folder { get; private set; }
    public List<Doc> Docs { get; } = new List<Doc>();
    public Doc? Current { get; private set; }
    public ObservableCollection<Entry> Entries { get; } = new ObservableCollection<Entry>();
    public ObservableCollection<Problem> Problems { get; } = new ObservableCollection<Problem>();
    public List<string> Output { get; } = new List<string>();
    public List<string> TerminalLines { get; } = new List<string>();
    public TextBox FindBox { get; } = new TextBox { Watermark = "cari", Width = 150 };
    public TextBox ReplaceBox { get; } = new TextBox { Watermark = "ganti dengan", Width = 150 };
    public TextBox TerminalInput { get; } = new TextBox { Watermark = "perintah shell, Enter untuk menjalankan" };
    public ListBox Explorer { get; }
    public ListBox ProblemList { get; }
    public TabControl Bottom { get; } = new TabControl();
    readonly TextBox _output = new TextBox { AcceptsReturn = true, IsReadOnly = true, BorderThickness = 0, CornerRadius = 0, Height = 128 };
    readonly TextBox _terminal = new TextBox { AcceptsReturn = true, IsReadOnly = true, BorderThickness = 0, CornerRadius = 0, Height = 98 };
    readonly StackPanel _tabs = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2 };
    readonly DockPanel _findBar = new DockPanel { IsVisible = false, Background = Theme.Surface };
    readonly TextBlock _status = new TextBlock { Margin = new Thickness(12, 4), TextTrimming = TextTrimming.CharacterEllipsis };
    readonly TextBlock _folderLabel = new TextBlock { Margin = new Thickness(12, 2), TextTrimming = TextTrimming.CharacterEllipsis };
    readonly SplitView _split;
    readonly WindowNotificationManager? _notes;
    bool _syncing;

    public Studio(string root, bool headless)
    {
        Root = root;
        Folder = root;
        if (Files.GetInfo(root) == null) Files.CreateDirectory(root);
        W = new Window("Buitenzorg Studio", 900, 580, headless);
        _notes = headless ? null : new WindowNotificationManager(W);

        Explorer = new ListBox { ItemsSource = Entries, BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0 };
        Explorer.SelectionChanged += l =>
        {
            if (_syncing || l.SelectedItem is not Entry e) return;
            if (e.IsDir) OpenFolder(e.Path);
            else OpenFile(e.Path);
        };
        ProblemList = new ListBox { ItemsSource = Problems, BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0, Height = 128 };
        ProblemList.SelectionChanged += l =>
        {
            if (l.SelectedItem is Problem p) { OpenFile(p.File); if (p.Line > 0) Editor.GoToLine(p.Line); Editor.Focus(); }
        };

        Editor.TextChanged += _ =>
        {
            if (_syncing || Current == null) return;
            if (!Current.Dirty) { Current.Dirty = true; RefreshTabs(); }
            UpdateStatus();
        };
        Editor.CaretMoved += _ => UpdateStatus();

        // find / replace bar
        var close = new Button(new TextBlock("x"), _ => HideFind()) { Padding = new Thickness(8, 2) }.WithClass("subtle");
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Margin = new Thickness(8, 4) };
        row.Children.Add(FindBox);
        row.Children.Add(new Button(new TextBlock("Berikutnya"), _ => FindNext()).WithClass("subtle"));
        row.Children.Add(ReplaceBox);
        row.Children.Add(new Button(new TextBlock("Ganti"), _ => Editor.ReplaceCurrent(FindBox.Text, ReplaceBox.Text)).WithClass("subtle"));
        row.Children.Add(new Button(new TextBlock("Semua"), _ => ReplaceAll()).WithClass("subtle"));
        _findBar.Children.Add(close.DockTo(Dock.Right));
        _findBar.Children.Add(row);
        FindBox.KeyDown += (_, e) => { if (e.Key == Keys.Enter) { FindNext(); e.Handled = true; } else if (e.Key == Keys.Escape) { HideFind(); e.Handled = true; } };
        TerminalInput.KeyDown += (_, e) => { if (e.Key == Keys.Enter) { TerminalRun(TerminalInput.Text); TerminalInput.Text = ""; e.Handled = true; } };

        // explorer pane
        var pane = new DockPanel();
        pane.Children.Add(new TextBlock("PROYEK") { Margin = new Thickness(12, 10, 0, 2) }.WithClass("eyebrow").DockTo(Dock.Top));
        pane.Children.Add(_folderLabel.WithClass("faint").DockTo(Dock.Top));
        var buttons = new StackPanel { Spacing = 4, Margin = new Thickness(8) };
        buttons.Children.Add(new Button(new TextBlock("+ Proyek baru"), _ => NewProjectDialog()) { HorizontalAlignment = HorizontalAlignment.Stretch });
        buttons.Children.Add(new Button(new TextBlock("+ Berkas baru"), _ => NewFileDialog()) { HorizontalAlignment = HorizontalAlignment.Stretch }.WithClass("subtle"));
        pane.Children.Add(buttons.DockTo(Dock.Bottom));
        pane.Children.Add(Explorer);

        // editor column: tabs, find bar, editor
        var editorCol = new DockPanel();
        var tabsRow = new DockPanel { Background = Theme.Surface };
        tabsRow.Children.Add(new Button(new TextBlock("Jalankan (F5)"), _ => Run()) { Margin = new Thickness(4, 3) }.DockTo(Dock.Right));
        tabsRow.Children.Add(_tabs);
        editorCol.Children.Add(tabsRow.DockTo(Dock.Top));
        editorCol.Children.Add(_findBar.DockTo(Dock.Top));
        editorCol.Children.Add(Editor);

        // bottom panel: output, problems, terminal
        var term = new DockPanel();
        term.Children.Add(TerminalInput.DockTo(Dock.Bottom));
        term.Children.Add(_terminal);
        Bottom.Add("Keluaran", _output).Add("Masalah", ProblemList).Add("Terminal", term);

        var center = new DockPanel();
        center.Children.Add(Bottom.DockTo(Dock.Bottom));
        center.Children.Add(editorCol);
        _split = new SplitView { OpenPaneLength = 190, CompactPaneLength = 0, DisplayMode = SplitViewDisplayMode.CompactInline, PaneBackground = Theme.Surface, Pane = pane, Content = center };

        var menu = new Menu();
        menu.Add(new MenuItem("Berkas")
            .Add(new MenuItem("Proyek baru...", _ => NewProjectDialog(), "Ctrl+N", IconKind.Plus))
            .Add(new MenuItem("Berkas baru...", _ => NewFileDialog(), "", IconKind.File))
            .Add(new MenuItem("Simpan", _ => Save(), "Ctrl+S", IconKind.Save))
            .Add(new MenuItem("Simpan semua", _ => SaveAll(), "", IconKind.Save))
            .Add(MenuItem.Separator())
            .Add(new MenuItem("Tutup tab", _ => CloseCurrent(), "Ctrl+W", IconKind.Close)));
        menu.Add(new MenuItem("Edit")
            .Add(new MenuItem("Urungkan", _ => Editor.Undo(), "Ctrl+Z"))
            .Add(new MenuItem("Ulangi", _ => Editor.Redo(), "Ctrl+Y"))
            .Add(new MenuItem("Duplikat baris", _ => Editor.DuplicateLine(), "Ctrl+D"))
            .Add(new MenuItem("Hapus baris", _ => Editor.DeleteLine(), "Ctrl+K"))
            .Add(MenuItem.Separator())
            .Add(new MenuItem("Cari / ganti", _ => ShowFind(), "Ctrl+F", IconKind.Search))
            .Add(new MenuItem("Ke baris...", _ => GoToLineDialog(), "Ctrl+G")));
        menu.Add(new MenuItem("Jalankan")
            .Add(new MenuItem("Jalankan berkas", _ => Run(), "F5", IconKind.Play))
            .Add(new MenuItem("Terminal", _ => { Bottom.SelectedIndex = 2; TerminalInput.Focus(); }, "")));
        menu.Add(new MenuItem("Tampilan")
            .Add(new MenuItem("Panel proyek", _ => _split.IsPaneOpen = !_split.IsPaneOpen, "Ctrl+B", IconKind.Menu))
            .Add(new MenuItem("Tema terang / gelap", _ => ToggleTheme(), "Ctrl+T", IconKind.Sun)));

        var status = new DockPanel { Background = Theme.Surface };
        status.Children.Add(_status.WithClass("muted"));

        var root2 = new DockPanel();
        root2.Children.Add(menu.DockTo(Dock.Top));
        root2.Children.Add(status.DockTo(Dock.Bottom));
        root2.Children.Add(_split);
        W.Content = root2;
        W.UnhandledKey += OnKey;

        OpenFolder(root);
        Application.Current?.Apply(W);
        W.UpdateLayout();
        Editor.Focus();
        UpdateStatus();
    }

    void OnKey(KeyEventArgs e)
    {
        switch (e.Key)
        {
            case (char)19: Save(); break;                 // Ctrl+S
            case (char)14: NewProjectDialog(); break;     // Ctrl+N
            case (char)23: CloseCurrent(); break;         // Ctrl+W
            case (char)6: ShowFind(); break;              // Ctrl+F
            case (char)18: ShowFind(); ReplaceBox.Focus(); break; // Ctrl+R
            case (char)7: GoToLineDialog(); break;        // Ctrl+G
            case (char)2: _split.IsPaneOpen = !_split.IsPaneOpen; break; // Ctrl+B
            case (char)20: ToggleTheme(); break;          // Ctrl+T
            case Keys.F5: Run(); break;
            case Keys.Escape: HideFind(); break;
            default: return;
        }
        e.Handled = true;
    }

    static void ToggleTheme()
    {
        var app = Application.Current;
        app?.SetTheme(app.ThemeVariant == ThemeVariant.Dark ? ThemeVariant.Light : ThemeVariant.Dark);
    }

    void Toast(string title, string text, NotificationType kind) => _notes?.Show(new Notification(title, text, kind, 2500));

    // ---- explorer ---------------------------------------------------------------------

    public void OpenFolder(string path)
    {
        Folder = path.TrimEnd('/');
        var list = new List<Entry>();
        if (Folder != Root && Folder.StartsWith(Root)) list.Add(new Entry("..", Folder.Substring(0, Folder.LastIndexOf('/')), true));
        var dirs = new List<Entry>();
        var files = new List<Entry>();
        foreach (var f in Files.List(Folder))
            (f.IsDirectory ? dirs : files).Add(new Entry(f.Name, Folder + "/" + f.Name, f.IsDirectory));
        dirs.Sort((a, b) => string.CompareOrdinal(a.Name, b.Name));
        files.Sort((a, b) => string.CompareOrdinal(a.Name, b.Name));
        list.AddRange(dirs);
        list.AddRange(files);
        _syncing = true;
        Entries.ResetWith(list);
        Explorer.SelectedIndex = -1;
        _syncing = false;
        _folderLabel.Text = Folder;
    }

    // ---- documents ----------------------------------------------------------------------

    void StoreEditor()
    {
        if (Current == null) return;
        Current.Text = Editor.Text;
        Current.Line = Editor.CaretLine;
    }

    public Doc OpenFile(string path)
    {
        foreach (var d in Docs)
            if (d.Path == path) { Show(d); return d; }
        var doc = new Doc(path, Files.ReadAllText(path));
        Docs.Add(doc);
        Show(doc);
        return doc;
    }

    void Show(Doc d)
    {
        if (Current == d) return;
        StoreEditor();
        Current = d;
        _syncing = true;
        Editor.Language = d.Lang;
        Editor.Text = d.Text;
        Editor.GoToLine(d.Line);
        MarkProblems();
        _syncing = false;
        RefreshTabs();
        UpdateStatus();
    }

    public bool Save()
    {
        if (Current == null) return false;
        StoreEditor();
        bool ok = Files.WriteAllText(Current.Path, Current.Text);
        if (ok) Current.Dirty = false;
        RefreshTabs();
        UpdateStatus();
        if (!ok) Toast("Tidak tersimpan", Current.Path, NotificationType.Error);
        return ok;
    }

    public int SaveAll()
    {
        StoreEditor();
        int n = 0;
        foreach (var d in Docs)
            if (d.Dirty && Files.WriteAllText(d.Path, d.Text)) { d.Dirty = false; n++; }
        RefreshTabs();
        UpdateStatus();
        return n;
    }

    public void CloseCurrent()
    {
        if (Current == null) return;
        var d = Current;
        if (d.Dirty) Save();
        int i = Docs.IndexOf(d);
        Docs.Remove(d);
        Current = null;
        if (Docs.Count > 0) Show(Docs[Math.Min(i, Docs.Count - 1)]);
        else { _syncing = true; Editor.Text = ""; Editor.Language = "text"; _syncing = false; RefreshTabs(); UpdateStatus(); }
    }

    void RefreshTabs()
    {
        _tabs.Children.Clear();
        foreach (var d in Docs)
        {
            var doc = d;
            var b = new Button(new TextBlock(d.Name + (d.Dirty ? " *" : "")), _ => Show(doc)) { Padding = new Thickness(10, 3), Margin = new Thickness(0, 3) };
            if (d != Current) b.Classes.Add("subtle");
            _tabs.Children.Add(b);
        }
    }

    void UpdateStatus()
    {
        if (Current == null) { _status.Text = "Buka berkas dari panel proyek, atau buat proyek baru (Ctrl+N)."; return; }
        _status.Text = Current.Name + "  ·  " + Syntax.DisplayName(Current.Lang) + "  ·  Baris " + Editor.CaretLine + ", Kolom " + Editor.CaretColumn
                       + "  ·  " + (Current.Dirty ? "belum disimpan" : "tersimpan") + "  ·  " + Current.Path;
    }

    // ---- projects and files ----------------------------------------------------------------

    /// <summary>An 8-character folder name from a project name.</summary>
    public static string FolderName(string name)
    {
        var sb = new System.Text.StringBuilder();
        foreach (char ch in name.ToUpperInvariant())
            if ((ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9') || ch == '_' || ch == '-') { sb.Append(ch); if (sb.Length == 8) break; }
        return sb.Length == 0 ? "PROYEK" : sb.ToString();
    }

    /// <summary>Create a project from a template; its folder, or null when refused.</summary>
    public string? CreateProject(Template t, string name)
    {
        string dir = Root + "/" + FolderName(name);
        if (Files.GetInfo(dir) != null) { Toast("Sudah ada", dir, NotificationType.Warning); return null; }
        if (!Files.CreateDirectory(dir)) { Toast("Gagal membuat folder", dir, NotificationType.Error); return null; }
        foreach (var (file, text) in t.Files)
            Files.WriteAllText(dir + "/" + file, text.TrimStart('\n'));
        OpenFolder(dir);
        OpenFile(dir + "/" + t.Entry);
        Toast("Proyek dibuat", t.Title + " di " + dir, NotificationType.Success);
        Log("proyek baru: " + dir + " (" + t.Language + " - " + t.Title + ")");
        return dir;
    }

    void NewProjectDialog()
    {
        var name = new TextBox { Watermark = "nama (maks 8 huruf, mis. HALO)", Text = "" };
        var combo = new ComboBox(Templates.Catalog, 0) { DisplayText = o => ((Template)o).ToString(), MinWidth = 340 };
        var about = new TextBlock(Templates.Catalog[0].UseCase + ": " + Templates.Catalog[0].Description) { TextWrapping = TextWrapping.Wrap, MaxWidth = 360 }.WithClass("faint");
        combo.SelectionChanged += c => { if (c.SelectedIndex >= 0) { var t = Templates.Catalog[c.SelectedIndex]; about.Text = t.UseCase + ": " + t.Description; } };
        var body = new StackPanel { Spacing = 8 };
        body.Children.Add(new TextBlock("Template (" + Templates.Catalog.Length + " pilihan, termasuk aplikasi ber-UI)"));
        body.Children.Add(combo);
        body.Children.Add(about);
        body.Children.Add(new TextBlock("Nama proyek"));
        body.Children.Add(name);
        Dialog.Show(W, "Proyek baru", body, "Buat", "Batal", ok =>
        {
            if (!ok || combo.SelectedIndex < 0) return;
            var t = Templates.Catalog[combo.SelectedIndex];
            CreateProject(t, name.Text.Trim().Length > 0 ? name.Text : t.Id);
            Editor.Focus();
        });
        name.Focus();
    }

    void NewFileDialog()
    {
        var name = new TextBox { Watermark = "NAMA.JS / .PY / .TS / .SH / .TXT" };
        var body = new StackPanel { Spacing = 8 };
        body.Children.Add(new TextBlock("Berkas baru di " + Folder));
        body.Children.Add(name);
        Dialog.Show(W, "Berkas baru", body, "Buat", "Batal", ok =>
        {
            string n = name.Text.Trim().ToUpperInvariant();
            if (!ok || n.Length == 0) return;
            if (!Files.IsValidName(n)) { Toast("Nama tidak sah", "pakai nama 8.3, mis. ALAT.JS", NotificationType.Error); return; }
            string path = Folder + "/" + n;
            if (Files.GetInfo(path) == null) Files.WriteAllText(path, "");
            OpenFolder(Folder);
            OpenFile(path);
            Editor.Focus();
        });
        name.Focus();
    }

    // ---- find / go to -------------------------------------------------------------------------

    public void ShowFind() { _findBar.IsVisible = true; FindBox.Focus(); }
    void HideFind() { if (_findBar.IsVisible) { _findBar.IsVisible = false; Editor.Focus(); } }

    public bool FindNext()
    {
        bool found = Editor.FindNext(FindBox.Text);
        if (!found) Toast("Tidak ditemukan", FindBox.Text, NotificationType.Information);
        return found;
    }

    void ReplaceAll()
    {
        int n = Editor.ReplaceAll(FindBox.Text, ReplaceBox.Text);
        Toast("Diganti", n + " kali", NotificationType.Success);
    }

    void GoToLineDialog()
    {
        var box = new TextBox { Watermark = "1.." + Editor.LineCount };
        Dialog.Show(W, "Ke baris", box, "Pergi", "Batal", ok =>
        {
            if (ok && int.TryParse(box.Text.Trim(), out int n)) Editor.GoToLine(n);
            Editor.Focus();
        });
        box.Focus();
    }

    // ---- run, output, problems, terminal --------------------------------------------------------

    void Log(string line)
    {
        Output.Add(line);
        if (Output.Count > 400) Output.RemoveRange(0, Output.Count - 400);
        _output.Text = string.Join("\n", Output);
        _output.CaretIndex = _output.Text.Length;
    }

    /// <summary>A script that opens a window (the UI API of the script host).</summary>
    public static bool IsUiScript(string text) => text.Contains("window(") || text.Contains("Ui.Window(");

    /// <summary>The shell command that runs a file, or null.</summary>
    public static string? RunCommand(string path)
    {
        switch (Syntax.LangOf(path))
        {
            case "js": return "script js " + path;
            case "ts": return "script ts " + path;
            case "py": return "script py " + path;
            case "sh": return "sh " + path;
            case "csx": return "csx " + path;
            default: return null;
        }
    }

    /// <summary>F5: save everything, run the current file, collect problems.</summary>
    public ShellResult? Run()
    {
        if (Current == null) { Log("Tidak ada berkas untuk dijalankan."); return null; }
        SaveAll();
        Bottom.SelectedIndex = 0;
        string? cmd = RunCommand(Current.Path);
        if (cmd == null)
        {
            Log(Current.Lang == "cs"
                ? Current.Name + ": C# dikompilasi di PC (bflat / dotnet) - lihat BUILD.TXT di proyek ini."
                : Current.Name + ": bukan program yang bisa dijalankan (" + Syntax.DisplayName(Current.Lang) + ").");
            return null;
        }
        // a UI script (it opens a window): its own process through the script host
        if (IsUiScript(Current.Text))
        {
            Log("$ run " + Current.Path);
            var started = Shell.Run("run " + Current.Path);
            foreach (var l in started.Lines) Log(l);
            Log(started.Ok ? "aplikasi berjalan di jendelanya sendiri (print() masuk ke log sistem; tutup jendelanya untuk berhenti)" : "gagal memulai aplikasi");
            Problems.Clear();
            MarkProblems();
            UpdateStatus();
            return started;
        }
        Log("$ " + cmd);
        long t0 = Clock.Now;
        var r = Shell.Run(cmd);
        foreach (var l in r.Lines) Log(l);
        Problems.Clear();
        foreach (var l in r.Lines) ParseProblem(Current.Path, l);
        MarkProblems();
        Log((r.Ok && Problems.Count == 0 ? "selesai" : "selesai dengan masalah (" + Math.Max(1, Problems.Count) + ")") + " dalam " + (Clock.Now - t0) + " ms");
        if (Problems.Count > 0) Bottom.SelectedIndex = 1;
        UpdateStatus();
        return r;
    }

    /// <summary>"error: parse: baris N: ..." (script interpreter) or "sh: FILE: ..." lines.</summary>
    void ParseProblem(string file, string line)
    {
        string t = line.Trim();
        int at = t.IndexOf("error:");
        string? msg = null;
        if (at >= 0) msg = t.Substring(at + 6).Trim();
        else if (t.StartsWith("sh: ") || t.StartsWith("script: ")) msg = t;
        if (msg == null) return;
        int ln = 0;
        int b = msg.IndexOf("baris ");
        if (b >= 0)
        {
            int i = b + 6, n = 0;
            while (i < msg.Length && char.IsDigit(msg[i])) { n = n * 10 + (msg[i] - '0'); i++; }
            ln = n;
        }
        Problems.Add(new Problem(file, ln, msg));
    }

    void MarkProblems()
    {
        Editor.ErrorLines.Clear();
        if (Current == null) return;
        foreach (var p in Problems) if (p.File == Current.Path && p.Line > 0) Editor.ErrorLines.Add(p.Line);
        Editor.InvalidateVisual();
    }

    public ShellResult TerminalRun(string line)
    {
        TerminalLines.Add("$ " + line);
        var r = Shell.Run(line);
        TerminalLines.AddRange(r.Lines);
        if (TerminalLines.Count > 400) TerminalLines.RemoveRange(0, TerminalLines.Count - 400);
        _terminal.Text = string.Join("\n", TerminalLines);
        _terminal.CaretIndex = _terminal.Text.Length;
        if (line.Trim().StartsWith("cd ") || line.Contains(">")) OpenFolder(Folder);
        return r;
    }

    // ---- self-test ---------------------------------------------------------------------------------

    public static Checks SelfTest()
    {
        var c = new Checks("Studio");
        // templates: plenty, every file 8.3, the entry among the files
        int langs = Templates.Catalog.Select(t => t.Language).Distinct().Count();
        int ui = Templates.Catalog.Count(t => t.Language.EndsWith(" UI"));
        int csx = Templates.Catalog.Count(t => t.Language.StartsWith("C# script"));
        c.That(Templates.Catalog.Length >= 40 && langs >= 11 && ui >= 11 && csx >= 9, Templates.Catalog.Length + " templates in " + langs + " groups, " + ui + " with a UI, " + csx + " C# script");
        bool names = true;
        foreach (var t in Templates.Catalog)
        {
            bool entry = false;
            foreach (var (f, _) in t.Files) { if (!Files.IsValidName(f)) names = false; if (f == t.Entry) entry = true; }
            if (!entry) names = false;
        }
        c.That(names, "every template file is 8.3 and has its entry");
        // syntax colors
        bool blk = false;
        var spans = Syntax.Line("function fib(n) { // akar", "js", ref blk);
        c.That(spans.Count > 3 && spans[0].Kind == Tk.Keyword && spans[2].Kind == Tk.Func && spans[spans.Count - 1].Kind == Tk.Comment, "JS line: keyword, function, comment");
        blk = false;
        var cs = Syntax.Line("foreach (var x in list) Console.WriteLine($\"{x}\");", Syntax.LangOf("A.CSX"), ref blk);
        c.That(cs[0].Kind == Tk.Keyword && cs.Exists(s => s.Kind == Tk.Type) && cs.Exists(s => s.Kind == Tk.Str), "C# script line: keyword, type, string");
        blk = false;
        var py = Syntax.Line("def f(x): return \"ok\" # c", "py", ref blk);
        c.That(py[0].Kind == Tk.Keyword && py.Exists(s => s.Kind == Tk.Str) && py[py.Count - 1].Kind == Tk.Comment, "Python line: def, string, comment");

        string root = Files.ScratchDirectory + "/STUDIOT";
        var st = new Studio(root, headless: true);
        var tpl = Templates.Find("js-rekursi")!;
        string? dir = st.CreateProject(tpl, "Uji coba");
        c.That(dir != null && dir.EndsWith("/UJICOBA") && Files.ReadAllText(dir + "/MAIN.JS").Contains("function fib"), "project created from a template: " + dir);
        c.That(st.Current != null && st.Current.Name == "MAIN.JS" && st.Editor.Language == "js", "entry file opened in the editor");
        c.That(st.Entries.Count >= 2, "explorer lists the project (" + st.Entries.Count + " entries)");

        // editing: auto-indent inside a block, undo / redo, Tab
        var ed = st.Editor;
        ed.GoToLine(2);   // "function fib(n) {"
        st.W.SimulateKeys(Keys.End.ToString());
        st.W.SimulateKeys("\n");
        c.That(ed.LineAt(ed.CaretLine - 1) == "  " || ed.LineAt(ed.CaretLine - 1) == "    ", "Enter after '{' indents the new line");
        int lines = ed.LineCount;
        st.W.SimulateKeys("x");
        st.W.SimulateKeys(((char)26).ToString());   // Ctrl+Z
        c.That(ed.LineCount == lines - 1, "Ctrl+Z undid the new line and the typing");
        st.W.SimulateKeys(((char)25).ToString());   // Ctrl+Y
        c.That(ed.LineCount == lines && ed.LineAt(ed.CaretLine - 1).Trim() == "x", "Ctrl+Y redid it");
        st.W.SimulateKeys(((char)26).ToString());
        int col = ed.CaretColumn;
        ed.GoToLine(1);
        st.W.SimulateKeys(Keys.Home.ToString() + Keys.Home.ToString() + "\t");
        c.That(ed.LineAt(0).StartsWith("    //"), "Tab inserts spaces (Tab reaches the editor)");
        st.W.SimulateKeys(((char)26).ToString());
        c.That(st.Current!.Dirty, "edits mark the tab unsaved");
        st.W.SimulateKeys(((char)19).ToString());   // Ctrl+S
        c.That(!st.Current.Dirty && Files.ReadAllText(dir + "/MAIN.JS") == ed.Text, "Ctrl+S saved the file");

        // find, go to line
        st.ShowFind();
        st.FindBox.Text = "faktorial(n - 1)";
        c.That(st.FindNext() && ed.LineAt(ed.CaretLine - 1).Contains("faktorial(n - 1)"), "find moves to the match (line " + ed.CaretLine + ")");
        ed.GoToLine(3);
        c.That(ed.CaretLine == 3, "go to line");

        // run (F5): real output on the device, the simulation elsewhere
        st.W.SimulateKeys(Keys.F5.ToString());
        string all = string.Join("\n", st.Output);
        if (Shell.IsAvailable)
        {
            c.That(all.Contains("fib=55") && all.Contains("faktorial=479001600"), "F5 ran MAIN.JS in the kernel interpreter");
            Files.WriteAllText(dir + "/BAD.JS", "let a = 1;\nlet b = ;\n");
            st.OpenFile(dir + "/BAD.JS");
            st.Run();
            c.That(st.Problems.Count == 1 && st.Problems[0].Line == 2 && st.Editor.ErrorLines.Contains(2), "a parse error lands in Problems with its line (" + (st.Problems.Count > 0 ? st.Problems[0].ToString() : "none") + ")");
            Files.WriteAllText(dir + "/T.PY", "for i in range(1, 4):\n    print(\"py \" + str(i * i))\n");
            st.OpenFile(dir + "/T.PY");
            st.Run();
            c.That(string.Join("\n", st.Output).Contains("py 9"), "F5 runs Python too");
            var tr = st.TerminalRun("echo halo studio | wc -w");
            c.That(tr.Ok && tr.Output.Trim() == "2", "terminal panel runs a pipeline");
            // every runnable template really runs: console ones in the kernel
            // interpreter / the shell, UI ones as headless processes (3 frames)
            string tdir = Files.ScratchDirectory + "/TPLTEST";
            if (Files.GetInfo(tdir) == null) Files.CreateDirectory(tdir);
            int ran = 0, uiRan = 0;
            var bad = new List<string>();
            foreach (var t in Templates.Catalog)
            {
                string path = tdir + "/" + t.Entry;
                string? cmd = RunCommand(path);
                if (cmd == null) continue;
                string text = t.Files.First(f => f.Name == t.Entry).Text;
                Files.WriteAllText(path, text.TrimStart('\n'));
                if (IsUiScript(text))
                {
                    long pid = Programs.Start(path);
                    long code = Programs.WaitForExit(pid);
                    if (code != 0) bad.Add(t.Id + " (exit " + code + ")"); else uiRan++;
                }
                else
                {
                    var r = Shell.Run(cmd);
                    if (r.Output.Contains("error:") || r.Output.StartsWith("sh: ")) bad.Add(t.Id + ": " + r.Output.Substring(Math.Max(0, r.Output.IndexOf("error:"))));
                    else ran++;
                }
            }
            c.That(bad.Count == 0, ran + " console + " + uiRan + " UI templates ran without errors" + (bad.Count > 0 ? "; failed: " + string.Join(" | ", bad) : ""));
        }
        else
        {
            c.That(all.Contains("(simulasi) script js"), "F5 asked the shell to run MAIN.JS (simulated off the device)");
            c.That(st.TerminalRun("echo halo studio").Output.Contains("halo studio"), "terminal panel (simulated)");
        }

        // a second template of another language, closing tabs
        string? d2 = st.CreateProject(Templates.Find("py-kalender")!, "kal");
        c.That(d2 != null && st.Current!.Lang == "py", "Python project opened");
        int open = st.Docs.Count;
        st.W.SimulateKeys(((char)23).ToString());   // Ctrl+W
        c.That(st.Docs.Count == open - 1, "Ctrl+W closed the tab");
        st.Editor.Focus();
        st.W.SimulateKeys(((char)14).ToString());   // Ctrl+N
        c.That(st.W.Popups.Count == 1, "Ctrl+N opens the new-project dialog (" + st.W.Popups.Count + " popup)");
        st.W.RenderFrame();
        c.That(st.W.LastFrameOps > 40, "frame drew " + st.W.LastFrameOps + " ops");
        return c;
    }
}
