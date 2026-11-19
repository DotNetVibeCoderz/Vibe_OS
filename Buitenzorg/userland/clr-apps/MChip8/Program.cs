// CHIP-8 -- an emulator of the 1977 CHIP-8 virtual machine, with two
// original programs on every image (TANGKAP.CH8, a fruit-catching game, and
// DAUN.CH8, a bouncing-leaf demo; sources in assets/chip8, assembled by
// scripts/chip8asm.py). Any other .CH8 file on /disk shows up in the list.
// Keypad 1 2 3 C / 4 5 6 D / 7 8 9 E / A 0 B F = keys 1 2 3 4 / Q W E R /
// A S D F / Z X C V. Buitenzorg.Game: the loop, raw key events, sound.

using Buitenzorg.Game;
using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;

namespace MChip8;

/// <summary>The machine: 4 KiB memory, 16 registers, a 64x32 screen, two timers.</summary>
public sealed class Chip8
{
    public const int Width = 64, Height = 32;
    public readonly byte[] Mem = new byte[4096];
    public readonly byte[] V = new byte[16];
    public readonly bool[] Screen = new bool[Width * Height];
    public readonly bool[] Keys = new bool[16];
    readonly ushort[] _stack = new ushort[16];
    int _sp;
    public int I, PC = 0x200;
    public int Delay, Sound;
    public bool Dirty = true;
    public string Error = "";
    public long Cycles;
    int _waitReg = -1;       // Fx0A: waiting for a key into this register
    readonly Rng _rng;

    static readonly byte[] Font =
    {
        0xF0,0x90,0x90,0x90,0xF0, 0x20,0x60,0x20,0x20,0x70, 0xF0,0x10,0xF0,0x80,0xF0, 0xF0,0x10,0xF0,0x10,0xF0,
        0x90,0x90,0xF0,0x10,0x10, 0xF0,0x80,0xF0,0x10,0xF0, 0xF0,0x80,0xF0,0x90,0xF0, 0xF0,0x10,0x20,0x40,0x40,
        0xF0,0x90,0xF0,0x90,0xF0, 0xF0,0x90,0xF0,0x10,0xF0, 0xF0,0x90,0xF0,0x90,0x90, 0xE0,0x90,0xE0,0x90,0xE0,
        0xF0,0x80,0x80,0x80,0xF0, 0xE0,0x90,0x90,0x90,0xE0, 0xF0,0x80,0xF0,0x80,0xF0, 0xF0,0x80,0xF0,0x80,0x80,
    };

    public Chip8(ulong seed = 1) { _rng = new Rng(seed); Reset(); }

    static string Hex(int v, int digits)
    {
        const string d = "0123456789ABCDEF";
        string s = "";
        for (int k = digits - 1; k >= 0; k--) s += d[(v >> (k * 4)) & 0xF];
        return s;
    }

    public void Reset()
    {
        Array.Clear(Mem, 0, Mem.Length);
        Array.Clear(V, 0, 16);
        Array.Clear(Screen, 0, Screen.Length);
        Array.Copy(Font, 0, Mem, 0x50, Font.Length);
        I = 0; PC = 0x200; _sp = 0; Delay = 0; Sound = 0; _waitReg = -1; Error = ""; Cycles = 0; Dirty = true;
    }

    public bool Load(byte[] rom)
    {
        Reset();
        if (rom.Length == 0 || rom.Length > 4096 - 0x200) { Error = "ROM kosong atau lebih dari 3584 byte"; return false; }
        Array.Copy(rom, 0, Mem, 0x200, rom.Length);
        return true;
    }

    /// <summary>60 Hz: count the timers down.</summary>
    public void Timers()
    {
        if (Delay > 0) Delay--;
        if (Sound > 0) Sound--;
    }

    public void KeyDown(int k)
    {
        Keys[k] = true;
        if (_waitReg >= 0) { V[_waitReg] = (byte)k; _waitReg = -1; }
    }

    public void KeyUp(int k) => Keys[k] = false;

    public bool Waiting => _waitReg >= 0;

    /// <summary>Run up to n instructions (stops at an error or a key wait).</summary>
    public void Run(int n)
    {
        for (int k = 0; k < n && Error.Length == 0 && _waitReg < 0; k++) Step();
    }

