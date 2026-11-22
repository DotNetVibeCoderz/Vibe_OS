// Pengaturan -- the desktop settings app: theme (+ animations, large cursor),
// wallpaper, screensaver (+ idle delay), sound (volume, test tone) and user
// accounts (add, delete, change password, profile picture), each a page
// behind a side navigation. Changes apply at once: the kernel recomposes
// the desktop (SETTING syscall). Buitenzorg.UI + Buitenzorg.App.Settings / Audio.

using Buitenzorg.UI;
using AppSettings = Buitenzorg.App.Settings;
using AppAudio = Buitenzorg.App.Audio;
using Users = Buitenzorg.App.Users;
using UserAccount = Buitenzorg.App.UserAccount;
using AppFiles = Buitenzorg.App.Files;
using Buitenzorg.Text;
using Buitenzorg.Net;
using System.Text;

namespace MSettings;

public static class SettingsApp
{
    /// <summary>Friendly names for the kernel's theme ids.</summary>
    public static string ThemeLabel(string id) => id switch
    {
        "kebun" => "Kebun (bawaan)",
        "dark" => "Gelap",
        "light" => "Terang",
        "neo-brutalism" or "neobrutalism" => "Neo Brutalism",
        "clean" => "Bersih",
        "material" => "Material",
        "bento" => "Bento",
        "classic-linux" => "Linux klasik",
        "classic-windows" => "Windows klasik",
        "sun" => "Sun",
        "beos" => "BeOS",
        _ => id,
    };

    public static string SaverLabel(string id) => id switch
    {
        "starfield" => "Bintang",
        "mystify" => "Mystify",
        "pipes" => "Pipa",
        "marquee" => "Teks berjalan",
        "bouncing" => "Logo memantul",
        "blank" => "Layar hitam",
        "off" => "Mati",
        _ => id,
    };

    public sealed class Parts
    {
        public ListBox Nav = null!, Themes = null!, Wallpapers = null!, Savers = null!;
        public Slider Delay = null!, Volume = null!;
        public ToggleSwitch Animations = null!, Cursor = null!;
        public ComboBox Resolution = null!, Scale = null!, Icons = null!;
        public TextBlock DelayText = null!, SoundStatus = null!;
        // Pengguna
        public ListBox Accounts = null!;
        public TextBox AdminName = null!, AdminPassword = null!;
        public TextBlock UserStatus = null!;
        public Func<string, string, bool, string?> AddUser = null!;
        public Func<UserAccount, string?> DeleteUser = null!;
        public Func<UserAccount, string, string, string?> SetPassword = null!;
        public Func<UserAccount, string, string?> SetAvatar = null!;
        // Fon
        public ListBox UiFonts = null!, MonoFonts = null!, Catalog = null!;
        public TextBlock FontStatus = null!;
        public FontPreview Preview = null!;
        public Func<string, string?> InstallFont = null!;
        public Func<string, string?> RemoveFont = null!;
        public string CatalogPath = FontCatalog.DefaultPath;
    }

    /// <summary>Samples of the chosen fonts, drawn by the kernel's font engine.</summary>
    public sealed class FontPreview : Control
    {
        public string UiFamily = "", MonoFamily = "";
        public FontPreview() { Height = 118; }
        protected override Size MeasureOverride(Size a) => new Size(Math.Min(a.Width, 420), 118);
        public override void Render(DrawingContext dc)
        {
            var r = Bounds;
            dc.FillRoundRect(r, 8, Theme.SurfaceLo);
            int ui = Fonts.Find(UiFamily), uiBold = Fonts.Find(UiFamily, true), mono = Fonts.Find(MonoFamily);
            int x = r.X + 14, y = r.Y + 10;
            if (uiBold > 0) dc.TextIn(x, y, "Kebun Raya Bogor", Theme.Text, uiBold, 24); else dc.Text(x, y, "Kebun Raya Bogor", Theme.Text, 2);
            if (ui > 0) dc.TextIn(x, y + 34, "Rusa tutul melompati pagar kebun, 1817.", Theme.TextMuted, ui, 15);
            else dc.Text(x, y + 34, "Rusa tutul melompati pagar kebun, 1817.", Theme.TextMuted, 0);
            if (mono > 0) dc.TextIn(x, y + 64, "fn main() { cetak(\"halo {}\", 42); }", Theme.Accent, mono, 14);
            else dc.MonoText(x, y + 64, "fn main() { cetak(\"halo {}\", 42); }", Theme.Accent, 0);
        }
    }

