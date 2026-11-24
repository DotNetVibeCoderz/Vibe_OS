// The page engine of Peramban: a tolerant HTML tokenizer and a flow layout
// into positioned items (text runs, links, images, rules) for the kernel's
// three monospaced fonts: headings, paragraphs, lists, links, images,
// preformatted text, quotes, rules, simple tables (one row per line), and a
// CSS subset (Css.cs: colors, backgrounds, borders, bold/size, alignment,
// display none/block/inline, left indents). No JavaScript.

using System.Text;
using Buitenzorg.UI;

namespace MWeb;

/// <summary>Bar = a block's CSS background (drawn behind its content), Frame = its border.</summary>
public enum ItemKind { Text, Rule, Image, Bar, PreBack, Frame }

public sealed class Item
{
    public ItemKind Kind;
    public int X, Y, W, H;
    public string Text = "";
    public int Style;          // 0 body, 1 bold, 2 heading
    public int Color;
    public string? Href;       // a link's target ("js:N" = a script handler, "input:N" = a text field)
    public int LinkIndex = -1; // index in Page.Links (the host's link number, for HtmlEvent)
    public ImageRef? Image;
}

/// <summary>An &lt;img&gt;: its address, the size it takes on the page, the decoded picture once loaded.</summary>
public sealed class ImageRef
{
    public string Src = "", Alt = "";
    public int AttrW, AttrH;
    public Buitenzorg.Media.Picture? Picture;
    public bool Failed;
}

public sealed class Page
{
    /// <summary>The host's copy of this page (htmlpage.rs), 0 off the device.</summary>
    public long NativeId;
    public string Title = "";
    public readonly List<ImageRef> Images = new List<ImageRef>();
    public readonly List<Token> Tokens = new List<Token>();
    public readonly List<string> Links = new List<string>();
    /// <summary>CSS: rules of the page's style sheets, and the linked sheets' addresses.</summary>
    public readonly List<CssRule> Rules = new List<CssRule>();
    public int CssOrder;
    public readonly List<string> CssLinks = new List<string>();
}

public sealed class Token
{
    public bool IsTag, Closing;
    public string Name = "";   // tag name, lower case
    public string Text = "";   // text content (entities decoded)
    public Dictionary<string, string>? Attrs;
    public string Attr(string n) => Attrs != null && Attrs.TryGetValue(n, out var v) ? v : "";
}

public static class Html
{
    static int _native; // 0 unknown, 1 the host engine, 2 this file's (off the device)

    static class Native
    {
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern long HtmlLoad(string html);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlInfo(long id, int what);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlSetImage(long id, int index, int w, int h);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlLayout(long id, int width, int[] metrics, int[] palette);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlItems(long id, int[] output);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlTexts(long id);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlFree(long id);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlAddCss(long id, string css);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlScripts(long id);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlScriptSource(long id, int index, string code);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlRun(long id, string url, long nowMs, int width);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlEvent(long id, int link, int kind, string text);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int HtmlTick(long id, long nowMs);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlConsole(long id);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlNavigation(long id);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlField(long id, int link);
        [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string HtmlForm(long id, int link, int pressed);
    }

    /// <summary>Add a fetched style sheet (a &lt;link rel=stylesheet&gt;); the rules it held.</summary>
    public static int AddCss(Page p, string css)
    {
        if (p.NativeId > 0) return Native.HtmlAddCss(p.NativeId, css);
        int before = p.Rules.Count;
        Css.ParseSheet(css, ref p.CssOrder, p.Rules);
        return p.Rules.Count - before;
    }

    // ---- JavaScript (on the device: js.rs + dom.rs in the host; off the device pages run no scripts)

    /// <summary>Flags from the script natives.</summary>
    public const int JsChanged = 1, JsTimers = 2, JsNavigate = 4, JsHandled = 8;

    /// <summary>The page's scripts: "" = inline, else the src address to fetch.</summary>
    public static List<string> ScriptSources(Page p)
    {
        var v = new List<string>();
        if (p.NativeId <= 0) return v;
        foreach (var line in Native.HtmlScripts(p.NativeId).Split('\n')) v.Add(line);
        if (v.Count > 0 && v[v.Count - 1].Length == 0) v.RemoveAt(v.Count - 1);
        return v;
    }

    public static void SetScriptSource(Page p, int index, string code) { if (p.NativeId > 0) Native.HtmlScriptSource(p.NativeId, index, code); }

    /// <summary>Run the page's scripts once (they may change it: then re-read it with Refresh).</summary>
    public static int RunScripts(Page p, string url, long nowMs, int width) => p.NativeId > 0 ? Native.HtmlRun(p.NativeId, url, nowMs, width) : 0;

