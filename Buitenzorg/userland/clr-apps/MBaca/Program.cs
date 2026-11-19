// Baca -- the e-book reader: EPUB books (.EPU on the FAT disk) and plain-text
// files, split into pages that fit the window, chapter by chapter; remembers
// where each book was left (/disk/BACA.CFG). EPUB archives are opened by the
// host (ZipList / ZipText natives, inflate); chapters are laid out with
// Peramban's page engine (MWeb.Html). One original story ships on every
// image: /disk/BENIH.EPU, "Benih dari Kebun Raya". Comics: a .CBZ (a ZIP of
// pictures, read in natural name order) shows one picture per page, fitted
// to the page (Buitenzorg.Media.Picture decodes PNG / JPEG / GIF / BMP).

using Buitenzorg.UI;
using MWeb;
using Files = Buitenzorg.App.Files;

namespace MBaca;

public sealed class Book
{
    public string Title = "";
    public string Author = "";
    public readonly List<string> ChapterTitles = new List<string>();
    public readonly List<string> ChapterHtml = new List<string>();
    public string Error = "";
    /// <summary>A comic: the archive and its picture entries, one per page.</summary>
    public byte[]? Zip;
    public readonly List<string> ComicPages = new List<string>();
    public bool IsComic => ComicPages.Count > 0;

    static class Native
    {
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string ZipList(byte[] zip);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string ZipText(byte[] zip, string name);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int ZipEntrySize(byte[] zip, string name);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int ZipEntryInto(byte[] zip, string name, byte[] buffer);
    }

