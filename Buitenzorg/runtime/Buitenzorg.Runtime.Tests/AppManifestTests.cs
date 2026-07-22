using Buitenzorg.Runtime.Apps;

namespace Buitenzorg.Runtime.Tests;

public class AppManifestTests
{
    private const string SpecExample = """
        {
          "id": "com.example.myapp",
          "name": "My App",
          "type": "desktop",
          "language": "csharp",
          "version": "1.0.0",
          "permissions": ["filesystem.read", "network", "ai.llm", "camera"],
          "theme": "system"
        }
        """;

    [Fact]
    public void ParsesTheSpecExample()
    {
        var m = AppManifest.Parse(SpecExample);
        Assert.Equal("com.example.myapp", m.Id);
        Assert.Equal("desktop", m.Type);
        Assert.Equal("csharp", m.Language);
        Assert.Empty(m.Validate());
    }

    [Fact]
    public void RejectsInvalidTypeLanguageAndPermission()
    {
        var m = AppManifest.Parse(SpecExample);
        m.Type = "daemon";
        m.Language = "cobol";
        m.Permissions.Add("root.everything");
        var problems = m.Validate();
        Assert.Equal(3, problems.Count);
    }

    [Fact]
    public void RoundTripsThroughJson()
    {
        var m = AppManifest.Parse(SpecExample);
        var again = AppManifest.Parse(m.ToJson());
        Assert.Equal(m.Id, again.Id);
        Assert.Equal(m.Permissions, again.Permissions);
    }
}
