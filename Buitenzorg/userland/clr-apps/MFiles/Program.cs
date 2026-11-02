// Berkas -- the managed file browser: a tree of mounts, a GridSplitter, a
// sortable DataGrid of files with their kind, a preview read on demand, and
// launching programs. Buitenzorg.UI.

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
        Entries.ResetWith(list);
        int programs = 0;
        foreach (var e in list) if (Kinds.IsProgram(e)) programs++;
        Summary = list.Count + " berkas" + (programs > 0 ? ", " + programs + " program" : "");
        Preview = "";
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
        grid.KeyDown += (_, e) => { if (e.Key == Keys.Enter) { OpenSelected(); e.Handled = true; } };

        var menu = new MenuFlyout()
            .Add(new MenuItem("Buka", _ => OpenSelected(), "Enter", IconKind.Play))
            .Add(new MenuItem("Muat ulang", _ => vm.Open(vm.Path), "F5", IconKind.Refresh));
        grid.WithContextMenu(menu);

        var path = new TextBlock { VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis }
            .WithClass("strong").Bind(TextBlock.TextProperty, vm, () => vm.Path.Length == 0 ? "Semua media" : vm.Path, nameof(FilesViewModel.Path));
        var summary = new TextBlock { VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(10, 0) }.WithClass("faint")
            .Bind(TextBlock.TextProperty, vm, () => vm.Summary, nameof(FilesViewModel.Summary));
        var bar = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        bar.Children.Add(new Button(new Icon(IconKind.Play, 14), _ => OpenSelected()).WithClass("accent").WithTip("Buka / jalankan (Enter)").DockTo(Dock.Right));
        bar.Children.Add(new Button(new Icon(IconKind.Refresh, 14), _ => vm.Open(vm.Path)) { Margin = new Thickness(0, 0, 6, 0) }.WithTip("Muat ulang").DockTo(Dock.Right));
        bar.Children.Add(new Icon(IconKind.Folder, 16) { Margin = new Thickness(0, 0, 8, 0) }.WithClass("accent").DockTo(Dock.Left));
        bar.Children.Add(summary.DockTo(Dock.Right));
        bar.Children.Add(path);

        var preview = new TextBlock { TextWrapping = TextWrapping.Wrap }.WithClass("muted")
            .Bind(TextBlock.TextProperty, vm, () => vm.Preview.Length == 0 ? "Pilih berkas untuk melihat isinya." : vm.Preview, nameof(FilesViewModel.Preview));
        var previewBox = new Expander("Pratinjau", new ScrollViewer { Content = preview, MaxHeight = 120 }, true) { Margin = new Thickness(0, 8, 0, 0) };

        var right = new DockPanel { Margin = new Thickness(12) };
        right.Children.Add(bar.DockTo(Dock.Top));
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
        w.UnhandledKey += e => { if (e.Key == Keys.Backspace) { vm.Open(""); e.Handled = true; } };

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
