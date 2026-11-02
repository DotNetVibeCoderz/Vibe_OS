// HelloGui -- a managed C# desktop app on Buitenzorg (RustCLR + Buitenzorg.App).
//
// An analog clock with the real time (DateTime.Now from the board's RTC),
// hands placed with Math.Sin/Cos, a gradient title bar rendered into an
// int[] and blitted, and a status list built with LINQ. Started from the
// desktop (`run hellogui` / Start menu) it keeps the clock running on a
// background thread until Esc or Q; during the boot self-test it draws one
// frame, reports, and exits.

using Buitenzorg.App;

namespace HelloGui;

static class Program
{
    const int W = 360, H = 300;
    const int Cx = 120, Cy = 168, R = 86;
    static readonly object Gate = new();

    static int Main(string[] args)
    {
        var window = new Window("Jam C# managed (RustCLR)", W, H);
        DrawStatic(window);
        DrawClock(window, DateTime.Now);
        window.Present();
        Console.WriteLine($"HelloGui: window {window.Id} drawn ({W}x{H} on a {Screen.Width}x{Screen.Height} screen), time {DateTime.Now:HH:mm:ss}");

        // `--selftest`: run the live loop for a few seconds without a user (the
        // boot self-test), and require the clock to have ticked.
        int selftest = Array.IndexOf(args, "--selftest") >= 0 ? 5 : 0;
        if (!Session.IsInteractive && selftest == 0)
            return 0;

        // Live: a background thread ticks the clock, this thread reads keys.
        bool running = true;
        int ticks = 0;
        var ticker = new Thread(() =>
        {
            int last = -1;
            while (Volatile.Read(ref running))
            {
                var now = DateTime.Now;
                if (now.Second != last)
                {
                    last = now.Second;
                    lock (Gate)
                    {
                        DrawClock(window, now);
                        window.Present();
                    }
                    Interlocked.Increment(ref ticks);
                }
                Thread.Sleep(100);
            }
        }) { IsBackground = true };
        ticker.Start();

        var watch = System.Diagnostics.Stopwatch.StartNew();
        while (true)
        {
            if (selftest > 0 && watch.ElapsedMilliseconds >= selftest * 1000)
                break;
            char key = Keyboard.Read();
            if (key == Keyboard.Escape || key == 'q' || key == 'Q')
                break;
            if (key == '\0')
                Thread.Sleep(50);
        }
        Volatile.Write(ref running, false);
        ticker.Join();
        if (selftest > 0)
        {
            int n = Volatile.Read(ref ticks);
            Console.WriteLine($"HelloGui: live clock ticked {n} times in {selftest} s on a background thread");
            return n >= 2 ? 0 : 1;
        }
        return 0;
    }

    static void DrawStatic(Window w)
    {
        w.Clear(Colors.Forest);

        // A gradient band, computed into a managed int[] and blitted.
        const int bandH = 36;
        int[] band = new int[W * bandH];
        for (int y = 0; y < bandH; y++)
            for (int x = 0; x < W; x++)
            {
                int g = 0x70 + x * 0x60 / W;
                int b = 0x50 + y * 0x50 / bandH;
                band[y * W + x] = unchecked((int)0xFF000000) | Colors.Rgb(0x20, g, b);
            }
        w.Blit(0, 0, W, bandH, band);
        w.DrawText(12, 10, "C# managed di Buitenzorg", Colors.White);

        // Facts, assembled with LINQ.
        var facts = new List<(string Key, string Value)>
        {
            ("Runtime", "RustCLR"),
            ("GC", "mark-sweep"),
            ("Bahasa", "C# 14"),
            ("Layar", $"{Screen.Width}x{Screen.Height}"),
        };
        int line = 0;
        foreach (var text in facts.Where(f => f.Value.Length > 0).Select(f => $"{f.Key}: {f.Value}"))
            w.DrawText(232, 70 + 18 * line++, text, Colors.Leaf);
        w.DrawText(232, 70 + 18 * line + 10, "Esc / Q: tutup", Colors.Gray);

        // The dial: rim and twelve ticks.
        w.FillEllipse(Cx - R, Cy - R, 2 * R, 2 * R, 0x1E4D33);
        w.DrawEllipse(Cx - R, Cy - R, 2 * R, 2 * R, Colors.Leaf);
        for (int i = 0; i < 12; i++)
        {
            double a = i * Math.PI / 6;
            int x0 = Cx + (int)Math.Round(Math.Sin(a) * (R - 10)), y0 = Cy - (int)Math.Round(Math.Cos(a) * (R - 10));
            int x1 = Cx + (int)Math.Round(Math.Sin(a) * (R - 2)), y1 = Cy - (int)Math.Round(Math.Cos(a) * (R - 2));
            w.DrawLine(x0, y0, x1, y1, Colors.White);
        }
    }

    static void Hand(Window w, double turns, int length, int color)
    {
        double a = turns * 2 * Math.PI;
        w.DrawLine(Cx, Cy, Cx + (int)Math.Round(Math.Sin(a) * length), Cy - (int)Math.Round(Math.Cos(a) * length), color);
    }

    static void DrawClock(Window w, DateTime t)
    {
        // Repaint the face inside the ticks, then the hands.
        w.FillEllipse(Cx - (R - 12), Cy - (R - 12), 2 * (R - 12), 2 * (R - 12), 0x1E4D33);
        Hand(w, (t.Hour % 12 + t.Minute / 60.0) / 12.0, R - 45, Colors.White);
        Hand(w, (t.Minute + t.Second / 60.0) / 60.0, R - 25, Colors.Leaf);
        Hand(w, t.Second / 60.0, R - 18, Colors.Yellow);
        w.FillEllipse(Cx - 4, Cy - 4, 8, 8, Colors.White);
        // The digital time under the title band (the window's client area is
        // a little smaller than its requested size: keep clear of the bottom).
        w.FillRect(12, 44, 200, 20, Colors.Forest);
        w.DrawText(12, 46, t.ToString("dddd HH:mm:ss"), Colors.White);
    }
}
