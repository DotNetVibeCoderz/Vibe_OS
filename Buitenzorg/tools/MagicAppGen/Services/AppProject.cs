using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Xml.Linq;

namespace MagicAppGen.Services;

/// <summary>The three ways an app runs on Buitenzorg OS.</summary>
public enum AppKind
{
    /// <summary>bflat --stdlib:zero + the bzstart shim → /disk/NAME.ELF (machine code, no GC).</summary>
    Native,
    /// <summary>dotnet build → /disk/NAME.DLL, run by RustCLR (CLRHOST.ELF) with a GC and the BCL.</summary>
    Managed,
    /// <summary>Python / JavaScript / TypeScript run by the in-kernel interpreter.</summary>
    Script,
}

/// <summary>One file to put on the device.</summary>
public sealed record DeployFile(string LocalPath, string RemotePath);

/// <summary>
/// A project folder on the host, classified by what is in it: a <c>.csproj</c>
/// makes it managed, a <c>main.py</c>/<c>.js</c>/<c>.ts</c> (with no C#) a
/// script, any other <c>.cs</c> native. Knows its 8.3 device name, its build
/// output folder and how the device shell starts it.
/// </summary>
public sealed class AppProject
{
    public string Dir { get; }
    public AppKind Kind { get; }
    /// <summary>8.3 base name in upper case, e.g. "CATATAN" (the file on /disk is NAME.ELF/.DLL/.PY).</summary>
    public string Name83 { get; }
    /// <summary>"py", "js" or "ts" for scripts; empty otherwise.</summary>
    public string ScriptLang { get; }
    public string MainFile { get; }
    public string OutDir => Path.Combine(Dir, "out");

    AppProject(string dir, AppKind kind, string name83, string lang, string main)
    {
        Dir = dir; Kind = kind; Name83 = name83; ScriptLang = lang; MainFile = main;
    }

    public string KindLabel => Kind switch
    {
        AppKind.Native => "C# native",
        AppKind.Managed => "C# managed",
        _ => ScriptLang switch { "py" => "Python", "ts" => "TypeScript", _ => "JavaScript" },
    };

    /// <summary>The name the device shell knows the app by (`run catatan`).</summary>
    public string LaunchName => Name83.ToLowerInvariant();

    public string RemotePath => Kind switch
    {
        AppKind.Native => $"/disk/{Name83}.ELF",
        AppKind.Managed => $"/disk/{Name83}.DLL",
        _ => $"/disk/{Name83}.{ScriptLang.ToUpperInvariant()}",
    };

    /// <summary>The shell command line that starts the deployed app.</summary>
    public string RunCommand(string args = "")
    {
        var a = string.IsNullOrWhiteSpace(args) ? "" : " " + args.Trim();
        return Kind == AppKind.Script ? $"script {ScriptLang} {RemotePath}{a}" : $"run {LaunchName}{a}";
    }

    public static AppProject? Open(string dir)
    {
        if (!Directory.Exists(dir)) return null;
        var files = Directory.GetFiles(dir);
        var csproj = files.FirstOrDefault(f => f.EndsWith(".csproj", StringComparison.OrdinalIgnoreCase));
        if (csproj != null)
        {
            var asm = ReadAssemblyName(csproj) ?? Path.GetFileNameWithoutExtension(csproj);
            var main = files.FirstOrDefault(f => Path.GetFileName(f).Equals("Program.cs", StringComparison.OrdinalIgnoreCase))
                       ?? files.FirstOrDefault(f => f.EndsWith(".cs", StringComparison.OrdinalIgnoreCase)) ?? csproj;
            return new AppProject(dir, AppKind.Managed, To83(asm), "", main);
        }
        var cs = files.Where(f => f.EndsWith(".cs", StringComparison.OrdinalIgnoreCase)).ToArray();
        string name = ManifestName(dir) ?? new DirectoryInfo(dir).Name;
        if (cs.Length == 0)
        {
            foreach (var ext in new[] { "py", "ts", "js" })
            {
                var main = files.FirstOrDefault(f => Path.GetFileName(f).Equals($"main.{ext}", StringComparison.OrdinalIgnoreCase))
                           ?? files.FirstOrDefault(f => f.EndsWith("." + ext, StringComparison.OrdinalIgnoreCase));
                if (main != null) return new AppProject(dir, AppKind.Script, To83(name), ext, main);
            }
            return null;
        }
        var mainCs = cs.FirstOrDefault(f => Path.GetFileName(f).Equals("main.cs", StringComparison.OrdinalIgnoreCase)) ?? cs[0];
        return new AppProject(dir, AppKind.Native, To83(name), "", mainCs);
    }

