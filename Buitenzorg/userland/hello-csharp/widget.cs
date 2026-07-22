// Widget (v0.9 "Serbuk") — a lightweight system-monitor widget docked on the
// widget board. The "widget:" title prefix makes the WM dock it on the right
// edge. Widget app variant (requirements.md §11.4): small, periodic updates.

using System;
using System.Runtime.InteropServices;
using Buitenzorg.Drawing;

unsafe class Widget
{
    struct SysStat { public ulong UptimeTicks, TickHz, HeapUsed, HeapTotal, TaskCount, MemTotalMiB; }
    [DllImport("*")] static extern unsafe ulong bz_sys_stat(SysStat* outp);

    static void Main()
    {
        Console.WriteLine("[widget] starting (system monitor widget)");
        // "widget:" prefix -> docked on the widget board.
        var g = Graphics.CreateWindow("widget:Monitor", 220, 120);

        for (int frame = 0; frame < 5; frame++)
        {
            Render(g);
            Sys.Sleep(9);
        }
        Console.WriteLine("[widget] done");
    }

    static void Render(Graphics g)
    {
        SysStat st; bz_sys_stat(&st);
        g.Clear(Color.FromRgb(0x0E, 0x18, 0x10));
        g.DrawString("Monitor Sistem", Color.Leaf, 8, 4);

        ulong secs = st.TickHz > 0 ? st.UptimeTicks / st.TickHz : 0;
        int memPct = st.HeapTotal > 0 ? (int)(st.HeapUsed * 100 / st.HeapTotal) : 0;

        char* line = stackalloc char[48];
        int n = 0; n = Str(line, n, "Uptime "); n = Num(line, n, secs); n = Str(line, n, "s");
        g.DrawChars(line, n, Color.Text, 8, 28);

        // Memory ring (ellipse) + label.
        g.DrawEllipse(new Pen(Color.Leaf), 8, 48, 40, 40);
        g.FillEllipse(new Brush(memPct > 80 ? Color.Red : Color.Green), 18, 58, 20, 20);
        n = 0; n = Str(line, n, "Mem "); n = Num(line, n, (ulong)memPct); n = Str(line, n, "%");
        g.DrawChars(line, n, Color.Text, 60, 58);
        n = 0; n = Str(line, n, "RAM "); n = Num(line, n, st.MemTotalMiB); n = Str(line, n, " MiB");
        g.DrawChars(line, n, Color.Text, 60, 78);

        g.Present();
    }

    static int Str(char* buf, int n, string s) { fixed (char* sc = s) for (int i = 0; i < s.Length; i++) buf[n++] = sc[i]; return n; }
    static int Num(char* buf, int n, ulong v)
    {
        char* t = stackalloc char[24]; int k = 0;
        if (v == 0) t[k++] = '0';
        while (v > 0) { t[k++] = (char)('0' + (int)(v % 10)); v /= 10; }
        for (int i = k - 1; i >= 0; i--) buf[n++] = t[i];
        return n;
    }
}