    /// <summary>The "Fon" page: the desktop and terminal font, a preview, and the catalog
    /// of fonts to download (Buitenzorg.Text.FontInstaller).</summary>
    static Control FontsPage(Parts parts)
    {
        var uiNames = AppSettings.Choices(AppSettings.UiFontKey);
        var monoNames = AppSettings.Choices(AppSettings.MonoFontKey);
        parts.Preview = new FontPreview();
        void Sync()
        {
            int u = AppSettings.UiFont, m = AppSettings.MonoFont;
            parts.Preview.UiFamily = u > 0 && u < uiNames.Count ? uiNames[u] : "";
            parts.Preview.MonoFamily = m > 0 && m < monoNames.Count ? monoNames[m] : "";
            parts.Preview.InvalidateVisual();
        }
        parts.UiFonts = Choice(uiNames, AppSettings.UiFont, AppSettings.UiFontKey);
        parts.UiFonts.Height = 96;
        parts.UiFonts.SelectionChanged += _ => Sync();
        parts.MonoFonts = Choice(monoNames, AppSettings.MonoFont, AppSettings.MonoFontKey);
        parts.MonoFonts.Height = 72;
        parts.MonoFonts.SelectionChanged += _ => Sync();
        Sync();

        var catalog = FontCatalog.Load(parts.CatalogPath);
        var names = FontCatalog.Names(catalog);
        parts.FontStatus = new TextBlock(names.Count + " fon di katalog -- unduh lewat HTTPS, diperiksa SHA-256.").WithClass("faint");
        parts.Catalog = new ListBox { Height = 120 };
        void Refill()
        {
            var labels = new List<string>();
            foreach (var n in names)
            {
                var st = FontCatalog.Styles(catalog, n);
                string state = FontInstaller.IsInstalled(st[0].Family) ? "terpasang" : (st[0].Size / 1024) + " KiB";
                labels.Add(st[0].Family + "  -  " + st[0].Licence + "  -  " + state);
            }
            int sel = parts.Catalog.SelectedIndex;
            parts.Catalog.ItemsSource = labels;
            parts.Catalog.SelectedIndex = sel;
        }
        Refill();
        parts.InstallFont = name =>
        {
            parts.FontStatus.Text = "mengunduh " + name + " ...";
            var err = FontInstaller.Install(name, parts.CatalogPath, true, line => parts.FontStatus.Text = line);
            parts.FontStatus.Text = err ?? (name + " terpasang: pilih di daftar fon di atas.");
            uiNames = AppSettings.Choices(AppSettings.UiFontKey);
            monoNames = AppSettings.Choices(AppSettings.MonoFontKey);
            parts.UiFonts.ItemsSource = uiNames;
            parts.UiFonts.SelectedIndex = AppSettings.UiFont;
            parts.MonoFonts.ItemsSource = monoNames;
            parts.MonoFonts.SelectedIndex = AppSettings.MonoFont;
            Refill();
            return err;
        };
        parts.RemoveFont = name =>
        {
            var err = FontInstaller.Remove(name, parts.CatalogPath);
            parts.FontStatus.Text = err ?? (name + " dihapus.");
            uiNames = AppSettings.Choices(AppSettings.UiFontKey);
            monoNames = AppSettings.Choices(AppSettings.MonoFontKey);
            parts.UiFonts.ItemsSource = uiNames;
            parts.UiFonts.SelectedIndex = AppSettings.UiFont;
            parts.MonoFonts.ItemsSource = monoNames;
            parts.MonoFonts.SelectedIndex = AppSettings.MonoFont;
            Refill();
            return err;
        };
        var install = new Button(new TextBlock("Pasang"));
        install.Click += _ =>
        {
            int i = parts.Catalog.SelectedIndex;
            if (i < 0) { parts.FontStatus.Text = "pilih fon di katalog dulu"; return; }
            var st = FontCatalog.Styles(catalog, names[i]);
            if (!FontCatalog.IsFree(st[0].Licence))
            {
                parts.FontStatus.Text = st[0].Family + " berlisensi " + st[0].Licence + ": dari Terminal, 'font install " + names[i] + " --accept-license' setelah membaca syaratnya.";
                return;
            }
            parts.InstallFont(names[i]);
        };
        var remove = new Button(new TextBlock("Hapus")).WithClass("subtle");
        remove.Click += _ =>
        {
            int i = parts.Catalog.SelectedIndex;
            if (i >= 0) parts.RemoveFont(names[i]);
        };
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        row.Children.Add(install);
        row.Children.Add(remove);
        var body = new StackPanel { Orientation = Orientation.Vertical, Spacing = 6 };
        body.Children.Add(new TextBlock("Fon desktop").WithClass("strong"));
        body.Children.Add(parts.UiFonts);
        body.Children.Add(new TextBlock("Fon terminal dan teks").WithClass("strong"));
        body.Children.Add(parts.MonoFonts);
        body.Children.Add(parts.Preview);
        body.Children.Add(new TextBlock("Katalog fon").WithClass("strong"));
        body.Children.Add(parts.Catalog);
        body.Children.Add(row);
        body.Children.Add(parts.FontStatus);
        var scroll = new ScrollViewer { Content = body, Height = 330 };
        return Page("Fon", "Fon TrueType untuk desktop dan terminal. Berlaku langsung.", scroll);
    }