    static string? ReadAssemblyName(string csproj)
    {
        try
        {
            var x = XDocument.Load(csproj);
            return x.Descendants().FirstOrDefault(e => e.Name.LocalName == "AssemblyName")?.Value?.Trim();
        }
        catch { return null; }
    }

    static string? ManifestName(string dir)
    {
        var m = Path.Combine(dir, "app.manifest");
        if (!File.Exists(m)) return null;
        try
        {
            using var doc = JsonDocument.Parse(File.ReadAllText(m));
            return doc.RootElement.TryGetProperty("name", out var n) ? n.GetString() : null;
        }
        catch { return null; }
    }

    /// <summary>An 8.3 base name: A-Z/0-9 only, upper case, at most 8 characters, starting with a letter.</summary>
    public static string To83(string name)
    {
        var sb = new StringBuilder();
        foreach (var c in name.Normalize(NormalizationForm.FormD).ToUpperInvariant())
            if (c is >= 'A' and <= 'Z' or >= '0' and <= '9') sb.Append(c);
        var s = sb.ToString();
        if (s.Length == 0 || char.IsDigit(s[0])) s = "APP" + s;
        return s.Length > 8 ? s[..8] : s;
    }

    // ---- script subset check ------------------------------------------------

    /// <summary>
    /// Warnings for constructs the in-kernel interpreter (kernel/bzkernel/src/script.rs)
    /// does not support: it has ints, strings, bools, variables, if/else, while,
    /// for (C-style / range()), functions + recursion, print/console.log/str/len.
    /// </summary>
    public static List<string> CheckScriptSubset(string lang, string source)
    {
        var rules = new List<(string Pattern, string Why)>
        {
            (@"\b\d+\.\d+\b", "floating-point numbers (only integers are supported)"),
            (@"\[", "lists/arrays and indexing"),
        };
        if (lang == "py")
            rules.AddRange(new[]
            {
                (@"^\s*(import|from)\s", "imports (no modules)"),
                (@"^\s*class\s", "classes"),
                (@"\blambda\b", "lambda"),
                (@"\bf""|\bf'", "f-strings (use str() and +)"),
                (@"\{", "dicts/sets"),
                (@"\.\w+\(", "method calls (only print/str/len/range)"),
            });
        else
            rules.AddRange(new[]
            {
                (@"^\s*(import|export)\s|\brequire\(", "modules"),
                (@"^\s*class\s|\bnew\s+\w", "classes / new"),
                (@"=>", "arrow functions"),
                ("`", "template strings (use + to concatenate)"),
                (@"(?<!console)\.\w+\(", "method calls (only console.log/str/len)"),
                (@"[=:(,]\s*\{", "object literals"),
            });
        if (lang != "py")
            rules.Add((@"^\s*(if|while|for)\b[^{]*\)\s*[A-Za-z_]|^\s*else\s+(?!if\b)[A-Za-z_]",
                       "a statement without { braces } after if/while/for/else"));
        var warnings = new List<string>();
        var lines = source.Replace("\r", "").Split('\n');
        for (int i = 0; i < lines.Length; i++)
        {
            if (lang == "ts")
            {
                // The TypeScript annotation stripper runs before comments are removed:
                // in a comment an apostrophe opens a "string", a colon eats text and
                // "interface"/"type" start a declaration that gets deleted.
                int cpos = lines[i].IndexOf("//", StringComparison.Ordinal);
                if (cpos >= 0 && Regex.IsMatch(lines[i][cpos..], @"['`:]|\b(interface|type)\b"))
                    warnings.Add($"line {i + 1}: avoid apostrophes, colons and the words interface/type in TypeScript comments - the kernel strips annotations before it removes comments");
            }
            var line = StripComment(lang, lines[i]);
            foreach (var (pat, why) in rules)
                if (Regex.IsMatch(line, pat))
                    warnings.Add($"line {i + 1}: {why} - not supported by the Buitenzorg script runtime");
        }
        return warnings;
    }

    static string StripComment(string lang, string line)
    {
        // drop string literals first so their content is not checked
        line = Regex.Replace(line, "\"(\\\\.|[^\"\\\\])*\"|'(\\\\.|[^'\\\\])*'", "\"\"");
        int c = lang == "py" ? line.IndexOf('#') : line.IndexOf("//", StringComparison.Ordinal);
        return c >= 0 ? line[..c] : line;
    }
}
