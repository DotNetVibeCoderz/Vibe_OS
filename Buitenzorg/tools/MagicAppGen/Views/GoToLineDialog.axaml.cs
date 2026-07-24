using System;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;

namespace MagicAppGen.Views;

public partial class GoToLineDialog : Window
{
    public GoToLineDialog()
    {
        InitializeComponent();
    }

    void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    void OnGo(object? sender, RoutedEventArgs e)
    {
        var box = this.FindControl<TextBox>("LineInput");
        Close(int.TryParse(box?.Text, out var n) ? n : (int?)null);
    }

    void OnCancel(object? sender, RoutedEventArgs e) => Close((int?)null);
}