    static readonly Dictionary<string, Bitmap?> _avatarCache = new Dictionary<string, Bitmap?>();

    /// <summary>A profile picture (cached), null when the file is missing.</summary>
    public static Bitmap? Avatar(string path)
    {
        if (!_avatarCache.TryGetValue(path, out var b))
        {
            b = Bitmap.FromBmp(AppFiles.ReadAllBytes(path));
            _avatarCache[path] = b;
        }
        return b;
    }

    static Control AvatarView(string path, int size)
    {
        var bmp = Avatar(path);
        if (bmp != null) return new Image { Source = bmp, Width = size, Height = size, VerticalAlignment = VerticalAlignment.Center };
        return new GlyphTile(IconKind.Home, 0x7CC4A4, size) { VerticalAlignment = VerticalAlignment.Center };
    }

    /// <summary>The "Pengguna" page: accounts, with an administrator's password for changes.</summary>
    static Control UsersPage(Window w, Parts parts)
    {
        parts.AdminName = new TextBox { Text = Users.DefaultAdministrator, Width = 150 };
        parts.AdminPassword = new TextBox { PasswordChar = '*', Watermark = "kata sandi", Width = 150 };
        parts.UserStatus = new TextBlock("Perubahan akun memerlukan kata sandi administrator.") { TextWrapping = TextWrapping.Wrap }.WithClass("faint");
        var accounts = new ObservableCollection<UserAccount>();
        void Reload(string? select = null)
        {
            accounts.ResetWith(Users.List());
            for (int i = 0; i < accounts.Count; i++) if (accounts[i].Name == select) parts.Accounts.SelectedIndex = i;
            if (parts.Accounts.SelectedIndex < 0 && accounts.Count > 0) parts.Accounts.SelectedIndex = 0;
        }
        parts.Accounts = new ListBox { ItemsSource = accounts, Height = 170 };
        parts.Accounts.ItemTemplate = o =>
        {
            var u = (UserAccount)o;
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            row.Children.Add(AvatarView(u.Avatar, 36));
            var text = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
            text.Children.Add(new TextBlock(u.Name).WithClass("strong"));
            text.Children.Add(new TextBlock(u.IsAdministrator ? "Administrator" : "Pengguna biasa").WithClass("faint"));
            row.Children.Add(text);
            return row;
        };
        string Admin() => parts.AdminName.Text.Trim();
        string AdminPw() => parts.AdminPassword.Text;
        string? Report(bool ok, string done, string failed) { parts.UserStatus.Text = ok ? done : failed; return ok ? null : failed; }

        parts.AddUser = (name, pw, admin) =>
        {
            if (!Users.IsValidName(name)) return Report(false, "", "Nama: 1-16 huruf kecil/angka/_-. diawali huruf.");
            if (!Users.IsValidPassword(pw)) return Report(false, "", "Kata sandi 4-64 karakter.");
            bool ok = Users.Add(Admin(), AdminPw(), name, pw, admin);
            if (ok) Reload(name);
            return Report(ok, "Pengguna " + name + " ditambahkan.", "Tidak bisa menambah: periksa kata sandi administrator (atau nama sudah ada).");
        };
        parts.DeleteUser = u =>
        {
            bool ok = Users.Remove(Admin(), AdminPw(), u.Name);
            if (ok) Reload();
            return Report(ok, "Pengguna " + u.Name + " dihapus.", "Tidak bisa menghapus " + u.Name + " (kata sandi administrator salah, atau administrator terakhir).");
        };
        parts.SetPassword = (u, current, next) =>
        {
            if (!Users.IsValidPassword(next)) return Report(false, "", "Kata sandi baru 4-64 karakter.");
            bool ok = current.Length > 0 ? Users.ChangePassword(u.Name, current, next) : Users.ResetPassword(Admin(), AdminPw(), u.Name, next);
            return Report(ok, "Kata sandi " + u.Name + " diganti.", current.Length > 0 ? "Kata sandi sekarang salah." : "Kata sandi administrator salah.");
        };
        parts.SetAvatar = (u, path) =>
        {
            bool ok = Users.SetAvatar(Admin(), AdminPw(), u.Name, path);
            if (ok) Reload(u.Name);
            return Report(ok, "Foto profil " + u.Name + " diganti.", "Tidak bisa mengganti foto: kata sandi " + Admin() + " salah.");
        };

        UserAccount? Selected() => parts.Accounts.SelectedItem as UserAccount;
        void AddDialog()
        {
            var name = new TextBox { Width = 260, Watermark = "mis. budi" };
            var pw = new TextBox { Width = 260, PasswordChar = '*', Watermark = "kata sandi" };
            var admin = new CheckBox("Jadikan administrator");
            var body = new StackPanel { Spacing = 8 };
            body.Children.Add(new TextBlock("Nama pengguna").WithClass("faint"));
            body.Children.Add(name);
            body.Children.Add(new TextBlock("Kata sandi").WithClass("faint"));
            body.Children.Add(pw);
            body.Children.Add(admin);
            Dialog.Show(w, "Tambah pengguna", body, "Tambah", "Batal", go => { if (go) parts.AddUser(name.Text.Trim(), pw.Text, admin.IsChecked); });
            name.Focus();
        }
        void PasswordDialog()
        {
            if (Selected() is not UserAccount u) return;
            var cur = new TextBox { Width = 260, PasswordChar = '*', Watermark = "kosongkan bila memakai kata sandi administrator" };
            var next = new TextBox { Width = 260, PasswordChar = '*' };
            var again = new TextBox { Width = 260, PasswordChar = '*' };
            var body = new StackPanel { Spacing = 8 };
            body.Children.Add(new TextBlock("Kata sandi sekarang").WithClass("faint"));
            body.Children.Add(cur);
            body.Children.Add(new TextBlock("Kata sandi baru").WithClass("faint"));
            body.Children.Add(next);
            body.Children.Add(new TextBlock("Ulangi kata sandi baru").WithClass("faint"));
            body.Children.Add(again);
            Dialog.Show(w, "Ganti kata sandi " + u.Name, body, "Ganti", "Batal", go =>
            {
                if (!go) return;
                if (next.Text != again.Text) { parts.UserStatus.Text = "Kata sandi baru tidak sama."; return; }
                parts.SetPassword(u, cur.Text, next.Text);
            });
            cur.Focus();
        }
        void DeleteDialog()
        {
            if (Selected() is not UserAccount u) return;
            Dialog.Show(w, "Hapus " + u.Name + "?", "Akun " + u.Name + " akan dihapus. Berkasnya di /disk tetap ada.", "Hapus", "Batal",
                go => { if (go) parts.DeleteUser(u); });
        }
        void AvatarDialog()
        {
            if (Selected() is not UserAccount u) return;
            var grid = new WrapPanel();
            Dialog? dlg = null;
            foreach (var path in Users.Avatars)
            {
                var b = new Button(AvatarView(path, 64), _ => { dlg?.Close(); parts.SetAvatar(u, path); }) { Padding = new Thickness(4), Margin = new Thickness(4), Tag = path };
                b.Classes.Add("subtle");
                grid.Children.Add(b);
            }
            dlg = Dialog.Show(w, "Foto profil " + u.Name, grid, "Batal", null, null);
        }

        var creds = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        creds.Children.Add(new TextBlock("Administrator") { VerticalAlignment = VerticalAlignment.Center }.WithClass("faint"));
        creds.Children.Add(parts.AdminName);
        creds.Children.Add(parts.AdminPassword);
        var buttons = new WrapPanel();
        foreach (var (label, act) in new (string, Action)[] { ("Tambah", AddDialog), ("Hapus", DeleteDialog), ("Ganti kata sandi", PasswordDialog), ("Foto profil", AvatarDialog) })
        {
            var b = new Button(label, _ => act()) { Margin = new Thickness(0, 0, 6, 6), Tag = label };
            if (label == "Tambah") b.Classes.Add("accent");
            buttons.Children.Add(b);
        }
        Reload();
        return Page("Pengguna", "Akun untuk masuk (layar login, SSH). Bawaan: administrator / ganteng@ku -- segera ganti.",
            creds, parts.Accounts, buttons, parts.UserStatus);
    }

