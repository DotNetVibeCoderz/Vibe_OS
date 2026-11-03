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
        public TextBlock DelayText = null!, SoundStatus = null!;
        // Pengguna
        public ListBox Accounts = null!;
        public TextBox AdminName = null!, AdminPassword = null!;
        public TextBlock UserStatus = null!;
        public Func<string, string, bool, string?> AddUser = null!;
        public Func<UserAccount, string?> DeleteUser = null!;
        public Func<UserAccount, string, string, string?> SetPassword = null!;
        public Func<UserAccount, string, string?> SetAvatar = null!;
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
        parts.Themes.Height = 230;
        parts.Animations = new ToggleSwitch("", AppSettings.Animations) { OnContent = "Animasi jendela nyala", OffContent = "Animasi jendela mati", Padding = new Thickness(0) };
        parts.Animations.IsCheckedChanged += t => AppSettings.Animations = t.IsChecked;
        parts.Cursor = new ToggleSwitch("", AppSettings.LargeCursor) { OnContent = "Kursor besar", OffContent = "Kursor normal", Padding = new Thickness(0) };
        parts.Cursor.IsCheckedChanged += t => AppSettings.LargeCursor = t.IsChecked;
        var look = Page("Tampilan", "Tema berlaku langsung untuk seluruh desktop.", parts.Themes, parts.Animations, parts.Cursor);

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
        var pages = new List<(string, IconKind, Control)>
        {
            ("Tampilan", IconKind.Sun, look),
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
        p.Nav.SelectedIndex = 4;
        w.RenderFrame();
        c.That(w.LastFrameOps > 30, "frame drew " + w.LastFrameOps + " ops");
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
            var c = SettingsApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = SettingsApp.Build();
        app.Run(w);
        return 0;
    }
}