    /// <summary>The bytes of one archive entry (null when missing).</summary>
    public static byte[]? Entry(byte[] zip, string name)
    {
        if (!_managedZip)
        {
            try
            {
                int n = Native.ZipEntrySize(zip, name);
                if (n < 0) return null;
                var buf = new byte[n];
                return Native.ZipEntryInto(zip, name, buf) == n ? buf : null;
            }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _managedZip = true; }
        }
        return StoredZip.Bytes(zip, name);
    }

    /// <summary>A comic book archive: every picture, in natural name order.</summary>
    static Book Comic(byte[] zip, string name)
    {
        var book = new Book { Title = BaseName(name), Zip = zip };
        var pics = new List<string>();
        foreach (var n in List(zip))
            if (!n.EndsWith("/") && Buitenzorg.Media.Picture.IsPictureName(n)) pics.Add(n);
        pics.Sort((a, b) => Ordinal(NaturalKey(a), NaturalKey(b)));
        foreach (var n in pics)
        {
            book.ComicPages.Add(n);
            book.ChapterTitles.Add("Halaman " + (book.ComicPages.Count) + "  " + BaseName("/" + n));
            book.ChapterHtml.Add("");
        }
        if (pics.Count == 0) book.Error = "CBZ tanpa gambar";
        return book;
    }

    static int Ordinal(string a, string b)
    {
        int n = Math.Min(a.Length, b.Length);
        for (int i = 0; i < n; i++) if (a[i] != b[i]) return a[i] - b[i];
        return a.Length - b.Length;
    }

    /// <summary>"page2.png" before "page10.png": numbers padded to 8 digits.</summary>
    static string NaturalKey(string n)
    {
        var sb = new System.Text.StringBuilder();
        int i = 0;
        string u = n.ToUpperInvariant();
        while (i < u.Length)
        {
            if (char.IsDigit(u[i]))
            {
                int j = i;
                while (j < u.Length && char.IsDigit(u[j])) j++;
                for (int z = j - i; z < 8; z++) sb.Append('0');
                sb.Append(u.Substring(i, j - i));
                i = j;
            }
            else sb.Append(u[i++]);
        }
        return sb.ToString();
    }

    static bool _managedZip;

    /// <summary>The entry names of a ZIP archive.</summary>
    static List<string> List(byte[] zip)
    {
        var names = new List<string>();
        if (!_managedZip)
        {
            try { foreach (var n in Native.ZipList(zip).Split('\n')) if (n.Length > 0) names.Add(n); return names; }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _managedZip = true; }
        }
        return StoredZip.Names(zip);
    }

    static string Text(byte[] zip, string name)
    {
        if (!_managedZip)
        {
            try { return Native.ZipText(zip, name); }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _managedZip = true; }
        }
        return StoredZip.Text(zip, name);
    }

    public static Book Load(string path)
    {
        var b = Files.ReadAllBytes(path);
        if (b == null) return new Book { Error = "berkas tidak terbaca" };
        return FromBytes(b, path);
    }

    public static Book FromBytes(byte[] data, string name)
    {
        if (data.Length > 4 && data[0] == 'P' && data[1] == 'K')
            return name.ToUpperInvariant().EndsWith(".CBZ") ? Comic(data, name) : Epub(data, name);
        return Plain(System.Text.Encoding.UTF8.GetString(data), name);
    }

    /// <summary>A text file: paragraphs at blank lines, one chapter.</summary>
    public static Book Plain(string text, string name)
    {
        var book = new Book { Title = BaseName(name) };
        var sb = new System.Text.StringBuilder();
        // paragraphs at blank lines (no Split(string): keep to what RustBCL has)
        var cur = new System.Text.StringBuilder();
        foreach (var line in text.Replace("\r", "").Split('\n'))
        {
            if (line.Trim().Length == 0)
            {
                if (cur.Length > 0) { sb.Append("<p>").Append(Escape(cur.ToString())).Append("</p>"); cur.Clear(); }
                continue;
            }
            if (cur.Length > 0) cur.Append(' ');
            cur.Append(line.Trim());
        }
        if (cur.Length > 0) sb.Append("<p>").Append(Escape(cur.ToString())).Append("</p>");
        book.ChapterTitles.Add(book.Title);
        book.ChapterHtml.Add(sb.ToString());
        return book;
    }

    static Book Epub(byte[] zip, string name)
    {
        var book = new Book { Title = BaseName(name) };
        var names = List(zip);
        string container = Text(zip, "META-INF/container.xml");
        string opfPath = Attr(container, "rootfile", "full-path");
        if (opfPath.Length == 0) { book.Error = "bukan EPUB (META-INF/container.xml tidak ada)"; return book; }
        string opf = Text(zip, opfPath);
        string dir = opfPath.IndexOf('/') >= 0 ? opfPath.Substring(0, opfPath.LastIndexOf('/') + 1) : "";
        string t = Element(opf, "dc:title");
        if (t.Length > 0) book.Title = Html.Decode(t);
        book.Author = Html.Decode(Element(opf, "dc:creator"));
        // manifest id -> href, then the spine in reading order
        var hrefs = new Dictionary<string, string>();
        int p = 0;
        while ((p = opf.IndexOf("<item ", p)) >= 0)
        {
            int end = opf.IndexOf('>', p);
            string tag = opf.Substring(p, end - p);
            hrefs[AttrIn(tag, "id")] = AttrIn(tag, "href");
            p = end;
        }
        p = 0;
        while ((p = opf.IndexOf("<itemref", p)) >= 0)
        {
            int end = opf.IndexOf('>', p);
            string id = AttrIn(opf.Substring(p, end - p), "idref");
            p = end;
            if (!hrefs.TryGetValue(id, out var href)) continue;
            string html = Text(zip, dir + href);
            if (html.Length == 0) continue;
            string title = Html.Decode(Element(html, "title"));
            book.ChapterTitles.Add(title.Length > 0 ? title : "Bab " + (book.ChapterHtml.Count + 1));
            book.ChapterHtml.Add(html);
        }
        if (book.ChapterHtml.Count == 0) book.Error = "EPUB tanpa bab yang terbaca (" + names.Count + " berkas di arsip)";
        return book;
    }

    static string Element(string xml, string tag)
    {
        int a = xml.IndexOf("<" + tag);
        if (a < 0) return "";
        a = xml.IndexOf('>', a) + 1;
        int b = xml.IndexOf("</" + tag, a);
        return b > a ? xml.Substring(a, b - a).Trim() : "";
    }

    static string Attr(string xml, string tag, string attr)
    {
        int a = xml.IndexOf("<" + tag + " "); // "<rootfile " -- not the "<rootfiles>" around it
        if (a < 0) return "";
        int b = xml.IndexOf('>', a);
        return AttrIn(xml.Substring(a, b - a), attr);
    }

    static string AttrIn(string tag, string attr)
    {
        int a = tag.IndexOf(" " + attr + "=\"");
        if (a < 0) return "";
        a += attr.Length + 3;
        int b = tag.IndexOf('"', a);
        return b > a ? tag.Substring(a, b - a) : "";
    }

    public static string BaseName(string path)
    {
        int s = path.LastIndexOf('/'), d = path.LastIndexOf('.');
        return d > s ? path.Substring(s + 1, d - s - 1) : path.Substring(s + 1);
    }

    public static string Escape(string s) => s.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;");
}

