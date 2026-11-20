// Buitenzorg.Text -- the font catalog and installer (`bz font install`, the
// Settings app's Fon page). The catalog (/disk/FONTS/CATALOG.TXT, like
// fontist's formulas) lists every installable style with a pinned URL, its
// size, SHA-256 and licence. Installing downloads the file over HTTPS
// (Buitenzorg.Net) and hands it to the kernel (FONT_OP INSTALL), which checks
// the SHA-256 and that it is a font before registering it.

using System.Runtime.InteropServices;
using Buitenzorg.App;
using Buitenzorg.Net;

namespace Buitenzorg.Text;

/// <summary>One style of a font in the catalog.</summary>
public sealed class CatalogFont
{
    public string Name = "", Family = "", Style = "", File = "", Sha256 = "", Licence = "", Url = "";
    public long Size;
}

static class FontNative
{
    [DllImport("buitenzorg")] internal static extern long FontInstall(string request);
    [DllImport("buitenzorg")] internal static extern long FontRemove(string file);
    [DllImport("buitenzorg")] internal static extern long FontFind(string family, int bold);
}

public static class FontCatalog
{
    /// <summary>Where fonts live and the catalog that lists the installable ones.</summary>
    public const string Directory = "/disk/FONTS", DefaultPath = "/disk/FONTS/CATALOG.TXT";

    /// <summary>Licences installed without asking (the font licences that allow it freely).</summary>
    public static bool IsFree(string licence) => licence == "OFL-1.1" || licence == "Apache-2.0" || licence == "MIT";

    /// <summary>Every style in the catalog (empty when it is missing).</summary>
    public static List<CatalogFont> Load(string? path = null)
    {
        var list = new List<CatalogFont>();
        string text = Files.ReadAllText(path ?? DefaultPath) ?? "";
        foreach (var raw in text.Split('\n'))
        {
            string line = raw.TrimEnd('\r');
            if (line.Length == 0 || line[0] == '#') continue;
            var f = line.Split('\t');
            if (f.Length < 8) continue;
            long.TryParse(f[4], out long size);
            list.Add(new CatalogFont { Name = f[0], Family = f[1], Style = f[2], File = f[3], Size = size, Sha256 = f[5], Licence = f[6], Url = f[7] });
        }
        return list;
    }

    /// <summary>The catalog names (one per family), in catalog order.</summary>
    public static List<string> Names(List<CatalogFont> catalog)
    {
        var names = new List<string>();
        foreach (var e in catalog) if (names.IndexOf(e.Name) < 0) names.Add(e.Name);
        return names;
    }

    /// <summary>The styles of one catalog name.</summary>
    public static List<CatalogFont> Styles(List<CatalogFont> catalog, string name)
    {
        var list = new List<CatalogFont>();
        foreach (var e in catalog) if (e.Name == name) list.Add(e);
        return list;
    }
}

public static class FontInstaller
{
    static bool _sim;
    static readonly List<string> _simInstalled = new List<string>();
    const string Temp = "/disk/FONTS/DOWNLOAD.TMP";

    /// <summary>Whether a family is installed (off the device: what this run installed).</summary>
    public static bool IsInstalled(string family)
    {
        if (!_sim)
        {
            try { return FontNative.FontFind(family, 0) > 0; }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _sim = true; }
        }
        return _simInstalled.IndexOf(family) >= 0;
    }

    /// <summary>Download and install a catalog font (every style of it). Returns null when
    /// it worked, else why not. A font whose licence is not a free font licence needs
    /// <paramref name="acceptLicence"/>. <paramref name="log"/> receives progress lines.</summary>
    public static string? Install(string name, string? catalogPath = null, bool acceptLicence = false, Action<string>? log = null)
    {
        var styles = FontCatalog.Styles(FontCatalog.Load(catalogPath), name.ToLowerInvariant());
        if (styles.Count == 0) return "'" + name + "' tidak ada di katalog";
        var first = styles[0];
        if (!FontCatalog.IsFree(first.Licence) && !acceptLicence)
            return first.Family + " berlisensi " + first.Licence + ": baca syaratnya dulu, lalu pasang dengan persetujuan lisensi";
        Files.CreateDirectory(FontCatalog.Directory);
        var done = new List<string>();
        int faces = 0;
        foreach (var s in styles)
        {
            if (done.IndexOf(s.File) >= 0) continue; // a variable font serves both weights
            done.Add(s.File);
            log?.Invoke("mengunduh " + s.File + " (" + s.Size / 1024 + " KiB) dari " + s.Url);
            Files.Delete(Temp);
            HttpResponse r;
            try { r = new Buitenzorg.Net.HttpClient().Download(s.Url, Temp, null, null, false); }
            catch (Exception e) { Files.Delete(Temp); return s.File + ": " + e.Message; }
            if (r.Status != 200 && r.Status != 206) { Files.Delete(Temp); return s.File + ": HTTP " + r.Status; }
            if (s.Size > 0 && r.Saved != s.Size) { Files.Delete(Temp); return s.File + ": " + r.Saved + " byte, katalog " + s.Size; }
            long n;
            try { n = FontNative.FontInstall(Temp + "\n" + s.File + "\n" + s.Sha256); }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException)
            {
                _sim = true;
                n = 1;
                Files.Delete(Temp);
                if (_simInstalled.IndexOf(s.Family) < 0) _simInstalled.Add(s.Family);
            }
            if (n < 0) { Files.Delete(Temp); return s.File + ": ditolak kernel (SHA-256 tidak cocok atau bukan font)"; }
            faces += (int)n;
            log?.Invoke(s.File + " terpasang (" + n + " gaya)");
        }
        log?.Invoke(first.Family + ": selesai, " + faces + " gaya baru");
        return null;
    }

    /// <summary>Remove a catalog font (its files) or one font file by its 8.3 name.</summary>
    public static string? Remove(string nameOrFile, string? catalogPath = null)
    {
        var files = new List<string>();
        foreach (var s in FontCatalog.Styles(FontCatalog.Load(catalogPath), nameOrFile.ToLowerInvariant()))
            if (files.IndexOf(s.File) < 0) files.Add(s.File);
        if (files.Count == 0) files.Add(nameOrFile.ToUpperInvariant());
        foreach (var f in files)
        {
            try
            {
                if (FontNative.FontRemove(f) < 0) return f + ": tidak terpasang";
            }
            catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException)
            {
                _sim = true;
                _simInstalled.Clear();
            }
        }
        return null;
    }
}
