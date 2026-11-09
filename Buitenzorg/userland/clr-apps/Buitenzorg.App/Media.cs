// Buitenzorg.Media -- sound files and a player on the kernel mixer.
//
// Wav parses RIFF/WAVE PCM (8-bit unsigned or 16-bit signed, mono or stereo,
// any rate; the title from a LIST/INFO "INAM" chunk). MediaPlayer keeps the
// program's mixer stream (AUDIO_WRITE) about 0.6 s ahead: the app calls Pump()
// from a UI timer (every 100 ms is plenty) -- no thread, so it is safe in any
// app. The kernel converts the format and mixes it with every other program's
// sound; Audio.AppVolume sets this program's level in the mix.
//
// Off the device (no sound card) the player still runs: Audio.Write takes
// everything, and the position follows the clock -- so the self-tests work
// on a PC.

namespace Buitenzorg.Media;

using Buitenzorg.App;

public sealed class WavInfo
{
    public int Rate { get; init; }
    public int Channels { get; init; }
    public int Bits { get; init; }
    public int DataOffset { get; init; }
    public int DataLength { get; init; }
    public string Title { get; init; } = "";
    public int FrameBytes => Channels * Bits / 8;
    public double Seconds => FrameBytes == 0 || Rate == 0 ? 0 : (double)DataLength / FrameBytes / Rate;
    public int Format => Audio.Format(Rate, Channels, Bits);
}

public static class Wav
{
    static int U32(byte[] b, int i) => b[i] | b[i + 1] << 8 | b[i + 2] << 16 | b[i + 3] << 24;
    static int U16(byte[] b, int i) => b[i] | b[i + 1] << 8;
    static bool Tag(byte[] b, int i, string t) => i + 4 <= b.Length && b[i] == t[0] && b[i + 1] == t[1] && b[i + 2] == t[2] && b[i + 3] == t[3];

    /// <summary>The header of a PCM WAV file, or null when it is not one this player plays.</summary>
    public static WavInfo? Parse(byte[] b)
    {
        if (b.Length < 44 || !Tag(b, 0, "RIFF") || !Tag(b, 8, "WAVE")) return null;
        int p = 12, rate = 0, ch = 0, bits = 0, fmt = 0, off = 0, len = 0;
        string title = "";
        while (p + 8 <= b.Length)
        {
            int size = U32(b, p + 4);
            int body = p + 8;
            if (size < 0 || body + size > b.Length) size = b.Length - body;
            if (Tag(b, p, "fmt ") && size >= 16)
            {
                fmt = U16(b, body); ch = U16(b, body + 2); rate = U32(b, body + 4); bits = U16(b, body + 14);
            }
            else if (Tag(b, p, "data")) { off = body; len = size; }
            else if (Tag(b, p, "LIST") && Tag(b, body, "INFO"))
            {
                int q = body + 4;
                while (q + 8 <= body + size)
                {
                    int s = U32(b, q + 4);
                    if (Tag(b, q, "INAM"))
                    {
                        int n = 0;
                        while (n < s && q + 8 + n < b.Length && b[q + 8 + n] != 0) n++;
                        title = System.Text.Encoding.UTF8.GetString(b, q + 8, n);
                    }
                    q += 8 + s + (s & 1);
                }
            }
            p = body + size + (size & 1);
        }
        if (fmt != 1 || (bits != 8 && bits != 16) || (ch != 1 && ch != 2) || rate < 4000 || off == 0) return null;
        return new WavInfo { Rate = rate, Channels = ch, Bits = bits, DataOffset = off, DataLength = len - len % (ch * bits / 8), Title = title };
    }

    /// <summary>A WAV file of 16-bit mono samples (tests, generated sounds).</summary>
    public static byte[] Build(short[] samples, int rate, string title = "")
    {
        byte[] name = System.Text.Encoding.UTF8.GetBytes(title);
        int pad = (name.Length + 1) & 1;
        int listLen = title.Length > 0 ? 4 + 8 + name.Length + 1 + pad : 0;
        int dataLen = samples.Length * 2;
        var o = new byte[12 + 24 + (listLen > 0 ? 8 + listLen : 0) + 8 + dataLen];
        int p = 0;
        Tag(o, ref p, "RIFF"); I32(o, ref p, o.Length - 8); Tag(o, ref p, "WAVE");
        Tag(o, ref p, "fmt "); I32(o, ref p, 16); I16(o, ref p, 1); I16(o, ref p, 1);
        I32(o, ref p, rate); I32(o, ref p, rate * 2); I16(o, ref p, 2); I16(o, ref p, 16);
        if (listLen > 0)
        {
            Tag(o, ref p, "LIST"); I32(o, ref p, listLen); Tag(o, ref p, "INFO");
            Tag(o, ref p, "INAM"); I32(o, ref p, name.Length + 1);
            for (int i = 0; i < name.Length; i++) o[p++] = name[i];
            p += 1 + pad;
        }
        Tag(o, ref p, "data"); I32(o, ref p, dataLen);
        for (int i = 0; i < samples.Length; i++) I16(o, ref p, samples[i]);
        return o;
    }

