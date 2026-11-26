// Radio & Podcast -- internet radio and podcasts. Buitenzorg.UI.
//
// Radio: a station is an http(s) address of an endless audio stream (MP3, as
// Icecast/SHOUTcast servers send it, or a WAV stream). A worker thread reads
// the stream (HttpClient.OpenRead); a UI timer cuts what arrived at MP3 frame
// boundaries, decodes each piece in the host (Buitenzorg.Media.Compressed)
// and keeps ~1.5 s queued in this program's mixer stream.
//
// Podcast: a subscription is an RSS feed; its episodes (<item> with an
// <enclosure>) are listed, and one is downloaded into the app's folder and
// played with MediaPlayer (the file stays for listening offline).
//
// Lists live in /disk/APPDATA/MRADIO (STASIUN.TXT, PODCAST.TXT: "name|url").

using System.Text;
using System.Threading;
using Buitenzorg.App;
using Buitenzorg.Media;
using Buitenzorg.Net;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;
using HttpClient = Buitenzorg.Net.HttpClient;
using Window = Buitenzorg.UI.Window;

namespace MRadio;

public sealed class Station
{
    public string Name;
    public string Url;
    public Station(string name, string url) { Name = name; Url = url; }
    public override string ToString() => Name;
}

public sealed class Episode
{
    public string Title = "";
    public string Url = "";
    public string Type = "";
    public long Length;
    public string Date = "";
    public string Duration = "";
    public override string ToString() => Title + (Duration.Length > 0 ? "   " + Duration : "") + (Date.Length > 0 ? "   " + Date : "");
}

/// <summary>An RSS 2.0 podcast feed: the channel title and its episodes.</summary>
public sealed class Feed
{
    public string Title = "";
    public readonly List<Episode> Episodes = new List<Episode>();

    public static Feed Parse(string xml)
    {
        var f = new Feed();
        int items = xml.IndexOf("<item", StringComparison.Ordinal);
        f.Title = Text(Tag(items < 0 ? xml : xml.Substring(0, items), "title"));
        int at = items;
        while (at >= 0)
        {
            int end = xml.IndexOf("</item>", at, StringComparison.Ordinal);
            if (end < 0) end = xml.Length;
            string item = xml.Substring(at, end - at);
            var e = new Episode { Title = Text(Tag(item, "title")), Date = ShortDate(Text(Tag(item, "pubDate"))), Duration = Text(Tag(item, "itunes:duration")) };
            int enc = item.IndexOf("<enclosure", StringComparison.Ordinal);
            if (enc >= 0)
            {
                int close = item.IndexOf('>', enc);
                string tag = close < 0 ? item.Substring(enc) : item.Substring(enc, close - enc);
                e.Url = Text(Attr(tag, "url"));
                e.Type = Attr(tag, "type");
                long.TryParse(Attr(tag, "length"), out e.Length);
            }
            if (e.Url.Length > 0) f.Episodes.Add(e);
            at = end >= xml.Length ? -1 : xml.IndexOf("<item", end, StringComparison.Ordinal);
        }
        return f;
    }

    /// <summary>The inner text of the first &lt;name&gt; (CDATA unwrapped), "" when absent.</summary>
    public static string Tag(string s, string name)
    {
        int i = s.IndexOf("<" + name, StringComparison.Ordinal);
        while (i >= 0)
        {
            int after = i + 1 + name.Length;
            if (after < s.Length && (s[after] == '>' || s[after] == ' ')) break;
            i = s.IndexOf("<" + name, after, StringComparison.Ordinal);
        }
        if (i < 0) return "";
        int gt = s.IndexOf('>', i);
        if (gt < 0 || s[gt - 1] == '/') return "";
        int end = s.IndexOf("</" + name + ">", gt, StringComparison.Ordinal);
        if (end < 0) return "";
        string inner = s.Substring(gt + 1, end - gt - 1).Trim();
        if (inner.StartsWith("<![CDATA[") && inner.EndsWith("]]>")) inner = inner.Substring(9, inner.Length - 12);
        return inner;
    }

