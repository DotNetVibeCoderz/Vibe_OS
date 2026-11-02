// Galeri Kontrol -- the Buitenzorg.UI control catalog (Avalonia's
// ControlCatalog): every control on a page by category, live, with the
// light/dark theme switch. Its self-test is the toolkit's own test.

using Buitenzorg.UI;

namespace MGallery;

public sealed class Plant
{
    public string Name { get; }
    public string Latin { get; }
    public string Family { get; }
    public int Height { get; }
    public IconKind Glyph { get; }
    public Plant(string name, string latin, string family, int height, IconKind glyph) { Name = name; Latin = latin; Family = family; Height = height; Glyph = glyph; }
    public override string ToString() => Name;
}

public static class GalleryApp
{
    public static readonly Plant[] Plants =
    {
        new Plant("Bunga bangkai", "Amorphophallus titanum", "Araceae", 300, IconKind.Sun),
        new Plant("Rafflesia", "Rafflesia arnoldii", "Rafflesiaceae", 1, IconKind.Sun),
        new Plant("Kenari", "Canarium indicum", "Burseraceae", 40, IconKind.Leaf),
        new Plant("Meranti", "Shorea leprosula", "Dipterocarpaceae", 60, IconKind.Leaf),
        new Plant("Anggrek bulan", "Phalaenopsis amabilis", "Orchidaceae", 1, IconKind.Sun),
        new Plant("Teratai raksasa", "Victoria amazonica", "Nymphaeaceae", 1, IconKind.Leaf),
        new Plant("Pakis haji", "Cycas rumphii", "Cycadaceae", 5, IconKind.Leaf),
        new Plant("Kantong semar", "Nepenthes gracilis", "Nepenthaceae", 2, IconKind.Leaf),
    };

    static Control Section(string title, string note, Control body)
    {
        var s = new StackPanel { Spacing = 8, Margin = new Thickness(0, 0, 0, 18) };
        s.Children.Add(new TextBlock(title).WithClass("strong"));
        if (note.Length > 0) s.Children.Add(new TextBlock(note) { TextWrapping = TextWrapping.Wrap }.WithClass("faint"));
        s.Children.Add(body);
        return s;
    }

    static StackPanel Row(params Control[] items)
    {
        var r = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        foreach (var i in items) r.Children.Add(i);
        return r;
    }

    static Control Page(params Control[] sections)
    {
        var s = new StackPanel { Margin = new Thickness(20, 16, 20, 8) };
        foreach (var x in sections) s.Children.Add(x);
        return new ScrollViewer { Content = s };
    }

    static Control Buttons(Window w)
    {
        int count = 0;
        var counter = new TextBlock("0") { VerticalAlignment = VerticalAlignment.Center }.WithClass("strong");
        var repeat = new RepeatButton("Tahan +1", _ => counter.Text = (++count).ToString());
        var menu = new MenuFlyout()
            .Add(new MenuItem("Siram", null, "S", IconKind.Refresh))
            .Add(new MenuItem("Pangkas", null, "P", IconKind.Close))
            .Add(MenuItem.Separator())
            .Add(new MenuItem("Lainnya").Add(new MenuItem("Pupuk")).Add(new MenuItem("Pindah pot")));
        var split = new SplitButton("Tanam") { Flyout = new MenuFlyout().Add(new MenuItem("Tanam di pot")).Add(new MenuItem("Tanam di kebun")) };
        return Page(
            Section("Button", "Tombol biasa, aksen, matahari, halus, bahaya, dan nonaktif. Warna berpindah dengan transisi.",
                Row(new Button("Biasa"), new Button("Aksen").WithClass("accent"), new Button("Matahari").WithClass("sun"),
                    new Button("Halus").WithClass("subtle"), new Button("Hapus").WithClass("danger"), new Button("Nonaktif") { IsEnabled = false })),
            Section("RepeatButton", "Terus mengklik selama ditahan.", Row(repeat, counter)),
            Section("ToggleButton  ·  HyperlinkButton", "", Row(new ToggleButton("Sematkan"), new HyperlinkButton("Kebun Raya Bogor"))),
            Section("DropDownButton  ·  SplitButton", "Menu muncul di flyout; tekan Esc atau klik di luar untuk menutup.",
                Row(new DropDownButton("Rawat") { Flyout = menu }, split)));
    }

