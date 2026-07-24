// Buitenzorg OS — v0.16 "Panen": Buitenzorg.UI toolkit demo (increment 2).
//
// Builds a richer retained UI (menu bar, slider, list box, radio buttons, text
// box, button, progress bar in a StackPanel), runs layout, routes simulated
// mouse events through the toolkit (hover / press / click / drag / select),
// verifies the resulting state + a Grid layout, renders into a Bitmap, and blits
// it to a window. Built with bflat --stdlib:zero together with bzui.cs, bzgfx.cs.

using System;
using Buitenzorg.Drawing;
using Buitenzorg.UI;

class UIDemo
{
    static void Main()
    {
        Console.WriteLine("UI: menguji Buitenzorg.UI (kontrol + layout + event)...");

        Font font = Font.Default();
        const int W = 380, H = 340;
        UIHost host = new UIHost("Buitenzorg.UI", W, H);

        StackPanel root = new StackPanel();
        root.Padding = 12; root.Spacing = 8;
        root.Background = new Color(0xFF1C2028);

        Menu menu = new Menu(font);
        menu.AddItem("FILE"); menu.AddItem("EDIT"); menu.AddItem("VIEW"); menu.AddItem("HELP");
        menu.Height = 18;

        TextBlock title = new TextBlock("BUITENZORG.UI TOOLKIT", font);
        title.Foreground = Color.White;

        Slider slider = new Slider();
        slider.Width = 220; slider.Value = 30;

        ListBox list = new ListBox(font);
        list.Width = 220; list.Height = 40;
        list.AddItem("ITEM SATU"); list.AddItem("ITEM DUA"); list.AddItem("ITEM TIGA");

        StackPanel radios = new StackPanel();
        radios.Horizontal = true; radios.Spacing = 24;
        RadioButton rA = new RadioButton("OPSI A", font);
        RadioButton rB = new RadioButton("OPSI B", font);
        radios.Add(rA); radios.Add(rB);

        TextBox tb = new TextBox("EDIT.TXT", font);
        tb.Width = 220;

        Button btn = new Button("KLIK SAYA", font);
        btn.Width = 130; btn.Height = 28;

        ProgressBar bar = new ProgressBar();
        bar.Width = 220; bar.Height = 16; bar.Value = 65;

        root.Add(menu); root.Add(title); root.Add(slider); root.Add(list);
        root.Add(radios); root.Add(tb); root.Add(btn); root.Add(bar);
        host.Root = root;
        host.Layout();

        // Route simulated mouse interactions (each is a down then up).
        Tap(host, slider.X + slider.W * 3 / 4, slider.Y + slider.H / 2);          // drag to ~75%
        Tap(host, list.X + 10, list.Y + 2 + list.ItemHeight + 3);                 // select item 1
        Tap(host, rA.X + 7, rA.Y + rA.H / 2);                                     // select radio A
        Tap(host, tb.X + 10, tb.Y + tb.H / 2);                                    // focus text box
        Tap(host, btn.X + btn.W / 2, btn.Y + btn.H / 2);                          // click button

        host.Render(new Color(0xFF141820));

        // Grid layout check (2 columns: 100px + star; 2 rows: 30px + star).
        Grid grid = new Grid();
        grid.AddColumn(100); grid.AddColumn(-1);
        grid.AddRow(30); grid.AddRow(-1);
        UIElement cell = new UIElement();
        cell.GridCol = 1; cell.GridRow = 1;
        grid.Add(cell);
        grid.Measure(300, 200);
        grid.Arrange(0, 0, 300, 200);
        bool gridOk = cell.X == 100 && cell.Y == 30 && cell.W == 200 && cell.H == 170;

        bool ok = btn.Clicks == 1
                  && slider.Value >= 70 && slider.Value <= 80
                  && list.SelectedIndex == 1
                  && rA.Selected
                  && tb.Focused
                  && gridOk;
        Color pf = host.Surface.GetPixel(bar.X + 6, bar.Y + bar.H / 2); // progress fill (green)
        if (!(pf.G > 120 && pf.R < 140)) ok = false;

        host.Present();

        bool adv = VerifyAdvanced(font);

        if (ok && adv)
            Console.WriteLine("MILESTONE: UI OK");
        else
            Console.WriteLine("UI: verifikasi gagal (event/grid/render/lanjutan)");
    }