    static ListBox Choice(List<string> labels, int current, int key, Func<int, string>? after = null)
    {
        var list = new ListBox { ItemsSource = labels, Height = 300 };
        list.SelectedIndex = current;
        list.SelectionChanged += l =>
        {
            int i = l.SelectedIndex;
            if (i >= 0 && i != AppSettings.Get(key)) AppSettings.Set(key, i);
        };
        return list;
    }

    static StackPanel Page(string title, string hint, params Control[] body)
    {
        var p = new StackPanel { Orientation = Orientation.Vertical, Spacing = 8 };
        p.Children.Add(new TextBlock(title).WithClass("h2"));
        p.Children.Add(new TextBlock(hint).WithClass("faint"));
        foreach (var c in body) p.Children.Add(c);
        return p;
    }

    public static (Window, Parts) Build(bool headless = false)
    {
        var parts = new Parts();
        var w = new Window("Pengaturan", 640, 470, headless);

        // Tampilan
        var themeIds = AppSettings.Choices(AppSettings.ThemeKey);
        parts.Themes = Choice(themeIds.ConvertAll(ThemeLabel), AppSettings.Theme, AppSettings.ThemeKey);
        parts.Themes.Height = 150;
        parts.Animations = new ToggleSwitch("", AppSettings.Animations) { OnContent = "Animasi jendela nyala", OffContent = "Animasi jendela mati", Padding = new Thickness(0) };
        parts.Animations.IsCheckedChanged += t => AppSettings.Animations = t.IsChecked;
        parts.Cursor = new ToggleSwitch("", AppSettings.LargeCursor) { OnContent = "Kursor besar", OffContent = "Kursor normal", Padding = new Thickness(0) };
        parts.Cursor.IsCheckedChanged += t => AppSettings.LargeCursor = t.IsChecked;
        parts.Resolution = new ComboBox(AppSettings.Choices(AppSettings.ResolutionKey), AppSettings.Resolution) { Width = 130 };
        parts.Resolution.SelectionChanged += cb => { if (cb.SelectedIndex >= 0 && cb.SelectedIndex != AppSettings.Resolution) AppSettings.Resolution = cb.SelectedIndex; };
        parts.Scale = new ComboBox(AppSettings.Choices(AppSettings.UiScaleKey), AppSettings.UiScale) { Width = 110 };
        parts.Scale.SelectionChanged += cb => { if (cb.SelectedIndex >= 0 && cb.SelectedIndex != AppSettings.UiScale) AppSettings.UiScale = cb.SelectedIndex; };
        var screenRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        screenRow.Children.Add(new TextBlock("Resolusi"));
        screenRow.Children.Add(parts.Resolution);
        screenRow.Children.Add(new TextBlock("Ukuran teks"));
        screenRow.Children.Add(parts.Scale);
        parts.Icons = new ComboBox(AppSettings.Choices(AppSettings.IconPackKey), AppSettings.IconPack) { Width = 110 };
        parts.Icons.SelectionChanged += cb => { if (cb.SelectedIndex >= 0 && cb.SelectedIndex != AppSettings.IconPack) AppSettings.IconPack = cb.SelectedIndex; };
        var iconRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        iconRow.Children.Add(new TextBlock("Paket ikon"));
        iconRow.Children.Add(parts.Icons);
        var look = Page("Tampilan", "Tema dan ukuran teks berlaku langsung; resolusi setelah komputer dimulai ulang.", parts.Themes, screenRow, iconRow, parts.Animations, parts.Cursor);

        // Wallpaper
        var wpNames = AppSettings.Choices(AppSettings.WallpaperKey);
        var wpText = AppSettings.Choices(AppSettings.WallpaperKey, descriptions: true);
        var wpLabels = new List<string>();
        for (int i = 0; i < wpNames.Count; i++) wpLabels.Add(wpNames[i] + (i < wpText.Count && wpText[i].Length > 0 ? "  -  " + wpText[i] : ""));
        parts.Wallpapers = Choice(wpLabels, AppSettings.Wallpaper, AppSettings.WallpaperKey);
        var paper = Page("Wallpaper", "Delapan wallpaper resmi Buitenzorg OS.", parts.Wallpapers);

        // Screensaver
        var saverIds = AppSettings.Choices(AppSettings.ScreensaverKey);
        parts.Savers = Choice(saverIds.ConvertAll(SaverLabel), AppSettings.Screensaver, AppSettings.ScreensaverKey);
        parts.Savers.Height = 200;
        int delay = Math.Max(5, AppSettings.ScreensaverDelaySeconds);
        parts.DelayText = new TextBlock("Mulai setelah " + delay + " detik tanpa input");
        parts.Delay = new Slider { Minimum = 5, Maximum = 600, Value = Math.Min(600, delay), TickFrequency = 5, IsSnapToTickEnabled = true, Width = 320 };
        parts.Delay.ValueChanged += s =>
        {
            int v = (int)s.Value;
            parts.DelayText.Text = "Mulai setelah " + v + " detik tanpa input";
            AppSettings.ScreensaverDelaySeconds = v;
        };
        var saver = Page("Screensaver", "Muncul saat desktop menganggur; gerakkan mouse atau tekan tombol untuk kembali.", parts.Savers, parts.DelayText, parts.Delay);

        // Suara
        bool present = AppAudio.IsPresent;
        parts.SoundStatus = new TextBlock(present
            ? "Kartu suara: " + AppAudio.SampleRate + " Hz, stereo"
            : "Tidak ada kartu suara. (Raspberry Pi: audio HDMI / jack belum didukung.)");
        parts.Volume = new Slider { Minimum = 0, Maximum = 100, Value = present ? AppAudio.Volume : 0, TickFrequency = 5, IsSnapToTickEnabled = true, Width = 320, IsEnabled = present };
        var volText = new TextBlock("Volume " + (int)parts.Volume.Value + "%");
        parts.Volume.ValueChanged += s => { AppAudio.Volume = (int)s.Value; volText.Text = "Volume " + (int)s.Value + "%"; };
        var beep = new Button(new TextBlock("Bunyikan nada uji")) { IsEnabled = present };
        beep.Click += _ => AppAudio.Beep(880, 250);
        var sound = Page("Suara", "Volume utama sistem.", parts.SoundStatus, volText, parts.Volume, beep);

        var users = UsersPage(w, parts);
        var fonts = FontsPage(parts);
        var pages = new List<(string, IconKind, Control)>
        {
            ("Tampilan", IconKind.Sun, look),
            ("Fon", IconKind.File, fonts),
            ("Wallpaper", IconKind.Leaf, paper),
            ("Screensaver", IconKind.Moon, saver),
            ("Suara", IconKind.Play, sound),
            ("Pengguna", IconKind.Home, users),
        };
        var host = new Border { Padding = new Thickness(16, 0, 0, 0) };
        var nav = new ListBox { BorderThickness = 0, Background = Color.Transparent, CornerRadius = 0, Padding = new Thickness(6), Width = 160 };
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

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Sistem", "Pengaturan", IconKind.Gear, 0xD9B38C).DockTo(Dock.Top));
        root.Children.Add(nav.DockTo(Dock.Left));
        root.Children.Add(host);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, parts);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Pengaturan");
        var (w, p) = Build(headless: true);
        int themes = p.Themes.ItemCount, papers = p.Wallpapers.ItemCount, savers = p.Savers.ItemCount;
        c.That(themes >= 2 && papers >= 1 && savers >= 2, "choices listed (" + themes + " themes, " + papers + " wallpapers, " + savers + " screensavers)");
        // screen: resolutions listed (not changed: it would apply at the next start), text scale round trip
        c.That(p.Resolution.Items.Count >= 4 && p.Resolution.SelectedIndex >= 0, "resolutions listed (" + p.Resolution.Items.Count + ")");
        int scale0 = AppSettings.UiScale;
        p.Scale.SelectedIndex = 1;
        bool scaled = AppSettings.UiScale == 1;
        p.Scale.SelectedIndex = scale0;
        c.That(scaled && AppSettings.UiScale == scale0, "text scale 110 % and back");
        int icons0 = AppSettings.IconPack;
        if (p.Icons.Items.Count > 1)
        {
            p.Icons.SelectedIndex = 1;
            bool packed = AppSettings.IconPack == 1;
            p.Icons.SelectedIndex = icons0;
            c.That(packed && AppSettings.IconPack == icons0, "icon pack " + p.Icons.Items[1] + " and back");
        }
        else c.That(p.Icons.Items.Count == 1, "only the built-in icons");

