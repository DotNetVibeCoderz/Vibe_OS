// The page engine of Peramban: a tolerant HTML tokenizer and a flow layout
// into positioned items (text runs, links, images, rules) for the kernel's
// three monospaced fonts. No CSS and no JavaScript: structure only --
// headings, paragraphs, lists, links, images, preformatted text, quotes,
// rules, simple tables (one row per line).

using System.Text;
using Buitenzorg.UI;

namespace MWeb;

public enum ItemKind { Text, Rule, Image, Bar, PreBack }

public sealed class Item
{
    public ItemKind Kind;
    public int X, Y, W, H;
    public string Text = "";
    public int Style;          // 0 body, 1 bold, 2 heading
    public int Color;
    public string? Href;       // a link's target
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
                Kind = data[o] == 0 ? ItemKind.Text : data[o] == 1 ? ItemKind.Rule : ItemKind.Image,
                X = data[o + 1], Y = data[o + 2], W = data[o + 3], H = data[o + 4], Style = data[o + 5], Color = data[o + 6],
                Text = k < texts.Length ? texts[k] : "",
            };
            int img = data[o + 7], href = data[o + 8];
            if (img >= 0 && img < page.Images.Count) it.Image = page.Images[img];
            if (href >= 0 && href < page.Links.Count) it.Href = page.Links[href];
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

        int Left() => margin + indent;
        int Right() => width - margin;
        int Style() => heading is 1 or 2 ? 2 : (heading > 0 || bold > 0) ? 1 : 0;
        int ColorOf() => href != null ? pal.Link : heading > 0 ? pal.Heading : pal.Text;

        void EndLine()
        {
            if (lineItems.Count == 0) return;
            int h = 0;
            foreach (var it in lineItems) h = Math.Max(h, it.H);
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

        int imgIndex = 0;
        foreach (var t in page.Tokens)
        {
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
            string nm = t.Name;
            if (nm == "br") { if (lineItems.Count == 0) y += lineH; EndLine(); continue; }
            if (nm == "hr")
            {
                Gap(8);
                items.Add(new Item { Kind = ItemKind.Rule, X = Left(), Y = y, W = Right() - Left(), H = 1, Color = pal.Rule });
                y += 9; lastBlockGap = 8;
                continue;
            }
            if (nm == "img" && !t.Closing)
            {
                if (imgIndex >= page.Images.Count) continue;
                var img = page.Images[imgIndex++];
                var (w, h) = ImageSize(img, Right() - Left());
                if (x + w > Right() && x > Left()) EndLine();
                if (pendingSpace && x > Left()) x += Fonts.GlyphWidth(0);
                lineItems.Add(new Item { Kind = ItemKind.Image, X = x, W = w, H = h, Image = img, Href = href, Text = img.Alt });
                x += w;
                pendingSpace = false;
                continue;
            }
            if (nm == "a") { href = t.Closing ? null : (t.Attr("href").Length > 0 ? t.Attr("href") : null); continue; }
            if (nm == "b" || nm == "strong" || nm == "th") { bold += t.Closing ? -1 : 1; if (bold < 0) bold = 0; if (nm != "th") continue; }
            if (nm == "td" || nm == "th") { if (!t.Closing && x > Left()) { x += Fonts.GlyphWidth(0) * 2; pendingSpace = false; } continue; }
            if (!IsBlock(nm)) continue;

            // block elements
            if (nm.Length == 2 && nm[0] == 'h' && nm[1] >= '1' && nm[1] <= '6')
            {
                Gap(t.Closing ? 6 : 12);
                heading = t.Closing ? 0 : nm[1] - '0';
                continue;
            }
            if (nm == "ul" || nm == "ol")
            {
                if (!t.Closing) { Gap(lists.Count == 0 ? 6 : 0); lists.Add(nm == "ol" ? 1 : -1); indent += 24; }
                else if (lists.Count > 0) { lists.RemoveAt(lists.Count - 1); indent = Math.Max(0, indent - 24); Gap(lists.Count == 0 ? 6 : 0); }
                continue;
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
                continue;
            }
            if (nm == "pre")
            {
                Gap(8);
                if (!t.Closing) { pre++; indent += 12; }
                else { pre = Math.Max(0, pre - 1); indent = Math.Max(0, indent - 12); }
                continue;
            }
            if (nm == "blockquote")
            {
                Gap(8);
                if (!t.Closing) { quote++; indent += 20; }
                else { quote = Math.Max(0, quote - 1); indent = Math.Max(0, indent - 20); }
                continue;
            }
            if (nm == "dd") { Gap(0); indent += t.Closing ? -20 : 20; if (indent < 0) indent = 0; continue; }
            Gap(nm == "p" || nm == "table" || nm == "figure" ? 10 : nm == "tr" ? 0 : 2);
        }
        EndLine();
        return (items, y + 16);
    }

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
