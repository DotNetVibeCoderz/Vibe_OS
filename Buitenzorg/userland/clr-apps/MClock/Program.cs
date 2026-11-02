// Jam -- managed clock: an analog face, a stopwatch with laps, a countdown
// timer and a calendar, in a TabControl. Buitenzorg.UI.

using Buitenzorg.UI;

namespace MClock;

/// <summary>The analog face: hour ticks, three hands, a sun-colored second hand.</summary>
public sealed class AnalogClock : Control
{
    public int Hour, Minute, Second;

    public AnalogClock() { IsHitTestVisible = false; }

    protected override Size MeasureOverride(Size a) { int s = Math.Min(Math.Min(a.Width, a.Height), 220); return new Size(s, s); }

    static void Hand(DrawingContext dc, int cx, int cy, double angle, int len, int width, int color)
    {
        int x = cx + (int)(Math.Sin(angle) * len), y = cy - (int)(Math.Cos(angle) * len);
        for (int t = -width / 2; t <= width / 2; t++)
        {
            bool steep = Math.Abs(y - cy) > Math.Abs(x - cx);
            if (steep) dc.Line(cx + t, cy, x + t, y, color); else dc.Line(cx, cy + t, x, y + t, color);
        }
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        int s = Math.Min(b.Width, b.Height);
        var face = new Rect(b.X + (b.Width - s) / 2, b.Y + (b.Height - s) / 2, s, s);
        int cx = face.X + s / 2, cy = face.Y + s / 2, r = s / 2;
        dc.Ellipse(face, Theme.Stroke, true);
        dc.Ellipse(face.Inflate(-3), Theme.SurfaceLo, true);
        for (int i = 0; i < 60; i++)
        {
            double a = i * Math.PI / 30;
            int outer = r - 10, inner = i % 5 == 0 ? r - 22 : r - 14;
            int c = i % 5 == 0 ? Theme.Text : Theme.TextFaint;
            dc.Line(cx + (int)(Math.Sin(a) * inner), cy - (int)(Math.Cos(a) * inner), cx + (int)(Math.Sin(a) * outer), cy - (int)(Math.Cos(a) * outer), c);
        }
        Hand(dc, cx, cy, (Hour % 12 + Minute / 60.0) * Math.PI / 6, r * 50 / 100, 5, Theme.Text);
        Hand(dc, cx, cy, (Minute + Second / 60.0) * Math.PI / 30, r * 72 / 100, 3, Theme.Accent);
        Hand(dc, cx, cy, Second * Math.PI / 30, r * 80 / 100, 1, Theme.Sun);
        dc.Ellipse(new Rect(cx - 5, cy - 5, 10, 10), Theme.Sun, true);
    }
}

public sealed class ClockViewModel : ObservableObject
{
    string _time = "", _date = "", _stopwatch = "00:00.0", _timer = "05:00";
    long _swStart, _swAccum;
    bool _swRunning;
    long _timerEnd, _timerTotal;
    bool _timerRunning;
    double _timerProgress;
    public ObservableCollection<string> Laps { get; } = new ObservableCollection<string>();

    public string Time { get => _time; private set => SetProperty(ref _time, value); }
    public string Date { get => _date; private set => SetProperty(ref _date, value); }
    public string Stopwatch { get => _stopwatch; private set => SetProperty(ref _stopwatch, value); }
    public string TimerText { get => _timer; private set => SetProperty(ref _timer, value); }
    public double TimerProgress { get => _timerProgress; private set => SetProperty(ref _timerProgress, value); }
    public bool StopwatchRunning => _swRunning;
    public bool TimerRunning => _timerRunning;
    public event Action? TimerDone;

    static readonly string[] Days = { "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu", "Minggu" };

    public void Tick(AnalogClock face)
    {
        var n = DateTime.Now;
        face.Hour = n.Hour; face.Minute = n.Minute; face.Second = n.Second;
        Time = n.Hour.ToString("00") + ":" + n.Minute.ToString("00") + ":" + n.Second.ToString("00");
        var d = new Buitenzorg.UI.Date(n.Year, n.Month, n.Day);
        Date = Days[d.DayOfWeek] + ", " + d.LongText;
        if (_swRunning) Stopwatch = FormatMs(ElapsedStopwatch);
        if (_timerRunning)
        {
            long left = Math.Max(0, _timerEnd - Clock.Now);
            TimerText = FormatClock(left);
            TimerProgress = 100.0 * (_timerTotal - left) / Math.Max(1, _timerTotal);
            if (left == 0) { _timerRunning = false; OnPropertyChanged(nameof(TimerRunning)); TimerDone?.Invoke(); }
        }
    }

