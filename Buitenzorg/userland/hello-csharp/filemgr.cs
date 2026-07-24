// Buitenzorg OS — v0.16 "Panen" preloaded suite: File Manager.
//
// Browses the VFS through the FS_LIST syscall: an empty path lists the mounts
// (/disk, /ram); a mount path lists its files. Built on Buitenzorg.UI + Drawing.
// The demo lists the mounts, navigates into /disk, verifies a known file is
// present, renders, and prints MILESTONE: FILES OK. When launched interactively
// (`run files` from the shell — detected via the IS_INTERACTIVE syscall) it then
// enters a live keyboard loop (KEY_READ): W/K or S/J move the selection, Enter
// opens the selected folder (".." / Backspace goes back to the mounts), ESC
// exits. During the headless boot demo IS_INTERACTIVE is 0 so it exits
// immediately. Built with bflat --stdlib:zero + bzui/bzgfx.

using System;
using System.Runtime.InteropServices;
using Buitenzorg;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

sealed unsafe class FileView : UIElement
{
    sealed class Item { public char[] Disp; public int DispN; public bool IsDir; public byte[] Raw; public int RawN; public Item Next; }
    Item _head, _tail;
    int _n;
    public int Sel;
    Font _font;
    public int RowH = 22;
    public readonly char[] Path = new char[64];
    public int PathN;
    public FileView(Font f) { _font = f; }

    public void Clear() { _head = null; _tail = null; _n = 0; Sel = 0; }
    /// <summary>Add a row from a directory entry (Buitenzorg.Bcl System.IO).</summary>
    public void Add(BzFileInfo e)
    {
        Item it = new Item();
        int nn = e.NameLen;
        it.Disp = new char[nn]; it.DispN = nn; it.Raw = new byte[nn]; it.RawN = nn;
        for (int i = 0; i < nn; i++) { it.Disp[i] = e.Name[i]; it.Raw[i] = (byte)e.Name[i]; }
        it.IsDir = e.IsDirectory;
        if (_tail == null) { _head = it; _tail = it; } else { _tail.Next = it; _tail = it; }
        _n++;
    }
    // Add a synthetic ".." entry at the front (go up to mounts).
    public void AddUp()
    {
        Item it = new Item(); it.Disp = new char[] { '.', '.' }; it.DispN = 2; it.Raw = new byte[0]; it.RawN = 0; it.IsDir = true;
        it.Next = _head; _head = it; if (_tail == null) _tail = it; _n++;
    }
    public int Count => _n;
    Item At(int i) { Item it = _head; while (i-- > 0 && it != null) it = it.Next; return it; }
    public bool SelIsDir() { Item it = At(Sel); return it != null && it.IsDir; }
    public char[] SelDisp() { Item it = At(Sel); return it == null ? null : it.Disp; }
    public int SelDispN() { Item it = At(Sel); return it == null ? 0 : it.DispN; }
    public byte[] SelRaw() { Item it = At(Sel); return it == null ? null : it.Raw; }
    public int SelRawN() { Item it = At(Sel); return it == null ? 0 : it.RawN; }
    // Does any entry's display name match `target` (case-sensitive)?
    public bool Has(string target)
    {
        for (Item it = _head; it != null; it = it.Next)
        {
            if (it.DispN != target.Length) continue;
            bool eq = true;
            for (int i = 0; i < it.DispN; i++) if (it.Disp[i] != target[i]) { eq = false; break; }
            if (eq) return true;
        }
        return false;
    }

    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 380; DesiredH = Height >= 0 ? Height : 240; }
    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF181C24), X, Y, W, H, 6);
        g.DrawRoundedRectangle(new Color(0xFF3A4050), X, Y, W, H, 6);
        // path header
        g.FillRectangle(new Color(0xFF232833), X + 1, Y + 1, W - 2, 18);
        g.DrawChars(_font, Path, PathN, new Color(0xFF7CC0FF), X + 8, Y + 6);
        int yy = Y + 22, i = 0;
        for (Item it = At(0); it != null; )
        {
            if (i == Sel) g.FillRoundedRectangle(new Color(0xFF2A3448), X + 2, yy, W - 4, RowH - 1, 3);
            // folder = amber square, file = gray square
            Color ic = it.IsDir ? new Color(0xFFE0A040) : new Color(0xFF6E7686);
            g.FillRoundedRectangle(ic, X + 8, yy + (RowH - 10) / 2, 12, 10, 2);
            g.DrawChars(_font, it.Disp, it.DispN, new Color(0xFFE6E6E6), X + 28, yy + (RowH - _font.CharH) / 2);
            yy += RowH; i++;
            // advance
            Item nx = At(i); it = nx;
        }
    }
    public override void MouseDown(int mx, int my) { int idx = (my - Y - 22) / RowH; if (idx >= 0 && idx < _n) Sel = idx; }
}

