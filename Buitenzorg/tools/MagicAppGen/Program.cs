using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Avalonia;
using MagicAppGen.Models;
using MagicAppGen.Services;

namespace MagicAppGen;

internal static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        // Headless helpers (no display needed):
        //   --list-templates
        //   --scaffold <template-id> <folder> [AppName]
        //   --build <folder>                        build a project into <folder>/out
        //   --deploy <folder> --host <ip> [--port 7070] [--code NNNN] [--run] [--args "..."] [--watch <s>]
        //   --fake-device [--port 7070] [--code NNNN] [--drop 0.2]    a loopback BZDP device
        //   --bzdp-selftest                         client vs. fake device, end to end
        //   --llm-test [--keys <file>] [--provider <name>]   one real round trip per provider with a key
        //   --generate [--model <id>] <outDir> <prompt...>
        //   --screenshot <file.png> [--scene main|settings|templates]   render the UI headlessly
        if (args.Length > 0 && args[0].StartsWith("--", StringComparison.Ordinal) && args[0] != "--screenshot")
        {
            Environment.ExitCode = RunHeadless(args).GetAwaiter().GetResult();
            return;
        }
        if (args.Length >= 2 && args[0] == "--screenshot")
        {
            Environment.ExitCode = Screenshot.Run(args);
            return;
        }
        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
    }

    static string? Opt(string[] a, string name) { int i = Array.IndexOf(a, name); return i >= 0 && i + 1 < a.Length ? a[i + 1] : null; }
    static bool Flag(string[] a, string name) => Array.IndexOf(a, name) >= 0;

    static async Task<int> RunHeadless(string[] args)
    {
        switch (args[0])
        {
            case "--list-templates":
                foreach (var t in ProjectTemplates.All)
                    Console.WriteLine($"{t.Id,-16} [{t.KindLabel,-7} {t.Language,-10} -> {t.Artifact}] {t.Summary}");
                return 0;
            case "--scaffold" when args.Length >= 3:
            {
                var t = ProjectTemplates.Find(args[1]);
                if (t is null) { Console.Error.WriteLine($"unknown template '{args[1]}'"); return 1; }
                Console.WriteLine(ProjectTemplates.Scaffold(t, args[2], args.Length > 3 ? args[3] : "MyApp"));
                return 0;
            }
            case "--build" when args.Length >= 2:
                return await BuildCmd(args[1], Flag(args, "--selftest"));
            case "--deploy" when args.Length >= 2:
                return await DeployCmd(args);
            case "--fake-device":
                return await FakeDeviceCmd(args);
            case "--bzdp-selftest":
                return await BzdpSelfTest.RunAsync();
            case "--llm-test":
                return await LlmTest.RunAsync(Opt(args, "--keys"), Opt(args, "--provider"), Opt(args, "--model"));
            case "--generate" when args.Length >= 3:
            {
                // --generate [--model <id>] [--provider <name>] [--keys <file>] <outDir> <prompt...>
                string? modelOverride = null, provider = null, keys = null;
                int i = 1;
                while (i + 1 < args.Length && args[i].StartsWith("--", StringComparison.Ordinal))
                {
                    if (args[i] == "--model") modelOverride = args[i + 1];
                    else if (args[i] == "--provider") provider = args[i + 1];
                    else if (args[i] == "--keys") keys = args[i + 1];
                    i += 2;
                }
                return await GenerateAsync(args[i], string.Join(' ', args, i + 1, args.Length - (i + 1)), modelOverride, provider, keys);
            }
            default:
                Console.Error.WriteLine("usage: see the comment at the top of Program.cs, or tools/MagicAppGen/README.md");
                return 2;
        }
    }

    static void Log(string s) => Console.Error.WriteLine(s);

    static async Task<int> BuildCmd(string dir, bool selftest)
    {
        var settings = Settings.Load();
        var p = AppProject.Open(dir);
        if (p is null) { Log($"no Buitenzorg project in {dir}"); return 1; }
        Log($"[build] {p.KindLabel} project {p.Name83} -> {p.RemotePath}");
        var b = new ProjectBuilder(settings, Log);
        var r = await b.BuildAsync(p);
        if (!r.Ok) { Log(r.Message); return 1; }
        foreach (var f in r.Files) Console.WriteLine($"{f.LocalPath} => {f.RemotePath}");
        foreach (var w in r.Warnings) Log("warning: " + w);
        if (selftest && p.Kind == AppKind.Managed)
        {
            var (ok, output) = await b.SelfTestManagedAsync(p);
            Console.WriteLine(output);
            Log($"[selftest] {(ok ? "passed" : "FAILED")}");
            return ok ? 0 : 1;
        }
        return 0;
    }

    static async Task<int> DeployCmd(string[] args)
    {
        var settings = Settings.Load();
        var dir = args[1];
        var dev = settings.Device.Clone();
        dev.Name = "cli";
        if (Opt(args, "--host") is { } h) dev.Host = h;
        if (int.TryParse(Opt(args, "--port"), out var port)) dev.Port = port;
        if (int.TryParse(Opt(args, "--code"), out var code)) dev.Code = code;
        if (Flag(args, "--with-libs")) settings.DeployRuntimeLibraries = true;

        var p = AppProject.Open(dir);
        if (p is null) { Log($"no Buitenzorg project in {dir}"); return 1; }
        var r = await new ProjectBuilder(settings, Log).BuildAsync(p);
        if (!r.Ok) { Log(r.Message); return 1; }
        try
        {
            using var d = new DeviceDeployer(dev, Log);
            var info = await d.ConnectAsync();
            Console.WriteLine($"device: {info.Text}");
            int lastPct = -1;
            await d.UploadAsync(r.Files, new Progress<(string File, double Fraction)>(x =>
            {
                int pct = (int)(x.Fraction * 100);
                if (pct / 10 != lastPct / 10) { lastPct = pct; Log($"[deploy] {x.File} {pct}%"); }
            }));
            foreach (var f in r.Files) Console.WriteLine($"uploaded {f.RemotePath}");
            if (Flag(args, "--run"))
            {
                var cmd = p.RunCommand(Opt(args, "--args") ?? "");
                var pid = await d.RunAsync(cmd);
                Console.WriteLine(pid == 0 ? $"ran: {cmd}" : $"started: {cmd} (pid {pid})");
                var watch = double.TryParse(Opt(args, "--watch"), out var w) ? w : 3;
                var until = DateTime.UtcNow.AddSeconds(watch);
                while (DateTime.UtcNow < until)
                {
                    Console.Write(await d.PollLogAsync());
                    await Task.Delay(400);
                }
                Console.Write(await d.PollLogAsync());
            }
            return 0;
        }
        catch (Exception ex) { Log($"[deploy] {ex.Message}"); return 3; }
    }

    static async Task<int> FakeDeviceCmd(string[] args)
    {
        int port = int.TryParse(Opt(args, "--port"), out var p) ? p : BzdpClient.DefaultPort;
        int code = int.TryParse(Opt(args, "--code"), out var c) ? c : 0;
        double drop = double.TryParse(Opt(args, "--drop"), System.Globalization.NumberStyles.Float,
                                      System.Globalization.CultureInfo.InvariantCulture, out var d) ? d : 0;
        using var dev = new FakeBzdpDevice(port, code) { DropRate = drop };
        Console.WriteLine($"fake BZDP device on udp 127.0.0.1:{dev.Port}, pairing code {code:0000}, drop {drop:P0}. Ctrl+C to stop.");
        var run = dev.RunAsync();
        var done = new TaskCompletionSource();
        Console.CancelKeyPress += (_, e) => { e.Cancel = true; done.TrySetResult(); };
        await done.Task;
        return 0;
    }

    static async Task<int> GenerateAsync(string outputDir, string prompt, string? modelOverride = null,
                                         string? provider = null, string? keyFile = null)
    {
        var settings = Settings.Load();
        LlmTest.ApplyEnvironment(settings);
        if (keyFile != null) LlmTest.ApplyKeyFile(settings, keyFile); // in memory only, never saved
        if (provider != null && Enum.TryParse<Provider>(provider, true, out var prov)) settings.ActiveProvider = prov;
        var profile = settings.Active;
        if (!string.IsNullOrWhiteSpace(modelOverride)) profile.Model = modelOverride;
        Log($"[gen] provider={settings.ActiveProvider} model={profile.Model} " +
            $"key={(string.IsNullOrWhiteSpace(profile.ApiKey) ? "MISSING" : "set")} root={settings.BuitenzorgRoot} outputDir={outputDir}");
        if (string.IsNullOrWhiteSpace(profile.ApiKey) && ModelCatalog.For(settings.ActiveProvider).NeedsKey)
        {
            Log("[gen] no API key for the active provider (set it in Settings).");
            return 2;
        }
        Directory.CreateDirectory(outputDir);
        var ai = new AiService(settings, line => Log("[log] " + line));
        ai.ToolCalled += n => Log($"[tool] {n}");
        var task = $"{prompt}\n\nWrite the complete Buitenzorg app into the folder \"{outputDir}\" (ScaffoldProject from a " +
                   $"fitting template, then WriteFile). Call GetApiReference for the exact API first. Then call BuildProject " +
                   $"on the folder; if it reports errors or warnings, fix them and build again until it says OK. Only then " +
                   $"give a short explanation.";
        using var cts = new CancellationTokenSource(TimeSpan.FromMinutes(6));
        try
        {
            await foreach (var token in ai.AskAsync(task, null, null, cts.Token)) Console.Write(token);
            Console.WriteLine();
        }
        catch (Exception ex) { Log($"[gen] ERROR: {ex.GetType().Name}: {ex.Message}"); return 1; }
        return 0;
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();
}
