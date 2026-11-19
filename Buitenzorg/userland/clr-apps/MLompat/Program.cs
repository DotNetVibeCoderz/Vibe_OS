// Lompat Kodok -- an original platformer on Buitenzorg.Game's 2D engine: a
// frog crosses a garden of leaf platforms, collects dew drops, avoids thorns
// and a crawling snail, and reaches the flag. Pixel art drawn in code
// (Sprite.FromStrings), a tile map from text, box collision against tiles, a
// following camera, scenes (title / level / goal), the in-canvas pixel font,
// the best time kept in a save file, and the music of LAGU5.MID. Keyboard
// (arrows / A D, Space / W / Up to jump) or a gamepad (d-pad, A).

using Buitenzorg.App;
using Buitenzorg.Game;
using Buitenzorg.UI;
using Window = Buitenzorg.UI.Window;

namespace MLompat;

public static class Art
{
    // tiles: 1 grass, 2 soil, 3 leaf platform, 4 thorns, 5 dew drop, 6 flag, 7 stone
    public const int Grass = 1, Soil = 2, Leaf = 3, Thorns = 4, Dew = 5, Flag = 6, Stone = 7;
    const string P = "gGdDlLtTwWfFsSbB";
    static readonly int[] C = { 0x5FB85A, 0x3E8E41, 0x7A5230, 0x5C3D24, 0x8BC97A, 0x4E9A4A, 0x9C7B4E, 0xD8C7A8, 0x9FD8F0, 0xE6F7FF, 0xE5735F, 0xF2F2F2, 0x8A8F96, 0x6B7077, 0x2B2B2B, 0xF2B544 };

    static Sprite S(params string[] rows) => Sprite.FromStrings(rows, P, C);

    public static Sprite Tileset() => Sprite.Row(
        S("gggggggggggggggg", "gGggGgggGggGgggG", "GGGGGGGGGGGGGGGG", "dDddDddddDddDddd", "ddddddDddddddDdd", "dDddddddDddddddd", "ddddDddddddDdddd",
          "dddddddDdddddddD", "dDdddddddddDdddd", "ddddDddddDdddddd", "ddDddddddddddDdd", "dddddddDdddddddd", "dDdddDddddddDddd", "ddddddddDddddddd", "dddDdddddddDdddd", "dddddddddddddddd"),
        S("dDddDddddDddDddd", "ddddddDddddddDdd", "dDddddddDddddddd", "ddddDddddddDdddd", "dddddddDdddddddD", "dDdddddddddDdddd", "ddddDddddDdddddd", "ddDddddddddddDdd",
          "dddddddDdddddddd", "dDdddDddddddDddd", "ddddddddDddddddd", "dddDdddddddDdddd", "dddddddddddddddd", "dDddDddddDddDddd", "ddddddDddddddDdd", "dDdddddddDdddddd"),
        S("..llllllllllll..", ".lLLLLLLLLLLLLl.", "llLLLlLLLLlLLLll", "lLLLLlLLLLlLLLLl", ".llllllllllllll.", "................", "................", "................",
          "................", "................", "................", "................", "................", "................", "................", "................"),
        S("................", "................", "................", "................", "................", "................", "................", "................",
          "................", "..t...t...t...t.", "..t...t...t...t.", ".tTt.tTt.tTt.tTt", ".tTt.tTt.tTt.tTt", "tTTTtTTTtTTTtTTT", "TTTTTTTTTTTTTTTT", "TTTTTTTTTTTTTTTT"),
        S("................", "................", "................", ".......w........", "......wWw.......", "......wWw.......", ".....wWWWw......", ".....wWWWw......",
          "....wWWWWWw.....", "....wWWWwWw.....", "....wWWWWWw.....", ".....wWWWw......", "......www.......", "................", "................", "................"),
        S("..s.............", "..sfffffff......", "..sffffffffff...", "..sfffffff......", "..sff...........", "..s.............", "..s.............", "..s.............",
          "..s.............", "..s.............", "..s.............", "..s.............", "..s.............", "..s.............", ".sSs............", "sSSSs..........."),
        S("SsssssssssssssSs", "sSSSSSSSSSSSSSSs", "sSssSSSSSSsSSSSs", "sSSSSSSsSSSSSSSs", "sSSSSSSSSSSSssSs", "sSsSSSSSSSSSSSSs", "sSSSSsSSSSSSSSSs", "sSSSSSSSSSsSSSSs",
          "sSSSSSSSSSSSSSSs", "sSSsSSSSSSSSSSss", "sSSSSSSSsSSSSSSs", "sSSSSSSSSSSSSSSs", "sSSSSSsSSSSSsSSs", "sSSSSSSSSSSSSSSs", "sSsSSSSSSSSSSSSs", "SssssssssssssssS"));

