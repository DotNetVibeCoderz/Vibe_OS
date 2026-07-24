using System;
using System.Globalization;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using MagicAppGen.Models;

namespace MagicAppGen.Views;

/// <summary>Edits every value in app.config from the UI (the user requirement:
/// "semua konfigurasi disimpan di app.config dan bisa di ubah di UI").</summary>
public partial class SettingsWindow : Window
{
    readonly Settings _settings;

    // Parameterless ctor exists only so the XAML previewer can load the window.
    public SettingsWindow() : this(Settings.Load()) { }

    public SettingsWindow(Settings settings)
    {
        _settings = settings;
        InitializeComponent();
        LoadFrom(settings);
    }

    void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    T C<T>(string name) where T : Control => this.FindControl<T>(name)!;

    void LoadFrom(Settings s)
    {
        C<ComboBox>("ProviderBox").SelectedIndex = (int)s.ActiveProvider;
        C<TextBox>("TempBox").Text = s.Temperature.ToString(CultureInfo.InvariantCulture);
        C<TextBox>("LangBox").Text = s.Language;
        C<TextBox>("RootBox").Text = s.BuitenzorgRoot;
        C<TextBox>("PromptBox").Text = s.SystemPrompt;
        C<TextBox>("TavilyBox").Text = s.TavilyApiKey;

        Fill(Provider.OpenAI, "OpenAi");
        Fill(Provider.Claude, "Claude");
        Fill(Provider.Gemini, "Gemini");
        Fill(Provider.Ollama, "Ollama");

        void Fill(Provider p, string prefix)
        {
            var prof = s.Profiles[p];
            C<TextBox>(prefix + "Model").Text = prof.Model;
            C<TextBox>(prefix + "Key").Text = prof.ApiKey;
            C<TextBox>(prefix + "Endpoint").Text = prof.Endpoint;
        }
    }

    void OnSave(object? sender, RoutedEventArgs e)
    {
        var idx = C<ComboBox>("ProviderBox").SelectedIndex;
        if (idx >= 0) _settings.ActiveProvider = (Provider)idx;

        if (double.TryParse(C<TextBox>("TempBox").Text, NumberStyles.Float,
                            CultureInfo.InvariantCulture, out var t))
            _settings.Temperature = t;

        _settings.Language = C<TextBox>("LangBox").Text ?? "C#";
        _settings.BuitenzorgRoot = C<TextBox>("RootBox").Text ?? "";
        _settings.SystemPrompt = C<TextBox>("PromptBox").Text ?? "";
        _settings.TavilyApiKey = C<TextBox>("TavilyBox").Text ?? "";

        Take(Provider.OpenAI, "OpenAi");
        Take(Provider.Claude, "Claude");
        Take(Provider.Gemini, "Gemini");
        Take(Provider.Ollama, "Ollama");

        _settings.Save();
        Close();

        void Take(Provider p, string prefix)
        {
            var prof = _settings.Profiles[p];
            prof.Model = C<TextBox>(prefix + "Model").Text ?? "";
            prof.ApiKey = C<TextBox>(prefix + "Key").Text ?? "";
            prof.Endpoint = C<TextBox>(prefix + "Endpoint").Text ?? "";
        }
    }

    void OnCancel(object? sender, RoutedEventArgs e) => Close();
}
