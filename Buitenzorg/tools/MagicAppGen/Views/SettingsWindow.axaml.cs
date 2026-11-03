using System;
using System.Collections.Generic;
using System.Configuration;
using System.Globalization;
using System.Linq;
using System.Threading;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using MagicAppGen.Models;
using MagicAppGen.Services;

namespace MagicAppGen.Views;

/// <summary>Edits every value in app.config from the UI ("semua konfigurasi
/// disimpan di app.config dan bisa di ubah di UI"): general, AI providers,
/// devices, tools.</summary>
public partial class SettingsWindow : Window
{
    readonly Settings _settings;
    readonly List<DeviceProfile> _devices;
    readonly Dictionary<Provider, (AutoCompleteBox Model, TextBox Key, TextBox Endpoint, TextBlock Result)> _rows = new();
    bool _syncing;
    TextBox? _claudeWorkspace;

    // Parameterless ctor exists only so the XAML previewer can load the window.
    public SettingsWindow() : this(Settings.Load()) { }

    public SettingsWindow(Settings settings, int tab = 0)
    {
        _settings = settings;
        _devices = settings.Devices.Select(d => d.Clone()).ToList();
        InitializeComponent();
        LoadFrom(settings);
        Tabs.SelectedIndex = tab;
        try { ConfigPath.Text = ConfigurationManager.OpenExeConfiguration(ConfigurationUserLevel.None).FilePath; } catch { }
    }

    void LoadFrom(Settings s)
    {
        RootBox.Text = s.BuitenzorgRoot;
        TempBox.Text = s.Temperature.ToString(CultureInfo.InvariantCulture);
        PromptBox.Text = s.SystemPrompt;
        TavilyBox.Text = s.TavilyApiKey;
        ProviderBox.ItemsSource = Enum.GetValues<Provider>().Select(p => ModelCatalog.For(p).DisplayName).ToList();
        ProviderBox.SelectedIndex = (int)s.ActiveProvider;
        foreach (var p in Enum.GetValues<Provider>()) ProviderRows.Children.Add(ProviderRow(p, s.Profiles[p]));

        DeviceList.ItemsSource = _devices.Select(d => d.Name).ToList();
        DeviceList.SelectedIndex = Math.Max(0, _devices.FindIndex(d => d.Name == s.Device.Name));
        ArchBox.SelectedIndex = s.NativeArch.Equals("arm64", StringComparison.OrdinalIgnoreCase) ? 1 : 0;
        LibsCheck.IsChecked = s.DeployRuntimeLibraries;
    }

