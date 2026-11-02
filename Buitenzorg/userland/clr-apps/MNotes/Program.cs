// Catatan -- managed notes: a menu bar with shortcuts, a SplitView with the
// list of notes, a multi-line editor, a status bar, toasts and a confirmation
// dialog. Notes are CATAT<n>.TXT files in /disk (or /ram). Buitenzorg.UI.

using Files = Buitenzorg.App.Files;
using Buitenzorg.UI;

namespace MNotes;

public sealed class Note : ObservableObject
{
    string _text;
    bool _dirty;
    public string Path { get; }
    public Note(string path, string text) { Path = path; _text = text; }

    public string Text { get => _text; set { if (SetProperty(ref _text, value)) { Dirty = true; OnPropertyChanged(nameof(Title)); } } }
    public bool Dirty { get => _dirty; set => SetProperty(ref _dirty, value); }

    public string Title
    {
        get
        {
            string t = _text.Trim();
            int nl = t.IndexOf('\n');
            if (nl >= 0) t = t.Substring(0, nl);
            if (t.Length == 0) return "Catatan kosong";
            return t.Length > 26 ? t.Substring(0, 25) + "~" : t;
        }
    }

    public override string ToString() => Title;
}

public sealed class NotesViewModel : ObservableObject
{
    readonly string _dir;
    Note? _current;
    string _status = "";
    public ObservableCollection<Note> Notes { get; } = new ObservableCollection<Note>();

    public NotesViewModel(string dir) { _dir = dir; }

    public string Folder => _dir;
    public Note? Current { get => _current; set { SetProperty(ref _current, value); OnPropertyChanged(nameof(Counts)); } }
    public string Status { get => _status; set => SetProperty(ref _status, value); }

    public string Counts
    {
        get
        {
            if (_current == null) return "";
            string t = _current.Text;
            int lines = t.Length == 0 ? 1 : t.Split('\n').Length;
            return lines + " baris  ·  " + t.Length + " karakter";
        }
    }

    public void Load()
    {
        var found = new List<Note>();
        foreach (var f in Files.List(_dir))
            if (!f.IsDirectory && f.Name.StartsWith("CATAT") && f.Name.EndsWith(".TXT"))
            {
                // a deleted note is an emptied file (the VFS has no delete call)
                string text = Files.ReadAllText(f.Path);
                if (text.Length > 0) found.Add(new Note(f.Path, text) { Dirty = false });
            }
        Notes.ResetWith(found);
        if (Notes.Count == 0) New("Selamat datang di Catatan.\n\nTulis apa saja; Ctrl+S menyimpan ke " + _dir + ".");
        Current = Notes[0];
        Status = Notes.Count + " catatan di " + _dir;
    }

    string FreePath()
    {
        for (int i = 1; i < 100; i++)
        {
            string p = _dir + "/CATAT" + i.ToString("00") + ".TXT";
            bool used = false;
            foreach (var n in Notes) if (n.Path == p) used = true;
            if (!used) return p;
        }
        return _dir + "/CATAT99.TXT";
    }

    public Note New(string text = "")
    {
        var n = new Note(FreePath(), text) { Dirty = true };
        Notes.Insert(0, n);
        Current = n;
        return n;
    }

    public bool Save()
    {
        if (_current == null) return false;
        bool ok = Files.WriteAllText(_current.Path, _current.Text);
        if (ok) _current.Dirty = false;
        Status = ok ? "Tersimpan: " + _current.Path : "Gagal menyimpan " + _current.Path;
        return ok;
    }

    public void Delete()
    {
        if (_current == null) return;
        Files.WriteAllText(_current.Path, "");
        int i = Notes.IndexOf(_current);
        Notes.Remove(_current);
        if (Notes.Count == 0) New();
        Current = Notes[Math.Max(0, Math.Min(i, Notes.Count - 1))];
        Status = "Catatan dihapus";
    }

    public void Touch() => OnPropertyChanged(nameof(Counts));
}