    /// <summary>A click on a link (handlers run; JsHandled = do not follow the address).</summary>
    public static int Click(Page p, int link) => p.NativeId > 0 && link >= 0 ? Native.HtmlEvent(p.NativeId, link, 0, "") : 0;

    /// <summary>Text entered into a field (fires input + change).</summary>
    public static int Input(Page p, int link, string text) => p.NativeId > 0 && link >= 0 ? Native.HtmlEvent(p.NativeId, link, 1, text) : 0;

    /// <summary>Run due timers; -1 = the page has none left.</summary>
    public static int Tick(Page p, long nowMs) => p.NativeId > 0 ? Native.HtmlTick(p.NativeId, nowMs) : -1;

    /// <summary>console.log lines (and script errors) since the last call.</summary>
    public static List<string> TakeConsole(Page p)
    {
        var v = new List<string>();
        if (p.NativeId <= 0) return v;
        foreach (var line in Native.HtmlConsole(p.NativeId).Split('\n')) if (line.Length > 0) v.Add(line);
        return v;
    }

    public static string TakeNavigation(Page p) => p.NativeId > 0 ? Native.HtmlNavigation(p.NativeId) : "";

    /// <summary>Submit the form around link `link` (pressed = that link is the button
    /// that was pressed; else Enter in a field). Null = nothing to send (no form, not a
    /// submit control, or a submit handler cancelled it); else { method, action, body }.
    /// `flags` = what the submit handlers did. Device only, like scripts.</summary>
    public static string[]? Form(Page p, int link, bool pressed, out int flags)
    {
        flags = 0;
        if (p.NativeId <= 0 || link < 0) return null;
        var f = Native.HtmlForm(p.NativeId, link, pressed ? 1 : 0).Split('\n');
        int.TryParse(f[0], out flags);
        if (f.Length < 4) return null;
        return new[] { f[1], f[2], f[3] };
    }

    public static string FieldText(Page p, int link) => p.NativeId > 0 && link >= 0 ? Native.HtmlField(p.NativeId, link) : "";

    /// <summary>After a script changed the page: title, links and images again (loaded pictures kept).</summary>
    public static void Refresh(Page p)
    {
        if (p.NativeId <= 0) return;
        long id = p.NativeId;
        p.Title = Native.HtmlInfo(id, 0);
        var old = new List<ImageRef>(p.Images);
        p.Images.Clear();
        foreach (var line in Native.HtmlInfo(id, 1).Split('\n'))
        {
            if (line.Length == 0) continue;
            var f = line.Split('\t');
            ImageRef? keep = null;
            foreach (var o in old) if (o.Src == f[0]) { keep = o; break; }
            var img = keep ?? new ImageRef { Src = f[0] };
            img.Alt = f.Length > 1 ? f[1] : "";
            if (f.Length > 3) { int.TryParse(f[2], out img.AttrW); int.TryParse(f[3], out img.AttrH); }
            p.Images.Add(img);
        }
        p.Links.Clear();
        foreach (var line in Native.HtmlInfo(id, 2).Split('\n')) if (line.Length > 0) p.Links.Add(line);
    }

    /// <summary>Forget the host's copy of a page.</summary>
    public static void Free(Page p)
    {
        if (p.NativeId > 0) { try { Native.HtmlFree(p.NativeId); } catch (Exception) { } p.NativeId = 0; }
    }

    /// <summary>On the device the host parses (RustCLR strings are UTF-8: indexing a long one is a scan).</summary>
    public static Page Parse(string html)
    {
        if (_native != 2)
        {
            try
            {
                long id = Native.HtmlLoad(html);
                _native = 1;
                var p = new Page { NativeId = id, Title = Native.HtmlInfo(id, 0) };
                foreach (var line in Native.HtmlInfo(id, 1).Split('\n'))
                {
                    if (line.Length == 0) continue;
                    var f = line.Split('\t');
                    var img = new ImageRef { Src = f[0], Alt = f.Length > 1 ? f[1] : "" };
                    if (f.Length > 3) { int.TryParse(f[2], out img.AttrW); int.TryParse(f[3], out img.AttrH); }
                    p.Images.Add(img);
                }
                foreach (var line in Native.HtmlInfo(id, 2).Split('\n')) if (line.Length > 0) p.Links.Add(line);
                foreach (var line in Native.HtmlInfo(id, 3).Split('\n')) if (line.Length > 0) p.CssLinks.Add(line);
                return p;
            }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _native = 2; }
        }
        return ParseManaged(html);
    }

