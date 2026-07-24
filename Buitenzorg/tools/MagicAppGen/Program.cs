using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Avalonia;
using MagicAppGen.Models;
using MagicAppGen.Services;

namespace MagicAppGen;

internal static class Program
{
    [System.STAThread]
    public static void Main(string[] args)
    {
        // Headless helpers, so the template catalog can be used (and verified)
        // without a display:
        //   MagicAppGen --list-templates
        //   MagicAppGen --scaffold <template-id> <folder> [AppName]
        if (args.Length > 0 && args[0] == "--list-templates")
        {
            foreach (var t in ProjectTemplates.All)
                Console.WriteLine($"{t.Id,-12} [{t.Language,-10}] {t.Summary}");
            return;
        }
        if (args.Length >= 3 && args[0] == "--scaffold")
        {
            var t = ProjectTemplates.Find(args[1]);
            if (t is null) { Console.Error.WriteLine($"unknown template '{args[1]}'"); Environment.Exit(1); return; }
            Console.WriteLine(ProjectTemplates.Scaffold(t, args[2], args.Length > 3 ? args[3] : "MyApp"));
            return;
        }

        // Headless AI generation, so "Jack" can be exercised end to end without a
        // display (real LLM call using the provider configured in app.config):
        //   MagicAppGen --generate <outputDir> <prompt...>
        // The assistant writes the generated project into <outputDir> via its
        // kernel functions. Streamed reply -> stdout, tool/log lines -> stderr.
        if (args.Length >= 3 && args[0] == "--generate")
        {
            // Optional model override: --generate --model <id> <outDir> <prompt...>
            string? modelOverride = null;
            int i = 1;
            if (args.Length >= 5 && args[1] == "--model") { modelOverride = args[2]; i = 3; }
            var outDir = args[i];
            var promptText = string.Join(' ', args, i + 1, args.Length - (i + 1));
            Environment.ExitCode = GenerateAsync(outDir, promptText, modelOverride).GetAwaiter().GetResult();
            return;
        }

        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    static async Task<int> GenerateAsync(string outputDir, string prompt, string? modelOverride = null)
    {
        var settings = Settings.Load();
        var profile = settings.Active;
        if (!string.IsNullOrWhiteSpace(modelOverride)) profile.Model = modelOverride;

        // CompileCheck/BuildApp need the repo root (to find tools/bflat and the
        // library sources). If it is not set in app.config, walk up from here
        // until we find the marker layout.
        if (string.IsNullOrWhiteSpace(settings.BuitenzorgRoot))
        {
            var dir = AppContext.BaseDirectory;
            for (int i = 0; i < 8 && dir != null; i++)
            {
                if (File.Exists(System.IO.Path.Combine(dir, "tools", "bflat", "bflat.exe")) ||
                    Directory.Exists(System.IO.Path.Combine(dir, "userland", "hello-csharp")))
                { settings.BuitenzorgRoot = dir; break; }
                dir = Directory.GetParent(dir)?.FullName;
            }
        }

        Console.Error.WriteLine($"[gen] provider={settings.ActiveProvider} model={profile.Model} " +
                                $"key={(string.IsNullOrWhiteSpace(profile.ApiKey) ? "MISSING" : "set")} " +
                                $"root={(string.IsNullOrWhiteSpace(settings.BuitenzorgRoot) ? "?" : settings.BuitenzorgRoot)} " +
                                $"outputDir={outputDir}");
        if (string.IsNullOrWhiteSpace(profile.ApiKey))
        {
            Console.Error.WriteLine("[gen] no API key for the active provider (set it in app.config).");
            return 2;
        }

        // Point the file-writing kernel functions at the repo root so the AI has
        // context, and steer it to scaffold into the requested output folder.
        System.IO.Directory.CreateDirectory(outputDir);
        var ai = new AiService(settings, line => Console.Error.WriteLine("[log] " + line));

        var task = $"{prompt}\n\nWrite the complete Buitenzorg C# app into the folder " +
                   $"\"{outputDir}\" using your ScaffoldProject/WriteFile functions. Follow the zerolib rules " +
                   $"(call GetApiReference for the exact API before writing UI/Drawing/Audio/Bcl code). " +
                   $"After writing, you MUST call CompileCheck on the .cs file to verify it builds with bflat; " +
                   $"if it reports errors, fix them with WriteFile and call CompileCheck again, repeating until " +
                   $"it says OK. Only then give a short explanation.";

        Console.Error.WriteLine("[gen] sending prompt to the model...");
        using var cts = new CancellationTokenSource(TimeSpan.FromMinutes(3));
        try
        {
            await foreach (var token in ai.AskAsync(task, null, null, cts.Token))
                Console.Write(token);
            Console.WriteLine();
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"[gen] ERROR: {ex.GetType().Name}: {ex.Message}");
            return 1;
        }
        Console.Error.WriteLine("[gen] done.");
        return 0;
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();
}
