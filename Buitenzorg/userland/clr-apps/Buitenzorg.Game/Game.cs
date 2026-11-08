// Buitenzorg.Game -- a small 2D game framework on Buitenzorg.UI.
//
// A game is a GameView: a control that owns a fixed-timestep loop (Update at
// a steady rate, Draw when the window renders), reads the keyboard as raw
// presses / releases (Window.RawKeys -> the KEY_EVENT syscall), and draws with
// the toolkit's DrawingContext (kernel draw ops: rectangles, rounded
// rectangles, gradients, lines, ellipses, text -- never per pixel in the
// interpreter). Around it: Input (held keys + edges), Rng (deterministic, for
// tests and replays), Collide (rectangles / circles), Particles, Sfx (the
// speaker through Buitenzorg.App.Audio) and HighScores (a file per game).
//
// Tests drive a game deterministically: GameView.Step(n) runs n updates
// without a clock, Window.SimulateKeyEvents / Input.Press feed keys.

using Buitenzorg.UI;

namespace Buitenzorg.Game;

/// <summary>Held keys and this frame's edges (pressed / released since the last update).</summary>
public sealed class Input
{
    readonly HashSet<int> _down = new HashSet<int>();
    readonly HashSet<int> _pressed = new HashSet<int>();
    readonly HashSet<int> _released = new HashSet<int>();

    public bool IsDown(int key) => _down.Contains(key);
    public bool WasPressed(int key) => _pressed.Contains(key);
    public bool WasReleased(int key) => _released.Contains(key);
    /// <summary>Any of the keys pressed this frame.</summary>
    public bool AnyPressed(params int[] keys) { foreach (var k in keys) if (_pressed.Contains(k)) return true; return false; }
    public bool AnyDown(params int[] keys) { foreach (var k in keys) if (_down.Contains(k)) return true; return false; }

    /// <summary>-1 / 0 / +1 from a pair of keys (e.g. Left / Right).</summary>
    public int Axis(int negative, int positive, int negative2 = 0, int positive2 = 0) =>
        (IsDown(positive) || (positive2 != 0 && IsDown(positive2)) ? 1 : 0) - (IsDown(negative) || (negative2 != 0 && IsDown(negative2)) ? 1 : 0);

    public void Press(int key) { if (_down.Add(key)) _pressed.Add(key); }
    public void Release(int key) { if (_down.Remove(key)) _released.Add(key); }
    /// <summary>After an update: edges are per frame.</summary>
    public void EndFrame() { _pressed.Clear(); _released.Clear(); }
    public void Clear() { _down.Clear(); _pressed.Clear(); _released.Clear(); }
}

/// <summary>xorshift64* -- deterministic, so a seeded game replays exactly.</summary>
public sealed class Rng
{
    ulong _s;
    public Rng(ulong seed) { _s = seed == 0 ? 0x9E3779B97F4A7C15UL : seed; }
    public ulong NextU64() { _s ^= _s >> 12; _s ^= _s << 25; _s ^= _s >> 27; return _s * 2685821657736338717UL; }
    /// <summary>0 .. n-1.</summary>
    public int Next(int n) => n <= 1 ? 0 : (int)(NextU64() % (ulong)n);
    public int Range(int lo, int hiInclusive) => lo + Next(hiInclusive - lo + 1);
    public double NextDouble() => (NextU64() >> 11) * (1.0 / (1UL << 53));
    public bool Chance(int percent) => Next(100) < percent;
    public void Shuffle<T>(IList<T> list)
    {
        for (int i = list.Count - 1; i > 0; i--) { int j = Next(i + 1); (list[i], list[j]) = (list[j], list[i]); }
    }
}

public struct Vec2
{
    public double X, Y;
    public Vec2(double x, double y) { X = x; Y = y; }
    public static Vec2 operator +(Vec2 a, Vec2 b) => new Vec2(a.X + b.X, a.Y + b.Y);
    public static Vec2 operator -(Vec2 a, Vec2 b) => new Vec2(a.X - b.X, a.Y - b.Y);
    public static Vec2 operator *(Vec2 a, double k) => new Vec2(a.X * k, a.Y * k);
    public double Length => Math.Sqrt(X * X + Y * Y);
    public Vec2 Normalized() { double l = Length; return l > 0 ? new Vec2(X / l, Y / l) : new Vec2(0, 0); }
    public override string ToString() => "(" + X.ToString("0.##") + ", " + Y.ToString("0.##") + ")";
}