    static Page ParseManaged(string html)
    {
        var page = new Page();
        int i = 0, n = html.Length;
        var text = new StringBuilder();
        string? skipUntil = null; // inside <script>/<style>: everything up to its end tag
        bool inTitle = false;
        void FlushText()
        {
            if (text.Length == 0) return;
            string t = Decode(text.ToString());
            text.Clear();
            if (inTitle) { page.Title += t; return; }
            page.Tokens.Add(new Token { Text = t });
        }
        while (i < n)
        {
            if (skipUntil != null)
            {
                int end = IndexOfIgnoreCase(html, "</" + skipUntil, i);
                if (end < 0) break;
                if (skipUntil == "style") Css.ParseSheet(html.Substring(i, end - i), ref page.CssOrder, page.Rules);
                i = end;
                skipUntil = null;
                continue;
            }
            char c = html[i];
            if (c != '<')
            {
                int lt = html.IndexOf('<', i);
                if (lt < 0) lt = n;
                text.Append(html, i, lt - i); // a whole text run at once (the interpreter is slow per char)
                i = lt;
                continue;
            }
            if (i + 3 < n && html[i + 1] == '!' && html[i + 2] == '-' && html[i + 3] == '-')
            {
                int end = html.IndexOf("-->", i + 4);
                i = end < 0 ? n : end + 3;
                continue;
            }
            if (i + 1 < n && (html[i + 1] == '!' || html[i + 1] == '?'))
            {
                int end = html.IndexOf('>', i);
                i = end < 0 ? n : end + 1;
                continue;
            }
            int close = html.IndexOf('>', i);
            if (close < 0 || !(i + 1 < n && (char.IsLetter(html[i + 1]) || html[i + 1] == '/'))) { text.Append(c); i++; continue; }
            FlushText();
            var tok = ParseTag(html.Substring(i + 1, close - i - 1));
            i = close + 1;
            if (tok.Name.Length == 0) continue;
            if (tok.Name == "title") { inTitle = !tok.Closing; continue; }
            if (!tok.Closing && (tok.Name == "script" || tok.Name == "style" || tok.Name == "noscript" || tok.Name == "svg" || tok.Name == "template"))
            {
                skipUntil = tok.Name;
                continue;
            }
            if (tok.Name == "img" && !tok.Closing)
            {
                var img = new ImageRef { Src = tok.Attr("src"), Alt = tok.Attr("alt") };
                int.TryParse(tok.Attr("width"), out img.AttrW);
                int.TryParse(tok.Attr("height"), out img.AttrH);
                if (img.Src.Length > 0) page.Images.Add(img);
            }
            if (tok.Name == "a" && !tok.Closing && tok.Attr("href").Length > 0) page.Links.Add(tok.Attr("href"));
            if (tok.Name == "link" && tok.Attr("rel").ToLowerInvariant().Contains("stylesheet") && tok.Attr("href").Length > 0) page.CssLinks.Add(tok.Attr("href"));
            page.Tokens.Add(tok);
        }
        FlushText();
        page.Title = Collapse(page.Title).Trim();
        return page;
    }

    static int IndexOfIgnoreCase(string s, string what, int from)
    {
        // the common spellings through the native IndexOf; the per-char scan only as a fallback
        int a = s.IndexOf(what, from), b = s.IndexOf(what.ToUpperInvariant(), from);
        if (a >= 0 || b >= 0) return a < 0 ? b : b < 0 ? a : Math.Min(a, b);
        if (s.IndexOf('<', from) < 0) return -1;
        for (int i = from; i + what.Length <= s.Length; i++)
        {
            int k = 0;
            while (k < what.Length && char.ToLowerInvariant(s[i + k]) == what[k]) k++;
            if (k == what.Length) return i;
        }
        return -1;
    }

