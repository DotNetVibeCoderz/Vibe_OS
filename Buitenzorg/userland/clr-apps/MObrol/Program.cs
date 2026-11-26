// Obrolan -- a chat client for IRC (RFC 1459 / 2812), the open chat
// protocol: any server (port 6697 = TLS, else plain), a nickname, channels.
// A reader thread receives lines; a UI timer shows them. Commands typed in
// the message box: /join #kanal, /part, /nick nama, /msg nama teks, /me aksi,
// /quit. PING is answered, NAMES fills the member list, a message that names
// you while the window is not focused posts a notification.
//
// Settings live in /disk/APPDATA/MOBROL/OBROLAN.CFG. Buitenzorg.UI.

using System.Text;
using System.Threading;
using Buitenzorg.App;
using Buitenzorg.Net;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;
using Window = Buitenzorg.UI.Window;

namespace MObrol;

/// <summary>One parsed IRC line: ":prefix COMMAND p1 p2 :trailing".</summary>
public sealed class IrcLine
{
    public string Prefix = "";
    public string Command = "";
    public readonly List<string> Params = new List<string>();
    public string Nick => Prefix.IndexOf('!') > 0 ? Prefix.Substring(0, Prefix.IndexOf('!')) : Prefix;
    public string Last => Params.Count > 0 ? Params[Params.Count - 1] : "";

    public static IrcLine Parse(string line)
    {
        var l = new IrcLine();
        int i = 0;
        if (line.StartsWith("@")) { int sp = line.IndexOf(' '); i = sp < 0 ? line.Length : sp + 1; } // IRCv3 tags: skipped
        if (i < line.Length && line[i] == ':')
        {
            int sp = line.IndexOf(' ', i);
            if (sp < 0) { l.Prefix = line.Substring(i + 1); return l; }
            l.Prefix = line.Substring(i + 1, sp - i - 1);
            i = sp + 1;
        }
        while (i < line.Length && line[i] == ' ') i++;
        int cmdEnd = line.IndexOf(' ', i);
        l.Command = (cmdEnd < 0 ? line.Substring(i) : line.Substring(i, cmdEnd - i)).ToUpperInvariant();
        i = cmdEnd < 0 ? line.Length : cmdEnd + 1;
        while (i < line.Length)
        {
            if (line[i] == ' ') { i++; continue; }
            if (line[i] == ':') { l.Params.Add(line.Substring(i + 1)); break; }
            int sp = line.IndexOf(' ', i);
            if (sp < 0) { l.Params.Add(line.Substring(i)); break; }
            l.Params.Add(line.Substring(i, sp - i));
            i = sp + 1;
        }
        return l;
    }
}

/// <summary>The IRC connection: a socket, a reader thread and a queue of lines.</summary>
public sealed class IrcClient
{
    INetStream? _s;
    Thread? _reader;
    volatile bool _stop;
    readonly object _lock = new object();
    readonly Queue<string> _in = new Queue<string>();
    public string Nick = "";
    public string Error = "";
    public bool Connected { get; private set; }

    public bool Connect(string host, int port, string nick, bool tls)
    {
        Close();
        Error = "";
        try
        {
            if (tls) _s = TlsConnection.Connect(host, port, 15000);
            else _s = TcpConnection.Connect(host, port, 10000);
            _s.TimeoutMs = 400;
        }
        catch (Exception e) { Error = e.Message; return false; }
        Nick = nick;
        Connected = true;
        _stop = false;
        Send("NICK " + nick);
        Send("USER " + nick + " 0 * :Buitenzorg Obrolan");
        _reader = new Thread(ReadLoop);
        _reader.Start();
        return true;
    }

    void ReadLoop()
    {
        var buf = new byte[4096];
        var line = new List<byte>();
        while (!_stop)
        {
            var s = _s;
            if (s == null) break;
            int n;
            try { n = s.Read(buf, 0, buf.Length); }
            catch (Exception e)
            {
                if (e.Message.StartsWith("waktu habis")) continue; // quiet: keep waiting
                Error = e.Message;
                n = 0;
            }
            if (n <= 0) break;
            for (int i = 0; i < n; i++)
            {
                byte b = buf[i];
                if (b == '\n')
                {
                    string text = Encoding.UTF8.GetString(line.ToArray()).TrimEnd('\r');
                    line.Clear();
                    if (text.StartsWith("PING")) { Send("PONG" + text.Substring(4)); continue; }
                    lock (_lock) _in.Enqueue(text);
                }
                else if (line.Count < 4096) line.Add(b);
            }
        }
        Connected = false;
        lock (_lock) _in.Enqueue(":* CLOSED :" + (Error.Length > 0 ? Error : "sambungan ditutup"));
    }

