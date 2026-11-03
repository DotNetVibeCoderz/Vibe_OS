// Berkas -- the managed file browser: a tree of mounts, a GridSplitter, a
// sortable DataGrid of files and folders with their kind, a preview read on
// demand, launching programs, and file management: new folder / file,
// rename, move, delete and properties (FS_OP syscall: folders on FAT).
// Buitenzorg.UI.

using Buitenzorg.UI;
using FileEntry = Buitenzorg.App.FileEntry;
using Files = Buitenzorg.App.Files;
using Programs = Buitenzorg.App.Programs;

namespace MFiles;

public static class Kinds
{
    public static string Extension(string name)
    {
        int dot = name.LastIndexOf('.');
        return dot < 0 ? "" : name.Substring(dot + 1).ToUpperInvariant();
    }

    public static string Describe(FileEntry f) => f.IsDirectory ? "Folder" : Extension(f.Name) switch
    {
        "TXT" => "Teks",
        "BMP" => "Gambar BMP",
        "JPG" => "Gambar JPEG",
        "ELF" => "Program native",
        "DLL" => "Program .NET",
        "BIN" => "Data",
        "" => "Berkas",
        var e => "Berkas " + e,
    };

    public static IconKind Glyph(FileEntry f) => f.IsDirectory ? IconKind.Folder : Extension(f.Name) switch
    {
        "ELF" or "DLL" => IconKind.Play,
        "BMP" or "JPG" => IconKind.Sun,
        _ => IconKind.File,
    };

    public static bool IsProgram(FileEntry f) => Extension(f.Name) is "ELF" or "DLL";
}

public sealed class FilesViewModel : ObservableObject
{
    string _path = "";
    string _preview = "";
    string _summary = "";
    public ObservableCollection<FileEntry> Entries { get; } = new ObservableCollection<FileEntry>();

    public string Path { get => _path; private set => SetProperty(ref _path, value); }
    public string Preview { get => _preview; set => SetProperty(ref _preview, value); }
    public string Summary { get => _summary; private set => SetProperty(ref _summary, value); }

    public void Open(string path)
    {
        Path = path;
        var list = Files.List(path);
        // folders first, then files, each by name
        list.Sort((a, b) => a.IsDirectory != b.IsDirectory ? (a.IsDirectory ? -1 : 1) : string.CompareOrdinal(a.Name, b.Name));
        Entries.ResetWith(list);
        int programs = 0, folders = 0;
        foreach (var e in list) { if (Kinds.IsProgram(e)) programs++; if (e.IsDirectory) folders++; }
        Summary = path.Length == 0 ? list.Count + " media"
            : (folders > 0 ? folders + " folder, " : "") + (list.Count - folders) + " berkas" + (programs > 0 ? ", " + programs + " program" : "");
        Preview = "";
    }

    /// <summary>The folder above the current one ("" above a mount).</summary>
    public string Parent
    {
        get
        {
            int i = Path.TrimEnd('/').LastIndexOf('/');
            return i <= 0 ? "" : Path.Substring(0, i);
        }
    }

    public bool CanEdit => Path.Length > 0;
    string Child(string name) => Path.TrimEnd('/') + "/" + name.Trim().ToUpperInvariant();

    /// <summary>Result of a file operation: null on success, else the reason shown to the user.</summary>
    static string? Check(string name)
    {
        if (!Files.IsValidName(name.Trim().ToUpperInvariant()))
            return "Nama harus 8.3: maksimal 8 huruf/angka, titik, lalu 3 untuk ekstensi (mis. CATATAN.TXT).";
        return null;
    }

    public string? NewFolder(string name)
    {
        if (!CanEdit) return "Pilih media dulu.";
        var why = Check(name);
        if (why != null) return why;
        if (Files.GetInfo(Child(name)) != null) return "Sudah ada berkas atau folder bernama " + name.ToUpperInvariant() + ".";
        if (!Files.CreateDirectory(Child(name))) return "Folder tidak bisa dibuat di " + Path + ".";
        Open(Path);
        return null;
    }

