// Buitenzorg.Game, 2D engine parts: a pixel canvas drawn by host natives
// (sprites, tiles: an interpreter loop per pixel is too slow), sprites from
// pixel-art strings or pictures, sprite sheets and animations, tile maps
// with box collision, a following camera, scenes, a small bitmap font for
// in-canvas text, save data and background music.

using Buitenzorg.App;
using Buitenzorg.Media;
using Buitenzorg.UI;

namespace Buitenzorg.Game;

static class PixNative
{
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int PixFill(int[] dst, int w, int h, int x, int y, int rw, int rh, int color);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int PixBlit(int[] dst, int dw, int dh, int[] src, int sw, int sh, int sx, int sy, int srw, int srh, int dx, int dy, int drw, int drh, int flags);
    [System.Runtime.InteropServices.DllImport("buitenzorg")] public static extern int PixTiles(int[] dst, int dw, int dh, int[] tileset, int tsw, int tsh, int tile, byte[] map, int mw, int mh, int camx, int camy);
    static bool? _native;
    /// <summary>The host natives exist (on Buitenzorg); off the device managed loops do the work.</summary>
    public static bool Native
    {
        get
        {
            if (_native == null)
            {
                try { PixFill(new int[1], 1, 1, 0, 0, 0, 0, 0); _native = true; }
                catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _native = false; }
            }
            return _native.Value;
        }
    }
}

/// <summary>An image: 0xAARRGGBB pixels; alpha 0 = transparent.</summary>
public sealed class Sprite
{
    public int Width { get; }
    public int Height { get; }
    public int[] Pixels { get; }
    public Sprite(int width, int height, int[] pixels) { Width = width; Height = height; Pixels = pixels; }

    /// <summary>Pixel art: one string per row, one character per pixel, colors
    /// from the palette (characters not in it -- '.' -- are transparent).</summary>
    public static Sprite FromStrings(string[] rows, string chars, int[] colors)
    {
        int h = rows.Length, w = 0;
        foreach (var r in rows) w = Math.Max(w, r.Length);
        var px = new int[w * h];
        for (int y = 0; y < h; y++)
            for (int x = 0; x < rows[y].Length; x++)
            {
                int k = chars.IndexOf(rows[y][x]);
                if (k >= 0) px[y * w + x] = unchecked((int)0xFF000000) | (colors[k] & 0xFFFFFF);
            }
        return new Sprite(w, h, px);
    }

    /// <summary>A picture (PNG / GIF / JPEG / BMP decoded by the host).</summary>
    public static Sprite? FromPicture(Picture? p, int frame = 0) => p == null ? null : new Sprite(p.Width, p.Height, p.Pixels(frame));

    /// <summary>Several sprites of the same size side by side (a sheet / tileset).</summary>
    public static Sprite Row(params Sprite[] parts)
    {
        int fw = parts[0].Width, fh = parts[0].Height;
        var px = new int[fw * parts.Length * fh];
        for (int i = 0; i < parts.Length; i++)
            for (int y = 0; y < fh; y++)
                Array.Copy(parts[i].Pixels, y * fw, px, y * fw * parts.Length + i * fw, fw);
        return new Sprite(fw * parts.Length, fh, px);
    }
}

/// <summary>Frames of equal size in one image, numbered row by row.</summary>
public sealed class SpriteSheet
{
    public Sprite Image { get; }
    public int FrameWidth { get; }
    public int FrameHeight { get; }
    public int Columns => Image.Width / FrameWidth;
    public int Count => Columns * (Image.Height / FrameHeight);
    public SpriteSheet(Sprite image, int frameWidth, int frameHeight) { Image = image; FrameWidth = frameWidth; FrameHeight = frameHeight; }
    public (int X, int Y) Origin(int frame) => (frame % Columns * FrameWidth, frame / Columns * FrameHeight);
}

