// Task Manager / Monitor Sistem (v0.9 "Serbuk") — a Windows-style task manager.
// Lists processes (kernel tasks + running app), shows CPU/memory/uptime, and
// can kill a task. Draws its UI with Buitenzorg.Drawing over the proc syscalls.
//
// Process and machine data come from Buitenzorg.Bcl — `BzProcess`
// (System.Diagnostics) and `BzSystemInfo` (System.Management) — rather than a
// private copy of the ABI structs, which used to be a second place that had to
// be updated by hand whenever ProcInfo/SysStat changed. Number and byte-size
// formatting comes from `BzCulture` (System.Globalization).

using System;
using Buitenzorg;
using Buitenzorg.Drawing;

unsafe class TaskMgr
{

    static void Main()
    {
        Console.WriteLine("[taskmgr] starting (process + resource monitor)");
        var g = Graphics.CreateWindow("Task Manager", 560, 380);

        for (int frame = 0; frame < 2; frame++) { Render(g); Sys.Sleep(3); }

        // Demonstrate PROC_KILL through System.Diagnostics.
        BzProcessInfo procs = BzProcess.GetProcesses(64);
        BzProcessInfo idle = BzProcess.FindByName(procs, "idle-demo");
        if (idle != null)
        {
            bool killed = BzProcess.Kill(idle.Pid);
            Console.WriteLine(killed ? "[taskmgr] killed idle-demo (PID freed)" : "[taskmgr] kill refused");
        }
        Render(g);
        Console.WriteLine("[taskmgr] done");
        Sys.Sleep(4);
    }

    static void Render(Graphics g)
    {
        g.Clear(Color.Soil);
        g.FillRectangle(new Brush(Color.Green), 0, 0, 560, 28);
        g.DrawString("Task Manager - Buitenzorg", Color.Soil, 10, 6);

        BzSystemInfo st = BzSystemInfo.Query();
        int memPct = st.HeapPercent;

        char* line = stackalloc char[80];
        int n;
        n = 0; n = Str(line, n, "Uptime: "); n = Num(line, n, st.UptimeSeconds); n = Str(line, n, "s");
        g.DrawChars(line, n, Color.Text, 10, 38);
        // Heap as human-readable sizes (System.Globalization).
        n = 0; n = Str(line, n, "Heap: "); n = Size(line, n, st.HeapUsed);
        n = Str(line, n, "/"); n = Size(line, n, st.HeapTotal);
        g.DrawChars(line, n, Color.Text, 190, 38);
        n = 0; n = Str(line, n, "RAM: "); n = Num(line, n, st.MemTotalMib); n = Str(line, n, " MiB");
        g.DrawChars(line, n, Color.Text, 430, 38);

        // Heap usage bar.
        g.DrawRectangle(new Pen(Color.Leaf), 10, 58, 540, 16);
        g.FillRectangle(new Brush(memPct > 80 ? Color.Red : Color.Leaf), 12, 60, 536 * memPct / 100, 12);

        // Process table header.
        g.FillRectangle(new Brush(Color.FromRgb(0x2C, 0x3A, 0x2C)), 10, 84, 540, 20);
        g.DrawString("PID", Color.Text, 16, 87);
        g.DrawString("Name", Color.Text, 90, 87);
        g.DrawString("Kind", Color.Text, 320, 87);
        g.DrawString("CPU(ticks)", Color.Text, 430, 87);

        BzProcessInfo procs = BzProcess.GetProcesses(64);
        int count = BzProcess.Count(procs);
        int y = 108;
        for (BzProcessInfo p = procs; p != null && y < 356; p = p.Next)
        {
            n = 0; n = Num(line, n, p.Pid);
            g.DrawChars(line, n, Color.Text, 16, y);
            DrawName(g, p.Name, p.NameLen, 90, y);
            g.DrawString(p.Kind == 1 ? "app" : "kernel", Color.Text, 320, y);
            n = 0; n = Num(line, n, p.CpuTicks);
            g.DrawChars(line, n, Color.Text, 430, y);
            y += 20;
        }
        n = 0; n = Num(line, n, (ulong)count); n = Str(line, n, " proses");
        g.DrawChars(line, n, Color.Text, 16, 360);
        g.Present();
    }

    // --- stack-only text helpers (no heap) ---

    static int Str(char* buf, int n, string s)
    {
        fixed (char* sc = s)
            for (int i = 0; i < s.Length; i++) buf[n++] = sc[i];
        return n;
    }

    // Numbers and byte sizes are formatted by System.Globalization; these two
    // just copy the result into the stack buffer the old Drawing API takes.
    static int Num(char* buf, int n, ulong v)
    {
        char[] tmp = new char[24];
        int t = BzCulture.FormatInt((long)v, tmp);
        for (int i = 0; i < t; i++) buf[n++] = tmp[i];
        return n;
    }

    static int Size(char* buf, int n, ulong bytes)
    {
        char[] tmp = new char[24];
        int t = BzCulture.FormatBytes(bytes, tmp);
        for (int i = 0; i < t; i++) buf[n++] = tmp[i];
        return n;
    }

    static void DrawName(Graphics g, char[] name, int len, int x, int y)
    {
        char* buf = stackalloc char[32];
        int n = 0;
        for (int i = 0; i < len && i < 31; i++) buf[n++] = name[i];
        g.DrawChars(buf, n, Color.Leaf, x, y);
    }
}