    static Token ParseTag(string body)
    {
        var t = new Token { IsTag = true };
        int p = 0;
        if (p < body.Length && body[p] == '/') { t.Closing = true; p++; }
        int start = p;
        while (p < body.Length && (char.IsLetterOrDigit(body[p]) || body[p] == '-')) p++;
        t.Name = body.Substring(start, p - start).ToLowerInvariant();
        while (p < body.Length)
        {
            while (p < body.Length && (char.IsWhiteSpace(body[p]) || body[p] == '/')) p++;
            int ns = p;
            while (p < body.Length && body[p] != '=' && !char.IsWhiteSpace(body[p]) && body[p] != '/') p++;
            if (p == ns) { p++; continue; }
            string name = body.Substring(ns, p - ns).ToLowerInvariant();
            string value = "";
            while (p < body.Length && char.IsWhiteSpace(body[p])) p++;
            if (p < body.Length && body[p] == '=')
            {
                p++;
                while (p < body.Length && char.IsWhiteSpace(body[p])) p++;
                if (p < body.Length && (body[p] == '"' || body[p] == '\''))
                {
                    char q = body[p++];
                    int vs = p;
                    while (p < body.Length && body[p] != q) p++;
                    value = body.Substring(vs, p - vs);
                    p++;
                }
                else
                {
                    int vs = p;
                    while (p < body.Length && !char.IsWhiteSpace(body[p])) p++;
                    value = body.Substring(vs, p - vs);
                }
            }
            t.Attrs ??= new Dictionary<string, string>();
            if (!t.Attrs.ContainsKey(name)) t.Attrs[name] = Decode(value);
        }
        return t;
    }

    /// <summary>Character references, and characters the bitmap font lacks mapped to ASCII.</summary>
    public static string Decode(string s)
    {
        if (s.IndexOf('&') < 0) return MapCommon(s);
        var sb = new StringBuilder(s.Length);
        for (int i = 0; i < s.Length; i++)
        {
            char c = s[i];
            if (c == '&')
            {
                int semi = s.IndexOf(';', i + 1);
                if (semi > i && semi - i <= 10)
                {
                    string e = s.Substring(i + 1, semi - i - 1);
                    int code = -1;
                    if (e.Length > 1 && e[0] == '#')
                    {
                        if (e[1] == 'x' || e[1] == 'X') { if (!int.TryParse(e.Substring(2), System.Globalization.NumberStyles.HexNumber, null, out code)) code = -1; }
                        else if (!int.TryParse(e.Substring(1), out code)) code = -1;
                    }
                    else code = Named(e);
                    if (code >= 0) { sb.Append(Ascii(code)); i = semi; continue; }
                }
            }
            sb.Append(Ascii(c));
        }
        return sb.ToString();
    }

    /// <summary>Typographic characters the bitmap font lacks, mapped with the runtime's native Replace.</summary>
    static string MapCommon(string s) =>
        s.Replace('\u00A0', ' ').Replace('\u2019', '\'').Replace('\u2018', '\'').Replace('\u201C', '"').Replace('\u201D', '"')
         .Replace("\u2014", "--").Replace('\u2013', '-').Replace("\u2026", "...").Replace('\u2022', '*').Replace('\u00B7', '*')
         .Replace('\t', ' ').Replace('\r', ' ');

    static int Named(string e) => e switch
    {
        "amp" => '&', "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'', "nbsp" => ' ',
        "copy" => 0xA9, "reg" => 0xAE, "mdash" => 0x2014, "ndash" => 0x2013, "hellip" => 0x2026,
        "laquo" => 0xAB, "raquo" => 0xBB, "lsquo" => 0x2018, "rsquo" => 0x2019, "ldquo" => 0x201C, "rdquo" => 0x201D,
        "bull" => 0x2022, "middot" => 0xB7, "times" => 0xD7, "deg" => 0xB0, "euro" => 0x20AC, "trade" => 0x2122,
        _ => -1,
    };

    static string Ascii(int c)
    {
        if (c == '\n') return "\n"; // kept for <pre>; Collapse() folds it elsewhere
        if (c == '\t' || c == '\r') return " ";
        if (c >= 32 && c < 127) return ((char)c).ToString();
        return c switch
        {
            0xA0 => " ", 0xA9 => "(c)", 0xAE => "(R)", 0x2122 => "(TM)", 0x2014 => "--", 0x2013 => "-", 0x2026 => "...",
            0xAB => "<<", 0xBB => ">>", 0x2018 or 0x2019 => "'", 0x201C or 0x201D => "\"", 0x2022 or 0xB7 => "*",
            0xD7 => "x", 0xB0 => " deg", 0x20AC => "EUR",
            >= 0xC0 and <= 0xC5 => "A", >= 0xE0 and <= 0xE5 => "a", 0xC7 => "C", 0xE7 => "c",
            >= 0xC8 and <= 0xCB => "E", >= 0xE8 and <= 0xEB => "e", >= 0xCC and <= 0xCF => "I", >= 0xEC and <= 0xEF => "i",
            0xD1 => "N", 0xF1 => "n", >= 0xD2 and <= 0xD6 => "O", >= 0xF2 and <= 0xF6 => "o",
            >= 0xD9 and <= 0xDC => "U", >= 0xF9 and <= 0xFC => "u", 0xDF => "ss",
            >= 0xD800 and <= 0xDFFF => "",
            _ => "?",
        };
    }