/// <summary>Overlap tests in game units (doubles: positions move by fractions).</summary>
public static class Collide
{
    public static bool Rects(double ax, double ay, double aw, double ah, double bx, double by, double bw, double bh) =>
        ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah;

    public static bool Circles(Vec2 a, double ra, Vec2 b, double rb)
    {
        double dx = a.X - b.X, dy = a.Y - b.Y, r = ra + rb;
        return dx * dx + dy * dy < r * r;
    }

    /// <summary>A circle against a rectangle: whether they touch, and from which side
    /// the circle came (for bouncing): 0 none, 1 horizontal face, 2 vertical face.</summary>
    public static int CircleRect(Vec2 c, double r, double x, double y, double w, double h)
    {
        double nx = Math.Clamp(c.X, x, x + w), ny = Math.Clamp(c.Y, y, y + h);
        double dx = c.X - nx, dy = c.Y - ny;
        if (dx * dx + dy * dy >= r * r) return 0;
        // the face it is closer to after leaving: compare penetration depths
        double penX = Math.Min(Math.Abs(c.X + r - x), Math.Abs(x + w - (c.X - r)));
        double penY = Math.Min(Math.Abs(c.Y + r - y), Math.Abs(y + h - (c.Y - r)));
        return penY <= penX ? 1 : 2;
    }
}

/// <summary>Short-lived colored squares (sparks, crumbs, confetti).</summary>
public sealed class Particles
{
    sealed class P { public double X, Y, Vx, Vy, Life, Max; public int Color, Size; }
    readonly List<P> _list = new List<P>();
    public int Count => _list.Count;
    public int Limit { get; set; } = 160;

    public void Burst(Rng rng, double x, double y, int n, int color, double speed = 120, double life = 0.6, int size = 3)
    {
        for (int i = 0; i < n && _list.Count < Limit; i++)
        {
            double a = rng.NextDouble() * Math.PI * 2, s = speed * (0.4 + rng.NextDouble() * 0.6);
            _list.Add(new P { X = x, Y = y, Vx = Math.Cos(a) * s, Vy = Math.Sin(a) * s, Life = life, Max = life, Color = color, Size = size });
        }
    }

    public void Update(double dt, double gravity = 200)
    {
        for (int i = _list.Count - 1; i >= 0; i--)
        {
            var p = _list[i];
            p.Life -= dt;
            if (p.Life <= 0) { _list.RemoveAt(i); continue; }
            p.Vy += gravity * dt;
            p.X += p.Vx * dt;
            p.Y += p.Vy * dt;
        }
    }

    public void Draw(DrawingContext dc, int ox, int oy)
    {
        foreach (var p in _list)
        {
            int s = Math.Max(1, (int)(p.Size * p.Life / p.Max + 0.5));
            dc.FillRect(new Rect(ox + (int)p.X - s / 2, oy + (int)p.Y - s / 2, s, s), p.Color);
        }
    }

    public void Clear() => _list.Clear();
}

/// <summary>Where the game runs.</summary>
public static class GameEnv
{
    static bool? _interactive;
    /// <summary>A live desktop (false off the device and during the boot self-tests).</summary>
    public static bool Interactive
    {
        get
        {
            if (_interactive == null)
            {
                try { _interactive = Buitenzorg.App.Session.IsInteractive; }
                catch (Exception) { _interactive = false; }
            }
            return _interactive.Value;
        }
    }
}

/// <summary>Sound effects on the speaker (Buitenzorg.App.Audio); silent when muted or absent.</summary>
public static class Sfx
{
    public static bool Enabled { get; set; } = true;
    /// <summary>The last sounds asked for (tests check them without a speaker).</summary>
    public static readonly List<int> Played = new List<int>();