    public static string Attr(string tag, string name)
    {
        int i = tag.IndexOf(" " + name + "=", StringComparison.Ordinal);
        if (i < 0) return "";
        int q = i + name.Length + 2;
        if (q >= tag.Length) return "";
        char quote = tag[q];
        if (quote != '"' && quote != '\'') return "";
        int end = tag.IndexOf(quote, q + 1);
        return end < 0 ? "" : tag.Substring(q + 1, end - q - 1);
    }

    /// <summary>Entities decoded, whitespace folded.</summary>
    public static string Text(string s)
    {
        var sb = new StringBuilder();
        bool space = false;
        for (int i = 0; i < s.Length; i++)
        {
            char c = s[i];
            if (c == '&')
            {
                int semi = s.IndexOf(';', i);
                if (semi > i && semi - i < 8)
                {
                    string ent = s.Substring(i + 1, semi - i - 1);
                    string? rep = ent == "amp" ? "&" : ent == "lt" ? "<" : ent == "gt" ? ">" : ent == "quot" ? "\"" : ent == "apos" || ent == "#39" ? "'" : null;
                    if (rep != null) { sb.Append(rep); i = semi; space = false; continue; }
                }
            }
            if (c == '\n' || c == '\r' || c == '\t' || c == ' ')
            {
                if (!space && sb.Length > 0) sb.Append(' ');
                space = true;
                continue;
            }
            space = false;
            sb.Append(c);
        }
        return sb.ToString().Trim();
    }

    // "Tue, 05 Nov 2026 08:00:00 +0700" -> "05 Nov 2026"
    static string ShortDate(string d)
    {
        int comma = d.IndexOf(',');
        if (comma >= 0) d = d.Substring(comma + 1).Trim();
        var parts = d.Split(' ');
        return parts.Length >= 3 ? parts[0] + " " + parts[1] + " " + parts[2] : d;
    }
}

/// <summary>Plays an endless stream: network bytes in, PCM out to the mixer.</summary>
public sealed class StreamPlayer
{
    readonly object _lock = new object();
    byte[] _in = new byte[256 * 1024];
    int _inCount;
    byte[] _pcm = Array.Empty<byte>();
    int _pcmAt;
    int _format;
    Thread? _reader;
    volatile bool _stop;
    HttpBodyStream? _body;
    bool _wav;
    int _wavHeader = -1;

    public string Status { get; private set; } = "Berhenti";
    public string Error { get; private set; } = "";
    public bool Playing { get; private set; }
    public long Received;
    public long Played;           // PCM bytes handed to the mixer
    public int Decoded;           // pieces decoded
    public string ContentType = "";
    public string StationTitle = "";
    /// <summary>Tests: how many bytes to read before the reader stops by itself (0 = endless).</summary>
    public long StopAfter;
    /// <summary>Tests: read synchronously from the caller of Pump.</summary>
    public bool Synchronous;

    public bool Start(string url)
    {
        Stop();
        Error = "";
        Received = Played = Decoded = 0;
        _inCount = 0;
        _pcm = Array.Empty<byte>();
        _pcmAt = 0;
        _wav = false;
        _wavHeader = -1;
        try
        {
            var http = new HttpClient { TimeoutMs = 10000 };
            _body = http.OpenRead(url, "Icy-MetaData: 0\r\n");
            var r = _body.Response;
            if (!r.Ok) { Error = "server menjawab " + r.Status + " " + r.Reason; _body.Close(); _body = null; Status = "Gagal"; return false; }
            ContentType = (r.Header("Content-Type") ?? "").ToLowerInvariant();
            StationTitle = r.Header("icy-name") ?? "";
            _wav = ContentType.Contains("wav");
        }
        catch (Exception e)
        {
            Error = e.Message;
            Status = "Gagal";
            return false;
        }
        _stop = false;
        Playing = true;
        Status = "Menyambung...";
        Session.KeepAwake(true);
        if (!Synchronous)
        {
            _reader = new Thread(ReadLoop);
            _reader.Start();
        }
        return true;
    }

