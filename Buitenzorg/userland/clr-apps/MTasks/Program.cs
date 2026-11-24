// Pemantau -- the managed task manager, in three tabs: Proses (a sortable
// DataGrid of processes, an "apps only" toggle, ending a task behind a
// confirmation dialog), Kinerja (live stat cards plus history graphs of CPU
// activity and kernel heap), Detail (every task with its CPU share and the
// selected one's figures). Refreshed by a DispatcherTimer. Buitenzorg.UI.

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

    /// <summary>The last <see cref="HistoryLength"/> samples, oldest first: CPU activity in
    /// percent of one CPU (ticks charged to tasks other than the idle ones, per tick
    /// of uptime), and kernel heap use in percent.</summary>
    public const int HistoryLength = 60;
    public List<double> CpuHistory { get; } = new List<double>();
    public List<double> HeapHistory { get; } = new List<double>();
    public ObservableCollection<ProcessInfo> AllRows { get; } = new ObservableCollection<ProcessInfo>();
    string _cpu = "", _detail = "";
    public string Cpu { get => _cpu; private set => SetProperty(ref _cpu, value); }
    public string Detail { get => _detail; set => SetProperty(ref _detail, value); }
    long _lastBusy = -1, _lastUptimeTicks;
    long _totalCpu;
    string _allShown = "";
    public event Action? Sampled;

    static bool IsIdle(ProcessInfo p) => p.Name.StartsWith("idle");

    /// <summary>The share of every task's CPU time in percent ("12.5 %").</summary>
    public string Share(ProcessInfo p)
    {
        if (_totalCpu <= 0) return "0 %";
        long tenths = p.CpuTicks * 1000 / _totalCpu;
        return (tenths / 10) + "." + (tenths % 10) + " %";
    }

    static void Push(List<double> h, double v)
    {
        h.Add(v);
        if (h.Count > HistoryLength) h.RemoveAt(0);
    }

    public void Describe(ProcessInfo? p)
    {
        Detail = p == null ? "Pilih tugas untuk melihat rinciannya."
            : "PID " + p.Pid + "  ·  " + p.Name + "  ·  " + (p.IsApp ? "aplikasi" : "tugas kernel") + "  ·  " + p.StateText
              + "  ·  " + p.CpuTicks + " tick CPU (" + Share(p) + " dari semua)";
    }

    public static string Duration(long s) => s >= 3600 ? (s / 3600) + " j " + (s % 3600 / 60) + " m" : (s / 60) + " m " + (s % 60) + " d";

    public void Refresh()
    {
        Uptime = Duration(SystemInfo.UptimeSeconds);
        long used = SystemInfo.KernelHeapUsed, total = Math.Max(1, SystemInfo.KernelHeapTotal);
        Heap = (used >> 20) + " / " + (total >> 20) + " MB";
        HeapPercent = used * 100.0 / total;
        Memory = SystemInfo.MemoryMiB + " MB";
        var list = new List<ProcessInfo>();
        var all = new List<ProcessInfo>();
        int apps = 0;
        long busy = 0, cpuTotal = 0;
        foreach (var p in Processes.List())
        {
            cpuTotal += p.CpuTicks;
            if (!IsIdle(p)) busy += p.CpuTicks;
            if (p.State == 2) continue;
            all.Add(p);
            if (p.IsApp) apps++;
            if (!_appsOnly || p.IsApp) list.Add(p);
        }
        _totalCpu = cpuTotal;
        // CPU activity since the previous sample, against the uptime ticks that passed
        long hz = Math.Max(1, SystemInfo.TickRate), now = SystemInfo.UptimeTicks;
        double cpu = 0;
        if (_lastBusy >= 0 && now > _lastUptimeTicks)
            cpu = Math.Min(100.0, Math.Max(0.0, (busy - _lastBusy) * 100.0 / (now - _lastUptimeTicks)));
        _lastBusy = busy;
        _lastUptimeTicks = now;
        Push(CpuHistory, cpu);
        Push(HeapHistory, HeapPercentOf(used, total2: SystemInfo.KernelHeapTotal));
        Cpu = ((int)cpu) + " %";
        var allKey = new System.Text.StringBuilder();
        foreach (var p in all) allKey.Append(p.Pid).Append(p.State).Append(p.CpuTicks).Append(',');
        if (allKey.ToString() != _allShown) { _allShown = allKey.ToString(); AllRows.ResetWith(all); }
        Tasks = list.Count + " aktif";
        Apps = apps + " aplikasi";
        // Rebuilding the grid redraws the window and recomposes the desktop: only when the list changed.
        var key = new System.Text.StringBuilder();
        foreach (var p in list) key.Append(p.Pid).Append(p.State).Append(',');
        if (key.ToString() != _shown) { _shown = key.ToString(); Rows.ResetWith(list); }
        Sampled?.Invoke();
    }

    static double HeapPercentOf(long used, long total2) => used * 100.0 / Math.Max(1, total2);
}

