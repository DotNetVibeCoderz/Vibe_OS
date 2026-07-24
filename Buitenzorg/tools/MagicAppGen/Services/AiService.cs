using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Threading;
using System.Threading.Tasks;
using MagicAppGen.Ai;
using MagicAppGen.Models;
using Microsoft.Extensions.AI;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.SemanticKernel;
using Microsoft.SemanticKernel.ChatCompletion;
using Microsoft.SemanticKernel.Connectors.OpenAI;

namespace MagicAppGen.Services;

/// <summary>The AI assistant "Jack - The Code Bender". Wraps Microsoft Semantic
/// Kernel: builds a <see cref="Kernel"/> for the active provider (OpenAI /
/// Claude / Gemini / Ollama), registers the Buitenzorg + common function
/// plugins, and streams a function-calling chat completion. The kernel is
/// rebuilt whenever the settings/provider change.</summary>
public sealed class AiService
{
    readonly Settings _settings;
    readonly ChatHistory _history = new();
    Kernel _kernel;

    public AiService(Settings settings, Action<string> log)
    {
        _settings = settings;
        _kernel = BuildKernel(settings, log);
        Reset();
    }

    /// <summary>Rebuild the kernel (call after settings/provider change).</summary>
    public void Rebuild(Action<string> log) => _kernel = BuildKernel(_settings, log);

    /// <summary>Clear the conversation and seed the system prompt.</summary>
    public void Reset()
    {
        _history.Clear();
        if (!string.IsNullOrWhiteSpace(_settings.SystemPrompt))
            _history.AddSystemMessage(_settings.SystemPrompt);
    }

    /// <summary>Send a user turn (optionally with an attached image) and stream
    /// the assistant's reply token by token.</summary>
    public async IAsyncEnumerable<string> AskAsync(
        string userText,
        byte[]? imageBytes,
        string? imageMime,
        [EnumeratorCancellation] CancellationToken ct = default)
    {
        if (imageBytes is { Length: > 0 })
        {
            var items = new ChatMessageContentItemCollection
            {
                // Fully qualified: Microsoft.Extensions.AI has same-named types.
                new Microsoft.SemanticKernel.TextContent(userText),
                new Microsoft.SemanticKernel.ImageContent(imageBytes, imageMime ?? "image/png"),
            };
            _history.AddUserMessage(items);
        }
        else
        {
            _history.AddUserMessage(userText);
        }

        var chat = _kernel.GetRequiredService<IChatCompletionService>();
        var exec = new OpenAIPromptExecutionSettings
        {
            Temperature = _settings.Temperature,
            FunctionChoiceBehavior = FunctionChoiceBehavior.Auto(),
        };

        var full = new System.Text.StringBuilder();
        await foreach (var chunk in chat.GetStreamingChatMessageContentsAsync(_history, exec, _kernel, ct))
        {
            if (chunk.Content is { Length: > 0 } piece)
            {
                full.Append(piece);
                yield return piece;
            }
        }
        _history.AddAssistantMessage(full.ToString());
    }

    static Kernel BuildKernel(Settings s, Action<string> log)
    {
        var builder = Kernel.CreateBuilder();
        var p = s.Active;
        try
        {
            switch (s.ActiveProvider)
            {
                case Provider.OpenAI:
                    if (!string.IsNullOrWhiteSpace(p.Endpoint))
                        builder.AddOpenAIChatCompletion(p.Model, new Uri(p.Endpoint), p.ApiKey);
                    else
                        builder.AddOpenAIChatCompletion(p.Model, p.ApiKey);
                    break;
                case Provider.Gemini:
#pragma warning disable SKEXP0070
                    builder.AddGoogleAIGeminiChatCompletion(p.Model, p.ApiKey);
#pragma warning restore SKEXP0070
                    break;
                case Provider.Ollama:
#pragma warning disable SKEXP0070
                    builder.AddOllamaChatCompletion(p.Model, new Uri(
                        string.IsNullOrWhiteSpace(p.Endpoint) ? "http://localhost:11434" : p.Endpoint));
#pragma warning restore SKEXP0070
                    break;
                case Provider.Claude:
                    // Claude has no first-party SK connector; bridge Anthropic.SDK's
                    // IChatClient into SK's chat-completion abstraction.
                    var anthropic = new Anthropic.SDK.AnthropicClient(p.ApiKey);
                    IChatClient claude = anthropic.Messages;
                    builder.Services.AddSingleton<IChatCompletionService>(
                        claude.AsChatCompletionService());
                    break;
            }
        }
        catch (Exception ex)
        {
            log($"[ai] provider init failed: {ex.Message}");
        }

        var kernel = builder.Build();
        // Register the assistant's tools (kernel functions).
        kernel.Plugins.AddFromObject(new BuitenzorgPlugin(s, log), "buitenzorg");
        kernel.Plugins.AddFromObject(new CommonPlugin(s, log), "common");
        return kernel;
    }
}