    Control ProviderRow(Provider p, LlmProfile prof)
    {
        var cat = ModelCatalog.For(p);
        var model = new AutoCompleteBox
        {
            ItemsSource = cat.Models, Text = prof.Model, MinimumPrefixLength = 0, FilterMode = AutoCompleteFilterMode.Contains,
            Watermark = cat.DefaultModel, FontFamily = (FontFamily)Application.Current!.FindResource("CodeFont")!,
        };
        var key = new TextBox { Text = prof.ApiKey, PasswordChar = '•', Watermark = cat.NeedsKey ? "kunci API" : "tidak perlu", Classes = { "code" } };
        var endpoint = new TextBox { Text = prof.Endpoint, Watermark = cat.EndpointHint, Classes = { "code" }, FontSize = 11.5 };
        var workspace = p == Provider.Claude
            ? new TextBox { Text = prof.Workspace, Watermark = "hanya untuk kunci yang tidak terikat workspace", Classes = { "code" }, FontSize = 11.5 }
            : null;
        if (workspace != null) _claudeWorkspace = workspace;
        var result = new TextBlock { Classes = { "faint" }, TextWrapping = TextWrapping.Wrap, IsVisible = false };
        var test = new Button { Content = "Uji", Padding = new Thickness(10, 4), FontSize = 12 };
        ToolTip.SetTip(test, "Satu permintaan nyata ke model ini (tanpa menyimpan)");
        test.Click += async (_, _) =>
        {
            test.IsEnabled = false;
            result.IsVisible = true;
            result.Foreground = (IBrush)Application.Current!.FindResource("Lichen")!;
            result.Text = "menghubungi…";
            var probe = Settings.Load();
            probe.ActiveProvider = p;
            probe.Profiles[p].Model = model.Text ?? "";
            probe.Profiles[p].ApiKey = key.Text ?? "";
            probe.Profiles[p].Endpoint = endpoint.Text ?? "";
            if (workspace != null) probe.Profiles[p].Workspace = workspace.Text ?? "";
            var t0 = DateTime.UtcNow;
            try
            {
                var ai = new AiService(probe, _ => { });
                var sb = new System.Text.StringBuilder();
                using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(60));
                await foreach (var tok in ai.AskAsync("Reply with exactly: siap", null, null, cts.Token)) sb.Append(tok);
                result.Foreground = (IBrush)Application.Current!.FindResource("Leaf")!;
                result.Text = $"menjawab dalam {(DateTime.UtcNow - t0).TotalSeconds:0.0} s: \"{Clip(sb.ToString().Trim(), 60)}\"";
            }
            catch (Exception ex)
            {
                result.Foreground = (IBrush)Application.Current!.FindResource("Hibiscus")!;
                var msg = ex.Message;
                if (!string.IsNullOrEmpty(key.Text)) msg = msg.Replace(key.Text, "***");
                result.Text = Clip(msg, 240);
            }
            finally { test.IsEnabled = true; }
        };
        _rows[p] = (model, key, endpoint, result);