/// <summary>Off the device: ZIP entries stored without compression (enough for tests).</summary>
static class StoredZip
{
    static int U16(byte[] d, int o) => d[o] | d[o + 1] << 8;
    static int U32(byte[] d, int o) => d[o] | d[o + 1] << 8 | d[o + 2] << 16 | d[o + 3] << 24;

    static IEnumerable<(string Name, int Method, int Start, int Size)> Walk(byte[] d)
    {
        int p = 0;
        while (p + 30 <= d.Length && d[p] == 'P' && d[p + 1] == 'K' && d[p + 2] == 3 && d[p + 3] == 4)
        {
            int method = U16(d, p + 8), size = U32(d, p + 18), nlen = U16(d, p + 26), xlen = U16(d, p + 28);
            string name = System.Text.Encoding.UTF8.GetString(d, p + 30, nlen);
            int start = p + 30 + nlen + xlen;
            yield return (name, method, start, size);
            p = start + size;
        }
    }

    public static List<string> Names(byte[] d) { var l = new List<string>(); foreach (var e in Walk(d)) l.Add(e.Name); return l; }

    public static string Text(byte[] d, string name)
    {
        foreach (var e in Walk(d))
            if (e.Name == name && e.Method == 0) return System.Text.Encoding.UTF8.GetString(d, e.Start, e.Size);
        return "";
    }

    public static byte[]? Bytes(byte[] d, string name)
    {
        foreach (var e in Walk(d))
            if (e.Name == name && e.Method == 0) { var b = new byte[e.Size]; Array.Copy(d, e.Start, b, 0, e.Size); return b; }
        return null;
    }

    /// <summary>Tests: a stored ZIP.</summary>
    public static byte[] Build(params (string Name, string Text)[] parts)
    {
        var raw = new (string, byte[])[parts.Length];
        for (int i = 0; i < parts.Length; i++) raw[i] = (parts[i].Name, System.Text.Encoding.UTF8.GetBytes(parts[i].Text));
        return BuildBytes(raw);
    }

    public static byte[] BuildBytes(params (string Name, byte[] Data)[] parts)
    {
        // a complete archive: local headers + data, central directory, end record
        // (CRC fields left 0: neither reader here checks them)
        var o = new List<byte>();
        var cd = new List<byte>();
        void W16(List<byte> l, int v) { l.Add((byte)v); l.Add((byte)(v >> 8)); }
        void W32(List<byte> l, int v) { W16(l, v); W16(l, v >> 16); }
        foreach (var (name, data) in parts)
        {
            var nb = System.Text.Encoding.UTF8.GetBytes(name);
            var tb = data;
            int offset = o.Count;
            o.Add((byte)'P'); o.Add((byte)'K'); o.Add(3); o.Add(4);
            W16(o, 20); W16(o, 0); W16(o, 0); W16(o, 0); W16(o, 0); W32(o, 0); W32(o, tb.Length); W32(o, tb.Length); W16(o, nb.Length); W16(o, 0);
            o.AddRange(nb); o.AddRange(tb);
            cd.Add((byte)'P'); cd.Add((byte)'K'); cd.Add(1); cd.Add(2);
            W16(cd, 20); W16(cd, 20); W16(cd, 0); W16(cd, 0); W16(cd, 0); W16(cd, 0); W32(cd, 0); W32(cd, tb.Length); W32(cd, tb.Length);
            W16(cd, nb.Length); W16(cd, 0); W16(cd, 0); W16(cd, 0); W16(cd, 0); W32(cd, 0); W32(cd, offset);
            cd.AddRange(nb);
        }
        int cdOffset = o.Count;
        o.AddRange(cd);
        o.Add((byte)'P'); o.Add((byte)'K'); o.Add(5); o.Add(6);
        W16(o, 0); W16(o, 0); W16(o, parts.Length); W16(o, parts.Length); W32(o, cd.Count); W32(o, cdOffset); W16(o, 0);
        return o.ToArray();
    }
}

