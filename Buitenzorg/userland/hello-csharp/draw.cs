// Buitenzorg OS — v0.16 "Panen": Buitenzorg.Drawing (software renderer) demo.
//
// Renders a rich scene entirely in C# into a managed Bitmap (gradient, shapes,
// polygon, alpha blending), verifies a few pixels, then blits the finished
// frame to a window with one BLIT syscall and draws a text label. This proves
// the client-side software-rendering path (the compositor model for the coming
// Buitenzorg.UI toolkit). Built with bflat --stdlib:zero together with bzgfx.cs.

using System;
using Buitenzorg.Drawing;

class DrawDemo
{
    static void Main()
    {
        Console.WriteLine("Draw: menguji Buitenzorg.Drawing (renderer software)...");

        const int W = 360, H = 240;
        Bitmap bmp = new Bitmap(W, H);
        Graphics g = new Graphics(bmp);

        // Gradient background.
        g.FillGradientV(0, 0, W, H, Color.FromRgb(30, 40, 60), Color.FromRgb(10, 15, 25));

        // Filled + outlined rectangle.
        g.FillRectangle(Color.Red, 20, 20, 80, 50);
        g.DrawRectangle(Color.White, 20, 20, 80, 50, 2);

        // Line.
        g.DrawLine(Color.Yellow, 120, 25, 340, 70, 2);

        // Circle (filled + outline).
        g.FillCircle(Color.Green, 60, 155, 35);
        g.DrawCircle(Color.White, 60, 155, 35);

        // Ellipse.
        g.FillEllipse(Color.Blue, 150, 110, 120, 70);

        // Triangle (polygon fill).
        int[] px = new int[3]; int[] py = new int[3];
        px[0] = 300; py[0] = 110; px[1] = 350; py[1] = 205; px[2] = 250; py[2] = 205;
        g.FillPolygon(Color.Orange, px, py, 3);

        // Alpha blend: 50% white over the red rectangle.
        g.FillRectangle(Color.FromArgb(128, 255, 255, 255), 40, 30, 40, 30);

        // Verify a few pixels (functional check of the renderer).
        bool ok = true;
        Color red = bmp.GetPixel(95, 25);       // solid red (outside the blend area)
        if (!(red.R > 180 && red.G < 120 && red.B < 120)) ok = false;
        Color grn = bmp.GetPixel(60, 155);      // green circle center
        if (!(grn.G > 120 && grn.R < 120)) ok = false;
        Color bld = bmp.GetPixel(50, 40);       // red blended with 50% white -> light red
        if (!(bld.R > 200 && bld.G > 120 && bld.G < 210)) ok = false;

        // Transform (translate + rotate) + GraphicsPath: a rotated square path.
        g.ResetTransform();
        g.TranslateTransform(305, 55);
        g.RotateTransform(30);
        GraphicsPath path = new GraphicsPath();
        path.AddRectangle(-16, -16, 32, 32);
        g.FillPath(Color.Cyan, path);
        g.DrawPath(Color.White, path, true);
        g.ResetTransform();
        Color pc = bmp.GetPixel(305, 55);   // center of the rotated filled square
        bool pathOk = pc.G > 150 && pc.B > 150 && pc.R < 150;

        // BMP encode/decode round-trip on a small bitmap.
        Bitmap sm = new Bitmap(16, 16);
        Graphics gs = new Graphics(sm);
        gs.FillRectangle(Color.Magenta, 0, 0, 16, 16);
        gs.FillRectangle(Color.Yellow, 4, 4, 8, 8);
        byte[] bytes = Bmp.Save(sm);
        Bitmap loaded = Bmp.Load(bytes);
        bool bmpOk = loaded.Width == 16 && loaded.Height == 16
                     && loaded.GetPixel(0, 0).Rgb24 == Color.Magenta.Rgb24
                     && loaded.GetPixel(6, 6).Rgb24 == Color.Yellow.Rgb24;

        // Font: DrawString into the Bitmap + MeasureString.
        Font font = Font.Default();
        int mw = font.Measure("HELLO");                       // 5 * 8 = 40
        g.DrawString(font, "BUITENZORG.DRAWING", Color.White, 18, 88);
        Color tp = bmp.GetPixel(18, 88);                      // top-left of 'B' (row "####.")
        bool textOk = mw == 40 && tp.R > 200 && tp.G > 200 && tp.B > 200;

        // TrueType text (FONT_OP RENDER): anti-aliased coverage blended in.
        TrueTypeFont tt = TrueTypeFont.Ui(18);
        Bitmap tb = new Bitmap(120, 30);
        Graphics tg = new Graphics(tb);
        tg.Clear(Color.Black);
        tg.DrawString(tt, "Halo Kebun", Color.White, 2, 2);
        int inked = 0, partial = 0;
        for (int i = 0; i < tb.Pixels.Length; i++)
        {
            uint r = (tb.Pixels[i] >> 16) & 255;
            if (r > 0) inked++;
            if (r > 0 && r < 250) partial++;
        }
        bool ttfOk = !tt.Available || (tt.Measure("Halo Kebun") > 40 && inked > 60 && partial > 10);
        Console.WriteLine(tt.Available ? (ttfOk ? "Draw: TrueType text OK (anti-aliased)" : "Draw: TrueType text gagal") : "Draw: no outline font, 8x8 fallback");
        textOk &= ttfOk;

        // Clip: a big fill limited to a small clip window.
        g.SetClip(150, 100, 18, 14);
        g.FillRectangle(Color.Magenta, 100, 90, 220, 60);
        g.ResetClip();
        bool clipOk = bmp.GetPixel(158, 106).Rgb24 == Color.Magenta.Rgb24   // inside clip
                      && bmp.GetPixel(140, 106).Rgb24 != Color.Magenta.Rgb24; // outside clip

        // Hatch brush.
        g.FillHatch(Color.Yellow, 20, 104, 44, 14, Graphics.HatchHorizontal, 4);
        bool hatchOk = bmp.GetPixel(25, 104).Rgb24 == Color.Yellow.Rgb24     // row 104 % 4 == 0
                       && bmp.GetPixel(25, 105).Rgb24 != Color.Yellow.Rgb24;  // row 105 skipped

        // Scaled image blit (nearest-neighbor).
        g.DrawImageScaled(sm, 250, 18, 40, 40);
        bool scaleOk = bmp.GetPixel(252, 20).Rgb24 == Color.Magenta.Rgb24;

        // --- v0.16 visual-enhancement primitives ---
        // A soft-shadowed rounded "button" with a vertical gradient fill.
        g.DrawShadow(112, 188, 92, 26, 9, 3, 130);
        g.FillRoundedGradientV(110, 186, 92, 26, 9, Color.FromRgb(96, 150, 220), Color.FromRgb(48, 88, 150));
        g.DrawRoundedRectangle(Color.FromRgb(200, 216, 235), 110, 186, 92, 26, 9);
        Color rr = bmp.GetPixel(156, 199);                    // button center (bluish)
        bool roundOk = rr.B > rr.R && rr.B > 120;

        // Horizontal gradient strip (red -> blue).
        g.FillGradientH(20, 190, 80, 12, Color.FromRgb(220, 80, 60), Color.FromRgb(60, 100, 220));
        Color gl = bmp.GetPixel(22, 196), gr = bmp.GetPixel(97, 196);
        bool gradHOk = gl.R > gl.B && gr.B > gr.R;

        // Anti-aliased circle (smooth thumb).
        g.FillCircleAA(Color.White, 224, 199, 8);
        Color aac = bmp.GetPixel(224, 199);                   // center is solid white
        bool aaOk = aac.R > 230 && aac.G > 230 && aac.B > 230;

        // --- Pen (dash / cap / join), TextureBrush, Region on a scratch bitmap ---
        Bitmap pb = new Bitmap(120, 80);
        Graphics pg = new Graphics(pb);
        pg.Clear(Color.Black);
        Pen dash = new Pen(Color.White, 2) { DashStyle = DashStyle.Dash };   // 6 on, 2 off
        pg.DrawLine(dash, 0, 5, 100, 5);
        bool dashOk = pb.GetPixel(2, 5).R > 200 && pb.GetPixel(7, 5).R < 50 && pb.GetPixel(10, 5).R > 200;
        Pen round = new Pen(Color.Red, 10) { StartCap = LineCap.Round, EndCap = LineCap.Round };
        pg.DrawLine(round, 20, 25, 60, 25);
        bool capOk = pb.GetPixel(17, 25).R > 200 && pb.GetPixel(40, 29).R > 200 && pb.GetPixel(40, 33).R < 50;
        Pen flat = new Pen(Color.Green, 10);
        pg.DrawLine(flat, 70, 25, 100, 25);
        bool flatOk = pb.GetPixel(67, 25).G < 50 && pb.GetPixel(80, 25).G > 140;
        // a right-angle polyline with a miter join fills the outer corner
        int[] lx = new int[3]; int[] ly = new int[3];
        lx[0] = 10; ly[0] = 70; lx[1] = 40; ly[1] = 70; lx[2] = 40; ly[2] = 45;
        Pen miter = new Pen(Color.Blue, 8) { LineJoin = LineJoin.Miter };
        pg.DrawLines(miter, lx, ly, 3);
        bool joinOk = pb.GetPixel(43, 73).B > 200;
        Bitmap tile = new Bitmap(2, 2);
        tile.SetPixel(0, 0, Color.Yellow); tile.SetPixel(1, 0, Color.Cyan);
        tile.SetPixel(0, 1, Color.Cyan); tile.SetPixel(1, 1, Color.Yellow);
        pg.FillRectangle(new TextureBrush(tile), 60, 40, 20, 20);
        bool texOk = pb.GetPixel(60, 40).Rgb24 == Color.Yellow.Rgb24 && pb.GetPixel(61, 40).Rgb24 == Color.Cyan.Rgb24
                     && pb.GetPixel(62, 41).Rgb24 == Color.Cyan.Rgb24;
        Region reg = new Region(85, 40, 30, 30);
        reg.ExcludeEllipse(90, 45, 20, 20);
        pg.FillRegion(Color.Magenta, reg);
        bool regOk = pb.GetPixel(86, 41).Rgb24 == Color.Magenta.Rgb24 && pb.GetPixel(100, 55).Rgb24 != Color.Magenta.Rgb24
                     && reg.IsVisible(114, 69) && !reg.IsVisible(100, 55) && !reg.IsVisible(120, 50);
        pg.SetClip(reg);
        pg.FillRectangle(Color.White, 80, 35, 40, 40);
        pg.ResetRegionClip();
        bool regClipOk = pb.GetPixel(86, 41).R > 200 && pb.GetPixel(100, 55).R < 250 && pb.GetPixel(82, 37).R < 50;
        bool penOk = dashOk && capOk && flatOk && joinOk && texOk && regOk && regClipOk;
        if (!dashOk) Console.WriteLine("Draw: pen dash gagal");
        if (!capOk) Console.WriteLine("Draw: pen round cap gagal");
        if (!flatOk) Console.WriteLine("Draw: pen flat cap gagal");
        if (!joinOk) Console.WriteLine("Draw: pen miter join gagal");
        if (!texOk) Console.WriteLine("Draw: texture brush gagal");
        if (!regOk) Console.WriteLine("Draw: region gagal");
        if (!regClipOk) Console.WriteLine("Draw: region clip gagal");
        if (penOk)
            Console.WriteLine("Draw: pen dash/cap/join, texture brush, region OK");
        g.DrawImage(pb, 236, 120);

        // Present the finished frame to a window + a native text label on top.
        Window win = Window.Create("Buitenzorg.Drawing", W, H);
        win.Blit(bmp);
        win.DrawText(20, 220, Color.White, "shapes + gradient + rounded + shadow + AA + path + font");
        win.Present();

        if (ok && pathOk && bmpOk && textOk && clipOk && hatchOk && scaleOk && roundOk && gradHOk && aaOk && penOk)
            Console.WriteLine("MILESTONE: DRAW OK");
        else
            Console.WriteLine("Draw: verifikasi gagal (pixel/path/bmp/font/clip/hatch/scale/rounded/gradH/aa)");
    }
}
