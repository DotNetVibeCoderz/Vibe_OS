using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using MagicAppGen.Models;

namespace MagicAppGen.Services;

/// <summary>Outcome of a build: the files to deploy (local → /disk path) or what went wrong.</summary>
public sealed record BuildResult(bool Ok, string Message, IReadOnlyList<DeployFile> Files, IReadOnlyList<string> Warnings)
{
    public static BuildResult Fail(string message) => new(false, message, Array.Empty<DeployFile>(), Array.Empty<string>());
}

/// <summary>
/// Builds an <see cref="AppProject"/> into <c>&lt;project&gt;/out</c> without
/// touching the Buitenzorg checkout:
/// native C# = bflat --stdlib:zero + rust-lld with the bzstart shim (x64, or
/// arm64 with the patched bflat copy the ARM64 build script prepares);
/// managed C# = dotnet build, the assembly copied as NAME.DLL (plus
/// BZAPP.DLL / BZUI.DLL when asked); scripts = a subset check and a copy.
/// </summary>
public sealed class ProjectBuilder
{
    readonly Settings _settings;
    readonly Action<string> _log;

    public ProjectBuilder(Settings settings, Action<string> log) { _settings = settings; _log = log; }

    string Root => string.IsNullOrWhiteSpace(_settings.BuitenzorgRoot) ? Directory.GetCurrentDirectory() : _settings.BuitenzorgRoot;

    public Task<BuildResult> BuildAsync(AppProject p, CancellationToken ct = default) => p.Kind switch
    {
        AppKind.Native => Task.Run(() => BuildNative(p, ct), ct),
        AppKind.Managed => BuildManagedAsync(p, ct),
        _ => Task.FromResult(BuildScript(p)),
    };

    // ---- scripts ------------------------------------------------------------

    BuildResult BuildScript(AppProject p)
    {
        Directory.CreateDirectory(p.OutDir);
        var src = File.ReadAllText(p.MainFile);
        var warnings = AppProject.CheckScriptSubset(p.ScriptLang, src);
        foreach (var w in warnings) _log($"[build] warning {Path.GetFileName(p.MainFile)} {w}");
        var outFile = Path.Combine(p.OutDir, $"{p.Name83}.{p.ScriptLang.ToUpperInvariant()}");
        // The device keeps scripts as plain text; normalise line endings for the interpreter.
        File.WriteAllText(outFile, src.Replace("\r\n", "\n"), new UTF8Encoding(false));
        _log($"[build] {p.KindLabel}: {Path.GetFileName(outFile)} ({warnings.Count} warning(s))");
        return new BuildResult(true, $"{Path.GetFileName(outFile)} siap", new[] { new DeployFile(outFile, p.RemotePath) }, warnings);
    }

    // ---- managed ------------------------------------------------------------

    async Task<BuildResult> BuildManagedAsync(AppProject p, CancellationToken ct)
    {
        var csproj = Directory.GetFiles(p.Dir, "*.csproj").FirstOrDefault();
        if (csproj is null) return BuildResult.Fail("no .csproj in the project");
        var props = $"-p:BuitenzorgRoot=\"{Root.TrimEnd('\\', '/')}\"";
        var (code, output) = await RunAsync("dotnet", $"build \"{csproj}\" -c Release -nologo -v q {props}", p.Dir, ct);
        if (code != 0) return BuildResult.Fail("dotnet build failed:\n" + Tail(ErrorLines(output)));

        var asm = Path.GetFileNameWithoutExtension(csproj);
        var x = System.Xml.Linq.XDocument.Load(csproj);
        asm = x.Descendants().FirstOrDefault(e => e.Name.LocalName == "AssemblyName")?.Value?.Trim() ?? asm;
        var bin = Directory.GetDirectories(Path.Combine(p.Dir, "bin", "Release")).OrderByDescending(d => d).FirstOrDefault();
        var dll = bin is null ? null : Path.Combine(bin, asm + ".dll");
        if (dll is null || !File.Exists(dll)) return BuildResult.Fail($"build output {asm}.dll not found under bin/Release");

        Directory.CreateDirectory(p.OutDir);
        var files = new List<DeployFile>();
        var outDll = Path.Combine(p.OutDir, p.Name83 + ".DLL");
        File.Copy(dll, outDll, true);
        files.Add(new DeployFile(outDll, p.RemotePath));
        if (_settings.DeployRuntimeLibraries)
        {
            foreach (var (lib, name) in new[] { ("Buitenzorg.App", "BZAPP.DLL"), ("Buitenzorg.UI", "BZUI.DLL") })
            {
                var built = Path.Combine(bin!, lib + ".dll");
                var dist = Path.Combine(Root, "userland", "clr-apps", "dist", name);
                var src = File.Exists(built) ? built : File.Exists(dist) ? dist : null;
                if (src is null) continue;
                var dst = Path.Combine(p.OutDir, name);
                File.Copy(src, dst, true);
                files.Add(new DeployFile(dst, "/disk/" + name));
            }
        }
        var warns = ErrorLines(output).Split('\n').Where(l => l.Contains(": warning ")).Distinct().Take(20).ToList();
        _log($"[build] {p.KindLabel}: {Path.GetFileName(outDll)} ({new FileInfo(outDll).Length:N0} byte)");
        return new BuildResult(true, $"{Path.GetFileName(outDll)} siap", files, warns);
    }

