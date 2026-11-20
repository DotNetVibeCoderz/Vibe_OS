// `bz font` -- the font catalog on the PC (like fontist): list / search / info,
// download a font with its SHA-256 checked into a folder, or straight onto a
// device (`--device`: pushed over bz-link, then the device's `font add`
// installs it -- the kernel checks the SHA-256 again).

using System.Security.Cryptography;

static class FontCli
{
    public sealed record Style(string Name, string Family, string StyleName, string File, long Size, string Sha256, string Licence, string Url);

    /// <summary>Licences installed without asking.</summary>
    public static bool IsFree(string licence) => licence is "OFL-1.1" or "Apache-2.0" or "MIT";

    /// <summary>The catalog: --catalog FILE, else assets/fonts/catalog.txt found upward from the
    /// working directory or this program.</summary>
    public static string? FindCatalog(string? given)
    {
        if (given != null) return File.Exists(given) ? given : null;
        foreach (var start in new[] { Directory.GetCurrentDirectory(), AppContext.BaseDirectory })
        {
            for (var d = new DirectoryInfo(start); d != null; d = d.Parent)
            {
                var p = Path.Combine(d.FullName, "assets", "fonts", "catalog.txt");
                if (File.Exists(p)) return p;
                p = Path.Combine(d.FullName, "Buitenzorg", "assets", "fonts", "catalog.txt");
                if (File.Exists(p)) return p;
            }
        }
        return null;
    }

    public static List<Style> Load(string path)
    {
        var list = new List<Style>();
        foreach (var raw in File.ReadAllLines(path))
        {
            if (raw.Length == 0 || raw[0] == '#') continue;
            var f = raw.Split('\t');
            if (f.Length < 8) continue;
            list.Add(new Style(f[0], f[1], f[2], f[3], long.TryParse(f[4], out var s) ? s : 0, f[5], f[6], f[7]));
        }
        return list;
    }

    static int Fail(string m) { Console.Error.WriteLine("bz font: " + m); return 1; }

    static string? Option(List<string> a, string name)
    {
        int i = a.IndexOf(name);
        if (i < 0 || i + 1 >= a.Count) return null;
        var v = a[i + 1];
        a.RemoveRange(i, 2);
        return v;
    }