    /// <summary>Lines received since the last call.</summary>
    public List<string> Take()
    {
        var list = new List<string>();
        lock (_lock) while (_in.Count > 0) list.Add(_in.Dequeue());
        return list;
    }

    public bool Send(string line)
    {
        var s = _s;
        if (s == null) return false;
        try
        {
            var b = Encoding.UTF8.GetBytes(line + "\r\n");
            lock (this) s.Write(b, 0, b.Length);
            return true;
        }
        catch (Exception e) { Error = e.Message; return false; }
    }

    public void Close()
    {
        if (_s != null && Connected) Send("QUIT :Buitenzorg");
        _stop = true;
        var s = _s;
        _s = null;
        try { s?.Close(); } catch (Exception) { }
        _reader?.Join();
        _reader = null;
        Connected = false;
    }
}

public sealed class ChatLine
{
    public string Who;
    public string Text;
    public bool Mine;
    public ChatLine(string who, string text, bool mine = false) { Who = who; Text = text; Mine = mine; }
    public override string ToString() => Who.Length == 0 ? "-- " + Text : (Who.StartsWith("*") ? Who + " " + Text : "<" + Who + "> " + Text);
}

public sealed class ObrolApp
{
    public Window W { get; }
    public readonly string Dir;
    public readonly IrcClient Irc = new IrcClient();
    public readonly ObservableCollection<ChatLine> Lines = new ObservableCollection<ChatLine>();
    public readonly ObservableCollection<string> Members = new ObservableCollection<string>();
    public string Channel = "";
    public int Mentions;
    readonly TextBox _server = new TextBox { Text = "irc.libera.chat" };
    readonly TextBox _port = new TextBox { Text = "6697", Width = 60 };
    readonly TextBox _nick = new TextBox { Text = "tamu_bz", Width = 110 };
    readonly TextBox _chan = new TextBox { Text = "#buitenzorg", Width = 120 };
    readonly TextBox _msg = new TextBox { Watermark = "Pesan, atau /join #kanal, /nick, /msg, /me" };
    readonly ListBox _list, _members;
    readonly TextBlock _status = new TextBlock("Belum tersambung");
    readonly DispatcherTimer _timer = new DispatcherTimer(150);
    readonly List<string> _names = new List<string>();