class FileMgr
{
    [DllImport("*")] static extern uint bz_key_read();
    [DllImport("*")] static extern ulong bz_is_interactive();

    // Directory listings come from Buitenzorg.Bcl (System.IO); the FsEntry
    // layout is decoded once, inside BzDir, instead of here.
    static int LoadDirChars(FileView fv, char[] path, int plen, bool showUp)
    {
        BzFileInfo head = BzDir.GetEntries(path, plen, 64);
        fv.Clear();
        if (plen == 0) { fv.Path[0] = '/'; fv.PathN = 1; }
        else { for (int i = 0; i < plen && i < 63; i++) fv.Path[i] = path[i]; fv.PathN = plen; }
        if (showUp) fv.AddUp();
        for (BzFileInfo e = head; e != null; e = e.Next) fv.Add(e);
        return BzDir.Count(head);
    }

    static int LoadDir(FileView fv, string path, bool showUp)
    {
        char[] p = new char[path.Length];
        for (int i = 0; i < path.Length; i++) p[i] = path[i];
        return LoadDirChars(fv, p, path.Length, showUp);
    }

    // Open the selected entry: a mount name becomes "/name"; ".." goes back.
    static bool OpenSelected(FileView fv)
    {
        if (!fv.SelIsDir()) return false;
        byte[] raw = fv.SelRaw();
        int rn = fv.SelRawN();
        char[] p = new char[128];
        if (rn == 0) return LoadDirChars(fv, p, 0, false) >= 0;   // ".." -> mount list
        int pl = 0;
        p[pl++] = BzPath.Separator;
        for (int i = 0; i < rn && pl < 126; i++) p[pl++] = (char)raw[i];
        return LoadDirChars(fv, p, pl, true) >= 0;
    }

    // Live keyboard loop: W/K up, S/J down, Enter opens dir, Backspace -> mounts,
    // ESC exits. Only entered when the desktop is up (a shell-launched app).
    static unsafe void Interactive(UIHost host, FileView fv)
    {
        while (true)
        {
            bool changed = false;
            uint k;
            while ((k = bz_key_read()) != 0)
            {
                if (k == 0x1B) return;
                if (k == 'w' || k == 'W' || k == 'k' || k == 'K')
                { if (fv.Sel > 0) { fv.Sel--; changed = true; } }
                else if (k == 's' || k == 'S' || k == 'j' || k == 'J')
                { if (fv.Sel < fv.Count - 1) { fv.Sel++; changed = true; } }
                else if (k == '\n' || k == '\r')
                { if (OpenSelected(fv)) changed = true; }
                else if (k == 0x08 || k == 0x7F)
                { LoadDirChars(fv, new char[1], 0, false); changed = true; }
            }
            if (changed) { host.Render(new Color(0xFF141820)); host.Present(); }
        }
    }

    static void Main()
    {
        Console.WriteLine("Files: File Manager (Buitenzorg.UI + VFS)...");
        Font font = Font.Default();
        const int W = 420, H = 320;
        UIHost host = new UIHost("File Manager", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);
        TextBlock title = new TextBlock("FILE MANAGER", font);
        title.Foreground = Color.White;

        FileView fv = new FileView(font);
        fv.Width = 396; fv.Height = 250;

        root.Add(title);
        root.Add(fv);
        host.Root = root;
        host.Layout();

        // List the mounts, then navigate into /disk and confirm a known file.
        int mounts = LoadDir(fv, "", false);
        bool mountsOk = mounts >= 1;
        int files = LoadDir(fv, "/disk", true);
        bool filesOk = files > 5 && fv.Has("CALC.ELF");

        host.Render(new Color(0xFF141820));
        host.Present();

        if (mountsOk && filesOk)
            Console.WriteLine("MILESTONE: FILES OK");
        else
            Console.WriteLine("Files: verifikasi gagal (mount/list)");

        // Live navigation when launched from the shell; skipped at boot.
        if (bz_is_interactive() != 0)
            Interactive(host, fv);
    }
}
