namespace MagicAppGen.Ai;

/// <summary>Reference text for the managed (RustCLR), script and deploy paths,
/// returned by <c>GetApiReference</c>. Kept in step with
/// userland/clr-apps/Buitenzorg.App, Buitenzorg.UI and kernel script.rs.</summary>
internal static class ManagedApiReference
{
    public const string App =
        "MANAGED C# (RustCLR) - a normal net10.0 `dotnet build` assembly, interpreted on the device with a real GC.\n" +
        "Everything zerolib forbids is fine here: static reference fields, object[], string interpolation/concat,\n" +
        "interfaces, generics, LINQ, lambdas, delegates/events, exceptions, Dictionary/List, Regex, decimal.\n" +
        "Project rules: OutputType Exe, TargetFramework net10.0, <AssemblyName> = 8.3 UPPER-CASE name (MYAPP ->\n" +
        "/disk/MYAPP.DLL, launched with `run myapp`), ProjectReference to\n" +
        "$(BuitenzorgRoot)/userland/clr-apps/Buitenzorg.App/Buitenzorg.App.csproj (or Buitenzorg.UI for windows).\n" +
        "Use the managed-console / managed-ui / managed-gpio templates as the starting point.\n" +
        "Off the device (dotnet X.dll on the PC) the library falls back to an in-memory VFS, sample processes and a\n" +
        "simulated Pi board, so give every app a `--selftest` argument that exercises it and returns 0/1.\n" +
        "Interpreter gaps: on arrays use Array.IndexOf(args, x) instead of args.Contains(x); keep per-frame work small.\n" +
        "namespace Buitenzorg.App (BZAPP.DLL):\n" +
        "  Window(string title, int w, int h): Clear(int rgb), FillRect/DrawRect(x,y,w,h,rgb), DrawLine(x0,y0,x1,y1,rgb),\n" +
        "    DrawEllipse/FillEllipse(x,y,w,h,rgb), DrawText(x,y,string,rgb), Blit(x,y,w,h,int[] pixels), Present(); Id/Width/Height\n" +
        "    (a raw canvas window; for real UIs use Buitenzorg.UI). Colors.Black/White/Red/Green/Blue/Yellow/Gray/Leaf/Forest, Colors.Rgb(r,g,b)\n" +
        "  Keyboard.Read() -> char (0 = none; Keyboard.Escape)   Screen.Width/Height   Session.IsInteractive\n" +
        "  Programs.Start(string pathOrName, string args = \"\") -> long pid; Programs.WaitForExit(pid) -> exit code\n" +
        "  Files.List(path) -> List<FileEntry{Name,Path,IsDirectory}> (\"\" lists mounts), ReadAllText(path), ReadAllBytes(path) -> byte[]?,\n" +
        "    Size(path), WriteAllText(path, text) -> bool, ScratchDirectory (\"/ram\" on PCs, \"/disk\" where there is no /ram).\n" +
        "    Names on /disk are 8.3 upper case, root folder only. There is no delete (write an empty file).\n" +
        "  Processes.List() -> List<ProcessInfo{Pid,Name,State,StateText,CpuTicks,IsApp}>, Processes.Kill(pid)\n" +
        "  SystemInfo.UptimeSeconds/KernelHeapUsed/KernelHeapTotal/TaskCount/MemoryMiB\n" +
        "  Settings.Theme/Wallpaper/Screensaver/ScreensaverDelaySeconds/Animations/LargeCursor (get/set), Settings.Choices(key)\n" +
        "  Audio.IsPresent/SampleRate/Volume (get/set)/IsMuted, Audio.Beep(int hz = 880, int ms = 200)\n" +
        "Console.WriteLine output goes to the device console (visible in MagicAppGen's device log).";

