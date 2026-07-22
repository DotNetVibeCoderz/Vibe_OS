// Paint — a Buitenzorg.Drawing (System.Drawing-style) demo app (v0.9).
// Draws shapes with Graphics/Pen/Brush to show the managed graphics library.

using System;
using Buitenzorg.Drawing;

unsafe class Paint
{
    static void Main()
    {
        Console.WriteLine("[paint] starting (Buitenzorg.Drawing demo)");
        var g = Graphics.CreateWindow("Paint - Buitenzorg.Drawing", 420, 300);

        g.Clear(Color.Soil);

        // Header bar.
        g.FillRectangle(new Brush(Color.Green), 0, 0, 420, 28);
        g.DrawString("System.Drawing-style API", Color.Soil, 10, 6);

        // Shapes: filled + outline ellipses, rectangles, lines.
        g.FillEllipse(new Brush(Color.Red), 30, 50, 90, 60);
        g.DrawEllipse(new Pen(Color.Yellow), 140, 50, 90, 60);
        g.FillRectangle(new Brush(Color.Blue), 250, 50, 90, 60);
        g.DrawRectangle(new Pen(Color.Leaf), 250, 50, 90, 60);

        // A little "chart": bars + a trend line.
        int baseY = 250;
        var barBrush = new Brush(Color.Leaf);
        int* hp = stackalloc int[7];
        hp[0] = 40; hp[1] = 70; hp[2] = 55; hp[3] = 95; hp[4] = 60; hp[5] = 110; hp[6] = 80;
        for (int i = 0; i < 7; i++)
            g.FillRectangle(barBrush, 30 + i * 34, baseY - hp[i], 24, hp[i]);
        var pen = new Pen(Color.Yellow);
        for (int i = 0; i < 6; i++)
            g.DrawLine(pen, 42 + i * 34, baseY - hp[i], 42 + (i + 1) * 34, baseY - hp[i + 1]);

        g.DrawString("Ellipse, Rectangle, Line, Fill", Color.Text, 10, 160);
        g.Present();

        Console.WriteLine("[paint] drawn; exiting");
        Sys.Sleep(27);
    }
}