public static class NotesApp
{
    public static (Window, NotesViewModel, TextBox, ListBox) Build(string dir, bool headless = false)
    {
        var vm = new NotesViewModel(dir);
        var w = new Window("Catatan", 620, 430, headless);
        var notes = headless ? null : new WindowNotificationManager(w);

        var editor = new TextBox { AcceptsReturn = true, Watermark = "Mulai menulis...", BorderThickness = 0, CornerRadius = 0, Background = Color.Transparent, Padding = new Thickness(18, 14) };
        var list = new ListBox { ItemsSource = vm.Notes, BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0 };
        list.ItemTemplate = o =>
        {
            var n = (Note)o;
            var row = new DockPanel();
            // unsaved marker: a small sun-colored dot
            var dot = new Border { Width = 7, Height = 7, CornerRadius = 4, Background = Theme.Sun, Margin = new Thickness(6, 0, 2, 0), VerticalAlignment = VerticalAlignment.Center };
            dot.Bind(Control.IsVisibleProperty, n, () => n.Dirty, nameof(Note.Dirty));
            row.Children.Add(dot.DockTo(Dock.Right));
            row.Children.Add(new TextBlock { TextTrimming = TextTrimming.CharacterEllipsis }.Bind(TextBlock.TextProperty, n, () => n.Title, nameof(Note.Title)));
            return row;
        };

        bool syncing = false;
        void ShowCurrent()
        {
            syncing = true;
            editor.Text = vm.Current?.Text ?? "";
            editor.CaretIndex = editor.Text.Length;
            int i = vm.Current == null ? -1 : vm.Notes.IndexOf(vm.Current);
            if (list.SelectedIndex != i) list.SelectedIndex = i;
            syncing = false;
        }
        vm.PropertyChanged += (_, n) => { if (n == nameof(NotesViewModel.Current)) ShowCurrent(); };
        list.SelectionChanged += l => { if (!syncing && l.SelectedItem is Note n && n != vm.Current) vm.Current = n; };
        editor.TextChanged += t => { if (!syncing && vm.Current != null) { vm.Current.Text = t.Text; vm.Touch(); } };

        void Save()
        {
            bool ok = vm.Save();
            notes?.Show(ok ? new Notification("Tersimpan", vm.Current!.Path, NotificationType.Success, 2500)
                           : new Notification("Tidak tersimpan", "Media " + vm.Folder + " menolak penulisan.", NotificationType.Error));
        }
        void NewNote() { vm.New(); editor.Focus(); }
        void Delete() =>
            Dialog.Show(w, "Hapus catatan?", "\"" + (vm.Current?.Title ?? "") + "\" akan dihapus dari " + vm.Folder + ".", "Hapus", "Batal",
                ok => { if (ok) vm.Delete(); editor.Focus(); });

        var split = new SplitView { OpenPaneLength = 200, CompactPaneLength = 0, DisplayMode = SplitViewDisplayMode.CompactInline, PaneBackground = Theme.Surface };
        var paneTitle = new TextBlock("CATATAN") { Margin = new Thickness(14, 12, 0, 6) }.WithClass("eyebrow");
        var pane = new DockPanel();
        pane.Children.Add(paneTitle.DockTo(Dock.Top));
        pane.Children.Add(new Button(new TextBlock("+ Catatan baru"), _ => NewNote()) { HorizontalAlignment = HorizontalAlignment.Stretch, Margin = new Thickness(8) }
            .WithClass("subtle").DockTo(Dock.Bottom));
        pane.Children.Add(list);
        split.Pane = pane;
        split.Content = editor;

        var menu = new Menu();
        menu.Add(new MenuItem("Berkas")
            .Add(new MenuItem("Catatan baru", _ => NewNote(), "Ctrl+N", IconKind.Plus))
            .Add(new MenuItem("Simpan", _ => Save(), "Ctrl+S", IconKind.Save))
            .Add(MenuItem.Separator())
            .Add(new MenuItem("Hapus catatan", _ => Delete(), "", IconKind.Trash)));
        menu.Add(new MenuItem("Tampilan")
            .Add(new MenuItem("Panel catatan", _ => split.IsPaneOpen = !split.IsPaneOpen, "Ctrl+B", IconKind.Menu))
            .Add(new MenuItem("Tema terang / gelap", _ => ToggleTheme(), "Ctrl+T", IconKind.Sun)));

        var status = new DockPanel { Background = Theme.Surface };
        var counts = new TextBlock { Margin = new Thickness(12, 4) }.WithClass("faint").Bind(TextBlock.TextProperty, vm, () => vm.Counts, nameof(NotesViewModel.Counts));
        status.Children.Add(counts.DockTo(Dock.Right));
        status.Children.Add(new TextBlock { Margin = new Thickness(12, 4), TextTrimming = TextTrimming.CharacterEllipsis }.WithClass("muted")
            .Bind(TextBlock.TextProperty, vm, () => vm.Status, nameof(NotesViewModel.Status)));

        var root = new DockPanel();
        root.Children.Add(menu.DockTo(Dock.Top));
        root.Children.Add(status.DockTo(Dock.Bottom));
        root.Children.Add(split);
        w.Content = root;
        w.UnhandledKey += e =>
        {
            switch (e.Key)
            {
                case (char)19: Save(); e.Handled = true; break;          // Ctrl+S
                case (char)14: NewNote(); e.Handled = true; break;       // Ctrl+N
                case (char)2: split.IsPaneOpen = !split.IsPaneOpen; e.Handled = true; break; // Ctrl+B
                case (char)20: ToggleTheme(); e.Handled = true; break;   // Ctrl+T
            }
        };

        vm.Load();
        ShowCurrent();
        Application.Current?.Apply(w);
        w.UpdateLayout();
        editor.Focus();
        return (w, vm, editor, list);
    }