    public const string Ui =
        "namespace Buitenzorg.UI (BZUI.DLL) - an Avalonia-style retained toolkit for managed apps. It draws with kernel\n" +
        "draw ops (never per pixel), so it is fast under the interpreter. Name clash: Buitenzorg.App also has Window -\n" +
        "use `using Buitenzorg.UI;` plus aliases like `using Files = Buitenzorg.App.Files;`.\n" +
        "Skeleton:\n" +
        "  var app = new Application();                         // first; ThemeVariant.Dark default\n" +
        "  var w = new Window(\"Title\", 520, 360, headless);     // headless=true for --selftest\n" +
        "  w.Content = rootControl; Application.Current?.Apply(w); w.UpdateLayout(); app.Run(w);\n" +
        "Self-test: w.SimulateClick(control), w.SimulateKeys(\"text\"), w.SimulateClickAt(x,y), w.RenderFrame(), w.LastFrameOps;\n" +
        "  var c = new Checks(\"App\"); c.That(bool, what); c.Equal(expected, actual, what); Console.WriteLine(c); return c.Ok ? 0 : 1;\n" +
        "Panels: StackPanel{Orientation, Spacing}, Grid{ColumnDefinitions=\"180,*,Auto\", RowDefinitions, RowSpacing, ColumnSpacing}\n" +
        "  + Grid.SetRow/SetColumn(child, i), DockPanel (child.DockTo(Dock.Top/Bottom/Left/Right), last child fills),\n" +
        "  WrapPanel, UniformGrid, Canvas, Border (.card/.inset classes), ScrollViewer, SplitView{Pane, Content, IsPaneOpen}.\n" +
        "  panel.Children.Add(c). Every Control: Margin/Padding = new Thickness(all | h,v | l,t,r,b), Width/Height,\n" +
        "  HorizontalAlignment/VerticalAlignment, Background/Foreground (int 0xRRGGBB), CornerRadius, IsVisible, IsEnabled,\n" +
        "  Focusable (set true on a custom control that takes keys), .WithClass(\"...\"), .Bind(Prop, source, () => value, nameof(Prop)).\n" +
        "Controls: TextBlock(string){Text, TextWrapping, TextTrimming} classes h1/h2/strong/muted/faint/accent/sun/eyebrow;\n" +
        "  Button(object content, Action<Button>? click) + event Click, classes accent/sun/danger/subtle; ToggleButton;\n" +
        "  CheckBox(string, bool) + IsChecked/IsCheckedChanged; RadioButton; ToggleSwitch; Slider{Minimum,Maximum,Value}+ValueChanged;\n" +
        "  ProgressBar{Value,Maximum}; TextBox{Text, Watermark, AcceptsReturn, CaretIndex}+TextChanged; NumericUpDown;\n" +
        "  ListBox{ItemsSource, ItemTemplate = o => control, SelectedIndex, SelectedItem}+SelectionChanged/ItemActivated;\n" +
        "  ComboBox(IEnumerable items, int selected)+SelectionChanged; TabControl with TabItem(string header, Control content);\n" +
        "  TreeView/TreeViewItem; DataGrid; Expander; Image; Calendar; Menu/MenuItem(header, Action, \"Ctrl+S\", IconKind) /\n" +
        "  ContextMenu; ToolTip; Dialog.Show(w, title, message, \"OK\", \"Batal\", ok => ...); WindowNotificationManager(w)\n" +
        "  .Show(new Notification(title, text, NotificationType.Success, ms)).\n" +
        "MVVM: class VM : ObservableObject { SetProperty(ref field, value) / OnPropertyChanged(name) }, ObservableCollection<T>, RelayCommand.\n" +
        "Timers: var t = new DispatcherTimer(1000); t.Tick += () => ...; t.Start(); keep background work quiet (every present\n" +
        "  recomposes the desktop - refresh at most every few seconds and only when something changed).\n" +
        "Keys: w.UnhandledKey += e => { if (e.Key == (char)19 /* Ctrl+S */) { ...; e.Handled = true; } };\n" +
        "Custom drawing: subclass Control, override MeasureOverride(Size) and Render(DrawingContext dc) with dc.Line/Ellipse/Rect...\n" +
        "Theme colors: Theme.Accent/Sun/Text/TextMuted/Surface/SurfaceLo/Stroke (ints).";

