// Buitenzorg OS — v0.16 "Panen": audio subsystem demo (ring-3 C#).
//
// Drives the OS audio syscalls through Buitenzorg.Audio: queries the AC'97
// device, sets + reads back the master volume (mixer round-trip), plays a
// generated tone, and streams a short square-wave PCM buffer. Verifies the
// deterministic parts (presence, format, volume round-trip) and prints
// MILESTONE: AUDIO OK. Built with bflat --stdlib:zero together with bzaudio.cs.

using System;
using System.Runtime.InteropServices;
using Buitenzorg.Audio;

class AudioDemo
{
    [DllImport("*")] static extern unsafe void bz_write(byte* buf, ulong len);

    static void Main()
    {
        Console.WriteLine("Audio: menguji Buitenzorg.Audio (AC'97 + mixer + PCM)...");

        AudioInfo info = Mixer.GetInfo();
        Line("Audio: present=", info.Present ? 1 : 0);
        Line("Audio: sample_rate=", info.SampleRate);
        Line("Audio: channels=", info.Channels);
        Line("Audio: bits=", info.Bits);
        Line("Audio: volume=", info.Volume);

        bool ok = info.Present
                  && info.SampleRate == 48000
                  && info.Channels == 2
                  && info.Bits == 16;

        // Mixer round-trip: set the volume and read it back.
        Mixer.SetVolume(45);
        int v = Mixer.GetVolume();
        Line("Audio: set 45 -> readback=", v);
        if (v != 45) ok = false;

        // Play a generated tone (C5, 523 Hz) through the DMA engine.
        if (!Mixer.Beep(523, 120)) ok = false;

        // Stream a short square wave (E5, 659 Hz) as raw PCM.
        int frames = 4800; // 0.1 s at 48 kHz
        short[] pcm = new short[frames * 2];
        int n = Tone.Square(pcm, frames, 659, 6000);
        if (!Mixer.Play(pcm, frames)) ok = false;
        Line("Audio: streamed PCM shorts=", n);

        // Restore a comfortable volume.
        Mixer.SetVolume(75);

        if (ok)
            Console.WriteLine("MILESTONE: AUDIO OK");
        else
            Console.WriteLine("Audio: verifikasi gagal (device/format/volume)");
    }

    // Print "<label><number>\n" without allocating a managed string.
    static unsafe void Line(string label, int value)
    {
        byte* buf = stackalloc byte[96];
        int i = 0;
        for (int k = 0; k < label.Length && i < 80; k++) buf[i++] = (byte)label[k];
        // decimal digits
        if (value < 0) { buf[i++] = (byte)'-'; value = -value; }
        if (value == 0) { buf[i++] = (byte)'0'; }
        else
        {
            byte* d = stackalloc byte[12];
            int j = 0;
            while (value > 0 && j < 12) { d[j++] = (byte)('0' + (value % 10)); value /= 10; }
            while (j > 0) buf[i++] = d[--j];
        }
        buf[i++] = (byte)'\n';
        bz_write(buf, (ulong)i);
    }
}