/// <summary>A sequence of sheet frames played at a rate.</summary>
public sealed class Animation
{
    readonly int[] _frames;
    readonly double _fps;
    readonly bool _loop;
    double _t;
    public Animation(int[] frames, double fps, bool loop = true) { _frames = frames; _fps = fps; _loop = loop; }
    public void Update(double dt) => _t += dt;
    public void Restart() => _t = 0;
    public bool Done => !_loop && _t * _fps >= _frames.Length;
    public int Frame
    {
        get
        {
            int i = (int)(_t * _fps);
            return _frames[_loop ? i % _frames.Length : Math.Min(i, _frames.Length - 1)];
        }
    }
}

/// <summary>An off-screen picture at the game's own resolution, drawn by host
/// natives, scaled onto the window in one blit per frame.</summary>
public sealed class Canvas2D
{
    public int Width { get; }
    public int Height { get; }
    public int[] Pixels { get; }
    int[]? _screen;
    int _sw, _sh;

    public Canvas2D(int width, int height) { Width = width; Height = height; Pixels = new int[width * height]; }

    public void Clear(int color) => Fill(0, 0, Width, Height, color);

    public void Fill(int x, int y, int w, int h, int color)
    {
        color = unchecked((int)0xFF000000) | (color & 0xFFFFFF);
        if (PixNative.Native) { PixNative.PixFill(Pixels, Width, Height, x, y, w, h, color); return; }
        for (int yy = Math.Max(0, y); yy < Math.Min(Height, y + h); yy++)
            for (int xx = Math.Max(0, x); xx < Math.Min(Width, x + w); xx++) Pixels[yy * Width + xx] = color;
    }

    /// <summary>A sprite (or part of one) at (x, y), scaled to dw x dh; flip = mirrored.</summary>
    public void Blit(Sprite s, int sx, int sy, int sw, int sh, int x, int y, int dw, int dh, bool flip = false)
    {
        int flags = 2 | (flip ? 1 : 0);
        if (PixNative.Native) { PixNative.PixBlit(Pixels, Width, Height, s.Pixels, s.Width, s.Height, sx, sy, sw, sh, x, y, dw, dh, flags); return; }
        ManagedBlit(Pixels, Width, Height, s.Pixels, s.Width, s.Height, sx, sy, sw, sh, x, y, dw, dh, flags);
    }

    public void Draw(Sprite s, int x, int y, bool flip = false) => Blit(s, 0, 0, s.Width, s.Height, x, y, s.Width, s.Height, flip);

    public void Draw(SpriteSheet sheet, int frame, int x, int y, bool flip = false)
    {
        var (ox, oy) = sheet.Origin(frame);
        Blit(sheet.Image, ox, oy, sheet.FrameWidth, sheet.FrameHeight, x, y, sheet.FrameWidth, sheet.FrameHeight, flip);
    }

    /// <summary>The visible part of a tile map, seen from the camera.</summary>
    public void Draw(TileMap map, Camera2D cam)
    {
        if (map.Tileset == null) return;
        var ts = map.Tileset;
        if (PixNative.Native) { PixNative.PixTiles(Pixels, Width, Height, ts.Pixels, ts.Width, ts.Height, map.TileSize, map.Cells, map.Columns, map.Rows, (int)cam.X, (int)cam.Y); return; }
        int t = map.TileSize, per = ts.Width / t;
        int cx = (int)cam.X, cy = (int)cam.Y;
        for (int ty = Math.Max(0, cy / t); ty <= Math.Min(map.Rows - 1, (cy + Height) / t); ty++)
            for (int tx = Math.Max(0, cx / t); tx <= Math.Min(map.Columns - 1, (cx + Width) / t); tx++)
            {
                int k = map[tx, ty];
                if (k == 0) continue;
                ManagedBlit(Pixels, Width, Height, ts.Pixels, ts.Width, ts.Height, (k - 1) % per * t, (k - 1) / per * t, t, t, tx * t - cx, ty * t - cy, t, t, 2);
            }
    }

