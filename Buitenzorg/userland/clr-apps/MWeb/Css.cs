// The CSS engine of Peramban's off-device page layout -- a copy of
// userland/clrhost/src/css.rs (the host's, used on Buitenzorg); keep them in
// step. Selectors: *, tag, .class, #id, compounds, descendant / child
// combinators (both as descendant), comma lists; pseudo-classes, attribute
// tests and sibling combinators are ignored. Properties: color,
// background(-color), border(-*), font-weight, font-size / font, display,
// text-align, margin(-left) / padding(-left).

namespace MWeb;

public sealed class CssCompound
{
    public string Tag = "", Id = "";
    public readonly List<string> Classes = new List<string>();
}

public sealed class CssRule
{
    public List<CssCompound> Sel = new List<CssCompound>();
    public int Spec, Order;
    public List<(string, string)> Decls = new List<(string, string)>();
}

public struct CssElem
{
    public string Tag, Id, Class;
    public CssElem(string tag, string id, string cls) { Tag = tag; Id = id; Class = cls; }
}

public struct CssStyle
{
    public int? Color;
    public int Font;      // 0 unset, 1 normal, 2 bold, 3 heading-sized
    public int Align;     // 0 left, 1 center, 2 right
    public int Display;   // 0 default, 1 none, 2 block, 3 inline
    public int? Background, Border;
    public int Indent;
    public bool OwnColor;
    public CssStyle Inherit() => new CssStyle { Color = Color, Font = Font, Align = Align };
}

public static class Css
{
    static string StripComments(string s)
    {
        var sb = new System.Text.StringBuilder();
        int i = 0;
        while (true)
        {
            int p = s.IndexOf("/*", i, StringComparison.Ordinal);
            if (p < 0) { sb.Append(s.Substring(i)); break; }
            sb.Append(s.Substring(i, p - i));
            int q = s.IndexOf("*/", p + 2, StringComparison.Ordinal);
            if (q < 0) break;
            i = q + 2;
        }
        return sb.ToString();
    }

    static bool WordChar(char c) => char.IsLetterOrDigit(c) || c == '-' || c == '_';

    static CssCompound? Compound(string s)
    {
        var c = new CssCompound();
        int i = 0;
        string Word()
        {
            int st = i;
            while (i < s.Length && WordChar(s[i])) i++;
            return s.Substring(st, i - st).ToLowerInvariant();
        }
        if (i < s.Length && s[i] == '*') i++;
        else if (i < s.Length && char.IsLetter(s[i])) c.Tag = Word();
        while (i < s.Length)
        {
            if (s[i] == '.') { i++; var w = Word(); if (w.Length == 0) return null; c.Classes.Add(w); }
            else if (s[i] == '#') { i++; var w = Word(); if (w.Length == 0) return null; c.Id = w; }
            else return null;
        }
        return c;
    }

    static (List<CssCompound>, int)? Selector(string s)
    {
        if (s.Contains('+') || s.Contains('~')) return null;
        var parts = new List<CssCompound>();
        foreach (var p in s.Replace('>', ' ').Split(new[] { ' ', '\t', '\n', '\r' }, StringSplitOptions.RemoveEmptyEntries))
        {
            var c = Compound(p);
            if (c == null) return null;
            parts.Add(c);
        }
        if (parts.Count == 0) return null;
        int spec = 0;
        foreach (var c in parts) spec += (c.Id.Length > 0 ? 10000 : 0) + c.Classes.Count * 100 + (c.Tag.Length > 0 ? 1 : 0);
        return (parts, spec);
    }

    public static List<(string, string)> Declarations(string s)
    {
        var o = new List<(string, string)>();
        foreach (var d in s.Split(';'))
        {
            int c = d.IndexOf(':');
            if (c < 0) continue;
            string name = d.Substring(0, c).Trim().ToLowerInvariant();
            string v = d.Substring(c + 1).Trim();
            int imp = v.ToLowerInvariant().IndexOf("!important", StringComparison.Ordinal);
            if (imp >= 0) v = v.Substring(0, imp).Trim();
            if (name.Length > 0 && v.Length > 0) o.Add((name, v));
        }
        return o;
    }

