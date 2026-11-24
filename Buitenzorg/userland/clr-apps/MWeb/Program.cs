using Buitenzorg.Security.Cryptography;
// Peramban -- the web browser: an address bar, back / forward / reload, pages
// over HTTP and HTTPS (Buitenzorg.Net; certificates verified), local files
// (/disk/X.HTM) and built-in about: pages. Html.cs parses and lays the page
// out; PageView draws the visible part, scrolls (wheel, PageUp/PageDown,
// arrows), follows links and shows images (PNG / JPEG / GIF / BMP through
// Buitenzorg.Media.Picture). Pages are fetched on a background thread.

using System.Text;
using System.Threading;
using Buitenzorg.UI;
using Buitenzorg.Net;
using HttpClient = Buitenzorg.Net.HttpClient;
using Buitenzorg.Media;
using Files = Buitenzorg.App.Files;

namespace MWeb;

/// <summary>Draws a laid-out page and turns clicks on links into navigation.</summary>
public sealed class PageView : Control
{
    public List<Item> Items = new List<Item>();
    public int ContentHeight;
    public int Scroll;
    public Palette Pal;
    public event Action<Item>? LinkClicked;
    public event Action<string>? Hover;
    readonly Dictionary<Item, (string Key, int[] Px)> _pix = new Dictionary<Item, (string, int[])>();
    string _hover = "";
    public int LayoutWidth;
    public int ImagesDrawn;

    public PageView() { Focusable = true; ClipToBounds = true; }

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(200, a.Width), Math.Max(150, a.Height));

    public void SetItems(List<Item> items, int height)
    {
        Items = items; ContentHeight = height; _pix.Clear();
        ScrollTo(Scroll);
        InvalidateVisual();
    }

    public void ScrollTo(int s)
    {
        int max = Math.Max(0, ContentHeight - Bounds.Height);
        Scroll = Math.Clamp(s, 0, max);
        InvalidateVisual();
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRect(b, Pal.Back);
        dc.PushClip(b);
        int top = Scroll, bottom = Scroll + b.Height;
        ImagesDrawn = 0;
        foreach (var it in Items)
        {
            if (it.Y + it.H < top || it.Y > bottom) continue;
            int x = b.X + it.X, y = b.Y + it.Y - Scroll;
            switch (it.Kind)
            {
                case ItemKind.Text:
                    dc.Text(x, y, it.Text, it.Color, it.Style);
                    if (it.Href != null) dc.FillRect(new Rect(x, y + it.H - 2, it.W, 1), it.Color);
                    break;
                case ItemKind.Rule:
                    dc.FillRect(new Rect(x, y, it.W, 1), it.Color);
                    break;
                case ItemKind.Bar: // a CSS background (before the block's content in the list)
                    dc.FillRect(new Rect(x, y, it.W, it.H), it.Color);
                    break;
                case ItemKind.Frame: // a CSS border
                    dc.FillRect(new Rect(x, y, it.W, 1), it.Color);
                    dc.FillRect(new Rect(x, y + it.H - 1, it.W, 1), it.Color);
                    dc.FillRect(new Rect(x, y, 1, it.H), it.Color);
                    dc.FillRect(new Rect(x + it.W - 1, y, 1, it.H), it.Color);
                    break;
                case ItemKind.Image:
                    DrawImage(dc, it, x, y, b);
                    break;
            }
        }
        // scroll thumb
        if (ContentHeight > b.Height)
        {
            int th = Math.Max(24, b.Height * b.Height / ContentHeight);
            int ty = b.Y + (int)((long)(b.Height - th) * Scroll / Math.Max(1, ContentHeight - b.Height));
            dc.FillRoundRect(new Rect(b.Right - 7, ty, 5, th), 2, Pal.Rule);
        }
        dc.PopClip();
    }

    void DrawImage(DrawingContext dc, Item it, int x, int y, Rect b)
    {
        var img = it.Image!;
        // the visible part only: Blit needs the whole rectangle inside the clip
        int vx0 = Math.Max(x, b.X), vy0 = Math.Max(y, b.Y);
        int vx1 = Math.Min(x + it.W, b.Right), vy1 = Math.Min(y + it.H, b.Bottom);
        if (vx1 <= vx0 || vy1 <= vy0) return;
        if (img.Picture == null)
        {
            dc.FillRect(new Rect(vx0, vy0, vx1 - vx0, vy1 - vy0), Pal.Rule);
            dc.Text(x + 4, y + 4, img.Failed ? "[gambar gagal]" : (img.Alt.Length > 0 ? img.Alt : "[gambar]"), Pal.Muted);
            return;
        }
        int dw = vx1 - vx0, dh = vy1 - vy0;
        string key = dw + "," + dh + "," + (vx0 - x) + "," + (vy0 - y);
        if (!_pix.TryGetValue(it, out var c) || c.Key != key)
        {
            var p = img.Picture;
            double sx = (double)p.Width / it.W, sy = (double)p.Height / it.H;
            var px = new int[dw * dh];
            p.Render(0, px, dw, dh, (int)((vx0 - x) * sx), (int)((vy0 - y) * sy), Math.Max(1, (int)(dw * sx)), Math.Max(1, (int)(dh * sy)), Pal.Back, Pal.Back);
            c = (key, px);
            _pix[it] = c;
        }
        dc.Blit(vx0, vy0, dw, dh, c.Px);
        ImagesDrawn++;
    }

    public Item? HitLink(int px, int py)
    {
        var b = Bounds;
        int x = px - b.X, y = py - b.Y + Scroll;
        foreach (var it in Items)
            if (it.Href != null && x >= it.X && x < it.X + it.W && y >= it.Y && y < it.Y + it.H) return it;
        return null;
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        var it = HitLink(e.X, e.Y);
        if (it != null) { LinkClicked?.Invoke(it); e.Handled = true; }
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        string h = HitLink(e.X, e.Y)?.Href ?? "";
        if (h != _hover) { _hover = h; Hover?.Invoke(h); }
    }

    protected override void OnPointerWheelChanged(PointerEventArgs e)
    {
        ScrollTo(Scroll - e.WheelDelta * 48);
        e.Handled = true;
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        int page = Math.Max(40, Bounds.Height - 40);
        if (e.Key == Keys.Down) ScrollTo(Scroll + 32);
        else if (e.Key == Keys.Up) ScrollTo(Scroll - 32);
        else if (e.Key == Keys.PageDown || e.Key == ' ') ScrollTo(Scroll + page);
        else if (e.Key == Keys.PageUp) ScrollTo(Scroll - page);
        else if (e.Key == Keys.Home) ScrollTo(0);
        else if (e.Key == Keys.End) ScrollTo(ContentHeight);
        else return;
        e.Handled = true;
    }
}

public sealed class BrowserApp
{
    public Window W { get; }
    public readonly PageView View = new PageView();
    readonly TextBox _address = new TextBox { Watermark = "Ketik alamat, mis. https://example.com" };
    readonly TextBlock _status = new TextBlock("");
    readonly TextBlock _titleText = new TextBlock("Peramban");
    public Page? Current { get; private set; }
    public string CurrentUrl { get; private set; } = "";
    public string Status => _status.Text;
    public string LastError = "";
    List<string> _back = new List<string>(), _forward = new List<string>();
    int _nav; // navigation id: a late answer to an older request is dropped
    public bool Loading { get; private set; }
    public bool Synchronous; // tests: fetch on the calling thread
    /// <summary>Script console (console.log, alert, errors) of the current page, newest last.</summary>
    public readonly List<string> ConsoleLines = new List<string>();
    /// <summary>Tests move the page's clock forward with this (ms).</summary>
    public long ClockOffset;
    readonly DispatcherTimer _jsTimer = new DispatcherTimer(40);
    readonly DockPanel _fieldBar = new DockPanel { Margin = new Thickness(0, 6, 0, 0) };
    readonly TextBox _field = new TextBox();
    readonly TextBlock _fieldLabel = new TextBlock("Isi kolom:");
    int _fieldLink = -1;

