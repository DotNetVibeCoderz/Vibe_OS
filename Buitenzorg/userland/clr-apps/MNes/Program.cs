// NES: a Famicom / NES emulator. The machine (6502 CPU, PPU, APU, mappers
// 0/2/3) is clrhost's native core `nes.rs` -- an interpreted 6502 would be
// far too slow -- and this app is its screen, controller and speaker. ROMs
// are *.NES files on /disk; the image carries KEBUN.NES ("Lebah Kebun"), an
// original homebrew game made by scripts/make-nes-rom.py. Commercial games
// are not included.
//
// Keys: arrows = d-pad, X = A, Z = B, Enter = Start, Right Shift / Space =
// Select; a gamepad works as well.

using Buitenzorg.Game;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;

namespace MNes;

static class NesNative
{
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int NesLoad(byte[] rom);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int NesFrame(int id, int buttons, int[] pixels);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int NesAudio(int id, byte[] output);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int NesPeek(int id, int address);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int NesInfo(int id, int what);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int NesFree(int id);

    static bool? _here;
    /// <summary>The native core exists (on Buitenzorg).</summary>
    public static bool Available
    {
        get
        {
            if (_here == null)
            {
                try { NesFree(0); _here = true; }
                catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _here = false; }
            }
            return _here.Value;
        }
    }
}

/// <summary>One cartridge in the native core.</summary>
public sealed class NesMachine : IDisposable
{
    public const int Width = 256, Height = 240;
    public const int A = 1, B = 2, Select = 4, Start = 8, Up = 16, Down = 32, Left = 64, Right = 128;
    public readonly int[] Pixels = new int[Width * Height];
    int _id;
    public string Error = "";
    public int Frames;
    readonly byte[] _audio = new byte[8192];

    /// <summary>The iNES header: (mapper, PRG KiB, CHR KiB), or null.</summary>
    public static (int mapper, int prg, int chr)? Header(byte[] rom)
    {
        if (rom.Length < 16 || rom[0] != 'N' || rom[1] != 'E' || rom[2] != 'S' || rom[3] != 0x1A) return null;
        return ((rom[6] >> 4) | (rom[7] & 0xF0), rom[4] * 16, rom[5] * 8);
    }

    public bool Load(byte[] rom)
    {
        Dispose();
        Error = "";
        Frames = 0;
        if (Header(rom) == null) { Error = "bukan berkas iNES"; return false; }
        if (!NesNative.Available) { Error = "inti NES hanya ada di Buitenzorg"; return false; }
        _id = NesNative.NesLoad(rom);
        if (_id == -2) { Error = "mapper " + NesNative.NesInfo(-1, 0) + " belum didukung (0, 2, 3)"; _id = 0; return false; }
        if (_id <= 0) { Error = "ROM rusak"; _id = 0; return false; }
        return true;
    }

    public bool Loaded => _id > 0;

    /// <summary>Run one frame with controller 1 held as `buttons`.</summary>
    public void Frame(int buttons)
    {
        if (_id <= 0) return;
        Frames = NesNative.NesFrame(_id, buttons, Pixels);
        if (NesNative.NesInfo(_id, 3) != 0 && Error.Length == 0) Error = "CPU berhenti (opcode tak dikenal)";
    }

    /// <summary>Run a frame into another buffer (timing tests: a 1-element array skips the picture copy).</summary>
    public void FrameInto(int buttons, int[] pixels) { if (_id > 0) Frames = NesNative.NesFrame(_id, buttons, pixels); }

    /// <summary>The sound of the frames run since the last call (48 kHz mono 16-bit).</summary>
    public int TakeAudio(out byte[] buffer)
    {
        buffer = _audio;
        return _id > 0 ? Math.Max(0, NesNative.NesAudio(_id, _audio)) : 0;
    }

    public int Peek(int address) => _id > 0 ? NesNative.NesPeek(_id, address) : -1;
    public int Mapper => _id > 0 ? NesNative.NesInfo(_id, 0) : -1;

    public void Dispose()
    {
        if (_id > 0) NesNative.NesFree(_id);
        _id = 0;
    }
}

/// <summary>The screen: one emulated frame per 60 Hz tick, sound to the mixer.</summary>
public sealed class NesView : GameView
{
    public readonly NesMachine Machine = new NesMachine();
    /// <summary>The zoom the screen was last drawn at; frames are rendered at it (1 = Machine.Pixels).</summary>
    int _scale = 1;
    int[] _scaled = new int[0];
    bool _zoomed;
    public int Zoom => _scale;
    /// <summary>A pixel of the zoomed picture (tests), or -1 when frames are not zoomed.</summary>
    public int ZoomedPixel(int x, int y) => _zoomed ? _scaled[y * _scale * NesMachine.Width * _scale + x * _scale] : -1;
    public int Buttons;
    /// <summary>Audio bytes handed to the mixer (tests).</summary>
    public long AudioBytes;
    const int Format = 48000 | 1 << 20 | 16 << 24;