    public long ElapsedStopwatch => _swAccum + (_swRunning ? Clock.Now - _swStart : 0);

    public static string FormatMs(long ms) => (ms / 60000).ToString("00") + ":" + (ms / 1000 % 60).ToString("00") + "." + (ms / 100 % 10);
    public static string FormatClock(long ms) { long s = (ms + 999) / 1000; return (s / 60).ToString("00") + ":" + (s % 60).ToString("00"); }

    public void StartStop()
    {
        if (_swRunning) { _swAccum += Clock.Now - _swStart; _swRunning = false; }
        else { _swStart = Clock.Now; _swRunning = true; }
        Stopwatch = FormatMs(ElapsedStopwatch);
        OnPropertyChanged(nameof(StopwatchRunning));
    }

    public void Lap() { if (_swRunning) Laps.Insert(0, "Putaran " + (Laps.Count + 1) + "   " + FormatMs(ElapsedStopwatch)); }

    public void ResetStopwatch()
    {
        _swRunning = false; _swAccum = 0;
        Laps.Clear();
        Stopwatch = FormatMs(0);
        OnPropertyChanged(nameof(StopwatchRunning));
    }

    public void StartTimer(double minutes)
    {
        _timerTotal = (long)(minutes * 60000);
        _timerEnd = Clock.Now + _timerTotal;
        _timerRunning = true;
        TimerProgress = 0;
        TimerText = FormatClock(_timerTotal);
        OnPropertyChanged(nameof(TimerRunning));
    }

    public void StopTimer() { _timerRunning = false; TimerProgress = 0; OnPropertyChanged(nameof(TimerRunning)); }
}

