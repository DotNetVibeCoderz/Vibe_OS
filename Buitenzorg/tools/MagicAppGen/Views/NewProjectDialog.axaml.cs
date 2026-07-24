using System;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using MagicAppGen.Services;

namespace MagicAppGen.Views;

/// <summary>The result of "New Project → From Template".</summary>
public sealed record NewProjectChoice(ProjectTemplate Template, string AppName);

public partial class NewProjectDialog : Window
{
    public NewProjectDialog()
    {
        InitializeComponent();
        var list = this.FindControl<ListBox>("TemplateList")!;
        list.ItemsSource = ProjectTemplates.All;
        list.SelectedIndex = 0;
    }

    void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    void OnCreate(object? sender, RoutedEventArgs e)
    {
        var list = this.FindControl<ListBox>("TemplateList")!;
        if (list.SelectedItem is not ProjectTemplate t) { Close(null); return; }
        var name = this.FindControl<TextBox>("NameBox")!.Text;
        Close(new NewProjectChoice(t, string.IsNullOrWhiteSpace(name) ? "MyApp" : name.Trim()));
    }

    void OnCancel(object? sender, RoutedEventArgs e) => Close(null);
}
