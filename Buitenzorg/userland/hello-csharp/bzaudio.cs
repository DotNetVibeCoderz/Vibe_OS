// Buitenzorg.Audio — a small managed audio library over the OS audio syscalls
// (v0.16 "Panen"). Wraps AUDIO_STAT / AUDIO_SET_VOLUME / AUDIO_TONE / AUDIO_PLAY
// exposed by the kernel AC'97 driver: query device info, control master volume
// and mute, play a generated tone, or stream 16-bit stereo PCM at 48 kHz.
//
// Freestanding (bflat --stdlib:zero): no BCL. Values come back through a small
// stack buffer; PCM is generated into a heap array and handed to the kernel.

using System;
using System.Runtime.InteropServices;

namespace Buitenzorg.Audio
{
    /// <summary>Device status snapshot returned by <see cref="Mixer.GetInfo"/>.</summary>
    public struct AudioInfo
    {
        public bool Present;
        public int SampleRate;
        public int Channels;
        public int Bits;
        public int Volume; // 0..100
        public bool Muted;
    }

    /// <summary>Master mixer + playback control over the OS audio syscalls.</summary>
    public static unsafe class Mixer
    {
        [DllImport("*")] static extern ulong bz_audio_stat(byte* outp);
        [DllImport("*")] static extern ulong bz_audio_set_volume(ulong pct);
        [DllImport("*")] static extern ulong bz_audio_tone(ulong freq, ulong ms);
        [DllImport("*")] static extern ulong bz_audio_play(byte* ptr, ulong len);

        public const int SampleRate = 48000;

        /// <summary>Read the current audio-device status.</summary>
        public static AudioInfo GetInfo()
        {
            // AudioInfo mirror: 6 x u64 = 48 bytes.
            byte* raw = stackalloc byte[48];
            AudioInfo info = default;
            if (bz_audio_stat(raw) != 0) return info;
            ulong* q = (ulong*)raw;
            info.Present = q[0] != 0;
            info.SampleRate = (int)q[1];
            info.Channels = (int)q[2];
            info.Bits = (int)q[3];
            info.Volume = (int)q[4];
            info.Muted = q[5] != 0;
            return info;
        }

        /// <summary>Is a sound card present?</summary>
        public static bool IsPresent() => GetInfo().Present;

        /// <summary>Set the master volume (0..100). Non-zero also un-mutes.</summary>
        public static bool SetVolume(int pct)
        {
            if (pct < 0) pct = 0; if (pct > 100) pct = 100;
            return bz_audio_set_volume((ulong)pct) == 0;
        }

        /// <summary>Current master volume (0..100).</summary>
        public static int GetVolume() => GetInfo().Volume;

        /// <summary>Mute by setting volume to 0.</summary>
        public static bool Mute() => bz_audio_set_volume(0) == 0;

        /// <summary>Play a generated sine tone (freq Hz) for the given duration.</summary>
        public static bool Beep(int freq, int ms) => bz_audio_tone((ulong)freq, (ulong)ms) == 0;

        /// <summary>Play interleaved 16-bit stereo PCM (L,R,L,R,...) at 48 kHz.</summary>
        public static bool Play(short[] pcm, int sampleCount)
        {
            if (pcm == null || sampleCount <= 0) return false;
            fixed (short* p = pcm)
            {
                return bz_audio_play((byte*)p, (ulong)(sampleCount * 2)) == 0;
            }
        }
    }

    /// <summary>Generates PCM waveforms into a caller buffer (48 kHz stereo).</summary>
    public static class Tone
    {
        /// <summary>Fill `frames` stereo pairs of a square wave into `buf`
        /// (length must be >= frames*2). Returns the number of shorts written.</summary>
        public static int Square(short[] buf, int frames, int freq, short amp)
        {
            int period = Mixer.SampleRate / (freq > 0 ? freq : 1);
            int half = period / 2; if (half < 1) half = 1;
            for (int n = 0; n < frames; n++)
            {
                short s = ((n % period) < half) ? amp : (short)(-amp);
                buf[n * 2] = s;
                buf[n * 2 + 1] = s;
            }
            return frames * 2;
        }
    }
}
