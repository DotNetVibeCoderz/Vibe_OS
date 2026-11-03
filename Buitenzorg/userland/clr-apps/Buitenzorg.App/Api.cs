// Buitenzorg.App -- the operating-system API for managed C# apps (RustCLR).
//
// A managed app is an ordinary `dotnet build` assembly that references this
// library; CLRHOST.ELF loads both from /disk and implements every
// [DllImport("buitenzorg")] below on top of the Buitenzorg syscalls. The host
// does the marshalling (a managed string becomes UTF-8 bytes, an int[] a
// pixel buffer), because a pointer in RustCLR is a reference into the managed
// heap, not an address the kernel could read.

using System.Runtime.InteropServices;

namespace Buitenzorg.App;

/// <summary>The host's native entry points (CLRHOST.ELF, "pinvoke:buitenzorg!...").</summary>
internal static class Native
{
    [DllImport("buitenzorg")] internal static extern int WinCreate(string title, int width, int height);
    [DllImport("buitenzorg")] internal static extern int WinDraw(int window, int op, int x, int y, int w, int h, int color, string text);
    [DllImport("buitenzorg")] internal static extern int WinBlit(int window, int x, int y, int w, int h, int[] pixels);
    [DllImport("buitenzorg")] internal static extern int WinPresent(int window);
    [DllImport("buitenzorg")] internal static extern int KeyRead();
    [DllImport("buitenzorg")] internal static extern int ScreenWidth();
    [DllImport("buitenzorg")] internal static extern int ScreenHeight();
    [DllImport("buitenzorg")] internal static extern int IsInteractive();
    [DllImport("buitenzorg")] internal static extern long Spawn(string path, string args);
    [DllImport("buitenzorg")] internal static extern long Wait(long pid);
    [DllImport("buitenzorg")] internal static extern string FsList(string path);
    [DllImport("buitenzorg")] internal static extern string FsReadText(string path);
    [DllImport("buitenzorg")] internal static extern long FsSize(string path);
    [DllImport("buitenzorg")] internal static extern int FsReadInto(string path, byte[] buffer);
    [DllImport("buitenzorg")] internal static extern int FsWriteText(string path, string text);
    [DllImport("buitenzorg")] internal static extern int FsOp(int op, string path, string path2);
    [DllImport("buitenzorg")] internal static extern string FsStat(string path);
    [DllImport("buitenzorg")] internal static extern string ProcList();
    [DllImport("buitenzorg")] internal static extern int ProcKill(long pid);
    [DllImport("buitenzorg")] internal static extern long SysStat(int field);
}

/// <summary>Colors as 0xRRGGBB.</summary>
public static class Colors
{
    public const int Black = 0x000000;
    public const int White = 0xFFFFFF;
    public const int Red = 0xE5484D;
    public const int Green = 0x46A758;
    public const int Blue = 0x3E63DD;
    public const int Yellow = 0xF5D90A;
    public const int Gray = 0x8B8D98;
    public const int Leaf = 0x7CC4A4;
    public const int Forest = 0x12301F;

    public static int Rgb(int r, int g, int b) => ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF);
}

/// <summary>A window on the Buitenzorg desktop. Draw into it, then <see cref="Present"/>.</summary>
public sealed class Window
{
    // DrawCmd ops (bz-abi draw_op).
    const int FillRectOp = 0, TextOp = 1, ClearOp = 2, LineOp = 3, EllipseOp = 4, FillEllipseOp = 5, RectOp = 6;

    public int Id { get; }
    public int Width { get; }
    public int Height { get; }

    public Window(string title, int width, int height)
    {
        Id = Native.WinCreate(title, width, height);
        if (Id == 0)
            throw new InvalidOperationException("Buitenzorg: the window could not be created (no desktop?)");
        Width = width;
        Height = height;
    }

    void Draw(int op, int x, int y, int w, int h, int color, string text = "")
    {
        if (Native.WinDraw(Id, op, x, y, w, h, color, text) != 0)
            throw new InvalidOperationException("Buitenzorg: draw command refused");
    }

    public void Clear(int color) => Draw(ClearOp, 0, 0, 0, 0, color);
    public void FillRect(int x, int y, int w, int h, int color) => Draw(FillRectOp, x, y, w, h, color);
    public void DrawRect(int x, int y, int w, int h, int color) => Draw(RectOp, x, y, w, h, color);
    public void DrawLine(int x0, int y0, int x1, int y1, int color) => Draw(LineOp, x0, y0, x1 - x0, y1 - y0, color);
    public void DrawEllipse(int x, int y, int w, int h, int color) => Draw(EllipseOp, x, y, w, h, color);
    public void FillEllipse(int x, int y, int w, int h, int color) => Draw(FillEllipseOp, x, y, w, h, color);
    public void DrawText(int x, int y, string text, int color) => Draw(TextOp, x, y, 0, 0, color, text);

