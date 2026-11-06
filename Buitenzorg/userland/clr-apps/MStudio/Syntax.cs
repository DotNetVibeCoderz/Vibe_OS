// Syntax highlighting for Buitenzorg Studio: a small per-line tokenizer for
// C#, JavaScript, TypeScript, Python, shell scripts and JSON/manifests. It
// carries one bit of state between lines (inside a /* */ block comment).

namespace MStudio;

public enum Tk { Plain, Keyword, Type, Str, Num, Comment, Punct, Func }

public readonly struct Span
{
    public readonly int Start, Length;
    public readonly Tk Kind;
    public Span(int start, int length, Tk kind) { Start = start; Length = length; Kind = kind; }
}

public static class Syntax
{
    /// <summary>The language of a file, from its extension: cs, js, ts, py, sh, json or text.</summary>
    public static string LangOf(string path)
    {
        string p = path.ToUpperInvariant();
        int dot = p.LastIndexOf('.');
        string ext = dot < 0 ? "" : p.Substring(dot + 1);
        switch (ext)
        {
            case "CS": return "cs";
            case "JS": return "js";
            case "TS": return "ts";
            case "PY": return "py";
            case "SH": return "sh";
            case "JSN": case "JSON": case "MAN": return "json";
            default: return "text";
        }
    }

    public static string DisplayName(string lang)
    {
        switch (lang)
        {
            case "cs": return "C#";
            case "js": return "JavaScript";
            case "ts": return "TypeScript";
            case "py": return "Python";
            case "sh": return "Skrip shell";
            case "json": return "JSON";
            default: return "Teks";
        }
    }

    static readonly HashSet<string> CsKeywords = new HashSet<string>
    {
        "abstract", "as", "base", "break", "case", "catch", "class", "const", "continue", "default", "delegate",
        "do", "else", "enum", "event", "explicit", "extern", "false", "finally", "fixed", "for", "foreach", "goto",
        "if", "implicit", "in", "interface", "internal", "is", "lock", "namespace", "new", "null", "operator",
        "out", "override", "params", "private", "protected", "public", "readonly", "ref", "return", "sealed",
        "sizeof", "stackalloc", "static", "struct", "switch", "this", "throw", "true", "try", "typeof", "unsafe",
        "using", "virtual", "void", "volatile", "while", "var", "async", "await", "record", "init", "get", "set", "yield",
    };
    static readonly HashSet<string> CsTypes = new HashSet<string>
    {
        "bool", "byte", "char", "decimal", "double", "float", "int", "long", "object", "sbyte", "short", "string",
        "uint", "ulong", "ushort", "nint", "nuint", "String", "Console", "Math", "List", "Dictionary",
    };
    static readonly HashSet<string> JsKeywords = new HashSet<string>
    {
        "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "export", "extends",
        "false", "finally", "for", "function", "if", "import", "in", "instanceof", "let", "new", "null", "of", "return",
        "switch", "this", "throw", "true", "try", "typeof", "undefined", "var", "void", "while", "async", "await",
    };
    static readonly HashSet<string> TsExtra = new HashSet<string>
    {
        "interface", "type", "enum", "implements", "public", "private", "protected", "readonly", "as", "declare",
    };
    static readonly HashSet<string> TsTypes = new HashSet<string> { "number", "string", "boolean", "any", "void", "never", "unknown", "console" };
    static readonly HashSet<string> PyKeywords = new HashSet<string>
    {
        "and", "as", "assert", "break", "class", "continue", "def", "del", "elif", "else", "except", "False", "finally",
        "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass",
        "raise", "return", "True", "try", "while", "with", "yield",
    };
    static readonly HashSet<string> PyTypes = new HashSet<string> { "int", "str", "float", "bool", "list", "dict", "range", "len", "print" };
    static readonly HashSet<string> ShKeywords = new HashSet<string>
    {
        "if", "then", "elif", "else", "fi", "for", "in", "do", "done", "while", "exit", "alias", "unalias", "source",
        "export", "unset", "test", "echo", "cd",
    };
    static readonly HashSet<string> JsonKeywords = new HashSet<string> { "true", "false", "null" };

    static bool IsKeyword(string lang, string w)
    {
        switch (lang)
        {
            case "cs": return CsKeywords.Contains(w);
            case "js": return JsKeywords.Contains(w);
            case "ts": return JsKeywords.Contains(w) || TsExtra.Contains(w);
            case "py": return PyKeywords.Contains(w);
            case "sh": return ShKeywords.Contains(w);
            case "json": return JsonKeywords.Contains(w);
            default: return false;
        }
    }

