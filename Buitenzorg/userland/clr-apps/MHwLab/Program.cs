// Lab Perangkat -- the board I/O sample (Raspberry Pi 4) for managed C#:
// live built-in sensors (SoC temperature with a history chart, clocks, core
// voltage, throttling), the two LEDs, the header GPIO pins (mode, level,
// drive), an I2C bus scan, and SPI / serial loopback tests. Buitenzorg.UI +
// Buitenzorg.Devices (BZAPP.DLL); off the device it runs on the simulated board.

using Buitenzorg.Devices;
using Buitenzorg.UI;

namespace MHwLab;

/// <summary>The temperature history: the last samples as a line chart over a faint grid.</summary>
public sealed class TempChart : Control
{
    public readonly List<double> Samples = new();
    public double Max = 85;

    public TempChart() { IsHitTestVisible = false; }

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(120, a.Width), 110);

    public void Add(double t)
    {
        if (double.IsNaN(t)) return;
        Samples.Add(t);
        if (Samples.Count > 60) Samples.RemoveAt(0);
        InvalidateVisual();
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(b, 8, Theme.SurfaceLo);
        for (int i = 1; i < 4; i++) dc.Line(b.X + 6, b.Y + b.Height * i / 4, b.X + b.Width - 6, b.Y + b.Height * i / 4, Theme.Stroke);
        dc.Text(b.X + 8, b.Y + 4, (int)Max + " C", Theme.TextFaint);
        if (Samples.Count < 2) return;
        int Y(double t) => b.Y + b.Height - 6 - (int)(Math.Clamp(t / Max, 0, 1) * (b.Height - 12));
        int X(int i) => b.X + 6 + i * (b.Width - 12) / 59;
        for (int i = 1; i < Samples.Count; i++)
        {
            int c = Samples[i] > Max * 0.85 ? Theme.Danger : Theme.Accent;
            dc.Line(X(i - 1), Y(Samples[i - 1]), X(i), Y(Samples[i]), c);
            dc.Line(X(i - 1), Y(Samples[i - 1]) + 1, X(i), Y(Samples[i]) + 1, c);
        }
    }
}

/// <summary>One header pin in the GPIO table.</summary>
public sealed class PinRow
{
    public int Pin;
    public string Mode = "";
    public string Level = "";
}

public sealed class LabViewModel : ObservableObject
{
    readonly GpioController _gpio = new();
    string _temp = "", _clock = "", _volt = "", _throttle = "", _model = "", _busResult = "Belum diuji.";
    double _tempPercent;
    public ObservableCollection<PinRow> Pins { get; } = new ObservableCollection<PinRow>();
    public TempChart Chart { get; } = new TempChart();

    public string Model { get => _model; private set => SetProperty(ref _model, value); }
    public string Temperature { get => _temp; private set => SetProperty(ref _temp, value); }
    public double TemperaturePercent { get => _tempPercent; private set => SetProperty(ref _tempPercent, value); }
    public string Clock { get => _clock; private set => SetProperty(ref _clock, value); }
    public string Voltage { get => _volt; private set => SetProperty(ref _volt, value); }
    public string Throttle { get => _throttle; private set => SetProperty(ref _throttle, value); }
    public string BusResult { get => _busResult; set => SetProperty(ref _busResult, value); }

    public static string Celsius(double t) => double.IsNaN(t) ? "?" : t.ToString("0.0") + " C";

    public static string ThrottleText(ThrottleState s)
    {
        if (s == ThrottleState.None) return "normal";
        var parts = new List<string>();
        if ((s & ThrottleState.UnderVoltage) != 0) parts.Add("tegangan kurang");
        if ((s & ThrottleState.Throttled) != 0) parts.Add("diperlambat");
        if ((s & ThrottleState.FrequencyCapped) != 0) parts.Add("frekuensi dibatasi");
        if ((s & ThrottleState.SoftTemperatureLimit) != 0) parts.Add("batas suhu");
        if (parts.Count == 0) parts.Add("pernah: 0x" + ((int)s).ToString("X"));
        return string.Join(", ", parts);
    }

    public void RefreshSensors()
    {
        Model = Board.Model;
        double t = Board.Temperature, max = Board.MaxTemperature;
        if (!double.IsNaN(max) && max > 0) Chart.Max = max;
        Temperature = Celsius(t);
        TemperaturePercent = double.IsNaN(t) ? 0 : Math.Clamp(t * 100 / Chart.Max, 0, 100);
        Chart.Add(t);
        Clock = (Board.ArmClockHz / 1_000_000) + " / " + (Board.CoreClockHz / 1_000_000) + " MHz";
        double v = Board.Voltage(1);
        Voltage = double.IsNaN(v) ? "?" : v.ToString("0.000") + " V";
        Throttle = ThrottleText(Board.Throttled);
    }

