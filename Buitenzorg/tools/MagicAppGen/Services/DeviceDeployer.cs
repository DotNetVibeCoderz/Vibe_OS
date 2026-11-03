using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using MagicAppGen.Models;

namespace MagicAppGen.Services;

/// <summary>
/// The "build → deploy → run → watch" flow over BZDP, shared by the Deploy
/// panel, the headless <c>--deploy</c> mode and Jack's DeployToDevice tool.
/// </summary>
public sealed class DeviceDeployer : IDisposable
{
    readonly BzdpClient _client;
    readonly Action<string> _log;
    uint _logCursor;

    public DeviceProfile Device { get; }
    public BzdpDeviceInfo? Info { get; private set; }
    public uint LastPid { get; private set; }

    public DeviceDeployer(DeviceProfile device, Action<string> log)
    {
        Device = device;
        _log = log;
        _client = new BzdpClient(device.Host, device.Port, device.Code) { Trace = log };
    }

    public void Dispose() => _client.Dispose();

    public async Task<BzdpDeviceInfo> ConnectAsync(CancellationToken ct = default)
    {
        Info = await _client.HelloAsync(ct);
        _log($"[device] {Info.Text}");
        return Info;
    }

    /// <summary>Upload every built file. Reports overall progress 0..1.</summary>
    public async Task UploadAsync(IReadOnlyList<DeployFile> files, IProgress<(string File, double Fraction)>? progress = null,
                                  CancellationToken ct = default)
    {
        if (Info is null) await ConnectAsync(ct);
        long total = 0, done = 0;
        var blobs = new List<(DeployFile F, byte[] Bytes)>();
        foreach (var f in files) { var b = await File.ReadAllBytesAsync(f.LocalPath, ct); blobs.Add((f, b)); total += b.Length; }
        foreach (var (f, bytes) in blobs)
        {
            long before = done;
            var p = new Progress<BzdpProgress>(x =>
                progress?.Report((f.RemotePath, total == 0 ? 1 : (before + x.Sent) / (double)total)));
            var t0 = DateTime.UtcNow;
            await _client.UploadAsync(f.RemotePath, bytes, p, ct);
            done += bytes.Length;
            var secs = Math.Max(0.001, (DateTime.UtcNow - t0).TotalSeconds);
            _log($"[deploy] {f.RemotePath}: {bytes.Length:N0} byte in {secs:0.0} s ({bytes.Length / 1024.0 / secs:0} KiB/s)");
            progress?.Report((f.RemotePath, total == 0 ? 1 : done / (double)total));
        }
    }

    public async Task<uint> RunAsync(string commandLine, CancellationToken ct = default)
    {
        // Start the log from "now" so the panel shows this run's output, not the whole boot log.
        if (_logCursor == 0) await SkipLogAsync(ct);
        LastPid = await _client.RunAsync(commandLine, ct);
        _log(LastPid == 0 ? $"[run] {commandLine} (selesai di shell)" : $"[run] {commandLine} -> pid {LastPid}");
        return LastPid;
    }

    /// <summary>Move the log cursor to the end of what the device has printed so far.</summary>
    public async Task SkipLogAsync(CancellationToken ct = default)
    {
        for (int i = 0; i < 4096; i++)
        {
            var (next, _) = await _client.LogAsync(_logCursor, ct);
            if (next == _logCursor) return;
            _logCursor = next;
        }
    }

    /// <summary>Fetch new console output (all chunks available right now).</summary>
    public async Task<string> PollLogAsync(CancellationToken ct = default)
    {
        var sb = new System.Text.StringBuilder();
        for (int i = 0; i < 64; i++)
        {
            var (next, text) = await _client.LogAsync(_logCursor, ct);
            if (next < _logCursor) { _logCursor = 0; continue; } // device rebooted: its ring buffer started over
            sb.Append(text);
            if (next == _logCursor) break;
            _logCursor = next;
        }
        return sb.ToString();
    }

    public async Task KillAsync(uint pid, CancellationToken ct = default)
    {
        await _client.KillAsync(pid, ct);
        _log($"[run] pid {pid} dihentikan");
        if (pid == LastPid) LastPid = 0;
    }

    public Task<IReadOnlyList<string>> ListAsync(string path, CancellationToken ct = default) => _client.ListAsync(path, ct);
}