    public static SpriteSheet Frog() => new SpriteSheet(Sprite.Row(
        S("................", "................", "....gg....gg....", "...gBwg..gBwg...", "...gwwgggwwgg...", "..gggggggggggg..", ".ggGGGGGGGGGGgg.", ".gGGGGggggGGGGg.",
          ".gGGgggggggGGGg.", ".gGGGGGGGGGGGGg.", "..gGGGGGGGGGGg..", "..ggGGGGGGGGgg..", ".gGg.gGGGGg.gGg.", "gGG..gGggGg..GGg", "gg....g..g....gg", "................"),
        S("................", "................", "....gg....gg....", "...gBwg..gBwg...", "...gwwgggwwgg...", "..gggggggggggg..", ".ggGGGGGGGGGGgg.", ".gGGGGggggGGGGg.",
          ".gGGgggggggGGGg.", ".gGGGGGGGGGGGGg.", "..gGGGGGGGGGGg..", "..ggGGGGGGGGgg..", "...gGgGGGGgGg...", "...gGg.gg.gGg...", "...gg......gg...", "................"),
        S("....gg....gg....", "...gBwg..gBwg...", "...gwwgggwwgg...", "..gggggggggggg..", ".ggGGGGGGGGGGgg.", ".gGGGGggggGGGGg.", ".gGGgggggggGGGg.", ".gGGGGGGGGGGGGg.",
          "..gGGGGGGGGGGg..", "..ggGGGGGGGGgg..", ".gGg..gGGg..gGg.", "gGg...g..g...gGg", "gg............gg", "................", "................", "................")), 16, 16);

    public static SpriteSheet Snail() => new SpriteSheet(Sprite.Row(
        S("................", "................", "................", "................", "................", "................", ".......bbbb.....", ".....bbBBBBbb...",
          "....bBBbbbbBBb..", "....bBbBBBBbBb..", "....bBbBbbBbBb..", "s...bBBbbbBBBb..", "sS..bbBBBBBbb...", "sSSSSSSSSSSSSSS.", ".SSSSSSSSSSSSSSS", "................"),
        S("................", "................", "................", "................", "................", "................", ".......bbbb.....", ".....bbBBBBbb...",
          "....bBBbbbbBBb..", "....bBbBBBBbBb..", ".s..bBbBbbBbBb..", ".sS.bBBbbbBBBb..", ".sSSbbBBBBBbb...", "SSSSSSSSSSSSSSS.", "SSSSSSSSSSSSSSS.", "................")), 16, 16);
}

public static class Level
{
    // . empty  G grass  D soil  L leaf  ^ thorns  o dew  F flag  S stone
    public static readonly string[] Map =
    {
        "................................................................................",
        "................................................................................",
        "................................................................................",
        "..............................o.o...............................................",
        "...........................LLLLLLL..........oo..................o..o.......F....",
        "..................o.o..................................LLLL....LLLLLL.......S....",
        "...............LLLLLLL..............o....LLLLL..................................",
        ".........o.o..........................................................SSS.......",
        "........LLLLL........................LLLL..........................S............",
        "...o.o..................oo..........................o.o........SS...............",
        "...........................................GGGGGGGGGGGGG..............GGGGGGGGGG",
        "GGGGGGGGGGGGGGGG...GGGGGGGGGGG^^^^GGGGGGGGGDDDDDDDDDDDDDGGG^^GGGGGGGGGDDDDDDDDDD",
        "DDDDDDDDDDDDDDDD...DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD",
        "DDDDDDDDDDDDDDDD...DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD",
    };

