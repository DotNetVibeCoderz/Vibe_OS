// Buitenzorg.App -- desktop settings and sound for managed apps: theme,
// wallpaper, screensaver (+ its delay), animations, cursor size (the SETTING
// syscall), and the sound card (AUDIO_* syscalls). Off the device (dotnet on a
// PC) both answer from an in-memory simulation, so a Settings app runs anywhere.

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace Buitenzorg.App;

internal static class SettingsNative
{
    [DllImport("buitenzorg")] internal static extern int SettingGet(int key);
    [DllImport("buitenzorg")] internal static extern int SettingSet(int key, int value);
    [DllImport("buitenzorg")] internal static extern int SettingCount(int key);
    [DllImport("buitenzorg")] internal static extern string SettingName(int key, int index, int description);
    [DllImport("buitenzorg")] internal static extern long AudioStat(int field);
    [DllImport("buitenzorg")] internal static extern int AudioSetVolume(int percent);
    [DllImport("buitenzorg")] internal static extern int AudioTone(int hz, int ms);
}

/// <summary>The desktop's settings (what the shell's theme / bg / saver / anim / cursor change).</summary>
public static class Settings
{
    public const int ThemeKey = 0, WallpaperKey = 1, ScreensaverKey = 2, ScreensaverDelayKey = 3, AnimationsKey = 4, LargeCursorKey = 5;

    static bool _sim;
    static readonly int[] SimValues = { 10, 0, 0, 12, 1, 0 };
    static readonly string[][] SimNames =
    {
        new[] { "dark", "light", "neo-brutalism", "clean", "material", "bento", "classic-linux", "classic-windows", "sun", "beos", "kebun" },
        new[] { "nature", "futuristic", "city", "art", "surreal", "imaginary", "space", "minimal" },
        new[] { "starfield", "mystify", "pipes", "marquee", "bouncing", "blank", "off" },
    };

    static bool Fallback(Exception e) { if (e is DllNotFoundException || e is EntryPointNotFoundException) { _sim = true; return true; } return false; }

    /// <summary>True off the device, against the in-memory simulation.</summary>
    public static bool IsSimulated { get { _ = Get(ThemeKey); return _sim; } }

    public static int Get(int key)
    {
        if (_sim) return key >= 0 && key < SimValues.Length ? SimValues[key] : -1;
        try { return SettingsNative.SettingGet(key); }
        catch (Exception e) when (Fallback(e)) { return Get(key); }
    }

    public static bool Set(int key, int value)
    {
        if (_sim)
        {
            if (key < 0 || key >= SimValues.Length) return false;
            if (key < 3 && (value < 0 || value >= SimNames[key].Length)) return false;
            SimValues[key] = value;
            return true;
        }
        try { return SettingsNative.SettingSet(key, value) == 0; }
        catch (Exception e) when (Fallback(e)) { return Set(key, value); }
    }

    /// <summary>The choices of a list setting (themes, wallpapers, screensavers + "off").</summary>
    public static List<string> Choices(int key, bool descriptions = false)
    {
        var list = new List<string>();
        if (!_sim)
        {
            try
            {
                int n = SettingsNative.SettingCount(key);
                for (int i = 0; i < n; i++) list.Add(SettingsNative.SettingName(key, i, descriptions ? 1 : 0));
                return list;
            }
            catch (Exception e) when (Fallback(e)) { }
        }
        if (key >= 0 && key < SimNames.Length)
            foreach (var s in SimNames[key]) list.Add(descriptions ? "(" + s + ")" : s);
        return list;
    }

    public static int Theme { get => Get(ThemeKey); set => Set(ThemeKey, value); }
    public static int Wallpaper { get => Get(WallpaperKey); set => Set(WallpaperKey, value); }
    public static int Screensaver { get => Get(ScreensaverKey); set => Set(ScreensaverKey, value); }
    public static int ScreensaverDelaySeconds { get => Get(ScreensaverDelayKey); set => Set(ScreensaverDelayKey, value); }
    public static bool Animations { get => Get(AnimationsKey) == 1; set => Set(AnimationsKey, value ? 1 : 0); }
    public static bool LargeCursor { get => Get(LargeCursorKey) == 1; set => Set(LargeCursorKey, value ? 1 : 0); }
}

/// <summary>The sound card: volume, mute, a test tone.</summary>
public static class Audio
{
    static bool _sim;
    static int _simVolume = 70;
    static bool Fallback(Exception e) { if (e is DllNotFoundException || e is EntryPointNotFoundException) { _sim = true; return true; } return false; }

    static long Stat(int field)
    {
        if (_sim) return field switch { 0 => 1, 1 => 48000, 2 => 2, 3 => 16, 4 => _simVolume, 5 => _simVolume == 0 ? 1 : 0, _ => -1 };
        try { return SettingsNative.AudioStat(field); }
        catch (Exception e) when (Fallback(e)) { return Stat(field); }
    }

    /// <summary>Is a sound card present (a PC's AC'97; none on a Raspberry Pi yet)?</summary>
    public static bool IsPresent => Stat(0) == 1;
    public static int SampleRate => (int)Stat(1);
    /// <summary>Master volume, 0-100.</summary>
    public static int Volume
    {
        get => (int)Math.Max(0, Stat(4));
        set
        {
            int v = Math.Clamp(value, 0, 100);
            if (_sim) { _simVolume = v; return; }
            try { SettingsNative.AudioSetVolume(v); }
            catch (Exception e) when (Fallback(e)) { _simVolume = v; }
        }
    }
    public static bool IsMuted => Stat(5) == 1;
    /// <summary>Play a sine tone (blocks for its length).</summary>
    public static bool Beep(int hz = 880, int ms = 200)
    {
        if (_sim) return true;
        try { return SettingsNative.AudioTone(hz, ms) == 0; }
        catch (Exception e) when (Fallback(e)) { return true; }
    }
}