    /// <summary>Run a managed app's own self-test on the host (<c>dotnet X.dll --selftest</c>):
    /// off the device Buitenzorg.App falls back to simulated files/processes/board.</summary>
    public async Task<(bool Ok, string Output)> SelfTestManagedAsync(AppProject p, CancellationToken ct = default)
    {
        var bin = Directory.GetDirectories(Path.Combine(p.Dir, "bin", "Release")).OrderByDescending(d => d).FirstOrDefault();
        var csproj = Directory.GetFiles(p.Dir, "*.csproj").First();
        var asm = System.Xml.Linq.XDocument.Load(csproj).Descendants().FirstOrDefault(e => e.Name.LocalName == "AssemblyName")?.Value?.Trim()
                  ?? Path.GetFileNameWithoutExtension(csproj);
        var dll = bin is null ? "" : Path.Combine(bin, asm + ".dll");
        if (!File.Exists(dll)) return (false, "build the project first");
        var (code, output) = await RunAsync("dotnet", $"\"{dll}\" --selftest", p.Dir, ct, TimeSpan.FromMinutes(2));
        return (code == 0, output.Trim());
    }

    // ---- native -------------------------------------------------------------

    BuildResult BuildNative(AppProject p, CancellationToken ct)
    {
        bool arm = _settings.NativeArch.Equals("arm64", StringComparison.OrdinalIgnoreCase);
        var userland = Path.Combine(Root, "userland", "hello-csharp");
        var bflat = arm
            ? Path.Combine(Root, "kernel", "target", "bflat-arm64", OperatingSystem.IsWindows() ? "bflat.exe" : "bflat")
            : Path.Combine(Root, "tools", "bflat", OperatingSystem.IsWindows() ? "bflat.exe" : "bflat");
        if (!File.Exists(bflat))
            return BuildResult.Fail(arm
                ? "the patched ARM64 bflat is missing: run scripts/build-hello-csharp-arm64.ps1 once (it prepares kernel/target/bflat-arm64)"
                : "bflat not found under <repo>/tools/bflat (run scripts/quickstart.ps1, or set the Buitenzorg root in Settings)");
        var lld = FindRustLld();
        if (lld is null) return BuildResult.Fail("rust-lld not found (install the rust nightly toolchain)");

        // The startup/PAL shim, compiled once per architecture into a cache outside the repo.
        var cache = Path.Combine(Path.GetTempPath(), "magicappgen");
        Directory.CreateDirectory(cache);
        var shimSrc = Path.Combine(userland, "bzstart.rs");
        var shim = Path.Combine(cache, $"bzstart-{(arm ? "arm64" : "x64")}.o");
        if (!File.Exists(shim) || File.GetLastWriteTimeUtc(shim) < File.GetLastWriteTimeUtc(shimSrc))
        {
            var target = arm ? "aarch64-unknown-none" : "x86_64-unknown-none";
            var (rc, ro) = Run("rustc", $"+nightly --edition 2021 --crate-type staticlib --emit obj --target {target} " +
                                        $"-C panic=abort -C opt-level=2 -o \"{shim}\" \"{shimSrc}\"", userland, ct);
            if (rc != 0) return BuildResult.Fail("building the bzstart shim failed:\n" + Tail(ro));
        }

        var sources = NativeSources(p.Dir, userland);
        Directory.CreateDirectory(p.OutDir);
        var obj = Path.Combine(cache, $"{p.Name83}-{Guid.NewGuid():N}.o");
        var elf = Path.Combine(p.OutDir, p.Name83 + ".ELF");
        _log($"[build] bflat {(arm ? "arm64" : "x64")}: {sources.Count} source(s)");
        var (cc, co) = Run(bflat, "build " + string.Join(' ', sources.Select(s => $"\"{s}\"")) +
                                  $" --stdlib:zero --os:linux --arch:{(arm ? "arm64" : "x64")} -c -Os --no-debug-info " +
                                  $"--no-reflection --no-stacktrace-data -o \"{obj}\"", userland, ct);
        if (cc != 0) return BuildResult.Fail("COMPILE ERRORS:\n" + Tail(co));
        try
        {
            var (lc, lo) = Run(lld, $"-flavor gnu -o \"{elf}\" -T \"{Path.Combine(userland, "user.ld")}\" " +
                                    $"--static --no-dynamic-linker -e _start \"{obj}\" \"{shim}\"", userland, ct);
            if (lc != 0) return BuildResult.Fail("LINK ERRORS:\n" + Tail(lo));
        }
        finally { try { File.Delete(obj); } catch { } }
        _log($"[build] {p.KindLabel}: {Path.GetFileName(elf)} ({new FileInfo(elf).Length:N0} byte)");
        return new BuildResult(true, $"{Path.GetFileName(elf)} siap", new[] { new DeployFile(elf, p.RemotePath) }, Array.Empty<string>());
    }

