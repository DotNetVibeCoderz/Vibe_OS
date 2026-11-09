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
    public event Action<string>? LinkClicked;
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
        if (it != null) { LinkClicked?.Invoke(it.Href!); e.Handled = true; }
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
    readonly List<string> _back = new List<string>(), _forward = new List<string>();
    int _nav; // navigation id: a late answer to an older request is dropped
    public bool Loading { get; private set; }
    public bool Synchronous; // tests: fetch on the calling thread

    public BrowserApp(bool headless)
    {
        W = new Window("Peramban", 860, 600, headless);
        View.Pal = new Palette { Text = 0x1F2A24, Link = 0x1565C0, Heading = 0x1B4D2E, Muted = 0x6B7A71, Rule = 0xD5DDD8, Back = 0xFBFCF8 };
        View.LinkClicked += href => { var u = Resolve(href); if (u != null) Navigate(u); };
        View.Hover += h => { if (!Loading) _status.Text = h.Length > 0 ? Resolve(h) ?? h : StatusIdle(); };
        _address.KeyDown += (_, e) => { if (e.Key == Keys.Enter) { Navigate(Normalize(_address.Text)); e.Handled = true; } };

        var bar = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        var nav = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, Margin = new Thickness(0, 0, 8, 0) };
        nav.Children.Add(new Button("<", _ => Back()).WithClass("subtle"));
        nav.Children.Add(new Button(">", _ => Forward()).WithClass("subtle"));
        nav.Children.Add(new Button("Muat ulang", _ => Reload()).WithClass("subtle"));
        nav.Children.Add(new Button("Beranda", _ => Navigate("about:home")).WithClass("subtle"));
        bar.Children.Add(nav.DockTo(Dock.Left));
        bar.Children.Add(new Button("Buka", _ => Navigate(Normalize(_address.Text))).WithClass("accent").DockTo(Dock.Right));
        _address.Margin = new Thickness(0, 0, 8, 0);
        bar.Children.Add(_address);

        var root = new DockPanel { Margin = new Thickness(12) };
        root.Children.Add(_titleText.WithClass("strong").DockTo(Dock.Top));
        root.Children.Add(bar.DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        View.Margin = new Thickness(0, 0, 0, 6);
        root.Children.Add(View);
        W.Content = root;
        W.UnhandledKey += e =>
        {
            if (e.Key == Keys.F5) { Reload(); e.Handled = true; }
            else if (e.Key == (char)12) { _address.Focus(); e.Handled = true; } // Ctrl+L
        };
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

    public void Navigate(string url, bool record = true)
    {
        if (record && CurrentUrl.Length > 0 && CurrentUrl != url) { _back.Add(CurrentUrl); _forward.Clear(); }
        _address.Text = url;
        Loading = true;
        _status.Text = "Memuat " + url + " ...";
        int id = ++_nav;
        if (Synchronous) { var r = Fetch(url); Show(id, r); LoadImages(id, r.Page, r.Url, sync: true); return; }
        var t = new Thread(() =>
        {
            var r = Fetch(url);
            Dispatcher.Post(() => Show(id, r));
            LoadImages(id, r.Page, r.Url, sync: false);
        });
        t.Start();
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
    public static Result Fetch(string url)
    {
        var r = new Result { Url = url };
        try
        {
            if (url.StartsWith("about:")) { r.Page = Html.Parse(AboutPage(url)); return r; }
            if (url.StartsWith("/"))
            {
                var bytes = Files.ReadAllBytes(url);
                if (bytes == null) throw new NetException("berkas " + url + " tidak ada");
                r.Page = Picture.IsPictureName(url) ? ImagePage(url) : Html.Parse(Encoding.UTF8.GetString(bytes));
                return r;
            }
            var http = new HttpClient();
            var resp = http.Get(url);
            r.Url = resp.Url;
            string ct = resp.ContentType.ToLowerInvariant();
            r.Info = resp.Status + " " + resp.Reason + (resp.Tls.Length > 0 ? "  -  terenkripsi (" + resp.Tls + ")" : "");
            if (ct.StartsWith("image/")) r.Page = ImagePage(resp.Url);
            else if (ct.StartsWith("text/plain")) r.Page = Html.Parse("<pre>" + Escape(resp.Text) + "</pre>");
            else r.Page = Html.Parse(resp.Text);
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
        Relayout();
        if (!_address.IsFocused) View.Focus(); // the keys scroll the page at once
        _status.Text = r.Error.Length > 0 ? "Gagal: " + r.Error : (r.Info.Length > 0 ? r.Info + "  -  " : "") + StatusIdle();
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
                        var resp = new HttpClient { MaxBody = 6 << 20 }.Get(abs);
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

    public static string AboutPage(string url)
    {
        if (url == "about:home" || url == "about:blank" && false)
            return "<title>Beranda Peramban</title>" +
                "<h1>Peramban Buitenzorg</h1>" +
                "<p>Peramban web bawaan Buitenzorg OS: <b>HTTP</b> dan <b>HTTPS</b> (sertifikat diperiksa terhadap " +
                "akar CA Mozilla), gambar PNG/JPEG/GIF, tautan, riwayat. Tanpa JavaScript dan CSS -- halaman " +
                "ditampilkan menurut strukturnya.</p>" +
                "<h2>Coba</h2><ul>" +
                "<li><a href=\"https://example.com/\">example.com</a> (HTTPS)</li>" +
                "<li><a href=\"http://info.cern.ch/hypertext/WWW/TheProject.html\">Situs web pertama di dunia</a> (CERN, 1991)</li>" +
                "<li><a href=\"https://text.npr.org/\">text.npr.org</a> -- berita, versi teks</li>" +
                "<li><a href=\"https://lite.duckduckgo.com/lite/\">DuckDuckGo Lite</a> -- pencarian</li>" +
                "<li><a href=\"/disk/KEBUN.JPG\">Foto Kebun Raya</a> (berkas di /disk)</li>" +
                "<li><a href=\"about:help\">Bantuan</a></li></ul>" +
                "<hr><p>Buitenzorg OS -- dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.</p>";
        if (url == "about:help")
            return "<title>Bantuan Peramban</title><h1>Bantuan</h1><ul>" +
                "<li>Ketik alamat lalu Enter; tanpa http:// dianggap http.</li>" +
                "<li>Ctrl+L ke kolom alamat, F5 muat ulang.</li>" +
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
                if (path == "/")
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
                else if (path == "/gambar.png")
                    Reply(c, "200 OK", img[0] == 'B' ? "image/bmp" : "image/png", img, chunked: false);
                else
                    Reply(c, "404 Not Found", "text/html", Encoding.UTF8.GetBytes("<title>404</title><p>tidak ada</p>"), chunked: false);
            }
            catch (Exception) { }
            c.Close();
        }
        server.Close();
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
        var wrap = Html.Layout(Html.Parse("<p>" + string.Join(" ", Enumerable.Repeat("kata", 60)) + "</p>"), 200, default).Item1;
        int lines = 0; int lastY = -1;
        foreach (var it in wrap) if (it.Y != lastY) { lines++; lastY = it.Y; }
        c.That(lines >= 10, "long paragraph wraps (" + lines + " lines at 200 px)");
        var u = Url.Parse("https://Example.com:8443/a/b/c.htm?x=1#top");
        c.That(u != null && u.Secure && u.Host == "example.com" && u.Port == 8443 && u.Path == "/a/b/c.htm?x=1", "URL parse");
        c.That(u!.Resolve("../d.htm")?.ToString() == "https://example.com:8443/a/d.htm" && u.Resolve("/e")?.ToString() == "https://example.com:8443/e"
            && u.Resolve("//other.org/x")?.ToString() == "https://other.org/x", "relative links resolve");
        c.That(Encoding.ASCII.GetString(HttpClient.Dechunk(Encoding.ASCII.GetBytes("3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"))) == "abcde", "chunked decoding");

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
                app.Navigate("about:home");
                c.That(app.Current?.Title == "Beranda Peramban" && app.Current.Links.Count >= 4, "about:home");
                app.Navigate("http://localhost:1/");
                c.That(app.LastError.Length > 0 && app.Current?.Title == "Tidak dapat memuat", "an unreachable address shows an error page (" + app.LastError + ")");
            }
            catch (Exception e) { c.That(false, "network: " + e.Message); }
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
        string start = "about:home";
        foreach (var a in args) if (!a.StartsWith("--") && a != "&") start = BrowserApp.Normalize(a);
        b.Navigate(start);
        app.Run(b.W);
        return 0;
    }
}