        // theme: pick another through the list, read it back, restore
        int theme0 = AppSettings.Theme;
        p.Themes.SelectedIndex = (theme0 + 1) % themes;
        c.That(AppSettings.Theme == (theme0 + 1) % themes, "theme changed through the list");
        p.Themes.SelectedIndex = theme0;
        c.That(AppSettings.Theme == theme0, "theme restored");

        int saver0 = AppSettings.Screensaver;
        p.Savers.SelectedIndex = savers - 1; // "off"
        c.That(AppSettings.Screensaver == savers - 1, "screensaver off");
        p.Savers.SelectedIndex = saver0 < 0 ? 0 : saver0;

        int delay0 = AppSettings.ScreensaverDelaySeconds;
        p.Delay.Value = 60;
        p.Delay.Value = 60; // (ValueChanged fires on a change)
        AppSettings.ScreensaverDelaySeconds = 60;
        c.That(Math.Abs(AppSettings.ScreensaverDelaySeconds - 60) <= 1, "screensaver delay 60 s (" + AppSettings.ScreensaverDelaySeconds + ")");
        AppSettings.ScreensaverDelaySeconds = Math.Max(5, delay0);

        bool anim0 = AppSettings.Animations;
        AppSettings.Animations = !anim0;
        c.That(AppSettings.Animations == !anim0, "animations switched");
        AppSettings.Animations = anim0;

