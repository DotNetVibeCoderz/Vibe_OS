// Task Manager / Monitor Sistem (v0.9 "Serbuk") — a Windows-style task manager.
// Lists processes (kernel tasks + running app), shows CPU/memory/uptime, and
// can kill a task. Draws its UI with Buitenzorg.Drawing over the proc syscalls.
//
// Freestanding (no GC): all text is built into stackalloc char buffers and
// drawn with DrawChars — no managed strings, no heap allocation.

using System;
using System.Runtime.InteropServices;
using Buitenzorg.Drawing;

unsafe class TaskMgr
{
    struct ProcInfo   // mirror bz_abi::ProcInfo (64 bytes)
    {
        public ulong Pid, State, CpuTicks, Kind;
        public fixed byte Name[32];
    }

    struct SysStat    // mirror bz_abi::SysStat (48 bytes)
    {
        public ulong UptimeTicks, TickHz, HeapUsed, HeapTotal, TaskCount, MemTotalMiB;
    }

    [DllImport("*")] static extern unsafe ulong bz_proc_list(ProcInfo* buf, ulong max);
    [DllImport("*")] static extern ulong bz_proc_kill(ulong pid);
    [DllImport("*")] static extern unsafe ulong bz_sys_stat(SysStat* outp);

    static void Main()
    {
        Console.WriteLine("[taskmgr] starting (process + resource monitor)");
        var g = Graphics.CreateWindow("Task Manager", 560, 380);

        for (int frame = 0; frame < 4; frame++) { Render(g); Sys.Sleep(9); }

        // Demonstrate PROC_KILL: terminate the idle-demo task if present.
        // Use a fixed byte buffer (4096 = 64 * sizeof(ProcInfo)) and reinterpret;
        // stackalloc of a struct array would emit a checked multiply that
        // zerolib's ThrowHelpers can't satisfy.
        byte* raw = stackalloc byte[4096];
        ProcInfo* procs = (ProcInfo*)raw;
        int n = (int)bz_proc_list(procs, 64);
        for (int i = 0; i < n; i++)
        {
            if (NameEquals(procs[i].Name, "idle-demo"))
            {
                ulong rc = bz_proc_kill(procs[i].Pid);
                Console.WriteLine(rc == 0 ? "[taskmgr] killed idle-demo (PID freed)" : "[taskmgr] kill refused");
                break;
            }
        }
        Render(g);
        Console.WriteLine("[taskmgr] done");
        Sys.Sleep(18);
    }

    static void Render(Graphics g)
    {
        g.Clear(Color.Soil);
        g.FillRectangle(new Brush(Color.Green), 0, 0, 560, 28);
        g.DrawString("Task Manager - Buitenzorg", Color.Soil, 10, 6);

        SysStat st; bz_sys_stat(&st);
        int memPct = st.HeapTotal > 0 ? (int)(st.HeapUsed * 100 / st.HeapTotal) : 0;
        ulong secs = st.TickHz > 0 ? st.UptimeTicks / st.TickHz : 0;

        char* line = stackalloc char[80];
        int n;
        n = 0; n = Str(line, n, "Uptime: "); n = Num(line, n, secs); n = Str(line, n, "s");
        g.DrawChars(line, n, Color.Text, 10, 38);
        n = 0; n = Str(line, n, "Heap: "); n = Num(line, n, st.HeapUsed / 1024);
        n = Str(line, n, "/"); n = Num(line, n, st.HeapTotal / 1024); n = Str(line, n, " KiB");
        g.DrawChars(line, n, Color.Text, 190, 38);
        n = 0; n = Str(line, n, "RAM: "); n = Num(line, n, st.MemTotalMiB); n = Str(line, n, " MiB");
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

        byte* raw = stackalloc byte[4096];
        ProcInfo* procs = (ProcInfo*)raw;
        int count = (int)bz_proc_list(procs, 64);
        int y = 108;
        for (int i = 0; i < count && y < 356; i++)
        {
            n = 0; n = Num(line, n, procs[i].Pid);
            g.DrawChars(line, n, Color.Text, 16, y);
            DrawName(g, procs[i].Name, 90, y);
            g.DrawString(procs[i].Kind == 1 ? "app" : "kernel", Color.Text, 320, y);
            n = 0; n = Num(line, n, procs[i].CpuTicks);
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

    static int Num(char* buf, int n, ulong v)
    {
        char* tmp = stackalloc char[24];
        int t = 0;
        if (v == 0) tmp[t++] = '0';
        while (v > 0) { tmp[t++] = (char)('0' + (int)(v % 10)); v /= 10; }
        for (int i = t - 1; i >= 0; i--) buf[n++] = tmp[i];
        return n;
    }

    static void DrawName(Graphics g, byte* name, int x, int y)
    {
        char* buf = stackalloc char[32];
        int n = 0;
        for (int i = 0; i < 31 && name[i] != 0; i++) buf[n++] = (char)name[i];
        g.DrawChars(buf, n, Color.Leaf, x, y);
    }

    static bool NameEquals(byte* name, string s)
    {
        for (int i = 0; i < s.Length; i++)
            if (name[i] != (byte)s[i]) return false;
        return name[s.Length] == 0;
    }
}
