using System;
using System.Collections.Generic;
using System.Configuration;

namespace MagicAppGen.Models;

/// <summary>One LLM provider's configuration (model, key, endpoint).</summary>
public sealed class LlmProfile
{
    public string Name { get; set; } = "";
    public string Model { get; set; } = "";
    public string ApiKey { get; set; } = "";
    public string Endpoint { get; set; } = "";
}

public enum Provider { OpenAI, Claude, Gemini, Ollama }

/// <summary>App configuration, backed by app.config's &lt;appSettings&gt; and
/// editable from the UI. <see cref="Save"/> writes it back to the config file so
/// changes survive a restart.</summary>
public sealed class Settings
{
    public Provider ActiveProvider { get; set; } = Provider.OpenAI;
    public string Language { get; set; } = "C#";
    public double Temperature { get; set; } = 0.3;
    public bool ShowLineNumbers { get; set; } = true;
    public string SystemPrompt { get; set; } = "";
    public string TavilyApiKey { get; set; } = "";
    public string BuitenzorgRoot { get; set; } = "";

    public readonly Dictionary<Provider, LlmProfile> Profiles = new();

    public LlmProfile Active => Profiles[ActiveProvider];

    public static Settings Load()
    {
        var s = new Settings();
        string Get(string k, string d = "") => ConfigurationManager.AppSettings[k] ?? d;

        s.ActiveProvider = Enum.TryParse<Provider>(Get("ActiveProvider", "OpenAI"), true, out var p) ? p : Provider.OpenAI;
        s.Language = Get("Language", "C#");
        s.Temperature = double.TryParse(Get("Temperature", "0.3"), out var t) ? t : 0.3;
        s.ShowLineNumbers = bool.TryParse(Get("ShowLineNumbers", "true"), out var b) && b;
        s.SystemPrompt = Get("SystemPrompt");
        s.TavilyApiKey = Get("Tavily.ApiKey");
        s.BuitenzorgRoot = Get("Buitenzorg.Root");

        foreach (var name in new[] { "OpenAI", "Claude", "Gemini", "Ollama" })
        {
            var prov = Enum.Parse<Provider>(name);
            s.Profiles[prov] = new LlmProfile
            {
                Name = name,
                Model = Get($"{name}.Model"),
                ApiKey = Get($"{name}.ApiKey"),
                Endpoint = Get($"{name}.Endpoint"),
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
        Set("Temperature", Temperature.ToString(System.Globalization.CultureInfo.InvariantCulture));
        Set("ShowLineNumbers", ShowLineNumbers.ToString());
        Set("SystemPrompt", SystemPrompt);
        Set("Tavily.ApiKey", TavilyApiKey);
        Set("Buitenzorg.Root", BuitenzorgRoot);
        foreach (var (prov, prof) in Profiles)
        {
            Set($"{prov}.Model", prof.Model);
            Set($"{prov}.ApiKey", prof.ApiKey);
            Set($"{prov}.Endpoint", prof.Endpoint);
        }
        cfg.Save(ConfigurationSaveMode.Modified);
        ConfigurationManager.RefreshSection("appSettings");
    }
}
