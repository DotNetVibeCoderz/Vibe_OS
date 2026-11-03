using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Threading;
using System.Threading.Tasks;
using Anthropic;
using MagicAppGen.Ai;
using MagicAppGen.Models;
using Microsoft.Extensions.AI;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.SemanticKernel;
using Microsoft.SemanticKernel.ChatCompletion;

namespace MagicAppGen.Services;

/// <summary>The AI assistant "Jack - The Code Bender". Wraps Microsoft Semantic
/// Kernel: builds a <see cref="Kernel"/> for the active provider (OpenAI /
/// Azure OpenAI / Claude / Gemini / DeepSeek / Ollama), registers the
/// Buitenzorg + common function plugins, and streams a function-calling chat
/// completion. The kernel is rebuilt whenever the settings/provider change.</summary>
public sealed class AiService
{
    readonly Settings _settings;
    readonly ChatHistory _history = new();
    readonly ToolCallFilter _filter = new();
    Kernel _kernel;

    /// <summary>Raised (on a worker thread) whenever Jack calls one of its tools.</summary>
    public event Action<string>? ToolCalled
    {
        add => _filter.Called += value;
        remove => _filter.Called -= value;
    }

    public AiService(Settings settings, Action<string> log)
    {
        _settings = settings;
        _kernel = BuildKernel(settings, log, _filter);
        Reset();
    }

    /// <summary>Rebuild the kernel (call after settings/provider change).</summary>
    public void Rebuild(Action<string> log) => _kernel = BuildKernel(_settings, log, _filter);

    /// <summary>Clear the conversation and seed the system prompt.</summary>
    public void Reset()
    {
        _history.Clear();
        var prompt = string.IsNullOrWhiteSpace(_settings.SystemPrompt) ? DefaultSystemPrompt : _settings.SystemPrompt;
        _history.AddSystemMessage(prompt);
    }

    public const string DefaultSystemPrompt =
        "You are Jack - The Code Bender, an expert assistant that writes applications for Buitenzorg OS " +
        "(made by Gravicode Studios, led by Kang Fadhil). A Buitenzorg app is one of three kinds: " +
        "(1) NATIVE C# - compiled with bflat --stdlib:zero + the bzstart shim into /disk/<NAME>.ELF; no GC, so " +
        "no static reference fields, no method-group delegates (use function pointers), no object[] element " +
        "stores (use linked lists), no string concat/ToString (build char[] + Graphics.DrawChars); libraries " +
        "Buitenzorg.Drawing/UI/Audio/Bcl. (2) MANAGED C# - a normal `dotnet build` net10.0 assembly run by the " +
        "RustCLR interpreter (/disk/<NAME>.DLL, full GC, LINQ, generics, lambdas, exceptions) using " +
        "Buitenzorg.App (Window, Files, Programs, Processes, SystemInfo, Settings, Audio, Keyboard) and " +
        "Buitenzorg.UI (an Avalonia-style toolkit: Application, Window, StackPanel/Grid/DockPanel, Button, " +
        "TextBox, ListBox, TabControl, styles, binding) plus Buitenzorg.Devices (GPIO/I2C/SPI/UART/LED on a " +
        "Raspberry Pi); its assembly name must be an 8.3 upper-case name. (3) SCRIPTS - Python, JavaScript or " +
        "TypeScript run by the in-kernel interpreter, which supports only ints, strings, bools, variables, " +
        "if/else, while, for (C-style or range()), functions with recursion, print/console.log/str/len - no " +
        "lists, objects, classes, imports or floats. Prefer MANAGED C# for anything with a real UI unless the " +
        "user asks for native. Call GetApiReference before writing code so you use exact signatures, then " +
        "build with BuildProject (or CompileCheck for native) and fix errors until it builds. You can deploy " +
        "and run on the user's device with DeployToDevice and read its console with ReadDeviceLog. Reply in " +
        "the user's language (often Indonesian), briefly.";

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
        var exec = ExecutionSettings(_settings);

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

