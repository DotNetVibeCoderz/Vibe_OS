// WebView (v0.9 "Serbuk") — a minimal web-app runtime. A deliberately tiny
// HTML-subset renderer (h1/h2, p, li, hr, button) drawn with Buitenzorg.Drawing.
// Web app variant (requirements.md §11.3): the "web app" is the markup below,
// rendered in a WebView window.
//
// NOTE: intentionally minimal — a full HTML/CSS/JS engine is later roadmap work.

using System;
using Buitenzorg.Drawing;

unsafe class WebView
{
    // The "web app" content (a tiny HTML document).
    const string Html =
        "<h1>Buitenzorg Web</h1>" +
        "<p>Halo dari WebView.</p>" +
        "<hr>" +
        "<h2>Fitur</h2>" +
        "<li>Render subset HTML</li>" +
        "<li>Digambar via Buitenzorg.Drawing</li>" +
        "<li>Varian app: web</li>" +
        "<hr>" +
        "<button>Mulai</button>";

    static void Main()
    {
        Console.WriteLine("[webview] starting (mini web-app runtime)");
        var g = Graphics.CreateWindow("WebView - web app", 420, 320);
        Render(g);
        Console.WriteLine("[webview] rendered HTML document");
        Sys.Sleep(27);
    }

    static void Render(Graphics g)
    {
        g.Clear(Color.FromRgb(0xF6, 0xFA, 0xF0)); // light "page"
        var ink = new Color(0x1C2A18);
        var muted = new Color(0x4A5A44);

        int y = 12;
        int i = 0;
        char* buf = stackalloc char[256];

        while (i < Html.Length)
        {
            if (Html[i] != '<') { i++; continue; }
            int close = IndexOf(Html, '>', i);
            if (close < 0) break;
            // tag name
            char* tag = stackalloc char[16];
            int tn = 0;
            for (int k = i + 1; k < close && tn < 15 && Html[k] != ' '; k++) tag[tn++] = Html[k];

            // text until next '<'
            int textStart = close + 1;
            int textEnd = IndexOf(Html, '<', textStart);
            if (textEnd < 0) textEnd = Html.Length;
            int tl = 0;
            for (int k = textStart; k < textEnd && tl < 255; k++) buf[tl++] = Html[k];

            if (TagIs(tag, tn, "h1")) { g.DrawChars(buf, tl, ink, 16, y); y += 26; }
            else if (TagIs(tag, tn, "h2")) { g.DrawChars(buf, tl, ink, 16, y); y += 22; }
            else if (TagIs(tag, tn, "p")) { g.DrawChars(buf, tl, muted, 16, y); y += 22; }
            else if (TagIs(tag, tn, "li")) { g.DrawString(" - ", muted, 16, y); g.DrawChars(buf, tl, ink, 40, y); y += 20; }
            else if (TagIs(tag, tn, "hr")) { g.DrawLine(new Pen(muted), 16, y + 4, 404, y + 4); y += 14; }
            else if (TagIs(tag, tn, "button"))
            {
                g.FillRectangle(new Brush(Color.Green), 16, y, 100, 26);
                g.DrawChars(buf, tl, new Color(0x0B120B), 40, y + 5);
                y += 34;
            }

            i = textEnd;
        }
        g.DrawString("(mini WebView: subset HTML)", muted, 16, 296);
        g.Present();
    }

    static int IndexOf(string s, char c, int from)
    {
        for (int i = from; i < s.Length; i++) if (s[i] == c) return i;
        return -1;
    }

    static bool TagIs(char* tag, int tn, string name)
    {
        if (tn != name.Length) return false;
        for (int i = 0; i < tn; i++) if (tag[i] != name[i]) return false;
        return true;
    }
}