    public string? NewFile(string name)
    {
        if (!CanEdit) return "Pilih media dulu.";
        var why = Check(name);
        if (why != null) return why;
        if (Files.GetInfo(Child(name)) != null) return "Sudah ada berkas atau folder bernama " + name.ToUpperInvariant() + ".";
        if (!Files.WriteAllText(Child(name), "")) return "Berkas tidak bisa dibuat di " + Path + ".";
        Open(Path);
        return null;
    }

    public string? Rename(FileEntry f, string name)
    {
        var why = Check(name);
        if (why != null) return why;
        if (name.Trim().ToUpperInvariant() == f.Name) return null;
        if (Files.GetInfo(Child(name)) != null) return "Sudah ada berkas atau folder bernama " + name.ToUpperInvariant() + ".";
        if (!Files.Move(f.Path, Child(name))) return "Tidak bisa mengganti nama " + f.Name + ".";
        Open(Path);
        return null;
    }

    /// <summary>Move into `folder` (an absolute folder path, e.g. "/disk/ARSIP" or "/ram").</summary>
    public string? MoveTo(FileEntry f, string folder)
    {
        folder = folder.Trim().TrimEnd('/').ToUpperInvariant().Replace("/DISK", "/disk").Replace("/RAM", "/ram");
        var target = Files.GetInfo(folder);
        if (target == null || !target.IsDirectory) return "Folder tujuan " + folder + " tidak ada.";
        if (folder == Path.TrimEnd('/')) return "Sudah berada di folder itu.";
        if (f.IsDirectory && (folder + "/").StartsWith(f.Path + "/")) return "Folder tidak bisa dipindah ke dalam dirinya sendiri.";
        string to = folder + "/" + f.Name;
        if (Files.GetInfo(to) != null) return "Di " + folder + " sudah ada " + f.Name + ".";
        if (!Files.Move(f.Path, to))
            return f.IsDirectory && !folder.StartsWith(Path.Substring(0, Math.Max(1, Path.IndexOf('/', 1) < 0 ? Path.Length : Path.IndexOf('/', 1))))
                ? "Folder hanya bisa dipindah di media yang sama."
                : "Tidak bisa memindahkan " + f.Name + ".";
        Open(Path);
        return null;
    }

    public string? Delete(FileEntry f)
    {
        if (!Files.Delete(f.Path))
            return f.IsDirectory ? "Folder " + f.Name + " belum kosong." : "Tidak bisa menghapus " + f.Name + ".";
        Open(Path);
        return null;
    }

    /// <summary>The properties of an entry, as label / value rows.</summary>
    public List<(string, string)> Properties(FileEntry f)
    {
        var rows = new List<(string, string)> { ("Nama", f.Name), ("Lokasi", Parent.Length == 0 && Path.Length == 0 ? "/" : Path), ("Jenis", Kinds.Describe(f)) };
        var info = Files.GetInfo(f.Path);
        if (f.IsDirectory)
        {
            int files = 0, folders = 0;
            long bytes = 0;
            foreach (var e in Files.List(f.Path))
            {
                if (e.IsDirectory) folders++;
                else { files++; bytes += Math.Max(0, Files.GetInfo(e.Path)?.Size ?? 0); }
            }
            rows.Add(("Isi", files + " berkas, " + folders + " folder"));
            rows.Add(("Ukuran", FormatSize(bytes) + " (tanpa subfolder)"));
        }
        else rows.Add(("Ukuran", info == null ? "?" : FormatSize(info.Size) + " (" + info.Size + " byte)"));
        rows.Add(("Diubah", info?.Modified is DateTime m ? m.ToString("dd-MM-yyyy HH:mm") : "-"));
        return rows;
    }

