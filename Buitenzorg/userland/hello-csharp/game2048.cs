// Buitenzorg OS — v0.16 "Panen" preloaded suite: 2048 (game).
//
// The classic 2048 sliding-tile game, built on Buitenzorg.UI + Drawing: a 4x4
// board of colored, numbered tiles rendered with rounded rectangles and the
// DrawChars numeric text path. The engine slides + merges tiles per move; the
// demo sets a known board, applies moves, verifies the merges, renders the
// window, and prints MILESTONE: GAME OK. Built with bflat --stdlib:zero
// together with bzui.cs, bzgfx.cs.

using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

// The 4x4 board + slide/merge engine (state in an int[16] value array).
sealed class Board2048 : UIElement
{
    readonly int[] _c = new int[16];
    Font _font;
    readonly char[] _tmp = new char[8];
    public int Score;
    public Board2048(Font f) { _font = f; }

    public int Get(int r, int c) => _c[r * 4 + c];
    public void Set(int r, int c, int v) => _c[r * 4 + c] = v;

    // Slide a 4-cell line toward index 0, merging equal neighbours once.
    bool SlideLine(int[] a)
    {
        int[] comp = new int[4]; int n = 0;
        for (int i = 0; i < 4; i++) if (a[i] != 0) comp[n++] = a[i];
        int[] outp = new int[4]; int m = 0, i2 = 0;
        while (i2 < n)
        {
            if (i2 + 1 < n && comp[i2] == comp[i2 + 1]) { int v = comp[i2] * 2; outp[m++] = v; Score += v; i2 += 2; }
            else { outp[m++] = comp[i2]; i2++; }
        }
        bool changed = false;
        for (int i = 0; i < 4; i++) { if (a[i] != outp[i]) changed = true; a[i] = outp[i]; }
        return changed;
    }

    // dir: 0=left, 1=right, 2=up, 3=down.
    public bool Move(int dir)
    {
        bool changed = false;
        int[] line = new int[4];
        for (int k = 0; k < 4; k++)
        {
            // Gather the line in slide order (toward index 0 = the move direction).
            for (int i = 0; i < 4; i++)
            {
                if (dir == 0) line[i] = Get(k, i);
                else if (dir == 1) line[i] = Get(k, 3 - i);
                else if (dir == 2) line[i] = Get(i, k);
                else line[i] = Get(3 - i, k);
            }
            if (SlideLine(line)) changed = true;
            for (int i = 0; i < 4; i++)
            {
                if (dir == 0) Set(k, i, line[i]);
                else if (dir == 1) Set(k, 3 - i, line[i]);
                else if (dir == 2) Set(i, k, line[i]);
                else Set(3 - i, k, line[i]);
            }
        }
        return changed;
    }

    static Color TileColor(int v)
    {
        if (v == 0) return new Color(0xFF2A2E38);
        if (v == 2) return new Color(0xFF6E7686);
        if (v == 4) return new Color(0xFF5E86B4);
        if (v == 8) return new Color(0xFFE09650);
        if (v == 16) return new Color(0xFFE07840);
        if (v == 32) return new Color(0xFFE05A46);
        if (v == 64) return new Color(0xFFD84632);
        if (v == 128) return new Color(0xFFE0C050);
        if (v == 256) return new Color(0xFFE0B840);
        if (v == 512) return new Color(0xFFE0B030);
        return new Color(0xFF50B478); // 1024+, green
    }
    int Format(int v)
    {
        int n = 0; char[] d = new char[8];
        if (v == 0) return 0;
        while (v > 0 && n < 7) { d[n++] = (char)('0' + v % 10); v /= 10; }
        int len = 0; while (n > 0) _tmp[len++] = d[--n];
        return len;
    }
    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 280; DesiredH = Height >= 0 ? Height : 280; }
    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF1A1D24), X, Y, W, H, 8);
        int pad = 8;
        int cell = (W - pad * 5) / 4;
        for (int r = 0; r < 4; r++)
            for (int c = 0; c < 4; c++)
            {
                int tx = X + pad + c * (cell + pad);
                int ty = Y + pad + r * (cell + pad);
                int v = Get(r, c);
                g.FillRoundedRectangle(TileColor(v), tx, ty, cell, cell, 5);
                if (v != 0)
                {
                    int len = Format(v);
                    int tw = len * _font.CharW;
                    Color fg = v <= 4 ? new Color(0xFFE6E6E6) : new Color(0xFF201810);
                    g.DrawChars(_font, _tmp, len, fg, tx + (cell - tw) / 2, ty + (cell - _font.CharH) / 2);
                }
            }
    }
}

class Game2048
{
    static void Main()
    {
        Console.WriteLine("2048: game (Buitenzorg.UI + Drawing)...");
        Font font = Font.Default();
        const int W = 300, H = 360;
        UIHost host = new UIHost("2048", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 10; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);
        TextBlock title = new TextBlock("2048", font);
        title.Foreground = Color.White;

        Board2048 board = new Board2048(font);
        board.Width = 280; board.Height = 280;

        root.Add(title);
        root.Add(board);
        host.Root = root;
        host.Layout();

        // Deterministic check: two 2s in a row slide-merge to a 4.
        board.Set(0, 0, 2); board.Set(0, 1, 2);
        bool moved = board.Move(0);                  // left
        bool mergeOk = moved && board.Get(0, 0) == 4 && board.Get(0, 1) == 0 && board.Score == 4;

        // A fuller check: 4 4 8 8 -> 8 16.
        board.Set(1, 0, 4); board.Set(1, 1, 4); board.Set(1, 2, 8); board.Set(1, 3, 8);
        board.Move(0);
        bool merge2Ok = board.Get(1, 0) == 8 && board.Get(1, 1) == 16 && board.Get(1, 2) == 0;

        // Populate a colourful board for the screenshot.
        board.Set(2, 0, 32); board.Set(2, 1, 64); board.Set(2, 2, 128); board.Set(2, 3, 256);
        board.Set(3, 0, 512); board.Set(3, 1, 1024); board.Set(3, 2, 2); board.Set(3, 3, 16);

        host.Render(new Color(0xFF141820));
        host.Present();

        if (mergeOk && merge2Ok)
            Console.WriteLine("MILESTONE: GAME OK");
        else
            Console.WriteLine("2048: verifikasi gagal (slide/merge)");
    }
}
