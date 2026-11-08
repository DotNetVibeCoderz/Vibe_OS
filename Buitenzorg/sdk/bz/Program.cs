// bz — the Buitenzorg OS CLI (requirements.md §14.2).
// Implemented today: `bz new`, `bz manifest validate`, `bz pack`, `bz inspect`,
// `bz version`, and the device commands over BZDP (Device.cs): `bz device`,
// `ls`, `push`, `pull`, `run`, `shell`, `logs`, `kill`, `deploy`.
// Other subcommands are stubs that report which roadmap version delivers them.

using Buitenzorg.Runtime.Apps;

if (Device.Run(args) is int deviceResult)
    return deviceResult;

return args switch
{
    ["deploy", .. var rest] when rest.Length >= 1 => DeployCmd(rest),
    ["deploy"] => Fail("usage: bz deploy <file|project-dir> [--as NAME.EXT] [--no-run] [--follow SECONDS]"),
    [] or ["help"] or ["--help"] or ["-h"] => Help(),
    ["version"] or ["--version"] => Version(),
    ["new", var template, var name] => NewProject(template, name),
    ["new", ..] => Fail("usage: bz new <template> <name>   (templates: console-csharp, desktop-csharp, js-app, ts-app, python-app)"),
    ["manifest", "validate", var path] => ValidateManifest(path),
    ["manifest", ..] => Fail("usage: bz manifest validate <path-to-app.manifest>"),
    ["pack", .. var rest] when rest.Length >= 1 => Pack(rest),
    ["pack"] => Fail("usage: bz pack <project-dir> [--entry FILE] [--name NAME] [-o OUT.bzpkg]"),
    ["inspect", var file] => Inspect(file),
    ["repo", .. var rest] => RepoCli.Run(rest),
    ["publish", .. var rest] when rest.Length >= 1 => RepoCli.Run(["publish", .. rest]),
    ["build-server", .. var rest] => BuildServer.Run(rest),
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
          bz pack <dir> [--entry F] [-o X] [--depends a,b]
                                            build a .bzpkg from <dir>/app.manifest + <dir>/package/*
          bz repo serve DIR [--port N]      serve a package repository (INDEX.TXT + .bzpkg files)
          bz repo index DIR                 (re)write DIR/INDEX.TXT
          bz publish PKG [--repo DIR]       put a package into a repository folder (+ index)
          bz build-server [--port N]        compile C# for devices (they send .CS, get a .dll for RustCLR)
          bz inspect <file.bzpkg>           list and verify a package

        a running device (BZDP over UDP 7070; on the device: 'deploy on [code]'):
          bz device connect <host[:port]> <code>   remember the device (~/.bz/device.json)
          bz device [info|forget]           say hello / forget it
          bz deploy <file|dir> [--as N] [--no-run] [--follow S]
                                            upload to /disk and start it: .bzpkg -> bz install,
                                            .ELF/.DLL/.JS/.TS/.PY/.CSX -> run, .SH -> sh;
                                            a project dir is packed (bz pack) first
          bz push <local> <remote>          upload a file (/disk/X.EXT or /disk/)
          bz pull <remote> [local]          download a file
          bz ls [path]                      list a device folder
          bz run <command line>             run a shell line on the device, print its output
          bz shell                          an interactive device shell
          bz logs [-f]                      the device's console log (follow with -f)
          bz prof APP [args] [--seconds N]  profile a program on the device (functions by samples)
          bz prof --pid N [--seconds N]     profile a running process
          bz dap                            debug adapter for VS Code (DAP on stdio)
          bz kill <pid>                     stop a process

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

// ---- .bzpkg (format v1; kernel side: kernel/bzkernel/src/bzpkg.rs) ----------

static bool Is83(string n)
{
    var parts = n.Split('.');
    static bool Ok(string s, int max) => s.Length >= 1 && s.Length <= max && s.All(c => c is >= 'A' and <= 'Z' or >= '0' and <= '9' or '_' or '-');
    return parts.Length switch { 1 => Ok(parts[0], 8), 2 => Ok(parts[0], 8) && Ok(parts[1], 3), _ => false };
}

static int Pack(string[] args)
{
    string dir = args[0];
    string? entry = null, outPath = null, nameOverride = null, depends = null;
    for (int i = 1; i < args.Length; i++)
    {
        switch (args[i])
        {
            case "--entry" when i + 1 < args.Length: entry = args[++i].ToUpperInvariant(); break;
            case "--name" when i + 1 < args.Length: nameOverride = args[++i]; break;
            case "-o" when i + 1 < args.Length: outPath = args[++i]; break;
            case "--depends" when i + 1 < args.Length: depends = args[++i]; break;
            default: return Fail($"unknown option '{args[i]}'");
        }
    }
    var manifestPath = Path.Combine(dir, "app.manifest");
    if (!File.Exists(manifestPath))
        return Fail($"no app.manifest in {dir}");
    var m = AppManifest.Load(manifestPath);
    var problems = m.Validate();
    if (problems.Count > 0)
        return Fail("app.manifest is invalid: " + string.Join("; ", problems));
    string name = (nameOverride ?? m.Id.Split('.').Last()).ToLowerInvariant();
    if (name.Length is 0 or > 23 || !name.All(c => c is >= 'a' and <= 'z' or >= '0' and <= '9' or '-'))
        return Fail($"package name '{name}' must be 1-23 of a-z 0-9 - (use --name)");
    var payload = Path.Combine(dir, "package");
    if (!Directory.Exists(payload))
        return Fail($"put the files to ship in {payload} (8.3 names: the ELF/DLL/script and assets)");
    var files = Directory.GetFiles(payload).OrderBy(f => f, StringComparer.Ordinal).ToList();
    if (files.Count is 0 or > 64)
        return Fail($"{payload} must hold 1-64 files");
    foreach (var f in files)
        if (!Is83(Path.GetFileName(f).ToUpperInvariant()))
            return Fail($"{Path.GetFileName(f)} is not an 8.3 name (FAT on the device)");
    entry ??= files.Select(f => Path.GetFileName(f).ToUpperInvariant())
                   .FirstOrDefault(f => f.EndsWith(".ELF") || f.EndsWith(".DLL") || f.EndsWith(".SH"));
    if (entry is not null && !files.Any(f => Path.GetFileName(f).ToUpperInvariant() == entry))
        return Fail($"entry {entry} is not in {payload}");

    string category = m.Type switch { "game" => "GAME", "widget" => "UTILITAS", "web" => "INTERNET", _ => "LAINNYA" };
    string manifest = $"name={name}\nversion={m.Version}\ntitle={m.Name}\nkind={m.Type}\ncategory={category}\nentry={entry ?? ""}\n"
        + (string.IsNullOrWhiteSpace(depends) ? "" : $"depends={depends.Replace(" ", "")}\n");
    using var ms = new MemoryStream();
    using (var w = new BinaryWriter(ms, System.Text.Encoding.UTF8, leaveOpen: true))
    {
        w.Write("BZPK"u8.ToArray());
        w.Write((ushort)1);
        w.Write((ushort)files.Count);
        var mb = System.Text.Encoding.UTF8.GetBytes(manifest);
        w.Write((uint)mb.Length);
        w.Write(mb);
        foreach (var f in files)
        {
            var fname = System.Text.Encoding.ASCII.GetBytes(Path.GetFileName(f).ToUpperInvariant());
            var data = File.ReadAllBytes(f);
            w.Write((byte)fname.Length);
            w.Write(fname);
            w.Write((uint)data.Length);
            w.Write(System.Security.Cryptography.SHA256.HashData(data));
            w.Write(data);
        }
    }
    var body = ms.ToArray();
    var bytes = body.Concat(System.Security.Cryptography.SHA256.HashData(body)).ToArray();
    bool hint = outPath is null; // bz deploy passes -o and uploads it itself
    outPath ??= name + ".bzpkg";
    File.WriteAllBytes(outPath, bytes);
    Console.WriteLine($"wrote {outPath}: {name} {m.Version}, {files.Count} file(s), entry {entry ?? "-"}, {bytes.Length} bytes");
    if (hint) Console.WriteLine($"on the device: copy it to /disk as an 8.3 name (e.g. {name.ToUpperInvariant()[..Math.Min(8, name.Length)]}.BZP), then 'bz install /disk/<that name>'");
    return 0;
}

static int DeployCmd(string[] args)
{
    string target = args[0];
    string? asName = null;
    bool run = true;
    int follow = 5;
    for (int i = 1; i < args.Length; i++)
    {
        switch (args[i])
        {
            case "--as" when i + 1 < args.Length: asName = args[++i]; break;
            case "--no-run": run = false; break;
            case "--follow" when i + 1 < args.Length && int.TryParse(args[i + 1], out var f): follow = f; i++; break;
            default: return Fail($"unknown option '{args[i]}'");
        }
    }
    if (Directory.Exists(target))
    {
        // a project: pack it first (app.manifest + package/*)
        string stem = new(Path.GetFileName(Path.GetFullPath(target).TrimEnd('/', '\\')).Where(char.IsAsciiLetterOrDigit).Take(8).ToArray());
        if (stem.Length == 0) stem = "APP";
        string tmp = Path.Combine(Path.GetTempPath(), stem.ToUpperInvariant() + ".BZP");
        int r = Pack(new[] { target, "-o", tmp });
        if (r != 0) return r;
        return Device.Deploy(tmp, asName, run, follow);
    }
    if (!File.Exists(target)) return Fail($"not found: {target}");
    return Device.Deploy(target, asName, run, follow);
}

static int Inspect(string path)
{
    if (!File.Exists(path))
        return Fail($"file not found: {path}");
    var b = File.ReadAllBytes(path);
    if (b.Length < 44 || b[0] != 'B' || b[1] != 'Z' || b[2] != 'P' || b[3] != 'K')
        return Fail("not a .bzpkg (no BZPK magic)");
    var body = b.AsSpan(0, b.Length - 32);
    bool whole = System.Security.Cryptography.SHA256.HashData(body).AsSpan().SequenceEqual(b.AsSpan(b.Length - 32));
    int p = 4;
    ushort ver = BitConverter.ToUInt16(b, p); p += 2;
    ushort count = BitConverter.ToUInt16(b, p); p += 2;
    int mlen = (int)BitConverter.ToUInt32(b, p); p += 4;
    Console.WriteLine($"format v{ver}, {count} file(s), archive checksum {(whole ? "ok" : "WRONG")}");
    Console.Write(System.Text.Encoding.UTF8.GetString(b, p, mlen)); p += mlen;
    bool allOk = whole;
    for (int i = 0; i < count; i++)
    {
        int n = b[p++];
        string name = System.Text.Encoding.ASCII.GetString(b, p, n); p += n;
        int size = (int)BitConverter.ToUInt32(b, p); p += 4;
        var hash = b.AsSpan(p, 32); p += 32;
        bool ok = System.Security.Cryptography.SHA256.HashData(b.AsSpan(p, size)).AsSpan().SequenceEqual(hash);
        p += size;
        allOk &= ok;
        Console.WriteLine($"  {name,-12} {size,9} B  sha256 {(ok ? "ok" : "WRONG")}");
    }
    return allOk ? 0 : 1;
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
