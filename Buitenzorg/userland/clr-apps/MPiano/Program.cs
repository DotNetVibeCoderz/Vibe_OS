// Piano -- one octave (C4 to C5) of white and black keys: click a key or type
// A W S E D F T G Y H U J K to play it through the sound card
// (Buitenzorg.App.Audio: a PC's AC'97, a Raspberry Pi 4's headphone jack).
// Buitenzorg.UI.

using Buitenzorg.UI;
using Audio = Buitenzorg.App.Audio;

namespace MPiano;

public sealed class Keyboard : Control
{
    // C D E F G A B C (white) and C# D# F# G# A# (black, after white key i)
    public static readonly (string Name, int Hz, char Key)[] White =
    {
        ("C", 262, 'a'), ("D", 294, 's'), ("E", 330, 'd'), ("F", 349, 'f'), ("G", 392, 'g'), ("A", 440, 'h'), ("B", 494, 'j'), ("C", 523, 'k'),
    };
    public static readonly (int After, int Hz, char Key)[] Black =
    {
        (0, 277, 'w'), (1, 311, 'e'), (3, 370, 't'), (4, 415, 'y'), (5, 466, 'u'),
    };

    public int Pressed = -1; // index: 0..7 white, 8.. black

    public Keyboard() { Focusable = true; }
    public readonly List<int> Played = new();

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(320, a.Width), Math.Min(220, Math.Max(150, a.Height)));

    Rect WhiteRect(int i) { var b = Bounds; int kw = b.Width / White.Length; return new Rect(b.X + i * kw, b.Y, kw - 3, b.Height); }
    Rect BlackRect(int j) { var b = Bounds; int kw = b.Width / White.Length; var w = WhiteRect(Black[j].After); return new Rect(w.X + kw * 2 / 3, b.Y, kw * 2 / 3, b.Height * 3 / 5); }

    public int KeyAt(int x, int y)
    {
        for (int j = 0; j < Black.Length; j++) if (BlackRect(j).Contains(x, y)) return 8 + j;
        for (int i = 0; i < White.Length; i++) if (WhiteRect(i).Contains(x, y)) return i;
        return -1;
    }

    public void Play(int k)
    {
        if (k < 0) return;
        Pressed = k;
        InvalidateVisual();
        Played.Add(k);
        int hz = k < 8 ? White[k].Hz : Black[k - 8].Hz;
        Audio.Beep(hz, 220);
    }

    public override void Render(DrawingContext dc)
    {
        for (int i = 0; i < White.Length; i++)
        {
            var r = WhiteRect(i);
            dc.FillRoundRect(r, 6, Pressed == i ? 0xD8EFE4 : 0xFAFAF7);
            dc.StrokeRoundRect(r, 6, Theme.Stroke);
            dc.Text(r.X + r.Width / 2 - 4, r.Y + r.Height - 40, White[i].Name, 0x445048);
            dc.Text(r.X + r.Width / 2 - 4, r.Y + r.Height - 22, White[i].Key.ToString().ToUpper(), 0x9AA39D);
        }
        for (int j = 0; j < Black.Length; j++)
        {
            var r = BlackRect(j);
            dc.FillRoundRect(r, 5, Pressed == 8 + j ? 0x2E7D5B : 0x1A1F1C);
            dc.Text(r.X + r.Width / 2 - 4, r.Y + r.Height - 20, Black[j].Key.ToString().ToUpper(), 0xBFC6C1);
        }
    }

    protected override void OnPointerPressed(PointerEventArgs e) { Focus(); Play(KeyAt(e.X, e.Y)); e.Handled = true; }
    protected override void OnPointerReleased(PointerEventArgs e) { Pressed = -1; InvalidateVisual(); base.OnPointerReleased(e); }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        char k = char.ToLower(e.Key);
        for (int i = 0; i < White.Length; i++) if (White[i].Key == k) { Play(i); e.Handled = true; }
        for (int j = 0; j < Black.Length; j++) if (Black[j].Key == k) { Play(8 + j); e.Handled = true; }
        base.OnKeyDown(e);
    }
}

public static class PianoApp
{
    public static (Window, Keyboard) Build(bool headless = false)
    {
        var w = new Window("Piano", 640, 380, headless);
        var keys = new Keyboard();
        string status = Audio.IsPresent ? "Klik tuts atau ketik A S D F G H J K (hitam: W E T Y U)." : "Tidak ada kartu suara: tuts tetap bisa ditekan, tapi tanpa bunyi.";
        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Piano", IconKind.Play, 0xC792EA).DockTo(Dock.Top));
        root.Children.Add(new TextBlock(status) { Margin = new Thickness(0, 0, 0, 10) }.WithClass("faint").DockTo(Dock.Top));
        root.Children.Add(keys);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        keys.Focus();
        return (w, keys);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Piano");
        var (w, keys) = Build(headless: true);
        var b = keys.Bounds;
        int kw = b.Width / Keyboard.White.Length;
        c.That(keys.KeyAt(b.X + kw / 2, b.Y + b.Height - 10) == 0, "bottom of the first key is C");
        c.That(keys.KeyAt(b.X + kw, b.Y + 10) == 8, "between C and D at the top is C#");
        w.SimulateKeys("asdk");
        c.That(keys.Played.Count == 4 && keys.Played[0] == 0 && keys.Played[3] == 7, "typed A S D K play C D E C");
        w.SimulateKeys("w");
        c.That(keys.Played.Count == 5 && keys.Played[4] == 8, "W plays C#");
        w.RenderFrame();
        c.That(w.LastFrameOps > 20, "frame drew " + w.LastFrameOps + " ops");
        return c;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = PianoApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = PianoApp.Build();
        app.Run(w);
        return 0;
    }
}