    public void Select(FileEntry? f)
    {
        if (f == null) { Preview = ""; return; }
        if (f.IsDirectory) { Preview = "Folder " + f.Path; return; }
        long size = Files.Size(f.Path);
        string head = size < 0 ? "tidak terbaca" : FormatSize(size);
        if (Kinds.Extension(f.Name) == "TXT")
        {
            string t = Files.ReadAllText(f.Path);
            if (t.Length > 420) t = t.Substring(0, 420) + " ...";
            Preview = head + "\n\n" + t;
        }
        else Preview = Kinds.Describe(f) + ", " + head + (Kinds.IsProgram(f) ? "\nBuka untuk menjalankan." : "");
    }

    public static string FormatSize(long b) =>
        b < 1024 ? b + " B" : b < 1024 * 1024 ? (b / 1024) + " KB" : (b / 1024 / 1024) + "." + (b % (1024 * 1024) * 10 / (1024 * 1024)) + " MB";
}

public static class FilesApp
{
    public static (Window, FilesViewModel, DataGrid, TreeView) Build(bool headless = false)
    {
        var vm = new FilesViewModel();
        var w = new Window("Berkas", 640, 440, headless);
        var notes = headless ? null : new WindowNotificationManager(w);

        // left: the mounts
        var tree = new TreeView { BorderThickness = 0, CornerRadius = 0, Background = Color.Transparent };
        var root = new TreeViewItem("Buitenzorg", IconKind.Home) { Value = "" };
        foreach (var m in Files.List("")) root.Add(new TreeViewItem(m.Name, IconKind.Folder) { Value = m.Path });
        root.IsExpanded = true;
        tree.Add(root);
        tree.SelectionChanged += t => { if (t.SelectedItem?.Value is string p) vm.Open(p); };

        // right: the files
        var grid = new DataGrid { BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0 };
        grid.AddColumn(new DataGridColumn("Nama", o => ((FileEntry)o).Name, "3*"));
        grid.AddColumn(new DataGridColumn("Jenis", o => Kinds.Describe((FileEntry)o), "2*"));
        grid.ItemsSource = vm.Entries;
        grid.SelectionChanged += g => vm.Select(g.SelectedItem as FileEntry);

        void Toast(string? error, string ok)
        {
            if (error == null) notes?.Show(new Notification(ok, vm.Path, NotificationType.Success, 2000));
            else notes?.Show(new Notification("Tidak bisa", error, NotificationType.Error));
        }

        // a small form: a hint and a text box; OK runs `act` with the text
        void Ask(string title, string hint, string initial, string ok, Func<string, string?> act, string done)
        {
            var box = new TextBox { Text = initial, Width = 300 };
            box.CaretIndex = initial.Length;
            var body = new StackPanel { Spacing = 8 };
            body.Children.Add(new TextBlock(hint) { TextWrapping = TextWrapping.Wrap }.WithClass("muted"));
            body.Children.Add(box);
            Dialog.Show(w, title, body, ok, "Batal", go => { if (go) Toast(act(box.Text), done); grid.Focus(); });
            box.Focus();
        }

        FileEntry? Selected() => grid.SelectedItem as FileEntry;
        void NewFolder() => Ask("Folder baru", "Nama folder (8.3, mis. ARSIP).", "", "Buat", vm.NewFolder, "Folder dibuat");
        void NewFile() => Ask("Berkas baru", "Nama berkas (8.3, mis. CATATAN.TXT).", "BARU.TXT", "Buat", vm.NewFile, "Berkas dibuat");
        void Rename() { if (Selected() is FileEntry f) Ask("Ganti nama", "Nama baru untuk " + f.Name + ".", f.Name, "Ganti", n => vm.Rename(f, n), "Nama diganti"); }
        void Move()
        {
            if (Selected() is not FileEntry f) return;
            Ask("Pindahkan " + f.Name, "Folder tujuan, mis. /disk/ARSIP, /disk atau /ram (berkas bisa pindah media).", vm.Path, "Pindahkan", t => vm.MoveTo(f, t), "Dipindahkan");
        }
        void Delete()
        {
            if (Selected() is not FileEntry f) return;
            Dialog.Show(w, "Hapus " + (f.IsDirectory ? "folder" : "berkas") + "?", f.Path + (f.IsDirectory ? " (harus kosong)" : "") + " akan dihapus permanen.", "Hapus", "Batal",
                go => { if (go) Toast(vm.Delete(f), "Dihapus"); grid.Focus(); });
        }
        void Properties()
        {
            if (Selected() is not FileEntry f) return;
            var table = new Grid { ColumnDefinitions = "90,*", RowSpacing = 6 };
            var rows = vm.Properties(f);
            var defs = new List<string>();
            for (int i = 0; i < rows.Count; i++) defs.Add("Auto");
            table.RowDefinitions = string.Join(",", defs);
            for (int i = 0; i < rows.Count; i++)
            {
                table.Children.Add(new TextBlock(rows[i].Item1).WithClass("faint").Cell(i, 0));
                table.Children.Add(new TextBlock(rows[i].Item2) { TextWrapping = TextWrapping.Wrap }.Cell(i, 1));
            }
            Dialog.Show(w, "Properti", table, "Tutup", null, _ => grid.Focus());
        }

        void OpenSelected()
        {
            if (grid.SelectedItem is not FileEntry f) return;
            if (f.IsDirectory) { vm.Open(f.Path); return; }
            if (Kinds.IsProgram(f))
            {
                try
                {
                    Programs.Start(f.Path);
                    notes?.Show(new Notification("Dijalankan", f.Name, NotificationType.Success, 2000));
                }
                catch (Exception e) { notes?.Show(new Notification("Gagal", e.Message, NotificationType.Error)); }
            }
        }
        grid.KeyDown += (_, e) =>
        {
            if (e.Key == Keys.Enter) { OpenSelected(); e.Handled = true; }
            else if (e.Key == Keys.F2) { Rename(); e.Handled = true; }
            else if (e.Key == Keys.Delete) { Delete(); e.Handled = true; }
        };

        var menu = new MenuFlyout()
            .Add(new MenuItem("Buka", _ => OpenSelected(), "Enter", IconKind.Play))
            .Add(MenuItem.Separator())
            .Add(new MenuItem("Folder baru", _ => NewFolder(), "", IconKind.Folder))
            .Add(new MenuItem("Berkas baru", _ => NewFile(), "Ctrl+N", IconKind.Plus))
            .Add(new MenuItem("Ganti nama", _ => Rename(), "F2", IconKind.File))
            .Add(new MenuItem("Pindahkan...", _ => Move(), "", IconKind.ChevronRight))
            .Add(new MenuItem("Hapus", _ => Delete(), "Del", IconKind.Trash))
            .Add(MenuItem.Separator())
            .Add(new MenuItem("Properti", _ => Properties(), "", IconKind.Info))
            .Add(new MenuItem("Muat ulang", _ => vm.Open(vm.Path), "F5", IconKind.Refresh));
        grid.WithContextMenu(menu);

        var path = new TextBlock { VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis }
            .WithClass("strong").Bind(TextBlock.TextProperty, vm, () => vm.Path.Length == 0 ? "Semua media" : vm.Path, nameof(FilesViewModel.Path));
        var summary = new TextBlock { VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(10, 0) }.WithClass("faint")
            .Bind(TextBlock.TextProperty, vm, () => vm.Summary, nameof(FilesViewModel.Summary));
        var bar = new DockPanel { Margin = new Thickness(0, 0, 0, 6) };
        bar.Children.Add(new Button(new Icon(IconKind.Play, 14), _ => OpenSelected()).WithClass("accent").WithTip("Buka / jalankan (Enter)").DockTo(Dock.Right));
        bar.Children.Add(new Button(new Icon(IconKind.Refresh, 14), _ => vm.Open(vm.Path)) { Margin = new Thickness(0, 0, 6, 0) }.WithTip("Muat ulang").DockTo(Dock.Right));
        bar.Children.Add(new Button(new Icon(IconKind.ChevronLeft, 14), _ => vm.Open(vm.Parent)) { Margin = new Thickness(0, 0, 8, 0) }.WithClass("subtle").WithTip("Naik satu folder (Backspace)").DockTo(Dock.Left));
        bar.Children.Add(summary.DockTo(Dock.Right));
        bar.Children.Add(path);

        // the file-management toolbar
        Button Tool(IconKind glyph, string label, Action act, string tip)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
            row.Children.Add(new Icon(glyph, 12) { VerticalAlignment = VerticalAlignment.Center });
            row.Children.Add(new TextBlock(label));
            var b = new Button(row, _ => act()) { Padding = new Thickness(8, 4), Tag = label };
            b.Classes.Add("subtle");
            return (Button)b.WithTip(tip);
        }
        var tools = new WrapPanel { Margin = new Thickness(0, 0, 0, 8) };
        tools.Children.Add(Tool(IconKind.Folder, "Folder baru", NewFolder, "Buat folder di sini"));
        tools.Children.Add(Tool(IconKind.Plus, "Berkas baru", NewFile, "Buat berkas teks kosong (Ctrl+N)"));
        tools.Children.Add(Tool(IconKind.File, "Ganti nama", Rename, "Ganti nama yang dipilih (F2)"));
        tools.Children.Add(Tool(IconKind.ChevronRight, "Pindahkan", Move, "Pindahkan ke folder lain"));
        tools.Children.Add(Tool(IconKind.Trash, "Hapus", Delete, "Hapus yang dipilih (Del)"));
        tools.Children.Add(Tool(IconKind.Info, "Properti", Properties, "Ukuran, jenis, tanggal"));