    static string Ascii(char c) => Ascii((int)c);

    static bool IsWs(char c) => c == ' ' || c == '\n' || c == '\r' || c == '\t';

    public static string Collapse(string s)
    {
        if (s.Length > 64)
        {
            // long runs: let the runtime's native Replace/Split/Join do the work
            var parts = s.Replace('\n', ' ').Replace('\r', ' ').Replace('\t', ' ').Split(' ');
            var keep = new List<string>(parts.Length);
            foreach (var p in parts) if (p.Length > 0) keep.Add(p);
            string joined = string.Join(" ", keep);
            bool lead = IsWs(s[0]), trail = IsWs(s[s.Length - 1]);
            if (joined.Length == 0) return lead || trail ? " " : "";
            return (lead ? " " : "") + joined + (trail ? " " : "");
        }
        var sb = new StringBuilder(s.Length);
        bool space = false;
        foreach (char c in s)
        {
            if (c == ' ' || c == '\n' || c == '\r' || c == '\t') { space = true; continue; }
            if (space && sb.Length > 0) sb.Append(' ');
            space = false;
            sb.Append(c);
        }
        if (space && sb.Length > 0) sb.Append(' ');
        return sb.ToString();
    }

    static bool IsBlock(string n) => n switch
    {
        "p" or "div" or "section" or "article" or "header" or "footer" or "nav" or "main" or "aside" or "form"
        or "ul" or "ol" or "li" or "dl" or "dt" or "dd" or "table" or "tr" or "blockquote" or "pre" or "figure"
        or "figcaption" or "center" or "address" or "fieldset" or "details" or "summary" or "body" or "html"
        or "h1" or "h2" or "h3" or "h4" or "h5" or "h6" or "hr" or "caption" or "thead" or "tbody" => true,
        _ => false,
    };

    /// <summary>Lay the page out at a width; returns the items and the total height.</summary>
    public static (List<Item>, int) Layout(Page page, int width, Palette pal)
    {
        if (page.NativeId > 0) return LayoutNative(page, width, pal);
        return LayoutManaged(page, width, pal);
    }

    static (List<Item>, int) LayoutNative(Page page, int width, Palette pal)
    {
        long id = page.NativeId;
        for (int k = 0; k < page.Images.Count; k++)
        {
            var p = page.Images[k].Picture;
            if (p != null) Native.HtmlSetImage(id, k, p.Width, p.Height);
        }
        var metrics = new int[] { Fonts.GlyphWidth(0), Fonts.LineHeight(0), Fonts.GlyphWidth(1), Fonts.LineHeight(1), Fonts.GlyphWidth(2), Fonts.LineHeight(2) };
        var palette = new int[] { pal.Text, pal.Link, pal.Heading, pal.Muted, pal.Rule };
        int count = Native.HtmlLayout(id, width, metrics, palette);
        var items = new List<Item>(Math.Max(0, count));
        if (count <= 0) return (items, 40);
        var data = new int[count * 9 + 1];
        Native.HtmlItems(id, data);
        var texts = Native.HtmlTexts(id).Split('\u0001');
        for (int k = 0; k < count; k++)
        {
            int o = k * 9;
            var it = new Item
            {
                Kind = data[o] switch { 0 => ItemKind.Text, 1 => ItemKind.Rule, 3 => ItemKind.Bar, 4 => ItemKind.Frame, _ => ItemKind.Image },
                X = data[o + 1], Y = data[o + 2], W = data[o + 3], H = data[o + 4], Style = data[o + 5], Color = data[o + 6],
                Text = k < texts.Length ? texts[k] : "",
            };
            int img = data[o + 7], href = data[o + 8];
            if (img >= 0 && img < page.Images.Count) it.Image = page.Images[img];
            if (href >= 0 && href < page.Links.Count) { it.Href = page.Links[href]; it.LinkIndex = href; }
            items.Add(it);
        }
        return (items, data[count * 9]);
    }

