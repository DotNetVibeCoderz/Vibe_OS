using System;
using System.Collections.Generic;
using System.Configuration;
using System.Globalization;
using System.IO;
using System.Linq;

namespace MagicAppGen.Models;

/// <summary>One LLM provider's configuration (model, key, endpoint).</summary>
public sealed class LlmProfile
{
    public string Name { get; set; } = "";
    public string Model { get; set; } = "";
    public string ApiKey { get; set; } = "";
    public string Endpoint { get; set; } = "";
    /// <summary>Claude only: the Anthropic workspace id, sent as the
    /// <c>anthropic-workspace-id</c> header (needed by keys that are not scoped to a workspace).</summary>
    public string Workspace { get; set; } = "";
}

/// <summary>The LLM back ends Jack can talk to. Order = order in the UI.</summary>
public enum Provider { OpenAI, AzureOpenAI, Claude, Gemini, DeepSeek, Ollama }

/// <summary>A saved Buitenzorg device reachable over BZDP (UDP).</summary>
public sealed class DeviceProfile
{
    public string Name { get; set; } = "";
    public string Host { get; set; } = "127.0.0.1";
    public int Port { get; set; } = 7070;
    public int Code { get; set; }

    public string CodeText => Code.ToString("0000", CultureInfo.InvariantCulture);
    public override string ToString() => Name;

    public DeviceProfile Clone() => new() { Name = Name, Host = Host, Port = Port, Code = Code };

    /// <summary>"name|host|port|code" records separated by ';' (the app.config form).</summary>
    public static List<DeviceProfile> ParseList(string s)
    {
        var list = new List<DeviceProfile>();
        foreach (var rec in s.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            var f = rec.Split('|');
            if (f.Length < 2 || string.IsNullOrWhiteSpace(f[0])) continue;
            list.Add(new DeviceProfile
            {
                Name = f[0].Trim(),
                Host = f[1].Trim(),
                Port = f.Length > 2 && int.TryParse(f[2], out var p) && p is > 0 and < 65536 ? p : 7070,
                Code = f.Length > 3 && int.TryParse(f[3], out var c) && c is >= 0 and <= 9999 ? c : 0,
            });
        }
        return list;
    }

    public static string FormatList(IEnumerable<DeviceProfile> list) =>
        string.Join(";", list.Select(d => $"{d.Name.Replace("|", "/").Replace(";", ",")}|{d.Host}|{d.Port}|{d.CodeText}"));
}

/// <summary>App configuration, backed by app.config's &lt;appSettings&gt; and
/// editable from the UI. <see cref="Save"/> writes it back to the config file so
/// changes survive a restart.</summary>
public sealed class Settings
{
    public Provider ActiveProvider { get; set; } = Provider.Claude;
    public string Language { get; set; } = "C#";
    public double Temperature { get; set; } = 0.3;
    public bool ShowLineNumbers { get; set; } = true;
    public string SystemPrompt { get; set; } = "";
    public string TavilyApiKey { get; set; } = "";
    public string BuitenzorgRoot { get; set; } = "";

    // ---- device / deploy -----------------------------------------------------
    public List<DeviceProfile> Devices { get; set; } = new();
    public string ActiveDevice { get; set; } = "";
    /// <summary>Also upload BZAPP.DLL / BZUI.DLL with a managed app.</summary>
    public bool DeployRuntimeLibraries { get; set; }
    /// <summary>Native C# target: x64 or arm64.</summary>
    public string NativeArch { get; set; } = "x64";

    public readonly Dictionary<Provider, LlmProfile> Profiles = new();

    public LlmProfile Active => Profiles[ActiveProvider];

    public DeviceProfile Device
    {
        get
        {
            var d = Devices.FirstOrDefault(x => x.Name == ActiveDevice) ?? Devices.FirstOrDefault();
            if (d is null) { d = new DeviceProfile { Name = "QEMU lokal" }; Devices.Add(d); ActiveDevice = d.Name; }
            return d;
        }
    }

    public static readonly string DefaultDevices = "QEMU lokal|127.0.0.1|7070|0000;Raspberry Pi|192.168.1.50|7070|0000";