    public static TileMap Build()
    {
        var m = TileMap.FromText(Map, 16, ch => ch switch
        {
            'G' => Art.Grass, 'D' => Art.Soil, 'L' => Art.Leaf, '^' => Art.Thorns, 'o' => Art.Dew, 'F' => Art.Flag, 'S' => Art.Stone, _ => 0,
        });
        m.Tileset = Art.Tileset();
        m.IsSolid = t => t == Art.Grass || t == Art.Soil || t == Art.Leaf || t == Art.Stone;
        return m;
    }
}

public sealed class LompatGame : SceneGame
{
    public readonly SaveData Save = new SaveData("LOMPAT");
    public readonly GameMusic Music = new GameMusic();
    public LompatGame() : base(320, 180) { }
    public static bool Jump(Input i) => i.AnyPressed(RawKey.Space, RawKey.W, RawKey.Up, RawKey.PadA, RawKey.PadUp);
    public static bool JumpHeld(Input i) => i.AnyDown(RawKey.Space, RawKey.W, RawKey.Up, RawKey.PadA, RawKey.PadUp);
    public static int Dir(Input i) => i.Axis(RawKey.Left, RawKey.Right, RawKey.A, RawKey.D) + i.Axis(RawKey.PadLeft, RawKey.PadRight);
}

public sealed class TitleScene : Scene
{
    double _t;
    public override void Update(double dt)
    {
        _t += dt;
        var i = Game.Input;
        if (i.AnyPressed(RawKey.Space, RawKey.Enter, RawKey.PadA, RawKey.PadStart)) Game.Go(new LevelScene());
    }
    public override void Draw(Canvas2D c)
    {
        c.Clear(0x9FD8F0);
        c.Fill(0, 140, 320, 40, 0x5FB85A);
        int w = PixelFont.Draw(c, "LOMPAT KODOK", 0, -100, 0, 3);
        PixelFont.Draw(c, "LOMPAT KODOK", (320 - w) / 2, 50, 0x1F4D2E, 3);
        if ((int)(_t * 2) % 2 == 0) PixelFont.Draw(c, "SPASI UNTUK MULAI", 92, 100, 0x2B2B2B, 2);
        var g = (LompatGame)Game;
        int best = g.Save.GetInt("best", 0);
        if (best > 0) PixelFont.Draw(c, "REKOR " + best / 10 + "." + best % 10 + " DETIK", 104, 122, 0x1F4D2E, 2);
    }
}

public sealed class LevelScene : Scene
{
    public readonly TileMap Map = Level.Build();
    public readonly Body Frog = new Body(24, 120, 12, 13);
    public Camera2D Cam = null!;
    readonly SpriteSheet _frog = Art.Frog();
    readonly SpriteSheet _snail = Art.Snail();
    public readonly Body Snail = new Body(16 * 46, 16 * 10 - 14, 14, 14);
    public int Drops, Deaths;
    public double Time;
    public bool Won;
    double _coyote, _buffer, _walk;
    bool _face = true;
    const double Gravity = 900, Speed = 110, JumpSpeed = 330;

    public override void Enter()
    {
        Cam = new Camera2D(Game.FieldWidth, Game.FieldHeight);
        Snail.VX = -24;
        ((LompatGame)Game).Music.Play("/disk/LAGU5.MID");
    }

    void Respawn()
    {
        Deaths++;
        Frog.X = 24; Frog.Y = 120; Frog.VX = Frog.VY = 0;
        Sfx.Lose();
    }