    public const string Devices =
        "namespace Buitenzorg.Devices (inside BZAPP.DLL) - Raspberry Pi 4 I/O; simulated off the Pi, so apps also run on PCs.\n" +
        "  Board.IsAvailable, IsSimulated, Model, Temperature (C, double), MaxTemperature, ArmClockHz, CoreClockHz, Voltage(),\n" +
        "    Throttled, Revision, Serial, MemoryBytes, GpioCount\n" +
        "  Led.Activity(bool on), Led.Power(bool on)\n" +
        "  var gpio = new GpioController(); gpio.OpenPin(pin, PinMode.Output|Input), Write(pin, PinValue.High|Low or bool),\n" +
        "    Read(pin) -> PinValue, SetPull(pin, PullMode.Up|Down|None), GetPinMode(pin); GpioController.IsUsable(pin):\n" +
        "    apps may use GPIO 0-27 except 14/15 (the serial console).\n" +
        "  new I2cDevice(bus, address){SpeedHz}: Write(byte[]), Read(byte[]), WriteRead(w, r), WriteByte, ReadByte,\n" +
        "    ReadRegister(reg, count) -> byte[]?, LastStatus (BusStatus); I2cDevice.Scan(bus) -> List<int>\n" +
        "  new SpiDevice(bus, chipSelect, clockHz = 1_000_000, mode = 0): TransferFullDuplex(w, r), Write(byte[])\n" +
        "  new SerialPort(port 2..5, baud = 115200): Open(), Write(byte[] | string), Read(byte[]) -> count, Close()\n" +
        "Native C# equivalent: bzhw.cs (namespace Buitenzorg.Devices) with fixed byte buffers.";

    public const string Scripts =
        "SCRIPTS (Python / JavaScript / TypeScript) run on the in-kernel interpreter (kernel script.rs). On the device:\n" +
        "  script py /disk/NAME.PY   |   script js /disk/NAME.JS   |   script ts /disk/NAME.TS  (aliases py/js/ts)\n" +
        "SUPPORTED: integer numbers, strings, booleans (true/True, false/False), nil/null/None; + - * / % on ints,\n" +
        "  string + string (use str(n) to convert), comparisons, and/or/not (&& || ! in JS); variables (let/var/const in\n" +
        "  JS/TS); if/else (elif in Python); while; for (C-style in JS/TS, `for i in range(a, b)` in Python); functions\n" +
        "  with parameters, return and recursion; print(...) / console.log(...) (several args are joined by spaces);\n" +
        "  str(x); len(s). TypeScript: interface/type declarations and `: Type` annotations are stripped first.\n" +
        "NOT SUPPORTED (the script fails): lists/arrays, dicts/objects, classes, imports/modules, floats, lambdas/arrow\n" +
        "  functions, template strings / f-strings, methods (\"a\".upper(), arr.push) and any library. There is a step\n" +
        "  budget, so avoid very deep loops. Keep scripts small and print results.";

    public const string Deploy =
        "DEPLOY to a device (BZDP over UDP port 7070): the device runs `deploy on` (shows a 4-digit pairing code);\n" +
        "MagicAppGen's Deploy panel / the DeployToDevice tool builds the project, uploads native -> /disk/NAME.ELF,\n" +
        "managed -> /disk/NAME.DLL (+ BZAPP.DLL/BZUI.DLL when the device's copies are older), scripts -> /disk/NAME.PY|JS|TS,\n" +
        "then runs `run name [args]` or `script py /disk/NAME.PY`, and streams the console log. QEMU needs\n" +
        "`-netdev user,id=n0,hostfwd=udp::7070-:7070` to reach the guest from the PC.";
}