    public static void Tone(int hz, int ms)
    {
        Played.Add(hz);
        if (Played.Count > 64) Played.RemoveAt(0);
        if (!Enabled || !GameEnv.Interactive) return;
        try { Buitenzorg.App.Audio.Beep(hz, ms); } catch (Exception) { }
    }

    public static void Blip() => Tone(880, 30);
    public static void Hit() => Tone(440, 40);
    public static void Score() => Tone(1320, 60);
    public static void Lose() => Tone(196, 220);
}

/// <summary>The best scores of a game, kept in a small text file.</summary>
public sealed class HighScores
{
    readonly string _path;
    public List<int> Scores { get; } = new List<int>();
    public int Best => Scores.Count > 0 ? Scores[0] : 0;

    /// <summary>`name` = the game's 8.3 stem (the file is /disk/NAME.HI, or scratch off-device).</summary>
    public HighScores(string name, bool load = true)
    {
        _path = Buitenzorg.App.Files.ScratchDirectory + "/" + name.ToUpperInvariant() + ".HI";
        if (!load) return;
        try
        {
            foreach (var l in Buitenzorg.App.Files.ReadAllText(_path).Split('\n'))
                if (int.TryParse(l.Trim(), out int v)) Scores.Add(v);
        }
        catch (Exception) { }
        Scores.Sort((a, b) => b.CompareTo(a));
    }

    /// <summary>Record a score; whether it made the top 5.</summary>
    public bool Add(int score, bool save = true)
    {
        Scores.Add(score);
        Scores.Sort((a, b) => b.CompareTo(a));
        if (Scores.Count > 5) Scores.RemoveRange(5, Scores.Count - 5);
        bool top = Scores.Contains(score);
        if (save) { try { Buitenzorg.App.Files.WriteAllText(_path, string.Join("\n", Scores)); } catch (Exception) { } }
        return top;
    }
}

/// <summary>
/// A game: a control with a fixed-timestep loop. Subclasses implement
/// <see cref="Update"/> (game logic, <see cref="Dt"/> seconds per call) and
/// <see cref="Draw"/> (the picture, in game units scaled to the control).
/// Keys arrive through the window's raw key events into <see cref="Input"/>.
/// </summary>
public abstract class GameView : Control
{
    /// <summary>Logical size of the playfield (game units).</summary>
    public int FieldWidth { get; }
    public int FieldHeight { get; }
    /// <summary>Updates per second.</summary>
    public int Rate { get; }
    public double Dt => 1.0 / Rate;
    public Input Input { get; } = new Input();
    public bool Paused { get; set; }
    /// <summary>Updates run so far (tests, animations).</summary>
    public long Frames { get; private set; }
    /// <summary>Frames drawn, and the measured draws per second on the device.</summary>
    public long Draws { get; private set; }
    public int Fps { get; private set; }

    DispatcherTimer? _timer;
    long _last, _fpsStart;
    double _acc;
    int _fpsCount;

    protected GameView(int fieldWidth, int fieldHeight, int rate = 60)
    {
        FieldWidth = fieldWidth;
        FieldHeight = fieldHeight;
        Rate = rate;
        Focusable = true;
    }

    protected abstract void Update();
    protected abstract void Draw(DrawingContext dc, Rect field);

    /// <summary>Start the real-time loop (an interactive window).</summary>
    public void Start()
    {
        if (_timer != null) return;
        _last = Clock.Now;
        _fpsStart = _last;
        _timer = new DispatcherTimer(Math.Max(1, 1000 / Rate));
        _timer.Tick += () =>
        {
            long now = Clock.Now;
            _acc += (now - _last) / 1000.0;
            _last = now;
            // at most 5 updates per tick: a slow machine slows the game rather than freezing it
            int n = 0;
            while (_acc >= Dt && n < 5) { Tick(); _acc -= Dt; n++; }
            if (n == 5) _acc = 0;
            InvalidateVisual();
        };
        _timer.Start();
    }

    public void Stop() { _timer?.Stop(); _timer = null; }

