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
    public const int ThemeKey = 0, WallpaperKey = 1, ScreensaverKey = 2, ScreensaverDelayKey = 3, AnimationsKey = 4, LargeCursorKey = 5,
        TimeZoneKey = 6, LanguageKey = 7, Clock24HourKey = 8, ClockKey = 9;

    static bool _sim;
    static readonly int[] SimValues = { 10, 0, 0, 12, 1, 0, 0, 0, 1, 0 };
    static long _simAdjust;
    static readonly string[][] SimNames =
    {
        new[] { "dark", "light", "neo-brutalism", "clean", "material", "bento", "classic-linux", "classic-windows", "sun", "beos", "kebun" },
        new[] { "nature", "futuristic", "city", "art", "surreal", "imaginary", "space", "minimal" },
        new[] { "starfield", "mystify", "pipes", "marquee", "bouncing", "blank", "off" },
        Array.Empty<string>(), Array.Empty<string>(), Array.Empty<string>(),
        new[] { "WIB", "WITA", "WIT", "UTC", "SGT", "ICT", "JST", "AEST", "IST", "GST", "CET", "EST" },
        new[] { "Bahasa Indonesia", "English" },
    };
    static readonly int[] SimZoneMinutes = { 420, 480, 540, 0, 480, 420, 540, 600, 330, 240, 60, -300 };

    static bool Fallback(Exception e) { if (e is DllNotFoundException || e is EntryPointNotFoundException) { _sim = true; return true; } return false; }

    /// <summary>True off the device, against the in-memory simulation.</summary>
    public static bool IsSimulated { get { _ = Get(ThemeKey); return _sim; } }

    public static int Get(int key)
    {
        if (_sim && key == ClockKey)
            return (int)(DateTimeOffset.UtcNow.ToUnixTimeSeconds() + SimZoneMinutes[SimValues[TimeZoneKey]] * 60L + _simAdjust);
        if (_sim) return key >= 0 && key < SimValues.Length ? SimValues[key] : -1;
        try { return SettingsNative.SettingGet(key); }
        catch (Exception e) when (Fallback(e)) { return Get(key); }
    }

    public static bool Set(int key, int value)
    {
        if (_sim)
        {
            if (key < 0 || key >= SimValues.Length) return false;
            if (key == ClockKey)
            {
                _simAdjust = value - (DateTimeOffset.UtcNow.ToUnixTimeSeconds() + SimZoneMinutes[SimValues[TimeZoneKey]] * 60L);
                return true;
            }
            if (key < SimNames.Length && SimNames[key].Length > 0 && (value < 0 || value >= SimNames[key].Length)) return false;
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

/// <summary>Date, time and region: the time zone, the interface language, 12/24-hour clock,
/// and setting the clock (the "Tanggal & Waktu" app; the tray clock follows).</summary>
public static class Region
{
    /// <summary>Short zone names ("WIB", ...).</summary>
    public static List<string> TimeZones => Settings.Choices(Settings.TimeZoneKey);
    /// <summary>Places + UTC offset of each zone.</summary>
    public static List<string> TimeZoneDescriptions => Settings.Choices(Settings.TimeZoneKey, descriptions: true);
    public static List<string> Languages => Settings.Choices(Settings.LanguageKey);

    public static int TimeZone { get => Settings.Get(Settings.TimeZoneKey); set => Settings.Set(Settings.TimeZoneKey, value); }
    public static int Language { get => Settings.Get(Settings.LanguageKey); set => Settings.Set(Settings.LanguageKey, value); }
    public static bool Use24Hour { get => Settings.Get(Settings.Clock24HourKey) != 0; set => Settings.Set(Settings.Clock24HourKey, value ? 1 : 0); }

    /// <summary>The local date and time (the RTC + time zone + correction).</summary>
    public static DateTime LocalTime
    {
        get => DateTime.UnixEpoch.AddSeconds((uint)Settings.Get(Settings.ClockKey));
        set => Settings.Set(Settings.ClockKey, (int)(uint)(value - DateTime.UnixEpoch).TotalSeconds);
    }

    /// <summary>"16:05" or "4:05 PM", following the 12/24-hour setting.</summary>
    public static string FormatTime(DateTime t, bool seconds = false)
    {
        string s = seconds ? ":" + t.Second.ToString("00") : "";
        if (Use24Hour) return t.Hour.ToString("00") + ":" + t.Minute.ToString("00") + s;
        int h = t.Hour % 12 == 0 ? 12 : t.Hour % 12;
        return h + ":" + t.Minute.ToString("00") + s + (t.Hour < 12 ? " AM" : " PM");
    }

    static readonly string[] DaysId = { "Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu" };
    static readonly string[] DaysEn = { "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday" };
    static readonly string[] MonthsId = { "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember" };
    static readonly string[] MonthsEn = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" };

    /// <summary>"Selasa, 3 November 2026" (or the English form).</summary>
    public static string FormatDate(DateTime t)
    {
        bool en = Language == 1;
        int wd = (int)t.DayOfWeek;
        return en ? DaysEn[wd] + ", " + MonthsEn[t.Month - 1] + " " + t.Day + ", " + t.Year
                  : DaysId[wd] + ", " + t.Day + " " + MonthsId[t.Month - 1] + " " + t.Year;
    }
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