    /// <summary>One browser tab: its page, address, history and scroll position.
    /// The active tab's state lives in the app's fields; the others wait here.</summary>
    public sealed class Tab
    {
        public Page? Page;
        public string Url = "", Title = "";
        public List<string> Back = new List<string>(), Forward = new List<string>();
        public int Scroll;
    }
    public readonly List<Tab> Tabs = new List<Tab>();
    public int ActiveTab { get; private set; }
    readonly StackPanel _tabStrip = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, Margin = new Thickness(0, 0, 0, 6) };
    /// <summary>Bookmarks: (address, title), saved in <see cref="BookmarkFile"/>.</summary>
    public readonly List<(string Url, string Title)> Bookmarks = new List<(string, string)>();
    public static string BookmarkFile = "/disk/APPDATA/MWEB/TANDA.TXT";
    /// <summary>Cookies of every page, download and picture (persistent ones are kept in <see cref="CookieFile"/>).</summary>
    public static CookieJar Jar = CookieJar.Shared;
    public static string CookieFile = "/disk/APPDATA/MWEB/KUKI.TXT";
    string _savedCookies = "";
    readonly bool _headless;
    static volatile string _bookmarksHtml = "";
    static readonly System.Diagnostics.Stopwatch _sw = System.Diagnostics.Stopwatch.StartNew();
    static long _epoch = -1;

    /// <summary>Wall-clock milliseconds since 1970 for scripts (Date.now, timers).</summary>
    public long NowMs()
    {
        if (_epoch < 0)
        {
            try { _epoch = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(); } catch (Exception) { _epoch = 0; }
            _epoch -= _sw.ElapsedMilliseconds;
        }
        return _epoch + _sw.ElapsedMilliseconds + ClockOffset;
    }

    public BrowserApp(bool headless)
    {
        _headless = headless;
        W = new Window("Peramban", 860, 600, headless);
        View.Pal = new Palette { Text = 0x1F2A24, Link = 0x1565C0, Heading = 0x1B4D2E, Muted = 0x6B7A71, Rule = 0xD5DDD8, Back = 0xFBFCF8 };
        View.LinkClicked += OnLink;
        _jsTimer.Tick += TickScripts;
        _field.KeyDown += (_, e) =>
        {
            if (e.Key == Keys.Enter) { SubmitField(); e.Handled = true; }
            else if (e.Key == (char)27) { CloseField(); e.Handled = true; }
        };
        View.Hover += h => { if (!Loading) _status.Text = h.Length > 0 ? Resolve(h) ?? h : StatusIdle(); };
        _address.KeyDown += (_, e) => { if (e.Key == Keys.Enter) { Navigate(Normalize(_address.Text)); e.Handled = true; } };

        var bar = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        var nav = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, Margin = new Thickness(0, 0, 8, 0) };
        nav.Children.Add(new Button("<", _ => Back()).WithClass("subtle"));
        nav.Children.Add(new Button(">", _ => Forward()).WithClass("subtle"));
        nav.Children.Add(new Button("Muat ulang", _ => Reload()).WithClass("subtle"));
        nav.Children.Add(new Button("Beranda", _ => Navigate("about:home")).WithClass("subtle"));
        nav.Children.Add(new Button("+ Tanda", _ => AddBookmark()).WithClass("subtle"));
        nav.Children.Add(new Button("Tanda", _ => Navigate("about:bookmarks")).WithClass("subtle"));
        bar.Children.Add(nav.DockTo(Dock.Left));
        bar.Children.Add(new Button("Buka", _ => Navigate(Normalize(_address.Text))).WithClass("accent").DockTo(Dock.Right));
        _address.Margin = new Thickness(0, 0, 8, 0);
        bar.Children.Add(_address);

        var root = new DockPanel { Margin = new Thickness(12) };
        root.Children.Add(_titleText.WithClass("strong").DockTo(Dock.Top));
        root.Children.Add(_tabStrip.DockTo(Dock.Top));
        root.Children.Add(bar.DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        _fieldBar.Children.Add(_fieldLabel.DockTo(Dock.Left));
        _fieldBar.Children.Add(new Button("Isi", _ => SubmitField()).WithClass("accent").DockTo(Dock.Right));
        _field.Margin = new Thickness(8, 0, 8, 0);
        _fieldBar.Children.Add(_field);
        _fieldBar.IsVisible = false;
        root.Children.Add(_fieldBar.DockTo(Dock.Bottom));
        View.Margin = new Thickness(0, 0, 0, 6);
        root.Children.Add(View);
        W.Content = root;
        W.UnhandledKey += e =>
        {
            if (e.Key == Keys.F5) { Reload(); e.Handled = true; }
            else if (e.Key == (char)12) { _address.Focus(); e.Handled = true; } // Ctrl+L
            else if (e.Key == (char)20) { NewTab(); e.Handled = true; }        // Ctrl+T
            else if (e.Key == (char)23) { CloseTab(ActiveTab); e.Handled = true; } // Ctrl+W
            else if (e.Key == (char)4) { AddBookmark(); e.Handled = true; }   // Ctrl+D
            else if (e.Key == Keys.F6) { SwitchTab((ActiveTab + 1) % Tabs.Count); e.Handled = true; }
        };
        Tabs.Add(new Tab());
        LoadBookmarks();
        RebuildTabs();
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    string StatusIdle() => Current == null ? "" : (Current.Title.Length > 0 ? Current.Title + "  -  " : "") + Current.Links.Count + " tautan, " + Current.Images.Count + " gambar";

    /// <summary>What the address bar holds, as an address: adds http:// to a bare host.</summary>
    public static string Normalize(string s)
    {
        s = s.Trim();
        if (s.Length == 0) return "about:home";
        if (s.StartsWith("about:") || s.StartsWith("/") || s.Contains("://")) return s;
        return "http://" + s;
    }

    string? Resolve(string href)
    {
        if (href.StartsWith("about:") || href.StartsWith("/disk/") || href.StartsWith("/ram/")) return href;
        if (CurrentUrl.StartsWith("http"))
            return Url.Parse(CurrentUrl)?.Resolve(href)?.ToString();
        if (href.Contains("://")) return href;
        if (CurrentUrl.StartsWith("/"))
        {
            int slash = CurrentUrl.LastIndexOf('/');
            return CurrentUrl.Substring(0, slash + 1) + href;
        }
        return null;
    }

    /// <summary>Open `url`; `post` = a form's urlencoded body to POST there.</summary>
    public void Navigate(string url, bool record = true, byte[]? post = null)
    {
        if (url.StartsWith("about:unmark?"))
        {
            int n;
            if (int.TryParse(url.Substring(13), out n)) RemoveBookmark(n);
            url = "about:bookmarks";
            record = false;
        }
        if (url == "about:bookmarks") _bookmarksHtml = BookmarksPage();
        if (record && CurrentUrl.Length > 0 && CurrentUrl != url) { _back.Add(CurrentUrl); _forward.Clear(); }
        _address.Text = url;
        Loading = true;
        _jsTimer.Stop();
        CloseField();
        _status.Text = "Memuat " + url + " ...";
        int id = ++_nav;
        if (Synchronous) { var r = Fetch(url, post); Show(id, r); LoadImages(id, r.Page, r.Url, sync: true); return; }
        var t = new Thread(() =>
        {
            var r = Fetch(url, post);
            Dispatcher.Post(() => Show(id, r));
            LoadImages(id, r.Page, r.Url, sync: false);
        });
        t.Start();
    }

    // ---- tabs ----------------------------------------------------------------

    void SaveTab()
    {
        var t = Tabs[ActiveTab];
        t.Page = Current;
        t.Url = CurrentUrl;
        t.Title = Current != null && Current.Title.Length > 0 ? Current.Title : CurrentUrl;
        t.Back = _back;
        t.Forward = _forward;
        t.Scroll = View.Scroll;
    }

    void LoadTab(int i)
    {
        ActiveTab = i;
        var t = Tabs[i];
        Current = t.Page;
        CurrentUrl = t.Url;
        _back = t.Back;
        _forward = t.Forward;
        _address.Text = t.Url;
        _titleText.Text = t.Title.Length > 0 ? t.Title : "Peramban";
        ConsoleLines.Clear();
        _nav++; // an answer still on its way belongs to the tab we left
        Loading = false;
        CloseField();
        if (Current != null)
        {
            Relayout();
            View.Scroll = t.Scroll;
            _jsTimer.Start(); // its timers run again (TickScripts stops when there are none)
        }
        else View.SetItems(new List<Item>(), 0);
        _status.Text = StatusIdle();
        RebuildTabs();
    }

    /// <summary>Open a tab (and go to <paramref name="url"/> in it). Returns its index.</summary>
    public int NewTab(string url = "about:home")
    {
        if (Tabs.Count >= 8) return ActiveTab;
        SaveTab();
        _jsTimer.Stop();
        Tabs.Add(new Tab());
        LoadTab(Tabs.Count - 1);
        Navigate(url, record: false);
        return ActiveTab;
    }

    public void SwitchTab(int i)
    {
        if (i < 0 || i >= Tabs.Count || i == ActiveTab) return;
        SaveTab();
        _jsTimer.Stop();
        LoadTab(i);
    }

    public void CloseTab(int i)
    {
        if (i < 0 || i >= Tabs.Count) return;
        if (Tabs.Count == 1) { Navigate("about:home"); return; }
        SaveTab();
        var gone = Tabs[i];
        Tabs.RemoveAt(i);
        if (gone.Page != null) Html.Free(gone.Page);
        _jsTimer.Stop();
        int next = i == ActiveTab ? Math.Min(i, Tabs.Count - 1) : (i < ActiveTab ? ActiveTab - 1 : ActiveTab);
        ActiveTab = next; // LoadTab must not save into the removed slot
        LoadTab(next);
    }

    void RebuildTabs()
    {
        _tabStrip.Children.Clear();
        for (int i = 0; i < Tabs.Count; i++)
        {
            int k = i;
            string title = i == ActiveTab ? (Current?.Title.Length > 0 ? Current.Title : CurrentUrl) : Tabs[i].Title;
            if (title.Length == 0) title = "Tab baru";
            if (title.Length > 18) title = title.Substring(0, 17) + "~";
            var b = new Button(title, _ => SwitchTab(k)).WithClass(i == ActiveTab ? "accent" : "subtle");
            _tabStrip.Children.Add(b);
        }
        if (Tabs.Count > 1) _tabStrip.Children.Add(new Button("x", _ => CloseTab(ActiveTab)).WithClass("subtle"));
        if (Tabs.Count < 8) _tabStrip.Children.Add(new Button("+", _ => NewTab()).WithClass("subtle"));
    }

    // ---- bookmarks ----------------------------------------------------------------

    public void LoadBookmarks()
    {
        Bookmarks.Clear();
        try
        {
            if (Files.GetInfo(BookmarkFile) == null) return;
            foreach (var line in Files.ReadAllText(BookmarkFile).Split('\n'))
            {
                int tab = line.IndexOf('\t');
                string url = (tab < 0 ? line : line.Substring(0, tab)).Trim();
                if (url.Length > 0) Bookmarks.Add((url, tab < 0 ? url : line.Substring(tab + 1).Trim()));
            }
        }
        catch (Exception) { }
    }

    /// <summary>Read the persistent cookies (the app's start).</summary>
    public void LoadCookies()
    {
        try
        {
            if (Files.GetInfo(CookieFile) == null) return;
            Jar.Load(Files.ReadAllText(CookieFile));
            _savedCookies = Jar.Save();
        }
        catch (Exception) { }
    }

    /// <summary>Write the persistent cookies when they changed (never from a headless test).</summary>
    void SaveCookies()
    {
        if (_headless) return;
        string now = Jar.Save();
        if (now == _savedCookies) return;
        try { if (EnsureFolder(CookieFile) && Files.WriteAllText(CookieFile, now)) _savedCookies = now; } catch (Exception) { }
    }

    static bool EnsureFolder(string file)
    {
        int slash = file.LastIndexOf('/');
        string dir = file.Substring(0, slash);
        if (Files.GetInfo(dir) != null) return true;
        int s1 = dir.LastIndexOf('/');
        if (s1 > 0 && Files.GetInfo(dir.Substring(0, s1)) == null) Files.CreateDirectory(dir.Substring(0, s1));
        return Files.CreateDirectory(dir);
    }

    bool SaveBookmarks()
    {
        var sb = new System.Text.StringBuilder();
        foreach (var (u, t) in Bookmarks) { sb.Append(u); sb.Append('\t'); sb.Append(t.Replace('\t', ' ').Replace('\n', ' ')); sb.Append('\n'); }
        try
        {
            int slash = BookmarkFile.LastIndexOf('/');
            string dir = BookmarkFile.Substring(0, slash);
            if (Files.GetInfo(dir) == null)
            {
                // /disk/APPDATA, then /disk/APPDATA/MWEB
                int s1 = dir.LastIndexOf('/');
                if (s1 > 0 && Files.GetInfo(dir.Substring(0, s1)) == null) Files.CreateDirectory(dir.Substring(0, s1));
                Files.CreateDirectory(dir);
            }
            return Files.WriteAllText(BookmarkFile, sb.ToString());
        }
        catch (Exception) { return false; }
    }

    /// <summary>Bookmark the page shown (once per address). False when there is none or it is already there.</summary>
    public bool AddBookmark()
    {
        if (CurrentUrl.Length == 0 || CurrentUrl == "about:bookmarks") return false;
        foreach (var (u, _) in Bookmarks) if (u == CurrentUrl) { _status.Text = "Sudah ditandai: " + CurrentUrl; return false; }
        string title = Current != null && Current.Title.Length > 0 ? Current.Title : CurrentUrl;
        Bookmarks.Add((CurrentUrl, title));
        bool saved = SaveBookmarks();
        _status.Text = (saved ? "Ditandai: " : "Ditandai (tidak tersimpan): ") + title;
        return true;
    }

    public bool RemoveBookmark(int i)
    {
        if (i < 0 || i >= Bookmarks.Count) return false;
        Bookmarks.RemoveAt(i);
        SaveBookmarks();
        return true;
    }

    string BookmarksPage()
    {
        var sb = new System.Text.StringBuilder();
        sb.Append("<title>Tanda</title><h1>Tanda</h1>");
        if (Bookmarks.Count == 0) sb.Append("<p>Belum ada tanda. Buka halaman lalu tekan <b>+ Tanda</b> (Ctrl+D).</p>");
        else
        {
            sb.Append("<ul>");
            for (int i = 0; i < Bookmarks.Count; i++)
            {
                sb.Append("<li><a href=\"" + Escape(Bookmarks[i].Url) + "\">" + Escape(Bookmarks[i].Title) + "</a> -- " + Escape(Bookmarks[i].Url));
                sb.Append(" <a href=\"about:unmark?" + i + "\">[hapus]</a></li>");
            }
            sb.Append("</ul>");
        }
        sb.Append("<p><a href=\"about:home\">Beranda</a></p>");
        return sb.ToString();
    }

    public void Back() { if (_back.Count == 0) return; string u = _back[_back.Count - 1]; _back.RemoveAt(_back.Count - 1); _forward.Add(CurrentUrl); Navigate(u, record: false); }
    public void Forward() { if (_forward.Count == 0) return; string u = _forward[_forward.Count - 1]; _forward.RemoveAt(_forward.Count - 1); _back.Add(CurrentUrl); Navigate(u, record: false); }
    public void Reload() { if (CurrentUrl.Length > 0) Navigate(CurrentUrl, record: false); }
    public bool CanGoBack => _back.Count > 0;

    public sealed class Result
    {
        public Page Page = new Page();
        public string Url = "", Error = "", Info = "";
    }

    /// <summary>Fetch and parse (any thread).</summary>
    public static Result Fetch(string url, byte[]? post = null)
    {
        var r = new Result { Url = url };
        try
        {
            if (url == "about:bookmarks") { r.Page = Html.Parse(_bookmarksHtml); return r; }
            if (url.StartsWith("about:")) { r.Page = Html.Parse(AboutPage(url)); return r; }
            if (url.StartsWith("/"))
            {
                var bytes = Files.ReadAllBytes(url);
                if (bytes == null) throw new NetException("berkas " + url + " tidak ada");
                r.Page = Picture.IsPictureName(url) ? ImagePage(url) : Html.Parse(Encoding.UTF8.GetString(bytes));
                LoadCss(r.Page, url);
                LoadScripts(r.Page, url);
                return r;
            }
            var http = new HttpClient { Cookies = Jar };
            var resp = post != null ? http.Post(url, "application/x-www-form-urlencoded", post) : http.Get(url);
            r.Url = resp.Url;
            string ct = resp.ContentType.ToLowerInvariant();
            r.Info = resp.Status + " " + resp.Reason + (resp.Tls.Length > 0 ? "  -  terenkripsi (" + resp.Tls + ")" : "");
            if (ct.StartsWith("image/")) r.Page = ImagePage(resp.Url);
            else if (ct.StartsWith("text/plain")) r.Page = Html.Parse("<pre>" + Escape(resp.Text) + "</pre>");
            else { r.Page = Html.Parse(resp.Text); LoadCss(r.Page, resp.Url); LoadScripts(r.Page, resp.Url); }
            if (!resp.Ok && r.Page.Title.Length == 0) r.Page.Title = "HTTP " + resp.Status;
        }
        catch (Exception e)
        {
            r.Error = e.Message;
            r.Page = Html.Parse("<title>Tidak dapat memuat</title><h1>Halaman tidak dapat dimuat</h1><p>" + Escape(url) + "</p><p>" + Escape(e.Message) +
                "</p><p>Periksa alamatnya, atau apakah jaringan tersambung (<b>ipconfig</b> di Terminal).</p><p><a href=\"about:home\">Kembali ke beranda</a></p>");
        }
        return r;
    }

    /// <summary>A page's linked style sheets (at most 4, each at most 256 KB): fetched
    /// and added before the first layout. A sheet that fails is skipped.</summary>
    public static int LoadCss(Page page, string baseUrl)
    {
        int added = 0, n = 0;
        foreach (var href in page.CssLinks)
        {
            if (n++ >= 4) break;
            try
            {
                string css;
                if (baseUrl.StartsWith("/"))
                {
                    string path = href.StartsWith("/") ? href : baseUrl.Substring(0, baseUrl.LastIndexOf('/') + 1) + href;
                    css = Files.ReadAllText(path);
                }
                else
                {
                    var u = Url.Parse(baseUrl)?.Resolve(href);
                    if (u == null) continue;
                    var resp = new HttpClient().Get(u.ToString());
                    if (!resp.Ok || resp.Text.Length > 256 * 1024) continue;
                    css = resp.Text;
                }
                if (css.Length > 0) added += Html.AddCss(page, css);
            }
            catch (Exception) { }
        }
        return added;
    }

    /// <summary>A page's external scripts (at most 8, each at most 512 KB), fetched
    /// before they run. A script that fails to load is skipped, like a browser does.</summary>
    public static int LoadScripts(Page page, string baseUrl)
    {
        int loaded = 0;
        var srcs = Html.ScriptSources(page);
        for (int k = 0; k < srcs.Count && k < 32; k++)
        {
            string href = srcs[k];
            if (href.Length == 0 || loaded >= 8) continue;
            try
            {
                string code;
                if (baseUrl.StartsWith("/"))
                {
                    string path = href.StartsWith("/") ? href : baseUrl.Substring(0, baseUrl.LastIndexOf('/') + 1) + href;
                    code = Files.ReadAllText(path);
                }
                else
                {
                    var u = Url.Parse(baseUrl)?.Resolve(href);
                    if (u == null) continue;
                    var resp = new HttpClient().Get(u.ToString());
                    if (!resp.Ok || resp.Text.Length > 512 * 1024) continue;
                    code = resp.Text;
                }
                Html.SetScriptSource(page, k, code);
                loaded++;
            }
            catch (Exception) { }
        }
        return loaded;
    }

    // ---- scripts ------------------------------------------------------------------

    void OnLink(Item it)
    {
        string href = it.Href ?? "";
        if (Current != null && Current.NativeId > 0 && it.LinkIndex >= 0)
        {
            if (href.StartsWith("input:")) { OpenField(it.LinkIndex); return; }
            int f = Html.Click(Current, it.LinkIndex);
            bool handled = (f & Html.JsHandled) != 0;
            HandleScript(f);
            if (!handled && Submit(it.LinkIndex, pressed: true)) return;
            if (handled || href.StartsWith("js:")) return;
        }
        if (href.StartsWith("js:") || href.StartsWith("input:") || href.StartsWith("javascript:") || href == "#") return;
        var u = Resolve(href);
        if (u != null) Navigate(u);
    }

    /// <summary>Click link number <paramref name="link"/> as a person would (tests).</summary>
    public void ClickLink(int link)
    {
        if (Current == null || link < 0 || link >= Current.Links.Count) return;
        OnLink(new Item { Href = Current.Links[link], LinkIndex = link });
    }

    void OpenField(int link)
    {
        if (Current == null) return;
        _fieldLink = link;
        _field.Text = Html.FieldText(Current, link);
        _fieldBar.IsVisible = true;
        _field.Focus();
    }

    void CloseField()
    {
        _fieldLink = -1;
        _fieldBar.IsVisible = false;
    }

    void SubmitField()
    {
        if (Current == null || _fieldLink < 0) { CloseField(); return; }
        int link = _fieldLink;
        CloseField();
        EnterText(link, _field.Text);
        View.Focus();
        Submit(link, pressed: false);
    }

    /// <summary>Send the form around link `link`: a pressed submit button, or (pressed
    /// false) Enter in a form's only text field. GET puts the fields in the address,
    /// POST sends them urlencoded. False = nothing was sent.</summary>
    public bool Submit(int link, bool pressed)
    {
        var page = Current;
        if (page == null) return false;
        var form = Html.Form(page, link, pressed, out int flags);
        HandleScript(flags);
        if (form == null || Current != page) return false;
        var target = Resolve(form[1].Length > 0 ? form[1] : CurrentUrl);
        if (target == null) return false;
        int h = target.IndexOf('#');
        if (h >= 0) target = target.Substring(0, h);
        if (form[0] == "POST") { Navigate(target, post: Encoding.UTF8.GetBytes(form[2])); return true; }
        int q = target.IndexOf('?');
        if (q >= 0) target = target.Substring(0, q);
        Navigate(target + "?" + form[2]);
        return true;
    }

    /// <summary>Put text into field link <paramref name="link"/> (fires input + change).</summary>
    public void EnterText(int link, string text)
    {
        if (Current == null) return;
        HandleScript(Html.Input(Current, link, text));
    }

    /// <summary>What a script run did: new console lines, a changed page, timers, a navigation.</summary>
    void HandleScript(int f)
    {
        var page = Current;
        if (page == null) return;
        var lines = Html.TakeConsole(page);
        foreach (var l in lines)
        {
            ConsoleLines.Add(l);
            if (ConsoleLines.Count > 200) ConsoleLines.RemoveAt(0);
        }
        if (lines.Count > 0) _status.Text = "konsol: " + lines[lines.Count - 1];
        if ((f & Html.JsChanged) != 0)
        {
            Html.Refresh(page);
            _titleText.Text = page.Title.Length > 0 ? page.Title : CurrentUrl;
            Relayout();
        }
        if ((f & Html.JsTimers) != 0 && !Synchronous) _jsTimer.Start();
        if ((f & Html.JsNavigate) != 0)
        {
            string to = Html.TakeNavigation(page);
            var u = to.Length > 0 ? Resolve(to) : null;
            if (u != null) Navigate(u);
        }
    }

    /// <summary>Run the page's due timers (the UI timer, or a test after moving ClockOffset).</summary>
    public void TickScripts()
    {
        if (Current == null) { _jsTimer.Stop(); return; }
        int f = Html.Tick(Current, NowMs());
        if (f < 0) { _jsTimer.Stop(); return; }
        HandleScript(f);
    }

    static Page ImagePage(string url) => Html.Parse("<title>" + Escape(url) + "</title><img src=\"" + Escape(url) + "\">");

    public static string Escape(string s) => s.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;").Replace("\"", "&quot;");

    void Show(int id, Result r)
    {
        if (id != _nav) return;
        Loading = false;
        if (Current != null && Current != r.Page) Html.Free(Current);
        Current = r.Page;
        CurrentUrl = r.Url;
        LastError = r.Error;
        _address.Text = r.Url;
        _titleText.Text = r.Page.Title.Length > 0 ? r.Page.Title : r.Url;
        View.Scroll = 0;
        ConsoleLines.Clear();
        int w = Math.Max(200, View.Bounds.Width > 0 ? View.Bounds.Width : 800);
        int jf = Html.RunScripts(r.Page, r.Url, NowMs(), w - 8);
        HandleScript(jf & ~Html.JsChanged);
        if ((jf & Html.JsChanged) != 0) { Html.Refresh(r.Page); _titleText.Text = r.Page.Title.Length > 0 ? r.Page.Title : r.Url; }
        Relayout();
        SaveCookies();
        if (!_address.IsFocused) View.Focus(); // the keys scroll the page at once
        _status.Text = r.Error.Length > 0 ? "Gagal: " + r.Error : (r.Info.Length > 0 ? r.Info + "  -  " : "") + StatusIdle();
        RebuildTabs();
    }

    public void Relayout()
    {
        if (Current == null) return;
        int w = Math.Max(200, View.Bounds.Width > 0 ? View.Bounds.Width : 800);
        var (items, h) = Html.Layout(Current, w - 8, View.Pal);
        View.LayoutWidth = w;
        View.SetItems(items, h);
    }

    /// <summary>Images of a page, one by one; each loaded image re-lays the page out.</summary>
    void LoadImages(int id, Page page, string baseUrl, bool sync)
    {
        int count = 0;
        foreach (var img in page.Images)
        {
            if (id != _nav || count++ >= 24) break;
            try
            {
                byte[]? data = null;
                string src = img.Src;
                if (src.StartsWith("/") && !baseUrl.StartsWith("http")) data = Files.ReadAllBytes(src);
                else
                {
                    string? abs = baseUrl.StartsWith("http") ? Url.Parse(baseUrl)?.Resolve(src)?.ToString() : (src.Contains("://") ? src : null);
                    if (abs != null)
                    {
                        var resp = new HttpClient { MaxBody = 6 << 20, Cookies = Jar }.Get(abs);
                        if (resp.Ok) data = resp.Body;
                    }
                }
                var pic = data == null ? null : Picture.FromBytes(data);
                if (sync) Apply(id, img, pic);
                else Dispatcher.Post(() => Apply(id, img, pic));
            }
            catch (Exception) { img.Failed = true; }
        }
    }

    void Apply(int id, ImageRef img, Picture? pic)
    {
        if (id != _nav) return;
        img.Picture = pic;
        img.Failed = pic == null;
        Relayout();
    }

    /// <summary>A page that exercises the script engine (also the self-test's page).</summary>
    public const string JsDemoPage =
        "<title>Demo JavaScript</title><h1 id=\"judul\">Demo JavaScript</h1>" +
        "<p id=\"info\">Skrip belum berjalan.</p>" +
        "<p><button id=\"tombol\">Tambah</button> <span id=\"hitung\">Diklik 0 kali</span></p>" +
        "<p>Nama: <input id=\"nama\" placeholder=\"ketik nama\" size=\"12\"> <span id=\"sapa\"></span></p>" +
        "<p><a href=\"#\" onclick=\"document.getElementById('warna').classList.toggle('hijau'); document.getElementById('warna').style.color = '#2e7d32'; return false;\">Ubah warna</a> " +
        "<span id=\"warna\">teks ini</span></p>" +
        "<ul id=\"daftar\"></ul><p id=\"waktu\">menunggu pengatur waktu...</p>" +
        "<script>\n" +
        "var tanaman = ['anggrek', 'kantong semar', 'bunga bangkai'];\n" +
        "var ul = document.getElementById('daftar');\n" +
        "tanaman.forEach(function (t, i) { var li = document.createElement('li'); li.textContent = (i + 1) + '. ' + t; ul.appendChild(li); });\n" +
        "document.getElementById('info').textContent = 'Skrip berjalan: ' + document.querySelectorAll('#daftar li').length + ' butir dibuat.';\n" +
        "let n = 0;\n" +
        "document.getElementById('tombol').addEventListener('click', () => { n++; document.querySelector('#hitung').textContent = `Diklik ${n} kali`; });\n" +
        "document.getElementById('nama').oninput = function (e) { document.getElementById('sapa').textContent = 'Halo, ' + e.target.value + '!'; };\n" +
        "setTimeout(function () { document.getElementById('waktu').innerHTML = '<b>waktu habis</b> setelah 100 ms'; }, 100);\n" +
        "console.log('demo siap', JSON.stringify({ butir: tanaman.length }));\n" +
        "</script>";

    public static string AboutPage(string url)
    {
        if (url == "about:home" || url == "about:blank" && false)
            return "<title>Beranda Peramban</title>" +
                "<h1>Peramban Buitenzorg</h1>" +
                "<p>Peramban web bawaan Buitenzorg OS: <b>HTTP</b> dan <b>HTTPS</b> (sertifikat diperiksa terhadap " +
                "akar CA Mozilla), gambar PNG/JPEG/GIF, CSS dasar, JavaScript (DOM, event, timer), tab, tanda, " +
                "tautan, riwayat.</p>" +
                "<h2>Coba</h2><ul>" +
                "<li><a href=\"https://example.com/\">example.com</a> (HTTPS)</li>" +
                "<li><a href=\"http://info.cern.ch/hypertext/WWW/TheProject.html\">Situs web pertama di dunia</a> (CERN, 1991)</li>" +
                "<li><a href=\"https://text.npr.org/\">text.npr.org</a> -- berita, versi teks</li>" +
                "<li><a href=\"https://lite.duckduckgo.com/lite/\">DuckDuckGo Lite</a> -- pencarian</li>" +
                "<li><a href=\"/disk/KEBUN.JPG\">Foto Kebun Raya</a> (berkas di /disk)</li>" +
                "<li><a href=\"about:js\">Demo JavaScript</a> -- skrip di halaman</li>" +
                "<li><a href=\"about:bookmarks\">Tanda</a> -- halaman yang Anda tandai</li>" +
                "<li><a href=\"about:help\">Bantuan</a></li></ul>" +
                "<hr><p>Buitenzorg OS -- dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.</p>";
        if (url == "about:js") return JsDemoPage;
        if (url == "about:help")
            return "<title>Bantuan Peramban</title><h1>Bantuan</h1><ul>" +
                "<li>Ketik alamat lalu Enter; tanpa http:// dianggap http.</li>" +
                "<li>Ctrl+L ke kolom alamat, F5 muat ulang.</li>" +
                "<li>Ctrl+T tab baru, Ctrl+W tutup tab, F6 tab berikutnya, Ctrl+D tandai halaman.</li>" +
                "<li>Roda tetikus, PageUp/PageDown, panah dan Spasi menggulir.</li>" +
                "<li>Berkas lokal: /disk/NAMA.HTM atau gambar di /disk.</li></ul>" +
                "<p><a href=\"about:home\">Beranda</a></p>";
        return "<title>" + Escape(url) + "</title><p>Halaman " + Escape(url) + " tidak dikenal.</p>";
    }

    // ---- self-test: a small HTTP server on this machine's own address -------

    const int TestPort = 8088;

    static byte[] TestImage()
    {
        var b = Files.ReadAllBytes("/disk/ANGGREK.PNG");
        if (b != null) return b;
        // off the device (no sample files): a 4x2 BMP
        int w = 4, h = 2, stride = 12, size = 54 + stride * h;
        var bmp = new byte[size];
        bmp[0] = (byte)'B'; bmp[1] = (byte)'M'; bmp[2] = (byte)size; bmp[10] = 54; bmp[14] = 40; bmp[18] = (byte)w; bmp[22] = (byte)h; bmp[26] = 1; bmp[28] = 24;
        for (int i = 54; i < size; i++) bmp[i] = 0x80;
        return bmp;
    }

    /// <summary>A request header's value ("" = absent).</summary>
    static string HeaderValue(string request, string name)
    {
        foreach (var l in request.Split('\n'))
        {
            int colon = l.IndexOf(':');
            if (colon > 0 && string.Equals(l.Substring(0, colon).Trim(), name, StringComparison.OrdinalIgnoreCase)) return l.Substring(colon + 1).Trim();
        }
        return "";
    }

    static void Serve(TcpServer server, Func<bool> stop)
    {
        var img = TestImage();
        while (!stop())
        {
            var c = server.Accept(50);
            if (c == null) continue;
            try
            {
                var buf = new byte[4096];
                var req = new StringBuilder();
                while (!req.ToString().Contains("\r\n\r\n"))
                {
                    int n = c.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    req.Append(Encoding.ASCII.GetString(buf, 0, n));
                }
                string line = req.ToString();
                int sp = line.IndexOf(' '), sp2 = line.IndexOf(' ', sp + 1);
                string path = sp > 0 && sp2 > sp ? line.Substring(sp + 1, sp2 - sp - 1) : "/";
                string reqBody = "";
                {
                    int headEnd = line.IndexOf("\r\n\r\n");
                    int cl = HeaderValue(line, "Content-Length").Length > 0 ? int.Parse(HeaderValue(line, "Content-Length")) : 0;
                    reqBody = headEnd >= 0 ? line.Substring(headEnd + 4) : "";
                    while (reqBody.Length < cl)
                    {
                        int n = c.Read(buf, 0, buf.Length);
                        if (n <= 0) break;
                        reqBody += Encoding.ASCII.GetString(buf, 0, n);
                    }
                }
                if (path == "/besar.bin")
                {
                    // a download: 20000 bytes, honouring "Range: bytes=N-"
                    var all = BigFile();
                    c.TimeoutMs = 300;    // a cancelled client stops reading: give up soon
                    long from = 0;
                    int ri = line.IndexOf("Range: bytes=");
                    if (ri >= 0)
                    {
                        int e = line.IndexOf('-', ri);
                        long.TryParse(line.Substring(ri + 13, e - ri - 13), out from);
                    }
                    if (from >= all.Length)
                    {
                        var h = Encoding.ASCII.GetBytes("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        c.Write(h, 0, h.Length);
                    }
                    else
                    {
                        string head = (from > 0 ? "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes " + from + "-" + (all.Length - 1) + "/" + all.Length + "\r\n" : "HTTP/1.1 200 OK\r\n")
                            + "Content-Type: application/octet-stream\r\nContent-Length: " + (all.Length - from) + "\r\nConnection: close\r\n\r\n";
                        var h = Encoding.ASCII.GetBytes(head);
                        c.Write(h, 0, h.Length);
                        for (long o = from; o < all.Length; o += 16384) c.Write(all, (int)o, (int)Math.Min(16384, all.Length - o));
                    }
                }
                else if (path == "/ws")
                {
                    c.TimeoutMs = 60000; // a slow emulator: the client may take a while between messages
                    var ws = WebSocket.Accept(c, line);
                    if (ws != null) EchoWs(ws);
                }
                else if (path == "/")
                {
                    string body = "<html><head><title>Uji Peramban</title><script>document.write('x')</script></head><body>" +
                        "<h1>Halo &amp; selamat datang</h1><p>Paragraf <b>tebal</b> dengan <a href=\"/dua\">tautan ke dua</a>.</p>" +
                        "<img src=\"/gambar.png\" alt=\"anggrek\"><ul><li>satu</li><li>dua</li></ul><pre>kode  dua\nbaris</pre></body></html>";
                    Reply(c, "200 OK", "text/html; charset=utf-8", Encoding.UTF8.GetBytes(body), chunked: false);
                }
                else if (path == "/dua")
                    Reply(c, "200 OK", "text/html", Encoding.UTF8.GetBytes("<title>Halaman Dua</title><p>Isi halaman kedua, dikirim berpotongan (chunked).</p><a href=\"/\">kembali</a>"), chunked: true);
                else if (path == "/lama")
                {
                    var h = Encoding.ASCII.GetBytes("HTTP/1.1 302 Found\r\nLocation: /dua\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    c.Write(h, 0, h.Length);
                }
                else if (path == "/kuki")
                {
                    var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nSet-Cookie: sesi=abc123; Path=/; HttpOnly\r\n" +
                        "Set-Cookie: tema=hijau; Max-Age=3600\r\nSet-Cookie: asing=1; Domain=example.com\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
                    c.Write(h, 0, h.Length);
                }
                else if (path == "/kuki-hapus")
                {
                    var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nSet-Cookie: sesi=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    c.Write(h, 0, h.Length);
                }
                else if (path == "/kuki-cek")
                    Reply(c, "200 OK", "text/plain", Encoding.UTF8.GetBytes("cookie=" + HeaderValue(line, "Cookie")), chunked: false);
                else if (path == "/formulir")
                    Reply(c, "200 OK", "text/html", Encoding.UTF8.GetBytes("<title>Formulir</title>" +
                        "<form action=\"/kirim\" method=\"post\"><input name=\"nama\" value=\"Ani\"> <input type=\"checkbox\" name=\"setuju\" checked>" +
                        "<input type=\"checkbox\" name=\"tidak\"><select name=\"kota\"><option>Bogor</option><option value=\"jkt\" selected>Jakarta</option></select>" +
                        "<textarea name=\"pesan\">halo dunia</textarea><input type=\"hidden\" name=\"token\" value=\"t&1\">" +
                        "<input type=\"submit\" name=\"aksi\" value=\"Kirim\"></form>" +
                        "<form action=\"/cari\"><input name=\"q\" placeholder=\"cari\"></form>" +
                        "<form action=\"/kirim\" method=\"post\"><input name=\"a\"><input name=\"b\"></form>" +
                        "<form action=\"/kirim\" method=\"post\" onsubmit=\"document.title = 'batal'; return false\"><input type=\"submit\" value=\"Batal\"></form>"), chunked: false);
                else if (path == "/kirim")
                    Reply(c, "200 OK", "text/html", Encoding.UTF8.GetBytes("<title>Terkirim</title><p>" + (line.StartsWith("POST") ? "POST" : "GET") + " diterima: " + Escape(reqBody) + "</p>"), chunked: false);
                else if (path.StartsWith("/cari?"))
                    Reply(c, "200 OK", "text/html", Encoding.UTF8.GetBytes("<title>Hasil</title><p>cari: " + Escape(path.Substring(6)) + "</p>"), chunked: false);
                else if (path == "/gambar.png")
                    Reply(c, "200 OK", img[0] == 'B' ? "image/bmp" : "image/png", img, chunked: false);
                else
                    Reply(c, "404 Not Found", "text/html", Encoding.UTF8.GetBytes("<title>404</title><p>tidak ada</p>"), chunked: false);
            }
            catch (Exception e) { ServerError = e.Message; }
            c.Close();
        }
        server.Close();
    }

    /// <summary>One end of an in-memory byte pipe (tests: framing without TCP).</summary>
    sealed class Pipe : INetStream
    {
        byte[] _buf = new byte[4096];
        int _start, _end;
        Pipe? _peer;
        public int TimeoutMs { get; set; }
        public static (Pipe, Pipe) Pair()
        {
            var a = new Pipe();
            var b = new Pipe();
            a._peer = b;
            b._peer = a;
            return (a, b);
        }
        void Take(byte[] data, int offset, int count)
        {
            if (_end + count > _buf.Length)
            {
                var n = new byte[Math.Max(_buf.Length * 2, _end - _start + count)];
                Array.Copy(_buf, _start, n, 0, _end - _start);
                _end -= _start;
                _start = 0;
                _buf = n;
            }
            Array.Copy(data, offset, _buf, _end, count);
            _end += count;
        }
        public void Write(byte[] data, int offset, int count) => _peer!.Take(data, offset, count);
        public int Read(byte[] buffer, int offset, int count)
        {
            int n = Math.Min(count, _end - _start);
            Array.Copy(_buf, _start, buffer, offset, n);
            _start += n;
            return n;
        }
        public void Close() { }
    }

    static byte[]? _bigFile;

    /// <summary>The download test's file (made once: a byte loop is slow in the interpreter).</summary>
    static byte[] BigFile()
    {
        if (_bigFile != null) return _bigFile;
        var b = new byte[20000];
        for (int i = 0; i < b.Length; i++) b[i] = (byte)(i * 13 + (i >> 9));
        _bigFile = b;
        return b;
    }

    static bool SameFile(byte[]? got, byte[] want)
    {
        if (got == null || got.Length != want.Length) return false;
        for (int i = 0; i < want.Length; i += 997) if (got[i] != want[i]) return false;
        return got[want.Length - 1] == want[want.Length - 1];
    }

    /// <summary>The test server's WebSocket: echo every message; before echoing
    /// "ping-me" it pings the client first (the client must answer on its own).</summary>
    /// <summary>The test server's last exception (shown in a failed check).</summary>
    static string ServerError = "";

    static void EchoWs(WebSocket ws)
    {
        while (true)
        {
            var m = ws.Receive();
            if (m == null) return;
            if (m.IsText && m.Text == "ping-me") ws.Ping(new byte[] { 1, 2, 3 });
            if (m.IsText) ws.Send(m.Text); else ws.Send(m.Data);
        }
    }

    static void Reply(TcpConnection c, string status, string type, byte[] body, bool chunked)
    {
        string head = "HTTP/1.1 " + status + "\r\nContent-Type: " + type + "\r\nConnection: close\r\n" +
            (chunked ? "Transfer-Encoding: chunked\r\n" : "Content-Length: " + body.Length + "\r\n") + "\r\n";
        var h = Encoding.ASCII.GetBytes(head);
        c.Write(h, 0, h.Length);
        if (!chunked) { c.Write(body, 0, body.Length); return; }
        for (int i = 0; i < body.Length; i += 17)
        {
            int n = Math.Min(17, body.Length - i);
            var size = Encoding.ASCII.GetBytes(n.ToString("x") + "\r\n");
            c.Write(size, 0, size.Length);
            c.Write(body, i, n);
            c.Write(new byte[] { 13, 10 }, 0, 2);
        }
        var end = Encoding.ASCII.GetBytes("0\r\n\r\n");
        c.Write(end, 0, end.Length);
    }

    public static Checks SelfTest(bool internet = false)
    {
        var c = new Checks("Peramban");
        var p = Html.Parse("<html><head><title> Judul  Uji </title><style>p{}</style></head><body><h1>A &amp; B</h1><p>teks <a href=\"x.htm\">tautan</a> &#65;&#x42;</p><script>var a='<p>';</script><img src=\"i.png\" width=\"40\" height=\"20\"></body>");
        c.That(p.Title == "Judul Uji" && p.Links.Count == 1 && p.Images.Count == 1, "parse: title, links, images (" + p.Title + ")");
        var (items, height) = Html.Layout(p, 400, new Palette { Text = 1, Link = 2, Heading = 3 });
        bool heading = items.Exists(i => i.Kind == ItemKind.Text && i.Style == 2 && i.Text == "A & B");
        bool link = items.Exists(i => i.Href == "x.htm" && i.Text == "tautan");
        bool ent = items.Exists(i => i.Text.Contains("AB"));
        bool noScript = !items.Exists(i => i.Text.Contains("var"));
        c.That(heading && link && ent && noScript, "layout: heading, link, entities, script skipped");
        c.That(items.Exists(i => i.Kind == ItemKind.Image && i.W == 40 && i.H == 20) && height > 40, "image box from width/height");
        // CSS: <style>, a fetched sheet (AddCss), style="", class / id / descendant selectors
        var cp = Html.Parse("<style>.box{background:#123456;color:#ffffff;border:1px solid #ff0000} .hid{display:none} #t h2{text-align:center}" +
            " span.b{font-weight:bold} a:hover{color:red}</style><div class=box><p>isi kotak</p></div><p class=hid>rahasia <b>sekali</b></p>" +
            "<section id=t><h2>tengah</h2></section><p style=\"color:#00ff00\">hijau <span class=b>tebal</span></p><p class=extra>biru</p>");
        int addedRules = Html.AddCss(cp, "/* fetched */ p.extra { color: #0000ff !important }");
        var (ci, _) = Html.Layout(cp, 400, new Palette { Text = 1, Link = 2, Heading = 3, Rule = 4, Muted = 5 });
        var bar = ci.Find(i => i.Kind == ItemKind.Bar);
        var inBox = ci.Find(i => i.Kind == ItemKind.Text && i.Text.Contains("isi kotak"));
        c.That(bar != null && bar.Color == 0x123456 && inBox != null && inBox.Color == 0xFFFFFF && inBox.Y >= bar.Y && inBox.Y + inBox.H <= bar.Y + bar.H
            && ci.IndexOf(bar) < ci.IndexOf(inBox) && ci.Exists(i => i.Kind == ItemKind.Frame && i.Color == 0xFF0000), "CSS: background + border box, text color inside it");
        var centered = ci.Find(i => i.Text == "tengah");
        c.That(!ci.Exists(i => i.Text.Contains("rahasia") || i.Text.Contains("sekali")) && centered != null && centered.X > 100, "CSS: display:none hidden, text-align:center by descendant selector (x " + (centered?.X ?? -1) + ")");
        var green = ci.Find(i => i.Text.StartsWith("hijau"));
        var bold = ci.Find(i => i.Text == "tebal");
        var blue = ci.Find(i => i.Text == "biru");
        c.That(green != null && green.Color == 0x00FF00 && bold != null && bold.Style == 1 && bold.Color == 0x00FF00 && addedRules == 1 && blue != null && blue.Color == 0x0000FF,
            "CSS: style attribute, inherited color, bold from a class, a fetched sheet");
        Html.Free(cp);
        var wrap = Html.Layout(Html.Parse("<p>" + string.Join(" ", Enumerable.Repeat("kata", 60)) + "</p>"), 200, default).Item1;
        int lines = 0; int lastY = -1;
        foreach (var it in wrap) if (it.Y != lastY) { lines++; lastY = it.Y; }
        c.That(lines >= 10, "long paragraph wraps (" + lines + " lines at 200 px)");
        var u = Url.Parse("https://Example.com:8443/a/b/c.htm?x=1#top");
        c.That(u != null && u.Secure && u.Host == "example.com" && u.Port == 8443 && u.Path == "/a/b/c.htm?x=1", "URL parse");
        c.That(u!.Resolve("../d.htm")?.ToString() == "https://example.com:8443/a/d.htm" && u.Resolve("/e")?.ToString() == "https://example.com:8443/e"
            && u.Resolve("//other.org/x")?.ToString() == "https://other.org/x", "relative links resolve");
        c.That(Encoding.ASCII.GetString(HttpClient.Dechunk(Encoding.ASCII.GetBytes("3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"))) == "abcde", "chunked decoding");

        // JavaScript: the host engine runs the page's scripts (off the device pages run none)
        {
            var app = new BrowserApp(headless: true) { Synchronous = true };
            app.Navigate("about:js");
            var pg = app.Current;
            if (pg != null && pg.NativeId > 0)
            {
                string Text(string start) { var it = app.View.Items.Find(i => i.Text.StartsWith(start)); return it == null ? "" : it.Text; }
                bool Has(string t) => app.View.Items.Exists(i => i.Text.Contains(t));
                c.That(Has("Skrip berjalan: 3 butir") && Has("kantong") && !Has("belum berjalan"), "JS: script ran, DOM built (createElement/appendChild/querySelectorAll)");
                c.That(app.ConsoleLines.Exists(l => l.Contains("demo siap") && l.Contains("{\"butir\":3}")), "JS: console.log + JSON (" + string.Join(" | ", app.ConsoleLines) + ")");
                int btn = pg.Links.FindIndex(l => l.StartsWith("js:"));
                app.ClickLink(btn);
                app.ClickLink(btn);
                c.That(Has("Diklik 2 kali"), "JS: addEventListener click + closure + template literal (" + Text("Diklik") + ")");
                int field = pg.Links.FindIndex(l => l.StartsWith("input:"));
                app.EnterText(field, "Bogor");
                c.That(Has("Halo, Bogor!") && Html.FieldText(pg, field) == "Bogor", "JS: text field + oninput event");
                int hash = pg.Links.IndexOf("#");
                string before = app.CurrentUrl;
                app.ClickLink(hash);
                var warna = app.View.Items.Find(i => i.Text.StartsWith("teks"));
                c.That(app.CurrentUrl == before && warna != null && warna.Color == 0x2E7D32, "JS: inline onclick, style change, no navigation");
                c.That(Has("menunggu"), "JS: timer not due yet");
                app.ClockOffset += 200;
                app.TickScripts();
                c.That(Has("waktu habis") && !Has("menunggu"), "JS: setTimeout + innerHTML after 100 ms");
                var ep = Html.Parse("<p id=x>a</p><script>document.getElementById('x').textContent = undefinedThing;</script><script>document.getElementById('x').textContent = 'lanjut';</script>");
                Html.RunScripts(ep, "about:x", 0, 400);
                var errs = Html.TakeConsole(ep);
                var (ei, _) = Html.Layout(ep, 400, default);
                c.That(errs.Exists(l => l.Contains("ReferenceError") && l.Contains("undefinedThing")) && ei.Exists(i => i.Text == "lanjut"), "JS: an error is reported and the next script still runs");
                Html.Free(ep);
            }
        }

        // the network: a server thread in this program, the client over TCP
        bool stop = false;
        TcpServer? server = null;
        try { server = new TcpServer(TestPort); } catch (Exception e) { c.That(false, "listen on " + TestPort + ": " + e.Message); }
        if (server != null)
        {
            var th = new Thread(() => Serve(server, () => stop));
            th.Start();
            try
            {
                var http = new HttpClient { TimeoutMs = 8000 };
                var r = http.Get("http://localhost:" + TestPort + "/");
                c.That(r.Status == 200 && r.Text.Contains("selamat datang") && r.ContentType.StartsWith("text/html"), "HTTP GET over TCP: " + r.Status + ", " + r.Body.Length + " bytes");
                var r2 = http.Get("http://localhost:" + TestPort + "/lama");
                c.That(r2.Status == 200 && r2.Url.EndsWith("/dua") && r2.Text.Contains("berpotongan"), "302 redirect followed + chunked body (" + r2.Url + ")");
                var r3 = http.Get("http://localhost:" + TestPort + "/tidak-ada");
                c.That(r3.Status == 404, "404 reported");

                // cookies: stored from Set-Cookie, sent back, a foreign domain refused, deleted by a past date
                var jar = new CookieJar();
                var hc = new HttpClient { TimeoutMs = 8000, Cookies = jar };
                hc.Get("http://localhost:" + TestPort + "/kuki");
                string sent = hc.Get("http://localhost:" + TestPort + "/kuki-cek").Text;
                bool persistOne = jar.Save().Contains("tema\thijau") && !jar.Save().Contains("sesi");
                hc.Get("http://localhost:" + TestPort + "/kuki-hapus");
                string after = hc.Get("http://localhost:" + TestPort + "/kuki-cek").Text;
                c.That(sent.Contains("sesi=abc123") && sent.Contains("tema=hijau") && !sent.Contains("asing") && persistOne
                    && !after.Contains("sesi") && after.Contains("tema=hijau"), "cookies: set, sent, foreign domain refused, saved, deleted (" + sent + " / " + after + ")");

                var app = new BrowserApp(headless: true) { Synchronous = true };
                app.Navigate("http://localhost:" + TestPort + "/");
                c.That(app.Current != null && app.Current.Title == "Uji Peramban" && app.LastError.Length == 0, "browser shows the page: " + (app.Current?.Title ?? app.LastError));
                var img = app.Current?.Images.Count > 0 ? app.Current.Images[0] : null;
                c.That(img?.Picture != null && img.Picture.Width > 0, "the page's image loaded (" + (img?.Picture == null ? "none" : img.Picture.Format + " " + img.Picture.Width + "x" + img.Picture.Height) + ")");
                app.W.RenderFrame();
                c.That(app.View.ImagesDrawn == 1 && app.W.LastFrameOps > 10, "drawn: " + app.W.LastFrameOps + " ops, image on screen");
                var linkItem = app.View.Items.Find(i => i.Href == "/dua");
                if (linkItem != null)
                {
                    var b = app.View.Bounds;
                    app.W.SimulateClickAt(b.X + linkItem.X + 2, b.Y + linkItem.Y - app.View.Scroll + 2);
                }
                c.That(app.Current?.Title == "Halaman Dua", "clicking the link navigates (" + app.Current?.Title + ")");
                app.Back();
                c.That(app.Current?.Title == "Uji Peramban", "back");
                // HTML forms (the page engine is the host's on the device; off it pages have no fields)
                app.Navigate("http://localhost:" + TestPort + "/formulir");
                var fp = app.Current;
                if (fp != null && fp.NativeId > 0)
                {
                    int Button(string label) { var it = app.View.Items.Find(i => i.Text == "[" + label + "]" && i.LinkIndex >= 0); return it == null ? -1 : it.LinkIndex; }
                    app.ClickLink(Button("Kirim"));
                    bool posted = app.Current?.Title == "Terkirim" && app.View.Items.Exists(i => i.Text.Contains("nama=Ani&setuju=on&kota=jkt&pesan=halo+dunia&token=t%261&aksi=Kirim"));
                    c.That(posted, "form POST: fields, checkbox, select, textarea, hidden, the button (" + string.Join(" ", app.View.Items.ConvertAll(i => i.Text)) + ")");
                    // text fields in document order: nama, pesan (textarea), q, a, b
                    List<int> Fields() { var v = new List<int>(); for (int i = 0; i < app.Current!.Links.Count; i++) if (app.Current.Links[i].StartsWith("input:")) v.Add(i); return v; }
                    app.Back();
                    int q = Fields()[2];
                    app.EnterText(q, "kebun raya");
                    bool sentGet = app.Submit(q, pressed: false);
                    c.That(sentGet && app.CurrentUrl.EndsWith("/cari?q=kebun+raya") && app.View.Items.Exists(i => i.Text.Contains("q=kebun+raya")), "form GET by Enter in its only field (" + app.CurrentUrl + ")");
                    app.Back();
                    fp = app.Current!;
                    string at = app.CurrentUrl;
                    bool none = !app.Submit(Fields()[3], pressed: false);
                    app.ClickLink(Button("Batal"));
                    c.That(none && app.CurrentUrl == at && app.Current?.Title == "batal", "Enter in a two-field form without a button sends nothing; onsubmit return false cancels (" + app.Current?.Title + ")");
                    bool implicitPost = Fields().Count > 0 && app.Submit(Fields()[0], pressed: false) && app.Current?.Title == "Terkirim" && app.View.Items.Exists(i => i.Text.Contains("&aksi=Kirim"));
                    c.That(implicitPost, "Enter in a field submits through the form's default button");
                    // DNS AAAA for apps (DNS_RESOLVE6): localhost = our link-local address, then TCP over IPv6 to the test server
                    var v6 = Dns.Resolve6("localhost");
                    bool tcp6 = false;
                    if (v6 != null && v6[0] == 0xFE && v6[1] == 0x80)
                    {
                        try
                        {
                            var hexs = new List<string>();
                            for (int k = 0; k < 16; k += 2) hexs.Add(((v6[k] << 8) | v6[k + 1]).ToString("x"));
                            var conn = TcpConnection.Connect(string.Join(":", hexs), TestPort, 5000);
                            var rq = Encoding.ASCII.GetBytes("GET /kuki-cek HTTP/1.0\r\n\r\n");
                            conn.Write(rq, 0, rq.Length);
                            var rb = new byte[512];
                            int got = conn.Read(rb, 0, rb.Length);
                            tcp6 = got > 0 && Encoding.ASCII.GetString(rb, 0, got).StartsWith("HTTP/1.1 200");
                            conn.Close();
                        }
                        catch (Exception) { }
                    }
                    c.That(tcp6, "DNS AAAA (localhost) + HTTP over IPv6");
                }
                app.Navigate("about:home");
                c.That(app.Current?.Title == "Beranda Peramban" && app.Current.Links.Count >= 4, "about:home");

                // tabs: a second tab keeps its own page + history, switching back restores the first
                app.Navigate("about:help");
                int second = app.NewTab("about:js");
                bool own = app.Tabs.Count == 2 && second == 1 && app.Current?.Title == "Demo JavaScript" && !app.CanGoBack;
                app.SwitchTab(0);
                bool back0 = app.Current?.Title == "Bantuan Peramban" && app.CanGoBack;
                app.Back();
                bool hist0 = app.Current?.Title == "Beranda Peramban";
                app.SwitchTab(1);
                bool kept1 = app.Current?.Title == "Demo JavaScript";
                app.CloseTab(1);
                c.That(own && back0 && hist0 && kept1 && app.Tabs.Count == 1 && app.ActiveTab == 0 && app.Current?.Title == "Beranda Peramban",
                    "tabs: own page and history per tab, switch, close");

                // bookmarks: add (once), saved to a file, listed on about:bookmarks, removed
                string keep = BrowserApp.BookmarkFile;
                BrowserApp.BookmarkFile = Files.ScratchDirectory + "/TANDAUJI.TXT";
                try
                {
                    app.Bookmarks.Clear();
                    app.Navigate("about:help");
                    bool added = app.AddBookmark() && !app.AddBookmark();
                    app.LoadBookmarks();
                    bool saved = app.Bookmarks.Count == 1 && app.Bookmarks[0].Url == "about:help" && app.Bookmarks[0].Title == "Bantuan Peramban";
                    app.Navigate("about:bookmarks");
                    bool listed = app.Current != null && app.Current.Links.Exists(l => l == "about:help");
                    app.Navigate("about:unmark?0");
                    app.LoadBookmarks();
                    c.That(added && saved && listed && app.Bookmarks.Count == 0 && app.Current?.Title == "Tanda", "bookmarks: add once, saved, listed, removed");
                }
                finally { BrowserApp.BookmarkFile = keep; app.LoadBookmarks(); }
                app.Navigate("http://localhost:1/");
                c.That(app.LastError.Length > 0 && app.Current?.Title == "Tidak dapat memuat", "an unreachable address shows an error page (" + app.LastError + ")");
            }
            catch (Exception e) { c.That(false, "network: " + e.Message); }
            try
            {
                // Buitenzorg.Security.Cryptography: known test vectors
                c.That(Hash.ToHex(Hash.Sha256("abc")) == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                    && Hash.ToHex(Hash.Hmac(HashAlgorithm.Sha256, Hash.FromHex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"), System.Text.Encoding.UTF8.GetBytes("Hi There")))
                       == "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7", "crypto: SHA-256 (FIPS 180) + HMAC (RFC 4231)");
                var gcm = new AesGcm(new byte[16]);
                var ct = gcm.Encrypt(new byte[12], new byte[16]);
                var bad = new byte[ct.Length]; for (int i = 0; i < ct.Length; i++) bad[i] = ct[i]; bad[3] ^= 1;
                var pt = gcm.Decrypt(new byte[12], ct);
                c.That(Hash.ToHex(ct) == "0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf" && pt != null && pt.Length == 16 && pt[0] == 0
                    && gcm.Decrypt(new byte[12], bad) == null, "crypto: AES-GCM (test case 3), tampered ciphertext refused");
                var ec = EcdsaP256.Generate();
                var msg = System.Text.Encoding.UTF8.GetBytes("tanda tangan Buitenzorg");
                var sig = ec.Sign(msg);
                c.That(ec.PublicKey.Length == 65 && sig.Length == 64 && EcdsaP256.Verify(ec.PublicKey, msg, sig)
                    && !EcdsaP256.Verify(ec.PublicKey, System.Text.Encoding.UTF8.GetBytes("tanda tangan lain"), sig), "crypto: ECDSA P-256 sign + verify");
                var rn = Hash.FromHex("ada16516670d6fba84027e5d50ba645fadcbf420e92274b76197c2665aba37866ee4eac532a06d4b71d32a7fe77fd9a2cdfceaaa201e0a7f45cf7894926490f57fd2f39dfb6cf3329ba73c3a139ae54d86acf73f20976c89ae5489d4581319910d9e5e33c3c89e78bfe15961a0aa4e99bcc79082c5596c7dfcb980b848df6bebba0f8128cb27dbfa585241019c663dbc5a81d098a7cf5b19ce40c70ee09ceb4dc5599cdb98353206a937e21aff4453104edaac3700311d4da637dbea40fe1532e66e0784902e1b1d8addf2525d8d309e99137b28907e562ceb6ab100ff8005f2dbc382871487582defa5eea64d2bf25ceeffd23a5773d884d02ba1703d392c09");
                var rs = Hash.FromHex("aca85bb88c6f23b27f2f6a0c8d08b7dcb82fff483509f4acc3124b1b6dd5853393da792f1b2d006869b780479828d9a6d1f74dffceaf091748a792569448f95fc943dfc603e07f67de78ddd4fce52a13d731785b255927afc0462679902d184d3969d41c1072ec9b384cd389f5dcdafce73f7be48ca0906edbaec8046353cce811ec4c371245e3f17c2b29dc9d413cda7ee374f463d76fd1c594c0c8c93f51eeec52d29f73ff98c8b1790ef1caa4bbd5813a687e1400871497565cb9f8e3c20f2d4a46e4807f1c6fd8c1fbb71bea271a4c7d4c0cb0ad2fbba3bce6c0e3c883ed7be712afa5c16fd37ef00def0fde49e896d5caddee2c29c90ba3b6d9ed8c518a");
                var e3 = new byte[] { 1, 0, 1 };
                c.That(Rsa.VerifyPkcs1Sha256(rn, e3, System.Text.Encoding.UTF8.GetBytes("Buitenzorg"), rs)
                    && !Rsa.VerifyPkcs1Sha256(rn, e3, System.Text.Encoding.UTF8.GetBytes("buitenzorg"), rs), "crypto: RSA-2048 PKCS#1 v1.5 signature (made by Python cryptography)");
                int roots = RootCertificates.Count;
                bool isrg = false;
                for (int i = 0; i < roots; i++) if (RootCertificates.Name(i) == "ISRG Root X1") isrg = true;
                c.That(roots >= 4 && isrg && SecureRandom.Bytes(32).Length == 32, "crypto: trusted roots (" + roots + ", ISRG Root X1 among them) + random bytes");
            }
            catch (Exception e) { c.That(false, "crypto: " + e.Message); }
            try
            {
                c.That(WebSocket.AcceptKey("dGhlIHNhbXBsZSBub25jZQ==") == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=", "WebSocket accept key (RFC 6455 example)");
                var ws = WebSocket.Connect("ws://localhost:" + TestPort + "/ws", 30000);
                try
                {
                ws.Send("halo websocket");
                var m1 = ws.Receive();
                c.That(m1 != null && m1.IsText && m1.Text == "halo websocket", "WebSocket text echo");
                var mid = new byte[300];
                for (int i = 0; i < mid.Length; i++) mid[i] = (byte)(i * 7);
                ws.Send(mid);
                var m2 = ws.Receive();
                bool same = m2 != null && !m2.IsText && m2.Data.Length == 300;
                for (int i = 0; same && i < 300; i++) same = m2!.Data[i] == mid[i];
                c.That(same, "WebSocket binary echo, 16-bit length");
                // a 64-bit length frame (> 65535 bytes) over an in-memory pipe:
                // the same framing code without 130 KB of emulated TCP
                var (pa, pb) = Pipe.Pair();
                var wc = WebSocket.Over(pa, true);
                var wsrv = WebSocket.Over(pb, false);
                // (sent by the server side: its frames are not masked, so no
                // per-byte loop in the interpreter -- the length is what is tested)
                var big = new byte[66000];
                big[0] = 1; big[1234] = 9; big[65999] = 7;
                wsrv.Send(big);
                var m3 = wc.Receive();
                c.That(m3 != null && m3.Data.Length == 66000 && m3.Data[65999] == 7 && m3.Data[1234] == 9 && m3.Data[0] == 1, "WebSocket 64-bit length (66000 bytes)");
                ws.Send("ping-me");
                var m4 = ws.Receive();
                c.That(m4 != null && m4.Text == "ping-me", "WebSocket ping from the server answered" + (m4 == null ? " (closed; server: " + ServerError + ")" : ""));
                ws.Close();
                c.That(!ws.IsOpen && ws.CloseCode == 1000, "WebSocket close handshake (" + ws.CloseCode + ")");
                }
                finally { if (ws.IsOpen) ws.Close(); }   // a failed check must not leave the test server busy
            }
            catch (Exception e) { c.That(false, "WebSocket: " + e.Message); }
            try
            {
                var want = BigFile();
                string dl = Files.ScratchDirectory + "/UNDUH.BIN";
                Files.Delete(dl);
                var http = new HttpClient { TimeoutMs = 30000, DownloadPiece = 4096 };
                int calls = 0;
                var r = http.Download("http://localhost:" + TestPort + "/besar.bin", dl, (done, total) => { calls++; });
                c.That(r.Status == 200 && r.Saved == 20000 && !r.Cancelled && SameFile(Files.ReadAllBytes(dl), want) && calls >= 2,
                    "download 20000 bytes to a file in pieces (" + r.Saved + ", " + calls + " progress calls)");
                // stop after the first piece, then continue it
                Files.Delete(dl);
                var r2 = http.Download("http://localhost:" + TestPort + "/besar.bin", dl, null, () => Files.Size(dl) > 0);
                long part = Files.Size(dl);
                c.That(r2.Cancelled && part > 0 && part < 20000, "download cancelled after " + part + " bytes");
                var r3 = http.Download("http://localhost:" + TestPort + "/besar.bin", dl);
                c.That(r3.Status == 206 && r3.Resumed && r3.Saved == 20000 && SameFile(Files.ReadAllBytes(dl), want), "download resumed with Range (206) and complete");
                var r4 = http.Download("http://localhost:" + TestPort + "/besar.bin", dl);
                c.That(r4.Status == 416 && r4.Saved == 20000, "a complete file is not fetched again (416)");
                Files.Delete(dl);
            }
            catch (Exception e) { c.That(false, "download: " + e.Message); }
            var a6 = TcpConnection.ParseIpv6("fe80::1:a");
            var b6 = TcpConnection.ParseIpv6("2001:db8:0:0:0:0:0:ff");
            c.That(a6 != null && a6[0] == 0xfe && a6[1] == 0x80 && a6[13] == 1 && a6[15] == 0x0a && b6 != null && b6[3] == 0xb8 && b6[15] == 0xff
                && TcpConnection.ParseIpv6("1::2::3") == null && TcpConnection.ParseIpv6("12345::") == null, "IPv6 address literals for TCP over IPv6");
            stop = true;
            th.Join();
        }
        if (internet)
        {
            try
            {
                var r = new HttpClient().Get("https://example.com/");
                c.That(r.Status == 200 && r.Text.Contains("Example Domain") && r.Tls.Length > 0, "HTTPS example.com: " + r.Status + " " + r.Tls);
            }
            catch (Exception e) { c.That(false, "HTTPS example.com: " + e.Message); }
        }
        return c;
    }
}

public static class Program
{
    /// <summary>`run mweb --bench URL`: where the time of one page goes.</summary>
    static void Bench(string url)
    {
        long t = Environment.TickCount64;
        long Lap() { long n = Environment.TickCount64, d = n - t; t = n; return d; }
        var u = Url.Parse(url)!;
        long ip = Dns.Resolve(u.Host);
        Console.WriteLine("bench: dns " + Lap() + " ms (" + Dns.Format(ip) + ")");
        INetStream s = u.Secure ? TlsConnection.Connect(u.Host, u.Port) : TcpConnection.Connect(u.Host, u.Port);
        Console.WriteLine("bench: connect" + (u.Secure ? " + TLS " : " ") + Lap() + " ms");
        var req = Encoding.ASCII.GetBytes("GET " + u.Path + " HTTP/1.1\r\nHost: " + u.Host + "\r\nUser-Agent: Buitenzorg/1.0\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n");
        s.Write(req, 0, req.Length);
        var buf = new byte[32768];
        long total = 0; int reads = 0;
        while (true) { int n = s.Read(buf, 0, buf.Length); if (n <= 0) break; total += n; reads++; }
        s.Close();
        long dl = Lap();
        Console.WriteLine("bench: download " + total + " bytes in " + reads + " reads, " + dl + " ms (" + (dl > 0 ? total / dl : 0) + " KB/s)");
        var r = new Buitenzorg.Net.HttpClient().Get(url);
        Console.WriteLine("bench: HttpClient.Get " + Lap() + " ms, " + r.Body.Length + " bytes");
        string text = r.Text;
        Console.WriteLine("bench: UTF-8 decode " + Lap() + " ms");
        var page = Html.Parse(text);
        Console.WriteLine("bench: parse " + Lap() + " ms (" + page.Tokens.Count + " tokens, " + page.Links.Count + " links)");
        var (items, h) = Html.Layout(page, 820, default);
        Console.WriteLine("bench: layout " + Lap() + " ms (" + items.Count + " items, " + h + " px)");
    }

    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = BrowserApp.SelfTest(internet: Array.IndexOf(args, "--internet") >= 0);
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        if (Array.IndexOf(args, "--bench") >= 0) { Bench(args[args.Length - 1]); return 0; }
        var b = new BrowserApp(headless: false);
        b.LoadCookies();
        string start = "about:home";
        foreach (var a in args) if (!a.StartsWith("--") && a != "&") start = BrowserApp.Normalize(a);
        b.Navigate(start);
        app.Run(b.W);
        return 0;
    }
}
