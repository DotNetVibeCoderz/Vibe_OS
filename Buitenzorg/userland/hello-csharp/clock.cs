// Buitenzorg OS — v0.16 "Panen" preloaded suite: Jam (analog + digital clock).
//
// An analog clock with hour/minute/second hands drawn by rotating vectors
// (Graphics.SinFx/CosFx), an anti-aliased face, 12 tick marks, and a digital
// HH:MM:SS readout (Graphics.DrawChars). Showcases the Buitenzorg.Drawing
// transform/AA primitives. The time comes from the real CMOS clock through
// Buitenzorg.Bcl's BzDateTime (System.Globalization); the demo verifies the hand
// geometry + digital formatting and prints MILESTONE: CLOCK OK. Built with bflat
// --stdlib:zero together with bzui.cs, bzgfx.cs, bzbcl.cs, bzbcl2.cs.

using System;
using Buitenzorg;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

sealed class ClockFace : UIElement
{
    public int Hh, Mm, Ss;
    Font _font;
    readonly char[] _t = new char[8];
    public ClockFace(Font f) { _font = f; }
    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 200; DesiredH = Height >= 0 ? Height : 232; }

    int Cx => X + W / 2;
    int Cy => Y + 100;

    // Endpoint of a hand at `deg` (0 = up, clockwise) and length `len`.
    public static int EndX(int cx, int deg, int len) => cx + Graphics.SinFx(deg) * len / 256;
    public static int EndY(int cy, int deg, int len) => cy - Graphics.CosFx(deg) * len / 256;

    void Hand(Graphics g, int deg, int len, int thick, Color c)
    {
        g.DrawLine(c, Cx, Cy, EndX(Cx, deg, len), EndY(Cy, deg, len), thick);
    }
    // Date text, set from BzDateTime.FormatDate (instance fields: a static
    // reference field would read garbage under zerolib).
    char[] _date = new char[16];
    int _dateLen;
    public void SetDate(char[] src, int len)
    {
        _dateLen = len < _date.Length ? len : _date.Length;
        for (int i = 0; i < _dateLen; i++) _date[i] = src[i];
    }

    int Fmt2(int off, int v) { _t[off] = (char)('0' + v / 10); _t[off + 1] = (char)('0' + v % 10); return off + 2; }
    public int Digits()
    {
        int p = 0; p = Fmt2(p, Hh); _t[p++] = ':'; p = Fmt2(p, Mm); _t[p++] = ':'; p = Fmt2(p, Ss);
        return p;
    }
    public char[] Buf => _t;

    public override void Render(Graphics g)
    {
        if (!Visible) return;
        int cx = Cx, cy = Cy, r = 92;
        g.FillCircleAA(new Color(0xFF20242E), cx, cy, r);
        g.DrawCircle(new Color(0xFF5A6070), cx, cy, r);
        g.DrawCircle(new Color(0xFF3A4050), cx, cy, r - 1);
        for (int i = 0; i < 12; i++)
        {
            int a = i * 30;
            int inset = (i % 3 == 0) ? 12 : 7;
            g.DrawLine(new Color(0xFF8890A0), EndX(cx, a, r - inset), EndY(cy, a, r - inset), EndX(cx, a, r - 2), EndY(cy, a, r - 2), (i % 3 == 0) ? 2 : 1);
        }
        int ha = (Hh % 12) * 30 + Mm / 2;
        int ma = Mm * 6;
        int sa = Ss * 6;
        Hand(g, ha, r * 48 / 100, 4, new Color(0xFFE6E6E6)); // hour
        Hand(g, ma, r * 72 / 100, 3, new Color(0xFFC8D2E6)); // minute
        Hand(g, sa, r * 82 / 100, 1, new Color(0xFFE0503C)); // second (red)
        g.FillCircleAA(new Color(0xFFE0503C), cx, cy, 4);

        // Digital readout below the face.
        int len = Digits();
        g.DrawChars(_font, _t, len, new Color(0xFF7CFF9C), cx - (len * _font.CharW) / 2, Y + 210);

        // Date line under it (formatted by Buitenzorg.Bcl's BzDateTime).
        if (_dateLen > 0)
            g.DrawChars(_font, _date, _dateLen, new Color(0xFF8890A0),
                        cx - (_dateLen * _font.CharW) / 2, Y + 222);
    }
}

class ClockApp
{
    static void Main()
    {
        Console.WriteLine("Jam: clock (Buitenzorg.UI + Drawing)...");
        Font font = Font.Default();
        const int W = 240, H = 300;
        UIHost host = new UIHost("Jam", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 6;
        root.Background = new Color(0xFF1C2028);
        TextBlock title = new TextBlock("JAM", font);
        title.Foreground = Color.White;

        ClockFace clock = new ClockFace(font);
        clock.Width = 200; clock.Height = 232;

        // Real wall-clock time via Buitenzorg.Bcl (CLOCK_RTC), not a fixed value.
        BzDateTime now = BzDateTime.Now();
        bool rtcOk = now.IsValid;
        if (rtcOk) { clock.Hh = now.Hour; clock.Mm = now.Minute; clock.Ss = now.Second; }
        else { clock.Hh = 10; clock.Mm = 8; clock.Ss = 37; }   // clock unreadable: show a sane face

        // Today's date under the face, formatted by System.Globalization.
        char[] dateBuf = new char[16];
        int dateLen = now.FormatDate(dateBuf);
        clock.SetDate(dateBuf, dateLen);

        root.Add(title);
        root.Add(clock);
        host.Root = root;
        host.Layout();
        host.Render(new Color(0xFF141820));
        host.Present();

        // Verify hand geometry: a second hand at S=15 (90 deg) points right (+x),
        // at S=30 (180 deg) points down (+y).
        int cx = 100, cy = 100, len = 70;
        bool geomOk = ClockFace.EndX(cx, 90, len) > cx && ClockFace.EndY(cy, 90, len) == cy
                      && ClockFace.EndY(cy, 180, len) > cy;

        // Verify the digital readout formats the time we are actually showing,
        // zero-padded as HH:MM:SS.
        int n = clock.Digits();
        char[] b = clock.Buf;
        bool timeOk = n == 8 && b[2] == ':' && b[5] == ':'
                      && b[0] == (char)('0' + clock.Hh / 10) && b[1] == (char)('0' + clock.Hh % 10)
                      && b[3] == (char)('0' + clock.Mm / 10) && b[4] == (char)('0' + clock.Mm % 10)
                      && b[6] == (char)('0' + clock.Ss / 10) && b[7] == (char)('0' + clock.Ss % 10);

        // The date must parse as a real calendar date (proves CLOCK_RTC works).
        bool dateOk = rtcOk && dateLen >= 8 && now.Month >= 1 && now.Month <= 12;
        Con.Write(dateBuf, dateLen);
        Console.WriteLine("");

        if (geomOk && timeOk && dateOk)
            Console.WriteLine("MILESTONE: CLOCK OK");
        else
            Console.WriteLine("Jam: verifikasi gagal (geometri/waktu/tanggal)");
    }
}