        if (AppAudio.IsPresent)
        {
            int v0 = AppAudio.Volume;
            AppAudio.Volume = 35;
            c.That(AppAudio.Volume == 35, "volume 35%");
            AppAudio.Volume = v0;
        }
        else c.That(!p.Volume.IsEnabled, "no sound card: the volume slider is disabled");

        c.Equal("Mati", SaverLabel("off"), "screensaver labels");

        // Pengguna: only with the default password (a changed one is left alone)
        c.That(p.Accounts.ItemCount >= 1, "accounts listed (" + p.Accounts.ItemCount + ")");
        if (Users.Verify(Users.DefaultAdministrator, "ganteng@ku"))
        {
            foreach (var u in Users.List()) if (u.Name == "tamu") { p.AdminPassword.Text = "ganteng@ku"; p.DeleteUser(u); }
            p.AdminPassword.Text = "salah";
            c.That(p.AddUser("tamu", "rahasia", false) != null, "a wrong administrator password is refused");
            p.AdminPassword.Text = "ganteng@ku";
            c.That(p.AddUser("tamu", "rahasia", false) == null && Users.Verify("tamu", "rahasia"), "add user tamu");
            UserAccount? tamu = null;
            foreach (var u in Users.List()) if (u.Name == "tamu") tamu = u;
            c.That(tamu != null && !tamu.IsAdministrator, "tamu is listed, not an administrator");
            c.That(p.SetPassword(tamu!, "rahasia", "baru123") == null && Users.Verify("tamu", "baru123"), "change own password");
            c.That(p.SetAvatar(tamu!, Users.Avatars[3]) == null, "profile picture set");
            foreach (var u in Users.List()) if (u.Name == "tamu") tamu = u;
            c.Equal(Users.Avatars[3], tamu!.Avatar, "picture recorded");
            c.That(p.DeleteUser(tamu) == null && !Users.Verify("tamu", "baru123"), "delete tamu");
            foreach (var u in Users.List()) if (u.Name == Users.DefaultAdministrator) c.That(p.DeleteUser(u) != null, "the last administrator stays");
            p.AdminPassword.Text = "";
        }
        FontChecks(c, p);
        p.Nav.SelectedIndex = 5;
        w.RenderFrame();
        c.That(w.LastFrameOps > 30, "frame drew " + w.LastFrameOps + " ops");
        return c;
    }

    const int FontTestPort = 8093;

    /// <summary>A tiny HTTP server for the font install test: /f.ttf = the given bytes.</summary>
    static void ServeFont(TcpServer server, byte[] data, Func<bool> stop, Action done)
    {
        while (!stop())
        {
            var c = server.Accept(50);
            if (c == null) continue;
            try
            {
                c.TimeoutMs = 2000;
                var buf = new byte[2048];
                var req = new StringBuilder();
                while (!req.ToString().Contains("\r\n\r\n"))
                {
                    int n = c.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    req.Append(Encoding.ASCII.GetString(buf, 0, n));
                }
                bool ok = req.ToString().StartsWith("GET /f.ttf");
                var h = Encoding.ASCII.GetBytes(ok ? "HTTP/1.1 200 OK\r\nContent-Length: " + data.Length + "\r\nConnection: close\r\n\r\n"
                    : "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                c.Write(h, 0, h.Length);
                if (ok) for (int o = 0; o < data.Length; o += 16384) c.Write(data, o, Math.Min(16384, data.Length - o));
            }
            catch (Exception) { }
            c.Close();
        }
        server.Close();
        done();
    }

    /// <summary>The Fon page: choices, switching the desktop font off and back, and the
    /// whole install path (catalog -> HTTP download -> kernel SHA-256 check -> remove)
    /// against a local server -- on the device only (it needs the font engine).</summary>
    static void FontChecks(Checks c, Parts p)
    {
        int ui0 = AppSettings.UiFont, mono0 = AppSettings.MonoFont;
        c.That(p.UiFonts.ItemCount >= 2 && p.MonoFonts.ItemCount >= 2, "fonts listed (" + p.UiFonts.ItemCount + " desktop, " + p.MonoFonts.ItemCount + " terminal)");
        if (AppSettings.IsSimulated) return;
        c.That(p.Catalog.ItemCount >= 10, "font catalog (" + p.Catalog.ItemCount + " families)");
        // the desktop font: to the bitmap font and back; text measuring follows
        int wide = Fonts.Measure("iiiiiiiiii", 0), wideW = Fonts.Measure("WWWWWWWWWW", 0);
        c.That(ui0 > 0 && Fonts.Proportional && wide < wideW, "proportional desktop font (" + wide + " px vs " + wideW + " px)");
        p.UiFonts.SelectedIndex = 0;
        c.That(AppSettings.UiFont == 0, "desktop font: bitmap");
        p.UiFonts.SelectedIndex = ui0;
        c.That(AppSettings.UiFont == ui0, "desktop font restored");
        // install from a local server: the catalog points at it with the real SHA-256
        // (/disk/UJIFONT.TTF: an 8 KB subset of JetBrains Mono renamed "Uji Mono")
        const string jb = "8f429b16956fcd08e0811964241428aef4627fb7d215e484282fc5fcaa7736f8";
        var bytes = AppFiles.ReadAllBytes("/disk/UJIFONT.TTF");
        if (bytes == null) { c.That(false, "/disk/UJIFONT.TTF"); return; }
        string cat = AppFiles.ScratchDirectory + "/FONTCAT.TXT";
        string url = "http://localhost:" + FontTestPort + "/f.ttf";
        AppFiles.WriteAllText(cat, "ujimono\tUji Mono\tRegular\tUJIMONO.TTF\t" + bytes.Length + "\t" + jb + "\tOFL-1.1\t" + url + "\t\n"
            + "ujisalah\tUji Salah\tRegular\tUJISALAH.TTF\t" + bytes.Length + "\t" + new string('0', 64) + "\tOFL-1.1\t" + url + "\t\n"
            + "ujilisen\tUji Lisensi\tRegular\tUJILISEN.TTF\t" + bytes.Length + "\t" + jb + "\tUFL-1.0\t" + url + "\t\n");
        bool stop = false, finished = false;
        TcpServer? server = null;
        try { server = new TcpServer(FontTestPort); } catch (Exception e) { c.That(false, "listen on " + FontTestPort + ": " + e.Message); }
        if (server == null) return;
        var th = new Thread(() => ServeFont(server, bytes, () => stop, () => finished = true));
        th.Start();
        try
        {
            var err = FontInstaller.Install("ujimono", cat);
            c.That(err == null && AppFiles.Size(FontCatalog.Directory + "/UJIMONO.TTF") == bytes.Length, "font downloaded, SHA-256 checked, installed: " + (err ?? "ok"));
            c.That(Fonts.Find("Uji Mono") > 0 && Fonts.MeasureIn(Fonts.Find("Uji Mono"), 16, "Uji") > 0, "the new family is registered and measures text");
            err = FontInstaller.Install("ujisalah", cat);
            c.That(err != null && AppFiles.Size(FontCatalog.Directory + "/UJISALAH.TTF") <= 0, "a wrong SHA-256 is refused: " + err);
            err = FontInstaller.Install("ujilisen", cat);
            c.That(err != null && err.Contains("UFL"), "another licence needs consent");
            err = FontInstaller.Remove("ujimono", cat);
            c.That(err == null && AppFiles.Size(FontCatalog.Directory + "/UJIMONO.TTF") <= 0 && Fonts.Find("Uji Mono") < 0, "font removed: " + (err ?? "ok"));
        }
        finally
        {
            // Join the server thread. It used to stall 30 s or more on the device:
            // an exiting thread frees its copy of the interpreter, and clrhost's
            // linked-list heap walked every free block for each free (fixed with
            // size classes in clrhost/src/heap.rs).
            stop = true;
            var waited = System.Diagnostics.Stopwatch.StartNew();
            th.Join();
            long joinMs = waited.ElapsedMilliseconds;
            c.That(finished && joinMs < 5000, "server thread joined in " + joinMs + " ms");
            AppFiles.Delete(cat);
        }
        AppSettings.MonoFont = mono0;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        // `font install NAME` in the Terminal: download in the background, no window
        int fi = Array.IndexOf(args, "--font-install");
        if (fi >= 0 && fi + 1 < args.Length)
        {
            var err = FontInstaller.Install(args[fi + 1], null, true, line => Console.WriteLine("[font] " + line));
            Console.WriteLine(err == null ? "[font] " + args[fi + 1] + ": terpasang -- font list, font ui / font mono" : "[font] " + args[fi + 1] + ": " + err);
            return err == null ? 0 : 1;
        }
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = SettingsApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, parts) = SettingsApp.Build();
        // --page N opens another page (0 Tampilan, 1 Fon, ...)
        int pi = Array.IndexOf(args, "--page");
        if (pi >= 0 && pi + 1 < args.Length && int.TryParse(args[pi + 1], out int page)) parts.Nav.SelectedIndex = page;
        app.Run(w);
        return 0;
    }
}
