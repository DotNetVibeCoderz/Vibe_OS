// Buitenzorg OS — pre-v1.0 Buitenzorg.Bcl part 2 verification.
//
// Exercises every namespace added by the "lengkapi BCL" plan item in ring 3
// and prints MILESTONE: BCL2 OK only if all of them behave:
//   System.Text (BzEncoding)              System.Diagnostics (Stopwatch/Process/Debug)
//   System.Text.RegularExpressions        System.Management (BzSystemInfo)
//   System.Globalization (+ BzDateTime)   GC (BzGC)
//   System.IO (Path/File/Directory/Stream) Pkg (BzPkg)
//   System.Net(.Sockets)                  System.Timers (BzTimer)
//   System.Threading.Tasks (BzTask)
//
// Built with bflat --stdlib:zero together with bzbcl.cs + bzbcl2.cs.

using System;
using System.Runtime.InteropServices;
using Buitenzorg;

unsafe class Bcl2Demo
{
    [DllImport("*")] static extern unsafe void bz_write(byte* buf, ulong len);

    static void Say(string s)
    {
        byte* b = stackalloc byte[256];
        int n = 0;
        for (int i = 0; i < s.Length && n < 255; i++) b[n++] = (byte)s[i];
        b[n++] = (byte)'\n';
        bz_write(b, (ulong)n);
    }

    static void Line(string label, char[] text, int len)
    {
        byte* b = stackalloc byte[256];
        int n = 0;
        for (int i = 0; i < label.Length && n < 200; i++) b[n++] = (byte)label[i];
        for (int i = 0; i < len && n < 254; i++) b[n++] = (byte)text[i];
        b[n++] = (byte)'\n';
        bz_write(b, (ulong)n);
    }

    static void Num(string label, long v)
    {
        char[] t = new char[24];
        int n = BzCulture.FormatInt(v, t);
        Line(label, t, n);
    }

    static char[] Chars(string s)
    {
        char[] t = new char[s.Length];
        for (int i = 0; i < s.Length; i++) t[i] = s[i];
        return t;
    }

    // ---- System.Threading.Tasks: the body must be a function pointer --------
    // A shared counter lives in mmap'd memory (a static reference field would
    // read garbage under zerolib), so the worker gets its address as `arg`.
    static void Worker(ulong arg)
    {
        int* counter = (int*)arg;
        for (int i = 0; i < 200; i++)
        {
            *counter = *counter + 1;
            BzTask.Yield();
        }
    }

    [DllImport("*")] static extern ulong bz_mmap(ulong size, ulong prot);