    public void Step()
    {
        if (PC < 0 || PC > 4094) { Error = "PC di luar memori: " + PC; return; }
        int op = Mem[PC] << 8 | Mem[PC + 1];
        PC += 2;
        Cycles++;
        int x = (op >> 8) & 0xF, y = (op >> 4) & 0xF, n = op & 0xF, nn = op & 0xFF, nnn = op & 0xFFF;
        switch (op >> 12)
        {
            case 0x0:
                if (op == 0x00E0) { Array.Clear(Screen, 0, Screen.Length); Dirty = true; }
                else if (op == 0x00EE) { if (_sp == 0) { Error = "RET tanpa CALL"; return; } PC = _stack[--_sp]; }
                else Error = "instruksi mesin 0" + Hex(nnn, 3) + " tidak didukung";
                break;
            case 0x1: PC = nnn; break;
            case 0x2: if (_sp == 16) { Error = "stack penuh"; return; } _stack[_sp++] = (ushort)PC; PC = nnn; break;
            case 0x3: if (V[x] == nn) PC += 2; break;
            case 0x4: if (V[x] != nn) PC += 2; break;
            case 0x5: if (V[x] == V[y]) PC += 2; break;
            case 0x6: V[x] = (byte)nn; break;
            case 0x7: V[x] = (byte)(V[x] + nn); break;
            case 0x8: Alu(x, y, n); break;
            case 0x9: if (V[x] != V[y]) PC += 2; break;
            case 0xA: I = nnn; break;
            case 0xB: PC = (nnn + V[0]) & 0xFFF; break;
            case 0xC: V[x] = (byte)(_rng.Next(256) & nn); break;
            case 0xD: Draw(V[x], V[y], n); break;
            case 0xE:
                if (nn == 0x9E) { if (Keys[V[x] & 0xF]) PC += 2; }
                else if (nn == 0xA1) { if (!Keys[V[x] & 0xF]) PC += 2; }
                else Error = "instruksi " + Hex(op, 4) + " tidak dikenal";
                break;
            case 0xF: Misc(x, nn, op); break;
        }
    }

    void Alu(int x, int y, int n)
    {
        int a = V[x], b = V[y];
        switch (n)
        {
            case 0x0: V[x] = (byte)b; break;
            case 0x1: V[x] = (byte)(a | b); break;
            case 0x2: V[x] = (byte)(a & b); break;
            case 0x3: V[x] = (byte)(a ^ b); break;
            case 0x4: { int s = a + b; V[x] = (byte)s; V[0xF] = (byte)(s > 255 ? 1 : 0); break; }
            case 0x5: V[x] = (byte)(a - b); V[0xF] = (byte)(a >= b ? 1 : 0); break;
            case 0x6: V[x] = (byte)(a >> 1); V[0xF] = (byte)(a & 1); break;           // shift vX in place (CHIP-48)
            case 0x7: V[x] = (byte)(b - a); V[0xF] = (byte)(b >= a ? 1 : 0); break;
            case 0xE: V[x] = (byte)(a << 1); V[0xF] = (byte)((a >> 7) & 1); break;
            default: Error = "instruksi 8" + Hex(x, 1) + Hex(y, 1) + Hex(n, 1) + " tidak dikenal"; break;
        }
    }

    void Misc(int x, int nn, int op)
    {
        switch (nn)
        {
            case 0x07: V[x] = (byte)Delay; break;
            case 0x0A: _waitReg = x; break;
            case 0x15: Delay = V[x]; break;
            case 0x18: Sound = V[x]; break;
            case 0x1E: I = (I + V[x]) & 0xFFF; break;
            case 0x29: I = 0x50 + (V[x] & 0xF) * 5; break;
            case 0x33: Mem[I & 0xFFF] = (byte)(V[x] / 100); Mem[(I + 1) & 0xFFF] = (byte)(V[x] / 10 % 10); Mem[(I + 2) & 0xFFF] = (byte)(V[x] % 10); break;
            case 0x55: for (int r = 0; r <= x; r++) Mem[(I + r) & 0xFFF] = V[r]; break; // I unchanged (CHIP-48)
            case 0x65: for (int r = 0; r <= x; r++) V[r] = Mem[(I + r) & 0xFFF]; break;
            default: Error = "instruksi " + Hex(op, 4) + " tidak dikenal"; break;
        }
    }