    public void RefreshPins()
    {
        var rows = new List<PinRow>();
        for (int p = 0; p <= 27; p++)
        {
            if (!GpioController.IsUsable(p)) continue;
            var m = _gpio.GetPinMode(p);
            rows.Add(new PinRow
            {
                Pin = p,
                Mode = m == PinMode.Input ? "masukan" : m == PinMode.Output ? "keluaran" : m.ToString(),
                Level = _gpio.Read(p) == PinValue.High ? "TINGGI" : "rendah",
            });
        }
        Pins.ResetWith(rows);
    }

    /// <summary>Drive a pin and read it back (the table is refreshed by the caller).</summary>
    public bool Drive(int pin, bool high)
    {
        bool ok = _gpio.SetPinMode(pin, PinMode.Output) && _gpio.Write(pin, high);
        return ok && (_gpio.Read(pin) == PinValue.High) == high;
    }

    public void MakeInput(int pin) => _gpio.SetPinMode(pin, PinMode.Input);

    public List<int> ScanI2c()
    {
        var found = I2cDevice.Scan(1);
        BusResult = found.Count == 0 ? "I2C bus 1: tidak ada perangkat menjawab."
            : "I2C bus 1: " + string.Join(" ", found.ConvertAll(a => "0x" + a.ToString("X2")));
        return found;
    }

    public bool SpiLoopback()
    {
        var spi = new SpiDevice(0, 0, 1_000_000, 0);
        byte[] tx = { 0xB2, 0x5A, 0x01, 0xFE }, rx = new byte[4];
        bool ok = spi.TransferFullDuplex(tx, rx);
        bool same = ok && rx[0] == tx[0] && rx[1] == tx[1] && rx[2] == tx[2] && rx[3] == tx[3];
        BusResult = !ok ? "SPI0: transfer gagal (" + spi.LastStatus + ")."
            : same ? "SPI0: loopback cocok (jumper MOSI GPIO 10 - MISO GPIO 9)."
            : "SPI0: transfer selesai; pasang jumper GPIO 10 - GPIO 9 untuk loopback.";
        return ok;
    }

    public bool UartLoopback()
    {
        using var port = new SerialPort(3, 115200);
        if (!port.Open()) { BusResult = "UART3: tidak dapat dibuka."; return false; }
        int sent = port.Write("buitenzorg");
        var buf = new byte[16];
        int got = 0;
        for (int i = 0; i < 200 && got < 10; i++)
        {
            int n = port.Read(buf);
            if (n > 0) got += n;
        }
        BusResult = sent != 10 ? "UART3: kirim gagal."
            : got >= 10 ? "UART3: loopback cocok (jumper TX GPIO 4 - RX GPIO 5)."
            : "UART3: 10 byte terkirim; pasang jumper GPIO 4 - GPIO 5 untuk loopback.";
        return sent == 10;
    }
}