    static void ManagedBlit(int[] d, int dw, int dh, int[] s, int sw, int sh, int sx, int sy, int srw, int srh, int dx, int dy, int drw, int drh, int flags)
    {
        if (srw <= 0 || srh <= 0 || drw <= 0 || drh <= 0) return;
        for (int y = Math.Max(0, dy); y < Math.Min(dh, dy + drh); y++)
        {
            int v = sy + (y - dy) * srh / drh;
            if (v < 0 || v >= sh) continue;
            for (int x = Math.Max(0, dx); x < Math.Min(dw, dx + drw); x++)
            {
                int u = (x - dx) * srw / drw;
                if ((flags & 1) != 0) u = srw - 1 - u;
                u += sx;
                if (u < 0 || u >= sw) continue;
                int p = s[v * sw + u];
                if ((flags & 2) != 0 && (uint)p >> 24 == 0) continue;
                d[y * dw + x] = p;
            }
        }
    }

    /// <summary>Show the canvas in `field`, scaled up by the largest whole factor
    /// that fits (sharp pixels), centered; one blit.</summary>
    public void Present(DrawingContext dc, Rect field)
    {
        int k = Math.Max(1, Math.Min(field.Width / Width, field.Height / Height));
        int w = Width * k, h = Height * k;
        if (_screen == null || _sw != w || _sh != h) { _screen = new int[w * h]; _sw = w; _sh = h; }
        if (PixNative.Native) PixNative.PixBlit(_screen, w, h, Pixels, Width, Height, 0, 0, Width, Height, 0, 0, w, h, 0);
        else ManagedBlit(_screen, w, h, Pixels, Width, Height, 0, 0, Width, Height, 0, 0, w, h, 0);
        dc.FillRect(field, 0x0B120E);
        dc.Blit(field.X + (field.Width - w) / 2, field.Y + (field.Height - h) / 2, w, h, _screen);
    }

    /// <summary>The pixel at (x, y), 0xRRGGBB (tests).</summary>
    public int At(int x, int y) => Pixels[y * Width + x] & 0xFFFFFF;
}

/// <summary>A grid of tiles: 0 = empty, n = the n-th tile of <see cref="Tileset"/>.</summary>
public sealed class TileMap
{
    public int Columns { get; }
    public int Rows { get; }
    public int TileSize { get; }
    public byte[] Cells { get; }
    public Sprite? Tileset { get; set; }
    /// <summary>Which tiles block movement.</summary>
    public Func<int, bool> IsSolid { get; set; } = t => t != 0;
    public int PixelWidth => Columns * TileSize;
    public int PixelHeight => Rows * TileSize;

    public TileMap(int columns, int rows, int tileSize) { Columns = columns; Rows = rows; TileSize = tileSize; Cells = new byte[columns * rows]; }

    public int this[int x, int y]
    {
        get => x < 0 || y < 0 || x >= Columns || y >= Rows ? 0 : Cells[y * Columns + x];
        set { if (x >= 0 && y >= 0 && x < Columns && y < Rows) Cells[y * Columns + x] = (byte)value; }
    }

    /// <summary>A map from text, one line per row; `tileOf` turns a character into a tile.</summary>
    public static TileMap FromText(string[] lines, int tileSize, Func<char, int> tileOf)
    {
        int w = 0;
        foreach (var l in lines) w = Math.Max(w, l.Length);
        var m = new TileMap(w, lines.Length, tileSize);
        for (int y = 0; y < lines.Length; y++)
            for (int x = 0; x < lines[y].Length; x++) m[x, y] = tileOf(lines[y][x]);
        return m;
    }

    /// <summary>Whether a box (pixels) overlaps a solid tile.</summary>
    public bool Hits(double x, double y, double w, double h)
    {
        int t = TileSize;
        for (int ty = (int)Math.Floor(y / t); ty <= (int)Math.Floor((y + h - 0.001) / t); ty++)
            for (int tx = (int)Math.Floor(x / t); tx <= (int)Math.Floor((x + w - 0.001) / t); tx++)
                if (tx < 0 || tx >= Columns || IsSolid(this[tx, ty])) return true;
        return false;
    }

