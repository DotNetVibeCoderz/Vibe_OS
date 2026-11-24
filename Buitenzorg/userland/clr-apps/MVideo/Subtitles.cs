using System;
using System.Collections.Generic;
using System.Text;

namespace MVideo;

/// <summary>One subtitle: shown from Start to End (seconds).</summary>
public sealed class Cue
{
    public double Start, End;
    public readonly List<string> Lines = new List<string>();
    public string Text => string.Join(" / ", Lines);
}

/// <summary>SubRip (.SRT) and WebVTT (.VTT) subtitles. Formatting tags
/// (&lt;i&gt;, &lt;b&gt;, &lt;font&gt;, {\an8}) are removed; positions are ignored.
/// (Written for the device's runtime: char loops, no string.Split(string).)</summary>
public sealed class Subtitles
{
    public readonly List<Cue> Cues = new List<Cue>();

    public static Subtitles Parse(string text)
    {
        var s = new Subtitles();
        var lines = text.Replace("\r", "").Split('\n');
        Cue? cur = null;
        foreach (var raw in lines)
        {
            string line = raw.Trim();
            if (line.Length > 0 && line[0] == '﻿') line = line.Substring(1);
            int arrow = line.IndexOf("-->");
            if (arrow > 0)
            {
                double a = Time(line.Substring(0, arrow)), b = Time(line.Substring(arrow + 3));
                if (a >= 0 && b > a)
                {
                    cur = new Cue { Start = a, End = b };
                    s.Cues.Add(cur);
                }
                else cur = null;
                continue;
            }
            if (line.Length == 0) { cur = null; continue; }
            if (cur == null) continue; // a cue number, WEBVTT header, NOTE, ...
            string clean = Strip(line);
            if (clean.Length > 0) cur.Lines.Add(clean);
        }
        for (int i = s.Cues.Count - 1; i >= 0; i--) if (s.Cues[i].Lines.Count == 0) s.Cues.RemoveAt(i);
        s.Cues.Sort((x, y) => x.Start < y.Start ? -1 : x.Start > y.Start ? 1 : 0);
        return s;
    }

    /// <summary>"00:01:02,500" / "01:02.500" / "1:02:03.4" (and trailing VTT settings) -> seconds; -1 if not a time.</summary>
    public static double Time(string t)
    {
        t = t.Trim();
        int sp = t.IndexOf(' ');
        if (sp > 0) t = t.Substring(0, sp);
        double total = 0, part = 0, frac = 0, scale = 0;
        bool any = false;
        foreach (char ch in t)
        {
            if (ch >= '0' && ch <= '9')
            {
                any = true;
                if (scale > 0) { frac += (ch - '0') * scale; scale /= 10; }
                else part = part * 10 + (ch - '0');
            }
            else if (ch == ':') { total = total * 60 + part; part = 0; }
            else if (ch == ',' || ch == '.') scale = 0.1;
            else return -1;
        }
        return any ? total * 60 + part + frac : -1;
    }

    static string Strip(string s)
    {
        var o = new StringBuilder();
        int depth = 0;
        char close = ' ';
        foreach (char ch in s)
        {
            if (depth == 0 && (ch == '<' || ch == '{')) { depth = 1; close = ch == '<' ? '>' : '}'; continue; }
            if (depth > 0) { if (ch == close) depth = 0; continue; }
            o.Append(ch);
        }
        return o.ToString().Replace("&amp;", "&").Replace("&lt;", "<").Replace("&gt;", ">").Trim();
    }

    /// <summary>The cue shown at a position, or null.</summary>
    public Cue? At(double seconds)
    {
        int lo = 0, hi = Cues.Count - 1, hit = -1;
        while (lo <= hi)
        {
            int mid = (lo + hi) / 2;
            if (Cues[mid].Start <= seconds) { hit = mid; lo = mid + 1; } else hi = mid - 1;
        }
        // overlapping cues: the latest one that started and has not ended
        for (int i = hit; i >= 0 && i > hit - 4; i--)
            if (Cues[i].End > seconds) return Cues[i];
        return null;
    }
}