    static Control Choices(out Slider slider, out ProgressBar bar)
    {
        slider = new Slider { Maximum = 100, Value = 40, TickFrequency = 10, IsSnapToTickEnabled = true, Width = 240 };
        bar = new ProgressBar { Width = 240, ShowProgressText = true, Value = 40 };
        var b = bar;
        slider.ValueChanged += s => b.Value = s.Value;
        var busy = new ProgressBar { Width = 240, IsIndeterminate = true };
        return Page(
            Section("CheckBox", "", Row(new CheckBox("Disiram", true), new CheckBox("Dipupuk"), new CheckBox("Dipangkas") { IsEnabled = false })),
            Section("RadioButton", "Satu pilihan dalam satu grup.", Row(new RadioButton("Pagi", "waktu") { IsChecked = true }, new RadioButton("Siang", "waktu"), new RadioButton("Sore", "waktu"))),
            Section("ToggleSwitch", "", Row(new ToggleSwitch("Penyiraman otomatis", true), new ToggleSwitch("Lampu tumbuh", false))),
            Section("Slider  ·  ProgressBar", "Slider dengan tick dan snap mengikat nilai progress bar.", new StackPanel { Spacing = 8, Children = { slider, bar, busy } }),
            Section("NumericUpDown", "Panah atas/bawah atau tombol +/-.", new NumericUpDown { Minimum = 0, Maximum = 50, Value = 12, Width = 160, HorizontalAlignment = HorizontalAlignment.Left }));
    }

    static Control Text(out AutoCompleteBox auto, out ComboBox combo)
    {
        auto = new AutoCompleteBox { Watermark = "Cari tanaman...", Width = 260, HorizontalAlignment = HorizontalAlignment.Left };
        foreach (var p in Plants) auto.ItemsSource.Add(p.Name);
        combo = new ComboBox(new[] { "Araceae", "Orchidaceae", "Nepenthaceae", "Dipterocarpaceae" }, 1) { Width = 220, HorizontalAlignment = HorizontalAlignment.Left };
        return Page(
            Section("TextBox", "Satu baris, kata sandi, dan banyak baris.", new StackPanel
            {
                Spacing = 8,
                Children =
                {
                    new TextBox { Watermark = "Nama spesimen", Width = 260, HorizontalAlignment = HorizontalAlignment.Left },
                    new TextBox { Watermark = "Kata sandi", PasswordChar = '*', Width = 260, HorizontalAlignment = HorizontalAlignment.Left },
                    new TextBox { AcceptsReturn = true, Text = "Catatan lapangan:\nDaun lebar, batang berduri.", Width = 320, HorizontalAlignment = HorizontalAlignment.Left },
                },
            }),
            Section("AutoCompleteBox", "Saran muncul saat mengetik; panah dan Enter memilih.", auto),
            Section("ComboBox", "", combo),
            Section("CalendarDatePicker", "", new CalendarDatePicker { Width = 200, HorizontalAlignment = HorizontalAlignment.Left }));
    }

    static Control Lists(out ListBox list, out DataGrid grid)
    {
        list = new ListBox { ItemsSource = Plants, Height = 150 };
        list.ItemTemplate = o =>
        {
            var p = (Plant)o;
            var r = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            r.Children.Add(new GlyphTile(p.Glyph, p.Glyph == IconKind.Sun ? 0xE9B44C : 0x7CC4A4, 26));
            var t = new StackPanel();
            t.Children.Add(new TextBlock(p.Name));
            t.Children.Add(new TextBlock(p.Latin).WithClass("faint"));
            r.Children.Add(t);
            return r;
        };
        list.SelectedIndex = 0;
        var tree = new TreeView { Height = 150 };
        var root = new TreeViewItem("Kebun Raya Bogor", IconKind.Home);
        var a = new TreeViewItem("Taman Meksiko", IconKind.Folder).Add(new TreeViewItem("Kaktus", IconKind.Leaf)).Add(new TreeViewItem("Agave", IconKind.Leaf));
        var b = new TreeViewItem("Taman Anggrek", IconKind.Folder).Add(new TreeViewItem("Phalaenopsis", IconKind.Sun));
        root.Add(a).Add(b);
        root.IsExpanded = true;
        a.IsExpanded = true;
        tree.Add(root);
        grid = new DataGrid { Height = 170 };
        grid.AddColumn(new DataGridColumn("Nama", o => ((Plant)o).Name, "3*"));
        grid.AddColumn(new DataGridColumn("Famili", o => ((Plant)o).Family, "3*"));
        grid.AddColumn(new DataGridColumn("Tinggi", o => ((Plant)o).Height + " m", "70") { SortKey = o => ((Plant)o).Height, Alignment = TextAlignment.Right });
        grid.ItemsSource = Plants;
        return Page(
            Section("ListBox + ItemTemplate", "Item dibangun dari data lewat template.", list),
            Section("TreeView", "Klik panah atau tekan kanan/kiri untuk membuka/menutup.", tree),
            Section("DataGrid", "Klik judul kolom untuk mengurutkan.", grid));
    }

