// Buitenzorg.Media.Movie: video files read by the host (clrhost movie.rs).

namespace Buitenzorg.Media;

internal static class MovieNative
{
    [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern long MovieOpen(byte[] file);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern string MovieInfo(long id);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int MovieFrame(long id, int index, int[] pixels, int dw, int dh);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int MovieAudio(long id, byte[] dst, int offset);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] internal static extern int MovieFree(long id);
}

/// <summary>A video file read by the host (Matroska / WebM, MP4 / QuickTime):
/// VP8 or Motion-JPEG pictures decoded per frame, the sound decoded whole
/// (Opus, Vorbis, MP3, PCM) to 16-bit PCM. Only on Buitenzorg -- off the device
/// <see cref="Open"/> returns null and <see cref="LastError"/> says why.</summary>
public sealed class Movie
{
    long _id;
    public int Width { get; private set; }
    public int Height { get; private set; }
    public double Fps { get; private set; }
    public int FrameCount { get; private set; }
    public double Seconds { get; private set; }
    public string VideoCodec { get; private set; } = "";
    public string AudioCodec { get; private set; } = "";
    public int AudioRate { get; private set; }
    public int AudioChannels { get; private set; }
    public int AudioBytes { get; private set; }
    public string Title { get; private set; } = "";
    /// <summary>What the file holds that cannot be decoded ("" = everything plays).</summary>
    public string Unsupported { get; private set; } = "";
    public bool HasVideo => FrameCount > 0 && Unsupported.IndexOf("video", StringComparison.Ordinal) < 0;
    public static string LastError { get; private set; } = "";

    public static bool IsMovieName(string name)
    {
        string n = name.ToUpperInvariant();
        return n.EndsWith(".WEBM") || n.EndsWith(".WEB") || n.EndsWith(".MKV") || n.EndsWith(".MP4") || n.EndsWith(".M4V") || n.EndsWith(".MOV");
    }

    public static Movie? Open(byte[] file)
    {
        long id;
        try { id = MovieNative.MovieOpen(file); }
        catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException)
        {
            LastError = "video MP4 / WebM hanya diputar di Buitenzorg (dekodernya di host)";
            return null;
        }
        if (id <= 0) { LastError = "bukan berkas video yang dikenal (Matroska / WebM / MP4)"; return null; }
        var m = new Movie { _id = id };
        var lines = MovieNative.MovieInfo(id).Split('\n');
        var f = lines[0].Split(' ');
        if (f.Length >= 10)
        {
            m.Width = Num(f[0]); m.Height = Num(f[1]); m.Fps = Num(f[2]) / 1000.0; m.FrameCount = Num(f[3]); m.Seconds = Num(f[4]) / 1000.0;
            m.VideoCodec = f[5] == "-" ? "" : f[5];
            m.AudioCodec = f[6] == "-" ? "" : f[6];
            m.AudioRate = Num(f[7]); m.AudioChannels = Num(f[8]); m.AudioBytes = Num(f[9]);
        }
        if (lines.Length > 1) m.Title = lines[1];
        if (lines.Length > 2) m.Unsupported = lines[2];
        if (m.Fps <= 0 && m.Seconds > 0 && m.FrameCount > 0) m.Fps = m.FrameCount / m.Seconds;
        LastError = "";
        return m;
    }

    static int Num(string s)
    {
        int v = 0;
        foreach (char c in s)
        {
            if (c < '0' || c > '9') break;
            v = v * 10 + (c - '0');
        }
        return v;
    }

    /// <summary>Frame `index` scaled to dw x dh into `pixels` (0xAARRGGBB).</summary>
    public bool Frame(int index, int[] pixels, int dw, int dh) => _id > 0 && MovieNative.MovieFrame(_id, index, pixels, dw, dh) == 0;

    /// <summary>The whole decoded sound (16-bit little endian, interleaved).</summary>
    public byte[] Sound()
    {
        var b = new byte[Math.Max(0, AudioBytes)];
        if (b.Length > 0) MovieNative.MovieAudio(_id, b, 0);
        return b;
    }

    public void Close()
    {
        if (_id > 0) { MovieNative.MovieFree(_id); _id = 0; }
    }
}