    public NesView() : base(NesMachine.Width * 2, NesMachine.Height * 2, 60) { Focusable = true; }

    static readonly int[,] Map =
    {
        { RawKey.X, RawKey.PadA, NesMachine.A }, { RawKey.Z, RawKey.PadB, NesMachine.B },
        { RawKey.Space, RawKey.PadSelect, NesMachine.Select }, { RawKey.Enter, RawKey.PadStart, NesMachine.Start },
        { RawKey.Up, RawKey.PadUp, NesMachine.Up }, { RawKey.Down, RawKey.PadDown, NesMachine.Down },
        { RawKey.Left, RawKey.PadLeft, NesMachine.Left }, { RawKey.Right, RawKey.PadRight, NesMachine.Right },
    };

    public int HeldButtons()
    {
        int b = Buttons;
        for (int i = 0; i < Map.GetLength(0); i++)
            if (Input.IsDown(Map[i, 0]) || Input.IsDown(Map[i, 1])) b |= Map[i, 2];
        if (Input.IsDown(RawKey.RightShift)) b |= NesMachine.Select;
        return b;
    }

    protected override void Update()
    {
        if (!Machine.Loaded || Machine.Error.Length > 0) return;
        if (_scale > 1 && _scaled.Length == NesMachine.Width * NesMachine.Height * _scale * _scale) { Machine.FrameInto(HeldButtons(), _scaled); _zoomed = true; }
        else { Machine.Frame(HeldButtons()); _zoomed = false; }
        int n = Machine.TakeAudio(out var pcm);
        if (n > 0)
        {
            AudioBytes += n;
            if (GameEnv.Interactive) Buitenzorg.App.Audio.Write(pcm, 0, n, Format);
        }
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillRect(f, 0x101418);
        if (!Machine.Loaded)
        {
            Banner(dc, f, "NES", Machine.Error.Length > 0 ? Machine.Error : "Pilih ROM .NES di kiri");
            return;
        }
        int s = Math.Max(1, Math.Min(4, Math.Min(f.Width / NesMachine.Width, f.Height / NesMachine.Height)));
        int w = NesMachine.Width * s, h = NesMachine.Height * s;
        int x = f.X + (f.Width - w) / 2, y = f.Y + (f.Height - h) / 2;
        // the zoom is done by the native core (an interpreted loop over 61k pixels is far too slow):
        // the first frame at a new zoom shows the 1x picture, the next ones arrive zoomed
        if (s != _scale) { _scale = s; _scaled = s > 1 ? new int[w * h] : new int[0]; }
        if (s > 1 && Machine.Frames > 0 && _scaled.Length == w * h && _zoomed) dc.Blit(x, y, w, h, _scaled);
        else dc.Blit(f.X + (f.Width - NesMachine.Width) / 2, f.Y + (f.Height - NesMachine.Height) / 2, NesMachine.Width, NesMachine.Height, Machine.Pixels);
        if (Machine.Error.Length > 0) Banner(dc, f, "Program berhenti", Machine.Error);
    }
}

public sealed class NesApp
{
    public Window W { get; }
    public readonly NesView View = new NesView();
    public readonly List<string> Paths = new List<string>();
    public readonly List<string> Names = new List<string>();
    readonly ListBox _list;
    readonly TextBlock _status = new TextBlock("");