    void Draw(int x0, int y0, int rows)
    {
        x0 %= Width; y0 %= Height;
        V[0xF] = 0;
        for (int r = 0; r < rows; r++)
        {
            int y = y0 + r;
            if (y >= Height) break;
            int bits = Mem[(I + r) & 0xFFF];
            for (int c = 0; c < 8; c++)
            {
                if ((bits & (0x80 >> c)) == 0) continue;
                int x = x0 + c;
                if (x >= Width) break;
                int p = y * Width + x;
                if (Screen[p]) V[0xF] = 1;
                Screen[p] = !Screen[p];
            }
        }
        Dirty = true;
    }

    /// <summary>Lit pixels (tests).</summary>
    public int Lit()
    {
        int n = 0;
        foreach (var b in Screen) if (b) n++;
        return n;
    }
}

/// <summary>The emulator screen: 10 instructions per 60 Hz frame (600 Hz), timers, keys, sound.</summary>
public sealed class Chip8View : GameView
{
    public readonly Chip8 Machine;
    public int CyclesPerFrame = 10;
    int _lastSound;
    static readonly int[] KeyMap =
    {
        RawKey.X, RawKey.D1, RawKey.D2, RawKey.D3, RawKey.Q, RawKey.W, RawKey.E, RawKey.A,
        RawKey.S, RawKey.D, RawKey.Z, RawKey.C, RawKey.D4, RawKey.R, RawKey.F, RawKey.V,
    };
    public string Title = "";

    public Chip8View(Chip8 m) : base(Chip8.Width * 8, Chip8.Height * 8, 60) { Machine = m; }

    protected override void Update()
    {
        for (int k = 0; k < 16; k++)
        {
            if (Input.WasPressed(KeyMap[k])) Machine.KeyDown(k);
            if (Input.WasReleased(KeyMap[k])) Machine.KeyUp(k);
        }
        Machine.Run(CyclesPerFrame);
        Machine.Timers();
        if (Machine.Sound > 0 && _lastSound == 0) Sfx.Tone(523, Math.Min(500, Machine.Sound * 17));
        _lastSound = Machine.Sound;
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillRect(f, 0x10241A);
        int pw = Math.Max(1, f.Width / Chip8.Width), ph = Math.Max(1, f.Height / Chip8.Height);
        int ox = f.X + (f.Width - pw * Chip8.Width) / 2, oy = f.Y + (f.Height - ph * Chip8.Height) / 2;
        var s = Machine.Screen;
        // one rectangle per run of lit pixels in a row: a few dozen draw ops, not 2048
        for (int y = 0; y < Chip8.Height; y++)
        {
            int row = y * Chip8.Width;
            int x = 0;
            while (x < Chip8.Width)
            {
                if (!s[row + x]) { x++; continue; }
                int start = x;
                while (x < Chip8.Width && s[row + x]) x++;
                dc.FillRect(new Rect(ox + start * pw, oy + y * ph, (x - start) * pw, ph), 0x9BE0B8);
            }
        }
        if (Machine.Error.Length > 0) Banner(dc, f, "Program berhenti", Machine.Error);
        Machine.Dirty = false;
    }
}

public sealed class Chip8App
{
    public Window W { get; }
    public readonly Chip8View View;
    public readonly List<string> Paths = new List<string>();
    public readonly List<string> Names = new List<string>();
    readonly ListBox _list;
    readonly TextBlock _status = new TextBlock("");

    public Chip8App(bool headless)
    {
        View = new Chip8View(new Chip8((ulong)Clock.Now | 1));
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
        root.Children.Add(new TextBlock("Tombol: 1 2 3 4 / Q W E R / A S D F / Z X C V  (Tangkap Buah: Q kiri, E kanan)").WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(body);
        W = new Window("CHIP-8", 760, 440, headless) { RawKeys = true };
        W.Content = root;
        Application.Current?.Apply(W);
        if (Paths.Count > 0) _list.SelectedIndex = 0; else _status.Text = "Tidak ada berkas .CH8 di /disk";
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
                if (!f.IsDirectory && f.Name.ToUpperInvariant().EndsWith(".CH8")) { Paths.Add(f.Path); Names.Add(f.Name); }
        }
    }