    /// <summary>Tests: run `n` updates without the clock.</summary>
    public void Step(int n = 1) { for (int i = 0; i < n; i++) Tick(); }

    void Tick()
    {
        if (!Paused) { Update(); Frames++; }
        Input.EndFrame();
    }

    protected override void OnKeyEvent(RawKeyEventArgs e)
    {
        if (e.Pressed) Input.Press(e.Code); else Input.Release(e.Code);
        if (e.Pressed && e.Code == RawKey.P) Paused = !Paused;
        e.Handled = true;
    }

    protected override void OnLostFocus() { Input.Clear(); base.OnLostFocus(); }

    protected override Size MeasureOverride(Size a)
    {
        // keep the playfield's aspect ratio inside what the parent offers
        int w = Math.Min(a.Width, FieldWidth * 2), h = Math.Min(a.Height, FieldHeight * 2);
        if (w * FieldHeight > h * FieldWidth) w = h * FieldWidth / FieldHeight; else h = w * FieldHeight / FieldWidth;
        return new Size(Math.Max(w, 1), Math.Max(h, 1));
    }

    /// <summary>The playfield's rectangle inside the control (aspect kept, centered).</summary>
    public Rect FieldRect
    {
        get
        {
            var b = Bounds;
            int w = b.Width, h = b.Height;
            if (w * FieldHeight > h * FieldWidth) w = h * FieldWidth / FieldHeight; else h = w * FieldHeight / FieldWidth;
            return new Rect(b.X + (b.Width - w) / 2, b.Y + (b.Height - h) / 2, w, h);
        }
    }

    /// <summary>Game units to pixels inside <see cref="FieldRect"/>.</summary>
    public int Sx(double x) { var f = FieldRect; return f.X + (int)(x * f.Width / FieldWidth); }
    public int Sy(double y) { var f = FieldRect; return f.Y + (int)(y * f.Height / FieldHeight); }
    public int Sl(double len) => (int)(len * FieldRect.Width / FieldWidth + 0.5);
    public Rect SRect(double x, double y, double w, double h) => new Rect(Sx(x), Sy(y), Math.Max(1, Sl(w)), Math.Max(1, (int)(h * FieldRect.Height / FieldHeight + 0.5)));

    public override void Render(DrawingContext dc)
    {
        var f = FieldRect;
        dc.PushClip(f);
        Draw(dc, f);
        if (Paused) Banner(dc, f, "Jeda", "P untuk lanjut");
        dc.PopClip();
        Draws++;
        _fpsCount++;
        long now = Clock.Now;
        if (now - _fpsStart >= 1000) { Fps = (int)(_fpsCount * 1000 / Math.Max(1, now - _fpsStart)); _fpsCount = 0; _fpsStart = now; }
    }

    /// <summary>A centered message band (pause, game over).</summary>
    public static void Banner(DrawingContext dc, Rect f, string title, string sub)
    {
        var band = new Rect(f.X + f.Width / 8, f.Y + f.Height / 2 - 44, f.Width * 3 / 4, 88);
        dc.BlendRoundRect(band, 14, 0x0E1A14, 220);
        dc.StrokeRoundRect(band, 14, 0x7CC4A4, 2);
        int tw = Fonts.Measure(title, 2);
        dc.Text(band.X + (band.Width - tw) / 2, band.Y + 14, title, 0xEAF6EE, 2);
        int sw = Fonts.Measure(sub, 0);
        dc.Text(band.X + (band.Width - sw) / 2, band.Y + 54, sub, 0xA9C7B5, 0);
    }
}

/// <summary>Opens a game in a window with raw keys and starts its loop.</summary>
public static class GameHost
{
    /// <summary>A window around `view` (plus an optional status line at the top).</summary>
    public static Window Build(string title, GameView view, int width, int height, Control? top = null, bool headless = false)
    {
        var w = new Window(title, width, height, headless) { RawKeys = true };
        var root = new DockPanel { Margin = new Thickness(10) };
        if (top != null) root.Children.Add(top.DockTo(Dock.Top));
        root.Children.Add(view);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        view.Focus();
        return w;
    }
}