    public NesApp(bool headless)
    {
        Scan();
        _list = new ListBox { ItemsSource = Names, Width = 150 };
        _list.SelectionChanged += l => { if (l.SelectedIndex >= 0) Open(l.SelectedIndex); };
        var reset = new Button("Mulai ulang", _ => { if (_list.SelectedIndex >= 0) Open(_list.SelectedIndex); }).WithClass("subtle");
        var top = new DockPanel { Margin = new Thickness(0, 0, 0, 8) };
        top.Children.Add(reset.DockTo(Dock.Right));
        top.Children.Add(_status);
        var body = new DockPanel();
        body.Children.Add(_list.DockTo(Dock.Left));
        View.Margin = new Thickness(10, 0, 0, 0);
        body.Children.Add(View);
        var root = new DockPanel { Margin = new Thickness(12) };
        root.Children.Add(top.DockTo(Dock.Top));
        root.Children.Add(new TextBlock("Panah = arah, X = A, Z = B, Enter = Start, Spasi = Select (gamepad juga bisa)").WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(body);
        W = new Window("NES", 720, 600, headless) { RawKeys = true };
        W.Content = root;
        Application.Current?.Apply(W);
        if (Paths.Count > 0) _list.SelectedIndex = 0; else _status.Text = "Tidak ada berkas .NES di /disk";
        W.UpdateLayout();
        View.Focus();
    }

    public void Scan()
    {
        Paths.Clear(); Names.Clear();
        var dirs = Files.ScratchDirectory == "/disk" ? new[] { "/disk" } : new[] { "/disk", Files.ScratchDirectory };
        foreach (var d in dirs)
        {
            List<Buitenzorg.App.FileEntry> files;
            try { files = Files.List(d); } catch (Exception) { continue; }
            foreach (var f in files)
                if (!f.IsDirectory && f.Name.ToUpperInvariant().EndsWith(".NES")) { Paths.Add(f.Path); Names.Add(f.Name); }
        }
    }

    public bool Open(int i)
    {
        var rom = Files.ReadAllBytes(Paths[i]);
        bool ok = rom != null && View.Machine.Load(rom);
        var h = rom == null ? null : NesMachine.Header(rom);
        _status.Text = ok && h != null
            ? Names[i] + "  (mapper " + h.Value.mapper + ", PRG " + h.Value.prg + " KiB, CHR " + h.Value.chr + " KiB)"
            : Names[i] + ": " + (rom == null ? "tidak terbaca" : View.Machine.Error);
        View.Focus();
        return ok;
    }

    public static byte[]? HostRom;

    public static Checks SelfTest()
    {
        var c = new Checks("NES");
        c.That(NesMachine.Header(new byte[] { (byte)'N', (byte)'E', (byte)'S', 0x1A, 1, 1, 0x20, 0x00, 0, 0, 0, 0, 0, 0, 0, 0 }) is { mapper: 2, prg: 16, chr: 8 }
            && NesMachine.Header(new byte[16]) == null, "iNES header: mapper, PRG and CHR sizes");
        var app = new NesApp(headless: true);
        int k = app.Names.FindIndex(n => n.ToUpperInvariant() == "KEBUN.NES");
        byte[]? rom = k >= 0 ? Files.ReadAllBytes(app.Paths[k]) : HostRom;
        if (!NesNative.Available)
        {
            var m0 = new NesMachine();
            c.That(!m0.Load(rom ?? new byte[16]) && m0.Error.Length > 0, "off the device the core is absent and says so (" + m0.Error + ")");
            return c;
        }
        if (rom == null) { c.That(false, "KEBUN.NES not found"); return c; }
        if (k >= 0) c.That(app.Open(k), "KEBUN.NES loaded: " + app.View.Machine.Mapper);
        else c.That(app.View.Machine.Load(rom), "KEBUN.NES loaded (host file)");
        var m = app.View.Machine;
        var sw = System.Diagnostics.Stopwatch.StartNew();
        app.View.Step(30);
        long perFrame = sw.ElapsedMilliseconds / 30;
        sw.Restart();
        var tiny = new int[1];
        for (int i = 0; i < 10; i++) m.FrameInto(0, tiny);
        Console.WriteLine("NES: " + perFrame + " ms per frame through the view, " + sw.ElapsedMilliseconds / 10 + " ms per bare NesFrame (Stopwatch)");
        c.That(m.Frames == 40 && m.Error.Length == 0 && m.Peek(2) == 100 && m.Peek(4) == 140, "30 frames (" + perFrame + " ms each): the bee at x 100, the flower at x 140");
        int sky = m.Pixels[10 * 256 + 10] & 0xFFFFFF, grass = m.Pixels[100 * 256 + 10] & 0xFFFFFF;
        c.That(sky == 0x64B0FF && (grass == 0x0C9300 || grass == 0xFE6ECC || grass == 0xEA9E22), "the picture: sky " + sky.ToString("X6") + ", garden " + grass.ToString("X6"));
        app.W.DispatchKeyEvent(RawKey.Right, true);
        app.View.Step(45);
        app.W.DispatchKeyEvent(RawKey.Right, false);
        app.View.Step(2);
        c.That(m.Peek(2) == 145 && m.Peek(0x0D) == 1 && m.Peek(6) == 1, "the arrow key flew the bee to the flower: x " + m.Peek(2) + ", score " + m.Peek(7) + m.Peek(6));
        c.That(app.View.AudioBytes > 9000, "the catch chirped on the APU (" + app.View.AudioBytes + " bytes of sound)");
        c.That(m.Peek(4) != 140 || m.Peek(5) != 120, "a new flower opened at (" + m.Peek(4) + ", " + m.Peek(5) + ")");
        app.W.RenderFrame();
        c.That(app.W.LastFrameOps > 3, "the screen drew " + app.W.LastFrameOps + " ops");
        app.View.Step(1);
        int z = app.View.ZoomedPixel(10, 10) & 0xFFFFFF;
        c.That(app.View.Zoom == 1 || z == 0x64B0FF, "the core zoomed the picture " + app.View.Zoom + "x (sky " + z.ToString("X6") + ")");
        m.Dispose();
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
            int r = Array.IndexOf(args, "--rom");
            if (r >= 0 && r + 1 < args.Length) NesApp.HostRom = System.IO.File.ReadAllBytes(args[r + 1]);
            var c = NesApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var a = new NesApp(headless: false);
        a.View.Start();
        app.Run(a.W);
        a.View.Machine.Dispose();
        return 0;
    }
}
