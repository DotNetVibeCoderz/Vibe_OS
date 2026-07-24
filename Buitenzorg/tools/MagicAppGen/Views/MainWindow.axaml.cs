using System;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using Avalonia.Threading;
using MagicAppGen.Models;
using MagicAppGen.Services;

namespace MagicAppGen.Views;

public partial class MainWindow : Window
{
    readonly Settings _settings;
    AiService _ai;
    string? _currentFile;
    string? _projectDir;
    byte[]? _attachBytes;
    string? _attachMime;
    CancellationTokenSource? _cts;
    // Setting SelectedIndex below raises SelectionChanged synchronously; the
    // handlers must not run until the window is fully constructed.
    bool _ready;

    public MainWindow(Settings settings)
    {
        _settings = settings;
        InitializeComponent();

        _ai = new AiService(_settings, Log);

        Editor.ShowLineNumbers = _settings.ShowLineNumbers;
        LineNumbersToggle.IsChecked = _settings.ShowLineNumbers;
        LanguageBox.SelectedIndex = LanguageIndex(_settings.Language);
        ProviderBox.SelectedIndex = (int)_settings.ActiveProvider;
        ModelText.Text = _settings.Active.Model;
        ProviderStatus.Text = $"{_settings.ActiveProvider} · {_settings.Active.Model}";

        _ready = true;
        AddBubble("system", "Hi, I'm Jack — The Code Bender. Describe the Buitenzorg app you want and I'll write it. I can also search the web, build, and run.");
        Log("MagicAppGen ready.");
    }

    // ---- logging & status ---------------------------------------------------
    void Log(string line) => Dispatcher.UIThread.Post(() =>
    {
        LogBox.Text += line + "\n";
        LogBox.CaretIndex = LogBox.Text?.Length ?? 0;
    });

    void Status(string s) => Dispatcher.UIThread.Post(() => StatusText.Text = s);

    static int LanguageIndex(string lang) => lang.ToLowerInvariant() switch
    {
        "javascript" or "js" => 1,
        "typescript" or "ts" => 2,
        "python" or "py" => 3,
        _ => 0,
    };