/// <summary>One page of the current chapter.</summary>
public sealed class ReaderView : Control
{
    public List<Item> Items = new List<Item>();
    public readonly List<int> PageTops = new List<int>();
    public int Page;
    public Palette Pal = new Palette { Text = 0x2A2418, Link = 0x1565C0, Heading = 0x3B5A2A, Muted = 0x7A6F5C, Rule = 0xD8CDB8, Back = 0xFAF5E8 };
    public event Action<int>? Turn; // -1 back, +1 forward
    public string Footer = "";
    /// <summary>A comic page: the picture shown instead of laid-out text.</summary>
    public Buitenzorg.Media.Picture? Picture;
    int[] _buf = new int[0];

    public ReaderView() { Focusable = true; }

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(200, a.Width), Math.Max(150, a.Height));

    public int PageHeight => Math.Max(60, Bounds.Height - 40);
    public int LayoutWidth => Math.Max(160, Bounds.Width - 40);

    /// <summary>Split the laid-out chapter into pages at line boundaries.</summary>
    public void Paginate(List<Item> items)
    {
        Items = items;
        PageTops.Clear();
        int top = 0, h = PageHeight;
        PageTops.Add(0);
        foreach (var it in items)
        {
            if (it.Y + it.H > top + h && it.Y > top)
            {
                top = it.Y;
                PageTops.Add(top);
            }
        }
        Page = Math.Clamp(Page, 0, PageTops.Count - 1);
        InvalidateVisual();
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(b, 8, Pal.Back);
        if (Picture != null)
        {
            // fit the page into the view (never wider than the source needs)
            var area = new Rect(b.X + 12, b.Y + 10, b.Width - 24, PageHeight);
            var (dw, dh) = Picture.FitSize(area.Width, area.Height, allowUp: true);
            if (dw > 0 && dh > 0)
            {
                if (_buf.Length != dw * dh) _buf = new int[dw * dh];
                Picture.Render(0, _buf, dw, dh, 0, 0, Picture.Width, Picture.Height);
                dc.Blit(area.X + (area.Width - dw) / 2, area.Y + (area.Height - dh) / 2, dw, dh, _buf);
            }
            dc.Text(b.X + 20, b.Bottom - 24, Footer, Pal.Muted);
            return;
        }
        if (PageTops.Count == 0) return;
        int top = PageTops[Page], bottom = Page + 1 < PageTops.Count ? PageTops[Page + 1] : int.MaxValue;
        var clip = new Rect(b.X, b.Y + 8, b.Width, PageHeight + 4);
        dc.PushClip(clip);
        foreach (var it in Items)
        {
            if (it.Y < top || it.Y >= bottom) continue;
            int x = b.X + 20 + it.X - 12, y = b.Y + 12 + it.Y - top;
            if (it.Kind == ItemKind.Text) dc.Text(x, y, it.Text, it.Color, it.Style);
            else if (it.Kind == ItemKind.Rule) dc.FillRect(new Rect(x, y, it.W, 1), it.Color);
            else if (it.Kind == ItemKind.Image) dc.FillRect(new Rect(x, y, it.W, it.H), Pal.Rule);
        }
        dc.PopClip();
        dc.Text(b.X + 20, b.Bottom - 24, Footer, Pal.Muted);
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        Turn?.Invoke(e.X < Bounds.X + Bounds.Width / 3 ? -1 : 1);
        e.Handled = true;
    }

    protected override void OnPointerWheelChanged(PointerEventArgs e) { Turn?.Invoke(e.WheelDelta > 0 ? -1 : 1); e.Handled = true; }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        if (e.Key == Keys.Right || e.Key == Keys.PageDown || e.Key == ' ' || e.Key == Keys.Down) Turn?.Invoke(1);
        else if (e.Key == Keys.Left || e.Key == Keys.PageUp || e.Key == Keys.Up) Turn?.Invoke(-1);
        else return;
        e.Handled = true;
    }
}