    public static void ParseSheet(string src, ref int order, List<CssRule> rules)
    {
        string s = StripComments(src);
        int i = 0;
        while (i < s.Length)
        {
            while (i < s.Length && char.IsWhiteSpace(s[i])) i++;
            if (i >= s.Length) break;
            if (s[i] == '@')
            {
                int semi = s.IndexOf(';', i), brace = s.IndexOf('{', i);
                if (semi >= 0 && (brace < 0 || semi < brace)) { i = semi + 1; continue; }
                if (brace < 0) break;
                int depth = 0, j = brace;
                for (; j < s.Length; j++)
                {
                    if (s[j] == '{') depth++;
                    else if (s[j] == '}' && --depth == 0) break;
                }
                i = j + 1;
                continue;
            }
            int open = s.IndexOf('{', i);
            if (open < 0) break;
            int close = s.IndexOf('}', open);
            if (close < 0) close = s.Length;
            var decls = Declarations(s.Substring(open + 1, close - open - 1));
            if (decls.Count > 0)
                foreach (var sel in s.Substring(i, open - i).Split(','))
                {
                    var r = Selector(sel.Trim());
                    if (r == null) continue;
                    order++;
                    rules.Add(new CssRule { Sel = r.Value.Item1, Spec = r.Value.Item2, Order = order, Decls = decls });
                }
            i = close + 1;
        }
    }

