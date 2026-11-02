// Pemantau -- the managed task manager: live stat cards (uptime, tasks,
// kernel heap with a progress bar, memory), a sortable DataGrid of processes
// refreshed by a DispatcherTimer, an "apps only" toggle, and ending a task
// behind a confirmation dialog. Buitenzorg.UI.

using Buitenzorg.UI;
using ProcessInfo = Buitenzorg.App.ProcessInfo;
using Processes = Buitenzorg.App.Processes;
using SystemInfo = Buitenzorg.App.SystemInfo;

namespace MTasks;

public sealed class TasksViewModel : ObservableObject
{
    string _uptime = "", _tasks = "", _apps = "", _heap = "", _memory = "";
    double _heapPercent;
    bool _appsOnly;
    string _shown = "";
    public ObservableCollection<ProcessInfo> Rows { get; } = new ObservableCollection<ProcessInfo>();

    public string Uptime { get => _uptime; private set => SetProperty(ref _uptime, value); }
    public string Tasks { get => _tasks; private set => SetProperty(ref _tasks, value); }
    public string Apps { get => _apps; private set => SetProperty(ref _apps, value); }
    public string Heap { get => _heap; private set => SetProperty(ref _heap, value); }
    public string Memory { get => _memory; private set => SetProperty(ref _memory, value); }
    public double HeapPercent { get => _heapPercent; private set => SetProperty(ref _heapPercent, value); }
    public bool AppsOnly { get => _appsOnly; set { if (SetProperty(ref _appsOnly, value)) Refresh(); } }

    public static string Duration(long s) => s >= 3600 ? (s / 3600) + " j " + (s % 3600 / 60) + " m" : (s / 60) + " m " + (s % 60) + " d";

    public void Refresh()
    {
        Uptime = Duration(SystemInfo.UptimeSeconds);
        long used = SystemInfo.KernelHeapUsed, total = Math.Max(1, SystemInfo.KernelHeapTotal);
        Heap = (used >> 20) + " / " + (total >> 20) + " MB";
        HeapPercent = used * 100.0 / total;
        Memory = SystemInfo.MemoryMiB + " MB";
        var list = new List<ProcessInfo>();
        int apps = 0;
        foreach (var p in Processes.List())
        {
            if (p.State == 2) continue;
            if (p.IsApp) apps++;
            if (!_appsOnly || p.IsApp) list.Add(p);
        }
        Tasks = list.Count + " aktif";
        Apps = apps + " aplikasi";
        // Rebuilding the grid redraws the window and recomposes the desktop: only when the list changed.
        var key = new System.Text.StringBuilder();
        foreach (var p in list) key.Append(p.Pid).Append(p.State).Append(',');
        if (key.ToString() != _shown) { _shown = key.ToString(); Rows.ResetWith(list); }
    }
}

