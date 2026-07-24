// Buitenzorg OS — v0.16 "Panen": audio-settings panel (ring-3 C#).
//
// A real settings window built on Buitenzorg.UI, wired to the OS audio
// subsystem via Buitenzorg.Audio: a master-volume slider, a mute checkbox, and
// a "test tone" button, plus the device info. Simulated mouse events drive the
// controls, each control applies to the live Mixer, and the resulting device
// state is verified -> MILESTONE: AUDIO PANEL OK. Built with bflat --stdlib:zero
// together with bzui.cs, bzgfx.cs, bzaudio.cs.

using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;
using Buitenzorg.Audio;

class AudioPanel
{
    static void Main()
    {
        Console.WriteLine("AudioPanel: panel pengaturan audio (Buitenzorg.UI + Audio)...");

        AudioInfo info = Mixer.GetInfo();
        if (!info.Present)
        {
            Console.WriteLine("AudioPanel: tidak ada perangkat audio");
            return;
        }

        Font font = Font.Default();
        const int W = 340, H = 250;
        UIHost host = new UIHost("Pengaturan Audio", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 14; root.Spacing = 10;
        root.Background = new Color(0xFF1C2028);

        TextBlock title = new TextBlock("PENGATURAN AUDIO", font);
        title.Foreground = Color.White;

        // Device info (format is fixed: AC'97 = 48 kHz stereo 16-bit).
        TextBlock dev = new TextBlock("PERANGKAT: AC97  48KHZ STEREO 16-BIT", font);
        dev.Foreground = new Color(0xFF9AA6B4);

        TextBlock volLabel = new TextBlock("VOLUME MASTER", font);
        Slider volume = new Slider();
        volume.Width = 300;
        volume.Value = Mixer.GetVolume();

        CheckBox mute = new CheckBox("BISUKAN (MUTE)", font);

        Button test = new Button("TES NADA", font);
        test.Width = 120; test.Height = 28;

        ProgressBar level = new ProgressBar();
        level.Width = 300; level.Height = 12;
        level.Value = volume.Value;

        root.Add(title); root.Add(dev); root.Add(volLabel);
        root.Add(volume); root.Add(mute); root.Add(test); root.Add(level);
        host.Root = root;
        host.Layout();

        bool ok = true;

        // 1) Drag the volume slider to ~35% and apply it to the mixer.
        host.Mouse(volume.X + volume.W * 35 / 100, volume.Y + volume.H / 2, true);
        host.Mouse(volume.X + volume.W * 35 / 100, volume.Y + volume.H / 2, false);
        Mixer.SetVolume(volume.Value);
        level.Value = volume.Value;
        int applied = Mixer.GetVolume();
        Line("AudioPanel: slider ", volume.Value, " -> device volume ", applied);
        if (applied < 30 || applied > 40) ok = false;

        // 2) Toggle mute -> volume 0 on the device.
        host.Mouse(mute.X + 7, mute.Y + mute.H / 2, true);
        host.Mouse(mute.X + 7, mute.Y + mute.H / 2, false);
        if (mute.Checked) { Mixer.Mute(); level.Value = 0; }
        int muted = Mixer.GetVolume();
        Line("AudioPanel: mute checked=", mute.Checked ? 1 : 0, " -> device volume ", muted);
        if (!(mute.Checked && muted == 0)) ok = false;

        // 3) Un-mute by dragging the slider back up, then hit "test tone".
        host.Mouse(volume.X + volume.W * 70 / 100, volume.Y + volume.H / 2, true);
        host.Mouse(volume.X + volume.W * 70 / 100, volume.Y + volume.H / 2, false);
        Mixer.SetVolume(volume.Value);
        level.Value = volume.Value;
        host.Mouse(test.X + test.W / 2, test.Y + test.H / 2, true);
        host.Mouse(test.X + test.W / 2, test.Y + test.H / 2, false);
        if (test.Clicks == 1) { if (!Mixer.Beep(660, 100)) ok = false; }
        else ok = false;

        host.Render(new Color(0xFF141820));
        host.Present();

        if (ok)
            Console.WriteLine("MILESTONE: AUDIO PANEL OK");
        else
            Console.WriteLine("AudioPanel: verifikasi gagal (kontrol/mixer)");
    }

    // Print "<a><n1><b><n2>\n" without allocating a managed string.
    static unsafe void Line(string a, int n1, string b, int n2)
    {
        byte* buf = stackalloc byte[128];
        int i = 0;
        i = Put(buf, i, a);
        i = PutNum(buf, i, n1);
        i = Put(buf, i, b);
        i = PutNum(buf, i, n2);
        buf[i++] = (byte)'\n';
        Write(buf, i);
    }

    static unsafe int Put(byte* buf, int i, string s)
    {
        for (int k = 0; k < s.Length && i < 120; k++) buf[i++] = (byte)s[k];
        return i;
    }

    static unsafe int PutNum(byte* buf, int i, int v)
    {
        if (v < 0) { buf[i++] = (byte)'-'; v = -v; }
        if (v == 0) { buf[i++] = (byte)'0'; return i; }
        byte* d = stackalloc byte[12];
        int j = 0;
        while (v > 0 && j < 12) { d[j++] = (byte)('0' + (v % 10)); v /= 10; }
        while (j > 0) buf[i++] = d[--j];
        return i;
    }

    [System.Runtime.InteropServices.DllImport("*")]
    static extern unsafe void bz_write(byte* buf, ulong len);
    static unsafe void Write(byte* buf, int n) => bz_write(buf, (ulong)n);
}