    void ReadLoop()
    {
        var buf = new byte[16384];
        while (!_stop && ReadOnce(buf)) { }
    }

    /// <summary>One read from the network into the input buffer (false = the stream ended).</summary>
    bool ReadOnce(byte[] buf)
    {
        var b = _body;
        if (b == null) return false;
        lock (_lock)
        {
            // the decoder is behind: wait a little instead of growing without end
            if (_inCount > _in.Length - buf.Length) return true;
        }
        int n;
        try { n = b.Read(buf, 0, buf.Length); }
        catch (Exception e) { Error = e.Message; n = 0; }
        if (n <= 0) { lock (_lock) Status = Error.Length > 0 ? "Terputus" : "Siaran selesai"; return false; }
        lock (_lock)
        {
            Array.Copy(buf, 0, _in, _inCount, n);
            _inCount += n;
            Received += n;
        }
        if (StopAfter > 0 && Received >= StopAfter) return false;
        return true;
    }

    public void Stop()
    {
        _stop = true;
        var b = _body;
        _body = null;
        b?.Close();
        _reader?.Join();
        _reader = null;
        if (Playing) Session.KeepAwake(false);
        Playing = false;
        Status = "Berhenti";
    }

    /// <summary>Feed the mixer (call every ~100 ms): decode what arrived, keep ~1.5 s queued.</summary>
    public void Pump()
    {
        if (!Playing) return;
        if (Synchronous) { var buf = new byte[16384]; if (!_stop && !ReadOnce(buf)) _stop = true; }
        int guard = 0;
        while (guard++ < 4 && (Audio.CanStream ? Audio.QueuedMs < 1500 : true))
        {
            if (_pcmAt >= _pcm.Length && !NextPiece()) break;
            int n = Math.Min(_pcm.Length - _pcmAt, 48000);
            int took = Audio.CanStream ? Audio.Write(_pcm, _pcmAt, n, _format) : n;
            if (took <= 0) break;
            _pcmAt += took;
            Played += took;
            if (Audio.CanStream) Status = "Mengudara";
        }
        if (!Audio.CanStream && Played > 0) Status = "Mengudara (tanpa kartu suara)";
        if (_stop && _pcmAt >= _pcm.Length && _inCount == 0 && (!Audio.CanStream || Audio.QueuedMs == 0))
        {
            Playing = false;
            Session.KeepAwake(false);
            if (Status.StartsWith("Mengudara")) Status = "Siaran selesai";
        }
    }

    // Take the input bytes collected so far and turn them into PCM.
    bool NextPiece()
    {
        byte[] piece;
        lock (_lock)
        {
            int min = _wav ? 4096 : 24 * 1024;
            if (_inCount < min && !(_stop && _inCount > 0)) return false;
            int cut = _wav ? _inCount & ~3 : LastFrameStart(_in, _inCount);
            if (cut <= 0) cut = _inCount;
            piece = new byte[cut];
            Array.Copy(_in, 0, piece, 0, cut);
            Array.Copy(_in, cut, _in, 0, _inCount - cut);
            _inCount -= cut;
        }
        if (_wav)
        {
            if (_wavHeader < 0)
            {
                var info = Wav.Parse(piece);
                if (info == null) { Error = "aliran WAV tanpa kepala yang sah"; _stop = true; return false; }
                _format = info.Format;
                _wavHeader = info.DataOffset;
                var data = new byte[piece.Length - info.DataOffset];
                Array.Copy(piece, info.DataOffset, data, 0, data.Length);
                piece = data;
            }
            _pcm = piece;
            _pcmAt = 0;
            Decoded++;
            return piece.Length > 0;
        }
        int start = FirstFrame(piece);
        if (start < 0) return false;
        if (start > 0)
        {
            var p = new byte[piece.Length - start];
            Array.Copy(piece, start, p, 0, p.Length);
            piece = p;
        }
        var wi = Compressed.Decode(piece, out var pcm);
        if (wi == null)
        {
            Error = Audio.CanStream ? "potongan siaran tidak bisa didekode" : "dekoder MP3 hanya ada di perangkat";
            return false;
        }
        _format = wi.Format;
        _pcm = pcm;
        _pcmAt = 0;
        Decoded++;
        return pcm.Length > 0;
    }