    static bool IsType(string lang, string w)
    {
        switch (lang)
        {
            case "cs": return CsTypes.Contains(w) || (w.Length > 1 && char.IsUpper(w[0]) && w != w.ToUpperInvariant());
            case "ts": return TsTypes.Contains(w);
            case "js": return w == "console" || w == "Math";
            case "py": return PyTypes.Contains(w);
            default: return false;
        }
    }

    static bool CStyle(string lang) => lang == "cs" || lang == "js" || lang == "ts";

    /// <summary>The spans of one line. <paramref name="inBlock"/> says whether the
    /// line starts inside a block comment, and is updated for the next line.</summary>
    public static List<Span> Line(string line, string lang, ref bool inBlock)
    {
        var spans = new List<Span>();
        if (lang == "text") { if (line.Length > 0) spans.Add(new Span(0, line.Length, Tk.Plain)); return spans; }
        int i = 0, n = line.Length;
        bool cstyle = CStyle(lang);
        while (i < n)
        {
            if (inBlock)
            {
                int end = line.IndexOf("*/", i);
                int stop = end < 0 ? n : end + 2;
                spans.Add(new Span(i, stop - i, Tk.Comment));
                i = stop;
                if (end >= 0) inBlock = false;
                continue;
            }
            char c = line[i];
            char next = i + 1 < n ? line[i + 1] : '\0';
            if (cstyle && c == '/' && next == '/') { spans.Add(new Span(i, n - i, Tk.Comment)); break; }
            if ((lang == "py" || lang == "sh") && c == '#') { spans.Add(new Span(i, n - i, Tk.Comment)); break; }
            if (cstyle && c == '/' && next == '*') { inBlock = true; continue; }
            if (c == '"' || c == '\'' || (c == '`' && (lang == "js" || lang == "ts")))
            {
                int j = i + 1;
                while (j < n && line[j] != c) { if (line[j] == '\\') j++; j++; }
                int stop = Math.Min(n, j + 1);
                spans.Add(new Span(i, stop - i, Tk.Str));
                i = stop;
                continue;
            }
            if (char.IsDigit(c))
            {
                int j = i + 1;
                while (j < n && (char.IsLetterOrDigit(line[j]) || line[j] == '.' || line[j] == '_')) j++;
                spans.Add(new Span(i, j - i, Tk.Num));
                i = j;
                continue;
            }
            if (char.IsLetter(c) || c == '_' || (c == '$' && lang != "sh"))
            {
                int j = i + 1;
                while (j < n && (char.IsLetterOrDigit(line[j]) || line[j] == '_')) j++;
                string w = line.Substring(i, j - i);
                Tk kind = IsKeyword(lang, w) ? Tk.Keyword
                        : IsType(lang, w) ? Tk.Type
                        : j < n && line[j] == '(' ? Tk.Func
                        : Tk.Plain;
                spans.Add(new Span(i, j - i, kind));
                i = j;
                continue;
            }
            if (c == '$' && lang == "sh")
            {
                int j = i + 1;
                if (j < n && line[j] == '{') { while (j < n && line[j] != '}') j++; j = Math.Min(n, j + 1); }
                else while (j < n && (char.IsLetterOrDigit(line[j]) || line[j] == '_' || line[j] == '#' || line[j] == '@' || line[j] == '?')) j++;
                spans.Add(new Span(i, j - i, Tk.Type));
                i = j;
                continue;
            }
            if (c == ' ' || c == '\t')
            {
                int j = i + 1;
                while (j < n && (line[j] == ' ' || line[j] == '\t')) j++;
                spans.Add(new Span(i, j - i, Tk.Plain));
                i = j;
                continue;
            }
            spans.Add(new Span(i, 1, Tk.Punct));
            i++;
        }
        return spans;
    }

    /// <summary>The block-comment state at the start of line <paramref name="upTo"/>.</summary>
    public static bool StateBefore(IReadOnlyList<string> lines, int upTo, string lang)
    {
        if (!CStyle(lang)) return false;
        bool inBlock = false;
        for (int i = 0; i < upTo && i < lines.Count; i++) Line(lines[i], lang, ref inBlock);
        return inBlock;
    }

    /// <summary>The color of a token kind for a dark or a light background.</summary>
    public static int Color(Tk kind, bool dark, int plain)
    {
        switch (kind)
        {
            case Tk.Keyword: return dark ? 0xC792EA : 0x8E3FB8;
            case Tk.Type: return dark ? 0x7FC8E8 : 0x1F6F9E;
            case Tk.Str: return dark ? 0xB5D98A : 0x3D7A24;
            case Tk.Num: return dark ? 0xF5A26B : 0xB4531A;
            case Tk.Comment: return dark ? 0x6F8F7A : 0x7A8A80;
            case Tk.Punct: return dark ? 0x9CC9D9 : 0x4A6670;
            case Tk.Func: return dark ? 0xE9C46A : 0x8A6A0A;
            default: return plain;
        }
    }
}