    static Control Layout()
    {
        var g = new Grid { ColumnDefinitions = "120,6,*", Height = 90 };
        g.Children.Add(new Border { Child = new TextBlock("Kolom 1") { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center } }.WithClass("inset").Col(0));
        g.Children.Add(new GridSplitter().Col(1));
        g.Children.Add(new Border { Child = new TextBlock("Seret pemisah  <->") { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center } }.WithClass("inset").Col(2));
        var wrap = new WrapPanel { ItemSpacing = 6, LineSpacing = 6 };
        foreach (var p in Plants) wrap.Children.Add(new Border { Child = new TextBlock(p.Name), Padding = new Thickness(10, 4), CornerRadius = 12, Background = Theme.SurfaceHi });
        var uni = new UniformGrid { Columns = 4, Spacing = 6 };
        for (int i = 1; i <= 8; i++) uni.Children.Add(new Border { Child = new TextBlock(i.ToString()) { HorizontalAlignment = HorizontalAlignment.Center }, Padding = new Thickness(0, 8) }.WithClass("inset"));
        var cards = new UniformGrid { Columns = 3, Spacing = 10 };
        cards.Children.Add(Kit.Stat("Spesimen", new TextBlock("15.000+")));
        cards.Children.Add(Kit.Stat("Luas", new TextBlock("87 ha")));
        cards.Children.Add(Kit.Stat("Berdiri", new TextBlock("1817")));
        return Page(
            Section("Grid  ·  GridSplitter", "Kolom Auto, piksel, dan bintang; pemisah mengubah lebar kolom.", g),
            Section("WrapPanel", "", wrap),
            Section("UniformGrid", "", uni),
            Section("Border.card", "Kartu dengan bayangan lembut.", cards),
            Section("Expander", "", new Expander("Sejarah", new TextBlock("Didirikan 18 Mei 1817 oleh C.G.C. Reinwardt sebagai 's Lands Plantentuin te Buitenzorg.") { TextWrapping = TextWrapping.Wrap })));
    }

    static Control Overlays(Window w, WindowNotificationManager? notes)
    {
        var flyoutButton = new Button("Buka flyout") { Flyout = new Flyout(new StackPanel { Spacing = 6, Children = { new TextBlock("Flyout").WithClass("strong"), new TextBlock("Konten bebas di dalam popup.").WithClass("muted") } }) };
        var dialogButton = new Button("Tampilkan dialog", _ => Dialog.Show(w, "Pindahkan tanaman?", "Tanaman akan dipindah ke rumah kaca 2.", "Pindahkan", "Batal")).WithClass("accent");
        var toast = new Button("Notifikasi", _ => notes?.Show(new Notification("Penyiraman selesai", "Zona B disiram 12 menit.", NotificationType.Success)));
        var warn = new Button("Peringatan", _ => notes?.Show(new Notification("Kelembapan rendah", "Rumah kaca 1 di bawah 40%.", NotificationType.Warning)));
        var hold = new Border { Child = new TextBlock("Tekan dan tahan di sini untuk menu konteks"), Padding = new Thickness(14, 18) }.WithClass("inset")
            .WithContextMenu(new MenuFlyout().Add(new MenuItem("Salin", null, "", IconKind.File)).Add(new MenuItem("Hapus", null, "", IconKind.Trash)));
        return Page(
            Section("Flyout  ·  Dialog", "", Row(flyoutButton, dialogButton)),
            Section("Notifikasi", "Kartu kecil di pojok kanan bawah, hilang sendiri.", Row(toast, warn)),
            Section("ToolTip", "Arahkan penunjuk dan diam sebentar.", Row(new Button("Arahkan ke sini").WithTip("Tooltip: petunjuk singkat."), new Icon(IconKind.Info, 18).WithTip("Ikon juga bisa diberi tooltip."))),
            Section("ContextMenu", "", hold),
            Section("Menu", "", new Menu().Add(new MenuItem("Berkas").Add(new MenuItem("Baru")).Add(new MenuItem("Buka"))).Add(new MenuItem("Bantuan").Add(new MenuItem("Tentang")))));
    }