    // ---- chat ---------------------------------------------------------------
    TextBlock AddBubble(string role, string text)
    {
        var (bg, fg, align) = role switch
        {
            "user" => (Color.Parse("#2A4D6E"), Colors.White, HorizontalAlignment.Right),
            "assistant" => (Color.Parse("#2C2F3A"), Color.Parse("#E6E6E6"), HorizontalAlignment.Left),
            _ => (Color.Parse("#1F2733"), Color.Parse("#9AD0FF"), HorizontalAlignment.Left),
        };
        var tb = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap, Foreground = new SolidColorBrush(fg) };
        var border = new Border
        {
            Background = new SolidColorBrush(bg),
            CornerRadius = new Avalonia.CornerRadius(8),
            Padding = new Avalonia.Thickness(10, 6),
            Margin = new Avalonia.Thickness(0, 2),
            HorizontalAlignment = align,
            MaxWidth = 320,
            Child = tb,
        };
        ChatList.Children.Add(border);
        Dispatcher.UIThread.Post(() => ChatScroll.ScrollToEnd(), DispatcherPriority.Background);
        return tb;
    }

    async void OnSend(object? sender, RoutedEventArgs e)
    {
        var text = InputBox.Text?.Trim();
        if (string.IsNullOrEmpty(text)) return;
        InputBox.Text = "";
        SendButton.IsEnabled = false;
        Status("Jack is thinking…");

        var shown = _attachBytes is { Length: > 0 } ? $"{text}  [+image]" : text;
        AddBubble("user", shown);
        var reply = AddBubble("assistant", "");
        var img = _attachBytes; var mime = _attachMime;
        _attachBytes = null; _attachMime = null; AttachInfo.Text = "";

        _cts = new CancellationTokenSource();
        try
        {
            await foreach (var token in _ai.AskAsync(text, img, mime, _cts.Token))
            {
                reply.Text += token;
                ChatScroll.ScrollToEnd();
            }
        }
        catch (Exception ex)
        {
            reply.Text += $"\n[error: {ex.Message}]";
            Log($"[ai] {ex.Message}");
        }
        SendButton.IsEnabled = true;
        Status("Ready");
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
        ChatList.Children.Clear();
        _ai.Reset();
        AddBubble("system", "Chat cleared.");
    }

    async void OnAttach(object? sender, RoutedEventArgs e)
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = "Attach image",
            AllowMultiple = false,
            FileTypeFilter = new[] { new FilePickerFileType("Images") { Patterns = new[] { "*.png", "*.jpg", "*.jpeg", "*.bmp", "*.gif" } } },
        });
        var f = files.FirstOrDefault();
        if (f is null) return;
        await using var s = await f.OpenReadAsync();
        using var ms = new MemoryStream();
        await s.CopyToAsync(ms);
        _attachBytes = ms.ToArray();
        _attachMime = f.Name.EndsWith(".jpg", StringComparison.OrdinalIgnoreCase) || f.Name.EndsWith(".jpeg", StringComparison.OrdinalIgnoreCase)
            ? "image/jpeg" : "image/png";
        AttachInfo.Text = $"Attached: {f.Name}";
    }

    // ---- provider / language ------------------------------------------------
    void OnProviderChanged(object? sender, SelectionChangedEventArgs e)
    {
        if (!_ready || ProviderBox.SelectedIndex < 0) return;
        _settings.ActiveProvider = (Provider)ProviderBox.SelectedIndex;
        ModelText.Text = _settings.Active.Model;
        ProviderStatus.Text = $"{_settings.ActiveProvider} · {_settings.Active.Model}";
        _settings.Save();
        _ai.Rebuild(Log);
        Log($"Provider: {_settings.ActiveProvider} ({_settings.Active.Model})");
    }

    void OnLanguageChanged(object? sender, SelectionChangedEventArgs e)
    {
        if (!_ready) return;
        if (LanguageBox.SelectedItem is ComboBoxItem it && it.Content is string lang)
        {
            _settings.Language = lang;
            _settings.Save();
        }
    }

    // ---- file / project -----------------------------------------------------
    async void OnNewBlank(object? sender, RoutedEventArgs e)
    {
        var dir = await PickFolder("New blank project folder");
        if (dir is null) return;
        _projectDir = dir;
        _currentFile = Path.Combine(dir, "main.cs");
        Editor.Text = "// New Buitenzorg app (blank). Ask Jack to fill it in.\n";
        Status($"Project: {dir}");
    }

    async void OnNewTemplate(object? sender, RoutedEventArgs e)
    {
        var choice = await new NewProjectDialog().ShowDialog<NewProjectChoice?>(this);
        if (choice is null) return;
        var dir = await PickFolder($"Folder for '{choice.AppName}'");
        if (dir is null) return;

        var main = ProjectTemplates.Scaffold(choice.Template, dir, choice.AppName);
        _projectDir = dir;
        _currentFile = main;
        Editor.Text = await File.ReadAllTextAsync(main);
        LanguageBox.SelectedIndex = LanguageIndex(choice.Template.Language);
        Log($"Scaffolded '{choice.Template.Title}' into {dir}");
        Status($"Project: {dir}");
        AddBubble("system", $"Created a '{choice.Template.Title}' project. Ask me to extend it.");
    }

    async void OnOpenProject(object? sender, RoutedEventArgs e)
    {
        var dir = await PickFolder("Open project folder");
        if (dir is null) return;
        _projectDir = dir;
        Status($"Project: {dir}");
        var first = Directory.EnumerateFiles(dir, "*.cs").FirstOrDefault();
        if (first != null) { _currentFile = first; Editor.Text = await File.ReadAllTextAsync(first); }
    }

    async void OnOpenFile(object? sender, RoutedEventArgs e)
    {
        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions { Title = "Open file", AllowMultiple = false });
        var f = files.FirstOrDefault();
        if (f?.TryGetLocalPath() is not string path) return;
        _currentFile = path;
        Editor.Text = await File.ReadAllTextAsync(path);
        Status($"Opened {Path.GetFileName(path)}");
    }

    async void OnSave(object? sender, RoutedEventArgs e)
    {
        if (_currentFile is null)
        {
            var f = await StorageProvider.SaveFilePickerAsync(new FilePickerSaveOptions { Title = "Save as" });
            if (f?.TryGetLocalPath() is not string p) return;
            _currentFile = p;
        }
        await File.WriteAllTextAsync(_currentFile, Editor.Text);
        Status($"Saved {Path.GetFileName(_currentFile)}");
    }

    void OnCloseProject(object? sender, RoutedEventArgs e)
    {
        _projectDir = null; _currentFile = null; Editor.Text = "";
        Status("Project closed");
    }

    void OnExit(object? sender, RoutedEventArgs e) => Close();

    async void OnGoToLine(object? sender, RoutedEventArgs e)
    {
        var dlg = new GoToLineDialog();
        var line = await dlg.ShowDialog<int?>(this);
        if (line is int n && n >= 1 && n <= Editor.Document.LineCount)
        {
            var l = Editor.Document.GetLineByNumber(n);
            Editor.CaretOffset = l.Offset;
            Editor.ScrollToLine(n);
            Editor.Focus();
        }
    }

    // ---- build / run / deploy ----------------------------------------------
    async void OnBuild(object? sender, RoutedEventArgs e) { Status("Building…"); await SaveIfNeeded(); Log("Build requested — ask Jack to run BuildApp, or run scripts/build.ps1."); }
    async void OnRun(object? sender, RoutedEventArgs e) { Status("Running…"); await SaveIfNeeded(); Log("Run requested — ask Jack to run RunApp, or run scripts/smoke-test.ps1."); }
    void OnDeploy(object? sender, RoutedEventArgs e) { Status("Deploy"); Log("Deploy: use scripts/make-vm-images.ps1 to produce VMware/VirtualBox images."); }

    async Task SaveIfNeeded() { if (_currentFile != null) await File.WriteAllTextAsync(_currentFile, Editor.Text); }

    // ---- view toggles & settings -------------------------------------------
    void OnToggleLineNumbers(object? sender, RoutedEventArgs e)
    {
        Editor.ShowLineNumbers = !Editor.ShowLineNumbers;
        LineNumbersToggle.IsChecked = Editor.ShowLineNumbers;
        _settings.ShowLineNumbers = Editor.ShowLineNumbers;
        _settings.Save();
    }

    void OnToggleChat(object? sender, RoutedEventArgs e) => ChatPanel.IsVisible = !ChatPanel.IsVisible;
    void OnToggleLogs(object? sender, RoutedEventArgs e) => LogBox.IsVisible = !LogBox.IsVisible;

    async void OnSettings(object? sender, RoutedEventArgs e)
    {
        var dlg = new SettingsWindow(_settings);
        await dlg.ShowDialog(this);
        _settings.Save();
        ProviderBox.SelectedIndex = (int)_settings.ActiveProvider;
        ModelText.Text = _settings.Active.Model;
        ProviderStatus.Text = $"{_settings.ActiveProvider} · {_settings.Active.Model}";
        _ai.Rebuild(Log);
        Log("Settings saved.");
    }

    async Task<string?> PickFolder(string title)
    {
        var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions { Title = title, AllowMultiple = false });
        return folders.FirstOrDefault()?.TryGetLocalPath();
    }
}
