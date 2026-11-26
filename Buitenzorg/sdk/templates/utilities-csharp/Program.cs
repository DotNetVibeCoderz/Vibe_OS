// __APPNAME__ -- a system utility made with bz new utilities-csharp.
//
// Three pages over Buitenzorg.App's system access:
//   Proses  -- the running processes (Processes.List), end one (Processes.Kill)
//   Sistem  -- uptime, kernel heap, memory, task count (SystemInfo)
//   Disk    -- files and bytes per folder of a mount (Files.List / Files.Size)
// The figures refresh every two seconds while the window is open.
//
// Ending another program needs the "system" permission (app.manifest).
// `dotnet bin/.../__NAME83__.dll --selftest` runs on the PC against sample
// data; on the device `run __NAME83__`.

using Buitenzorg.App;
using Buitenzorg.UI;
using Window = Buitenzorg.UI.Window;

namespace __NAME83__;

/// <summary>Files and bytes in one folder (and its sub-folders, `depth` levels).</summary>
public sealed class FolderUsage
{
    public string Path = "";
    public int Files;
    public long Bytes;
    public override string ToString() => Path + "   " + Files + " berkas, " + Human(Bytes);

    public static string Human(long b) =>
        b >= 1L << 20 ? (b >> 20) + " MiB" : b >= 1024 ? (b >> 10) + " KiB" : b + " B";

    public static FolderUsage Measure(string path, int depth)
    {
        var u = new FolderUsage { Path = path };
        foreach (var e in Buitenzorg.App.Files.List(path))
        {
            if (e.IsDirectory)
            {
                if (depth <= 0 || e.Name == "." || e.Name == "..") continue;
                var sub = Measure(e.Path, depth - 1);
                u.Files += sub.Files;
                u.Bytes += sub.Bytes;
            }
            else
            {
                long n = Buitenzorg.App.Files.Size(e.Path);
                u.Files++;
                if (n > 0) u.Bytes += n;
            }
        }
        return u;
    }
}

public sealed class UtilWindow
{
    public readonly Window W;
    public readonly ObservableCollection<ProcessInfo> Procs = new ObservableCollection<ProcessInfo>();
    public readonly ObservableCollection<FolderUsage> Folders = new ObservableCollection<FolderUsage>();
    readonly ListBox _procList;
    readonly TextBlock _uptime = new TextBlock(""), _heap = new TextBlock(""), _ram = new TextBlock(""), _tasks = new TextBlock("");
    readonly ProgressBar _heapBar = new ProgressBar { Height = 10 };
    readonly TextBox _mount = new TextBox { Text = "/disk", Width = 160 };
    readonly TextBlock _status = new TextBlock("");
    readonly DispatcherTimer _timer = new DispatcherTimer(2000);