    public sealed class Parts
    {
        public Window Window = null!;
        public ListBox Nav = null!;
        public Slider Slider = null!;
        public ProgressBar Bar = null!;
        public AutoCompleteBox Auto = null!;
        public ComboBox Combo = null!;
        public ListBox List = null!;
        public DataGrid Grid = null!;
        public ToggleSwitch Dark = null!;
        public SplitView Split = null!;
    }

    public static Parts Build(bool headless = false)
    {
        var parts = new Parts();
        var w = new Window("Galeri Kontrol", 660, 480, headless);
        parts.Window = w;
        var notes = headless ? null : new WindowNotificationManager(w);
        var pages = new List<(string, IconKind, Control)>
        {
            ("Tombol", IconKind.Play, Buttons(w)),
            ("Pilihan", IconKind.Check, Choices(out parts.Slider, out parts.Bar)),
            ("Teks", IconKind.File, Text(out parts.Auto, out parts.Combo)),
            ("Daftar", IconKind.Menu, Lists(out parts.List, out parts.Grid)),
            ("Tata letak", IconKind.Grid, Layout()),
            ("Lapisan", IconKind.Info, Overlays(w, notes)),
        };
        var host = new Border();
        var nav = new ListBox { BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0, Padding = new Thickness(6) };
        var names = new List<string>();
        foreach (var p in pages) names.Add(p.Item1);
        nav.ItemsSource = names;
        nav.ItemTemplate = o =>
        {
            int i = names.IndexOf((string)o);
            var r = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            r.Children.Add(new Icon(pages[i].Item2, 14) { VerticalAlignment = VerticalAlignment.Center });
            r.Children.Add(new TextBlock((string)o));
            return r;
        };
        nav.SelectionChanged += l => { if (l.SelectedIndex >= 0) host.Child = pages[l.SelectedIndex].Item3; };
        nav.SelectedIndex = 0;
        parts.Nav = nav;

        var dark = new ToggleSwitch("", true) { OnContent = "Gelap", OffContent = "Terang", Padding = new Thickness(0) };
        dark.IsCheckedChanged += t => Application.Current?.SetTheme(t.IsChecked ? ThemeVariant.Dark : ThemeVariant.Light);
        parts.Dark = dark;

        var paneHead = new StackPanel { Margin = new Thickness(14, 14, 10, 10), Spacing = 2 };
        paneHead.Children.Add(new TextBlock("BUITENZORG.UI").WithClass("eyebrow"));
        paneHead.Children.Add(new TextBlock("Galeri").WithClass("h2"));
        var pane = new DockPanel();
        pane.Children.Add(paneHead.DockTo(Dock.Top));
        pane.Children.Add(new Border { Child = dark, Padding = new Thickness(14, 10) }.DockTo(Dock.Bottom));
        pane.Children.Add(nav);
        var split = new SplitView { OpenPaneLength = 170, CompactPaneLength = 0, PaneBackground = Theme.Surface, Pane = pane, Content = host };
        parts.Split = split;
        w.Content = split;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        nav.Focus();
        return parts;
    }

    static Control? Find(Window w, Func<Control, bool> pred)
    {
        foreach (var d in w.Descendants()) if (pred(d)) return d;
        return null;
    }

