// Widget (v0.9 "Serbuk") — a lightweight system-monitor widget docked on the
// widget board. The "widget:" title prefix makes the WM dock it on the right
// edge. Widget app variant (requirements.md §11.4): small, periodic updates.
//
// Machine data comes from Buitenzorg.Bcl's `BzSystemInfo` (System.Management)
// and the numbers are formatted by `BzCulture` (System.Globalization), so the
// SysStat layout is not mirrored a second time here. The refresh cadence is a
// `BzTimer` (System.Timers), polled from the render loop.

using System;
using Buitenzorg;
using Buitenzorg.Drawing;

unsafe class Widget
{

    static void Main()
    {
        Console.WriteLine("[widget] starting (system monitor widget)");
        // "widget:" prefix -> docked on the widget board.
        var g = Graphics.CreateWindow("widget:Monitor", 220, 120);

        // Refresh on a timer rather than a bare sleep: the widget variant is
        // meant to update periodically, and BzTimer is what an app would use.
        BzTimer refresh = new BzTimer(3);
        refresh.Start();
        for (int frame = 0; frame < 3; frame++)
        {
            Render(g);
            while (!refresh.Poll()) { }
        }
        Console.WriteLine("[widget] done");
    }

    static void Render(Graphics g)
    {
        BzSystemInfo st = BzSystemInfo.Query();
        g.Clear(Color.FromRgb(0x0E, 0x18, 0x10));
        g.DrawString("Monitor Sistem", Color.Leaf, 8, 4);

        ulong secs = st.UptimeSeconds;
        int memPct = st.HeapPercent;

        char* line = stackalloc char[48];
        int n = 0; n = Str(line, n, "Uptime "); n = Num(line, n, secs); n = Str(line, n, "s");
        g.DrawChars(line, n, Color.Text, 8, 28);

        // Memory ring (ellipse) + label.
        g.DrawEllipse(new Pen(Color.Leaf), 8, 48, 40, 40);
        g.FillEllipse(new Brush(memPct > 80 ? Color.Red : Color.Green), 18, 58, 20, 20);
        n = 0; n = Str(line, n, "Mem "); n = Num(line, n, (ulong)memPct); n = Str(line, n, "%");
        g.DrawChars(line, n, Color.Text, 60, 58);
        n = 0; n = Str(line, n, "RAM "); n = Num(line, n, st.MemTotalMib); n = Str(line, n, " MiB");
        g.DrawChars(line, n, Color.Text, 60, 78);

        g.Present();
    }

    static int Str(char* buf, int n, string s) { fixed (char* sc = s) for (int i = 0; i < s.Length; i++) buf[n++] = sc[i]; return n; }
    // Formatting via System.Globalization, copied into the stack buffer the
    // older Drawing API takes.
    static int Num(char* buf, int n, ulong v)
    {
        char[] tmp = new char[24];
        int t = BzCulture.FormatInt((long)v, tmp);
        for (int i = 0; i < t; i++) buf[n++] = tmp[i];
        return n;
    }
}