    public UtilWindow(bool headless = false)
    {
        W = new Window("__APPNAME__", 560, 460, headless);
        _procList = new ListBox { ItemsSource = Procs };

        var tabs = new TabControl();
        tabs.Add("Proses", ProcessPage(), IconKind.Cpu);
        tabs.Add("Sistem", SystemPage(), IconKind.Info);
        tabs.Add("Disk", DiskPage(), IconKind.Folder);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(new TextBlock("__APPNAME__").WithClass("eyebrow").DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(tabs);
        W.Content = root;
        _timer.Tick += Refresh;
        Refresh();
        ScanDisk();
        _timer.Start();
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    Control ProcessPage()
    {
        var end = new Button("Akhiri proses", _ => EndSelected()).WithClass("danger");
        var p = new DockPanel { Margin = new Thickness(0, 10, 0, 0) };
        p.Children.Add(end.DockTo(Dock.Bottom));
        p.Children.Add(_procList);
        return p;
    }

    Control SystemPage()
    {
        var s = new StackPanel { Spacing = 10, Margin = new Thickness(0, 10, 0, 0) };
        s.Children.Add(_uptime);
        s.Children.Add(_heap);
        s.Children.Add(_heapBar);
        s.Children.Add(_ram);
        s.Children.Add(_tasks);
        return s;
    }

    Control DiskPage()
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        row.Children.Add(_mount);
        row.Children.Add(new Button("Hitung", _ => ScanDisk()).WithClass("accent"));
        var p = new DockPanel { Margin = new Thickness(0, 10, 0, 0) };
        p.Children.Add(row.DockTo(Dock.Top));
        p.Children.Add(new ListBox { ItemsSource = Folders, Margin = new Thickness(0, 8, 0, 0) });
        return p;
    }

    /// <summary>Re-read the processes and system figures.</summary>
    public void Refresh()
    {
        var keep = (_procList.SelectedItem as ProcessInfo)?.Pid ?? -1;
        Procs.Clear();
        foreach (var p in Processes.List()) if (p.State != 2) Procs.Add(p);
        for (int i = 0; i < Procs.Count; i++) if (Procs[i].Pid == keep) _procList.SelectedIndex = i;

        long up = SystemInfo.UptimeSeconds;
        _uptime.Text = "Menyala " + (up / 3600) + " jam " + (up / 60 % 60) + " menit " + (up % 60) + " detik";
        long used = SystemInfo.KernelHeapUsed, total = Math.Max(1, SystemInfo.KernelHeapTotal);
        _heap.Text = "Heap kernel " + FolderUsage.Human(used) + " dari " + FolderUsage.Human(total);
        _heapBar.Value = (int)(used * 100 / total);
        _ram.Text = "Memori " + SystemInfo.MemoryMiB + " MiB";
        _tasks.Text = SystemInfo.TaskCount + " tugas kernel, " + Procs.Count + " proses";
    }

    /// <summary>Usage of the mount and each top-level folder in it.</summary>
    public void ScanDisk()
    {
        Folders.Clear();
        string root = _mount.Text.Trim();
        if (root.Length == 0) return;
        Folders.Add(FolderUsage.Measure(root, 0));
        foreach (var e in Buitenzorg.App.Files.List(root))
            if (e.IsDirectory && e.Name != "." && e.Name != "..") Folders.Add(FolderUsage.Measure(e.Path, 3));
    }

    public bool EndSelected()
    {
        if (_procList.SelectedItem is not ProcessInfo p) { _status.Text = "Pilih sebuah proses dulu"; return false; }
        if (!p.IsApp) { _status.Text = p.Name + " adalah tugas kernel"; return false; }
        bool ok = Processes.Kill(p.Pid);
        _status.Text = ok ? p.Name + " diakhiri" : "Tidak bisa mengakhiri " + p.Name;
        Refresh();
        return ok;
    }

    public static Checks SelfTest()
    {
        var c = new Checks("__APPNAME__");
        c.That(FolderUsage.Human(512) == "512 B" && FolderUsage.Human(4096) == "4 KiB" && FolderUsage.Human(3 << 20) == "3 MiB", "sizes read like a person would");
        var w = new UtilWindow(headless: true);
        c.That(w.Procs.Count > 0, w.Procs.Count + " processes listed");
        // a file of known size in a scratch folder
        string dir = Buitenzorg.App.Files.ScratchDirectory + "/UTILTES";
        Buitenzorg.App.Files.CreateDirectory(dir);
        Buitenzorg.App.Files.WriteAllText(dir + "/A.TXT", new string('x', 1000));
        var u = FolderUsage.Measure(dir, 0);
        c.That(u.Files == 1 && u.Bytes == 1000, "folder usage counts files and bytes (" + u + ")");
        Buitenzorg.App.Files.Delete(dir + "/A.TXT");
        Buitenzorg.App.Files.Delete(dir);
        c.That(!w.EndSelected(), "ending needs a selected process");
        w.W.RenderFrame();
        c.That(w.W.LastFrameOps > 10, "the window drew (" + w.W.LastFrameOps + " ops)");
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
            var c = UtilWindow.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        app.Run(new UtilWindow().W);
        return 0;
    }
}