    /// <summary>The toolkit test: every page renders, and the interactive pieces work through real routing.</summary>
    public static Checks SelfTest(bool headless = true)
    {
        var c = new Checks("Galeri");
        var p = Build(headless);
        var w = p.Window;
        int totalOps = 0;
        for (int i = 0; i < p.Nav.ItemCount; i++)
        {
            p.Nav.SelectedIndex = i;
            w.RenderFrame();
            totalOps += w.LastFrameOps;
            c.That(w.LastFrameOps > 25, "page " + i + " drew " + w.LastFrameOps + " ops");
        }
        // Tombol: DropDownButton opens its menu flyout; Escape closes it
        p.Nav.SelectedIndex = 0;
        w.UpdateLayout();
        var dd = (DropDownButton)Find(w, d => d is DropDownButton)!;
        w.SimulateClick(dd);
        c.That(w.Popups.Count == 1, "dropdown opened a flyout");
        w.SimulateKeys(Keys.Escape.ToString());
        c.That(w.Popups.Count == 0, "Escape closed it");
        // light dismiss: open again, click outside
        w.SimulateClick(dd);
        w.SimulateClickAt(w.ClientWidth - 10, w.ClientHeight - 10);
        c.That(w.Popups.Count == 0, "a click outside closed it");
        // Pilihan: slider keys drive the bound progress bar (snapped to ticks)
        p.Nav.SelectedIndex = 1;
        w.UpdateLayout();
        p.Slider.Focus();
        w.SimulateKeys(Keys.Right.ToString() + Keys.Right);
        c.That(p.Slider.Value == 60 && p.Bar.Value == 60, "slider snapped to 60 and drove the progress bar (" + p.Slider.Value + ")");
        var radios = new List<RadioButton>();
        foreach (var d in w.Descendants()) if (d is RadioButton r) radios.Add(r);
        w.SimulateClick(radios[2]);
        c.That(radios[2].IsChecked && !radios[0].IsChecked, "radio group keeps one choice");
        // Avalonia selector semantics: "ToggleButton:checked" must not reach a RadioButton (a subclass)
        c.That(radios[2].Foreground != Theme.OnAccent, "exact-type selector: checked radio keeps its text color");
        // Teks: autocomplete suggests and picks; combo opens and selects through its popup
        p.Nav.SelectedIndex = 2;
        w.UpdateLayout();
        p.Auto.Focus();
        w.SimulateKeys("kan");
        c.That(p.Auto.IsDropDownOpen, "autocomplete suggested");
        w.SimulateKeys(Keys.Enter.ToString());
        c.Equal("Kantong semar", p.Auto.Text, "autocomplete picked with Enter");
        w.SimulateClick(p.Combo);
        c.That(p.Combo.IsDropDownOpen, "combo opened");
        var item = Find(w, d => d is ListBoxItem li && li.Content is TextBlock t && t.Text == "Nepenthaceae");
        if (item != null) w.SimulateClick(item);
        c.That(p.Combo.SelectedIndex == 2 && !p.Combo.IsDropDownOpen, "combo item clicked (" + p.Combo.SelectedIndex + ")");
        // Daftar: list keyboard, grid sort
        p.Nav.SelectedIndex = 3;
        w.UpdateLayout();
        p.List.Focus();
        w.SimulateKeys(Keys.Down.ToString() + Keys.Down);
        c.That(p.List.SelectedIndex == 2, "list moved with the arrow keys");
        c.That(Find(w, d => d is GlyphTile) != null, "an ItemTemplate set after ItemsSource still applies");
        var header = Find(w, d => d is DataGridColumnHeader)!;
        header.FindAncestor<ScrollViewer>()?.ScrollIntoView(p.Grid);
        w.UpdateLayout();
        w.SimulateClick(header);
        p.Grid.Select(0);
        c.Equal("Anggrek bulan", ((Plant)p.Grid.SelectedItem!).Name, "grid sorted by name");
        // Lapisan: a dialog is modal; its default button closes it
        p.Nav.SelectedIndex = 5;
        w.UpdateLayout();
        var dlgButton = Find(w, d => d is Button b && (b.Content as string) == "Tampilkan dialog")!;
        w.SimulateClick(dlgButton);
        c.That(w.Popups.Count == 1, "dialog opened");
        w.SimulateKeys(Keys.Enter.ToString());
        c.That(w.Popups.Count == 0, "Enter pressed the dialog's default button");
        // Tab walks focus through the focusable controls
        var seen = new List<Control>();
        for (int i = 0; i < 6; i++) { w.MoveFocus(); if (w.FocusedControl != null && !seen.Contains(w.FocusedControl)) seen.Add(w.FocusedControl); }
        c.That(seen.Count >= 5, "Tab visited " + seen.Count + " controls");
        // theme switch restyles everything
        int before = w.Background;
        w.SimulateClick(p.Dark);
        w.UpdateLayout();
        c.That(w.Background != before && Theme.Variant == ThemeVariant.Light, "light theme applied");
        w.SimulateClick(p.Dark);
        w.UpdateLayout();
        c.That(Theme.Variant == ThemeVariant.Dark && w.Background == before, "dark theme back");
        w.RenderFrame();
        c.That(totalOps > 300, "gallery drew " + totalOps + " ops in all");
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
            var c = GalleryApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var parts = GalleryApp.Build();
        app.Run(parts.Window);
        return 0;
    }
}