    public static Settings Load()
    {
        var s = new Settings();
        string Get(string k, string d = "") => ConfigurationManager.AppSettings[k] is { Length: > 0 } v ? v : d;

        s.ActiveProvider = Enum.TryParse<Provider>(Get("ActiveProvider", "Claude"), true, out var p) ? p : Provider.Claude;
        s.Language = Get("Language", "C#");
        s.Temperature = double.TryParse(Get("Temperature", "0.3"), NumberStyles.Float, CultureInfo.InvariantCulture, out var t) ? t : 0.3;
        s.ShowLineNumbers = !bool.TryParse(Get("ShowLineNumbers", "true"), out var b) || b;
        s.SystemPrompt = Get("SystemPrompt");
        s.TavilyApiKey = Get("Tavily.ApiKey");
        s.BuitenzorgRoot = Get("Buitenzorg.Root");
        if (string.IsNullOrWhiteSpace(s.BuitenzorgRoot) || !LooksLikeRepo(s.BuitenzorgRoot))
            s.BuitenzorgRoot = FindRepoRoot() ?? s.BuitenzorgRoot;

        s.Devices = DeviceProfile.ParseList(Get("Device.Profiles", DefaultDevices));
        s.ActiveDevice = Get("Device.Active", s.Devices.FirstOrDefault()?.Name ?? "");
        s.DeployRuntimeLibraries = bool.TryParse(Get("Deploy.RuntimeLibraries", "false"), out var rl) && rl;
        s.NativeArch = Get("Native.Arch", "x64");

        foreach (var prov in Enum.GetValues<Provider>())
        {
            var name = prov.ToString();
            var cat = ModelCatalog.For(prov);
            s.Profiles[prov] = new LlmProfile
            {
                Name = name,
                Model = Get($"{name}.Model", cat.DefaultModel),
                ApiKey = Get($"{name}.ApiKey"),
                Endpoint = Get($"{name}.Endpoint", cat.DefaultEndpoint),
                Workspace = Get($"{name}.Workspace"),
            };
        }
        return s;
    }

    public void Save()
    {
        var cfg = ConfigurationManager.OpenExeConfiguration(ConfigurationUserLevel.None);
        void Set(string k, string v)
        {
            if (cfg.AppSettings.Settings[k] == null) cfg.AppSettings.Settings.Add(k, v);
            else cfg.AppSettings.Settings[k].Value = v;
        }
        Set("ActiveProvider", ActiveProvider.ToString());
        Set("Language", Language);
        Set("Temperature", Temperature.ToString(CultureInfo.InvariantCulture));
        Set("ShowLineNumbers", ShowLineNumbers.ToString());
        Set("SystemPrompt", SystemPrompt);
        Set("Tavily.ApiKey", TavilyApiKey);
        Set("Buitenzorg.Root", BuitenzorgRoot);
        Set("Device.Profiles", DeviceProfile.FormatList(Devices));
        Set("Device.Active", ActiveDevice);
        Set("Deploy.RuntimeLibraries", DeployRuntimeLibraries.ToString());
        Set("Native.Arch", NativeArch);
        foreach (var (prov, prof) in Profiles)
        {
            Set($"{prov}.Model", prof.Model);
            Set($"{prov}.ApiKey", prof.ApiKey);
            Set($"{prov}.Endpoint", prof.Endpoint);
            if (prov == Provider.Claude) Set($"{prov}.Workspace", prof.Workspace);
        }
        cfg.Save(ConfigurationSaveMode.Modified);
        ConfigurationManager.RefreshSection("appSettings");
    }

    static bool LooksLikeRepo(string dir) =>
        Directory.Exists(Path.Combine(dir, "userland", "hello-csharp")) && Directory.Exists(Path.Combine(dir, "kernel"));

    /// <summary>Walk up from the executable (and the working directory) to the Buitenzorg checkout.</summary>
    public static string? FindRepoRoot()
    {
        foreach (var start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
        {
            var dir = start;
            for (int i = 0; i < 10 && dir != null; i++)
            {
                if (LooksLikeRepo(dir)) return dir;
                dir = Directory.GetParent(dir)?.FullName;
            }
        }
        return null;
    }
}