    readonly bool _headless;
    public ObrolApp(bool headless, string? dir = null)
    {
        _headless = headless;
        Dir = dir ?? "/disk/APPDATA/MOBROL";
        W = new Window("Obrolan", 640, 520, headless);
        _list = new ListBox { ItemsSource = Lines };
        _members = new ListBox { ItemsSource = Members, Width = 130 };
        var connect = new Button("Sambung", _ => Connect()).WithClass("accent");
        var send = new Button("Kirim", _ => SendTyped()).WithClass("accent");
        _msg.KeyDown += (_, e) => { if (e.Key == '\r' || e.Key == '\n') { SendTyped(); e.Handled = true; } };

        var top = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Margin = new Thickness(0, 0, 0, 8) };
        _server.Width = 150;
        top.Children.Add(_server);
        top.Children.Add(_port);
        top.Children.Add(_nick);
        top.Children.Add(_chan);
        top.Children.Add(connect);
        var input = new DockPanel { Margin = new Thickness(0, 8, 0, 0) };
        input.Children.Add(send.DockTo(Dock.Right));
        input.Children.Add(_msg);
        var middle = new DockPanel();
        middle.Children.Add(_members.DockTo(Dock.Right));
        middle.Children.Add(_list);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Internet", "Obrolan", IconKind.Leaf, 0x7CC4A4).DockTo(Dock.Top));
        root.Children.Add(top.DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(input.DockTo(Dock.Bottom));
        root.Children.Add(middle);
        W.Content = root;
        W.Closing += () => { SaveConfig(); Irc.Close(); };
        _timer.Tick += Pump;
        LoadConfig();
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    string ConfigFile => Dir + "/OBROLAN.CFG";

    void LoadConfig()
    {
        if (Files.Size(ConfigFile) < 0) return;
        foreach (var raw in Files.ReadAllText(ConfigFile).Split('\n'))
        {
            int eq = raw.IndexOf('=');
            if (eq <= 0) continue;
            string k = raw.Substring(0, eq).Trim(), v = raw.Substring(eq + 1).Trim();
            if (k == "server") _server.Text = v;
            else if (k == "port") _port.Text = v;
            else if (k == "nick") _nick.Text = v;
            else if (k == "channel") _chan.Text = v;
        }
    }

    public bool SaveConfig()
    {
        Files.CreateDirectory("/disk/APPDATA");
        Files.CreateDirectory(Dir);
        return Files.WriteAllText(ConfigFile, "server=" + _server.Text + "\nport=" + _port.Text + "\nnick=" + _nick.Text + "\nchannel=" + _chan.Text + "\n");
    }

    public void Configure(string server, int port, string nick, string channel)
    {
        _server.Text = server;
        _port.Text = port.ToString();
        _nick.Text = nick;
        _chan.Text = channel;
    }

    public bool Connect()
    {
        if (!int.TryParse(_port.Text.Trim(), out int port) || port <= 0 || port > 65535) { _status.Text = "Port tidak sah"; return false; }
        string nick = _nick.Text.Trim();
        if (nick.Length == 0 || nick.IndexOf(' ') >= 0) { _status.Text = "Nama panggilan tanpa spasi"; return false; }
        Lines.Clear();
        Members.Clear();
        Add("", "Menyambung ke " + _server.Text.Trim() + ":" + port + (port == 6697 ? " (TLS)" : "") + " ...");
        if (!Irc.Connect(_server.Text.Trim(), port, nick, port == 6697 || port == 7000))
        {
            Add("", "Gagal: " + Irc.Error);
            _status.Text = "Gagal tersambung";
            return false;
        }
        SaveConfig();
        _timer.Start();
        _status.Text = "Tersambung sebagai " + nick;
        return true;
    }

    void Add(string who, string text, bool mine = false)
    {
        Lines.Add(new ChatLine(who, text, mine));
        while (Lines.Count > 500) Lines.RemoveAt(0);
        _list.SelectedIndex = Lines.Count - 1; // follow the newest line
    }

    public void SendTyped()
    {
        string t = _msg.Text.Trim();
        if (t.Length == 0) return;
        _msg.Text = "";
        Command(t);
    }

    /// <summary>A line typed by the user: a /command or a message to the channel.</summary>
    public bool Command(string t)
    {
        if (!Irc.Connected) { Add("", "Belum tersambung"); return false; }
        if (!t.StartsWith("/"))
        {
            if (Channel.Length == 0) { Add("", "Masuk kanal dulu: /join #kanal"); return false; }
            Irc.Send("PRIVMSG " + Channel + " :" + t);
            Add(Irc.Nick, t, true);
            return true;
        }
        int sp = t.IndexOf(' ');
        string cmd = (sp < 0 ? t.Substring(1) : t.Substring(1, sp - 1)).ToLowerInvariant();
        string arg = sp < 0 ? "" : t.Substring(sp + 1).Trim();
        switch (cmd)
        {
            case "join":
            case "j":
                if (arg.Length == 0) return false;
                if (!arg.StartsWith("#")) arg = "#" + arg;
                if (Channel.Length > 0) Irc.Send("PART " + Channel);
                return Irc.Send("JOIN " + arg);
            case "part":
                if (Channel.Length == 0) return false;
                Irc.Send("PART " + Channel);
                Channel = "";
                Members.Clear();
                return true;
            case "nick":
                return arg.Length > 0 && Irc.Send("NICK " + arg);
            case "msg":
            {
                int s2 = arg.IndexOf(' ');
                if (s2 <= 0) return false;
                Irc.Send("PRIVMSG " + arg.Substring(0, s2) + " :" + arg.Substring(s2 + 1));
                Add("-> " + arg.Substring(0, s2), arg.Substring(s2 + 1), true);
                return true;
            }
            case "me":
                if (Channel.Length == 0) return false;
                Irc.Send("PRIVMSG " + Channel + " :\u0001ACTION " + arg + "\u0001");
                Add("* " + Irc.Nick, arg, true);
                return true;
            case "quit":
                Irc.Close();
                Add("", "Keluar dari server");
                return true;
            default:
                return Irc.Send(t.Substring(1)); // a raw IRC command
        }
    }

    /// <summary>Show what arrived (UI timer; tests call it directly).</summary>
    public void Pump()
    {
        foreach (var raw in Irc.Take()) Handle(IrcLine.Parse(raw));
        if (!Irc.Connected && !_timer.IsEnabled) return;
        if (!Irc.Connected) { _timer.Stop(); _status.Text = "Terputus"; }
    }

    void Handle(IrcLine l)
    {
        switch (l.Command)
        {
            case "001":
                Add("", "Selamat datang: " + l.Last);
                if (_chan.Text.Trim().StartsWith("#")) Irc.Send("JOIN " + _chan.Text.Trim());
                break;
            case "433":
                Irc.Nick += "_";
                Irc.Send("NICK " + Irc.Nick);
                Add("", "Nama sudah dipakai, mencoba " + Irc.Nick);
                break;
            case "JOIN":
                if (l.Nick == Irc.Nick) { Channel = l.Last; Members.Clear(); _status.Text = "Di " + Channel + " sebagai " + Irc.Nick; Add("", "Masuk " + Channel); }
                else { Add("", l.Nick + " masuk"); AddMember(l.Nick); }
                break;
            case "PART":
            case "QUIT":
                if (l.Nick != Irc.Nick) { Add("", l.Nick + " keluar"); RemoveMember(l.Nick); }
                break;
            case "NICK":
                if (l.Nick == Irc.Nick) Irc.Nick = l.Last;
                RemoveMember(l.Nick);
                AddMember(l.Last);
                Add("", l.Nick + " sekarang " + l.Last);
                break;
            case "353":
                foreach (var n in l.Last.Split(' ')) if (n.Length > 0) _names.Add(n.TrimStart('@', '+', '%', '~', '&'));
                break;
            case "366":
                Members.Clear();
                _names.Sort(string.CompareOrdinal);
                foreach (var n in _names) Members.Add(n);
                _names.Clear();
                break;
            case "PRIVMSG":
            case "NOTICE":
            {
                string text = l.Last;
                string target = l.Params.Count > 0 ? l.Params[0] : "";
                // (char checks: a culture StartsWith("\u0001") ignores the control char and matches everything)
                bool ctcp = text.Length > 0 && text[0] == (char)1;
                if (ctcp && text.Length > 8 && text.Substring(1, 7) == "ACTION ") Add("* " + l.Nick, text.Substring(8).TrimEnd((char)1));
                else if (ctcp) break; // other CTCP: ignored
                else Add(target == Irc.Nick && l.Command == "PRIVMSG" ? l.Nick + " (pribadi)" : l.Nick, text);
                if (Irc.Nick.Length > 0 && (target == Irc.Nick || text.IndexOf(Irc.Nick, StringComparison.OrdinalIgnoreCase) >= 0))
                {
                    Mentions++;
                    if (!_headless) Notifications.Show("Obrolan: " + l.Nick, text);
                }
                break;
            }
            case "CLOSED":
                Add("", l.Last);
                break;
            default:
                // numeric replies with text (MOTD, errors) are shown; the rest skipped
                if (l.Command.Length == 3 && char.IsDigit(l.Command[0]) && l.Params.Count > 1 && l.Command[0] != '0' || l.Command == "ERROR")
                    Add("", l.Last);
                break;
        }
    }

    void AddMember(string n)
    {
        if (Members.IndexOf(n) < 0) Members.Add(n);
    }

    void RemoveMember(string n)
    {
        int i = Members.IndexOf(n);
        if (i >= 0) Members.RemoveAt(i);
    }

    // ---- self-test: a tiny IRC server on this machine ----

    const int TestPort = 8094;

    static void Serve(TcpServer server, Func<bool> stop, List<string> got)
    {
        while (!stop())
        {
            var c = server.Accept(50);
            if (c == null) continue;
            c.TimeoutMs = 300;
            var pending = new StringBuilder();
            string nick = "";
            var buf = new byte[2048];
            bool open = true;
            void Say(string s) { var b = Encoding.UTF8.GetBytes(s + "\r\n"); c.Write(b, 0, b.Length); }
            while (open && !stop())
            {
                int n;
                try { n = c.Read(buf, 0, buf.Length); }
                catch (Exception) { continue; }
                if (n <= 0) break;
                pending.Append(Encoding.UTF8.GetString(buf, 0, n));
                string all = pending.ToString();
                int nl;
                while ((nl = all.IndexOf('\n')) >= 0)
                {
                    string line = all.Substring(0, nl).TrimEnd('\r');
                    all = all.Substring(nl + 1);
                    lock (got) got.Add(line);
                    var l = IrcLine.Parse(line);
                    if (l.Command == "NICK")
                    {
                        if (nick.Length == 0 && l.Last == "dipakai") { Say(":uji.server 433 * dipakai :Nickname is already in use"); continue; }
                        if (nick.Length > 0) Say(":" + nick + "!u@h NICK :" + l.Last);
                        nick = l.Last;
                    }
                    else if (l.Command == "USER")
                    {
                        Say("PING :uji");
                        Say(":uji.server 001 " + nick + " :Selamat datang di server uji");
                    }
                    else if (l.Command == "JOIN")
                    {
                        Say(":" + nick + "!u@h JOIN :" + l.Last);
                        Say(":uji.server 353 " + nick + " = " + l.Last + " :@kodok " + nick + " +melati");
                        Say(":uji.server 366 " + nick + " " + l.Last + " :End of NAMES");
                        Say(":kodok!k@h PRIVMSG " + l.Last + " :halo " + nick + ", apa kabar?");
                        Say(":melati!m@h PRIVMSG " + l.Last + " :\u0001ACTION menyiram tanaman\u0001");
                    }
                    else if (l.Command == "PRIVMSG" && l.Last == "keluar dong")
                        Say(":kodok!k@h PART " + l.Params[0]);
                    else if (l.Command == "QUIT") { open = false; break; }
                }
                pending.Clear();
                pending.Append(all);
            }
            c.Close();
        }
        server.Close();
    }

    static bool WaitFor(ObrolApp app, Func<bool> cond)
    {
        for (int i = 0; i < 100; i++)
        {
            app.Pump();
            if (cond()) return true;
            Thread.Sleep(30);
        }
        return false;
    }

    /// <summary>Wait (up to 3 s) until the test server received a matching line.</summary>
    static bool Heard(List<string> got, Func<string, bool> match)
    {
        for (int i = 0; i < 100; i++)
        {
            lock (got) foreach (var l in got) if (match(l)) return true;
            Thread.Sleep(30);
        }
        return false;
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Obrolan");
        var p = IrcLine.Parse(":kodok!k@host PRIVMSG #kebun :halo semua");
        c.That(p.Nick == "kodok" && p.Command == "PRIVMSG" && p.Params.Count == 2 && p.Params[0] == "#kebun" && p.Last == "halo semua", "an IRC line parses");
        var q = IrcLine.Parse("@time=x :srv 353 me = #k :a b");
        c.That(q.Command == "353" && q.Last == "a b" && q.Params.Count == 4, "tags skipped, numeric with params");

        string dir = Files.ScratchDirectory + "/OBROLTES";
        Files.CreateDirectory(dir);
        bool stop = false;
        var got = new List<string>();
        TcpServer? server = null;
        try { server = new TcpServer(TestPort); } catch (Exception e) { c.That(false, "listen on " + TestPort + ": " + e.Message); }
        if (server != null)
        {
            var th = new Thread(() => Serve(server, () => stop, got));
            th.Start();
            try
            {
                var app = new ObrolApp(true, dir);
                app.Configure("localhost", TestPort, "dipakai", "#kebun");
                c.That(app.Connect(), "connected (" + app.Irc.Error + ")");
                c.That(WaitFor(app, () => app.Channel == "#kebun" && app.Members.Count == 3), "a taken nick got '_', joined #kebun with 3 members");
                c.That(app.Irc.Nick == "dipakai_", "nick is now " + app.Irc.Nick);
                c.That(WaitFor(app, () => app.Lines.Count > 0 && app.Lines[app.Lines.Count - 1].Who == "* melati"), "a channel message and a /me action arrived");
                string seen = ""; foreach (var ln in app.Lines) seen += ln + " | ";
                c.That(app.Mentions >= 1, "the message naming us counts as a mention" + (app.Mentions >= 1 ? "" : ": " + seen));
                bool pong = Heard(got, l => l == "PONG :uji");
                c.That(pong, "PING answered");
                c.That(app.Command("keluar dong") && WaitFor(app, () => app.Members.IndexOf("kodok") < 0), "our message went out; the reply (PART) updated the members");
                c.That(app.Command("/nick pakis") && WaitFor(app, () => app.Irc.Nick == "pakis"), "/nick renamed us");
                c.That(app.Command("/me melambai"), "/me");
                bool action = Heard(got, l => l == "PRIVMSG #kebun :\u0001ACTION melambai\u0001");
                c.That(action, "/me sent as a CTCP ACTION");
                app.W.RenderFrame();
                c.That(app.W.LastFrameOps > 20, "the window drew " + app.W.LastFrameOps + " ops");
                app.Irc.Close();
                bool quit = Heard(got, l => l.StartsWith("QUIT"));
                c.That(quit && !app.Irc.Connected, "QUIT sent on close");
                c.That(new ObrolApp(true, dir).Lines.Count == 0 && Files.ReadAllText(dir + "/OBROLAN.CFG").Contains("port=" + TestPort), "settings saved");
            }
            catch (Exception e) { c.That(false, "chat: " + e.Message); }
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
            var c = ObrolApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var o = new ObrolApp(headless: false);
        app.Run(o.W);
        return 0;
    }
}
