using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using MagicAppGen.Models;

namespace MagicAppGen.Services;

/// <summary>
/// <c>--llm-test [--keys file] [--provider name]</c>: one real round trip per
/// provider that has a key, through the same AiService (Semantic Kernel +
/// Jack's tools) the UI uses. The answer must arrive and the model must call a
/// tool (CheckDateTime). Keys come from app.config, environment variables
/// (OPENAI_API_KEY, AZURE_OPENAI_API_KEY + AZURE_OPENAI_ENDPOINT, ANTHROPIC_API_KEY,
/// GEMINI_API_KEY, DEEPSEEK_API_KEY) or a key file; they are only held in
/// memory and never printed or saved.
/// </summary>
public static class LlmTest
{
    public static async Task<int> RunAsync(string? keyFile, string? only, string? model = null)
    {
        var settings = Settings.Load();
        ApplyEnvironment(settings);
        if (keyFile != null) ApplyKeyFile(settings, keyFile);
        if (model != null && only != null && Enum.TryParse<Provider>(only, true, out var mp)) settings.Profiles[mp].Model = model;

        int tested = 0, ok = 0;
        foreach (var prov in Enum.GetValues<Provider>())
        {
            if (only != null && !prov.ToString().Equals(only, StringComparison.OrdinalIgnoreCase)) continue;
            var prof = settings.Profiles[prov];
            bool needsKey = ModelCatalog.For(prov).NeedsKey;
            if (needsKey && string.IsNullOrWhiteSpace(prof.ApiKey))
            {
                Console.WriteLine($"{prov,-12} skipped (no key)");
                continue;
            }
            if (prov == Provider.Ollama && !await OllamaUp(prof.Endpoint))
            {
                Console.WriteLine($"{prov,-12} skipped (no Ollama server at {prof.Endpoint})");
                continue;
            }
            tested++;
            settings.ActiveProvider = prov;
            var tools = new List<string>();
            var ai = new AiService(settings, _ => { });
            ai.ToolCalled += n => { lock (tools) tools.Add(n); };
            var sw = Stopwatch.StartNew();
            var reply = new System.Text.StringBuilder();
            try
            {
                using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(120));
                await foreach (var t in ai.AskAsync(
                    "Health check. Call the CheckDateTime tool, then answer in ONE short line: " +
                    "'online, today is <date>'. No other text.", null, null, cts.Token))
                    reply.Append(t);
                var text = reply.ToString().Trim().Replace('\n', ' ');
                bool good = text.Length > 0 && tools.Contains("CheckDateTime");
                if (good) ok++;
                Console.WriteLine($"{prov,-12} {(good ? "OK  " : "FAIL")} model={prof.Model} {sw.ElapsedMilliseconds,6} ms " +
                                  $"tools=[{string.Join(",", tools)}] reply=\"{Clip(text, 90)}\"");
            }
            catch (Exception ex)
            {
                Console.WriteLine($"{prov,-12} FAIL model={prof.Model} {sw.ElapsedMilliseconds,6} ms {ex.GetType().Name}: {Clip(Redact(ex.Message, prof.ApiKey), 220)}");
            }
        }
        Console.WriteLine($"LLM test: {ok}/{tested} provider(s) answered with a tool call");
        return tested > 0 && ok == tested ? 0 : 1;
    }

    static string Clip(string s, int n) => s.Length > n ? s[..n] + "..." : s;
    static string Redact(string s, string key) => string.IsNullOrEmpty(key) ? s : s.Replace(key, "***");

    static async Task<bool> OllamaUp(string endpoint)
    {
        try
        {
            using var http = new System.Net.Http.HttpClient { Timeout = TimeSpan.FromSeconds(2) };
            var r = await http.GetAsync(endpoint.TrimEnd('/') + "/api/tags");
            return r.IsSuccessStatusCode;
        }
        catch { return false; }
    }

    internal static void ApplyEnvironment(Settings s)
    {
        void Env(Provider p, string keyVar, string? endpointVar = null, string? modelVar = null)
        {
            if (Environment.GetEnvironmentVariable(keyVar) is { Length: > 0 } k) s.Profiles[p].ApiKey = k;
            if (endpointVar != null && Environment.GetEnvironmentVariable(endpointVar) is { Length: > 0 } e) s.Profiles[p].Endpoint = e;
            if (modelVar != null && Environment.GetEnvironmentVariable(modelVar) is { Length: > 0 } m) s.Profiles[p].Model = m;
        }
        Env(Provider.OpenAI, "OPENAI_API_KEY", null, "OPENAI_MODEL");
        Env(Provider.AzureOpenAI, "AZURE_OPENAI_API_KEY", "AZURE_OPENAI_ENDPOINT", "AZURE_OPENAI_DEPLOYMENT");
        Env(Provider.Claude, "ANTHROPIC_API_KEY", null, "ANTHROPIC_MODEL");
        if (Environment.GetEnvironmentVariable("ANTHROPIC_WORKSPACE_ID") is { Length: > 0 } ws) s.Profiles[Provider.Claude].Workspace = ws;
        Env(Provider.Gemini, "GEMINI_API_KEY", null, "GEMINI_MODEL");
        Env(Provider.DeepSeek, "DEEPSEEK_API_KEY", null, "DEEPSEEK_MODEL");
    }

    /// <summary>
    /// A loose "Section / key: value" text file, e.g.
    /// <code>
    /// Anthropic
    /// ApiKey: ...
    /// model: claude-sonnet-5-5, claude-opus-5-5
    /// </code>
    /// Sections are matched by name (OpenAI, Azure OpenAI, Anthropic/Claude, Gemini/Google, DeepSeek, Tavily);
    /// a section whose endpoint is a realtime/audio URL is ignored (not a chat model).
    /// </summary>
    internal static void ApplyKeyFile(Settings s, string path)
    {
        if (!File.Exists(path)) { Console.WriteLine($"key file not found: {path}"); return; }
        string? section = null;
        var values = new Dictionary<string, Dictionary<string, string>>();
        foreach (var raw in File.ReadAllLines(path))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith('#')) continue;
            int colon = line.IndexOf(':');
            bool looksLikeKv = colon > 0 && !line.StartsWith("export ", StringComparison.Ordinal) &&
                               !line[..colon].Contains("//");
            if (!looksLikeKv) { section = line; continue; }
            if (section is null) continue;
            var k = line[..colon].Trim().ToLowerInvariant().Replace(" ", "");
            var v = line[(colon + 1)..].Trim();
            if (!values.TryGetValue(section, out var d)) values[section] = d = new();
            d.TryAdd(k, v);
        }
        foreach (var (sec, d) in values)
        {
            var name = sec.ToLowerInvariant();
            string? Get(params string[] keys) => keys.Select(k => d.GetValueOrDefault(k)).FirstOrDefault(v => !string.IsNullOrEmpty(v));
            var key = Get("apikey", "deepseekapikey", "key", "api_key");
            var endpoint = Get("endpoint", "deepseekendpoint", "baseurl");
            var model = Get("model", "modelid")?.Split(',')[0].Trim();
            if (endpoint != null && endpoint.Contains("realtime", StringComparison.OrdinalIgnoreCase)) continue;
            Provider? p = name.Contains("azure") ? Provider.AzureOpenAI
                : name.Contains("anthropic") || name.Contains("claude") ? Provider.Claude
                : name.Contains("deepseek") || name.Contains("deep seek") ? Provider.DeepSeek
                : name.Contains("gemini") || name.Contains("google") ? Provider.Gemini
                : name.Contains("openai") ? Provider.OpenAI
                : null;
            if (name.Contains("tavily") && key != null) { s.TavilyApiKey = key; continue; }
            if (p is null || key is null) continue;
            var prof = s.Profiles[p.Value];
            prof.ApiKey = key;
            if (endpoint != null) prof.Endpoint = endpoint;
            if (model != null) prof.Model = model;
            if (Get("workspaceid", "workspace") is { } ws) prof.Workspace = ws;
        }
    }
}