public sealed class ReaderApp
{
    public Window W { get; }
    public readonly ReaderView View = new ReaderView();
    public readonly List<string> Paths = new List<string>();
    public readonly List<string> Names = new List<string>();
    readonly ListBox _books;
    readonly ListBox _chapters;
    readonly TextBlock _title = new TextBlock("Baca");
    public Book? Current { get; private set; }
    public string CurrentPath = "";
    public int Chapter { get; private set; }
    Page? _page;
    public const string Bookmarks = "/disk/BACA.CFG";

    public ReaderApp(bool headless)
    {
        W = new Window("Baca", 820, 580, headless);
        Scan();
        _books = new ListBox { ItemsSource = Names, Height = 150 };
        _books.SelectionChanged += l => { if (l.SelectedIndex >= 0 && (l.SelectedIndex >= Paths.Count || Paths[l.SelectedIndex] != CurrentPath)) Open(l.SelectedIndex); };
        _chapters = new ListBox { ItemsSource = new List<string>() };
        _chapters.SelectionChanged += l => { if (l.SelectedIndex >= 0 && l.SelectedIndex != Chapter) ShowChapter(l.SelectedIndex, 0); };
        View.Turn += d => { if (d > 0) Next(); else Prev(); };

        var side = new DockPanel { Width = 200, Margin = new Thickness(0, 0, 12, 0) };
        side.Children.Add(new TextBlock("Buku").WithClass("strong").DockTo(Dock.Top));
        side.Children.Add(_books.DockTo(Dock.Top));
        side.Children.Add(new TextBlock("Bab").WithClass("strong").DockTo(Dock.Top));
        side.Children.Add(_chapters);
        var root = new DockPanel { Margin = new Thickness(14) };
        root.Children.Add(_title.WithClass("strong").DockTo(Dock.Top));
        root.Children.Add(side.DockTo(Dock.Left));
        View.Margin = new Thickness(0, 8, 0, 0);
        root.Children.Add(View);
        W.Content = root;
        Application.Current?.Apply(W);
        W.UpdateLayout();
        if (Paths.Count > 0) Open(0); else _title.Text = "Belum ada buku: salin berkas .EPU (EPUB) atau .TXT ke /disk";
        View.Focus();
    }

    /// <summary>EPUB books (.EPU / .EPUB) and text files larger than 1 KB.</summary>
    public void Scan()
    {
        Paths.Clear(); Names.Clear();
        var dirs = Files.ScratchDirectory == "/disk" ? new[] { "/disk" } : new[] { "/disk", Files.ScratchDirectory };
        foreach (var d in dirs)
        {
            List<Buitenzorg.App.FileEntry> files;
            try { files = Files.List(d); } catch (Exception) { continue; }
            foreach (var f in files)
            {
                string n = f.Name.ToUpperInvariant();
                bool epub = n.EndsWith(".EPU") || n.EndsWith(".EPUB") || n.EndsWith(".CBZ");
                bool text = n.EndsWith(".TXT") && Files.Size(f.Path) > 1024;
                if (!f.IsDirectory && (epub || text)) { Paths.Add(f.Path); Names.Add(f.Name); }
            }
        }
    }

    public bool Open(int i)
    {
        var book = Book.Load(Paths[i]);
        return Show(book, Paths[i]);
    }

    public bool Show(Book book, string path)
    {
        Current = book;
        CurrentPath = path;
        if (book.Error.Length > 0 || book.ChapterHtml.Count == 0) { _title.Text = Book.BaseName(path) + ": " + (book.Error.Length > 0 ? book.Error : "kosong"); return false; }
        _title.Text = book.Title + (book.Author.Length > 0 ? "  -  " + book.Author : "");
        _chapters.ItemsSource = book.ChapterTitles;
        var (ch, pg) = LoadMark(path);
        ShowChapter(Math.Clamp(ch, 0, book.ChapterHtml.Count - 1), pg);
        return true;
    }

    public int Pages => View.PageTops.Count;
    public int PageIndex => View.Page;

