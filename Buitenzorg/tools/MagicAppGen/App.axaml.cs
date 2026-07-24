using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using MagicAppGen.Models;
using MagicAppGen.Views;

namespace MagicAppGen;

public partial class App : Application
{
    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var settings = Settings.Load();
            desktop.MainWindow = new MainWindow(settings);
        }
        base.OnFrameworkInitializationCompleted();
    }
}