/// <summary>A line graph of the last samples (0..100), drawn with the kernel's line op.</summary>
public sealed class HistoryGraph : Control
{
    readonly List<double> _data;
    readonly int _color;
    public string Caption { get; set; }

    public HistoryGraph(string caption, List<double> data, int color)
    {
        Caption = caption;
        _data = data;
        _color = color;
    }

    protected override Size MeasureOverride(Size a) => new Size(200, 110);

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(b, 10, Theme.SurfaceLo);
        dc.StrokeRoundRect(b, 10, Theme.StrokeSoft);
        int top = b.Y + Fonts.LineHeight(0) + 10, left = b.X + 10, w = b.Width - 20, h = b.Bottom - 10 - top;
        string last = _data.Count > 0 ? ((int)_data[_data.Count - 1]) + " %" : "-";
        dc.Text(left, b.Y + 6, Caption, Theme.TextMuted, 0);
        dc.Text(b.Right - 10 - Fonts.Measure(last, 1), b.Y + 6, last, Theme.Text, 1);
        if (w < 4 || h < 4) return;
        for (int g = 0; g <= 4; g++)
        {
            int y = top + h * g / 4;
            dc.Line(left, y, left + w, y, Theme.StrokeSoft);
        }
        int n = TasksViewModel.HistoryLength;
        int px = -1, py = -1;
        for (int i = 0; i < _data.Count; i++)
        {
            int x = left + w * (n - _data.Count + i) / (n - 1);
            int y = top + h - (int)(h * Math.Max(0, Math.Min(100, _data[i])) / 100);
            if (px >= 0)
            {
                dc.Line(px, py, x, y, _color);
                dc.Line(px, py + 1, x, y + 1, _color);
            }
            px = x;
            py = y;
        }
    }
}

public static class TasksApp
{
    public static (Window, TasksViewModel, DataGrid) Build(bool headless = false) => BuildAll(headless).Item1;