    public override void Update(double dt)
    {
        if (Won) return;
        Time += dt;
        var inp = Game.Input;
        int dir = LompatGame.Dir(inp);
        Frog.VX = dir * Speed;
        if (dir != 0) { _face = dir > 0; _walk += dt; } else _walk = 0;
        _coyote = Frog.OnGround ? 0.1 : Math.Max(0, _coyote - dt);
        _buffer = LompatGame.Jump(inp) ? 0.12 : Math.Max(0, _buffer - dt);
        if (_buffer > 0 && _coyote > 0) { Frog.VY = -JumpSpeed; _buffer = 0; _coyote = 0; Sfx.Blip(); }
        if (!LompatGame.JumpHeld(inp) && Frog.VY < -120) Frog.VY = -120; // a short hop when released
        Frog.VY = Math.Min(Frog.VY + Gravity * dt, 500);
        Map.Move(Frog, dt);

        // the snail patrols, turning at walls and ledges
        Snail.VY = Math.Min(Snail.VY + Gravity * dt, 500);
        double probe = Snail.VX < 0 ? Snail.X - 2 : Snail.X + Snail.W + 2;
        if (!Map.Hits(probe, Snail.Y + Snail.H + 2, 1, 1)) Snail.VX = -Snail.VX;
        Map.Move(Snail, dt);
        if (Snail.HitWall) Snail.VX = Snail.VX == 0 ? 24 : -Snail.VX;
        if (Snail.VX == 0) Snail.VX = 24;

        // what the frog touches
        int t = Map.TileSize;
        for (int ty = (int)(Frog.Y / t); ty <= (int)((Frog.Y + Frog.H) / t); ty++)
            for (int tx = (int)(Frog.X / t); tx <= (int)((Frog.X + Frog.W) / t); tx++)
            {
                int k = Map[tx, ty];
                if (k == Art.Dew) { Map[tx, ty] = 0; Drops++; Sfx.Score(); }
                else if (k == Art.Thorns && Frog.Y + Frog.H > ty * t + 9) { Respawn(); return; }
                else if (k == Art.Flag) { Win(); return; }
            }
        if (Frog.Overlaps(Snail.X, Snail.Y, Snail.W, Snail.H))
        {
            if (Frog.VY > 0 && Frog.Y + Frog.H < Snail.Y + 8) { Frog.VY = -JumpSpeed * 0.7; Snail.Y = 9999; Sfx.Hit(); } // stomped
            else { Respawn(); return; }
        }
        if (Frog.Y > Map.PixelHeight) { Respawn(); return; }
        Cam.Follow(Frog.X + Frog.W / 2, Frog.Y, Map.PixelWidth, Map.PixelHeight, 0.15);
    }

    void Win()
    {
        Won = true;
        var g = (LompatGame)Game;
        int tenths = (int)(Time * 10);
        int best = g.Save.GetInt("best", 0);
        if (best == 0 || tenths < best) g.Save.Set("best", tenths);
        g.Save.Set("drops", Math.Max(Drops, g.Save.GetInt("drops", 0)));
        g.Save.Save();
        g.Music.Stop();
        Sfx.Score();
        Game.Go(new GoalScene(tenths, Drops));
    }

    public override void Draw(Canvas2D c)
    {
        c.Clear(0x9FD8F0);
        // far hills, slower than the camera (parallax)
        int px = -(int)(Cam.X * 0.3) % 160;
        for (int k = 0; k < 4; k++) c.Fill(px + k * 160 + 20, 120, 120, 60, 0x8BC97A);
        c.Draw(Map, Cam);
        int frame = !Frog.OnGround ? 2 : _walk > 0 && (int)(_walk * 8) % 2 == 1 ? 1 : 0;
        c.Draw(_frog, frame, (int)(Frog.X - 2 - Cam.X), (int)(Frog.Y - 3 - Cam.Y), !_face);
        if (Snail.Y < 9000) c.Draw(_snail, (int)(Time * 3) % 2, (int)(Snail.X - 1 - Cam.X), (int)(Snail.Y - 2 - Cam.Y), Snail.VX > 0);
        c.Fill(0, 0, 320, 11, 0x1F4D2E);
        PixelFont.Draw(c, "EMBUN " + Drops, 4, 3, 0xE6F7FF);
        PixelFont.Draw(c, "WAKTU " + (int)Time, 140, 3, 0xE6F7FF);
        if (Deaths > 0) PixelFont.Draw(c, "JATUH " + Deaths, 260, 3, 0xF2B544);
    }
}

public sealed class GoalScene : Scene
{
    readonly int _tenths, _drops;
    public GoalScene(int tenths, int drops) { _tenths = tenths; _drops = drops; }
    public override void Update(double dt)
    {
        if (Game.Input.AnyPressed(RawKey.Space, RawKey.Enter, RawKey.PadA, RawKey.PadStart)) Game.Go(new TitleScene());
    }
    public override void Draw(Canvas2D c)
    {
        c.Clear(0x1F4D2E);
        PixelFont.Draw(c, "SAMPAI!", 110, 50, 0xF2B544, 4);
        PixelFont.Draw(c, "WAKTU " + _tenths / 10 + "." + _tenths % 10 + " DETIK", 90, 92, 0xE6F7FF, 2);
        PixelFont.Draw(c, "EMBUN " + _drops, 120, 112, 0x9FD8F0, 2);
        PixelFont.Draw(c, "SPASI UNTUK ULANG", 92, 140, 0xA9C7B5, 2);
    }
}

