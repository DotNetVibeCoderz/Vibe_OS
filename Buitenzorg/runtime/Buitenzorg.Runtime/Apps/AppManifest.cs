using System.Text.Json;
using System.Text.Json.Serialization;

namespace Buitenzorg.Runtime.Apps;

/// <summary>
/// Unified app manifest (requirements.md §11): one model for all four app
/// variants (console/desktop/web/widget) and all languages (C#/JS/TS/Python).
/// </summary>
public sealed class AppManifest
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = "";

    [JsonPropertyName("name")]
    public string Name { get; set; } = "";

    /// <summary>console | desktop | web | widget</summary>
    [JsonPropertyName("type")]
    public string Type { get; set; } = "console";

    /// <summary>csharp | js | ts | python</summary>
    [JsonPropertyName("language")]
    public string Language { get; set; } = "csharp";

    [JsonPropertyName("version")]
    public string Version { get; set; } = "0.1.0";

    [JsonPropertyName("permissions")]
    public List<string> Permissions { get; set; } = [];

    /// <summary>system | dark | light | a theme id</summary>
    [JsonPropertyName("theme")]
    public string Theme { get; set; } = "system";

    public static readonly string[] ValidTypes = ["console", "desktop", "web", "widget"];
    public static readonly string[] ValidLanguages = ["csharp", "js", "ts", "python"];

    /// <summary>Known permission ids (§11); grows with the platform.</summary>
    public static readonly string[] KnownPermissions =
    [
        "filesystem.read", "filesystem.write", "network",
        "ai.llm", "ai.vision", "ai.genai",
        "camera", "microphone", "gallery",
    ];

    public static AppManifest Load(string path)
        => Parse(File.ReadAllText(path));

    public static AppManifest Parse(string json)
        => JsonSerializer.Deserialize(json, ManifestJsonContext.Default.AppManifest)
           ?? throw new InvalidDataException("app.manifest is empty");

    public string ToJson()
        => JsonSerializer.Serialize(this, ManifestJsonContext.Default.AppManifest);

    /// <summary>Validate the manifest; returns human-readable problems (empty = valid).</summary>
    public IReadOnlyList<string> Validate()
    {
        var problems = new List<string>();
        if (string.IsNullOrWhiteSpace(Id) || !Id.Contains('.'))
            problems.Add("id: required, reverse-DNS style (e.g. com.example.myapp)");
        if (string.IsNullOrWhiteSpace(Name))
            problems.Add("name: required");
        if (!ValidTypes.Contains(Type))
            problems.Add($"type: '{Type}' is not one of: {string.Join(", ", ValidTypes)}");
        if (!ValidLanguages.Contains(Language))
            problems.Add($"language: '{Language}' is not one of: {string.Join(", ", ValidLanguages)}");
        if (!System.Version.TryParse(Version, out _))
            problems.Add($"version: '{Version}' is not a valid version number");
        foreach (var p in Permissions.Where(p => !KnownPermissions.Contains(p)))
            problems.Add($"permissions: unknown permission '{p}'");
        return problems;
    }
}

[JsonSourceGenerationOptions(WriteIndented = true)]
[JsonSerializable(typeof(AppManifest))]
public sealed partial class ManifestJsonContext : JsonSerializerContext;
