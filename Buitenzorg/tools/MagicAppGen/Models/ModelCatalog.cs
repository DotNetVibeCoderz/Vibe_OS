using System;
using System.Collections.Generic;

namespace MagicAppGen.Models;

/// <summary>Suggested model ids and defaults per provider (reviewed 2026-11).
/// The Settings dialog offers these as suggestions; any id can be typed.</summary>
public sealed record ModelCatalog(
    Provider Provider,
    string DisplayName,
    string DefaultModel,
    string DefaultEndpoint,
    IReadOnlyList<string> Models,
    bool NeedsKey,
    string EndpointHint)
{
    public static ModelCatalog For(Provider p) => p switch
    {
        Provider.OpenAI => new(p, "OpenAI", "gpt-5.6-sol", "https://api.openai.com/v1",
            new[] { "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5", "gpt-5.4-mini", "gpt-5-mini" },
            true, "https://api.openai.com/v1 (or any OpenAI-compatible base URL)"),
        Provider.AzureOpenAI => new(p, "Azure OpenAI", "gpt-5.6-luna", "",
            new[] { "gpt-5.6-luna", "gpt-5-mini", "gpt-5.5" },
            true, "https://<resource>.openai.azure.com/  (model = deployment name)"),
        Provider.Claude => new(p, "Claude (Anthropic)", "claude-opus-5-5", "",
            new[] { "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-fable-5-1" },
            true, "empty = https://api.anthropic.com"),
        Provider.Gemini => new(p, "Gemini (Google AI)", "gemini-3.5-flash", "",
            new[] { "gemini-3.5-flash", "gemini-3.1-pro-preview", "gemini-3-flash-preview" },
            true, "not used (Google AI Studio key)"),
        Provider.DeepSeek => new(p, "DeepSeek", "deepseek-v4-flash", "https://api.deepseek.com",
            new[] { "deepseek-v4-flash", "deepseek-chat", "deepseek-reasoner" },
            true, "https://api.deepseek.com"),
        Provider.Ollama => new(p, "Ollama (lokal)", "qwen3", "http://localhost:11434",
            new[] { "qwen3", "llama3.3", "gpt-oss", "deepseek-r1" },
            false, "http://localhost:11434"),
        _ => throw new ArgumentOutOfRangeException(nameof(p)),
    };

    /// <summary>Whether a request may carry a custom sampling temperature.
    /// Current Claude models (Opus 5.5 / Sonnet 5.5 / Haiku 5.5 / Fable 5.1)
    /// reject temperature with HTTP 400, and OpenAI's reasoning models (the
    /// gpt-5 and o-series families) accept only the default.</summary>
    public static bool SupportsTemperature(Provider p, string model)
    {
        var m = model.ToLowerInvariant();
        return p switch
        {
            Provider.Claude => false,
            Provider.OpenAI or Provider.AzureOpenAI =>
                !(m.StartsWith("gpt-5") || m.StartsWith("o1") || m.StartsWith("o3") || m.StartsWith("o4")),
            Provider.DeepSeek => !m.Contains("reasoner"),
            _ => true,
        };
    }
}