    static bool Sync(byte[] b, int i) => b[i] == 0xFF && (b[i + 1] & 0xE0) == 0xE0 && (b[i + 1] & 0x18) != 0x08 && (b[i + 1] & 0x06) != 0 && (b[i + 2] & 0xF0) != 0xF0 && (b[i + 2] & 0x0C) != 0x0C;

    public static int FirstFrame(byte[] b)
    {
        for (int i = 0; i + 3 < b.Length; i++) if (Sync(b, i)) return i;
        return -1;
    }

    /// <summary>Start of the last MP3 frame header in b[0..count) (0 = none):
    /// a piece is cut there so no frame is split between two decodes.</summary>
    public static int LastFrameStart(byte[] b, int count)
    {
        for (int i = count - 4; i > 0; i--) if (Sync(b, i)) return i;
        return 0;
    }
}

public sealed class RadioApp
{
    public Window W { get; }
    public readonly string Dir;
    public readonly ObservableCollection<Station> Stations = new ObservableCollection<Station>();
    public readonly ObservableCollection<Station> Podcasts = new ObservableCollection<Station>();
    public readonly ObservableCollection<Episode> Episodes = new ObservableCollection<Episode>();
    public readonly StreamPlayer Live = new StreamPlayer();
    public readonly MediaPlayer Player = new MediaPlayer();
    readonly ListBox _stations, _podcasts, _episodes;
    readonly TextBox _name = new TextBox { Watermark = "Nama" };
    readonly TextBox _url = new TextBox { Watermark = "http(s)://alamat aliran" };
    readonly TextBox _feed = new TextBox { Watermark = "alamat RSS podcast" };
    readonly TextBlock _now = new TextBlock("Pilih stasiun atau episode");
    readonly TextBlock _status = new TextBlock("");
    readonly DispatcherTimer _timer = new DispatcherTimer(100);
    public string FeedTitle = "";
    public bool Synchronous;

    public static readonly Station[] DefaultStations =
    {
        new Station("SomaFM Groove Salad", "https://ice1.somafm.com/groovesalad-128-mp3"),
        new Station("SomaFM Drone Zone", "https://ice1.somafm.com/dronezone-128-mp3"),
    };