    /// <summary>Move a body by its velocity, one axis at a time, stopping at solid
    /// tiles; sets <see cref="Body.OnGround"/>, <see cref="Body.HitWall"/>.</summary>
    public void Move(Body b, double dt)
    {
        b.OnGround = false;
        b.HitWall = false;
        double nx = b.X + b.VX * dt;
        if (!Hits(nx, b.Y, b.W, b.H)) b.X = nx;
        else
        {
            int t = TileSize;
            b.X = b.VX > 0 ? Math.Floor((nx + b.W) / t) * t - b.W - 0.001 : Math.Floor(nx / t) * t + t;
            if (Hits(b.X, b.Y, b.W, b.H)) b.X = nx - b.VX * dt; // safety: stay where it was
            b.VX = 0;
            b.HitWall = true;
        }
        double ny = b.Y + b.VY * dt;
        if (!Hits(b.X, ny, b.W, b.H)) b.Y = ny;
        else
        {
            int t = TileSize;
            if (b.VY > 0) { b.Y = Math.Floor((ny + b.H) / t) * t - b.H - 0.001; b.OnGround = true; }
            else b.Y = Math.Floor(ny / t) * t + t;
            b.VY = 0;
        }
    }
}

/// <summary>A moving box (the player, enemies) for <see cref="TileMap.Move"/>.</summary>
public sealed class Body
{
    public double X, Y, W, H, VX, VY;
    public bool OnGround, HitWall;
    public Body(double x, double y, double w, double h) { X = x; Y = y; W = w; H = h; }
    public bool Overlaps(double x, double y, double w, double h) => Collide.Rects(X, Y, W, H, x, y, w, h);
}

/// <summary>The view onto a larger world: follows a point, stays inside the world.</summary>
public sealed class Camera2D
{
    public double X, Y;
    public int ViewWidth { get; }
    public int ViewHeight { get; }
    public Camera2D(int viewWidth, int viewHeight) { ViewWidth = viewWidth; ViewHeight = viewHeight; }
    /// <summary>Center on (x, y), smoothly (`ease` 1 = at once), kept inside world size.</summary>
    public void Follow(double x, double y, int worldWidth, int worldHeight, double ease = 1)
    {
        double tx = x - ViewWidth / 2.0, ty = y - ViewHeight / 2.0;
        X += (tx - X) * ease;
        Y += (ty - Y) * ease;
        X = Math.Max(0, Math.Min(X, worldWidth - ViewWidth));
        Y = Math.Max(0, Math.Min(Y, worldHeight - ViewHeight));
    }
}

/// <summary>One screen of a game (title, level, game over): a <see cref="SceneGame"/>
/// runs the current one.</summary>
public abstract class Scene
{
    public SceneGame Game { get; internal set; } = null!;
    public virtual void Enter() { }
    public abstract void Update(double dt);
    public abstract void Draw(Canvas2D c);
}

/// <summary>A game made of scenes, drawn into a <see cref="Canvas2D"/>.</summary>
public class SceneGame : GameView
{
    public Canvas2D Canvas { get; }
    public Scene? Current { get; private set; }
    public SceneGame(int width, int height, int rate = 60) : base(width, height, rate) { Canvas = new Canvas2D(width, height); }
    public void Go(Scene s) { s.Game = this; Current = s; s.Enter(); }
    protected override void Update() => Current?.Update(Dt);
    protected override void Draw(DrawingContext dc, Rect field)
    {
        Current?.Draw(Canvas);
        Canvas.Present(dc, field);
    }
    /// <summary>Draw the current scene into the canvas without a window (tests).</summary>
    public void RenderCanvas() => Current?.Draw(Canvas);
}