    public void ShowChapter(int ch, int page)
    {
        if (Current == null || ch < 0 || ch >= Current.ChapterHtml.Count) return;
        Chapter = ch;
        if (Current.IsComic)
        {
            var bytes = Book.Entry(Current.Zip!, Current.ComicPages[ch]);
            View.Picture = bytes == null ? null : Buitenzorg.Media.Picture.FromBytes(bytes);
            View.Page = 0;
            View.Paginate(new List<Item>());
            _chapters.SelectedIndex = ch;
            Refresh();
            return;
        }
        View.Picture = null;
        if (_page != null) Html.Free(_page);
        _page = Html.Parse(Current.ChapterHtml[ch]);
        var (items, _) = Html.Layout(_page, View.LayoutWidth, View.Pal);
        View.Page = page < 0 ? int.MaxValue : page;
        View.Paginate(items);
        _chapters.SelectedIndex = ch;
        Refresh();
    }

    public void Next()
    {
        if (Current == null) return;
        if (View.Page + 1 < View.PageTops.Count) { View.Page++; Refresh(); }
        else if (Chapter + 1 < Current.ChapterHtml.Count) ShowChapter(Chapter + 1, 0);
    }

    public void Prev()
    {
        if (Current == null) return;
        if (View.Page > 0) { View.Page--; Refresh(); }
        else if (Chapter > 0) ShowChapter(Chapter - 1, -1); // the previous chapter's last page
    }

    void Refresh()
    {
        if (Current == null) return;
        View.Footer = Current.IsComic
            ? "Halaman " + (Chapter + 1) + "/" + Current.ComicPages.Count + "   (panah / Spasi / klik untuk membalik)"
            : "Bab " + (Chapter + 1) + "/" + Current.ChapterHtml.Count + "   -   halaman " + (View.Page + 1) + "/" + View.PageTops.Count + "   (panah / Spasi / klik untuk membalik)";
        View.InvalidateVisual();
        SaveMark();
    }

    // ---- where each book was left: "path<TAB>chapter<TAB>page" lines ----

    static List<string> MarkLines()
    {
        var l = new List<string>();
        try { foreach (var s in Files.ReadAllText(Bookmarks).Split('\n')) if (s.Trim().Length > 0) l.Add(s.TrimEnd('\r')); } catch (Exception) { }
        return l;
    }

    public static (int, int) LoadMark(string path)
    {
        foreach (var line in MarkLines())
        {
            var f = line.Split('\t');
            if (f.Length == 3 && f[0] == path && int.TryParse(f[1], out var c) && int.TryParse(f[2], out var p)) return (c, p);
        }
        return (0, 0);
    }

    void SaveMark()
    {
        var lines = MarkLines();
        for (int k = lines.Count - 1; k >= 0; k--) if (lines[k].StartsWith(CurrentPath + "\t")) lines.RemoveAt(k);
        lines.Add(CurrentPath + "\t" + Chapter + "\t" + View.Page);
        if (lines.Count > 50) lines.RemoveAt(0);
        try { Files.WriteAllText(Bookmarks, string.Join("\n", lines) + "\n"); } catch (Exception) { }
    }