    /// <summary>`bz font ...` without a device. Device installs go through
    /// <see cref="Fetch"/> from Device.cs.</summary>
    public static int Run(string[] args)
    {
        var a = args.ToList();
        string? catalogArg = Option(a, "--catalog");
        if (a is ["--selftest"]) return SelfTest(catalogArg);
        var catalog = FindCatalog(catalogArg);
        if (catalog == null) return Fail("no catalog: run inside the Buitenzorg tree or pass --catalog <catalog.txt>");
        var all = Load(catalog);
        switch (a)
        {
            case [] or ["list"] or ["catalog"] or ["search"]:
                return List(all, "");
            case ["search" or "list" or "catalog", .. var q]:
                return List(all, string.Join(' ', q).ToLowerInvariant());
            case ["info", var name]:
            {
                var st = all.Where(s => s.Name == name.ToLowerInvariant()).ToList();
                if (st.Count == 0) return Fail($"'{name}' is not in the catalog");
                Console.WriteLine($"{st[0].Family} ({st[0].Name}), licence {st[0].Licence}{(IsFree(st[0].Licence) ? "" : " -- needs --accept-license")}");
                foreach (var s in st) Console.WriteLine($"  {s.StyleName,-8} {s.File,-13} {s.Size / 1024,5} KiB  sha256 {s.Sha256}\n           {s.Url}");
                return 0;
            }
            case ["install", var name, .. var rest]:
            {
                var opts = rest.ToList();
                string dir = Option(opts, "--dir") ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".bz", "fonts");
                bool accept = opts.Remove("--accept-license");
                var got = Fetch(all, name, dir, accept, out var err);
                if (got == null) return Fail(err!);
                foreach (var (s, path) in got) Console.WriteLine($"{s.Family} {s.StyleName}: {path} (sha256 ok)");
                return 0;
            }
            default:
                return Fail("usage: bz font [list|search TEXT|info NAME|install NAME [--dir DIR] [--device] [--accept-license]] [--catalog FILE]");
        }
    }

    static int List(List<Style> all, string q)
    {
        foreach (var g in all.GroupBy(s => s.Name))
        {
            var f = g.First();
            if (q.Length > 0 && !f.Name.Contains(q) && !f.Family.ToLowerInvariant().Contains(q)) continue;
            Console.WriteLine($"{f.Name,-14} {f.Family,-24} {f.Licence,-11} {string.Join("+", g.Select(s => s.StyleName)),-14} {g.Select(s => s.File).Distinct().Count()} file(s), {g.GroupBy(s => s.File).Sum(x => x.First().Size) / 1024} KiB");
        }
        return 0;
    }

    /// <summary>Download every file of catalog font <paramref name="name"/> into
    /// <paramref name="dir"/> (kept when already there with the right SHA-256), checking
    /// size and SHA-256. Returns (style, local path) per file, or null with the reason.</summary>
    public static List<(Style, string)>? Fetch(List<Style> all, string name, string dir, bool accept, out string? error)
    {
        error = null;
        var styles = all.Where(s => s.Name == name.ToLowerInvariant()).ToList();
        if (styles.Count == 0) { error = $"'{name}' is not in the catalog (bz font search)"; return null; }
        if (!IsFree(styles[0].Licence) && !accept)
        {
            error = $"{styles[0].Family} is licensed under {styles[0].Licence}, not a free font licence: read its terms, then add --accept-license";
            return null;
        }
        Directory.CreateDirectory(dir);
        var result = new List<(Style, string)>();
        using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(5) };
        foreach (var s in styles.GroupBy(x => x.File).Select(x => x.First()))
        {
            var path = Path.Combine(dir, s.File);
            if (!(File.Exists(path) && Sha(File.ReadAllBytes(path)) == s.Sha256))
            {
                Console.Error.WriteLine($"bz font: downloading {s.File} ({s.Size / 1024} KiB) ...");
                byte[] data;
                try { data = http.GetByteArrayAsync(s.Url).GetAwaiter().GetResult(); }
                catch (Exception e) { error = $"{s.File}: {e.Message}"; return null; }
                if (s.Size > 0 && data.Length != s.Size) { error = $"{s.File}: {data.Length} bytes, the catalog says {s.Size}"; return null; }
                if (Sha(data) != s.Sha256) { error = $"{s.File}: SHA-256 does not match the catalog"; return null; }
                File.WriteAllBytes(path, data);
            }
            result.Add((s, path));
        }
        return result;
    }

    public static string Sha(byte[] data) => Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();

    /// <summary>`bz font --selftest`: the catalog is well formed (CI).</summary>
    static int SelfTest(string? catalogArg)
    {
        int fails = 0, checks = 0;
        void That(bool ok, string what) { checks++; if (!ok) { fails++; Console.WriteLine("FAIL " + what); } }
        var path = FindCatalog(catalogArg);
        That(path != null, "catalog found");
        if (path == null) { Console.WriteLine($"bz font: {checks} checks, {fails} failed"); return 1; }
        var all = Load(path);
        That(all.Count >= 20, $"catalog lists {all.Count} styles");
        foreach (var s in all)
        {
            That(s.Sha256.Length == 64 && s.Sha256.All(Uri.IsHexDigit), $"{s.Name} {s.StyleName}: SHA-256");
            That(s.Size > 0, $"{s.Name} {s.StyleName}: size");
            That(s.Url.StartsWith("https://"), $"{s.Name} {s.StyleName}: https URL");
            That(s.File.Length <= 12 && s.File.EndsWith(".TTF") && s.File.Split('.')[0].Length <= 8, $"{s.Name}: 8.3 file name {s.File}");
            That(s.StyleName is "Regular" or "Bold", $"{s.Name}: style {s.StyleName}");
        }
        // one file never belongs to two families
        foreach (var g in all.GroupBy(s => s.File)) That(g.Select(s => s.Family).Distinct().Count() == 1, $"{g.Key}: one family");
        // a fake download through Fetch: a file already present with the right hash is kept
        var dir = Path.Combine(Path.GetTempPath(), "bzfonttest-" + Environment.ProcessId);
        Directory.CreateDirectory(dir);
        var data = new byte[] { 1, 2, 3, 4 };
        File.WriteAllBytes(Path.Combine(dir, "TEST.TTF"), data);
        var fake = new List<Style> { new("test", "Test", "Regular", "TEST.TTF", 4, Sha(data), "OFL-1.1", "https://invalid.invalid/t.ttf"),
            new("other", "Other", "Regular", "OTHER.TTF", 4, Sha(data), "UFL-1.0", "https://invalid.invalid/o.ttf") };
        That(Fetch(fake, "test", dir, false, out _) is { Count: 1 }, "a downloaded file with the right SHA-256 is kept");
        That(Fetch(fake, "other", dir, false, out var e1) == null && e1!.Contains("UFL"), "another licence needs --accept-license");
        That(Fetch(fake, "nope", dir, false, out _) == null, "unknown names are refused");
        Directory.Delete(dir, true);
        Console.WriteLine($"bz font: {checks} checks, {fails} failed");
        return fails == 0 ? 0 : 1;
    }
}