    public RadioApp(bool headless, string? dir = null)
    {
        Dir = dir ?? "/disk/APPDATA/MRADIO";
        W = new Window("Radio & Podcast", 600, 540, headless);
        _stations = new ListBox { ItemsSource = Stations, Height = 200 };
        _stations.SelectionChanged += l => { if (l.SelectedIndex >= 0) Tune(l.SelectedIndex); };
        _podcasts = new ListBox { ItemsSource = Podcasts, Height = 90 };
        _podcasts.SelectionChanged += l => { if (l.SelectedIndex >= 0) OpenFeed(l.SelectedIndex); };
        _episodes = new ListBox { ItemsSource = Episodes, Height = 170 };
        _episodes.SelectionChanged += l => { if (l.SelectedIndex >= 0) PlayEpisode(l.SelectedIndex); };

        var tabs = new TabControl();
        tabs.Add("Radio", RadioPage(), IconKind.Play);
        tabs.Add("Podcast", PodcastPage(), IconKind.File);
        var stop = new Button("Berhenti", _ => StopAll()).WithClass("subtle");
        var bar = new DockPanel { Margin = new Thickness(0, 6, 0, 0) };
        bar.Children.Add(stop.DockTo(Dock.Right));
        bar.Children.Add(_now.WithClass("strong"));
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Radio & Podcast", IconKind.Play, 0xE8A25F).DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(bar.DockTo(Dock.Bottom));
        root.Children.Add(tabs);
        W.Content = root;
        W.Closing += () => StopAll();
        _timer.Tick += Tick;
        Load();
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    Control RadioPage()
    {
        var add = new Button("Tambah", _ => AddStation(_name.Text, _url.Text)).WithClass("accent");
        var remove = new Button("Hapus", _ => RemoveStation(_stations.SelectedIndex)).WithClass("subtle");
        var row = new DockPanel { Margin = new Thickness(0, 6, 0, 0) };
        row.Children.Add(remove.DockTo(Dock.Right));
        row.Children.Add(add.DockTo(Dock.Right));
        _name.Width = 150;
        row.Children.Add(_name.DockTo(Dock.Left));
        row.Children.Add(_url);
        var p = new DockPanel { Margin = new Thickness(0, 8, 0, 0) };
        p.Children.Add(row.DockTo(Dock.Bottom));
        p.Children.Add(_stations);
        return p;
    }

    Control PodcastPage()
    {
        var sub = new Button("Langganan", _ => Subscribe(_feed.Text)).WithClass("accent");
        var unsub = new Button("Hapus", _ => Unsubscribe(_podcasts.SelectedIndex)).WithClass("subtle");
        var row = new DockPanel { Margin = new Thickness(0, 0, 0, 6) };
        row.Children.Add(unsub.DockTo(Dock.Right));
        row.Children.Add(sub.DockTo(Dock.Right));
        row.Children.Add(_feed);
        var p = new DockPanel { Margin = new Thickness(0, 8, 0, 0) };
        p.Children.Add(row.DockTo(Dock.Top));
        p.Children.Add(_podcasts.DockTo(Dock.Top));
        p.Children.Add(new TextBlock("Episode").WithClass("faint").DockTo(Dock.Top));
        p.Children.Add(_episodes);
        return p;
    }

    string StationsFile => Dir + "/STASIUN.TXT";
    string PodcastFile => Dir + "/PODCAST.TXT";

    void Load()
    {
        Stations.Clear();
        foreach (var s in ReadList(StationsFile)) Stations.Add(s);
        if (!(Files.Size(StationsFile) >= 0)) foreach (var s in DefaultStations) Stations.Add(s);
        Podcasts.Clear();
        foreach (var s in ReadList(PodcastFile)) Podcasts.Add(s);
    }

    static List<Station> ReadList(string path)
    {
        var list = new List<Station>();
        string text = (Files.Size(path) >= 0) ? Files.ReadAllText(path) : "";
        foreach (var raw in text.Split('\n'))
        {
            string line = raw.Trim();
            int bar = line.IndexOf('|');
            if (bar > 0 && bar < line.Length - 1) list.Add(new Station(line.Substring(0, bar), line.Substring(bar + 1)));
        }
        return list;
    }

    bool Save(string path, IEnumerable<Station> items)
    {
        Files.CreateDirectory("/disk/APPDATA");
        Files.CreateDirectory(Dir);
        var sb = new StringBuilder();
        foreach (var s in items) sb.Append(s.Name.Replace('|', '/')).Append('|').Append(s.Url).Append('\n');
        return Files.WriteAllText(path, sb.ToString());
    }

    static bool IsAddress(string url) => url.StartsWith("http://") || url.StartsWith("https://");

    public bool AddStation(string name, string url)
    {
        url = url.Trim();
        if (!IsAddress(url)) { _status.Text = "Alamat stasiun harus diawali http:// atau https://"; return false; }
        if (name.Trim().Length == 0) name = url;
        Stations.Add(new Station(name.Trim(), url));
        _name.Text = "";
        _url.Text = "";
        _status.Text = "Stasiun ditambahkan: " + name.Trim();
        return Save(StationsFile, Stations);
    }

    public void RemoveStation(int i)
    {
        if (i < 0 || i >= Stations.Count) return;
        Stations.RemoveAt(i);
        Save(StationsFile, Stations);
    }

    public bool Tune(int i)
    {
        if (i < 0 || i >= Stations.Count) return false;
        StopAll();
        var s = Stations[i];
        _now.Text = s.Name;
        _status.Text = "Menyambung ke " + s.Url + " ...";
        Live.Synchronous = Synchronous;
        if (!Live.Start(s.Url)) { _status.Text = "Gagal: " + Live.Error; return false; }
        if (Live.StationTitle.Length > 0) _now.Text = s.Name + " -- " + Live.StationTitle;
        _timer.Start();
        return true;
    }

    public bool Subscribe(string url)
    {
        url = url.Trim();
        if (!IsAddress(url)) { _status.Text = "Alamat RSS harus diawali http:// atau https://"; return false; }
        var feed = Fetch(url);
        if (feed == null) return false;
        Podcasts.Add(new Station(feed.Title.Length > 0 ? feed.Title : url, url));
        _feed.Text = "";
        Save(PodcastFile, Podcasts);
        Show(feed);
        return true;
    }

    public void Unsubscribe(int i)
    {
        if (i < 0 || i >= Podcasts.Count) return;
        Podcasts.RemoveAt(i);
        Episodes.Clear();
        Save(PodcastFile, Podcasts);
    }

    Feed? Fetch(string url)
    {
        try
        {
            var r = new HttpClient { TimeoutMs = 15000 }.Get(url);
            if (!r.Ok) { _status.Text = "Feed: " + r.Status + " " + r.Reason; return null; }
            var f = Feed.Parse(Encoding.UTF8.GetString(r.Body));
            if (f.Episodes.Count == 0) { _status.Text = "Feed tanpa episode audio: " + url; return null; }
            return f;
        }
        catch (Exception e) { _status.Text = "Feed: " + e.Message; return null; }
    }

    void Show(Feed f)
    {
        FeedTitle = f.Title;
        Episodes.Clear();
        foreach (var e in f.Episodes) Episodes.Add(e);
        _status.Text = f.Title + ": " + f.Episodes.Count + " episode";
    }

    public bool OpenFeed(int i)
    {
        if (i < 0 || i >= Podcasts.Count) return false;
        var f = Fetch(Podcasts[i].Url);
        if (f == null) return false;
        Show(f);
        return true;
    }

    /// <summary>The 8.3 file an episode is kept in (EP + a hash of its address).</summary>
    public string EpisodeFile(Episode e)
    {
        uint h = 2166136261;
        foreach (char ch in e.Url) h = (h ^ ch) * 16777619;
        string ext = e.Url.ToLowerInvariant().Contains(".wav") || e.Type.Contains("wav") ? "WAV"
            : e.Type.Contains("ogg") || e.Url.ToLowerInvariant().Contains(".ogg") ? "OGG" : "MP3";
        const string hex = "0123456789ABCDEF";
        var sb = new StringBuilder("EP");
        for (int k = 0; k < 6; k++) sb.Append(hex[(int)(h >> (k * 4)) & 15]);
        return Dir + "/" + sb + "." + ext;
    }

    public bool PlayEpisode(int i)
    {
        if (i < 0 || i >= Episodes.Count) return false;
        StopAll();
        var e = Episodes[i];
        string path = EpisodeFile(e);
        Files.CreateDirectory("/disk/APPDATA");
        Files.CreateDirectory(Dir);
        if (Files.Size(path) <= 0 || (e.Length > 0 && Files.Size(path) < e.Length))
        {
            _status.Text = "Mengunduh " + e.Title + " ...";
            try
            {
                var r = new HttpClient { TimeoutMs = 20000 }.Download(e.Url, path, null, null, true);
                if (!r.Ok && r.Status != 416) { _status.Text = "Unduh gagal: " + r.Status + " " + r.Reason; return false; }
            }
            catch (Exception ex) { _status.Text = "Unduh gagal: " + ex.Message; return false; }
        }
        if (!Player.Load(path)) { _status.Text = "Episode tidak bisa diputar (format?)"; return false; }
        Player.Play();
        _now.Text = e.Title;
        _status.Text = "Memutar episode (disimpan di " + path + ")";
        _timer.Start();
        return true;
    }

    public void StopAll()
    {
        Live.Stop();
        Player.Stop();
        _timer.Stop();
    }

    void Tick()
    {
        if (Live.Playing)
        {
            Live.Pump();
            _status.Text = Live.Status + " -- " + Live.Received / 1024 + " KB diterima" + (Live.Error.Length > 0 ? " (" + Live.Error + ")" : "");
        }
        else if (Player.State == PlayState.Playing) Player.Pump();
        else if (!Live.Playing) _timer.Stop();
    }

    // ---- self-test: a local server with a feed, an episode and an endless WAV stream ----

    const int TestPort = 8093;

    static byte[] ToneWav(int seconds)
    {
        var s = new short[16000 * seconds];
        for (int i = 0; i < s.Length; i++) s[i] = (short)(((i / 18) & 1) == 0 ? 6000 : -6000);
        return Wav.Build(s, 16000, "Episode Uji");
    }

    static void Serve(TcpServer server, Func<bool> stop)
    {
        var episode = ToneWav(1);
        string feed = "<?xml version=\"1.0\"?><rss version=\"2.0\" xmlns:itunes=\"http://www.itunes.com/dtds/podcast-1.0.dtd\"><channel>"
            + "<title>Kabar &amp; Kebun</title><item><title><![CDATA[Episode 2: Anggrek]]></title><pubDate>Tue, 24 Nov 2026 08:00:00 +0700</pubDate>"
            + "<itunes:duration>00:01</itunes:duration><enclosure url=\"http://localhost:" + TestPort + "/ep2.wav\" length=\"" + episode.Length + "\" type=\"audio/wav\"/></item>"
            + "<item><title>Tanpa audio</title></item>"
            + "<item><title>Episode 1</title><enclosure type='audio/wav' url='http://localhost:" + TestPort + "/ep1.wav'/></item></channel></rss>";
        while (!stop())
        {
            var c = server.Accept(50);
            if (c == null) continue;
            try
            {
                c.TimeoutMs = 2000;
                var buf = new byte[4096];
                var req = new StringBuilder();
                while (!req.ToString().Contains("\r\n\r\n"))
                {
                    int n = c.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    req.Append(Encoding.ASCII.GetString(buf, 0, n));
                }
                string line = req.ToString();
                byte[] body;
                string type;
                if (line.StartsWith("GET /feed.xml")) { body = Encoding.UTF8.GetBytes(feed); type = "application/rss+xml"; }
                else if (line.StartsWith("GET /ep")) { body = episode; type = "audio/wav"; }
                else if (line.StartsWith("GET /live"))
                {
                    // a "radio": a WAV header, then sound until the client hangs up (at most 4 s)
                    var head = Encoding.ASCII.GetBytes("HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nicy-name: Radio Uji\r\n\r\n");
                    c.Write(head, 0, head.Length);
                    var wav = ToneWav(4);
                    for (int o = 0; o < wav.Length && !stop(); o += 8192) c.Write(wav, o, Math.Min(8192, wav.Length - o));
                    c.Close();
                    continue;
                }
                else { body = new byte[0]; type = "text/plain"; }
                bool found = body.Length > 0;
                var h = Encoding.ASCII.GetBytes((found ? "HTTP/1.1 200 OK" : "HTTP/1.1 404 Not Found") + "\r\nContent-Type: " + type + "\r\nContent-Length: " + body.Length + "\r\nConnection: close\r\n\r\n");
                c.Write(h, 0, h.Length);
                for (int o = 0; o < body.Length; o += 16384) c.Write(body, o, Math.Min(16384, body.Length - o));
            }
            catch (Exception) { }
            c.Close();
        }
        server.Close();
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Radio");
        // the feed parser, on its own
        var f = Feed.Parse("<rss><channel><title>A &amp; B</title><item><title>X</title><enclosure url=\"http://h/x.mp3\" length=\"12\" type=\"audio/mpeg\"/></item><item><title>no audio</title></item></channel></rss>");
        c.That(f.Title == "A & B" && f.Episodes.Count == 1 && f.Episodes[0].Url == "http://h/x.mp3" && f.Episodes[0].Length == 12, "RSS: title, one episode with an enclosure");
        var frames = new byte[] { 0, 0, 0xFF, 0xFB, 0x90, 0x44, 1, 2, 3, 0xFF, 0xFB, 0x90, 0x44, 9, 9 };
        c.That(StreamPlayer.FirstFrame(frames) == 2 && StreamPlayer.LastFrameStart(frames, frames.Length) == 9, "MP3 frame boundaries found");

        string dir = Files.ScratchDirectory + "/RADIOTES";
        foreach (var e in Files.List(dir)) Files.Delete(e.Path);
        Files.CreateDirectory(dir);
        bool stop = false;
        TcpServer? server = null;
        try { server = new TcpServer(TestPort); } catch (Exception e) { c.That(false, "listen on " + TestPort + ": " + e.Message); }
        if (server != null)
        {
            var th = new Thread(() => Serve(server, () => stop));
            th.Start();
            try
            {
                var app = new RadioApp(true, dir) { Synchronous = true };
                c.That(app.Stations.Count == DefaultStations.Length, "first start lists the default stations");
                c.That(!app.AddStation("x", "ftp://nope") && app.AddStation("Radio Uji", "http://localhost:" + TestPort + "/live"), "a station needs an http(s) address");
                int k = app.Stations.Count - 1;
                c.That(app.Tune(k) && app.Live.Playing, "tuning in opens the stream (" + app.Live.Error + ")");
                // the mixer drains in real time (virtio-snd, AC'97): pump until the
                // stream ends or enough was handed over, at most 20 s
                var clock = System.Diagnostics.Stopwatch.StartNew();
                for (int i = 0; app.Live.Playing && clock.ElapsedMilliseconds < 20000; i++)
                {
                    long before = app.Live.Played;
                    app.Live.Pump();
                    if (app.Live.Played > 50000 && app.Live.Decoded >= 2 && i >= 400) break;
                    if (app.Live.Played == before && i >= 400) Thread.Sleep(20);
                }
                c.That(app.Live.Received > 60000 && app.Live.Played > 50000 && app.Live.Decoded >= 2 && app.Live.StationTitle == "Radio Uji",
                    "the stream played: " + app.Live.Received + " bytes in, " + app.Live.Played + " PCM bytes out in " + app.Live.Decoded + " pieces");
                app.StopAll();
                c.That(app.Subscribe("http://localhost:" + TestPort + "/feed.xml") && app.Podcasts.Count == 1 && app.Podcasts[0].Name == "Kabar & Kebun",
                    "subscribed: '" + (app.Podcasts.Count > 0 ? app.Podcasts[0].Name : "-") + "'");
                c.That(app.Episodes.Count == 2 && app.Episodes[0].Title == "Episode 2: Anggrek" && app.Episodes[0].Date == "24 Nov 2026" && app.Episodes[0].Duration == "00:01",
                    "episodes listed (" + app.Episodes.Count + ")");
                c.That(app.PlayEpisode(0) && app.Player.State == PlayState.Playing && app.Player.Title == "Episode Uji", "an episode downloads and plays");
                c.That(Files.Size(app.EpisodeFile(app.Episodes[0])) == ToneWav(1).Length, "the episode is kept for offline listening");
                app.StopAll();
                var again = new RadioApp(true, dir);
                c.That(again.Stations.Count == DefaultStations.Length + 1 && again.Podcasts.Count == 1, "stations and subscriptions survive a restart");
                again.W.RenderFrame();
                c.That(again.W.LastFrameOps > 20, "the window drew " + again.W.LastFrameOps + " ops");
                app.RemoveStation(k);
                app.Unsubscribe(0);
                c.That(new RadioApp(true, dir).Podcasts.Count == 0, "unsubscribed");
            }
            catch (Exception e) { c.That(false, "radio: " + e.Message); }
            stop = true;
            th.Join();
        }
        foreach (var e in Files.List(dir)) Files.Delete(e.Path);
        Files.Delete(dir);
        return c;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = RadioApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var r = new RadioApp(headless: false);
        app.Run(r.W);
        return 0;
    }
}
