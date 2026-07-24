using System;
using System.ComponentModel;
using System.Data;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading.Tasks;
using MagicAppGen.Models;
using Microsoft.SemanticKernel;

namespace MagicAppGen.Ai;

/// <summary>General-purpose kernel functions available to the assistant:
/// internet search (Tavily), web scraping, arithmetic, and date/time.</summary>
public sealed class CommonPlugin
{
    static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(30) };
    readonly Settings _settings;
    readonly Action<string> _log;

    public CommonPlugin(Settings settings, Action<string> log)
    {
        _settings = settings;
        _log = log;
    }

    [KernelFunction, Description("Search the internet for up-to-date information using Tavily. Returns a short synthesized answer plus top result snippets.")]
    public async Task<string> SearchInternet([Description("the search query")] string query)
    {
        if (string.IsNullOrWhiteSpace(_settings.TavilyApiKey))
            return "error: Tavily API key not set (Settings > Tavily.ApiKey)";
        _log($"[common] searching: {query}");
        try
        {
            var body = JsonSerializer.Serialize(new
            {
                api_key = _settings.TavilyApiKey,
                query,
                search_depth = "basic",
                include_answer = true,
                max_results = 5,
            });
            using var resp = await Http.PostAsync("https://api.tavily.com/search",
                new StringContent(body, Encoding.UTF8, "application/json"));
            var json = await resp.Content.ReadAsStringAsync();
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            var sb = new StringBuilder();
            if (root.TryGetProperty("answer", out var ans) && ans.ValueKind == JsonValueKind.String)
                sb.AppendLine(ans.GetString());
            if (root.TryGetProperty("results", out var results))
                foreach (var r in results.EnumerateArray())
                {
                    var title = r.TryGetProperty("title", out var t) ? t.GetString() : "";
                    var url = r.TryGetProperty("url", out var u) ? u.GetString() : "";
                    sb.AppendLine($"- {title}: {url}");
                }
            return sb.ToString();
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description("Fetch a web page and return its readable text content (HTML tags stripped).")]
    public async Task<string> ScrapeWebPage([Description("the page URL")] string url)
    {
        _log($"[common] scraping: {url}");
        try
        {
            var html = await Http.GetStringAsync(url);
            html = Regex.Replace(html, "<script.*?</script>", " ", RegexOptions.Singleline | RegexOptions.IgnoreCase);
            html = Regex.Replace(html, "<style.*?</style>", " ", RegexOptions.Singleline | RegexOptions.IgnoreCase);
            var text = Regex.Replace(html, "<.*?>", " ");
            text = System.Net.WebUtility.HtmlDecode(text);
            text = Regex.Replace(text, "\\s+", " ").Trim();
            return text.Length > 4000 ? text[..4000] : text;
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description("Evaluate an arithmetic expression (e.g. '3*(4+5)/2') and return the result.")]
    public string MathCalculation([Description("the arithmetic expression")] string expression)
    {
        try
        {
            var result = new DataTable().Compute(expression, null);
            return Convert.ToString(result, System.Globalization.CultureInfo.InvariantCulture) ?? "";
        }
        catch (Exception ex) { return $"error: {ex.Message}"; }
    }

    [KernelFunction, Description("Return the current local date and time.")]
    public string CheckDateTime()
        => DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss zzz", System.Globalization.CultureInfo.InvariantCulture);
}
