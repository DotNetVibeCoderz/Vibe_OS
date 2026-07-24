// Buitenzorg OS — v0.16 "Panen" preloaded suite: Kalkulator.
//
// A calculator app built on Buitenzorg.UI: a Grid of themed buttons over a
// numeric display. Button clicks are dispatched (by Tag) into the Calc engine;
// the demo simulates "12 + 3 =", verifies the result, renders the window, and
// prints MILESTONE: CALC OK. Built with bflat --stdlib:zero together with
// bzui.cs, bzgfx.cs.

using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

// The calculator engine (all state in instance fields — no statics under zerolib).
sealed class Calc
{
    long _acc, _cur;
    int _op;        // 0=none, 10=+, 11=-, 12=*, 13=/
    bool _entering;

    public void Press(int k)
    {
        if (k >= 0 && k <= 9) { _cur = _cur * 10 + k; _entering = true; }
        else if (k == 15) { _acc = 0; _cur = 0; _op = 0; _entering = false; }   // C
        else if (k == 14)                                                        // =
        {
            _acc = _op == 0 ? _cur : Compute(_acc, _cur, _op);
            _op = 0; _cur = 0; _entering = false;
        }
        else                                                                     // + - * /
        {
            _acc = _op == 0 ? (_entering ? _cur : _acc) : Compute(_acc, _cur, _op);
            _op = k; _cur = 0; _entering = false;
        }
    }
    static long Compute(long a, long b, int o)
    {
        if (o == 10) return a + b;
        if (o == 11) return a - b;
        if (o == 12) return a * b;
        if (o == 13) return b != 0 ? a / b : 0;
        return b;
    }
    public long Shown() => _entering ? _cur : _acc;
}

// A right-aligned numeric display (renders its long value without a managed string).
sealed class CalcDisplay : UIElement
{
    public long Value;
    Font _font;
    readonly char[] _tmp = new char[24];
    readonly char[] _dig = new char[24];
    public CalcDisplay(Font f) { _font = f; }
    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 200; DesiredH = Height >= 0 ? Height : 42; }
    int Format(long v)
    {
        bool neg = v < 0; if (neg) v = -v;
        int n = 0;
        if (v == 0) _dig[n++] = '0';
        else while (v > 0 && n < 20) { _dig[n++] = (char)('0' + (int)(v % 10)); v /= 10; }
        int len = 0;
        if (neg) _tmp[len++] = '-';
        while (n > 0) _tmp[len++] = _dig[--n];
        return len;
    }
    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF10141A), X, Y, W, H, 6);
        g.DrawRoundedRectangle(new Color(0xFF3A4050), X, Y, W, H, 6);
        int len = Format(Value);
        int tw = len * _font.CharW;
        g.DrawChars(_font, _tmp, len, new Color(0xFF7CFF9C), X + W - tw - 8, Y + (H - _font.CharH) / 2);
    }
}

class CalcApp
{
    static Button Key(Grid grid, Font f, string label, int tag, int row, int col)
    {
        Button b = new Button(label, f);
        b.Tag = tag; b.GridRow = row; b.GridCol = col;
        if (tag >= 10) { b.Normal = new Color(0xFF8C5A28); b.Hover = new Color(0xFFB47838); } // operators warm
        grid.Add(b);
        return b;
    }

    static void Tap(UIHost h, Calc c, CalcDisplay d, Button b)
    {
        int before = b.Clicks;
        h.Mouse(b.X + b.W / 2, b.Y + b.H / 2, true);
        h.Mouse(b.X + b.W / 2, b.Y + b.H / 2, false);
        if (b.Clicks > before) c.Press(b.Tag);
        d.Value = c.Shown();
    }

    static void Main()
    {
        Console.WriteLine("Calc: Kalkulator (Buitenzorg.UI)...");
        Font font = Font.Default();
        const int W = 240, H = 300;
        UIHost host = new UIHost("Kalkulator", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 10; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);

        TextBlock title = new TextBlock("KALKULATOR", font);
        title.Foreground = Color.White;

        CalcDisplay disp = new CalcDisplay(font);
        disp.Height = 42;

        Grid grid = new Grid();
        grid.Height = 188; grid.Spacing = 6;
        grid.AddColumn(-1); grid.AddColumn(-1); grid.AddColumn(-1); grid.AddColumn(-1);
        grid.AddRow(-1); grid.AddRow(-1); grid.AddRow(-1); grid.AddRow(-1);

        Key(grid, font, "7", 7, 0, 0); Key(grid, font, "8", 8, 0, 1); Key(grid, font, "9", 9, 0, 2); Button bplus = Key(grid, font, "+", 10, 0, 3);
        Key(grid, font, "4", 4, 1, 0); Key(grid, font, "5", 5, 1, 1); Key(grid, font, "6", 6, 1, 2); Key(grid, font, "-", 11, 1, 3);
        Button b1 = Key(grid, font, "1", 1, 2, 0); Button b2 = Key(grid, font, "2", 2, 2, 1); Button b3 = Key(grid, font, "3", 3, 2, 2); Key(grid, font, "*", 12, 2, 3);
        Key(grid, font, "C", 15, 3, 0); Key(grid, font, "0", 0, 3, 1); Button beq = Key(grid, font, "=", 14, 3, 2); Key(grid, font, "/", 13, 3, 3);

        root.Add(title); root.Add(disp); root.Add(grid);
        host.Root = root;
        host.Layout();

        // Simulate "1 2 + 3 =" -> 15 by clicking the actual buttons.
        Calc calc = new Calc();
        Tap(host, calc, disp, b1);
        Tap(host, calc, disp, b2);
        Tap(host, calc, disp, bplus);
        Tap(host, calc, disp, b3);
        Tap(host, calc, disp, beq);

        host.Render(new Color(0xFF141820));
        host.Present();

        bool ok = disp.Value == 15 && beq.Clicks == 1 && b1.Clicks == 1;
        if (ok)
            Console.WriteLine("MILESTONE: CALC OK");
        else
            Console.WriteLine("Calc: verifikasi gagal (hitung/tombol)");
    }
}
