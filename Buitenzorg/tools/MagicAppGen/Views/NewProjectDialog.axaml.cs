using System;
using System.IO;
using System.Linq;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using MagicAppGen.Models;
using MagicAppGen.Services;

namespace MagicAppGen.Views;

/// <summary>The result of "Proyek baru": template, display name and the folder to create.</summary>
public sealed record NewProjectChoice(ProjectTemplate Template, string AppName, string Folder);

public partial class NewProjectDialog : Window
{
    readonly Settings _settings;
    string _parent;
    bool _folderEdited;

    public NewProjectDialog() : this(Settings.Load()) { }

    public NewProjectDialog(Settings settings)
    {
        _settings = settings;
        _parent = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Buitenzorg Apps");
        InitializeComponent();
        TemplateList.ItemsSource = Ordered("all");
        TemplateList.SelectedIndex = 0;
        FolderBox.TextChanged += (_, _) => { if (FolderBox.IsFocused) _folderEdited = true; };
        Refresh();
    }

    ProjectTemplate? Selected => TemplateList.SelectedItem as ProjectTemplate;
    string AppName => string.IsNullOrWhiteSpace(NameBox.Text) ? "MyApp" : NameBox.Text.Trim();

    void Refresh()
    {
        var t = Selected;
        var n83 = AppProject.To83(AppName);
        if (!_folderEdited) FolderBox.Text = Path.Combine(_parent, AppName);
        if (t is null) { TargetText.Text = RunText.Text = NoteText.Text = ""; return; }
        TargetText.Text = $"/disk/{n83}.{t.Artifact}";
        RunText.Text = t.Kind == AppKind.Script
            ? $"script {t.Artifact.ToLowerInvariant()} /disk/{n83}.{t.Artifact}"
            : $"run {n83.ToLowerInvariant()}";
        NoteText.Text = t.Kind switch
        {
            AppKind.Managed => "Dibangun dengan `dotnet build` (.NET SDK 10) terhadap pustaka di repo Buitenzorg. Bisa diuji di PC dengan --selftest.",
            AppKind.Native => $"Dibangun dengan bflat --stdlib:zero untuk {_settings.NativeArch}. Tanpa GC: ikuti aturan zerolib (lihat README proyek).",
            _ => "Tidak perlu dibangun: Bangun hanya memeriksa sintaks yang didukung interpreter kernel lalu menyalin skrip.",
        };
        if (t.Kind == AppKind.Managed && string.IsNullOrWhiteSpace(_settings.BuitenzorgRoot))
            NoteText.Text += " Repo Buitenzorg belum ditemukan — atur di Pengaturan.";
    }

    void OnFilter(object? sender, RoutedEventArgs e)
    {
        var tag = (sender as RadioButton)?.Tag as string ?? "all";
        var items = Ordered(tag);
        TemplateList.ItemsSource = items;
        TemplateList.SelectedIndex = items.Count > 0 ? 0 : -1;
    }

    /// <summary>Managed first (the recommended path), then native, then scripts.</summary>
    static System.Collections.Generic.List<ProjectTemplate> Ordered(string tag) =>
        ProjectTemplates.All.Where(t => tag == "all" || t.Kind.ToString() == tag)
            .OrderBy(t => t.Kind switch { AppKind.Managed => 0, AppKind.Native => 1, _ => 2 }).ToList();

    void OnSelectionChanged(object? sender, SelectionChangedEventArgs e) => Refresh();
    void OnNameChanged(object? sender, TextChangedEventArgs e) => Refresh();

    async void OnBrowse(object? sender, RoutedEventArgs e)
    {
        var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions { Title = "Folder induk proyek", AllowMultiple = false });
        if (folders.FirstOrDefault()?.TryGetLocalPath() is not string p) return;
        _parent = p;
        _folderEdited = false;
        Refresh();
    }

    void OnCreate(object? sender, RoutedEventArgs e)
    {
        if (Selected is not ProjectTemplate t) { ShowError("Pilih templat dulu."); return; }
        var folder = FolderBox.Text?.Trim();
        if (string.IsNullOrEmpty(folder)) { ShowError("Isi folder proyek."); return; }
        if (Directory.Exists(folder) && Directory.EnumerateFileSystemEntries(folder).Any() && AppProject.Open(folder) != null)
        {
            ShowError("Folder itu sudah berisi proyek. Pilih nama atau folder lain.");
            return;
        }
        Close(new NewProjectChoice(t, AppName, folder));
    }

    void ShowError(string s) { ErrorText.Text = s; ErrorText.IsVisible = true; }

    void OnCancel(object? sender, RoutedEventArgs e) => Close(null);
}