public static class ClockApp
{
    public static (Window, ClockViewModel, TabControl, AnalogClock) Build(bool headless = false)
    {
        var vm = new ClockViewModel();
        var w = new Window("Jam", 420, 470, headless);
        var notes = headless ? null : new WindowNotificationManager(w);
        var face = new AnalogClock { Margin = new Thickness(0, 8) };

        // Jam
        var clockPage = new StackPanel { Spacing = 6 };
        clockPage.Children.Add(face);
        clockPage.Children.Add(new TextBlock { FontSize = 24, HorizontalAlignment = HorizontalAlignment.Center }.Bind(TextBlock.TextProperty, vm, () => vm.Time, nameof(ClockViewModel.Time)));
        clockPage.Children.Add(new TextBlock { HorizontalAlignment = HorizontalAlignment.Center }.WithClass("muted").Bind(TextBlock.TextProperty, vm, () => vm.Date, nameof(ClockViewModel.Date)));

        // Stopwatch
        var start = new Button("Mulai") { MinWidth = 90 }.WithClass("accent");
        vm.PropertyChanged += (_, n) => { if (n == nameof(ClockViewModel.StopwatchRunning)) start.Content = vm.StopwatchRunning ? "Jeda" : "Mulai"; };
        start.Click += _ => vm.StartStop();
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center };
        buttons.Children.Add(start);
        buttons.Children.Add(new Button("Putaran", _ => vm.Lap()));
        buttons.Children.Add(new Button("Ulang", _ => vm.ResetStopwatch()).WithClass("subtle"));
        var swPage = new DockPanel();
        var big = new TextBlock { FontSize = 24, HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 16, 0, 14) }
            .Bind(TextBlock.TextProperty, vm, () => vm.Stopwatch, nameof(ClockViewModel.Stopwatch));
        swPage.Children.Add(big.DockTo(Dock.Top));
        swPage.Children.Add(buttons.DockTo(Dock.Top));
        swPage.Children.Add(new ListBox { ItemsSource = vm.Laps, Margin = new Thickness(0, 14, 0, 0) });

        // Timer
        var minutes = new NumericUpDown { Minimum = 0.25, Maximum = 120, Increment = 1, Decimals = 2, Value = 5, Width = 150, HorizontalAlignment = HorizontalAlignment.Center };
        var progress = new ProgressBar { Margin = new Thickness(20, 10, 20, 0) }.Bind(ProgressBar.ValueProperty, vm, () => vm.TimerProgress, nameof(ClockViewModel.TimerProgress));
        var go = new Button("Mulai timer").WithClass("sun");
        go.Click += _ => { if (vm.TimerRunning) vm.StopTimer(); else vm.StartTimer(minutes.Value); };
        vm.PropertyChanged += (_, n) => { if (n == nameof(ClockViewModel.TimerRunning)) go.Content = vm.TimerRunning ? "Hentikan" : "Mulai timer"; };
        vm.TimerDone += () => notes?.Show(new Notification("Waktu habis", "Timer " + minutes.Value + " menit selesai.", NotificationType.Warning, 6000));
        var timerPage = new StackPanel { Spacing = 12 };
        timerPage.Children.Add(new TextBlock("MENIT") { HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 12, 0, 0) }.WithClass("faint"));
        timerPage.Children.Add(minutes);
        timerPage.Children.Add(new TextBlock { FontSize = 24, HorizontalAlignment = HorizontalAlignment.Center }.Bind(TextBlock.TextProperty, vm, () => vm.TimerText, nameof(ClockViewModel.TimerText)));
        timerPage.Children.Add(progress);
        timerPage.Children.Add(new Border { Child = go, HorizontalAlignment = HorizontalAlignment.Center });

        // Kalender
        var cal = new Calendar { HorizontalAlignment = HorizontalAlignment.Center };
        var picked = new TextBlock { HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 10, 0, 0) }.WithClass("muted");
        picked.Text = "Pilih tanggal di kalender.";
        cal.SelectedDateChanged += c2 => picked.Text = c2.SelectedDate.HasValue ? "Dipilih: " + c2.SelectedDate.Value.LongText : "";
        var calPage = new StackPanel();
        calPage.Children.Add(cal);
        calPage.Children.Add(picked);

        var tabs = new TabControl()
            .Add("Jam", clockPage, IconKind.Clock)
            .Add("Stopwatch", swPage, IconKind.Play)
            .Add("Timer", timerPage, IconKind.Refresh)
            .Add("Kalender", calPage, IconKind.Grid);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Utilitas", "Jam", IconKind.Clock, 0xE9B44C).DockTo(Dock.Top));
        root.Children.Add(tabs);
        w.Content = root;

        vm.Tick(face);
        if (!headless)
        {
            var t = new DispatcherTimer(100);
            int frames = 0;
            t.Tick += () =>
            {
                // the face only redraws once a second; the stopwatch every tick
                vm.Tick(face);
                if (++frames % 10 == 0 || vm.StopwatchRunning) face.InvalidateVisual();
            };
            t.Start();
        }
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, vm, tabs, face);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Jam");
        var (w, vm, tabs, face) = Build(headless: true);
        c.That(vm.Time.Length == 8 && vm.Time[2] == ':', "digital time " + vm.Time);
        c.That(vm.Date.Contains(","), "date line " + vm.Date);
        w.RenderFrame();
        c.That(w.LastFrameOps > 60, "analog face drew " + w.LastFrameOps + " ops");
        // stopwatch through the tab and its buttons
        tabs.SelectedIndex = 1;
        w.UpdateLayout();
        Button? startBtn = null;
        foreach (var d in w.Descendants()) if (d is Button b && (b.Content as string) == "Mulai") startBtn = b;
        c.That(startBtn != null, "stopwatch page shows its start button");
        if (startBtn != null) w.SimulateClick(startBtn);
        c.That(vm.StopwatchRunning, "stopwatch runs after a click");
        long t0 = Clock.Now;
        while (Clock.Now - t0 < 120) { }
        vm.Lap();
        vm.StartStop();
        c.That(vm.ElapsedStopwatch >= 100 && vm.Laps.Count == 1, "elapsed " + vm.ElapsedStopwatch + " ms, one lap");
        c.Equal("01:05.3", ClockViewModel.FormatMs(65300), "stopwatch format");
        vm.StartTimer(0.001);
        t0 = Clock.Now;
        bool done = false;
        vm.TimerDone += () => done = true;
        while (!done && Clock.Now - t0 < 1000) vm.Tick(face);
        c.That(done, "countdown timer fires");
        tabs.SelectedIndex = 3;
        w.UpdateLayout();
        CalendarDayButton? day15 = null;
        foreach (var d in w.Descendants()) if (d is CalendarDayButton b && (b.Content as string) == "15" && !b.Classes.Contains("other")) day15 = b;
        if (day15 != null) w.SimulateClick(day15);
        Calendar? cal = null;
        foreach (var d in w.Descendants()) if (d is Calendar k) cal = k;
        c.That(cal?.SelectedDate?.Day == 15, "calendar day click selects the 15th");
        w.RenderFrame();
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
            var c = ClockApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _, _) = ClockApp.Build();
        app.Run(w);
        return 0;
    }
}
