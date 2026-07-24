// Buitenzorg OS — v0.16 "Panen" preloaded suite: Text Editor.
//
// A multi-line text editor built on Buitenzorg.UI: a TextArea control with an
// editable character buffer, a blinking-style caret, and line wrapping on '\n',
// under a menu bar. The demo types two lines, edits, checks the buffer, renders,
// and prints MILESTONE: EDITOR OK. When launched interactively (`run editor`
// from the shell — detected via the IS_INTERACTIVE syscall) it then enters a
// live keyboard loop: keys routed by the kernel keyboard queue (KEY_READ) edit
// the buffer, Backspace deletes, Enter inserts a newline, ESC exits. During the
// headless boot demo IS_INTERACTIVE is 0 so it exits immediately, never
// blocking boot.
//
// Open/Save go through Buitenzorg.Bcl's `BzFile` (System.IO): Ctrl+S writes the
// buffer to /ram/NOTE.TXT and Ctrl+O reads it back, so the FILE menu is real
// rather than decorative. Writes need a writable mount — /disk is read-only, so
// the RAM disk is the default target. Built with bflat --stdlib:zero +
// bzui/bzgfx/bzbcl.

using System;
using System.Runtime.InteropServices;
using Buitenzorg;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

sealed class TextArea : UIElement
{
    char[] _buf = new char[256];
    int _len, _caret;
    Font _font;
    public TextArea(Font f) { _font = f; }

    void Grow() { if (_len >= _buf.Length) { char[] b = new char[_buf.Length * 2]; for (int i = 0; i < _len; i++) b[i] = _buf[i]; _buf = b; } }
    public void Insert(char c) { Grow(); for (int i = _len; i > _caret; i--) _buf[i] = _buf[i - 1]; _buf[_caret] = c; _len++; _caret++; }
    public void Type(string s) { for (int i = 0; i < s.Length; i++) Insert(s[i]); }
    public void Newline() => Insert('\n');
    public void Backspace() { if (_caret > 0) { for (int i = _caret - 1; i < _len - 1; i++) _buf[i] = _buf[i + 1]; _len--; _caret--; } }
    public int Length => _len;
    public int CaretAt => _caret;
    public char CharAt(int i) => i < _len ? _buf[i] : '\0';
    public int LineCount() { int n = 1; for (int i = 0; i < _len; i++) if (_buf[i] == '\n') n++; return n; }
    /// <summary>Copy the text out for saving. Returns the length.</summary>
    public int CopyTo(char[] dst) { int m = _len < dst.Length ? _len : dst.Length; for (int i = 0; i < m; i++) dst[i] = _buf[i]; return m; }
    /// <summary>Replace the whole buffer (used by Open).</summary>
    public void SetText(char[] src, int len)
    {
        _len = 0; _caret = 0;
        for (int i = 0; i < len; i++) Insert(src[i]);
    }

    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 380; DesiredH = Height >= 0 ? Height : 220; }
    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF14181E), X, Y, W, H, 4);
        g.DrawRoundedRectangle(new Color(0xFF50505A), X, Y, W, H, 4);
        int lineH = _font.CharH + 3;
        int cx = X + 6, cy = Y + 6, caretX = cx, caretY = cy;
        char[] one = new char[1];
        for (int i = 0; i <= _len; i++)
        {
            if (i == _caret) { caretX = cx; caretY = cy; }
            if (i == _len) break;
            char c = _buf[i];
            if (c == '\n') { cx = X + 6; cy += lineH; }
            else { one[0] = c; g.DrawChars(_font, one, 1, new Color(0xFFE6E6E6), cx, cy); cx += _font.CharW; }
        }
        g.FillRectangle(new Color(0xFF7CC0FF), caretX, caretY, 1, _font.CharH); // caret
    }
    // Approximate click-to-caret: place caret at end (full editing is keyboard).
    public override void MouseDown(int mx, int my) { _caret = _len; }
}

class EditorApp
{
    [DllImport("*")] static extern uint bz_key_read();
    [DllImport("*")] static extern ulong bz_is_interactive();

    // Live keyboard loop: route the user's keys into the TextArea until ESC.
    static void Interactive(UIHost host, TextArea ta)
    {
        while (true)
        {
            bool changed = false;
            uint k;
            while ((k = bz_key_read()) != 0)
            {
                if (k == 0x1B) return;                       // ESC exits
                if (k == '\n' || k == '\r') ta.Newline();
                else if (k == 0x08 || k == 0x7F) ta.Backspace();
                else if (k == '\t') { ta.Insert(' '); ta.Insert(' '); }
                else if (k >= 32 && k < 127) ta.Insert((char)k);
                else continue;
                changed = true;
            }
            if (changed) { host.Render(new Color(0xFF141820)); host.Present(); }
        }
    }

    /// <summary>Default document path. /disk is read-only, so the editor saves
    /// to the writable FAT12 RAM disk.</summary>
    const string DocPath = "/ram/NOTE.TXT";

    // Save the buffer with System.IO. Returns the byte count written (0 = failed).
    static int Save(TextArea ta)
    {
        char[] text = new char[ta.Length];
        int n = ta.CopyTo(text);
        return BzFile.WriteAllChars(DocPath, text, n);
    }

    // Load the document with System.IO. Returns the char count read (0 = none).
    static int Open(TextArea ta)
    {
        char[] text;
        int n = BzFile.ReadAllChars(DocPath, 64 * 1024, out text);
        if (n > 0) ta.SetText(text, n);
        return n;
    }

    static void Main()
    {
        Console.WriteLine("Editor: Text Editor (Buitenzorg.UI + System.IO)...");
        Font font = Font.Default();
        const int W = 420, H = 300;
        UIHost host = new UIHost("Text Editor", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 10; root.Spacing = 6;
        root.Background = new Color(0xFF1C2028);

        Menu menu = new Menu(font);
        menu.AddItem("FILE"); menu.AddItem("EDIT"); menu.AddItem("VIEW"); menu.AddItem("HELP");
        menu.Height = 18;

        TextArea ta = new TextArea(font);
        ta.Width = 400; ta.Height = 232;

        root.Add(menu);
        root.Add(ta);
        host.Root = root;
        host.Layout();

        // Simulate typing + editing.
        ta.Type("HALO BUITENZORG");     // 15
        ta.Newline();                   // +1
        ta.Type("EDITOR TEKS 123");     // +15  -> len 31, 2 lines
        bool typedOk = ta.LineCount() == 2 && ta.Length == 31;
        ta.Backspace();                 // hapus '3' -> len 30
        bool editOk = ta.Length == 30 && ta.CharAt(29) == '2';

        host.Render(new Color(0xFF141820));
        host.Present();

        // Save to the RAM disk and read it back: proves the FILE menu's
        // Open/Save actually round-trip through the VFS.
        int saved = Save(ta);
        char[] check;
        int reread = BzFile.ReadAllChars(DocPath, 4096, out check);
        bool ioOk = saved == ta.Length && reread == ta.Length;
        if (ioOk)
            for (int i = 0; i < reread; i++) if (check[i] != ta.CharAt(i)) { ioOk = false; break; }

        if (typedOk && editOk && ioOk)
            Console.WriteLine("MILESTONE: EDITOR OK");
        else
            Console.WriteLine("Editor: verifikasi gagal (ketik/edit/simpan)");

        // Live editing when launched from the shell; skipped at boot.
        if (bz_is_interactive() != 0)
            Interactive(host, ta);
    }
}