    public static ((Window, TasksViewModel, DataGrid), TabControl, DataGrid, HistoryGraph) BuildAll(bool headless = false)
    {
        var vm = new TasksViewModel();
        var w = new Window("Pemantau", 640, 480, headless);
        var notes = headless ? null : new WindowNotificationManager(w);

        // ---- Proses
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
        var processes = new DockPanel();
        processes.Children.Add(footer.DockTo(Dock.Bottom));
        processes.Children.Add(grid);

        // ---- Kinerja
        var uptime = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Uptime, nameof(TasksViewModel.Uptime));
        var tasks = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Tasks, nameof(TasksViewModel.Tasks));
        var heap = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Heap, nameof(TasksViewModel.Heap));
        var heapBar = new ProgressBar { Margin = new Thickness(0, 4, 0, 0) }.Bind(ProgressBar.ValueProperty, vm, () => vm.HeapPercent, nameof(TasksViewModel.HeapPercent));
        var memory = new TextBlock().Bind(TextBlock.TextProperty, vm, () => vm.Memory, nameof(TasksViewModel.Memory));
        var cpuLine = new TextBlock().WithClass("faint").Bind(TextBlock.TextProperty, vm, () => "CPU " + vm.Cpu, nameof(TasksViewModel.Cpu));
        var cards = new UniformGrid { Columns = 4, Spacing = 10, Margin = new Thickness(0, 0, 0, 12) };
        cards.Children.Add(Kit.Stat("Aktif sejak", uptime));
        var appsLine = new TextBlock().WithClass("faint").Bind(TextBlock.TextProperty, vm, () => vm.Apps, nameof(TasksViewModel.Apps));
        cards.Children.Add(Kit.Stat("Tugas", tasks, appsLine));
        cards.Children.Add(Kit.Stat("Heap kernel", heap, heapBar));
        cards.Children.Add(Kit.Stat("Memori", memory, cpuLine));
        var cpuGraph = new HistoryGraph("Aktivitas CPU", vm.CpuHistory, Theme.Accent);
        var heapGraph = new HistoryGraph("Heap kernel terpakai", vm.HeapHistory, Theme.Info);
        var graphs = new UniformGrid { Columns = 1, Spacing = 10 };
        graphs.Children.Add(cpuGraph);
        graphs.Children.Add(heapGraph);
        var performance = new DockPanel();
        performance.Children.Add(cards.DockTo(Dock.Top));
        performance.Children.Add(graphs);

        // ---- Detail
        var details = new DataGrid();
        details.AddColumn(new DataGridColumn("PID", o => ((ProcessInfo)o).Pid.ToString(), "60") { SortKey = o => ((ProcessInfo)o).Pid });
        details.AddColumn(new DataGridColumn("Nama", o => ((ProcessInfo)o).Name, "3*"));
        details.AddColumn(new DataGridColumn("Jenis", o => ((ProcessInfo)o).IsApp ? "aplikasi" : "kernel", "2*"));
        details.AddColumn(new DataGridColumn("Status", o => ((ProcessInfo)o).StateText, "2*"));
        details.AddColumn(new DataGridColumn("Tick CPU", o => ((ProcessInfo)o).CpuTicks.ToString(), "80") { SortKey = o => ((ProcessInfo)o).CpuTicks, Alignment = TextAlignment.Right });
        details.AddColumn(new DataGridColumn("Porsi", o => vm.Share((ProcessInfo)o), "80") { SortKey = o => ((ProcessInfo)o).CpuTicks, Alignment = TextAlignment.Right });
        details.ItemsSource = vm.AllRows;
        vm.Describe(null);
        details.SelectionChanged += g => vm.Describe(g.SelectedItem as ProcessInfo);
        var detailLine = new TextBlock { Margin = new Thickness(0, 10, 0, 0) }.WithClass("faint")
            .Bind(TextBlock.TextProperty, vm, () => vm.Detail, nameof(TasksViewModel.Detail));
        var detailPage = new DockPanel();
        detailPage.Children.Add(detailLine.DockTo(Dock.Bottom));
        detailPage.Children.Add(details);

        var tabs = new TabControl();
        tabs.Add("Proses", processes, IconKind.Cpu);
        tabs.Add("Kinerja", performance);
        tabs.Add("Detail", detailPage);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Sistem", "Pemantau", IconKind.Cpu, 0x7FB2D9).DockTo(Dock.Top));
        root.Children.Add(tabs);
        w.Content = root;

        vm.Sampled += () => { cpuGraph.InvalidateVisual(); heapGraph.InvalidateVisual(); };
        vm.Refresh();
        if (!headless)
        {
            // the graphs need a steady pulse; the grids only rebuild when their rows changed
            var timer = new DispatcherTimer(2000);
            timer.Tick += vm.Refresh;
            timer.Start();
        }
        Application.Current?.Apply(w);
        w.UpdateLayout();
        grid.Focus();
        return ((w, vm, grid), tabs, details, cpuGraph);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Pemantau");
        var ((w, vm, grid), tabs, details, cpuGraph) = BuildAll(headless: true);
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
        // tabs: Kinerja shows the history graphs, Detail every task with its share
        c.That(tabs.Tabs.Count == 3, "three tabs");
        int samples = vm.CpuHistory.Count;
        vm.Refresh();
        c.That(vm.CpuHistory.Count == samples + 1 && vm.HeapHistory.Count == vm.CpuHistory.Count, "history sampled (" + vm.CpuHistory.Count + ")");
        bool inRange = true;
        foreach (var v in vm.CpuHistory) if (v < 0 || v > 100) inRange = false;
        c.That(inRange && vm.HeapHistory[vm.HeapHistory.Count - 1] > 0, "samples in range");
        for (int i = 0; i < TasksViewModel.HistoryLength + 5; i++) vm.Refresh();
        c.That(vm.CpuHistory.Count == TasksViewModel.HistoryLength, "history keeps the last 60");
        tabs.SelectedIndex = 1;
        w.UpdateLayout();
        w.RenderFrame();
        c.That(cpuGraph.Bounds.Height >= 60 && w.LastFrameOps > 40, "performance tab drew graphs (" + w.LastFrameOps + " ops)");
        tabs.SelectedIndex = 2;
        w.UpdateLayout();
        c.That(vm.AllRows.Count >= vm.Rows.Count, "details list every task (" + vm.AllRows.Count + ")");
        details.Select(0);
        c.That(vm.Detail.StartsWith("PID ") && vm.Detail.Contains("%"), "details of the selected task");
        w.RenderFrame();
        c.That(w.LastFrameOps > 30, "details tab drew " + w.LastFrameOps + " ops");
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