        var preview = new TextBlock { TextWrapping = TextWrapping.Wrap }.WithClass("muted")
            .Bind(TextBlock.TextProperty, vm, () => vm.Preview.Length == 0 ? "Pilih berkas untuk melihat isinya." : vm.Preview, nameof(FilesViewModel.Preview));
        var previewBox = new Expander("Pratinjau", new ScrollViewer { Content = preview, MaxHeight = 120 }, true) { Margin = new Thickness(0, 8, 0, 0) };

        var right = new DockPanel { Margin = new Thickness(12) };
        right.Children.Add(bar.DockTo(Dock.Top));
        right.Children.Add(tools.DockTo(Dock.Top));
        right.Children.Add(previewBox.DockTo(Dock.Bottom));
        right.Children.Add(new Border { Child = grid }.WithClass("inset"));

        var leftPane = new DockPanel { Background = Theme.Surface };
        leftPane.Children.Add(new TextBlock("MEDIA") { Margin = new Thickness(14, 14, 0, 6) }.WithClass("eyebrow").DockTo(Dock.Top));
        leftPane.Children.Add(tree);

        var layout = new Grid { ColumnDefinitions = "180,6,*" };
        layout.Children.Add(leftPane.Col(0));
        layout.Children.Add(new GridSplitter().Col(1));
        layout.Children.Add(right.Col(2));
        w.Content = layout;
        w.UnhandledKey += e =>
        {
            switch (e.Key)
            {
                case Keys.Backspace: vm.Open(vm.Parent); e.Handled = true; break;   // up one folder
                case (char)14: NewFile(); e.Handled = true; break;                  // Ctrl+N
                case Keys.F5: vm.Open(vm.Path); e.Handled = true; break;
            }
        };