    /// <summary>Copy a w*h block of 0xAARRGGBB pixels into the window at (x, y).</summary>
    public void Blit(int x, int y, int w, int h, int[] pixels)
    {
        if (pixels.Length < w * h)
            throw new ArgumentException("pixel buffer smaller than w*h", nameof(pixels));
        if (Native.WinBlit(Id, x, y, w, h, pixels) != 0)
            throw new InvalidOperationException("Buitenzorg: blit refused");
    }

    /// <summary>Recompose the desktop so what was drawn becomes visible.</summary>
    public void Present() => Native.WinPresent(Id);
}

/// <summary>The keyboard (keys reach the app whose window has the focus).</summary>
public static class Keyboard
{
    public const char Escape = (char)27;

    /// <summary>One key, or '\0' when none is waiting.</summary>
    public static char Read() => (char)Native.KeyRead();
}

/// <summary>The display.</summary>
public static class Screen
{
    public static int Width => Native.ScreenWidth();
    public static int Height => Native.ScreenHeight();
}

/// <summary>The session the app runs in.</summary>
public static class Session
{
    /// <summary>
    /// True once the desktop is live (an app started from the shell or the
    /// Start menu); false while the boot self-tests run, when an app should
    /// draw, report and exit rather than wait for a keyboard.
    /// </summary>
    public static bool IsInteractive => Native.IsInteractive() != 0;
}

/// <summary>Other programs.</summary>
public static class Programs
{
    /// <summary>Start a program (an app name, or a /disk path to an ELF or a .DLL); its pid.</summary>
    public static long Start(string path, string args = "")
    {
        long pid = Native.Spawn(path, args);
        if (pid <= 0)
            throw new InvalidOperationException("Buitenzorg: could not start " + path);
        return pid;
    }

    /// <summary>Wait for a program to exit; its exit code.</summary>
    public static long WaitForExit(long pid) => Native.Wait(pid);
}

/// <summary>Size, kind and last change of a file or folder (<see cref="Files.GetInfo"/>).</summary>
public sealed class FileInfoData
{
    public string Path { get; }
    public string Name { get; }
    public long Size { get; }
    public bool IsDirectory { get; }
    /// <summary>Last change (local time, 2-second steps); null when unknown.</summary>
    public DateTime? Modified { get; }
    public FileInfoData(string path, long size, bool dir, DateTime? modified)
    {
        Path = path;
        Name = path.TrimEnd('/').Substring(path.TrimEnd('/').LastIndexOf('/') + 1);
        Size = size;
        IsDirectory = dir;
        Modified = modified;
    }
}

/// <summary>An entry of a directory listing.</summary>
public sealed class FileEntry
{
    public string Name { get; }
    public string Path { get; }
    public bool IsDirectory { get; }
    public FileEntry(string name, string path, bool isDirectory) { Name = name; Path = path; IsDirectory = isDirectory; }
    public override string ToString() => Name;
}

/// <summary>The virtual file system: /disk (FAT, persistent), /ram, ... with folders
/// (8.3 names per component: /disk/APPS/DEMO/README.TXT).
/// Off the device (tests on a development machine) it falls back to an in-memory /ram.</summary>
public static class Files
{
    static Dictionary<string, string>? _sim;
    static readonly HashSet<string> _simDirs = new HashSet<string>();
    static readonly Dictionary<string, DateTime> _simTimes = new Dictionary<string, DateTime>();

    static bool Sim(Exception e)
    {
        if (e is OutOfMemoryException) return false;
        _sim ??= new Dictionary<string, string>();
        return true;
    }

    /// <summary>The mounts ("") or the files of a mount ("/disk").</summary>
    public static List<FileEntry> List(string path)
    {
        string text;
        if (_sim != null) text = SimList(path);
        else
        {
            try { text = Native.FsList(path); }
            catch (Exception e) when (Sim(e)) { text = SimList(path); }
        }
        var result = new List<FileEntry>();
        foreach (var line in text.Split('\n'))
        {
            if (line.Length == 0) continue;
            bool dir = line.EndsWith("/");
            string name = dir ? line.Substring(0, line.Length - 1) : line;
            string full = path.Length == 0 ? "/" + name : path.TrimEnd('/') + "/" + name;
            result.Add(new FileEntry(name, full, dir || path.Length == 0));
        }
        return result;
    }