    static void ToggleTheme()
    {
        var app = Application.Current;
        app?.SetTheme(app.ThemeVariant == ThemeVariant.Dark ? ThemeVariant.Light : ThemeVariant.Dark);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Catatan");
        // the scratch folder: /ram on a PC, /disk where there is no RAM disk
        string dir = Files.ScratchDirectory;
        Files.WriteAllText(dir + "/CATAT01.TXT", "Daftar belanja\nkopi\ngula aren");
        var (w, vm, editor, list) = Build(dir, headless: true);
        c.That(vm.Notes.Count >= 1 && vm.Current != null, "notes loaded (" + vm.Notes.Count + ")");
        c.Equal("Daftar belanja", vm.Current?.Title ?? "", "title is the first line");
        w.SimulateKeys(Keys.End.ToString());
        w.SimulateKeys("\nteh");
        c.That(vm.Current!.Text.EndsWith("teh") && vm.Current.Dirty, "typing edits the note and marks it unsaved");
        w.SimulateKeys(((char)19).ToString()); // Ctrl+S
        c.That(!vm.Current.Dirty, "Ctrl+S saved");
        c.That(Files.ReadAllText(dir + "/CATAT01.TXT").EndsWith("teh"), "file on " + dir + " holds the edit");
        int count0 = vm.Notes.Count;
        w.SimulateKeys(((char)14).ToString()); // Ctrl+N
        c.That(vm.Current!.Text.Length == 0, "Ctrl+N made an empty note");
        w.SimulateKeys("Ide kedua");
        c.Equal("Ide kedua", vm.Notes[0].Title, "new note title follows its text");
        c.That(list.ItemCount == count0 + 1, "list shows the new note too");
        int old = -1;
        for (int i = 0; i < vm.Notes.Count; i++) if (vm.Notes[i].Path.EndsWith("CATAT01.TXT")) old = i;
        list.SelectedIndex = old;
        c.Equal("Daftar belanja", vm.Current!.Title, "selecting in the list switches the note");
        c.That(editor.Text.StartsWith("Daftar"), "editor shows the selected note");
        w.RenderFrame();
        c.That(w.LastFrameOps > 20, "frame drew " + w.LastFrameOps + " ops");
        return c;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = NotesApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        // persistent notes on /disk when it is there, else /ram
        string dir = "/ram";
        foreach (var m in Files.List("")) if (m.Name == "disk") dir = "/disk";
        var (w, _, _, _) = NotesApp.Build(dir);
        app.Run(w);
        return 0;
    }
}
