using System;
using System.IO;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Threading;
using MagicAppGen.Models;
using MagicAppGen.Services;
using MagicAppGen.Views;

namespace MagicAppGen;

/// <summary>
/// <c>--screenshot &lt;file.png&gt; [--scene main|templates|settings|devices] [--size 1360x820]</c>:
/// renders a window with Skia on the headless platform (no display needed) and
/// saves the frame, so the UI can be checked and documented from a script.
/// </summary>
internal static class Screenshot
{
    public static int Run(string[] args)
    {
        var file = Path.GetFullPath(args[1]);
        string scene = Opt(args, "--scene") ?? "main";
        int w = 1360, h = 820;
        if (Opt(args, "--size") is { } size && size.Split('x') is [var sw, var sh]) { w = int.Parse(sw); h = int.Parse(sh); }

        AppBuilder.Configure<App>()
            .UseSkia()
            .UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false })
            .WithInterFont()
            .SetupWithoutStarting();

        var settings = Settings.Load();
        Window win = scene switch
        {
            "templates" => new NewProjectDialog(settings),
            "settings" => new SettingsWindow(settings, 1),
            "devices" => new SettingsWindow(settings, 2),
            _ => new MainWindow(settings),
        };
        win.Width = scene == "main" ? w : win.Width;
        win.Height = scene == "main" ? h : win.Height;
        win.Show();

        if (win is MainWindow mw)
        {
            var dir = Path.Combine(Path.GetTempPath(), "magicappgen-shot", "Catatan");
            if (Directory.Exists(dir)) Directory.Delete(dir, true);
            ProjectTemplates.Scaffold(ProjectTemplates.Find("managed-ui")!, dir, "Catatan", settings.BuitenzorgRoot);
            Directory.CreateDirectory(Path.Combine(dir, "out"));
            File.WriteAllBytes(Path.Combine(dir, "out", "CATATAN.DLL"), new byte[18_432]);
            var t = mw.StageForScreenshot(dir);
            while (!t.IsCompleted) Dispatcher.UIThread.RunJobs();
            t.GetAwaiter().GetResult();
        }

        for (int i = 0; i < 6; i++)
        {
            Dispatcher.UIThread.RunJobs();
            AvaloniaHeadlessPlatform.ForceRenderTimerTick();
        }
        var frame = win.CaptureRenderedFrame();
        if (frame is null) { Console.Error.WriteLine("no frame rendered"); return 1; }
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        frame.Save(file);
        Console.WriteLine($"saved {file} ({frame.PixelSize.Width}x{frame.PixelSize.Height}, scene {scene})");
        win.Close();
        return 0;
    }

    static string? Opt(string[] a, string name) { int i = Array.IndexOf(a, name); return i >= 0 && i + 1 < a.Length ? a[i + 1] : null; }
}