    static string SimList(string path)
    {
        if (path.Length == 0) return "ram/\n";
        string prefix = path.TrimEnd('/') + "/";
        var seen = new List<string>();
        var sb = new System.Text.StringBuilder();
        void Add(string line) { if (!seen.Contains(line)) { seen.Add(line); sb.Append(line).Append('\n'); } }
        foreach (var d in _simDirs)
            if (d.StartsWith(prefix) && d.IndexOf('/', prefix.Length) < 0) Add(d.Substring(prefix.Length) + "/");
        foreach (var k in _sim!.Keys)
            if (k.StartsWith(prefix) && k.IndexOf('/', prefix.Length) < 0) Add(k.Substring(prefix.Length));
        return sb.ToString();
    }

    static bool SimExists(string p) => _sim!.ContainsKey(p) || _simDirs.Contains(p) || p == "/ram";
    static string ParentOf(string p) { int i = p.TrimEnd('/').LastIndexOf('/'); return i <= 0 ? "/" : p.Substring(0, i); }

    /// <summary>Create a folder (its parent must exist); false if refused (exists, bad 8.3 name, read-only).</summary>
    public static bool CreateDirectory(string path)
    {
        path = path.TrimEnd('/');
        if (_sim != null)
        {
            if (SimExists(path) || !SimExists(ParentOf(path))) return false;
            _simDirs.Add(path);
            _simTimes[path] = DateTime.Now;
            return true;
        }
        try { return Native.FsOp(0, path, "") == 0; }
        catch (Exception e) when (Sim(e)) { return CreateDirectory(path); }
    }

    /// <summary>Delete a file or an empty folder.</summary>
    public static bool Delete(string path)
    {
        path = path.TrimEnd('/');
        if (_sim != null)
        {
            if (_sim.Remove(path)) return true;
            if (!_simDirs.Contains(path)) return false;
            foreach (var k in _sim.Keys) if (k.StartsWith(path + "/")) return false;
            foreach (var d in _simDirs) if (d.StartsWith(path + "/")) return false;
            return _simDirs.Remove(path);
        }
        try { return Native.FsOp(1, path, "") == 0; }
        catch (Exception e) when (Sim(e)) { return Delete(path); }
    }

    /// <summary>Rename and / or move a file or folder (to another drive: files only, copied).
    /// The target must not exist.</summary>
    public static bool Move(string from, string to)
    {
        from = from.TrimEnd('/'); to = to.TrimEnd('/');
        if (_sim != null)
        {
            if (!SimExists(from) || SimExists(to) || !SimExists(ParentOf(to)) || to.StartsWith(from + "/")) return false;
            if (_sim.TryGetValue(from, out var text)) { _sim.Remove(from); _sim[to] = text; return true; }
            var moves = new List<string>();
            foreach (var d in _simDirs) if (d == from || d.StartsWith(from + "/")) moves.Add(d);
            foreach (var d in moves) { _simDirs.Remove(d); _simDirs.Add(to + d.Substring(from.Length)); }
            var files = new List<string>(_sim.Keys);
            foreach (var k in files) if (k.StartsWith(from + "/")) { var t = _sim[k]; _sim.Remove(k); _sim[to + k.Substring(from.Length)] = t; }
            return true;
        }
        try { return Native.FsOp(2, from, to) == 0; }
        catch (Exception e) when (Sim(e)) { return Move(from, to); }
    }

    /// <summary>Size, kind and last change; null if it does not exist.</summary>
    public static FileInfoData? GetInfo(string path)
    {
        path = path.TrimEnd('/');
        if (_sim != null)
        {
            DateTime? t = _simTimes.TryGetValue(path, out var w) ? w : null;
            if (_sim.TryGetValue(path, out var text)) return new FileInfoData(path, System.Text.Encoding.UTF8.GetByteCount(text), false, t);
            if (_simDirs.Contains(path) || path == "/ram") return new FileInfoData(path, 0, true, t);
            return null;
        }
        string s;
        try { s = Native.FsStat(path); }
        catch (Exception e) when (Sim(e)) { return GetInfo(path); }
        var f = s.Split(',');
        if (f.Length < 4) return null;
        int date = int.Parse(f[2]), time = int.Parse(f[3]);
        DateTime? when = null;
        if (date != 0)
        {
            int y = 1980 + (date >> 9), mo = (date >> 5) & 15, d = date & 31;
            int h = time >> 11, mi = (time >> 5) & 63, sec = (time & 31) * 2;
            if (mo >= 1 && mo <= 12 && d >= 1 && d <= 31 && h < 24 && mi < 60 && sec < 60) when = new DateTime(y, mo, d, h, mi, sec);
        }
        return new FileInfoData(path, long.Parse(f[0]), f[1] != "0", when);
    }

