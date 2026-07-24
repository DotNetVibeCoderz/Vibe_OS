// Buitenzorg OS — v0.16 "Panen" preloaded suite: App Store (wired to pkg.rs).
//
// A store front built on Buitenzorg.UI, now backed by the kernel package
// manager: the catalog comes from PKG_LIST (registry + install state) and the
// PASANG/HAPUS button installs/removes via PKG_SET (which gates the shell's
// `run`). The demo loads the catalog, installs a not-yet-installed package,
// re-reads the registry to confirm the kernel state changed, renders, and
// prints MILESTONE: STORE OK.
//
// The registry is read and written through Buitenzorg.Bcl's `BzPkg`, so the raw
// PkgInfo layout (name[24] + category[16] + installed) lives in one place
// instead of being decoded by pointer arithmetic here. Built with bflat
// --stdlib:zero with bzui.cs, bzgfx.cs, bzbcl.cs, bzbcl2.cs.

using System;
using Buitenzorg;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

sealed unsafe class StoreView : UIElement
{
    sealed class Item
    {
        public char[] Disp; public int DispN;   // uppercased name (display)
        public byte[] NameB; public int NameN;   // raw name (for PKG_SET)
        public char[] Cat; public int CatN;
        public bool Installed;
        public BzPkgInfo Pkg;   // the entry this row came from
        public Item Next;
    }
    Item _head, _tail;
    int _n;
    public int Sel;
    Font _font;
    public int RowH = 30;
    public StoreView(Font f) { _font = f; }

    /// <summary>Add a row from a package-manager entry (Buitenzorg.Bcl).</summary>
    public void Add(BzPkgInfo pkg)
    {
        Item it = new Item();
        it.DispN = pkg.NameLen; it.NameN = pkg.NameLen;
        it.Disp = new char[pkg.NameLen]; it.NameB = new byte[pkg.NameLen];
        for (int i = 0; i < pkg.NameLen; i++)
        {
            char c = pkg.Name[i];
            it.NameB[i] = (byte)c;                       // PKG_SET wants the real name
            it.Disp[i] = (c >= 'a' && c <= 'z') ? (char)(c - 32) : c;   // display uppercase
        }
        it.Cat = new char[pkg.CategoryLen]; it.CatN = pkg.CategoryLen;
        for (int i = 0; i < pkg.CategoryLen; i++) it.Cat[i] = pkg.Category[i];
        it.Installed = pkg.Installed;
        it.Pkg = pkg;
        if (_tail == null) { _head = it; _tail = it; } else { _tail.Next = it; _tail = it; }
        _n++;
    }
    public int Count => _n;
    Item At(int i) { Item it = _head; while (i-- > 0 && it != null) it = it.Next; return it; }
    public bool SelInstalled() { Item it = At(Sel); return it != null && it.Installed; }
    public void SetSelInstalled(bool v) { Item it = At(Sel); if (it != null) it.Installed = v; }
    public BzPkgInfo SelPkg() { Item it = At(Sel); return it == null ? null : it.Pkg; }
    // Index of the first not-installed package (or -1).
    public int FirstAvailable() { int i = 0; for (Item it = _head; it != null; it = it.Next) { if (!it.Installed) return i; i++; } return -1; }

    public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 380; DesiredH = Height >= 0 ? Height : _n * RowH + 4; }
    public override void Render(Graphics g)
    {
        if (!Visible) return;
        g.FillRoundedRectangle(new Color(0xFF181C24), X, Y, W, H, 6);
        g.DrawRoundedRectangle(new Color(0xFF3A4050), X, Y, W, H, 6);
        int i = 0, yy = Y + 2;
        for (Item it = _head; it != null; it = it.Next)
        {
            if (i == Sel) g.FillRoundedRectangle(new Color(0xFF2A3448), X + 2, yy, W - 4, RowH - 2, 4);
            g.DrawChars(_font, it.Disp, it.DispN, new Color(0xFFE6E6E6), X + 10, yy + (RowH - _font.CharH) / 2);
            g.DrawChars(_font, it.Cat, it.CatN, new Color(0xFF8890A0), X + 150, yy + (RowH - _font.CharH) / 2);
            Color bg = it.Installed ? new Color(0xFF2E7D4E) : new Color(0xFF3A4050);
            string s = it.Installed ? "TERPASANG" : "TERSEDIA";
            int bw = s.Length * _font.CharW + 12;
            int bx = X + W - bw - 8;
            g.FillRoundedRectangle(bg, bx, yy + 4, bw, RowH - 10, 6);
            g.DrawString(_font, s, new Color(0xFFE6ECF0), bx + 6, yy + (RowH - _font.CharH) / 2);
            yy += RowH; i++;
        }
    }
    public override void MouseDown(int mx, int my) { int idx = (my - Y - 2) / RowH; if (idx >= 0 && idx < _n) Sel = idx; }
}

class StoreApp
{
    /// <summary>Fill the view from the kernel package registry. Returns the count.</summary>
    static int Load(StoreView view)
    {
        BzPkgInfo pkgs = BzPkg.List(32);
        for (BzPkgInfo p = pkgs; p != null; p = p.Next) view.Add(p);
        return BzPkg.Count(pkgs);
    }

    static unsafe void Main()
    {
        Console.WriteLine("Store: App Store (Buitenzorg.UI + pkg.rs)...");
        Font font = Font.Default();
        const int W = 440, H = 340;
        UIHost host = new UIHost("App Store", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);
        TextBlock title = new TextBlock("APP STORE - BUITENZORG", font);
        title.Foreground = Color.White;

        StoreView view = new StoreView(font);
        view.Width = 416; view.Height = 240;

        int count = Load(view);

        Button action = new Button("PASANG / HAPUS", font);
        action.Width = 160; action.Height = 30;

        root.Add(title);
        root.Add(view);
        root.Add(action);
        host.Root = root;
        host.Layout();

        // Select the first available package and install it via the button.
        int avail = view.FirstAvailable();
        if (avail < 0) avail = 0;
        view.Sel = avail;
        bool wasInstalled = view.SelInstalled();

        int before = action.Clicks;
        host.Mouse(action.X + action.W / 2, action.Y + action.H / 2, true);
        host.Mouse(action.X + action.W / 2, action.Y + action.H / 2, false);
        bool kernelOk = false;
        if (action.Clicks > before)
        {
            BzPkgInfo pkg = view.SelPkg();
            bool applied = wasInstalled ? BzPkg.Remove(pkg) : BzPkg.Install(pkg);
            if (applied) view.SetSelInstalled(!wasInstalled);
            // Re-read the registry: proves the change landed in the kernel, not
            // just in this app's copy.
            char[] nm = new char[pkg.NameLen];
            for (int i = 0; i < pkg.NameLen; i++) nm[i] = pkg.Name[i];
            BzPkgInfo fresh = BzPkg.List(32);
            BzPkgInfo again = null;
            for (BzPkgInfo q = fresh; q != null; q = q.Next)
            {
                if (q.NameLen != pkg.NameLen) continue;
                bool eq = true;
                for (int i = 0; i < q.NameLen; i++) if (q.Name[i] != nm[i]) { eq = false; break; }
                if (eq) { again = q; break; }
            }
            kernelOk = again != null && again.Installed == !wasInstalled;
        }

        host.Render(new Color(0xFF141820));
        host.Present();

        bool ok = count > 0 && !wasInstalled && view.SelInstalled() && kernelOk;
        if (ok)
            Console.WriteLine("MILESTONE: STORE OK");
        else
            Console.WriteLine("Store: verifikasi gagal (list/install/kernel)");
    }
}