    /// <summary>The project's .cs files plus the Buitenzorg library sources its usings need.</summary>
    public static List<string> NativeSources(string projectDir, string userland)
    {
        var mine = Directory.GetFiles(projectDir, "*.cs").Select(Path.GetFullPath).ToList();
        var all = string.Concat(mine.Select(File.ReadAllText));
        var sources = new List<string>(mine);
        void Need(string marker, params string[] files)
        {
            if (!all.Contains(marker)) return;
            foreach (var f in files)
            {
                var path = Path.GetFullPath(Path.Combine(userland, f));
                if (File.Exists(path) && !sources.Contains(path) &&
                    !mine.Any(m => Path.GetFileName(m).Equals(f, StringComparison.OrdinalIgnoreCase)))
                    sources.Add(path);
            }
        }
        Need("Buitenzorg.Drawing", "bzgfx.cs");
        Need("Buitenzorg.UI", "bzui.cs", "bzgfx.cs");
        Need("Buitenzorg.Audio", "bzaudio.cs");
        Need("Buitenzorg.Devices", "bzhw.cs", "bzbcl.cs");
        Need("using Buitenzorg;", "bzbcl.cs", "bzbcl2.cs");
        return sources;
    }

    public static string? FindRustLld()
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var toolchains = Path.Combine(home, ".rustup", "toolchains");
        if (!Directory.Exists(toolchains)) return null;
        var name = OperatingSystem.IsWindows() ? "rust-lld.exe" : "rust-lld";
        foreach (var tc in Directory.GetDirectories(toolchains).OrderByDescending(d => d.Contains("nightly")))
        {
            try
            {
                var hit = Directory.GetFiles(tc, name, SearchOption.AllDirectories).FirstOrDefault();
                if (hit != null) return hit;
            }
            catch { }
        }
        return null;
    }

    // ---- process helpers ----------------------------------------------------

    static string Tail(string s) => s.Length > 2500 ? s[^2500..] : s;

    /// <summary>Keep the diagnostic lines of an MSBuild log (errors first).</summary>
    static string ErrorLines(string output)
    {
        var lines = output.Replace("\r", "").Split('\n');
        var errs = lines.Where(l => l.Contains(": error ") || l.Contains(": warning ")).Distinct().ToList();
        return errs.Count > 0 ? string.Join('\n', errs) : output;
    }

    static (int Code, string Output) Run(string file, string args, string cwd, CancellationToken ct) =>
        RunAsync(file, args, cwd, ct).GetAwaiter().GetResult();

    public static async Task<(int Code, string Output)> RunAsync(string file, string args, string cwd, CancellationToken ct,
                                                                TimeSpan? timeout = null)
    {
        var psi = new ProcessStartInfo
        {
            FileName = file, Arguments = args, WorkingDirectory = cwd,
            RedirectStandardOutput = true, RedirectStandardError = true,
            UseShellExecute = false, CreateNoWindow = true,
        };
        psi.Environment["DOTNET_CLI_UI_LANGUAGE"] = "en";
        using var proc = Process.Start(psi) ?? throw new InvalidOperationException($"cannot start {file}");
        var o = proc.StandardOutput.ReadToEndAsync(ct);
        var e = proc.StandardError.ReadToEndAsync(ct);
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        cts.CancelAfter(timeout ?? TimeSpan.FromMinutes(5));
        try { await proc.WaitForExitAsync(cts.Token); }
        catch (OperationCanceledException)
        {
            try { proc.Kill(true); } catch { }
            return (-1, $"{Path.GetFileName(file)} timed out or was cancelled");
        }
        return (proc.ExitCode, (await e + "\n" + await o).Trim());
    }
}