    /// <summary>True for a valid 8.3 name ("CATATAN.TXT", "FOTO"): what the FAT volumes accept.</summary>
    public static bool IsValidName(string name)
    {
        if (name.Length == 0 || name.Length > 12) return false;
        int dot = name.IndexOf('.');
        string b = dot < 0 ? name : name.Substring(0, dot), x = dot < 0 ? "" : name.Substring(dot + 1);
        if (b.Length == 0 || b.Length > 8 || x.Length > 3 || x.IndexOf('.') >= 0) return false;
        foreach (char c in name)
            if (c != '.' && !(char.IsLetterOrDigit(c) || c == '_' || c == '-' || c == '~' || c == '!' || c == '#' || c == '$' || c == '&')) return false;
        return true;
    }

    /// <summary>A writable place for scratch files: /ram when it is mounted (PC), else /disk.</summary>
    public static string ScratchDirectory
    {
        get
        {
            foreach (var m in List("")) if (m.Name == "ram") return "/ram";
            return "/disk";
        }
    }

    public static string ReadAllText(string path)
    {
        if (_sim != null) return _sim.TryGetValue(path, out var t) ? t : "";
        try { return Native.FsReadText(path); }
        catch (Exception e) when (Sim(e)) { return ReadAllText(path); }
    }

    /// <summary>The size in bytes, -1 when it cannot be read.</summary>
    /// <summary>A file's bytes (null if it cannot be read; up to 16 MiB).</summary>
    public static byte[]? ReadAllBytes(string path)
    {
        if (_sim != null) return _sim.TryGetValue(path, out var t) ? System.Text.Encoding.UTF8.GetBytes(t) : null;
        long size = Size(path);
        if (size <= 0) return null;
        var buf = new byte[size];
        try
        {
            int n = Native.FsReadInto(path, buf);
            if (n < 0) return null;
            if (n < buf.Length) Array.Resize(ref buf, n);
            return buf;
        }
        catch (Exception e) when (Sim(e)) { return ReadAllBytes(path); }
    }

    public static long Size(string path)
    {
        if (_sim != null) return _sim.TryGetValue(path, out var t) ? t.Length : -1;
        try { return Native.FsSize(path); }
        catch (Exception e) when (Sim(e)) { return Size(path); }
    }

    /// <summary>Create or replace a file; false when the mount refused it.</summary>
    public static bool WriteAllText(string path, string text)
    {
        if (_sim != null) { if (_simDirs.Contains(path)) return false; _sim[path] = text; _simTimes[path] = DateTime.Now; return true; }
        try { return Native.FsWriteText(path, text) == System.Text.Encoding.UTF8.GetByteCount(text) || text.Length == 0; }
        catch (Exception e) when (Sim(e)) { return WriteAllText(path, text); }
    }
}

/// <summary>A process or kernel task (PROC_LIST).</summary>
public sealed class ProcessInfo
{
    public long Pid { get; }
    public string Name { get; }
    /// <summary>0 runnable, 1 running, 2 finished.</summary>
    public int State { get; }
    public long CpuTicks { get; }
    public bool IsApp { get; }

    public ProcessInfo(long pid, string name, int state, long cpu, bool app) { Pid = pid; Name = name; State = state; CpuTicks = cpu; IsApp = app; }

    public string StateText => State == 1 ? "berjalan" : State == 0 ? "siap" : "selesai";
}

public static class Processes
{
    /// <summary>Off the device: a fixed sample, so apps can be tested on a development machine.</summary>
    const string Sample = "0,1,812,0,boot\n1,0,40,0,dahan\n7,1,95,1,clrhost\n9,0,12,1,files\n";

    public static List<ProcessInfo> List()
    {
        var l = new List<ProcessInfo>();
        string text;
        try { text = Native.ProcList(); } catch (Exception e) when (e is not OutOfMemoryException) { text = Sample; }
        foreach (var line in text.Split('\n'))
        {
            var f = line.Split(',');
            if (f.Length < 5) continue;
            l.Add(new ProcessInfo(long.Parse(f[0]), f[4], int.Parse(f[1]), long.Parse(f[2]), f[3] == "1"));
        }
        return l;
    }

    public static bool Kill(long pid)
    {
        try { return Native.ProcKill(pid) == 0; } catch (Exception e) when (e is not OutOfMemoryException) { return false; }
    }
}

/// <summary>Live system figures (SYS_STAT).</summary>
public static class SystemInfo
{
    static long Stat(int field)
    {
        try { return Native.SysStat(field); }
        catch (Exception e) when (e is not OutOfMemoryException) { return field switch { 0 => 1820, 1 => 18, 2 => 9 << 20, 3 => 32 << 20, 4 => 12, _ => 512 }; }
    }

    public static long UptimeSeconds { get { long hz = Stat(1); return hz > 0 ? Stat(0) / hz : 0; } }
    public static long KernelHeapUsed => Stat(2);
    public static long KernelHeapTotal => Stat(3);
    public static long TaskCount => Stat(4);
    public static long MemoryMiB => Stat(5);
}