public static class LabApp
{
    public static (Window, LabViewModel, DataGrid) Build(bool headless = false)
    {
        var vm = new LabViewModel();
        var w = new Window("Lab Perangkat", 680, 520, headless);
        var notes = headless ? null : new WindowNotificationManager(w);

        TextBlock Bound(Func<string> get, string name) => new TextBlock().Bind(TextBlock.TextProperty, vm, get, name);
        var tempBar = new ProgressBar { Margin = new Thickness(0, 4, 0, 0) }.Bind(ProgressBar.ValueProperty, vm, () => vm.TemperaturePercent, nameof(LabViewModel.TemperaturePercent));
        var cards = new UniformGrid { Columns = 4, Spacing = 10, Margin = new Thickness(0, 0, 0, 10) };
        cards.Children.Add(Kit.Stat("Suhu SoC", Bound(() => vm.Temperature, nameof(LabViewModel.Temperature)), tempBar));
        cards.Children.Add(Kit.Stat("Clock ARM / core", Bound(() => vm.Clock, nameof(LabViewModel.Clock))));
        cards.Children.Add(Kit.Stat("Tegangan core", Bound(() => vm.Voltage, nameof(LabViewModel.Voltage))));
        cards.Children.Add(Kit.Stat("Throttling", Bound(() => vm.Throttle, nameof(LabViewModel.Throttle))));

        // LEDs
        var green = new ToggleSwitch("", false) { OnContent = "LED hijau nyala", OffContent = "LED hijau mati", Padding = new Thickness(0) };
        green.IsCheckedChanged += t => Led.Activity(t.IsChecked);
        var red = new ToggleSwitch("", true) { OnContent = "LED merah nyala", OffContent = "LED merah mati", Padding = new Thickness(0) };
        red.IsCheckedChanged += t => Led.Power(t.IsChecked);
        var leds = new StackPanel { Orientation = Orientation.Vertical, Spacing = 6, Margin = new Thickness(0, 8, 0, 8) };
        leds.Children.Add(green);
        leds.Children.Add(red);

        // bus tests
        var busLine = new TextBlock().WithClass("faint").Bind(TextBlock.TextProperty, vm, () => vm.BusResult, nameof(LabViewModel.BusResult));
        var scan = new Button(new TextBlock("Pindai I2C"));
        scan.Click += _ => vm.ScanI2c();
        var spi = new Button(new TextBlock("Uji SPI"));
        spi.Click += _ => vm.SpiLoopback();
        var uart = new Button(new TextBlock("Uji UART3"));
        uart.Click += _ => vm.UartLoopback();
        var busButtons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        busButtons.Children.Add(scan);
        busButtons.Children.Add(spi);
        busButtons.Children.Add(uart);

        var left = new StackPanel { Orientation = Orientation.Vertical, Spacing = 6 };
        left.Children.Add(new TextBlock("Riwayat suhu (2 detik per titik)").WithClass("faint"));
        left.Children.Add(vm.Chart);
        left.Children.Add(leds);
        left.Children.Add(busButtons);
        left.Children.Add(busLine);

        // GPIO table + actions on the selected pin
        var grid = new DataGrid();
        grid.AddColumn(new DataGridColumn("GPIO", o => ((PinRow)o).Pin.ToString(), "60") { SortKey = o => ((PinRow)o).Pin });
        grid.AddColumn(new DataGridColumn("Mode", o => ((PinRow)o).Mode, "2*"));
        grid.AddColumn(new DataGridColumn("Level", o => ((PinRow)o).Level, "2*"));
        grid.ItemsSource = vm.Pins;
        var high = new Button(new TextBlock("Tinggi")) { IsEnabled = false };
        var low = new Button(new TextBlock("Rendah")) { IsEnabled = false };
        var input = new Button(new TextBlock("Masukan")) { IsEnabled = false };
        grid.SelectionChanged += g => { bool sel = g.SelectedItem is PinRow; high.IsEnabled = low.IsEnabled = input.IsEnabled = sel; };
        void Drive(bool level)
        {
            if (grid.SelectedItem is not PinRow r) return;
            bool ok = vm.Drive(r.Pin, level);
            vm.RefreshPins();
            notes?.Show(new Notification("GPIO " + r.Pin, ok ? (level ? "tinggi" : "rendah") : "tidak terbaca kembali", ok ? NotificationType.Success : NotificationType.Warning, 1800));
        }
        high.Click += _ => Drive(true);
        low.Click += _ => Drive(false);
        input.Click += _ => { if (grid.SelectedItem is PinRow r) { vm.MakeInput(r.Pin); vm.RefreshPins(); } };
        var pinButtons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Margin = new Thickness(0, 6, 0, 0) };
        pinButtons.Children.Add(high);
        pinButtons.Children.Add(low);
        pinButtons.Children.Add(input);
        var right = new DockPanel { Margin = new Thickness(12, 0, 0, 0) };
        right.Children.Add(pinButtons.DockTo(Dock.Bottom));
        right.Children.Add(grid);

        var body = new Grid();
        body.Columns.Add(GridLength.Star(3));
        body.Columns.Add(GridLength.Star(2));
        body.Children.Add(left);
        Grid.SetColumn(right, 1);
        body.Children.Add(right);

        var model = new TextBlock().WithClass("faint").Bind(TextBlock.TextProperty, vm, () => vm.Model, nameof(LabViewModel.Model));
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Perangkat keras", "Lab Perangkat", IconKind.Cpu, 0x9BD08F, model).DockTo(Dock.Top));
        root.Children.Add(cards.DockTo(Dock.Top));
        root.Children.Add(body);
        w.Content = root;

        vm.RefreshSensors();
        vm.RefreshPins();
        if (!headless)
        {
            var timer = new DispatcherTimer(2000);
            timer.Tick += vm.RefreshSensors;
            timer.Start();
        }
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, vm, grid);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Lab Perangkat");
        var (w, vm, grid) = Build(headless: true);
        if (!Board.IsAvailable)
        {
            c.That(true, "no board I/O on this machine: " + Board.Model);
            return c;
        }
        double t = Board.Temperature;
        c.That(!double.IsNaN(t) && t > 0 && t < 120, "SoC temperature " + LabViewModel.Celsius(t));
        c.That(vm.Temperature.EndsWith(" C"), "temperature card filled");
        c.That(Led.Activity(true) && Led.Activity(false), "activity LED switched");
        c.That(vm.Drive(17, true) && vm.Drive(17, false), "GPIO 17 driven high/low and read back");
        vm.MakeInput(17);
        vm.RefreshPins();
        c.That(vm.Pins.Count == 26, "header pins listed (" + vm.Pins.Count + ")");
        vm.ScanI2c();
        c.That(vm.BusResult.StartsWith("I2C bus 1"), "I2C bus 1 scanned: " + vm.BusResult);
        c.That(vm.SpiLoopback(), "SPI0 transfer");
        c.That(vm.UartLoopback(), "UART3 open + write");
        c.Equal("normal", LabViewModel.ThrottleText(ThrottleState.None), "throttle text");
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
            var c = LabApp.SelfTest();
            Console.WriteLine(c.ToString());
            if (c.Ok) Console.WriteLine("MILESTONE: HWLAB OK");
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = LabApp.Build();
        app.Run(w);
        return 0;
    }
}