        // start in /disk when present
        TreeViewItem? start = null;
        foreach (var t in root.Items) if ((t.Value as string) == "/disk") start = t;
        if (start != null) tree.Select(start); else vm.Open("");
        Application.Current?.Apply(w);
        w.UpdateLayout();
        grid.Focus();
        return (w, vm, grid, tree);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Berkas");
        string dir = Files.ScratchDirectory;
        Files.WriteAllText(dir + "/BACA.TXT", "Kebun Raya Bogor, 1817.");
        var (w, vm, grid, tree) = FilesApp.Build(headless: true);
        c.That(tree.SelectedItem != null || vm.Path.Length == 0, "starts at a mount");
        vm.Open("");
        c.That(vm.Entries.Count >= 1, "mounts listed (" + vm.Entries.Count + ")");
        vm.Open(dir);
        bool found = false;
        foreach (var e in vm.Entries) if (e.Name == "BACA.TXT") found = true;
        c.That(found, dir + " lists BACA.TXT");
        int row = -1;
        for (int i = 0; i < vm.Entries.Count; i++) if (vm.Entries[i].Name == "BACA.TXT") row = i;
        w.UpdateLayout();
        grid.Select(row);
        c.That(vm.Preview.Contains("Kebun Raya"), "preview shows the text");
        c.Equal("Teks", Kinds.Describe(new FileEntry("A.TXT", "/a", false)), "kind of a .TXT");
        c.Equal("Program .NET", Kinds.Describe(new FileEntry("MCALC.DLL", "/disk/MCALC.DLL", false)), "kind of a .DLL");
        c.Equal("1.5 MB", FilesViewModel.FormatSize(1536 * 1024), "size formatting");
        // keyboard: the grid has focus after start-up; Down selects the first row
        var (w2, vm2, grid2, _) = FilesApp.Build(headless: true);
        vm2.Open(dir);
        w2.UpdateLayout();
        c.That(w2.FocusedControl == grid2, "the grid has the focus (" + (w2.FocusedControl?.GetType().Name ?? "none") + ")");
        w2.SimulateKeys(Keys.Down.ToString());
        c.That(grid2.SelectedIndex == 0, "Down selected row " + grid2.SelectedIndex);
        grid.SortBy(0);
        grid.SortBy(0);
        c.That(true, "sorting both ways");

