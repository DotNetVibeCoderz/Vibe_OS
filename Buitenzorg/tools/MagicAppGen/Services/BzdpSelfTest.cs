using System;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Threading.Tasks;
using MagicAppGen.Models;

namespace MagicAppGen.Services;

/// <summary><c>--bzdp-selftest</c>: the BZDP client against <see cref="FakeBzdpDevice"/>
/// on loopback, including a lossy link, a wrong pairing code and an unreachable port.</summary>
public static class BzdpSelfTest
{
    public static async Task<int> RunAsync()
    {
        int pass = 0, fail = 0;
        void Check(bool ok, string what)
        {
            if (ok) pass++; else fail++;
            Console.WriteLine($"  [{(ok ? "ok" : "FAIL")}] {what}");
        }

        // 1. A clean link, small chunks.
        Console.WriteLine("BZDP self-test: clean link");
        using (var dev = new FakeBzdpDevice(0, 4321) { MaxChunk = 512 })
        {
            _ = dev.RunAsync();
            using var c = new BzdpClient("127.0.0.1", dev.Port, 4321);
            var info = await c.HelloAsync();
            Check(info.Arch == "x86_64" && info.Hostname == "fake-device" && info.MaxChunk == 512 && c.MaxChunk == 512,
                  $"HELLO -> \"{info.Text}\"");
            var blob = new byte[200_003];
            new Random(1).NextBytes(blob);
            int reports = 0;
            await c.UploadAsync("/disk/BLOB.BIN", blob, new Progress<BzdpProgress>(_ => reports++));
            Check(dev.Files.TryGetValue("/disk/BLOB.BIN", out var got) && got.SequenceEqual(blob),
                  $"upload 200,003 bytes in {(blob.Length + 511) / 512} chunks, byte-identical");
            var pyfile = "print(\"halo\")\n"u8.ToArray();
            await c.UploadAsync("/disk/HALO.PY", pyfile);
            var list = await c.ListAsync("/disk");
            Check(list.Contains("BLOB.BIN") && list.Contains("HALO.PY"), $"LIST /disk -> {string.Join(", ", list)}");
            var pid0 = await c.RunAsync("script py /disk/HALO.PY");
            Check(pid0 == 0, "RUN script -> pid 0 (synchronous)");
            await c.UploadAsync("/disk/DEMO.DLL", new byte[3000]);
            var pid = await c.RunAsync("run demo --x");
            Check(pid > 0, $"RUN run demo -> pid {pid}");
            uint cursor = 0;
            string text = "";
            for (int i = 0; i < 20; i++)
            {
                var (next, t) = await c.LogAsync(cursor);
                text += t;
                if (next == cursor) break;
                cursor = next;
            }
            Check(text.Contains("$ run demo --x") && text.Contains("pid " + pid), $"LOG paged {text.Length} chars up to cursor {cursor}");
            await c.KillAsync(pid);
            Check(!dev.Running.ContainsKey(pid), "KILL pid");
            try { await c.KillAsync(pid); Check(false, "KILL unknown pid -> not found"); }
            catch (BzdpException e) { Check(e.Status == BzdpStatus.NotFound, $"KILL unknown pid -> {e.Status}"); }
            try { await c.RunAsync("run nothere"); Check(false, "RUN missing app -> not found"); }
            catch (BzdpException e) { Check(e.Status == BzdpStatus.NotFound, $"RUN missing app -> \"{e.Message}\""); }
        }

        // 2. A lossy link: a quarter of the requests vanish; retries + resend-from-offset must cope.
        Console.WriteLine("BZDP self-test: 25% of datagrams dropped");
        using (var dev = new FakeBzdpDevice(0, 0) { DropRate = 0.25 })
        {
            _ = dev.RunAsync();
            using var c = new BzdpClient("127.0.0.1", dev.Port, 0) { ReplyTimeout = TimeSpan.FromMilliseconds(120) };
            var blob = new byte[64 * 1024];
            new Random(2).NextBytes(blob);
            await c.UploadAsync("/disk/LOSSY.ELF", blob);
            Check(dev.Files.TryGetValue("/disk/LOSSY.ELF", out var got) && got.SequenceEqual(blob),
                  $"upload 64 KiB intact despite {dev.Dropped} dropped datagrams");
        }

        // 3. Wrong pairing code.
        Console.WriteLine("BZDP self-test: errors");
        using (var dev = new FakeBzdpDevice(0, 1111))
        {
            _ = dev.RunAsync();
            using var c = new BzdpClient("127.0.0.1", dev.Port, 2222);
            try { await c.HelloAsync(); Check(false, "wrong code rejected"); }
            catch (BzdpException e) { Check(e.Status == BzdpStatus.BadCode, $"wrong code -> \"{e.Message}\""); }
        }

        // 4. Nothing listening: a clear timeout message.
        int deadPort;
        using (var probe = new UdpClient(new IPEndPoint(IPAddress.Loopback, 0))) deadPort = ((IPEndPoint)probe.Client.LocalEndPoint!).Port;
        using (var c = new BzdpClient("127.0.0.1", deadPort, 0) { ReplyTimeout = TimeSpan.FromMilliseconds(100) })
        {
            var t0 = DateTime.UtcNow;
            try { await c.HelloAsync(); Check(false, "unreachable device times out"); }
            catch (BzdpTimeoutException e)
            {
                Check(e.Message.Contains("deploy on") && e.Message.Contains($"{deadPort}"),
                      $"unreachable -> after {(DateTime.UtcNow - t0).TotalMilliseconds:0} ms: \"{e.Message}\"");
            }
        }

        // 5. The full flow on real projects: build each script template and deploy + run it.
        Console.WriteLine("BZDP self-test: build + deploy + run (DeviceDeployer)");
        using (var dev = new FakeBzdpDevice(0, 7070))
        {
            _ = dev.RunAsync();
            var settings = Settings.Load();
            foreach (var id in new[] { "python", "ts", "js" })
            {
                var dir = Path.Combine(Path.GetTempPath(), "magicappgen-selftest", id);
                if (Directory.Exists(dir)) Directory.Delete(dir, true);
                ProjectTemplates.Scaffold(ProjectTemplates.Find(id)!, dir, "Uji " + id);
                var p = AppProject.Open(dir)!;
                var r = await new ProjectBuilder(settings, _ => { }).BuildAsync(p);
                using var d = new DeviceDeployer(new DeviceProfile { Name = "fake", Host = "127.0.0.1", Port = dev.Port, Code = 7070 }, _ => { });
                await d.UploadAsync(r.Files);
                var pid = await d.RunAsync(p.RunCommand());
                var log = await d.PollLogAsync();
                Check(r.Ok && r.Warnings.Count == 0 && dev.Files.ContainsKey(p.RemotePath) && pid == 0 && log.Contains(p.RemotePath),
                      $"{id}: {p.RemotePath}, `{p.RunCommand()}`, {r.Warnings.Count} subset warning(s)");
            }
        }

        Console.WriteLine($"BZDP self-test: {pass} passed, {fail} failed");
        return fail == 0 ? 0 : 1;
    }
}
