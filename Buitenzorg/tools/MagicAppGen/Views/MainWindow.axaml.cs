using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using Avalonia.Threading;
using AvaloniaEdit.TextMate;
using MagicAppGen.Models;
using MagicAppGen.Services;
using TextMateSharp.Grammars;
using Path = System.IO.Path;

namespace MagicAppGen.Views;

public partial class MainWindow : Window
{
    readonly Settings _settings;
    AiService _ai;
    AppProject? _project;
    string? _projectDir;
    string? _currentFile;
    bool _dirty, _loadingFile;
    byte[]? _attachBytes;
    string? _attachMime;
    CancellationTokenSource? _chatCts;
    DeviceDeployer? _deployer;
    uint _pid;
    bool _busy;
    readonly DispatcherTimer _logTimer;
    bool _polling;
    TextMate.Installation? _textMate;
    RegistryOptions? _registry;
    // Setting SelectedIndex raises SelectionChanged synchronously; handlers wait for this.
    bool _ready;

    static readonly IBrush LeafBrush = new SolidColorBrush(Color.Parse("#7CC4A4"));
    static readonly IBrush PollenBrush = new SolidColorBrush(Color.Parse("#E8C35A"));
    static readonly IBrush HibiscusBrush = new SolidColorBrush(Color.Parse("#E8836B"));
    static readonly IBrush VeinBrush = new SolidColorBrush(Color.Parse("#2A4436"));
    static readonly IBrush MossBrush = new SolidColorBrush(Color.Parse("#6C8A7B"));

    public MainWindow() : this(Settings.Load()) { }

    public MainWindow(Settings settings)
    {
        _settings = settings;
        InitializeComponent();

        _ai = new AiService(_settings, Log);
        _ai.ToolCalled += OnToolCalled;

        // Syntax colouring: TextMate grammars under a Dark+ theme, on our canopy background.
        try
        {
            _registry = new RegistryOptions(ThemeName.DarkPlus);
            _textMate = Editor.InstallTextMate(_registry);
        }
        catch (Exception ex) { Log($"[editor] syntax highlighting unavailable: {ex.Message}"); }
        Editor.Background = new SolidColorBrush(Color.Parse("#0E1A14"));
        Editor.TextArea.SelectionBrush = new SolidColorBrush(Color.Parse("#447CC4A4"));
        Editor.TextArea.Caret.CaretBrush = LeafBrush;
        Editor.ShowLineNumbers = _settings.ShowLineNumbers;
        Editor.TextChanged += (_, _) => { if (!_loadingFile) SetDirty(true); };
        Editor.TextArea.Caret.PositionChanged += (_, _) =>
            CaretText.Text = $"Ln {Editor.TextArea.Caret.Line}, Col {Editor.TextArea.Caret.Column}";

        ProviderBox.ItemsSource = Enum.GetValues<Provider>().Select(p => ModelCatalog.For(p).DisplayName).ToList();
        ProviderBox.SelectedIndex = (int)_settings.ActiveProvider;
        RefreshProviderText();
        RefreshDevices();
        LibsCheck.IsChecked = _settings.DeployRuntimeLibraries;

        _logTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(900) };
        _logTimer.Tick += async (_, _) => await PollDeviceLog();