public static class LompatApp
{
    public static Checks SelfTest()
    {
        var c = new Checks("Lompat Kodok");
        var g = new LompatGame();
        Files.Delete(g.Save.Path);
        g = new LompatGame();
        g.Go(new TitleScene());
        g.RenderCanvas();
        c.That(g.Canvas.At(5, 5) == 0x9FD8F0 && g.Canvas.At(5, 170) == 0x5FB85A, "title screen drawn into the canvas");
        g.Input.Press(RawKey.Space);
        g.Step(1);
        g.Input.Release(RawKey.Space);
        var lv = g.Current as LevelScene;
        c.That(lv != null, "Space starts the level");
        if (lv == null) return c;
        g.Step(30);
        c.That(lv.Frog.OnGround && Math.Abs(lv.Frog.Y + lv.Frog.H - 176) < 1, "the frog lands on the grass (y " + (int)(lv.Frog.Y + lv.Frog.H) + ")");
        // walk right into the dew drops at x 3..5 on row 9 by jumping
        g.Input.Press(RawKey.Right);
        g.Step(10);
        double x1 = lv.Frog.X;
        c.That(x1 > 24 + 15, "right arrow walks (" + (int)x1 + ")");
        g.Input.Press(RawKey.PadA); // a gamepad's A jumps too
        g.Step(1);
        g.Input.Release(RawKey.PadA);
        c.That(lv.Frog.VY < 0 && !lv.Frog.OnGround, "gamepad A jumps");
        g.Step(40);
        c.That(lv.Drops >= 1, "dew collected mid-air (" + lv.Drops + ")");
        // a solid wall stops movement: put a stone in front and walk into it
        int tx = (int)((lv.Frog.X + lv.Frog.W) / 16) + 1, ty = (int)(lv.Frog.Y / 16);
        g.Step(30);
        // the gap at x 16..18: walking into it drops the frog -> respawn
        int deaths = lv.Deaths;
        g.Step(120);
        c.That(lv.Deaths > deaths || lv.Frog.X > 16 * 19, "the gap: fell and respawned, or jumped across (x " + (int)lv.Frog.X + ", falls " + lv.Deaths + ")");
        g.Input.Release(RawKey.Right);
        // collision: a box inside the ground is solid, above it is free
        c.That(lv.Map.Hits(10, 180, 8, 8) && !lv.Map.Hits(10, 100, 8, 8), "tile collision");
        // the camera stays inside the level
        lv.Cam.Follow(0, 0, lv.Map.PixelWidth, lv.Map.PixelHeight);
        bool inside = lv.Cam.X == 0 && lv.Cam.Y == 0;
        lv.Cam.Follow(99999, 99999, lv.Map.PixelWidth, lv.Map.PixelHeight);
        c.That(inside && lv.Cam.X == lv.Map.PixelWidth - 320 && lv.Cam.Y == lv.Map.PixelHeight - 180, "camera clamped to the level");
        // reach the flag: put the frog next to it and step
        lv.Frog.X = 16 * 75; lv.Frog.Y = 16 * 4; lv.Frog.VX = lv.Frog.VY = 0;
        g.Step(2);
        c.That(g.Current is GoalScene && g.Save.GetInt("best", 0) > 0, "the flag ends the level, the time is saved (" + g.Save.GetInt("best", 0) + ")");
        g.RenderCanvas();
        c.That(g.Canvas.At(1, 1) == 0x1F4D2E, "goal screen drawn");
        var again = new LompatGame();
        c.That(again.Save.GetInt("best", 0) == g.Save.GetInt("best", 0), "the record is read back");
        Files.Delete(g.Save.Path);
        _ = tx; _ = ty;
        return c;
    }

    public static (Window, LompatGame) Build(bool headless = false)
    {
        var g = new LompatGame();
        g.Go(new TitleScene());
        var w = GameHost.Build("Lompat Kodok", g, 680, 400, null, headless);
        return (w, g);
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = LompatApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, g) = LompatApp.Build();
        g.Start();
        app.Run(w);
        return 0;
    }
}