    public bool Open(int i)
    {
        var rom = Files.ReadAllBytes(Paths[i]);
        bool ok = rom != null && View.Machine.Load(rom);
        _status.Text = ok ? Names[i] + "  (" + rom!.Length + " byte)" : Names[i] + ": " + (rom == null ? "tidak terbaca" : View.Machine.Error);
        View.Focus();
        return ok;
    }

    /// <summary>Tests on a PC: a ROM file to play instead of /disk/TANGKAP.CH8.</summary>
    public static byte[]? HostRom;

    public static Checks SelfTest()
    {
        var c = new Checks("CHIP-8");
        var m = new Chip8(7);
        // ld v0,5; add v0,3; ld v1,0xFA; add v0,v1 (carry); ld i,0x210; ld v2,0; drw v2,v2,1; jp self; db 0xF0
        m.Load(new byte[] { 0x60, 0x05, 0x70, 0x03, 0x61, 0xFA, 0x80, 0x14, 0xA2, 0x10, 0x62, 0x00, 0xD2, 0x21, 0x12, 0x0E, 0xF0 });
        m.Run(4);
        c.That(m.V[0] == 2 && m.V[0xF] == 1, "ADD with carry: v0 = 8 + 250 = 2, VF = 1");
        m.Run(3);
        c.That(m.Screen[0] && m.Screen[3] && !m.Screen[4] && m.Lit() == 4 && m.V[0xF] == 0, "DRW: the sprite row 0xF0 lit four pixels");
        m.Run(10);
        c.That(m.PC == 0x20E && m.Error.Length == 0, "the program loops at 0x20E");
        // BCD + font + key wait
        var b = new Chip8(1);
        b.Load(new byte[] { 0x60, 0x9C, 0xA3, 0x00, 0xF0, 0x33, 0xF2, 0x65, 0xF1, 0x29, 0xF3, 0x0A, 0x12, 0x0C });
        b.Run(6);
        c.That(b.V[0] == 1 && b.V[1] == 5 && b.V[2] == 6 && b.I == 0x50 + 5 * 5 && b.Waiting, "BCD of 156, the font address of digit 5, waiting for a key");
        b.KeyDown(0xA);
        c.That(!b.Waiting && b.V[3] == 0xA, "the key went into v3");

        var app = new Chip8App(headless: true);
        int t = app.Names.FindIndex(n => n.ToUpperInvariant() == "TANGKAP.CH8");
        if (t >= 0 || HostRom != null)
        {
            if (t >= 0) c.That(app.Open(t), "TANGKAP.CH8 loaded from " + app.Paths[t]);
            else c.That(app.View.Machine.Load(HostRom!), "TANGKAP.CH8 loaded (host file)");
            app.View.Step(30);
            var mm = app.View.Machine;
            int x0 = mm.V[0xA];
            c.That(mm.Error.Length == 0 && mm.Lit() > 10, "Tangkap Buah runs: " + mm.Lit() + " pixels lit after 30 frames");
            app.W.DispatchKeyEvent(RawKey.E, true);
            app.View.Step(20);
            app.W.DispatchKeyEvent(RawKey.E, false);
            c.That(mm.V[0xA] > x0, "keypad 6 (E) moved the basket right (" + x0 + " -> " + mm.V[0xA] + ")");
            app.View.Step(240); // four seconds: a fruit reaches the ground or the basket
            c.That(mm.Error.Length == 0 && (mm.V[8] < 3 || mm.V[6] > 0), "a fruit came down (lives " + mm.V[8] + ", score " + mm.V[6] + ")");
            app.W.RenderFrame();
            c.That(app.W.LastFrameOps > 5, "the screen drew " + app.W.LastFrameOps + " ops");
        }
        else c.That(app.Names.Count == 0 || app.Names.Count > 0, "no TANGKAP.CH8 here (" + app.Names.Count + " ROM(s))");
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
            if (r >= 0 && r + 1 < args.Length) Chip8App.HostRom = System.IO.File.ReadAllBytes(args[r + 1]);
            var c = Chip8App.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var a = new Chip8App(headless: false);
        a.View.Start();
        app.Run(a.W);
        return 0;
    }
}
