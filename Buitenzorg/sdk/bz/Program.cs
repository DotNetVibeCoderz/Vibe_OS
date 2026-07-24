// bz — the Buitenzorg OS CLI (requirements.md §14.2).
// Implemented today: `bz new`, `bz manifest validate`, `bz version`.
// Other subcommands are stubs that report which roadmap version delivers them.

using Buitenzorg.Runtime.Apps;

return args switch
{
    [] or ["help"] or ["--help"] or ["-h"] => Help(),
    ["version"] or ["--version"] => Version(),
    ["new", var template, var name] => NewProject(template, name),
    ["new", ..] => Fail("usage: bz new <template> <name>   (templates: console-csharp, desktop-csharp, js-app, ts-app, python-app)"),
    ["manifest", "validate", var path] => ValidateManifest(path),
    ["manifest", ..] => Fail("usage: bz manifest validate <path-to-app.manifest>"),
    ["app", ..] => Stub("app", "v0.10 'Buah' (package manager + registry)"),
    ["theme", ..] => Stub("theme", "v0.10 'Buah' (theme engine, 8 built-in styles)"),
    ["model", ..] => Stub("model", "v0.12 'Nalar' (Model Manager + Hugging Face gallery)"),
    ["vm", ..] => Stub("vm", "v0.13 'Lapis' (type-2 hypervisor + virtualization manager)"),
    _ => Fail($"unknown command '{args[0]}' — try: bz help"),
};

static int Help()
{
    Console.WriteLine("""
        bz — Buitenzorg OS CLI

        commands:
          bz new <template> <name>          scaffold an app from an SDK template
          bz manifest validate <file>       validate an app.manifest
          bz app|theme|model|vm ...         planned (see roadmap in requirements.md §16)
          bz version                        show version
        """);
    return 0;
}

static int Version()
{
    Console.WriteLine("bz 0.8.0 'Kembang' — Buitenzorg OS SDK CLI");
    return 0;
}

static int NewProject(string template, string name)
{
    var templateRoot = FindTemplateRoot();
    if (templateRoot is null)
        return Fail("could not locate the sdk/templates directory");

    var source = Path.Combine(templateRoot, template);
    if (!Directory.Exists(source))
    {
        var available = Directory.GetDirectories(templateRoot).Select(Path.GetFileName);
        return Fail($"unknown template '{template}'. available: {string.Join(", ", available)}");
    }

    var target = Path.GetFullPath(name);
    if (Directory.Exists(target))
        return Fail($"directory already exists: {target}");

    CopyTree(source, target);

    // Personalize the manifest.
    var manifestPath = Path.Combine(target, "app.manifest");
    if (File.Exists(manifestPath))
    {
        var manifest = AppManifest.Load(manifestPath);
        manifest.Id = $"local.{name.ToLowerInvariant().Replace(' ', '-')}";
        manifest.Name = name;
        File.WriteAllText(manifestPath, manifest.ToJson());
    }

    Console.WriteLine($"created {template} app in {target}");
    Console.WriteLine("next: cd into it and read README.md");
    return 0;
}

static int ValidateManifest(string path)
{
    if (!File.Exists(path))
        return Fail($"file not found: {path}");

    AppManifest manifest;
    try
    {
        manifest = AppManifest.Load(path);
    }
    catch (Exception ex)
    {
        return Fail($"not valid JSON: {ex.Message}");
    }

    var problems = manifest.Validate();
    if (problems.Count == 0)
    {
        Console.WriteLine($"OK: {manifest.Id} ({manifest.Type}, {manifest.Language}, v{manifest.Version})");
        return 0;
    }
    foreach (var p in problems)
        Console.Error.WriteLine($"invalid: {p}");
    return 1;
}

static string? FindTemplateRoot()
{
    // Walk up from the executable and the CWD looking for sdk/templates.
    foreach (var start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
    {
        for (var dir = new DirectoryInfo(start); dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, "sdk", "templates");
            if (Directory.Exists(candidate))
                return candidate;
        }
    }
    return null;
}

static void CopyTree(string source, string target)
{
    Directory.CreateDirectory(target);
    foreach (var dir in Directory.GetDirectories(source, "*", SearchOption.AllDirectories))
        Directory.CreateDirectory(dir.Replace(source, target));
    foreach (var file in Directory.GetFiles(source, "*", SearchOption.AllDirectories))
        File.Copy(file, file.Replace(source, target));
}

static int Stub(string command, string roadmap)
{
    Console.WriteLine($"bz {command}: not implemented yet — arrives with {roadmap}.");
    return 2;
}

static int Fail(string message)
{
    Console.Error.WriteLine($"bz: {message}");
    return 1;
}