    static bool Matches(CssCompound c, CssElem e)
    {
        if (c.Tag.Length > 0 && c.Tag != e.Tag) return false;
        if (c.Id.Length > 0 && !string.Equals(c.Id, e.Id, StringComparison.OrdinalIgnoreCase)) return false;
        if (c.Classes.Count == 0) return true;
        var have = (e.Class ?? "").Split(new[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
        foreach (var k in c.Classes)
        {
            bool found = false;
            foreach (var h in have) if (string.Equals(h, k, StringComparison.OrdinalIgnoreCase)) { found = true; break; }
            if (!found) return false;
        }
        return true;
    }

    static bool RuleMatches(CssRule r, CssElem e, List<CssElem> anc)
    {
        if (!Matches(r.Sel[r.Sel.Count - 1], e)) return false;
        int k = anc.Count;
        for (int s = r.Sel.Count - 2; s >= 0; s--)
        {
            while (true)
            {
                if (k == 0) return false;
                k--;
                if (Matches(r.Sel[s], anc[k])) break;
            }
        }
        return true;
    }

    static readonly (string, int)[] Names =
    {
        ("black", 0x000000), ("white", 0xFFFFFF), ("red", 0xFF0000), ("green", 0x008000), ("blue", 0x0000FF), ("yellow", 0xFFFF00),
        ("gray", 0x808080), ("grey", 0x808080), ("silver", 0xC0C0C0), ("maroon", 0x800000), ("navy", 0x000080), ("teal", 0x008080),
        ("olive", 0x808000), ("purple", 0x800080), ("orange", 0xFFA500), ("lime", 0x00FF00), ("aqua", 0x00FFFF), ("fuchsia", 0xFF00FF),
        ("brown", 0xA52A2A), ("pink", 0xFFC0CB), ("darkgreen", 0x006400), ("darkblue", 0x00008B), ("darkred", 0x8B0000), ("lightgray", 0xD3D3D3),
        ("lightgrey", 0xD3D3D3), ("gold", 0xFFD700),
    };

    static int Hex(string s) { try { return Convert.ToInt32(s, 16); } catch (Exception) { return -1; } }

    public static int? Color(string v)
    {
        v = v.Trim().ToLowerInvariant();
        if (v.StartsWith("#"))
        {
            string h = v.Substring(1);
            if (h.Length == 3 || h.Length == 4)
            {
                int r = Hex(h.Substring(0, 1)), g = Hex(h.Substring(1, 1)), b = Hex(h.Substring(2, 1));
                if (r < 0 || g < 0 || b < 0) return null;
                return (r * 17) << 16 | (g * 17) << 8 | b * 17;
            }
            if (h.Length == 6 || h.Length == 8) { int x = Hex(h.Substring(0, 6)); return x < 0 ? null : x; }
            return null;
        }
        if (v.StartsWith("rgb"))
        {
            int a = v.IndexOf('(');
            if (a < 0) return null;
            var inner = v.Substring(a + 1).TrimEnd(')');
            var n = new List<int>();
            foreach (var x in inner.Split(new[] { ',', ' ', '/' }, StringSplitOptions.RemoveEmptyEntries))
            {
                if (n.Count == 3) break;
                if (x.EndsWith("%")) n.Add((int.TryParse(x.TrimEnd('%'), out int p) ? p : 0) * 255 / 100);
                else n.Add(double.TryParse(x, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out double d) ? (int)d : 0);
            }
            if (n.Count != 3) return null;
            return Math.Clamp(n[0], 0, 255) << 16 | Math.Clamp(n[1], 0, 255) << 8 | Math.Clamp(n[2], 0, 255);
        }
        foreach (var (name, c) in Names) if (name == v) return c;
        return null;
    }

    public static int? Length(string v)
    {
        v = v.Trim().ToLowerInvariant();
        int end = 0;
        while (end < v.Length && (char.IsDigit(v[end]) || v[end] == '.' || v[end] == '-')) end++;
        if (end == 0 || !double.TryParse(v.Substring(0, end), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out double n)) return null;
        string unit = v.Substring(end);
        double px = unit switch { "" or "px" => n, "em" or "rem" => n * 16, "pt" => n * 4 / 3, "%" => n * 16 / 100, _ => double.NaN };
        if (double.IsNaN(px)) return null;
        return (int)px;
    }

    static int FontClass(int px) => px >= 22 ? 3 : px >= 17 ? 2 : 1;

    static bool BoldWord(string t) => t == "bold" || t == "bolder" || (int.TryParse(t, out int n) && n >= 600);

    public static void Apply(ref CssStyle st, string name, string value)
    {
        string lv = value.Trim().ToLowerInvariant();
        var toks = lv.Split(new[] { ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries);
        switch (name)
        {
            case "color":
                { var c = Color(lv); if (c != null) { st.Color = c; st.OwnColor = true; } }
                break;
            case "background":
            case "background-color":
                if (lv == "none" || lv == "transparent") st.Background = null;
                else
                {
                    int? c = null;
                    foreach (var t in toks) { c = Color(t); if (c != null) break; }
                    c ??= Color(lv);
                    if (c != null) st.Background = c;
                }
                break;
            case "border":
            case "border-top":
            case "border-bottom":
            case "border-left":
            case "border-right":
            case "border-color":
                if (lv.Contains("none") || lv == "0" || lv.StartsWith("0 ")) st.Border = null;
                else
                {
                    int? c = null;
                    foreach (var t in toks) { c = Color(t); if (c != null) break; }
                    if (c != null) st.Border = c;
                    else if (name == "border" && lv.Contains("solid")) st.Border = 0x000000;
                }
                break;
            case "font-weight":
                if (st.Font != 3) st.Font = BoldWord(lv) ? 2 : 1;
                break;
            case "font-size":
                {
                    int? px = lv switch
                    {
                        "xx-large" or "xxx-large" => 32,
                        "x-large" or "larger" => 24,
                        "large" => 18,
                        "medium" or "small" or "x-small" or "xx-small" or "smaller" => 14,
                        _ => Length(lv),
                    };
                    if (px != null) st.Font = Math.Max(FontClass(px.Value), st.Font == 2 ? 2 : 0);
                }
                break;
            case "font":
                {
                    int f = 1;
                    foreach (var t in toks)
                    {
                        if (BoldWord(t)) f = Math.Max(f, 2);
                        var px = Length(t.Split('/')[0]);
                        if (px != null && (t.Contains("px") || t.Contains("em") || t.Contains("pt"))) f = Math.Max(f, FontClass(px.Value));
                    }
                    st.Font = f;
                }
                break;
            case "display":
                st.Display = lv switch
                {
                    "none" => 1,
                    "block" or "flex" or "grid" or "list-item" or "table" or "flow-root" => 2,
                    "inline" or "inline-block" or "inline-flex" or "contents" => 3,
                    _ => st.Display,
                };
                break;
            case "text-align":
                st.Align = lv switch { "center" => 1, "right" or "end" => 2, _ => 0 };
                break;
            case "margin-left":
            case "padding-left":
                { var n = Length(lv); if (n != null) st.Indent += Math.Clamp(n.Value, 0, 200); }
                break;
            case "margin":
            case "padding":
                {
                    string left = toks.Length switch { 1 => toks[0], 2 or 3 => toks[1], >= 4 => toks[3], _ => "0" };
                    var n = Length(left);
                    if (n != null) st.Indent += Math.Clamp(n.Value, 0, 200);
                }
                break;
        }
    }

    public static CssStyle Compute(CssStyle parent, CssElem e, List<CssElem> anc, List<CssRule> rules, string inline)
    {
        var st = parent.Inherit();
        var hits = new List<CssRule>();
        foreach (var r in rules) if (RuleMatches(r, e, anc)) hits.Add(r);
        hits.Sort((a, b) => a.Spec != b.Spec ? a.Spec.CompareTo(b.Spec) : a.Order.CompareTo(b.Order));
        foreach (var r in hits) foreach (var (n, v) in r.Decls) Apply(ref st, n, v);
        if (inline.Length > 0) foreach (var (n, v) in Declarations(inline)) Apply(ref st, n, v);
        return st;
    }
}