    // Exercise the richer controls (ComboBox / TabControl / TreeView /
    // ScrollViewer / DataGrid / RadioGroup) with direct interaction, then render
    // a second "advanced controls" window for the screenshot.
    static bool VerifyAdvanced(Font font)
    {
        bool ok = true;

        // ComboBox: verified below through the popup layer (host2.Mouse).
        ComboBox combo = new ComboBox(font);
        combo.Width = 150;
        combo.AddItem("MERAH"); combo.AddItem("HIJAU"); combo.AddItem("BIRU");

        // TreeView: expand the root node, then select its child.
        TreeView tree = new TreeView(font);
        TreeNode sys = tree.AddRoot("SISTEM");
        TreeNode kern = sys.AddChild("KERNEL");
        TreeNode drv = sys.AddChild("DRIVER"); drv.AddChild("AC97");
        sys.AddChild("APP");
        tree.Measure(180, 200); tree.Arrange(0, 0, 180, 120);
        tree.MouseDown(tree.X + 6, tree.Y + 2 + tree.RowHeight / 2);   // toggle SISTEM expander
        tree.MouseDown(tree.X + 30, tree.Y + 2 + tree.RowHeight + tree.RowHeight / 2); // select KERNEL
        if (!(sys.Expanded && tree.Selected == kern)) ok = false;

        // DataGrid: header + rows; select row 1.
        DataGrid grid = new DataGrid(font);
        grid.AddColumn("PID", 60); grid.AddColumn("NAMA", 110);
        grid.AddRow().Cell("1").Cell("INIT");
        grid.AddRow().Cell("2").Cell("SHELL");
        grid.AddRow().Cell("3").Cell("WM");
        grid.Measure(170, 120); grid.Arrange(0, 0, 170, 120);
        grid.MouseDown(grid.X + 10, grid.Y + grid.RowHeight * 2 + 2);  // row index 1
        if (grid.SelectedRow != 1) ok = false;

        // TabControl: two tabs; switch to the second.
        TabControl tabs = new TabControl(font);
        tabs.Height = 140;
        tabs.AddTab("POHON", tree);
        tabs.AddTab("TABEL", grid);
        tabs.Measure(320, 160); tabs.Arrange(0, 0, 320, 140);
        int t0 = font.Measure("POHON") + 16;
        tabs.MouseDown(tabs.X + t0 + 4, tabs.Y + tabs.TabH / 2);       // click "TABEL"
        if (tabs.SelectedIndex != 1) ok = false;
        tabs.SelectedIndex = 0; // show the tree in the screenshot

        // ScrollViewer: tall content scrolls.
        ScrollViewer scroll = new ScrollViewer();
        StackPanel tall = new StackPanel();
        for (int i = 0; i < 16; i++) tall.Add(new TextBlock("BARIS", font));
        scroll.SetContent(tall);
        scroll.Measure(160, 70); scroll.Arrange(0, 0, 160, 70);
        scroll.ScrollBy(30);
        if (scroll.Offset <= 0) ok = false;

        // RadioGroup: selecting one clears the rest.
        RadioGroup rg = new RadioGroup();
        RadioButton r1 = new RadioButton("SATU", font);
        RadioButton r2 = new RadioButton("DUA", font);
        RadioButton r3 = new RadioButton("TIGA", font);
        rg.Attach(r1); rg.Attach(r2); rg.Attach(r3);
        r1.MouseDown(0, 0); r3.MouseDown(0, 0);
        if (!(r3.Selected && !r1.Selected && !r2.Selected)) ok = false;

        // Render an "advanced controls" window showcasing the themed look
        // (rounded + gradient + shadow + AA) plus combo/tabbed tree/grid.
        UIHost host2 = new UIHost("Buitenzorg.UI - Lanjutan", 380, 330);
        StackPanel root2 = new StackPanel();
        root2.Padding = 12; root2.Spacing = 8;
        root2.Background = new Color(0xFF1C2028);
        TextBlock t2 = new TextBlock("KONTROL LANJUTAN", font);
        t2.Foreground = Color.White;
        Button showBtn = new Button("TOMBOL BERGRADASI", font); showBtn.Height = 28;
        Slider showSld = new Slider(); showSld.Value = 60; showSld.Height = 18;
        ProgressBar showBar = new ProgressBar(); showBar.Value = 72; showBar.Height = 14;
        root2.Add(t2);
        root2.Add(combo);
        root2.Add(showBtn);
        root2.Add(showSld);
        root2.Add(showBar);
        root2.Add(tabs);
        host2.Root = root2;
        host2.Layout();

        // Popup routing: open the combo and click a dropdown item that lies OVER
        // the tab control behind it. The popup layer must give the click to the
        // combo (select "BIRU") and leave the tab control untouched.
        int tabSel = tabs.SelectedIndex;
        host2.Mouse(combo.X + 10, combo.Y + combo.H / 2, true);
        host2.Mouse(combo.X + 10, combo.Y + combo.H / 2, false);          // open
        int itemY = combo.Y + combo.H + combo.RowHeight * 2 + 2;          // item index 2, over tabs
        host2.Mouse(combo.X + 10, itemY, true);
        host2.Mouse(combo.X + 10, itemY, false);                          // pick "BIRU"
        if (!(combo.SelectedIndex == 2 && !combo.IsOpen && tabs.SelectedIndex == tabSel)) ok = false;

        host2.Render(new Color(0xFF141820));
        host2.Present();

        return ok;
    }

    static void Tap(UIHost host, int x, int y)
    {
        host.Mouse(x, y, true);
        host.Mouse(x, y, false);
    }
}