    static (List<Item>, int) LayoutManaged(Page page, int width, Palette pal)
    {
        var items = new List<Item>();
        int margin = 12, indent = 0;
        int y = 8, x = margin, lineH = Fonts.LineHeight(0);
        var lineItems = new List<Item>();
        int bold = 0, heading = 0, pre = 0, quote = 0;
        string? href = null;
        var lists = new List<int>(); // -1 = bullet, n = next number
        bool pendingSpace = false;
        int lastBlockGap = 0;
        var frames = new List<Frame>();
        int hidden = -1;           // frames index of a display:none element
        int? linkColor = null, pageBg = null;

        int Left() => margin + indent;
        int Right() => width - margin;
        CssStyle Top() => frames.Count > 0 ? frames[frames.Count - 1].St : default;
        int Style()
        {
            int f = Top().Font;
            if (heading is 1 or 2 || f == 3) return 2;
            if (((heading > 0 || bold > 0 || f == 2) && f != 1) || (f == 1 && (bold > 0 || heading > 2))) return 1;
            return 0;
        }
        int ColorOf() => href != null ? (linkColor ?? pal.Link) : Top().Color ?? (heading > 0 ? pal.Heading : pal.Text);

        void EndLine()
        {
            if (lineItems.Count == 0) return;
            int h = 0;
            foreach (var it in lineItems) h = Math.Max(h, it.H);
            int align = Top().Align;
            if (align != 0)
            {
                int x0 = int.MaxValue, x1 = 0;
                foreach (var it in lineItems) { x0 = Math.Min(x0, it.X); x1 = Math.Max(x1, it.X + it.W); }
                int room = Right() - x1;
                int shift = align == 1 ? (room - (x0 - Left())) / 2 : room;
                if (shift > 0) foreach (var it in lineItems) it.X += shift;
            }
            foreach (var it in lineItems)
                if (it.Kind == ItemKind.Text) it.Y = y + h - it.H; // baseline-ish: bottoms aligned
            items.AddRange(lineItems);
            lineItems.Clear();
            y += h + 2;
            x = Left();
            pendingSpace = false;
            lastBlockGap = 0;
        }

        void Gap(int g)
        {
            EndLine();
            if (g > lastBlockGap) { y += g - lastBlockGap; lastBlockGap = g; }
            x = Left();
        }

        void Word(string w)
        {
            int st = Style();
            int gw = Fonts.GlyphWidth(st), lh = Fonts.LineHeight(st);
            int need = w.Length * gw + (pendingSpace && x > Left() ? gw : 0);
            if (x + need > Right() && x > Left()) EndLine();
            if (pendingSpace && x > Left()) x += gw;
            pendingSpace = false;
            // a word longer than the line: split it
            int room = Math.Max(1, (Right() - x) / gw);
            while (w.Length > room && room > 0)
            {
                lineItems.Add(new Item { Kind = ItemKind.Text, X = x, W = room * gw, H = lh, Text = w.Substring(0, room), Style = st, Color = ColorOf(), Href = href });
                w = w.Substring(room);
                EndLine();
                room = Math.Max(1, (Right() - x) / gw);
            }
            // merge with the previous run when nothing changed (fewer draw ops)
            var last = lineItems.Count > 0 ? lineItems[lineItems.Count - 1] : null;
            if (last != null && last.Kind == ItemKind.Text && last.Style == st && last.Href == href && last.Color == ColorOf() && last.X + last.W <= x)
            {
                int pad = (x - (last.X + last.W)) / gw;
                last.Text += new string(' ', pad) + w;
                last.W = last.Text.Length * gw;
            }
            else lineItems.Add(new Item { Kind = ItemKind.Text, X = x, W = w.Length * gw, H = lh, Text = w, Style = st, Color = ColorOf(), Href = href });
            x += w.Length * gw;
        }

        CssStyle Push(Token t, bool blockDefault)
        {
            var parent = Top();
            var anc = new List<CssElem>();
            foreach (var f in frames) anc.Add(new CssElem(f.Tag, f.Id, f.Class));
            var st = page.Rules.Count == 0 && t.Attr("style").Length == 0 ? parent.Inherit()
                : Css.Compute(parent, new CssElem(t.Name, t.Attr("id"), t.Attr("class")), anc, page.Rules, t.Attr("style"));
            bool block = st.Display == 2 ? true : st.Display == 3 ? false : blockDefault;
            frames.Add(new Frame { Tag = t.Name, Id = t.Attr("id"), Class = t.Attr("class"), St = st, Block = block });
            return st;
        }

        void StartBox()
        {
            var f = frames[frames.Count - 1];
            if (f.Tag == "body" || f.Tag == "html") { if (f.St.Background != null) pageBg = f.St.Background; }
            else if (f.Block && (f.St.Background != null || f.St.Border != null))
            {
                EndLine();
                f.Boxed = true; f.BoxItem = items.Count; f.BoxY = y; f.BoxL = Left(); f.BoxR = Right();
                y += 4;
            }
            if (f.Block && f.St.Indent > 0) { indent += f.St.Indent; f.Indent = f.St.Indent; x = Left(); }
            if (f.Tag == "a") linkColor = f.St.OwnColor ? f.St.Color : null;
        }

        bool? PopTo(string name)
        {
            int at = -1;
            for (int k = frames.Count - 1; k >= 0; k--) if (frames[k].Tag == name) { at = k; break; }
            if (at < 0) return null;
            bool block = false;
            while (frames.Count > at)
            {
                var f = frames[frames.Count - 1];
                if (f.Block) EndLine();
                if (f.Boxed)
                {
                    y += 4;
                    int h = Math.Max(2, y - f.BoxY), k = f.BoxItem;
                    if (f.St.Background != null) items.Insert(k++, new Item { Kind = ItemKind.Bar, X = f.BoxL, Y = f.BoxY, W = f.BoxR - f.BoxL, H = h, Color = f.St.Background.Value });
                    if (f.St.Border != null) items.Insert(k, new Item { Kind = ItemKind.Frame, X = f.BoxL, Y = f.BoxY, W = f.BoxR - f.BoxL, H = h, Color = f.St.Border.Value });
                    y += 2;
                    lastBlockGap = 0;
                }
                indent = Math.Max(0, indent - f.Indent);
                if (f.Tag == "a") linkColor = null;
                block = f.Block;
                frames.RemoveAt(frames.Count - 1);
            }
            x = Left();
            return block;
        }

        int imgIndex = 0;
        void TagLogic(Token t, bool blk)
        {
                string nm = t.Name;
                if (nm == "br") { if (lineItems.Count == 0) y += lineH; EndLine(); return; }
                if (nm == "hr")
                {
                    Gap(8);
                    items.Add(new Item { Kind = ItemKind.Rule, X = Left(), Y = y, W = Right() - Left(), H = 1, Color = pal.Rule });
                    y += 9; lastBlockGap = 8;
                    return;
                }
                if (nm == "img" && !t.Closing)
                {
                    if (imgIndex >= page.Images.Count) return;
                    var img = page.Images[imgIndex++];
                    var (w, h) = ImageSize(img, Right() - Left());
                    if (x + w > Right() && x > Left()) EndLine();
                    if (pendingSpace && x > Left()) x += Fonts.GlyphWidth(0);
                    lineItems.Add(new Item { Kind = ItemKind.Image, X = x, W = w, H = h, Image = img, Href = href, Text = img.Alt });
                    x += w;
                    pendingSpace = false;
                    return;
                }
                if (nm == "a") { href = t.Closing ? null : (t.Attr("href").Length > 0 ? t.Attr("href") : null); return; }
                if (nm == "b" || nm == "strong" || nm == "th") { bold += t.Closing ? -1 : 1; if (bold < 0) bold = 0; if (nm != "th") return; }
                if (nm == "td" || nm == "th") { if (!t.Closing && x > Left()) { x += Fonts.GlyphWidth(0) * 2; pendingSpace = false; } return; }
                if (!blk) return; // inline (or made inline by display)
                if (!IsBlock(nm)) { Gap(2); return; } // an inline element made a block by display

                // block elements
                if (nm.Length == 2 && nm[0] == 'h' && nm[1] >= '1' && nm[1] <= '6')
                {
                    Gap(t.Closing ? 6 : 12);
                    heading = t.Closing ? 0 : nm[1] - '0';
                    return;
                }
                if (nm == "ul" || nm == "ol")
                {
                    if (!t.Closing) { Gap(lists.Count == 0 ? 6 : 0); lists.Add(nm == "ol" ? 1 : -1); indent += 24; }
                    else if (lists.Count > 0) { lists.RemoveAt(lists.Count - 1); indent = Math.Max(0, indent - 24); Gap(lists.Count == 0 ? 6 : 0); }
                    return;
                }
                if (nm == "li")
                {
                    Gap(t.Closing ? 0 : 2);
                    if (!t.Closing)
                    {
                        string mark = "*";
                        if (lists.Count > 0 && lists[lists.Count - 1] > 0) { mark = lists[lists.Count - 1] + "."; lists[lists.Count - 1]++; }
                        int gw = Fonts.GlyphWidth(0);
                        items.Add(new Item { Kind = ItemKind.Text, X = Left() - (mark.Length + 1) * gw, Y = y, W = mark.Length * gw, H = Fonts.LineHeight(0), Text = mark, Color = pal.Muted });
                    }
                    return;
                }
                if (nm == "pre")
                {
                    Gap(8);
                    if (!t.Closing) { pre++; indent += 12; }
                    else { pre = Math.Max(0, pre - 1); indent = Math.Max(0, indent - 12); }
                    return;
                }
                if (nm == "blockquote")
                {
                    Gap(8);
                    if (!t.Closing) { quote++; indent += 20; }
                    else { quote = Math.Max(0, quote - 1); indent = Math.Max(0, indent - 20); }
                    return;
                }
                if (nm == "dd") { Gap(0); indent += t.Closing ? -20 : 20; if (indent < 0) indent = 0; return; }
                Gap(nm == "p" || nm == "table" || nm == "figure" ? 10 : nm == "tr" ? 0 : 2);
        }

        foreach (var t in page.Tokens)
        {
            if (hidden >= 0)
            {
                // inside a display:none element: only track nesting
                if (t.IsTag && !IsVoid(t.Name))
                {
                    if (t.Closing)
                    {
                        for (int k = frames.Count - 1; k >= 0; k--) if (frames[k].Tag == t.Name) { frames.RemoveRange(k, frames.Count - k); break; }
                    }
                    else frames.Add(new Frame { Tag = t.Name, Id = "", Class = "" });
                }
                if (frames.Count <= hidden) hidden = -1;
                continue;
            }
            if (t.IsTag)
            {
                bool isVoid = IsVoid(t.Name);
                bool blk = IsBlock(t.Name);
                if (t.Closing && !isVoid)
                {
                    var b = PopTo(t.Name);
                    if (b == null) continue; // a stray closing tag
                    blk = b.Value;
                }
                if (!t.Closing && !isVoid)
                {
                    var st = Push(t, blk);
                    if (st.Display == 1) { hidden = frames.Count - 1; continue; }
                    blk = frames[frames.Count - 1].Block;
                }
                TagLogic(t, blk);
                if (!t.Closing && !isVoid) StartBox();
                continue;
            }
            if (!t.IsTag)
            {
                if (pre > 0)
                {
                    var lines = t.Text.Replace("\r", "").Split('\n');
                    // Decode() turned newlines into spaces; pre keeps the raw text in the token
                    for (int li = 0; li < lines.Length; li++)
                    {
                        if (li > 0) { EndLine(); if (lineItems.Count == 0) y += 0; }
                        if (lines[li].Length > 0) { pendingSpace = false; Word(lines[li]); }
                    }
                    continue;
                }
                string s = t.Text;
                bool lead = s.Length > 0 && s[0] == ' ';
                var words = Collapse(s).Trim().Split(' ');
                if (lead) pendingSpace = true;
                bool any = false;
                foreach (var w in words)
                {
                    if (w.Length == 0) continue;
                    if (any) pendingSpace = true;
                    Word(w);
                    any = true;
                }
                if (s.Length > 0 && s[s.Length - 1] == ' ' && any) pendingSpace = true;
                continue;
            }
        }
        while (frames.Count > 0) PopTo(frames[frames.Count - 1].Tag); // elements never closed
        EndLine();
        if (pageBg != null) items.Insert(0, new Item { Kind = ItemKind.Bar, X = 0, Y = 0, W = width, H = y + 16, Color = pageBg.Value });
        return (items, y + 16);
    }