        var grid = new Grid { ColumnDefinitions = new ColumnDefinitions("*,10,*"), RowDefinitions = new RowDefinitions("Auto,Auto,Auto,Auto,Auto") };
        void Put(Control c, int r, int col, int span = 1) { Grid.SetRow(c, r); Grid.SetColumn(c, col); Grid.SetColumnSpan(c, span); grid.Children.Add(c); }
        var head = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        DockPanel.SetDock(test, Dock.Right);
        head.Children.Add(test);
        head.Children.Add(new TextBlock { Text = cat.DisplayName, Classes = { "title" }, VerticalAlignment = VerticalAlignment.Center });
        Put(head, 0, 0, 3);
        Put(Labeled("MODEL", model), 1, 0);
        Put(Labeled("KUNCI API", key), 1, 2);
        if (workspace != null)
        {
            Put(Labeled("ENDPOINT", endpoint), 2, 0);
            Put(Labeled("WORKSPACE ID", workspace), 2, 2);
        }
        else Put(Labeled("ENDPOINT", endpoint), 2, 0, 3);
        result.Margin = new Thickness(0, 6, 0, 0);
        Put(result, 3, 0, 3);
        return new Border { Classes = { "card" }, Child = grid };
    }

    static Control Labeled(string label, Control c)
    {
        var sp = new StackPanel { Spacing = 4, Margin = new Thickness(0, 0, 0, 6) };
        sp.Children.Add(new TextBlock { Text = label, Classes = { "eyebrow" } });
        sp.Children.Add(c);
        return sp;
    }

    static string Clip(string s, int n) => s.Length > n ? s[..n] + "…" : s;

    // ---- devices ------------------------------------------------------------
    DeviceProfile? CurrentDevice => DeviceList.SelectedIndex >= 0 && DeviceList.SelectedIndex < _devices.Count ? _devices[DeviceList.SelectedIndex] : null;

    void OnDeviceSelected(object? sender, SelectionChangedEventArgs e)
    {
        var d = CurrentDevice;
        if (d is null) return;
        _syncing = true;
        DevName.Text = d.Name;
        DevHost.Text = d.Host;
        DevPort.Text = d.Port.ToString(CultureInfo.InvariantCulture);
        DevCode.Text = d.CodeText;
        _syncing = false;
        DevError.IsVisible = false;
    }

    void OnDeviceEdited(object? sender, TextChangedEventArgs e)
    {
        if (_syncing || CurrentDevice is not DeviceProfile d) return;
        string? err = null;
        d.Name = string.IsNullOrWhiteSpace(DevName.Text) ? d.Name : DevName.Text.Trim();
        d.Host = DevHost.Text?.Trim() ?? "";
        if (int.TryParse(DevPort.Text, out var port) && port is > 0 and < 65536) d.Port = port; else err = "Port harus 1-65535.";
        if (int.TryParse(DevCode.Text, out var code) && code is >= 0 and <= 9999) d.Code = code; else err = "Kode pemasangan: 4 digit (0000-9999).";
        if (string.IsNullOrWhiteSpace(d.Host)) err = "Isi alamat IP atau nama host.";
        DevError.Text = err ?? "";
        DevError.IsVisible = err != null;
        if (sender == DevName)
        {
            int i = DeviceList.SelectedIndex;
            _syncing = true;
            DeviceList.ItemsSource = _devices.Select(x => x.Name).ToList();
            DeviceList.SelectedIndex = i;
            _syncing = false;
        }
    }

    void OnAddDevice(object? sender, RoutedEventArgs e)
    {
        var d = new DeviceProfile { Name = $"Perangkat {_devices.Count + 1}", Host = "192.168.1.50" };
        _devices.Add(d);
        DeviceList.ItemsSource = _devices.Select(x => x.Name).ToList();
        DeviceList.SelectedIndex = _devices.Count - 1;
        DevName.Focus();
    }

    void OnRemoveDevice(object? sender, RoutedEventArgs e)
    {
        if (CurrentDevice is not DeviceProfile d || _devices.Count <= 1) return;
        _devices.Remove(d);
        DeviceList.ItemsSource = _devices.Select(x => x.Name).ToList();
        DeviceList.SelectedIndex = 0;
    }

    // ---- save ---------------------------------------------------------------
    void OnSave(object? sender, RoutedEventArgs e)
    {
        if (ProviderBox.SelectedIndex >= 0) _settings.ActiveProvider = (Provider)ProviderBox.SelectedIndex;
        if (double.TryParse(TempBox.Text, NumberStyles.Float, CultureInfo.InvariantCulture, out var t)) _settings.Temperature = Math.Clamp(t, 0, 2);
        _settings.BuitenzorgRoot = string.IsNullOrWhiteSpace(RootBox.Text) ? Settings.FindRepoRoot() ?? "" : RootBox.Text.Trim();
        _settings.SystemPrompt = PromptBox.Text ?? "";
        _settings.TavilyApiKey = TavilyBox.Text ?? "";
        foreach (var (p, row) in _rows)
        {
            var prof = _settings.Profiles[p];
            prof.Model = string.IsNullOrWhiteSpace(row.Model.Text) ? ModelCatalog.For(p).DefaultModel : row.Model.Text.Trim();
            prof.ApiKey = row.Key.Text?.Trim() ?? "";
            prof.Endpoint = row.Endpoint.Text?.Trim() ?? "";
        }
        if (_claudeWorkspace != null) _settings.Profiles[Provider.Claude].Workspace = _claudeWorkspace.Text?.Trim() ?? "";
        // Keep names unique so the active device can be found again.
        var seen = new HashSet<string>();
        foreach (var d in _devices) { var n = d.Name; int k = 2; while (!seen.Add(d.Name)) d.Name = $"{n} ({k++})"; }
        _settings.Devices = _devices;
        if (CurrentDevice is DeviceProfile cur) _settings.ActiveDevice = cur.Name;
        _settings.NativeArch = ArchBox.SelectedIndex == 1 ? "arm64" : "x64";
        _settings.DeployRuntimeLibraries = LibsCheck.IsChecked == true;
        _settings.Save();
        Close();
    }

    void OnCancel(object? sender, RoutedEventArgs e) => Close();
}