/// <summary>A 3x5 pixel font (digits, A-Z, a little punctuation) for text drawn
/// into a <see cref="Canvas2D"/> (scores, labels in pixel-art style).</summary>
public static class PixelFont
{
    const string Glyphs = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ:.-!/ ";
    // 5 rows of 3 bits per glyph
    static readonly int[] Bits = {
        0b111101101101111, 0b010110010010111, 0b111001111100111, 0b111001111001111, 0b101101111001001,
        0b111100111001111, 0b111100111101111, 0b111001001010010, 0b111101111101111, 0b111101111001111,
        0b010101111101101, 0b110101110101110, 0b011100100100011, 0b110101101101110, 0b111100110100111,
        0b111100110100100, 0b011100101101011, 0b101101111101101, 0b111010010010111, 0b001001001101010,
        0b101101110101101, 0b100100100100111, 0b101111111101101, 0b110101101101101, 0b010101101101010,
        0b110101110100100, 0b010101101110011, 0b110101110101101, 0b011100010001110, 0b111010010010010,
        0b101101101101111, 0b101101101010010, 0b101101111111101, 0b101101010101101, 0b101101010010010,
        0b111001010100111, 0b000010000010000, 0b000000000000010, 0b000000111000000, 0b010010010000010,
        0b001001010100100, 0b000000000000000,
    };

    /// <summary>Draw text with its top-left at (x, y), `scale` pixels per dot; the width.</summary>
    public static int Draw(Canvas2D c, string text, int x, int y, int color, int scale = 1)
    {
        int cx = x;
        foreach (char ch in text.ToUpperInvariant())
        {
            int g = Glyphs.IndexOf(ch);
            if (g < 0) g = Glyphs.Length - 1;
            int bits = Bits[g];
            for (int r = 0; r < 5; r++)
                for (int col = 0; col < 3; col++)
                    if ((bits >> (14 - (r * 3 + col)) & 1) != 0) c.Fill(cx + col * scale, y + r * scale, scale, scale, color);
            cx += 4 * scale;
        }
        return cx - x;
    }
}

/// <summary>A game's saved progress: key = value lines in a small file
/// (/disk/NAME.SAV, or the scratch folder off the device).</summary>
public sealed class SaveData
{
    readonly string _path;
    readonly Dictionary<string, string> _v = new Dictionary<string, string>();
    public SaveData(string name)
    {
        _path = Files.ScratchDirectory + "/" + name.ToUpperInvariant() + ".SAV";
        foreach (var l in Files.ReadAllText(_path).Split('\n'))
        {
            int eq = l.IndexOf('=');
            if (eq > 0) _v[l.Substring(0, eq).Trim()] = l.Substring(eq + 1).Trim();
        }
    }
    public string Get(string key, string def = "") => _v.TryGetValue(key, out var s) ? s : def;
    public int GetInt(string key, int def = 0) => int.TryParse(Get(key), out int v) ? v : def;
    public void Set(string key, string value) => _v[key] = value;
    public void Set(string key, int value) => _v[key] = value.ToString();
    public bool Save()
    {
        var sb = new System.Text.StringBuilder();
        foreach (var kv in _v) sb.Append(kv.Key).Append('=').Append(kv.Value).Append('\n');
        return Files.WriteAllText(_path, sb.ToString());
    }
    public string Path => _path;
}

/// <summary>Background music for a game: a song file (WAV, FLAC, MP3, Ogg, MIDI)
/// played on the mixer and started again when it ends. Silent off the live desktop.</summary>
public sealed class GameMusic
{
    readonly MediaPlayer _player = new MediaPlayer();
    DispatcherTimer? _timer;
    public bool Playing { get; private set; }
    public string Title => _player.Title;

    public bool Play(string path)
    {
        if (!GameEnv.Interactive) return false;
        if (!_player.Load(path)) return false;
        _player.Ended += () => _player.Play();
        _player.Play();
        _timer = new DispatcherTimer(100);
        _timer.Tick += () => _player.Pump();
        _timer.Start();
        Playing = true;
        return true;
    }

    public void Stop()
    {
        _timer?.Stop();
        _player.Stop();
        Playing = false;
    }
}