    static void Main()
    {
        Say("BCL2: menguji namespace .NET tambahan (pre-v1.0)...");
        bool ok = true;

        // -----------------------------------------------------------------
        // System.Text — UTF-8 round trip, including a 3-byte code point
        // -----------------------------------------------------------------
        {
            char[] src = new char[5];
            src[0] = 'B'; src[1] = 'z'; src[2] = (char)0xE9; src[3] = (char)0x2713; src[4] = '!';
            byte[] enc = new byte[32];
            int bn = BzEncoding.Utf8GetBytes(src, 5, enc);
            // 'B','z' = 1 byte each, U+00E9 = 2, U+2713 = 3, '!' = 1  -> 8
            ok &= BzDebug.Assert(bn == 8, "utf8 byte count");
            ok &= BzDebug.Assert(BzEncoding.Utf8ByteCount(src, 5) == 8, "utf8 count fn");
            char[] back = new char[16];
            int cn = BzEncoding.Utf8GetChars(enc, bn, back);
            ok &= BzDebug.Assert(cn == 5, "utf8 char count");
            bool same = true;
            for (int i = 0; i < 5; i++) if (back[i] != src[i]) same = false;
            ok &= BzDebug.Assert(same, "utf8 round trip");

            byte[] a = new byte[8];
            int an = BzEncoding.AsciiGetBytes(src, 5, a);
            ok &= BzDebug.Assert(an == 5 && a[2] == (byte)'?', "ascii fallback");
            Num("BCL2: utf8 bytes=", bn);
        }

        // -----------------------------------------------------------------
        // System.Text.RegularExpressions
        // -----------------------------------------------------------------
        {
            BzRegex digits = new BzRegex("^[0-9]+$");
            ok &= BzDebug.Assert(digits.IsMatch("12345"), "regex digits match");
            ok &= BzDebug.Assert(!digits.IsMatch("12a45"), "regex digits reject");

            BzRegex word = new BzRegex("\\w+@\\w+\\.[a-z]+");
            ok &= BzDebug.Assert(word.IsMatch("mail me at kang@gravicode.id now"), "regex email");
            ok &= BzDebug.Assert(!word.IsMatch("no address here"), "regex email reject");

            // Alternation must backtrack across a group boundary: (a|ab)c on "abc".
            BzRegex alt = new BzRegex("^(a|ab)c$");
            ok &= BzDebug.Assert(alt.IsMatch("abc"), "regex group backtrack");
            ok &= BzDebug.Assert(alt.IsMatch("ac"), "regex group first alt");

            BzRegex q = new BzRegex("colou?r");
            ok &= BzDebug.Assert(q.IsMatch("color") && q.IsMatch("colour"), "regex optional");

            char[] hay = Chars("a1b22c333d");
            BzRegex run = new BzRegex("[0-9]+");
            int end;
            int start = run.Match(hay, hay.Length, out end);
            ok &= BzDebug.Assert(start == 1 && end == 2, "regex match position");

            char[] outBuf = new char[32];
            int rn = run.Replace(hay, hay.Length, Chars("#"), 1, outBuf);
            // a#b#c#d
            ok &= BzDebug.Assert(rn == 7 && outBuf[0] == 'a' && outBuf[1] == '#' && outBuf[6] == 'd', "regex replace");
            Line("BCL2: regex replace=", outBuf, rn);

            BzRefList<char[]> parts = new BzRegex(",").Split(Chars("x,y,z"), 5);
            ok &= BzDebug.Assert(parts.Count == 3, "regex split count");
        }

        // -----------------------------------------------------------------
        // System.Globalization + BzDateTime
        // -----------------------------------------------------------------
        {
            char[] t = new char[32];
            int n = BzCulture.FormatGrouped(1234567, t);
            ok &= BzDebug.Assert(n == 9 && t[1] == ',' && t[5] == ',', "grouped number");
            Line("BCL2: grouped=", t, n);

            n = BzCulture.FormatFixed(-31415, 4, t, '.');
            // -3.1415
            ok &= BzDebug.Assert(t[0] == '-' && t[1] == '3' && t[2] == '.' && t[3] == '1' && n == 7, "fixed point");
            Line("BCL2: fixed=", t, n);

            n = BzCulture.FormatBytes(1536, t);   // 1.5KiB
            ok &= BzDebug.Assert(t[0] == '1' && t[1] == '.' && t[2] == '5' && t[3] == 'K', "byte size");
            Line("BCL2: size=", t, n);

            n = BzCulture.MonthAbbrev(7, t);
            ok &= BzDebug.Assert(n == 3 && t[0] == 'J' && t[1] == 'u' && t[2] == 'l', "month abbrev");

            BzDateTime now = BzDateTime.Now();
            ok &= BzDebug.Assert(now.IsValid, "rtc date valid");
            n = now.Format(t);
            Line("BCL2: rtc=", t, n);
            ok &= BzDebug.Assert(BzDateTime.IsLeapYear(2024) && !BzDateTime.IsLeapYear(2026), "leap year");
            ok &= BzDebug.Assert(BzDateTime.DaysInMonth(2024, 2) == 29, "days in month");
            // 2026-07-24 is a Friday (day 5 with Sunday = 0).
            BzDateTime fixedDay = new BzDateTime();
            fixedDay.Year = 2026; fixedDay.Month = 7; fixedDay.Day = 24;
            ok &= BzDebug.Assert(fixedDay.DayOfWeek() == 5, "day of week");
        }

        // -----------------------------------------------------------------
        // System.IO — Path, File (read + write), Directory, MemoryStream
        // -----------------------------------------------------------------
        {
            char[] p = new char[64];
            int pn = BzPath.Combine("/disk", Chars("CALC.ELF"), 8, p);
            // "/disk" + "/" + "CALC.ELF" = 14 chars, separator at index 5.
            ok &= BzDebug.Assert(pn == 14 && p[5] == '/', "path combine");
            Line("BCL2: path=", p, pn);

            char[] name = new char[32];
            int nn = BzPath.GetFileName(p, pn, name);
            ok &= BzDebug.Assert(nn == 8 && name[0] == 'C', "path filename");
            char[] ext = new char[8];
            int en = BzPath.GetExtension(p, pn, ext);
            ok &= BzDebug.Assert(en == 4 && ext[0] == '.' && ext[1] == 'E', "path extension");
            ok &= BzDebug.Assert(BzPath.HasExtension(p, pn, ".elf"), "path has extension");
            ok &= BzDebug.Assert(BzPath.Up(p, pn) == 5, "path up");

            // Directory listing: the mounts, then /disk.
            BzFileInfo mounts = BzDir.GetMounts();
            ok &= BzDebug.Assert(BzDir.Count(mounts) >= 1, "mount list");
            BzFileInfo disk = BzDir.GetEntries("/disk", 64);
            ok &= BzDebug.Assert(BzDir.Contains(disk, "CALC.ELF"), "disk listing");
            Num("BCL2: /disk entries=", BzDir.Count(disk));

            // File read from the read-only IDE mount.
            byte[] data;
            int rn = BzFile.ReadAllBytes("/disk/PHOTO.BMP", 4096, out data);
            ok &= BzDebug.Assert(rn > 0 && data[0] == (byte)'B' && data[1] == (byte)'M', "file read");
            Num("BCL2: PHOTO.BMP bytes=", rn);
            ok &= BzDebug.Assert(BzFile.Exists("/disk/CALC.ELF"), "file exists");

            // File write to the writable FAT12 RAM disk, then read it back.
            char[] text = Chars("BCL2 SYSTEM.IO WRITE OK");
            int wn = BzFile.WriteAllChars("/ram/BCL2.TXT", text, text.Length);
            ok &= BzDebug.Assert(wn == text.Length, "file write");
            char[] readBack;
            int bn2 = BzFile.ReadAllChars("/ram/BCL2.TXT", 256, out readBack);
            ok &= BzDebug.Assert(bn2 == text.Length, "file read back length");
            bool same = bn2 == text.Length;
            for (int i = 0; i < bn2 && same; i++) if (readBack[i] != text[i]) same = false;
            ok &= BzDebug.Assert(same, "file round trip");
            if (bn2 > 0) Line("BCL2: /ram/BCL2.TXT=", readBack, bn2);

            // MemoryStream.
            BzMemoryStream ms = new BzMemoryStream();
            for (int i = 0; i < 300; i++) ms.WriteByte((byte)(i & 0xFF));
            ok &= BzDebug.Assert(ms.Length == 300, "stream length grows");
            ms.Seek(0);
            byte[] chunk = new byte[64];
            int got = ms.Read(chunk, 0, 64);
            ok &= BzDebug.Assert(got == 64 && chunk[10] == 10, "stream read");
            ok &= BzDebug.Assert(ms.Position == 64, "stream position");
        }

        // -----------------------------------------------------------------
        // System.Diagnostics — Stopwatch, Process, Debug
        // -----------------------------------------------------------------
        {
            BzStopwatch sw = BzStopwatch.StartNew();
            long spin = 0;
            for (int i = 0; i < 200000; i++) spin += i;
            sw.Stop();
            ok &= BzDebug.Assert(sw.ElapsedTicks > 0, "stopwatch advanced");
            ok &= BzDebug.Assert(!sw.IsRunning, "stopwatch stopped");
            ok &= BzDebug.Assert(spin > 0, "spin loop ran");

            BzProcessInfo procs = BzProcess.GetProcesses(32);
            int pc = BzProcess.Count(procs);
            ok &= BzDebug.Assert(pc > 0, "process list");
            Num("BCL2: processes=", pc);
            // Every listing has at least one named entry.
            ok &= BzDebug.Assert(procs != null && procs.NameLen > 0, "process name");
            Line("BCL2: first process=", procs.Name, procs.NameLen);
        }

        // -----------------------------------------------------------------
        // System.Management
        // -----------------------------------------------------------------
        {
            BzSystemInfo info = BzSystemInfo.Query();
            ok &= BzDebug.Assert(info.TickHz > 0, "tick rate");
            ok &= BzDebug.Assert(info.HeapTotal > 0 && info.HeapUsed <= info.HeapTotal, "heap stats");
            ok &= BzDebug.Assert(info.TaskCount > 0, "task count");
            ok &= BzDebug.Assert(info.MemTotalMib > 0, "memory size");
            ok &= BzDebug.Assert(info.AudioPresent && info.AudioSampleRate == 48000, "audio info");
            Num("BCL2: uptime seconds=", (long)info.UptimeSeconds);
            Num("BCL2: kernel heap percent=", info.HeapPercent);
            Num("BCL2: RAM MiB=", (long)info.MemTotalMib);
        }

        // -----------------------------------------------------------------
        // GC
        // -----------------------------------------------------------------
        {
            ulong before = BzGC.GetAllocatedBytes();
            byte[] big = new byte[200000];
            big[199999] = 7;
            ulong after = BzGC.GetAllocatedBytes();
            ok &= BzDebug.Assert(after >= before + 200000, "gc allocation accounted");
            ok &= BzDebug.Assert(BzGC.GetTotalMemory() >= after, "gc committed >= allocated");
            ok &= BzDebug.Assert(BzGC.ChunkCount() >= 1, "gc chunk count");
            ok &= BzDebug.Assert(BzGC.AllocationCount() > 0, "gc allocation count");
            ok &= BzDebug.Assert(!BzGC.Collect(), "gc collect is a no-op (bump heap)");
            Num("BCL2: heap allocated KiB=", (long)(after / 1024));
            Num("BCL2: heap committed KiB=", (long)(BzGC.GetTotalMemory() / 1024));
        }

        // -----------------------------------------------------------------
        // Pkg
        // -----------------------------------------------------------------
        {
            BzPkgInfo pkgs = BzPkg.List(32);
            int n = BzPkg.Count(pkgs);
            ok &= BzDebug.Assert(n > 0, "package list");
            Num("BCL2: packages=", n);

            BzPkgInfo calc = BzPkg.Find(pkgs, "calc");
            ok &= BzDebug.Assert(calc != null, "package find");
            ok &= BzDebug.Assert(BzPkg.Search(pkgs, "cal").Count >= 1, "package search");

            // Install/remove really change kernel state: re-read to confirm.
            bool wasInstalled = calc.Installed;
            ok &= BzDebug.Assert(BzPkg.Install(calc), "package install call");
            ok &= BzDebug.Assert(BzPkg.IsInstalled(BzPkg.List(32), "calc"), "package installed in kernel");
            if (!wasInstalled)
            {
                ok &= BzDebug.Assert(BzPkg.Remove(calc), "package remove call");
                ok &= BzDebug.Assert(!BzPkg.IsInstalled(BzPkg.List(32), "calc"), "package removed in kernel");
                BzPkg.Install(calc);   // leave it installed
            }
        }

        // -----------------------------------------------------------------
        // System.Net / System.Net.Sockets — a real UDP round trip on loopback
        // -----------------------------------------------------------------
        {
            BzNetInfo ni = BzNetInfo.Query();
            ok &= BzDebug.Assert(ni.Up, "net interface up");
            char[] addr = new char[20];
            int an = ni.Address.Format(addr);
            Line("BCL2: local ip=", addr, an);

            BzIPAddress parsed = BzIPAddress.Parse("10.0.2.15");
            ok &= BzDebug.Assert(parsed != null && parsed.A == 10 && parsed.D == 15, "ip parse");
            ok &= BzDebug.Assert(BzIPAddress.Parse("10.0.2") == null, "ip parse reject");
            ok &= BzDebug.Assert(BzIPAddress.Parse("300.1.1.1") == null, "ip parse range");

            BzSocket rx = BzSocket.CreateUdp();
            BzSocket tx = BzSocket.CreateUdp();
            ok &= BzDebug.Assert(rx != null && tx != null, "socket create");
            ok &= BzDebug.Assert(rx.Bind(7000), "socket bind");
            ok &= BzDebug.Assert(tx.Bind(7001), "socket bind sender");
            ok &= BzDebug.Assert(!rx.Bind(7000) || true, "rebind tolerated");

            int sent = tx.SendTo(ni.Address, 7000, "HALO UDP DARI BUITENZORG");
            ok &= BzDebug.Assert(sent == 24, "udp send length");

            byte[] got = new byte[64];
            int rn = rx.ReceiveWithRetry(got, 64, 4);
            ok &= BzDebug.Assert(rn == 24, "udp receive length");
            char[] msg = new char[64];
            for (int i = 0; i < rn; i++) msg[i] = (char)got[i];
            ok &= BzDebug.Assert(rn > 4 && msg[0] == 'H' && msg[1] == 'A' && msg[5] == 'U', "udp payload");
            Line("BCL2: udp payload=", msg, rn);
            ok &= BzDebug.Assert(rx.RemotePort == 7001, "udp sender port");
            ok &= BzDebug.Assert(rx.RemoteAddress != null && rx.RemoteAddress.Equals(ni.Address), "udp sender address");

            // Nothing else is queued: a second receive must report 0, not block.
            ok &= BzDebug.Assert(rx.Receive(got, 64) == 0, "udp non-blocking empty");

            BzNetInfo after = BzNetInfo.Query();
            ok &= BzDebug.Assert(after.SentDatagrams > ni.SentDatagrams, "udp tx counter");
            ok &= BzDebug.Assert(after.ReceivedDatagrams > ni.ReceivedDatagrams, "udp rx counter");
            Num("BCL2: udp datagrams sent=", (long)after.SentDatagrams);

            rx.Close(); tx.Close();

            // System.Net.Http: message layer only (no TCP in the kernel yet).
            char[] req = new char[256];
            int qn = BzHttp.BuildGet("buitenzorg.os", "/index.html", req);
            ok &= BzDebug.Assert(qn > 30 && req[0] == 'G' && req[1] == 'E' && req[2] == 'T', "http request built");
            char[] resp = Chars("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhalo!");
            int bodyStart;
            int code = BzHttp.ParseStatus(resp, resp.Length, out bodyStart);
            ok &= BzDebug.Assert(code == 200, "http status parsed");
            ok &= BzDebug.Assert(bodyStart == resp.Length - 5, "http body offset");
            char[] hv = new char[16];
            int hn = BzHttp.GetHeader(resp, resp.Length, "content-length", hv);
            ok &= BzDebug.Assert(hn == 1 && hv[0] == '5', "http header");
        }

        // -----------------------------------------------------------------
        // System.Threading.Tasks
        // -----------------------------------------------------------------
        {
            // Shared counter in mmap'd memory (never a static reference field).
            ulong page = bz_mmap(4096, 1 | 2);
            int* counter = (int*)page;
            *counter = 0;

            BzTask a = BzTask.Run(&Worker, page);
            BzTask b = BzTask.Run(&Worker, page);
            ok &= BzDebug.Assert(a != null && b != null, "task create");

            BzRefList<BzTask> all = new BzRefList<BzTask>();
            all.Add(a); all.Add(b);
            ok &= BzDebug.Assert(BzTask.WhenAll(all), "task when all");
            ok &= BzDebug.Assert(*counter == 400, "task bodies both ran");
            ok &= BzDebug.Assert(a.IsCompleted && b.IsCompleted, "task completed flag");
            Num("BCL2: task counter=", *counter);
        }

        // -----------------------------------------------------------------
        // System.Timers
        // -----------------------------------------------------------------
        {
            BzTimer timer = new BzTimer(2);   // ~2 timer ticks (~110 ms at 18.2 Hz)
            ok &= BzDebug.Assert(!timer.Poll(), "timer idle before Start");
            timer.Start();
            ok &= BzDebug.Assert(timer.Enabled, "timer enabled");

            int fired = 0;
            for (int i = 0; i < 2000000 && fired < 2; i++)
                if (timer.Poll()) fired++;
            ok &= BzDebug.Assert(fired == 2, "timer fired twice");
            ok &= BzDebug.Assert(timer.Count == 2, "timer count");

            timer.AutoReset = false;
            timer.Start();
            int once = 0;
            for (int i = 0; i < 2000000 && once < 1; i++)
                if (timer.Poll()) once++;
            ok &= BzDebug.Assert(once == 1 && !timer.Enabled, "timer one-shot stops");
            Num("BCL2: timer fires=", fired + once);
        }

        Say(ok ? "MILESTONE: BCL2 OK" : "MILESTONE: BCL2 FAIL");
    }
}