    /// <summary>A w x h 24-bit BMP of one colour (tests).</summary>
    static byte[] Bmp(int w, int h, int rgb)
    {
        int row = (w * 3 + 3) & ~3;
        var b = new byte[54 + row * h];
        b[0] = (byte)'B'; b[1] = (byte)'M';
        void Put(int o, int v) { b[o] = (byte)v; b[o + 1] = (byte)(v >> 8); b[o + 2] = (byte)(v >> 16); b[o + 3] = (byte)(v >> 24); }
        Put(2, b.Length); Put(10, 54); Put(14, 40); Put(18, w); Put(22, h);
        b[26] = 1; b[28] = 24;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int o = 54 + y * row + x * 3;
                b[o] = (byte)rgb; b[o + 1] = (byte)(rgb >> 8); b[o + 2] = (byte)(rgb >> 16);
            }
        return b;
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Baca");
        // a small EPUB built here (stored entries), opened through the same code as the real ones
        var zip = StoredZip.Build(
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", "<container><rootfiles><rootfile full-path=\"OPS/book.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>"),
            ("OPS/book.opf", "<package><metadata><dc:title>Uji &amp; Coba</dc:title><dc:creator>Penguji</dc:creator></metadata><manifest><item id=\"a\" href=\"a.xhtml\"/><item id=\"b\" href=\"b.xhtml\"/></manifest><spine><itemref idref=\"b\"/><itemref idref=\"a\"/></spine></package>"),
            ("OPS/a.xhtml", "<html><head><title>Kedua</title></head><body><h1>Kedua</h1>" + string.Concat(Enumerable.Repeat("<p>Paragraf yang cukup panjang untuk mengisi beberapa halaman pembaca kecil ini.</p>", 40)) + "</body></html>"),
            ("OPS/b.xhtml", "<html><head><title>Pertama</title></head><body><h1>Pertama</h1><p>Halo.</p></body></html>"));
        var book = Book.FromBytes(zip, "/ram/UJI.EPU");
        c.That(book.Error.Length == 0 && book.Title == "Uji & Coba" && book.Author == "Penguji", "EPUB metadata: " + book.Title + " / " + book.Author + book.Error);
        c.That(book.ChapterTitles.Count == 2 && book.ChapterTitles[0] == "Pertama" && book.ChapterTitles[1] == "Kedua", "chapters in spine order (not manifest order)");
        var txt = Book.Plain("Satu dua.\n\nTiga & empat.\r\n\r\nLima.", "/disk/CATATAN.TXT");
        c.That(txt.ChapterHtml[0] == "<p>Satu dua.</p><p>Tiga &amp; empat.</p><p>Lima.</p>" && txt.Title == "CATATAN", "a text file becomes paragraphs");

        var app = new ReaderApp(headless: true);
        app.Show(book, "/ram/UJI.EPU");
        c.That(app.Chapter == 0 && app.Pages == 1, "chapter 1 fits one page");
        app.Next();
        c.That(app.Chapter == 1 && app.PageIndex == 0 && app.Pages > 2, "the next page opens chapter 2 (" + app.Pages + " pages)");
        app.Next(); app.Next();
        c.That(app.PageIndex == 2, "turning pages");
        app.Prev();
        c.That(app.PageIndex == 1, "and back");
        var (ch, pg) = LoadMark("/ram/UJI.EPU");
        c.That(ch == 1 && pg == 1, "the position is saved (" + ch + ", " + pg + ")");
        app.ShowChapter(0, 0);
        app.Prev();
        c.That(app.Chapter == 0 && app.PageIndex == 0, "no page before the first");
        app.W.RenderFrame();
        c.That(app.W.LastFrameOps > 5, "a page drew " + app.W.LastFrameOps + " ops");
        // a comic: pictures in natural order, one per page
        var cbz = StoredZip.BuildBytes(("p10.bmp", Bmp(3, 2, 0x204060)), ("p2.bmp", Bmp(4, 3, 0xC08020)), ("info.txt", new byte[] { 65 }));
        var comic = Book.FromBytes(cbz, "/ram/KOMIK.CBZ");
        c.That(comic.IsComic && comic.ComicPages.Count == 2 && comic.ComicPages[0] == "p2.bmp", "CBZ: 2 pictures, natural order (p2 before p10)");
        app.Show(comic, "/ram/KOMIK.CBZ");
        app.ShowChapter(0, 0); // a saved position (an earlier boot on the same disk) must not decide the test
        c.That(app.View.Picture != null && app.View.Picture.Width == 4, "the first page is the 4x3 picture");
        app.Next();
        c.That(app.Chapter == 1 && app.View.Picture != null && app.View.Picture.Width == 3, "next page: the 3x2 picture");
        app.W.RenderFrame();
        c.That(app.W.LastFrameOps > 2, "a comic page drew " + app.W.LastFrameOps + " ops");
        app.ShowChapter(0, 0); // leave the saved position at the start
        int benih = app.Names.FindIndex(n => n.ToUpperInvariant() == "BENIH.EPU");
        if (benih >= 0)
        {
            var b = Book.Load(app.Paths[benih]);
            c.That(b.Error.Length == 0 && b.Title == "Benih dari Kebun Raya" && b.ChapterHtml.Count == 4, "BENIH.EPU (deflated EPUB) opened: " + b.Title + ", " + b.ChapterHtml.Count + " chapters" + b.Error);
        }
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
            var c = ReaderApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var r = new ReaderApp(headless: false);
        app.Run(r.W);
        return 0;
    }
}