public static class TasksApp
{
    public static (Window, TasksViewModel, DataGrid) Build(bool headless = false)
    {
        var vm = new TasksViewModel();
        var w = new Window("Pemantau", 600, 440, headless);
        var notes = headless ? null : new WindowNotificationManager(w);

        var uptime = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Uptime, nameof(TasksViewModel.Uptime));
        var tasks = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Tasks, nameof(TasksViewModel.Tasks));
        var heap = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Heap, nameof(TasksViewModel.Heap));
        var heapBar = new ProgressBar { Margin = new Thickness(0, 4, 0, 0) }.Bind(ProgressBar.ValueProperty, vm, () => vm.HeapPercent, nameof(TasksViewModel.HeapPercent));
        var memory = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Memory, nameof(TasksViewModel.Memory));
        var cards = new UniformGrid { Columns = 4, Spacing = 10, Margin = new Thickness(0, 0, 0, 12) };
        cards.Children.Add(Kit.Stat("Aktif sejak", uptime));
        var appsLine = new TextBlock().WithClass("faint").Bind(TextBlock.TextProperty, vm, () => vm.Apps, nameof(TasksViewModel.Apps));
        cards.Children.Add(Kit.Stat("Tugas", tasks, appsLine));
        cards.Children.Add(Kit.Stat("Heap kernel", heap, heapBar));
        cards.Children.Add(Kit.Stat("Memori", memory));

        var grid = new DataGrid();
        grid.AddColumn(new DataGridColumn("PID", o => ((ProcessInfo)o).Pid.ToString(), "60") { SortKey = o => ((ProcessInfo)o).Pid });
        grid.AddColumn(new DataGridColumn("Nama", o => ((ProcessInfo)o).Name, "3*"));
        grid.AddColumn(new DataGridColumn("Jenis", o => ((ProcessInfo)o).IsApp ? "aplikasi" : "kernel", "2*"));
        grid.AddColumn(new DataGridColumn("Status", o => ((ProcessInfo)o).StateText, "2*"));
        grid.AddColumn(new DataGridColumn("CPU", o => ((ProcessInfo)o).CpuTicks.ToString(), "70") { SortKey = o => ((ProcessInfo)o).CpuTicks, Alignment = TextAlignment.Right });
        grid.ItemsSource = vm.Rows;

        var end = new Button(new TextBlock("Akhiri tugas")) { IsEnabled = false }.WithClass("danger");
        grid.SelectionChanged += g => end.IsEnabled = g.SelectedItem is ProcessInfo p && p.IsApp;
        end.Click += _ =>
        {
            if (grid.SelectedItem is not ProcessInfo p) return;
            Dialog.Show(w, "Akhiri " + p.Name + "?", "Proses " + p.Pid + " dihentikan; data yang belum disimpan hilang.", "Akhiri", "Batal", ok =>
            {
                if (!ok) return;
                bool done = Processes.Kill(p.Pid);
                notes?.Show(done ? new Notification("Dihentikan", p.Name + " (" + p.Pid + ")", NotificationType.Success, 2500)
                                 : new Notification("Gagal", "Proses " + p.Pid + " tidak dapat dihentikan.", NotificationType.Error));
                vm.Refresh();
            });
        };
        var appsOnly = new ToggleSwitch("", false) { OnContent = "Hanya aplikasi", OffContent = "Semua tugas", Padding = new Thickness(0) };
        appsOnly.BindTwoWay(ToggleButton.IsCheckedProperty, vm, nameof(TasksViewModel.AppsOnly), () => vm.AppsOnly, v => vm.AppsOnly = v);

        var footer = new DockPanel { Margin = new Thickness(0, 10, 0, 0) };
        footer.Children.Add(end.DockTo(Dock.Right));
        footer.Children.Add(appsOnly);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Sistem", "Pemantau", IconKind.Cpu, 0x7FB2D9).DockTo(Dock.Top));
        root.Children.Add(cards.DockTo(Dock.Top));
        root.Children.Add(footer.DockTo(Dock.Bottom));
        root.Children.Add(grid);
        w.Content = root;

        vm.Refresh();
        if (!headless)
        {
            var timer = new DispatcherTimer(5000);
            timer.Tick += vm.Refresh;
            timer.Start();
        }
        Application.Current?.Apply(w);
        w.UpdateLayout();
        grid.Focus();
        return (w, vm, grid);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Pemantau");
        var (w, vm, grid) = Build(headless: true);
        c.That(vm.Rows.Count > 0, "processes listed (" + vm.Rows.Count + ")");
        c.That(vm.Uptime.Length > 0 && vm.Heap.Contains("MB"), "stat cards filled");
        c.That(vm.HeapPercent > 0 && vm.HeapPercent <= 100, "heap percentage " + (int)vm.HeapPercent);
        int all = vm.Rows.Count;
        vm.AppsOnly = true;
        bool onlyApps = true;
        foreach (var p in vm.Rows) if (!p.IsApp) onlyApps = false;
        c.That(onlyApps && vm.Rows.Count <= all, "apps-only filter");
        vm.AppsOnly = false;
        w.UpdateLayout();
        grid.Select(0);
        c.That(grid.SelectedItem != null, "row selection");
        grid.SortBy(4);
        c.That(true, "sort by CPU");
        c.Equal("1 m 5 d", TasksViewModel.Duration(65), "duration format");
        w.RenderFrame();
        c.That(w.LastFrameOps > 30, "frame drew " + w.LastFrameOps + " ops");
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
            var c = TasksApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = TasksApp.Build();
        app.Run(w);
        return 0;
    }
}