    static void Tag(byte[] o, ref int p, string t) { for (int i = 0; i < 4; i++) o[p++] = (byte)t[i]; }
    static void I16(byte[] o, ref int p, int v) { o[p++] = (byte)v; o[p++] = (byte)(v >> 8); }
    static void I32(byte[] o, ref int p, int v) { I16(o, ref p, v); I16(o, ref p, v >> 16); }
}

public enum PlayState { Stopped, Playing, Paused }

/// <summary>Plays one WAV at a time through the program's mixer stream.</summary>
public sealed class MediaPlayer
{
    byte[] _data = Array.Empty<byte>();
    WavInfo? _info;
    int _written;          // bytes of the data chunk handed to the mixer
    long _startMs;          // off the device: the clock when play started (position source)
    double _base;           // seconds played before the last (re)start
    bool _sim;

    public PlayState State { get; private set; }
    public string Title { get; private set; } = "";
    public WavInfo? Info => _info;
    public double Duration => _info?.Seconds ?? 0;
    /// <summary>The loudness of the last chunk handed out, 0..100 (for meters).</summary>
    public int Level { get; private set; }
    /// <summary>The track ended (played to its end).</summary>
    public event Action? Ended;

    /// <summary>Seconds played (what is still queued in the mixer is not counted).</summary>
    public double Position
    {
        get
        {
            if (_info == null) return 0;
            if (State == PlayState.Paused) return _base; // also right after a Seek while paused
            if (_sim) return State == PlayState.Playing ? Math.Min(Duration, _base + (Environment.TickCount64 - _startMs) / 1000.0) : _base;
            double written = (double)_written / _info.FrameBytes / _info.Rate;
            return Math.Max(0, written - (State == PlayState.Stopped ? 0 : Audio.QueuedMs / 1000.0));
        }
    }

    public bool Load(byte[] bytes, string fallbackTitle)
    {
        var info = Wav.Parse(bytes);
        if (info == null) return false;
        Stop();
        _data = bytes;
        _info = info;
        Title = info.Title.Length > 0 ? info.Title : fallbackTitle;
        return true;
    }

    public bool Load(string path)
    {
        try { var b = Files.ReadAllBytes(path); return b != null && Load(b, BaseName(path)); }
        catch (Exception) { return false; }
    }

    static string BaseName(string path)
    {
        int s = path.LastIndexOf('/'), d = path.LastIndexOf('.');
        return d > s ? path.Substring(s + 1, d - s - 1) : path.Substring(s + 1);
    }

    public void Play()
    {
        if (_info == null) return;
        if (State == PlayState.Playing) return;
        if (_written >= _info.DataLength) { _written = 0; _base = 0; }
        _sim = !Audio.CanStream;
        _startMs = Environment.TickCount64;
        State = PlayState.Playing;
        Pump();
    }

    public void Pause()
    {
        if (State != PlayState.Playing) return;
        _base = Position;
        // what is queued (at most ~0.6 s) still plays out; resuming continues after it
        State = PlayState.Paused;
    }

    public void Stop()
    {
        State = PlayState.Stopped;
        _written = 0;
        _base = 0;
        Level = 0;
    }

    /// <summary>Jump to `seconds` (from the next pump on).</summary>
    public void Seek(double seconds)
    {
        if (_info == null) return;
        int frame = (int)(Math.Clamp(seconds, 0, Duration) * _info.Rate);
        _written = Math.Min(_info.DataLength, frame * _info.FrameBytes);
        _base = (double)_written / _info.FrameBytes / _info.Rate;
        _startMs = Environment.TickCount64;
    }

    /// <summary>Keep the stream ~0.6 s ahead (call every ~100 ms while playing).</summary>
    public void Pump()
    {
        if (State != PlayState.Playing || _info == null) return;
        int chunk = _info.Rate / 4 * _info.FrameBytes; // a quarter second
        int guard = 0;
        while (_written < _info.DataLength && (_sim ? _written < SimTarget() : Audio.QueuedMs < 600) && guard++ < 8)
        {
            int n = Math.Min(chunk, _info.DataLength - _written);
            // no streaming card (ARM64 / RISC-V, a PC without sound): the clock plays it
            int took = _sim ? n : Audio.Write(_data, _info.DataOffset + _written, n, _info.Format);
            if (took <= 0) break;
            Level = Loudness(_info.DataOffset + _written, took);
            _written += took;
        }
        if (_written >= _info.DataLength && (_sim ? Position >= Duration : Audio.QueuedMs == 0))
        {
            State = PlayState.Stopped;
            _base = Duration;
            Level = 0;
            Ended?.Invoke();
        }
    }

    int SimTarget() => (int)Math.Min(_info!.DataLength, (Position + 0.6) * _info.Rate * _info.FrameBytes);

    /// <summary>Peak of a range, 0..100 (sampled: a few hundred values).</summary>
    int Loudness(int from, int count)
    {
        if (_info == null || count <= 0) return 0;
        int step = Math.Max(_info.FrameBytes, count / 256 / _info.FrameBytes * _info.FrameBytes);
        int peak = 0;
        for (int i = from; i + 1 < from + count && i + 1 < _data.Length; i += step)
        {
            int v = _info.Bits == 8 ? (_data[i] - 128) << 8 : (short)(_data[i] | _data[i + 1] << 8);
            peak = Math.Max(peak, Math.Abs(v));
        }
        return peak * 100 / 32768;
    }
}