    /// <summary>Provider-neutral settings: automatic function calling, plus a
    /// temperature only where the model accepts one (see ModelCatalog).</summary>
    static PromptExecutionSettings ExecutionSettings(Settings s)
    {
        var exec = new PromptExecutionSettings { FunctionChoiceBehavior = FunctionChoiceBehavior.Auto() };
        var ext = new Dictionary<string, object>();
        if (ModelCatalog.SupportsTemperature(s.ActiveProvider, s.Active.Model))
            ext["temperature"] = s.Temperature;
        if (s.ActiveProvider == Provider.Ollama)
            ext["num_ctx"] = 16384;
        if (ext.Count > 0) exec.ExtensionData = ext;
        return exec;
    }

    static Kernel BuildKernel(Settings s, Action<string> log, ToolCallFilter filter)
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
                case Provider.AzureOpenAI:
                    // Model = the Azure deployment name; endpoint = https://<resource>.openai.azure.com/
                    builder.AddAzureOpenAIChatCompletion(p.Model, p.Endpoint, p.ApiKey);
                    break;
                case Provider.DeepSeek:
                    // DeepSeek speaks the OpenAI chat-completions protocol.
                    builder.AddOpenAIChatCompletion(p.Model,
                        new Uri(string.IsNullOrWhiteSpace(p.Endpoint) ? "https://api.deepseek.com" : p.Endpoint), p.ApiKey);
                    break;
                case Provider.Gemini:
                    builder.AddGoogleAIGeminiChatCompletion(p.Model, p.ApiKey);
                    break;
                case Provider.Ollama:
                    builder.AddOllamaChatCompletion(p.Model, new Uri(
                        string.IsNullOrWhiteSpace(p.Endpoint) ? "http://localhost:11434" : p.Endpoint));
                    break;
                case Provider.Claude:
                {
                    // Official Anthropic C# SDK -> Microsoft.Extensions.AI IChatClient,
                    // with SK's kernel-function invocation layered on top so
                    // FunctionChoiceBehavior.Auto() actually runs Jack's tools.
                    IAnthropicClient anthropic = new AnthropicClient
                    {
                        ApiKey = p.ApiKey,
                        BaseUrl = string.IsNullOrWhiteSpace(p.Endpoint) ? "https://api.anthropic.com" : p.Endpoint,
                    };
                    // Keys that are not scoped to one workspace must name it on every request.
                    if (!string.IsNullOrWhiteSpace(p.Workspace))
                        anthropic = anthropic.WithOptions(o => o with
                        {
                            ExtraHeaders = new Dictionary<string, string> { ["anthropic-workspace-id"] = p.Workspace.Trim() },
                        });
                    IChatClient claude = anthropic.AsIChatClient(p.Model, 32000)
                        .AsBuilder()
                        .UseKernelFunctionInvocation()
                        .Build();
                    builder.Services.AddSingleton<IChatCompletionService>(claude.AsChatCompletionService());
                    break;
                }
            }
        }
        catch (Exception ex)
        {
            log($"[ai] provider init failed: {ex.Message}");
        }

        var kernel = builder.Build();
        kernel.AutoFunctionInvocationFilters.Add(filter);
        // Register the assistant's tools (kernel functions).
        kernel.Plugins.AddFromObject(new BuitenzorgPlugin(s, log), "buitenzorg");
        kernel.Plugins.AddFromObject(new CommonPlugin(s, log), "common");
        return kernel;
    }

    /// <summary>Reports each tool call so the UI can show what Jack is doing.</summary>
    sealed class ToolCallFilter : IAutoFunctionInvocationFilter
    {
        public event Action<string>? Called;
        public async Task OnAutoFunctionInvocationAsync(AutoFunctionInvocationContext context,
                                                        Func<AutoFunctionInvocationContext, Task> next)
        {
            Called?.Invoke(context.Function.Name);
            await next(context);
        }
    }
}