    sealed class Frame
    {
        public string Tag = "", Id = "", Class = "";
        public CssStyle St;
        public bool Block, Boxed;
        public int BoxItem, BoxY, BoxL, BoxR, Indent;
    }

    static bool IsVoid(string n) => n is "br" or "hr" or "img" or "input" or "meta" or "link" or "area" or "base" or "col" or "embed" or "source" or "wbr" or "param" or "track";

    /// <summary>The size an image takes: its own (or its attributes'), at most the line width.</summary>
    public static (int, int) ImageSize(ImageRef img, int maxW)
    {
        int w = img.AttrW, h = img.AttrH;
        if (img.Picture != null)
        {
            int pw = img.Picture.Width, ph = img.Picture.Height;
            if (w > 0 && h <= 0) h = (int)((long)ph * w / Math.Max(1, pw));
            else if (h > 0 && w <= 0) w = (int)((long)pw * h / Math.Max(1, ph));
            else if (w <= 0) { w = pw; h = ph; }
        }
        else if (w <= 0 || h <= 0) { w = w > 0 ? w : 120; h = h > 0 ? h : 90; }
        if (w > maxW) { h = (int)((long)h * maxW / Math.Max(1, w)); w = maxW; }
        return (Math.Max(4, w), Math.Max(4, h));
    }
}

public struct Palette
{
    public int Text, Link, Heading, Muted, Rule, Back;
}
