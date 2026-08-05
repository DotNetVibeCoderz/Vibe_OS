// Buitenzorg desktop app template (v0.16 "Panen").
//
// A retained-mode Buitenzorg.UI window: a title, a live counter drawn with a
// custom UIElement, a "+1" Button, and a Gauge that tracks the count. It builds
// freestanding with bflat (--stdlib:zero) against the Buitenzorg.UI / .Drawing
// library sources, links with the bzstart shim into a static ELF, and deploys
// as /disk/USERAPP.ELF — launch it in the OS with `run myapp`.
//
// When launched from the desktop shell it runs a live keyboard loop
// (SPACE = +1, R = reset, ESC = exit); during a headless boot it renders once
// and exits so it can never block the boot.

using System;
using System.Runtime.InteropServices;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

class App
{
    [DllImport("*")] static extern uint bz_key_read();
    [DllImport("*")] static extern ulong bz_is_interactive();

    const int W = 360, H = 260;

    static void Main()
    {
        Console.WriteLine("[myapp] starting Buitenzorg.UI template app...");

        Font font = Font.Default();
        UIHost host = new UIHost("My Buitenzorg App", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 14; root.Spacing = 12;
        root.Background = new Color(0xFF141C16);

        TextBlock title = new TextBlock("BUITENZORG APP", font);
        title.Foreground = new Color(0xFF7FD48C);

        CounterView counter = new CounterView(font);
        counter.Height = 56;

        Gauge gauge = new Gauge(font);
        gauge.Height = 90; gauge.Min = 0; gauge.Max = 20;
        gauge.Foreground = Color.White;

        Button plus = new Button("+1", font);
        plus.Width = 90; plus.Height = 30;

        root.Add(title);
        root.Add(counter);
        root.Add(gauge);
        root.Add(plus);
        host.Root = root;
        host.Layout();

        // Headless self-check: two simulated clicks must bump the counter.
        int before = plus.Clicks;
        Tap(host, plus.X + plus.W / 2, plus.Y + plus.H / 2);
        Tap(host, plus.X + plus.W / 2, plus.Y + plus.H / 2);
        counter.Value = plus.Clicks - before;
        gauge.Value = counter.Value;
        host.Render(new Color(0xFF141820));
        host.Present();

        if (counter.Value == 2)
            Console.WriteLine("MILESTONE: MYAPP OK");
        else
            Console.WriteLine("[myapp] self-check failed");

        // Interactive session (launched from the desktop): loop on the keyboard.
        if (bz_is_interactive() != 0)
            Interactive(host, counter, gauge);
    }

    static void Interactive(UIHost host, CounterView counter, Gauge gauge)
    {
        while (true)
        {
            bool changed = false;
            uint k;
            while ((k = bz_key_read()) != 0)
            {
                if (k == 0x1B) return;                 // ESC exits
                else if (k == ' ') counter.Value++;    // SPACE = +1
                else if (k == 'r' || k == 'R') counter.Value = 0;
                else continue;
                if (counter.Value < 0) counter.Value = 0;
                gauge.Value = counter.Value;
                changed = true;
            }
            if (changed) { host.Render(new Color(0xFF141820)); host.Present(); }
        }
    }

    static void Tap(UIHost host, int x, int y)
    {
        host.Mouse(x, y, true);
        host.Mouse(x, y, false);
    }
}

// A custom control that draws a dynamic number. TextBlock takes a string and
// cannot show live values (no string concat under zerolib), so live numbers are
// rendered with Graphics.DrawChars over a char[] buffer filled by hand.
sealed class CounterView : UIElement
{
    public int Value;
    readonly Font _font;
    public CounterView(Font f) { _font = f; }

    public override void Measure(int aw, int ah)
    {
        DesiredW = Width >= 0 ? Width : aw;
        DesiredH = Height >= 0 ? Height : 48;
    }
    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF10160F), X, Y, W, H, 8);
        g.DrawRoundedRectangle(new Color(0xFF2C3A30), X, Y, W, H, 8);
        char[] buf = new char[12];
        int n = UiText.Int(Value, buf);
        int tw = n * _font.CharW;
        g.DrawChars(_font, buf, n, new Color(0xFF5FD46E), X + (W - tw) / 2, Y + (H - _font.CharH) / 2);
    }
}