        KeyDown += OnWindowKeyDown;
        _ready = true;
        ShowWelcome();
        Log($"MagicAppGen siap. Repo Buitenzorg: {(_settings.BuitenzorgRoot is { Length: > 0 } r ? r : "(tidak ditemukan - atur di Pengaturan)")}");
        UpdateProjectUi();
        SetLanguage("C#");
    }

    // ---- logging & status ---------------------------------------------------
    void Log(string line) => Dispatcher.UIThread.Post(() =>
    {
        LogBox.Text += line + "\n";
        LogBox.CaretIndex = LogBox.Text?.Length ?? 0;
    });

    enum Tone { Idle, Busy, Ok, Error }

    void Status(string s, Tone tone = Tone.Idle) => Dispatcher.UIThread.Post(() =>
    {
        StatusText.Text = s;
        StatusDot.Fill = tone switch { Tone.Busy => PollenBrush, Tone.Error => HibiscusBrush, _ => LeafBrush };
    });

    void SetDirty(bool d)
    {
        _dirty = d;
        DirtyDot.IsVisible = d;
    }

    // ---- keyboard -----------------------------------------------------------
    void OnWindowKeyDown(object? sender, KeyEventArgs e)
    {
        var ctrl = e.KeyModifiers.HasFlag(KeyModifiers.Control);
        var shift = e.KeyModifiers.HasFlag(KeyModifiers.Shift);
        switch (e.Key)
        {
            case Key.S when ctrl: OnSave(this, e); e.Handled = true; break;
            case Key.B when ctrl: OnBuild(this, e); e.Handled = true; break;
            case Key.G when ctrl: OnGoToLine(this, e); e.Handled = true; break;
            case Key.O when ctrl && shift: OnOpenProject(this, e); e.Handled = true; break;
            case Key.O when ctrl: OnOpenFile(this, e); e.Handled = true; break;
            case Key.N when ctrl && shift: OnNewTemplate(this, e); e.Handled = true; break;
            case Key.F5 when shift: OnStop(this, e); e.Handled = true; break;
            case Key.F5: OnBuildDeployRun(this, e); e.Handled = true; break;
        }
    }

    // ---- project ------------------------------------------------------------
    void UpdateProjectUi()
    {
        bool has = _projectDir != null;
        ExplorerEmpty.IsVisible = !has;
        FileTree.IsVisible = has;
        ExplorerName.IsVisible = has;
        ExplorerPath.IsVisible = has;
        if (_project != null)
        {
            ProjectTitle.Text = new DirectoryInfo(_project.Dir).Name;
            ProjectTitle.Classes.Set("muted", false);
            ProjectKindText.Text = _project.KindLabel;
            ProjectKindChip.IsVisible = true;
            ProjectTarget.Text = "→ " + _project.RemotePath;
            ExplorerName.Text = _project.Name83;
            LibsCheck.IsVisible = _project.Kind == AppKind.Managed;
            Step3Text.Text = _project.RunCommand(ArgsBox.Text ?? "");
        }
        else
        {
            ProjectTitle.Text = has ? new DirectoryInfo(_projectDir!).Name : "Belum ada proyek";
            ProjectKindChip.IsVisible = false;
            ProjectTarget.Text = "";
            ExplorerName.Text = has ? new DirectoryInfo(_projectDir!).Name : "";
            LibsCheck.IsVisible = false;
            Step3Text.Text = "—";
        }
        ExplorerPath.Text = _projectDir ?? "";
        Title = _project != null ? $"{new DirectoryInfo(_project.Dir).Name} — Magic App Generator" : "Magic App Generator";
    }

    async Task OpenProject(string dir, string? fileToOpen = null)
    {
        _projectDir = dir;
        _project = AppProject.Open(dir);
        RefreshTree();
        UpdateProjectUi();
        ResetSteps();
        var file = fileToOpen ?? _project?.MainFile ?? Directory.EnumerateFiles(dir).FirstOrDefault();
        if (file != null && File.Exists(file)) await OpenFile(file);
        Status($"Proyek: {dir}");
    }

    static readonly HashSet<string> HiddenDirs = new(StringComparer.OrdinalIgnoreCase) { "bin", "obj", ".git", ".vs", "node_modules" };

    void RefreshTree()
    {
        if (_projectDir is null || !Directory.Exists(_projectDir)) { FileTree.ItemsSource = null; return; }
        FileTree.ItemsSource = BuildNodes(_projectDir, 0);
    }

    List<TreeViewItem> BuildNodes(string dir, int depth)
    {
        var items = new List<TreeViewItem>();
        if (depth > 4) return items;
        foreach (var d in Directory.GetDirectories(dir).OrderBy(x => x))
        {
            var name = Path.GetFileName(d);
            if (HiddenDirs.Contains(name)) continue;
            items.Add(new TreeViewItem
            {
                Header = NodeHeader(name, true, name == "out" ? "hasil bangun" : null),
                ItemsSource = BuildNodes(d, depth + 1),
                IsExpanded = name == "out",
                Tag = d,
            });
        }
        foreach (var f in Directory.GetFiles(dir).OrderBy(x => x))
            items.Add(new TreeViewItem { Header = NodeHeader(Path.GetFileName(f), false, null), Tag = f });
        return items;
    }

    static Control NodeHeader(string name, bool folder, string? note)
    {
        var sp = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 7 };
        sp.Children.Add(new Avalonia.Controls.Shapes.Path
        {
            Data = (Geometry)Application.Current!.FindResource(folder ? "IconFolder" : "IconFile")!,
            Stroke = folder ? LeafBrush : new SolidColorBrush(Color.Parse("#9DB5A8")),
            StrokeThickness = 1.6, Width = 13, Height = 13, Stretch = Stretch.Uniform, VerticalAlignment = VerticalAlignment.Center,
        });
        sp.Children.Add(new TextBlock { Text = name, VerticalAlignment = VerticalAlignment.Center });
        if (note != null) sp.Children.Add(new TextBlock { Text = note, Classes = { "faint" }, VerticalAlignment = VerticalAlignment.Center });
        return sp;
    }

    async void OnTreeSelection(object? sender, SelectionChangedEventArgs e)
    {
        if (FileTree.SelectedItem is TreeViewItem { Tag: string path } && File.Exists(path))
        {
            if (string.Equals(path, _currentFile, StringComparison.OrdinalIgnoreCase)) return;
            if (IsBinary(path)) { Status($"{Path.GetFileName(path)}: berkas biner ({new FileInfo(path).Length:N0} byte)"); return; }
            await SaveIfNeeded();
            await OpenFile(path);
        }
    }

    static bool IsBinary(string path) =>
        new[] { ".dll", ".elf", ".exe", ".o", ".png", ".jpg", ".bmp", ".pdb", ".bin" }.Contains(Path.GetExtension(path).ToLowerInvariant());

    async Task OpenFile(string path)
    {
        _loadingFile = true;
        Editor.Text = await File.ReadAllTextAsync(path);
        _loadingFile = false;
        _currentFile = path;
        SetDirty(false);
        FileTabText.Text = Path.GetFileName(path);
        SetLanguage(LanguageOf(path));
        Editor.ScrollToHome();
    }

    static string LanguageOf(string path) => Path.GetExtension(path).ToLowerInvariant() switch
    {
        ".py" => "Python",
        ".ts" => "TypeScript",
        ".js" => "JavaScript",
        ".csproj" or ".xml" or ".axaml" => "XML",
        ".json" or ".manifest" => "JSON",
        ".md" => "Markdown",
        _ => "C#",
    };

    void SetLanguage(string lang)
    {
        LangText.Text = lang;
        if (_textMate is null || _registry is null) return;
        var ext = lang switch
        {
            "Python" => ".py", "TypeScript" => ".ts", "JavaScript" => ".js", "XML" => ".xml",
            "JSON" => ".json", "Markdown" => ".md", _ => ".cs",
        };
        try { _textMate.SetGrammar(_registry.GetScopeByExtension(ext)); } catch { }
    }

    // ---- file / project commands -------------------------------------------
    async void OnNewBlank(object? sender, RoutedEventArgs e)
    {
        var dir = await PickFolder("Folder untuk proyek kosong");
        if (dir is null) return;
        var main = Path.Combine(dir, "main.cs");
        if (!File.Exists(main)) await File.WriteAllTextAsync(main, "// Aplikasi Buitenzorg baru. Minta Jack mengisinya.\n");
        await OpenProject(dir, main);
    }

    async void OnNewTemplate(object? sender, RoutedEventArgs e)
    {
        var choice = await new NewProjectDialog(_settings).ShowDialog<NewProjectChoice?>(this);
        if (choice is null) return;
        try
        {
            var main = ProjectTemplates.Scaffold(choice.Template, choice.Folder, choice.AppName, _settings.BuitenzorgRoot);
            Log($"Templat '{choice.Template.Title}' dibuat di {choice.Folder}");
            await OpenProject(choice.Folder, main);
            AddSystemNote($"Proyek {choice.Template.KindLabel.ToLowerInvariant()} \"{choice.AppName}\" siap. Tekan F5 untuk membangun dan menjalankannya di perangkat, atau minta saya mengembangkannya.");
        }
        catch (Exception ex) { ShowError($"Gagal membuat proyek: {ex.Message}"); }
    }

    async void OnOpenProject(object? sender, RoutedEventArgs e)
    {
        var dir = await PickFolder("Buka folder proyek");
        if (dir is null) return;
        await SaveIfNeeded();
        await OpenProject(dir);
    }

    async void OnOpenFile(object? sender, RoutedEventArgs e)
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions { Title = "Buka berkas", AllowMultiple = false });
        if (files.FirstOrDefault()?.TryGetLocalPath() is not string path) return;
        await SaveIfNeeded();
        var dir = Path.GetDirectoryName(path)!;
        if (_projectDir is null || !path.StartsWith(_projectDir, StringComparison.OrdinalIgnoreCase)) await OpenProject(dir, path);
        else await OpenFile(path);
    }

    async void OnSave(object? sender, RoutedEventArgs e) => await SaveCurrent();

    async Task SaveCurrent()
    {
        if (_currentFile is null)
        {
            var f = await StorageProvider.SaveFilePickerAsync(new FilePickerSaveOptions { Title = "Simpan sebagai" });
            if (f?.TryGetLocalPath() is not string p) return;
            _currentFile = p;
            FileTabText.Text = Path.GetFileName(p);
        }
        await File.WriteAllTextAsync(_currentFile, Editor.Text);
        SetDirty(false);
        Status($"Tersimpan: {Path.GetFileName(_currentFile)}", Tone.Ok);
    }

    async Task SaveIfNeeded() { if (_currentFile != null && _dirty) await SaveCurrent(); }

    void OnCloseProject(object? sender, RoutedEventArgs e)
    {
        _projectDir = null; _project = null; _currentFile = null;
        _loadingFile = true; Editor.Text = ""; _loadingFile = false;
        SetDirty(false);
        FileTabText.Text = "tanpa judul";
        RefreshTree();
        UpdateProjectUi();
        Status("Proyek ditutup");
    }

    void OnRefreshTree(object? sender, RoutedEventArgs e) => RefreshTree();
    void OnExit(object? sender, RoutedEventArgs e) => Close();

    async void OnGoToLine(object? sender, RoutedEventArgs e)
    {
        var line = await new GoToLineDialog().ShowDialog<int?>(this);
        if (line is int n && n >= 1 && n <= Editor.Document.LineCount)
        {
            Editor.CaretOffset = Editor.Document.GetLineByNumber(n).Offset;
            Editor.ScrollToLine(n);
            Editor.Focus();
        }
    }

    // ---- build / deploy / run ----------------------------------------------
    enum StepState { Idle, Active, Done, Failed }

    void SetStep(int n, StepState s, string? text = null)
    {
        var dot = n switch { 1 => Step1Dot, 2 => Step2Dot, _ => Step3Dot };
        var label = n switch { 1 => Step1Text, 2 => Step2Text, _ => Step3Text };
        dot.Fill = s switch { StepState.Active => PollenBrush, StepState.Done => LeafBrush, StepState.Failed => HibiscusBrush, _ => VeinBrush };
        if (text != null) label.Text = text;
    }

    void ResetSteps()
    {
        SetStep(1, StepState.Idle, "belum dibangun");
        SetStep(2, StepState.Idle, "ke /disk perangkat");
        SetStep(3, StepState.Idle, _project?.RunCommand(ArgsBox.Text ?? "") ?? "—");
        UploadProgress.IsVisible = false;
        DeviceErrorCard.IsVisible = false;
    }

    void ShowError(string message)
    {
        DeviceErrorText.Text = message;
        DeviceErrorCard.IsVisible = true;
        Status(message.Split('\n')[0], Tone.Error);
        Log("[error] " + message);
    }

    bool RequireProject()
    {
        if (_project != null) return true;
        if (_projectDir != null) _project = AppProject.Open(_projectDir);
        if (_project != null) { UpdateProjectUi(); return true; }
        ShowSide(device: true);
        ShowError("Buka atau buat proyek dulu: sebuah folder dengan .csproj (managed), .cs (native) atau main.py/js/ts (skrip).");
        return false;
    }

    async Task<BuildResult?> BuildStep()
    {
        await SaveIfNeeded();
        _project = AppProject.Open(_project!.Dir) ?? _project;
        SetStep(1, StepState.Active, $"membangun {_project.KindLabel}…");
        Status($"Membangun {_project.Name83}…", Tone.Busy);
        BottomTabs.SelectedIndex = 0;
        var r = await new ProjectBuilder(_settings, Log).BuildAsync(_project);
        RefreshTree();
        if (!r.Ok)
        {
            SetStep(1, StepState.Failed, "gagal — lihat Keluaran");
            ShowError("Bangun gagal:\n" + Clip(r.Message, 900));
            return null;
        }
        var warn = r.Warnings.Count > 0 ? $" · {r.Warnings.Count} peringatan" : "";
        SetStep(1, StepState.Done, string.Join(", ", r.Files.Select(f => Path.GetFileName(f.LocalPath))) + warn);
        Status($"Dibangun: {r.Message}{warn}", r.Warnings.Count > 0 ? Tone.Busy : Tone.Ok);
        return r;
    }

    DeviceDeployer Deployer()
    {
        var d = _settings.Device;
        if (_deployer is null || _deployer.Device.Host != d.Host || _deployer.Device.Port != d.Port || _deployer.Device.Code != d.Code)
        {
            _deployer?.Dispose();
            _deployer = new DeviceDeployer(d.Clone(), Log);
        }
        return _deployer;
    }

    async Task<bool> DeployStep(BuildResult r)
    {
        SetStep(2, StepState.Active, $"menyapa {_settings.Device.Host}…");
        UploadProgress.Value = 0;
        UploadProgress.IsVisible = true;
        var dep = Deployer();
        if (dep.Info is null) { var info = await dep.ConnectAsync(); ShowDeviceInfo(info); }
        var progress = new Progress<(string File, double Fraction)>(x =>
        {
            UploadProgress.Value = x.Fraction;
            Step2Text.Text = $"{x.File} · {x.Fraction:P0}";
        });
        Status("Mengirim ke perangkat…", Tone.Busy);
        await dep.UploadAsync(r.Files, progress);
        UploadProgress.IsVisible = false;
        SetStep(2, StepState.Done, string.Join(", ", r.Files.Select(f => f.RemotePath)));
        Status("Terkirim ke perangkat", Tone.Ok);
        return true;
    }

    async Task RunStep()
    {
        var cmd = _project!.RunCommand(ArgsBox.Text ?? "");
        SetStep(3, StepState.Active, cmd);
        BottomTabs.SelectedIndex = 1;
        var pid = await Deployer().RunAsync(cmd);
        _pid = pid;
        StopButton.IsEnabled = pid != 0;
        SetStep(3, StepState.Done, pid == 0 ? $"{cmd} · selesai" : $"{cmd} · pid {pid}");
        DeviceDot.Fill = pid != 0 ? PollenBrush : LeafBrush;
        Status(pid == 0 ? $"Dijalankan: {cmd}" : $"Berjalan di perangkat (pid {pid})", Tone.Ok);
        if (FollowLog.IsChecked == true) _logTimer.Start();
        await PollDeviceLog();
    }

    async Task Flow(bool build, bool deploy, bool run)
    {
        if (_busy || !RequireProject()) return;
        _busy = true;
        SetFlowEnabled(false);
        DeviceErrorCard.IsVisible = false;
        if (deploy || run) ShowSide(device: true);
        try
        {
            var r = await BuildStep();
            if (r is null || !deploy) return;
            await DeployStep(r);
            if (run) await RunStep();
        }
        catch (BzdpTimeoutException ex) { DeviceUnreachable(ex.Message); }
        catch (BzdpException ex) { MarkActiveFailed(); ShowError($"Perangkat menolak: {ex.Message}"); }
        catch (Exception ex) { MarkActiveFailed(); ShowError(ex.Message); }
        finally { _busy = false; SetFlowEnabled(true); UploadProgress.IsVisible = false; }
    }

    void MarkActiveFailed()
    {
        foreach (var (dot, n) in new[] { (Step1Dot, 1), (Step2Dot, 2), (Step3Dot, 3) })
            if (dot.Fill == PollenBrush) SetStep(n, StepState.Failed);
    }

    void DeviceUnreachable(string message)
    {
        MarkActiveFailed();
        DeviceDot.Fill = HibiscusBrush;
        TagSubtitle.Text = "tidak menjawab";
        _deployer?.Dispose(); _deployer = null;
        ShowError($"Perangkat tidak terjangkau di {_settings.Device.Host}:{_settings.Device.Port}. Sudahkah `deploy on` dijalankan di perangkat, " +
                  $"dan port UDP {_settings.Device.Port} diteruskan (QEMU: hostfwd=udp::{_settings.Device.Port}-:{_settings.Device.Port})?");
        Log("[device] " + message);
    }

    void SetFlowEnabled(bool on)
    {
        FlowButton.IsEnabled = on;
        RunTopButton.IsEnabled = on;
        HelloButton.IsEnabled = on;
    }

    async void OnBuild(object? sender, RoutedEventArgs e) => await Flow(true, false, false);
    async void OnDeploy(object? sender, RoutedEventArgs e) => await Flow(true, true, false);
    async void OnBuildDeployRun(object? sender, RoutedEventArgs e) => await Flow(true, true, true);

    async void OnSelfTest(object? sender, RoutedEventArgs e)
    {
        if (!RequireProject()) return;
        if (_project!.Kind != AppKind.Managed) { Status("Uji mandiri di PC hanya untuk proyek managed (--selftest)."); return; }
        var b = new ProjectBuilder(_settings, Log);
        var r = await BuildStep();
        if (r is null) return;
        Status("Menjalankan --selftest di PC…", Tone.Busy);
        BottomTabs.SelectedIndex = 0;
        var (ok, output) = await b.SelfTestManagedAsync(_project);
        Log(output);
        Status(ok ? "Uji mandiri lulus" : "Uji mandiri GAGAL — lihat Keluaran", ok ? Tone.Ok : Tone.Error);
    }

    async void OnStop(object? sender, RoutedEventArgs e)
    {
        if (_pid == 0 || _deployer is null) { Status("Tidak ada aplikasi yang berjalan di perangkat."); return; }
        try
        {
            await _deployer.KillAsync(_pid);
            SetStep(3, StepState.Idle, $"pid {_pid} dihentikan");
            _pid = 0;
            StopButton.IsEnabled = false;
            DeviceDot.Fill = LeafBrush;
            await PollDeviceLog();
        }
        catch (BzdpTimeoutException ex) { DeviceUnreachable(ex.Message); }
        catch (Exception ex) { ShowError(ex.Message); }
    }

    async void OnHello(object? sender, RoutedEventArgs e)
    {
        DeviceErrorCard.IsVisible = false;
        TagSubtitle.Text = "menyapa…";
        HelloButton.IsEnabled = false;
        try
        {
            _deployer?.Dispose(); _deployer = null;
            var info = await Deployer().ConnectAsync();
            ShowDeviceInfo(info);
            Status($"Perangkat menjawab: {info.Text}", Tone.Ok);
        }
        catch (BzdpTimeoutException ex) { DeviceUnreachable(ex.Message); }
        catch (BzdpException ex) { TagSubtitle.Text = "menolak"; ShowError(ex.Message); }
        catch (Exception ex) { TagSubtitle.Text = "galat"; ShowError(ex.Message); }
        finally { HelloButton.IsEnabled = true; }
    }

    void ShowDeviceInfo(BzdpDeviceInfo info)
    {
        TagSpecies.Text = $"Buitenzorg OS · {info.Arch}";
        TagSubtitle.Text = $"{info.Hostname}, menjawab {DateTime.Now:HH:mm}";
        TagChunk.Text = $"{info.MaxChunk} byte";
        DeviceDot.Fill = LeafBrush;
        DevicePillText.Text = $"{_settings.Device.Name} · {info.Arch}";
        if (_project?.Kind == AppKind.Native)
        {
            bool arm = info.Arch.Contains("arm", StringComparison.OrdinalIgnoreCase) || info.Arch.Contains("aarch64");
            bool armBuild = _settings.NativeArch.Equals("arm64", StringComparison.OrdinalIgnoreCase);
            if (arm != armBuild && !info.Arch.Contains("riscv"))
                ShowError($"Perangkat ini {info.Arch}, tetapi native dibangun untuk {_settings.NativeArch}. Ubah 'Arsitektur native' di Pengaturan → Perangkat.");
            else if (info.Arch.Contains("riscv"))
                ShowError("Perangkat RISC-V: C# native (bflat) tidak punya target RISC-V. Gunakan proyek managed atau skrip.");
        }
    }

    async void OnListDisk(object? sender, RoutedEventArgs e)
    {
        try
        {
            var names = await Deployer().ListAsync("/disk");
            DeviceLogBox.Text += $"--- /disk ({names.Count}) ---\n{string.Join("  ", names)}\n";
            BottomTabs.SelectedIndex = 1;
        }
        catch (BzdpTimeoutException ex) { DeviceUnreachable(ex.Message); }
        catch (Exception ex) { ShowError(ex.Message); }
    }

    async Task PollDeviceLog()
    {
        if (_polling || _deployer is null) return;
        _polling = true;
        try
        {
            var text = await _deployer.PollLogAsync();
            if (text.Length > 0)
            {
                DeviceLogBox.Text += text.Replace("\r\n", "\n");
                if ((DeviceLogBox.Text?.Length ?? 0) > 200_000) DeviceLogBox.Text = DeviceLogBox.Text![^150_000..];
                DeviceLogBox.CaretIndex = DeviceLogBox.Text?.Length ?? 0;
            }
            if (FollowLog.IsChecked != true) _logTimer.Stop();
        }
        catch (BzdpTimeoutException) { _logTimer.Stop(); DeviceDot.Fill = HibiscusBrush; Status("Konsol perangkat berhenti: perangkat tidak menjawab", Tone.Error); }
        catch (Exception ex) { _logTimer.Stop(); Log($"[device] log: {ex.Message}"); }
        finally { _polling = false; }
    }

    void OnClearDeviceLog(object? sender, RoutedEventArgs e) => DeviceLogBox.Text = "";

    async void OnBuildImage(object? sender, RoutedEventArgs e)
    {
        Status("Membangun image OS… (scripts/build.ps1)", Tone.Busy);
        BottomTabs.SelectedIndex = 0;
        var plugin = new Ai.BuitenzorgPlugin(_settings, Log);
        var tail = await plugin.BuildApp();
        Status(tail.StartsWith("error") ? tail : "Image OS selesai dibangun (lihat Keluaran)", tail.StartsWith("error") ? Tone.Error : Tone.Ok);
    }

    // ---- devices ------------------------------------------------------------
    void RefreshDevices()
    {
        var active = _settings.Device;
        DeviceBox.ItemsSource = _settings.Devices.Select(d => $"{d.Name}  ·  {d.Host}:{d.Port}").ToList();
        DeviceBox.SelectedIndex = _settings.Devices.IndexOf(active);
        ShowDeviceProfile();
    }

    void ShowDeviceProfile()
    {
        var d = _settings.Device;
        TagHost.Text = $"{d.Host}:{d.Port}";
        TagCode.Text = d.CodeText;
        TagChunk.Text = "—";
        TagSpecies.Text = "Buitenzorg OS";
        TagSubtitle.Text = "belum disapa";
        DevicePillText.Text = d.Name;
        DeviceDot.Fill = MossBrush;
    }

    void OnDeviceChanged(object? sender, SelectionChangedEventArgs e)
    {
        if (!_ready || DeviceBox.SelectedIndex < 0 || DeviceBox.SelectedIndex >= _settings.Devices.Count) return;
        _settings.ActiveDevice = _settings.Devices[DeviceBox.SelectedIndex].Name;
        _settings.Save();
        _deployer?.Dispose(); _deployer = null;
        _logTimer.Stop();
        _pid = 0; StopButton.IsEnabled = false;
        DeviceErrorCard.IsVisible = false;
        ShowDeviceProfile();
    }

    void OnLibsChanged(object? sender, RoutedEventArgs e)
    {
        _settings.DeployRuntimeLibraries = LibsCheck.IsChecked == true;
        _settings.Save();
    }

    async void OnManageDevices(object? sender, RoutedEventArgs e) => await OpenSettings(2);
    void OnShowDevice(object? sender, RoutedEventArgs e) => ShowSide(device: true);

    // ---- side panel / views -------------------------------------------------
    void ShowSide(bool device)
    {
        RailFiles.IsChecked = !device;
        RailDevice.IsChecked = device;
        ExplorerPanel.IsVisible = !device;
        DevicePanel.IsVisible = device;
        SetSideOpen(true);
    }

    void SetSideOpen(bool open)
    {
        BodyGrid.ColumnDefinitions[1].Width = open ? new GridLength(Math.Max(240, _sideWidth)) : new GridLength(0);
        SideSplitter.IsVisible = open;
    }

    double _sideWidth = 272;
    bool SideOpen => BodyGrid.ColumnDefinitions[1].Width.Value > 0;

    void OnRailFiles(object? sender, RoutedEventArgs e) => ToggleRail(device: false);
    void OnRailDevice(object? sender, RoutedEventArgs e) => ToggleRail(device: true);

    void ToggleRail(bool device)
    {
        bool showingThis = SideOpen && DevicePanel.IsVisible == device;
        if (showingThis)
        {
            _sideWidth = BodyGrid.ColumnDefinitions[1].Width.Value;
            SetSideOpen(false);
            RailFiles.IsChecked = false;
            RailDevice.IsChecked = false;
        }
        else ShowSide(device);
    }

    void OnToggleSide(object? sender, RoutedEventArgs e)
    {
        if (SideOpen) { _sideWidth = BodyGrid.ColumnDefinitions[1].Width.Value; SetSideOpen(false); RailFiles.IsChecked = RailDevice.IsChecked = false; }
        else ShowSide(DevicePanel.IsVisible);
    }

    double _chatWidth = 384;
    void OnToggleChat(object? sender, RoutedEventArgs e)
    {
        var col = BodyGrid.ColumnDefinitions[5];
        if (col.Width.Value > 0) { _chatWidth = col.Width.Value; col.Width = new GridLength(0); ChatPanel.IsVisible = false; ChatSplitter.IsVisible = false; }
        else { col.Width = new GridLength(_chatWidth); ChatPanel.IsVisible = true; ChatSplitter.IsVisible = true; }
    }

    double _logHeight = 200;
    void OnToggleLogs(object? sender, RoutedEventArgs e)
    {
        var row = CenterGrid.RowDefinitions[3];
        if (row.Height.Value > 0) { _logHeight = row.Height.Value; row.Height = new GridLength(0); BottomTabs.IsVisible = false; }
        else { row.Height = new GridLength(_logHeight); BottomTabs.IsVisible = true; }
    }

    void OnToggleLineNumbers(object? sender, RoutedEventArgs e)
    {
        Editor.ShowLineNumbers = !Editor.ShowLineNumbers;
        _settings.ShowLineNumbers = Editor.ShowLineNumbers;
        _settings.Save();
    }

    async void OnSettings(object? sender, RoutedEventArgs e) => await OpenSettings(0);

    async Task OpenSettings(int tab)
    {
        RailSettings.IsChecked = false;
        var dlg = new SettingsWindow(_settings, tab);
        await dlg.ShowDialog(this);
        _ready = false;
        ProviderBox.SelectedIndex = (int)_settings.ActiveProvider;
        RefreshDevices();
        LibsCheck.IsChecked = _settings.DeployRuntimeLibraries;
        _ready = true;
        RefreshProviderText();
        _ai.Rebuild(Log);
        _deployer?.Dispose(); _deployer = null;
        Log("Pengaturan disimpan.");
    }

    async Task<string?> PickFolder(string title)
    {
        var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions { Title = title, AllowMultiple = false });
        return folders.FirstOrDefault()?.TryGetLocalPath();
    }

    static string Clip(string s, int n) => s.Length > n ? s[..n] + "…" : s;

    // ---- provider -----------------------------------------------------------
    void RefreshProviderText()
    {
        var m = _settings.Active.Model;
        bool keyOk = !ModelCatalog.For(_settings.ActiveProvider).NeedsKey || !string.IsNullOrWhiteSpace(_settings.Active.ApiKey);
        ModelText.Text = keyOk ? m : $"{m} · kunci API belum diisi";
        ModelText.Foreground = keyOk ? MossBrush : PollenBrush;
        ProviderStatus.Text = $"{ModelCatalog.For(_settings.ActiveProvider).DisplayName} · {m}";
    }

    void OnProviderChanged(object? sender, SelectionChangedEventArgs e)
    {
        if (!_ready || ProviderBox.SelectedIndex < 0) return;
        _settings.ActiveProvider = (Provider)ProviderBox.SelectedIndex;
        _settings.Save();
        _ai.Rebuild(Log);
        RefreshProviderText();
        Log($"Penyedia AI: {_settings.ActiveProvider} ({_settings.Active.Model})");
    }

    // ---- chat ---------------------------------------------------------------
    StackPanel? _currentTurn;

    static readonly (string Label, string Prompt)[] Suggestions =
    {
        ("Aplikasi catatan berjendela (C# managed)", "Buatkan aplikasi catatan sederhana berjendela dengan Buitenzorg.UI (C# managed): daftar catatan, kotak teks, tombol simpan ke Files.ScratchDirectory, plus --selftest."),
        ("Skrip Python: bilangan prima & FizzBuzz", "Tulis skrip Python untuk interpreter Buitenzorg yang mencetak bilangan prima di bawah 100 lalu FizzBuzz 1..30."),
        ("Kedipkan LED Raspberry Pi", "Buat aplikasi C# managed yang mengedipkan LED ACT Raspberry Pi 5 kali dan membaca suhu papan, lalu jalankan di perangkat saya."),
    };

    void ShowWelcome()
    {
        ChatList.Children.Clear();
        var intro = new StackPanel { Spacing = 10, Margin = new Thickness(2, 6, 2, 0) };
        intro.Children.Add(new TextBlock
        {
            Text = "Halo, saya Jack. Ceritakan aplikasi Buitenzorg yang Anda mau — saya tulis kodenya, membangunnya, lalu menjalankannya di perangkat Anda.",
            TextWrapping = TextWrapping.Wrap, FontSize = 13, LineHeight = 19, Foreground = new SolidColorBrush(Color.Parse("#E4EFE8")),
        });
        intro.Children.Add(new TextBlock { Text = "COBA SALAH SATU", Classes = { "eyebrow" }, Margin = new Thickness(0, 6, 0, 0) });
        foreach (var (label, prompt) in Suggestions)
        {
            var b = new Button
            {
                Content = new TextBlock { Text = label, TextWrapping = TextWrapping.Wrap },
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
                Padding = new Thickness(12, 8),
            };
            b.Click += (_, _) => { InputBox.Text = prompt; InputBox.Focus(); InputBox.CaretIndex = prompt.Length; };
            intro.Children.Add(b);
        }
        ChatList.Children.Add(intro);
    }

    void AddSystemNote(string text)
    {
        ChatList.Children.Add(new TextBlock
        {
            Text = text, TextWrapping = TextWrapping.Wrap, FontSize = 12, Foreground = new SolidColorBrush(Color.Parse("#9DB5A8")),
            Margin = new Thickness(4, 2), FontStyle = FontStyle.Italic,
        });
        ScrollChat();
    }

    void ScrollChat() => Dispatcher.UIThread.Post(() => ChatScroll.ScrollToEnd(), DispatcherPriority.Background);

    Border UserBubble(string text) => new()
    {
        Background = new SolidColorBrush(Color.Parse("#1F3F33")),
        CornerRadius = new CornerRadius(12, 12, 3, 12),
        Padding = new Thickness(12, 8),
        HorizontalAlignment = HorizontalAlignment.Right,
        MaxWidth = 310,
        Child = new SelectableTextBlock { Text = text, TextWrapping = TextWrapping.Wrap, FontSize = 13, LineHeight = 19 },
    };

    void OnToolCalled(string name) => Dispatcher.UIThread.Post(() =>
    {
        if (_currentTurn is null) return;
        var label = name switch
        {
            "GetApiReference" => "membaca referensi API",
            "ListTemplates" or "GetTemplateSource" => "melihat templat",
            "ScaffoldProject" => "membuat proyek",
            "WriteFile" => "menulis berkas",
            "ReadFile" => "membaca berkas",
            "CompileCheck" or "BuildProject" => "membangun",
            "DeployToDevice" => "mengirim ke perangkat",
            "ReadDeviceLog" => "membaca konsol perangkat",
            "SearchInternet" or "ScrapeWebPage" => "mencari di web",
            _ => name,
        };
        var chip = new Border { Classes = { "chip", "pollen" }, Margin = new Thickness(0, 0, 4, 4), HorizontalAlignment = HorizontalAlignment.Left, Child = new TextBlock { Text = "◆ " + label } };
        ToolTip.SetTip(chip, name);
        // chips go into the wrap panel at the top of the turn
        if (_currentTurn.Children[0] is WrapPanel wp) wp.Children.Add(chip);
        Status($"Jack sedang {label}…", Tone.Busy);
        if (name is "WriteFile" or "ScaffoldProject" or "BuildProject") _refreshAfterTurn = true;
        ScrollChat();
    });

    bool _refreshAfterTurn;

    async void OnSend(object? sender, RoutedEventArgs e)
    {
        if (_chatCts != null) { _chatCts.Cancel(); return; } // the button doubles as "stop" while Jack answers
        var text = InputBox.Text?.Trim();
        if (string.IsNullOrEmpty(text)) return;
        await SaveIfNeeded();
        InputBox.Text = "";
        if (ChatList.Children.Count == 1 && ChatList.Children[0] is StackPanel { Children.Count: > 2 }) ChatList.Children.Clear();

        ChatList.Children.Add(UserBubble(_attachBytes is { Length: > 0 } ? $"{text}\n[+ gambar]" : text));
        _currentTurn = new StackPanel { Spacing = 6, HorizontalAlignment = HorizontalAlignment.Stretch };
        _currentTurn.Children.Add(new WrapPanel { Orientation = Orientation.Horizontal });
        var live = new SelectableTextBlock { TextWrapping = TextWrapping.Wrap, FontSize = 13, LineHeight = 19 };
        _currentTurn.Children.Add(live);
        ChatList.Children.Add(_currentTurn);
        ScrollChat();

        var img = _attachBytes; var mime = _attachMime;
        _attachBytes = null; _attachMime = null; AttachInfo.IsVisible = false;

        // Give Jack the working context without cluttering the bubble.
        var context = _project != null
            ? $"\n\n[Konteks MagicAppGen: proyek aktif = \"{_project.Dir}\" ({_project.KindLabel}, berkas perangkat {_project.RemotePath}); " +
              $"berkas terbuka = \"{_currentFile}\"; perangkat = {_settings.Device.Name} {_settings.Device.Host}:{_settings.Device.Port}. " +
              "Tulis perubahan ke folder proyek ini kecuali diminta lain.]"
            : $"\n\n[Konteks MagicAppGen: belum ada proyek terbuka. Buat proyek baru di \"{DefaultProjectsDir()}\\<NamaApp>\" " +
              "dengan ScaffoldProject.]";

        _chatCts = new CancellationTokenSource();
        SendText.Text = "Berhenti";
        Status("Jack sedang berpikir…", Tone.Busy);
        _refreshAfterTurn = false;
        var full = new System.Text.StringBuilder();
        try
        {
            await foreach (var token in _ai.AskAsync(text + context, img, mime, _chatCts.Token))
            {
                full.Append(token);
                live.Text = full.ToString();
                ScrollChat();
            }
            Status("Siap", Tone.Ok);
        }
        catch (OperationCanceledException) { full.Append("\n\n(dihentikan)"); Status("Jawaban dihentikan"); }
        catch (Exception ex)
        {
            full.Append($"\n\n[galat: {ex.Message}]");
            Log($"[ai] {ex.Message}");
            Status("Jack gagal menjawab — periksa kunci API dan model di Pengaturan", Tone.Error);
        }
        finally
        {
            _chatCts.Dispose();
            _chatCts = null;
            SendText.Text = "Kirim";
        }
        _currentTurn.Children.Remove(live);
        foreach (var block in RenderReply(full.ToString())) _currentTurn.Children.Add(block);
        _currentTurn = null;
        ScrollChat();
        await AfterJackTurn();
    }

    /// <summary>Jack may have written files: pick up a new project folder, refresh the tree and reload the open file.</summary>
    async Task AfterJackTurn()
    {
        if (!_refreshAfterTurn) return;
        if (_projectDir is null)
        {
            var root = DefaultProjectsDir();
            if (Directory.Exists(root))
            {
                var newest = new DirectoryInfo(root).GetDirectories().OrderByDescending(d => d.LastWriteTimeUtc).FirstOrDefault();
                if (newest != null && (DateTime.UtcNow - newest.LastWriteTimeUtc).TotalMinutes < 10 && AppProject.Open(newest.FullName) != null)
                {
                    await OpenProject(newest.FullName);
                    return;
                }
            }
        }
        RefreshTree();
        _project = _projectDir != null ? AppProject.Open(_projectDir) ?? _project : _project;
        UpdateProjectUi();
        if (_currentFile != null && File.Exists(_currentFile) && !_dirty)
        {
            var disk = await File.ReadAllTextAsync(_currentFile);
            if (disk != Editor.Text) await OpenFile(_currentFile);
        }
    }

    static string DefaultProjectsDir() =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Buitenzorg Apps");

    /// <summary>Split a reply into prose and fenced code blocks; code gets copy / insert actions.</summary>
    IEnumerable<Control> RenderReply(string text)
    {
        var parts = Regex.Split(text, "```");
        for (int i = 0; i < parts.Length; i++)
        {
            var part = parts[i];
            if (i % 2 == 0)
            {
                var prose = part.Trim();
                if (prose.Length > 0)
                    yield return new SelectableTextBlock { Text = prose, TextWrapping = TextWrapping.Wrap, FontSize = 13, LineHeight = 19 };
                continue;
            }
            var nl = part.IndexOf('\n');
            var lang = nl > 0 ? part[..nl].Trim() : "";
            var code = (nl >= 0 ? part[(nl + 1)..] : part).TrimEnd();
            yield return CodeBlock(lang, code);
        }
    }

    Control CodeBlock(string lang, string code)
    {
        var copy = new Button { Classes = { "ghost" }, Content = "Salin", Padding = new Thickness(6, 2), FontSize = 11.5 };
        copy.Click += async (_, _) => { if (Clipboard != null) await Clipboard.SetTextAsync(code); Status("Kode disalin"); };
        var insert = new Button { Classes = { "ghost" }, Content = "Sisipkan", Padding = new Thickness(6, 2), FontSize = 11.5 };
        ToolTip.SetTip(insert, "Sisipkan di posisi kursor editor");
        insert.Click += (_, _) => { Editor.Document.Insert(Editor.CaretOffset, code); Editor.Focus(); };
        var head = new DockPanel { Margin = new Thickness(10, 4, 4, 0) };
        var buttons = new StackPanel { Orientation = Orientation.Horizontal };
        buttons.Children.Add(copy);
        buttons.Children.Add(insert);
        DockPanel.SetDock(buttons, Dock.Right);
        head.Children.Add(buttons);
        head.Children.Add(new TextBlock { Text = lang.Length > 0 ? lang : "kode", Classes = { "eyebrow" }, VerticalAlignment = VerticalAlignment.Center });
        var body = new SelectableTextBlock
        {
            Text = code, FontFamily = (FontFamily)Application.Current!.FindResource("CodeFont")!, FontSize = 12,
            TextWrapping = TextWrapping.Wrap, Margin = new Thickness(10, 2, 10, 10),
        };
        var stack = new StackPanel();
        stack.Children.Add(head);
        stack.Children.Add(new ScrollViewer { Content = body, HorizontalScrollBarVisibility = Avalonia.Controls.Primitives.ScrollBarVisibility.Disabled, MaxHeight = 320 });
        return new Border
        {
            Background = new SolidColorBrush(Color.Parse("#0E1A14")),
            BorderBrush = VeinBrush, BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(8),
            Child = stack,
        };
    }

    void OnInputKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && e.KeyModifiers.HasFlag(KeyModifiers.Control))
        {
            e.Handled = true;
            OnSend(sender, e);
        }
    }

    void OnClearChat(object? sender, RoutedEventArgs e)
    {
        _chatCts?.Cancel();
        _ai.Reset();
        ShowWelcome();
    }

    async void OnAttach(object? sender, RoutedEventArgs e)
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Lampirkan gambar",
            AllowMultiple = false,
            FileTypeFilter = new[] { new FilePickerFileType("Gambar") { Patterns = new[] { "*.png", "*.jpg", "*.jpeg", "*.bmp", "*.gif" } } },
        });
        var f = files.FirstOrDefault();
        if (f is null) return;
        await using var s = await f.OpenReadAsync();
        using var ms = new MemoryStream();
        await s.CopyToAsync(ms);
        _attachBytes = ms.ToArray();
        _attachMime = f.Name.EndsWith(".jpg", StringComparison.OrdinalIgnoreCase) || f.Name.EndsWith(".jpeg", StringComparison.OrdinalIgnoreCase)
            ? "image/jpeg" : "image/png";
        AttachInfo.Text = $"+ {f.Name}";
        AttachInfo.IsVisible = true;
    }

    protected override void OnClosed(EventArgs e)
    {
        _logTimer.Stop();
        _deployer?.Dispose();
        base.OnClosed(e);
    }

    // ---- screenshot support ---------------------------------------------------
    /// <summary>Fill the window with a realistic session for headless screenshots.</summary>
    internal async Task StageForScreenshot(string projectDir)
    {
        await OpenProject(projectDir);
        ShowSide(device: true);
        TagSpecies.Text = "Buitenzorg OS · x86_64";
        TagSubtitle.Text = "buitenzorg-qemu, menjawab 10:42";
        TagChunk.Text = "1024 byte";
        DeviceDot.Fill = PollenBrush;
        DevicePillText.Text = $"{_settings.Device.Name} · x86_64";
        SetStep(1, StepState.Done, $"{_project!.Name83}.DLL · 18 KB");
        SetStep(2, StepState.Done, _project.RemotePath);
        SetStep(3, StepState.Active, _project.RunCommand() + " · pid 37");
        StopButton.IsEnabled = true;
        BottomTabs.SelectedIndex = 1;
        DeviceLogBox.Text = "$ run catatan\n[clr] CATATAN.DLL dimuat (BZUI.DLL, BZAPP.DLL)\n[app] catatan berjalan sebagai pid 37\nCatatan: 3 catatan di /disk\n";
        ChatList.Children.Clear();
        ChatList.Children.Add(UserBubble("Buatkan aplikasi catatan berjendela, simpan ke /disk."));
        var turn = new StackPanel { Spacing = 6 };
        var chips = new WrapPanel();
        foreach (var l in new[] { "membaca referensi API", "membuat proyek", "menulis berkas", "membangun" })
            chips.Children.Add(new Border { Classes = { "chip", "pollen" }, Margin = new Thickness(0, 0, 4, 4), Child = new TextBlock { Text = "◆ " + l } });
        turn.Children.Add(chips);
        foreach (var c in RenderReply("Selesai. Catatan adalah aplikasi C# managed (Buitenzorg.UI): daftar di kiri, editor di kanan, Ctrl+S menyimpan ke /disk. Bangun + uji mandiri lulus (4 cek).\n```csharp\nvar save = new Button(new TextBlock(\"Simpan\"), _ => vm.Save()).WithClass(\"accent\");\n```\nTekan F5 untuk menjalankannya di QEMU lokal."))
            turn.Children.Add(c);
        ChatList.Children.Add(turn);
        Status("Berjalan di perangkat (pid 37)", Tone.Ok);
    }
}
