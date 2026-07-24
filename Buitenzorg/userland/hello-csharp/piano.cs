// Buitenzorg OS — v0.16 "Panen" preloaded suite: Piano (music).
//
// A one-octave on-screen piano built on Buitenzorg.UI + Drawing + Audio: white
// and black keys drawn as rounded rectangles; clicking a white key plays its
// note through Buitenzorg.Audio (AUDIO_TONE). The demo simulates pressing a key,
// verifies the note played, renders the window, and prints MILESTONE: PIANO OK.
// Built with bflat --stdlib:zero together with bzui.cs, bzgfx.cs, bzaudio.cs.

using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;
using Buitenzorg.Audio;

sealed class Piano : UIElement
{
    Font _font;
    public int LastNote = -1;
    public int Played;
    // Note frequencies (C4..C5) — instance arrays (static ref fields fault under zerolib).
    readonly int[] _wf = new int[] { 262, 294, 330, 349, 392, 440, 494, 523 };
    readonly char[] _wn = new char[] { 'C', 'D', 'E', 'F', 'G', 'A', 'B', 'C' };
    // Black keys sit after white indices 0,1,3,4,5 (no black after E or B).
    readonly int[] _bAfter = new int[] { 0, 1, 3, 4, 5 };
    readonly int[] _bf = new int[] { 277, 311, 370, 415, 466 };
    public Piano(Font f) { _font = f; }
    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 336; DesiredH = Height >= 0 ? Height : 150; }

    int KeyW => W / 8;

    public override void Render(Graphics g)
    {
        if (!Visible) return;
        int kw = KeyW;
        for (int i = 0; i < 8; i++)
        {
            int kx = X + i * kw;
            Color c = (i == LastNote) ? new Color(0xFF9AC0FF) : new Color(0xFFE8ECF0);
            g.FillRoundedRectangle(c, kx + 1, Y, kw - 2, H, 4);
            g.DrawRoundedRectangle(new Color(0xFF404650), kx + 1, Y, kw - 2, H, 4);
            _label[0] = _wn[i];
            g.DrawChars(_font, _label, 1, new Color(0xFF303840), kx + kw / 2 - _font.CharW / 2, Y + H - 14);
        }
        int bh = H * 60 / 100, bw = kw * 6 / 10;
        for (int j = 0; j < 5; j++)
        {
            int bx = X + (_bAfter[j] + 1) * kw - bw / 2;
            g.FillRoundedRectangle(new Color(0xFF14181E), bx, Y, bw, bh, 3);
            g.DrawRoundedRectangle(new Color(0xFF000000), bx, Y, bw, bh, 3);
        }
    }
    readonly char[] _label = new char[1];

    public override void MouseDown(int mx, int my)
    {
        // Black keys are on top of the upper part of the keyboard.
        int kw = KeyW, bh = H * 60 / 100, bw = kw * 6 / 10;
        if (my - Y < bh)
        {
            for (int j = 0; j < 5; j++)
            {
                int bx = X + (_bAfter[j] + 1) * kw - bw / 2;
                if (mx >= bx && mx < bx + bw) { Play(100 + j, _bf[j]); return; }
            }
        }
        int idx = (mx - X) / kw;
        if (idx >= 0 && idx < 8) Play(idx, _wf[idx]);
    }
    void Play(int note, int freq)
    {
        LastNote = note < 100 ? note : LastNote; // highlight white keys
        Played++;
        Mixer.Beep(freq, 200);
    }
}

class PianoApp
{
    static void Main()
    {
        Console.WriteLine("Piano: music (Buitenzorg.UI + Audio)...");
        Font font = Font.Default();
        const int W = 360, H = 220;
        UIHost host = new UIHost("Piano", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);
        TextBlock title = new TextBlock("PIANO", font);
        title.Foreground = Color.White;

        Piano piano = new Piano(font);
        piano.Width = 336; piano.Height = 150;

        root.Add(title);
        root.Add(piano);
        host.Root = root;
        host.Layout();

        // Play a short arpeggio (C E G C) through the audio subsystem.
        Mixer.SetVolume(70);
        int[] seq = new int[] { 0, 2, 4, 7 };
        for (int i = 0; i < seq.Length; i++)
        {
            int kx = piano.X + seq[i] * (piano.W / 8) + (piano.W / 16);
            host.Mouse(kx, piano.Y + piano.H - 10, true);
            host.Mouse(kx, piano.Y + piano.H - 10, false);
        }

        host.Render(new Color(0xFF141820));
        host.Present();

        bool ok = piano.Played == 4 && piano.LastNote == 7 && Mixer.IsPresent();
        if (ok)
            Console.WriteLine("MILESTONE: PIANO OK");
        else
            Console.WriteLine("Piano: verifikasi gagal (nada/audio)");
    }
}