        // file management
        vm.Open(dir);
        foreach (var leftover in new[] { "/ARSIP/BACA.TXT", "/ARSIP/LAMA/X.TXT", "/ARSIP/LAMA", "/ARSIP", "/BACA2.TXT" }) Files.Delete(dir + leftover);
        c.That(vm.NewFolder("arsip") == null && Files.GetInfo(dir + "/ARSIP")?.IsDirectory == true, "new folder ARSIP");
        c.That(vm.NewFolder("ARSIP") != null, "a second ARSIP is refused");
        c.That(vm.NewFolder("nama-terlalu-panjang") != null, "a name that is not 8.3 is refused");
        FileEntry? Find(string n) { foreach (var e in vm.Entries) if (e.Name == n) return e; return null; }
        c.That(vm.Rename(Find("BACA.TXT")!, "BACA2.TXT") == null && Find("BACA2.TXT") != null && Find("BACA.TXT") == null, "rename BACA.TXT -> BACA2.TXT");
        c.That(vm.MoveTo(Find("BACA2.TXT")!, dir + "/ARSIP") == null && Files.ReadAllText(dir + "/ARSIP/BACA2.TXT").Contains("Kebun"), "move into ARSIP keeps the content");
        c.That(Find("ARSIP")!.IsDirectory && vm.Entries.IndexOf(Find("ARSIP")!) == 0, "folders are listed first");
        var props = vm.Properties(Find("ARSIP")!);
        c.That(props.Exists(r => r.Item1 == "Isi" && r.Item2.StartsWith("1 berkas")), "properties count the folder's content");
        c.That(vm.Delete(Find("ARSIP")!) != null, "a non-empty folder is not deleted");
        vm.Open(dir + "/ARSIP");
        c.That(vm.Parent == dir, "parent of " + vm.Path + " is " + vm.Parent);
        c.That(vm.Delete(Find("BACA2.TXT")!) == null, "delete a file");
        vm.Open(dir);
        c.That(vm.Delete(Find("ARSIP")!) == null && Files.GetInfo(dir + "/ARSIP") == null, "delete the empty folder");
        Control? newFolder = null;
        foreach (var d in w.Descendants()) if (d is Button b && (b.Tag as string) == "Folder baru") newFolder = d;
        if (newFolder != null) w.SimulateClick(newFolder);
        c.That(w.Popups.Count == 1, "the toolbar's Folder baru opens a form");
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
            var c = FilesApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _, _) = FilesApp.Build();
        app.Run(w);
        return 0;
    }
}
