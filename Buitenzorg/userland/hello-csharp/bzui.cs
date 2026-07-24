// Buitenzorg.UI (v0.16 "Panen") — a lightweight retained-mode UI toolkit in the
// WPF/Avalonia style, built on Buitenzorg.Drawing. Controls form a visual tree;
// layout is a Measure/Arrange pass; rendering draws the whole tree into a
// managed Bitmap that is blitted to the window in one syscall (a software
// compositor). Runs on the managed heap (v0.15). Self-contained (arrays only).

using Buitenzorg.Drawing;

namespace Buitenzorg.UI
{
    /// <summary>Base of every control: bounds, layout hooks, children, hit-test.</summary>
    public class UIElement
    {
        public int X, Y, W, H;              // arranged bounds (pixels)
        public int Width = -1, Height = -1; // requested size (-1 = auto)
        public int DesiredW, DesiredH;      // from Measure
        public Color Background = Color.Transparent;
        public Color Foreground = new Color(0xFFE6E6E6);
        public bool Visible = true;
        public int CornerRadius = 0; // rounded corners for themed containers

        // Popups (e.g. a ComboBox dropdown) render on top of the whole tree and
        // get first shot at hit-testing. Default: no popup.
        public virtual void RenderPopup(Graphics g) { }
        public virtual UIElement PopupHitTest(int px, int py) { return null; }

        // Attached layout properties (used by Grid / Canvas).
        public int GridRow, GridCol;
        public int GridRowSpan = 1, GridColSpan = 1;
        public int CanvasLeft, CanvasTop;

        // Input events, routed by UIHost (virtual dispatch works under zerolib).
        public virtual void MouseEnter() { }
        public virtual void MouseLeave() { }
        public virtual void MouseDown(int mx, int my) { }
        public virtual void MouseUp(int mx, int my) { }
        public virtual void MouseMove(int mx, int my) { }

        // Children are a linked list rather than an object[] on purpose: under
        // zerolib, storing a reference into an object-array element (`stelem.ref`
        // -> RhpStelemRef, array-covariance check) faults, while an object *field*
        // store (RhpAssignRef) works — so we store child refs in node fields.
        sealed class ChildNode { public UIElement E; public ChildNode Next; }
        ChildNode _head, _tail;
        int _n;
        public void Add(UIElement e)
        {
            ChildNode node = new ChildNode();
            node.E = e;
            if (_tail == null) { _head = node; _tail = node; }
            else { _tail.Next = node; _tail = node; }
            _n++;
        }
        public int ChildCount => _n;
        public UIElement Child(int i)
        {
            ChildNode c = _head;
            while (i-- > 0 && c != null) c = c.Next;
            return c == null ? null : c.E;
        }

        public virtual void Measure(int availW, int availH)
        {
            int cw = 0, ch = 0;
            for (ChildNode n = _head; n != null; n = n.Next) { n.E.Measure(availW, availH); if (n.E.DesiredW > cw) cw = n.E.DesiredW; if (n.E.DesiredH > ch) ch = n.E.DesiredH; }
            DesiredW = Width >= 0 ? Width : cw;
            DesiredH = Height >= 0 ? Height : ch;
        }
        public virtual void Arrange(int x, int y, int w, int h)
        {
            X = x; Y = y; W = w; H = h;
            for (ChildNode n = _head; n != null; n = n.Next) n.E.Arrange(x, y, w, h);
        }
        public virtual void Render(Graphics g)
        {
            if (!Visible) return;
            if (Background.A > 0) g.FillRectangle(Background, X, Y, W, H);
            for (ChildNode n = _head; n != null; n = n.Next) n.E.Render(g);
        }
        /// <summary>Deepest visible element under (px,py), or null (last hit = topmost).</summary>
        public virtual UIElement HitTest(int px, int py)
        {
            if (!Visible || px < X || px >= X + W || py < Y || py >= Y + H) return null;
            UIElement found = this;
            for (ChildNode n = _head; n != null; n = n.Next) { UIElement h = n.E.HitTest(px, py); if (h != null) found = h; }
            return found;
        }
    }

    /// <summary>Stacks children vertically (default) or horizontally.</summary>
    public class StackPanel : UIElement
    {
        public bool Horizontal = false;
        public int Spacing = 0;
        public int Padding = 0;

        public override void Measure(int aw, int ah)
        {
            int main = 0, cross = 0;
            for (int i = 0; i < ChildCount; i++)
            {
                UIElement c = Child(i); c.Measure(aw, ah);
                if (Horizontal) { main += c.DesiredW + (i > 0 ? Spacing : 0); if (c.DesiredH > cross) cross = c.DesiredH; }
                else { main += c.DesiredH + (i > 0 ? Spacing : 0); if (c.DesiredW > cross) cross = c.DesiredW; }
            }
            main += 2 * Padding; cross += 2 * Padding;
            if (Horizontal) { DesiredW = Width >= 0 ? Width : main; DesiredH = Height >= 0 ? Height : cross; }
            else { DesiredW = Width >= 0 ? Width : cross; DesiredH = Height >= 0 ? Height : main; }
        }
        public override void Arrange(int x, int y, int w, int h)
        {
            X = x; Y = y; W = w; H = h;
            int pos = (Horizontal ? x : y) + Padding;
            int cross0 = (Horizontal ? y : x) + Padding;
            int crossSize = (Horizontal ? h : w) - 2 * Padding;
            for (int i = 0; i < ChildCount; i++)
            {
                UIElement c = Child(i);
                if (Horizontal)
                {
                    int chh = c.Height >= 0 ? c.DesiredH : crossSize; // explicit Height wins, else stretch
                    c.Arrange(pos, cross0, c.DesiredW, chh); pos += c.DesiredW + Spacing;
                }
                else
                {
                    int cww = c.Width >= 0 ? c.DesiredW : crossSize;  // explicit Width wins, else stretch
                    c.Arrange(cross0, pos, cww, c.DesiredH); pos += c.DesiredH + Spacing;
                }
            }
        }
    }

    /// <summary>A grid of rows/columns (fixed pixel size, or -1 for a star share).</summary>
    public class Grid : UIElement
    {
        int[] _cols = new int[4]; int _ncol;
        int[] _rows = new int[4]; int _nrow;
        int[] _colW = new int[0]; int[] _colX = new int[0];
        int[] _rowH = new int[0]; int[] _rowY = new int[0];
        public int Spacing = 0;

        static int[] EnsureCap(int[] a, int n) { if (n < a.Length) return a; int[] b = new int[n == 0 ? 4 : n * 2]; for (int i = 0; i < n; i++) b[i] = a[i]; return b; }
        public void AddColumn(int w) { _cols = EnsureCap(_cols, _ncol); _cols[_ncol++] = w; }
        public void AddRow(int h) { _rows = EnsureCap(_rows, _nrow); _rows[_nrow++] = h; }

        void Compute(int w, int h)
        {
            _colW = new int[_ncol]; _colX = new int[_ncol];
            _rowH = new int[_nrow]; _rowY = new int[_nrow];
            int fixedW = 0, starC = 0;
            for (int i = 0; i < _ncol; i++) { if (_cols[i] >= 0) fixedW += _cols[i]; else starC++; }
            int remW = w - fixedW - Spacing * (_ncol > 0 ? _ncol - 1 : 0); if (remW < 0) remW = 0;
            int starW = starC > 0 ? remW / starC : 0;
            int fixedH = 0, starR = 0;
            for (int i = 0; i < _nrow; i++) { if (_rows[i] >= 0) fixedH += _rows[i]; else starR++; }
            int remH = h - fixedH - Spacing * (_nrow > 0 ? _nrow - 1 : 0); if (remH < 0) remH = 0;
            int starH = starR > 0 ? remH / starR : 0;
            int cx = 0;
            for (int i = 0; i < _ncol; i++) { _colW[i] = _cols[i] >= 0 ? _cols[i] : starW; _colX[i] = cx; cx += _colW[i] + Spacing; }
            int cy = 0;
            for (int i = 0; i < _nrow; i++) { _rowH[i] = _rows[i] >= 0 ? _rows[i] : starH; _rowY[i] = cy; cy += _rowH[i] + Spacing; }
        }
        public override void Measure(int aw, int ah)
        {
            for (int i = 0; i < ChildCount; i++) Child(i).Measure(aw, ah);
            DesiredW = Width >= 0 ? Width : aw;
            DesiredH = Height >= 0 ? Height : ah;
        }
        public override void Arrange(int x, int y, int w, int h)
        {
            X = x; Y = y; W = w; H = h;
            Compute(w, h);
            for (int i = 0; i < ChildCount; i++)
            {
                UIElement c = Child(i);
                int col = c.GridCol; if (col < 0) col = 0; if (col >= _ncol) col = _ncol - 1;
                int row = c.GridRow; if (row < 0) row = 0; if (row >= _nrow) row = _nrow - 1;
                if (_ncol == 0 || _nrow == 0) { c.Arrange(x, y, w, h); continue; }
                int cw = 0; for (int k = 0; k < c.GridColSpan && col + k < _ncol; k++) cw += _colW[col + k] + (k > 0 ? Spacing : 0);
                int ch = 0; for (int k = 0; k < c.GridRowSpan && row + k < _nrow; k++) ch += _rowH[row + k] + (k > 0 ? Spacing : 0);
                c.Arrange(x + _colX[col], y + _rowY[row], cw, ch);
            }
        }
    }

    /// <summary>Absolute positioning via each child's CanvasLeft/CanvasTop.</summary>
    public class Canvas : UIElement
    {
        public override void Measure(int aw, int ah)
        {
            for (int i = 0; i < ChildCount; i++) Child(i).Measure(aw, ah);
            DesiredW = Width >= 0 ? Width : aw;
            DesiredH = Height >= 0 ? Height : ah;
        }
        public override void Arrange(int x, int y, int w, int h)
        {
            X = x; Y = y; W = w; H = h;
            for (int i = 0; i < ChildCount; i++) { UIElement c = Child(i); c.Arrange(x + c.CanvasLeft, y + c.CanvasTop, c.DesiredW, c.DesiredH); }
        }
    }

    /// <summary>A bordered container with padding (like Border).</summary>
    public class Border : UIElement
    {
        public Color BorderColor = new Color(0xFF50505A);
        public int BorderThickness = 1;
        public int Padding = 0;
        public bool Shadow = false;
        int Inset => BorderThickness + Padding;
        public override void Measure(int aw, int ah)
        {
            int cw = 0, ch = 0;
            for (int i = 0; i < ChildCount; i++) { UIElement c = Child(i); c.Measure(aw, ah); if (c.DesiredW > cw) cw = c.DesiredW; if (c.DesiredH > ch) ch = c.DesiredH; }
            DesiredW = Width >= 0 ? Width : cw + 2 * Inset;
            DesiredH = Height >= 0 ? Height : ch + 2 * Inset;
        }
        public override void Arrange(int x, int y, int w, int h)
        {
            X = x; Y = y; W = w; H = h;
            for (int i = 0; i < ChildCount; i++) Child(i).Arrange(x + Inset, y + Inset, w - 2 * Inset, h - 2 * Inset);
        }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            int r = CornerRadius;
            if (Shadow) g.DrawShadow(X + 1, Y + 3, W, H, r, 4, 80);
            if (r > 0)
            {
                if (Background.A > 0) g.FillRoundedRectangle(Background, X, Y, W, H, r);
                for (int t = 0; t < BorderThickness; t++) g.DrawRoundedRectangle(BorderColor, X + t, Y + t, W - 2 * t, H - 2 * t, r);
            }
            else
            {
                if (Background.A > 0) g.FillRectangle(Background, X, Y, W, H);
                for (int t = 0; t < BorderThickness; t++) g.DrawRectangle(BorderColor, X + t, Y + t, W - 2 * t, H - 2 * t);
            }
            for (int i = 0; i < ChildCount; i++) Child(i).Render(g);
        }
    }

    /// <summary>A text label (like TextBlock).</summary>
    public class TextBlock : UIElement
    {
        public string Text;
        public Font Font;
        public TextBlock(string t, Font f) { Text = t; Font = f; }
        public override void Measure(int aw, int ah)
        {
            DesiredW = Width >= 0 ? Width : Font.Measure(Text);
            DesiredH = Height >= 0 ? Height : Font.CharH;
        }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            if (Background.A > 0) g.FillRectangle(Background, X, Y, W, H);
            g.DrawString(Font, Text, Foreground, X, Y);
        }
    }

    /// <summary>A push button with normal/hover/pressed states and a click count.</summary>
    public class Button : UIElement
    {
        public string Text;
        public Font Font;
        public Color Normal = new Color(0xFF3C5A8C);
        public Color Hover = new Color(0xFF5078B4);
        public Color Pressed = new Color(0xFF283C64);
        public Color BorderColor = new Color(0xFFC8D2E6);
        public bool IsHover, IsPressed;
        public int Clicks;
        public int Tag; // app-defined identifier (e.g. a calculator key)
        public Button(string t, Font f) { Text = t; Font = f; CornerRadius = 6; }
        public override void Measure(int aw, int ah)
        {
            DesiredW = Width >= 0 ? Width : Font.Measure(Text) + 20;
            DesiredH = Height >= 0 ? Height : Font.CharH + 12;
        }
        static Color Lighten(Color c, int d) => Color.FromRgb(Clamp(c.R + d), Clamp(c.G + d), Clamp(c.B + d));
        static int Clamp(int v) => v < 0 ? 0 : (v > 255 ? 255 : v);
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            Color bg = IsPressed ? Pressed : (IsHover ? Hover : Normal);
            int r = CornerRadius;
            if (!IsPressed) g.DrawShadow(X + 1, Y + 2, W, H, r, 3, 90); // soft drop shadow
            g.FillRoundedGradientV(X, Y, W, H, r, Lighten(bg, 28), bg);  // gradient fill
            g.DrawRoundedRectangle(BorderColor, X, Y, W, H, r);
            int tx = X + (W - Font.Measure(Text)) / 2;
            int ty = Y + (H - Font.CharH) / 2 + (IsPressed ? 1 : 0);
            g.DrawString(Font, Text, Foreground, tx, ty);
        }
        public void Click() => Clicks++;
        public override void MouseEnter() => IsHover = true;
        public override void MouseLeave() { IsHover = false; IsPressed = false; }
        public override void MouseDown(int mx, int my) => IsPressed = true;
        public override void MouseUp(int mx, int my) { if (IsPressed) Click(); IsPressed = false; }
    }

    /// <summary>A labelled checkbox.</summary>
    public class CheckBox : UIElement
    {
        public string Text;
        public Font Font;
        public bool Checked;
        public CheckBox(string t, Font f) { Text = t; Font = f; }
        public override void Measure(int aw, int ah)
        {
            DesiredW = Width >= 0 ? Width : 18 + Font.Measure(Text);
            DesiredH = Height >= 0 ? Height : (Font.CharH > 14 ? Font.CharH : 14);
        }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            int by = Y + (H - 14) / 2;
            g.FillRoundedRectangle(new Color(0xFF14181E), X, by, 14, 14, 3);
            g.DrawRoundedRectangle(Checked ? new Color(0xFF5AB45A) : Foreground, X, by, 14, 14, 3);
            if (Checked) g.FillRoundedRectangle(new Color(0xFF5AB45A), X + 3, by + 3, 8, 8, 2);
            g.DrawString(Font, Text, Foreground, X + 18, Y + (H - Font.CharH) / 2);
        }
        public void Toggle() => Checked = !Checked;
        public override void MouseDown(int mx, int my) => Checked = !Checked;
    }

    /// <summary>A horizontal progress bar (Value 0..100).</summary>
    public class ProgressBar : UIElement
    {
        public int Value;
        public Color Track = new Color(0xFF282832);
        public Color Fill = new Color(0xFF50B478);
        public override void Measure(int aw, int ah)
        {
            DesiredW = Width >= 0 ? Width : 120;
            DesiredH = Height >= 0 ? Height : 14;
        }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            int r = H / 2; // pill shape
            g.FillRoundedRectangle(Track, X, Y, W, H, r);
            int fw = W * Value / 100; if (fw < 0) fw = 0; if (fw > W) fw = W;
            if (fw > 0) g.FillRoundedGradientV(X, Y, fw, H, r, Color.FromRgb(112, 208, 144), Fill);
            g.DrawRoundedRectangle(new Color(0xFF5A5A64), X, Y, W, H, r);
        }
    }

    /// <summary>A draggable horizontal slider (Value 0..100).</summary>
    public class Slider : UIElement
    {
        public int Value;
        public Color Track = new Color(0xFF303040);
        public Color Fill = new Color(0xFF5078B4);
        public Color Thumb = new Color(0xFFC8D2E6);
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 140; DesiredH = Height >= 0 ? Height : 18; }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            int cy = Y + H / 2;
            g.FillRoundedRectangle(Track, X, cy - 2, W, 4, 2);
            int fw = W * Value / 100;
            if (fw > 0) g.FillRoundedRectangle(Fill, X, cy - 2, fw, 4, 2);
            g.DrawShadow(X + fw, cy + 1, 1, 1, 0, 3, 90);
            g.FillCircleAA(Thumb, X + fw, cy, 7);
        }
        void SetFromX(int mx) { int v = (mx - X) * 100 / (W > 0 ? W : 1); if (v < 0) v = 0; if (v > 100) v = 100; Value = v; }
        public override void MouseDown(int mx, int my) => SetFromX(mx);
        public override void MouseMove(int mx, int my) => SetFromX(mx);
    }

    /// <summary>A labelled radio button (grouping is the app's responsibility).</summary>
    public class RadioButton : UIElement
    {
        public string Text;
        public Font Font;
        public bool Selected;
        public int Group;
        public RadioGroup Owner;
        public RadioButton(string t, Font f) { Text = t; Font = f; }
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 18 + Font.Measure(Text); DesiredH = Height >= 0 ? Height : (Font.CharH > 14 ? Font.CharH : 14); }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            int cy = Y + H / 2;
            g.FillCircleAA(new Color(0xFF14181E), X + 7, cy, 7);
            g.DrawCircle(Selected ? new Color(0xFF5AB45A) : Foreground, X + 7, cy, 7);
            if (Selected) g.FillCircleAA(new Color(0xFF5AB45A), X + 7, cy, 3);
            g.DrawString(Font, Text, Foreground, X + 18, Y + (H - Font.CharH) / 2);
        }
        public override void MouseDown(int mx, int my) { if (Owner != null) Owner.Select(this); else Selected = true; }
    }

    /// <summary>A single-select list of text items.</summary>
    public class ListBox : UIElement
    {
        sealed class ItemNode { public string Text; public ItemNode Next; }
        ItemNode _head, _tail;
        int _count;
        public int SelectedIndex = -1;
        public Font Font;
        public int ItemHeight = 12;
        public Color ItemBg = new Color(0xFF20242E);
        public Color SelBg = new Color(0xFF3C5A8C);
        public ListBox(Font f) { Font = f; }
        public void AddItem(string s) { ItemNode n = new ItemNode(); n.Text = s; if (_tail == null) { _head = n; _tail = n; } else { _tail.Next = n; _tail = n; } _count++; }
        public int Count => _count;
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 120; DesiredH = Height >= 0 ? Height : _count * ItemHeight + 4; }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRectangle(ItemBg, X, Y, W, H);
            g.DrawRectangle(new Color(0xFF50505A), X, Y, W, H);
            int i = 0, yy = Y + 2;
            for (ItemNode n = _head; n != null; n = n.Next)
            {
                if (i == SelectedIndex) g.FillRectangle(SelBg, X + 1, yy, W - 2, ItemHeight);
                g.DrawString(Font, n.Text, Foreground, X + 4, yy + (ItemHeight - Font.CharH) / 2);
                yy += ItemHeight; i++;
            }
        }
        public override void MouseDown(int mx, int my) { int idx = (my - Y - 2) / ItemHeight; if (idx >= 0 && idx < _count) SelectedIndex = idx; }
    }

    /// <summary>A single-line text field (display + caret; focus on click).</summary>
    public class TextBox : UIElement
    {
        public string Text;
        public Font Font;
        public bool Focused;
        public TextBox(string t, Font f) { Text = t; Font = f; }
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 140; DesiredH = Height >= 0 ? Height : Font.CharH + 8; }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRectangle(new Color(0xFF14181E), X, Y, W, H);
            g.DrawRectangle(Focused ? new Color(0xFF5078B4) : new Color(0xFF50505A), X, Y, W, H);
            g.DrawString(Font, Text, Foreground, X + 4, Y + (H - Font.CharH) / 2);
            if (Focused) g.FillRectangle(Foreground, X + 4 + Font.Measure(Text), Y + 3, 1, H - 6);
        }
        public override void MouseDown(int mx, int my) => Focused = true;
    }

    /// <summary>A horizontal menu bar of text items.</summary>
    public class Menu : UIElement
    {
        sealed class ItemNode { public string Text; public ItemNode Next; }
        ItemNode _head, _tail;
        public Font Font;
        public Menu(Font f) { Font = f; }
        public void AddItem(string s) { ItemNode n = new ItemNode(); n.Text = s; if (_tail == null) { _head = n; _tail = n; } else { _tail.Next = n; _tail = n; } }
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : aw; DesiredH = Height >= 0 ? Height : Font.CharH + 8; }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRectangle(new Color(0xFF262A34), X, Y, W, H);
            int xx = X + 8;
            for (ItemNode n = _head; n != null; n = n.Next) { g.DrawString(Font, n.Text, Foreground, xx, Y + (H - Font.CharH) / 2); xx += Font.Measure(n.Text) + 16; }
        }
    }

    /// <summary>Groups radio buttons so selecting one clears the rest.</summary>
    public sealed class RadioGroup
    {
        sealed class Node { public RadioButton R; public Node Next; }
        Node _head, _tail;
        public void Attach(RadioButton r)
        {
            Node n = new Node(); n.R = r; r.Owner = this;
            if (_tail == null) { _head = n; _tail = n; } else { _tail.Next = n; _tail = n; }
        }
        public void Select(RadioButton r)
        {
            for (Node n = _head; n != null; n = n.Next) n.R.Selected = (n.R == r);
        }
    }

    /// <summary>A drop-down single-select combo box.</summary>
    public class ComboBox : UIElement
    {
        sealed class ItemNode { public string Text; public ItemNode Next; }
        ItemNode _head, _tail;
        int _count;
        public int SelectedIndex = -1;
        public Font Font;
        public bool IsOpen;
        public int RowHeight = 14;
        public ComboBox(Font f) { Font = f; CornerRadius = 4; }
        public void AddItem(string s) { ItemNode n = new ItemNode(); n.Text = s; if (_tail == null) { _head = n; _tail = n; } else { _tail.Next = n; _tail = n; } _count++; if (SelectedIndex < 0) SelectedIndex = 0; }
        public int Count => _count;
        string SelectedText() { int i = 0; for (ItemNode n = _head; n != null; n = n.Next) { if (i == SelectedIndex) return n.Text; i++; } return ""; }
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 150; DesiredH = Height >= 0 ? Height : RowHeight + 6; }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRoundedGradientV(X, Y, W, H, CornerRadius, new Color(0xFF2A2F3A), new Color(0xFF20242E));
            g.DrawRoundedRectangle(new Color(0xFF50505A), X, Y, W, H, CornerRadius);
            g.DrawString(Font, SelectedText(), Foreground, X + 6, Y + (H - Font.CharH) / 2);
            g.DrawString(Font, IsOpen ? "^" : "v", Foreground, X + W - 12, Y + (H - Font.CharH) / 2);
        }
        // The dropdown list is a popup: drawn on top of the whole tree and
        // hit-tested before the regular tree, so siblings behind it can't steal
        // the click (the "last-hit-wins" problem with in-tree overlays).
        public override void RenderPopup(Graphics g)
        {
            if (!Visible || !IsOpen) return;
            int dy = Y + H;
            g.DrawShadow(X + 1, dy + 2, W, _count * RowHeight, 4, 3, 90);
            g.FillRoundedRectangle(new Color(0xFF181C24), X, dy, W, _count * RowHeight, 4);
            g.DrawRoundedRectangle(new Color(0xFF50505A), X, dy, W, _count * RowHeight, 4);
            int i = 0;
            for (ItemNode n = _head; n != null; n = n.Next)
            {
                if (i == SelectedIndex) g.FillRoundedRectangle(new Color(0xFF3C5A8C), X + 2, dy + 1, W - 4, RowHeight, 3);
                g.DrawString(Font, n.Text, Foreground, X + 6, dy + (RowHeight - Font.CharH) / 2);
                dy += RowHeight; i++;
            }
        }
        public override UIElement PopupHitTest(int px, int py)
        {
            if (!Visible || !IsOpen) return null;
            if (px >= X && px < X + W && py >= Y + H && py < Y + H + _count * RowHeight) return this;
            return null;
        }
        public override UIElement HitTest(int px, int py)
        {
            if (!Visible) return null;
            if (px >= X && px < X + W && py >= Y && py < Y + H) return this;
            return null;
        }
        public override void MouseDown(int mx, int my)
        {
            if (my < Y + H) { IsOpen = !IsOpen; return; }
            if (IsOpen) { int idx = (my - (Y + H)) / RowHeight; if (idx >= 0 && idx < _count) SelectedIndex = idx; IsOpen = false; }
        }
    }

    /// <summary>A tabbed container: a tab strip over one visible content panel.</summary>
    public class TabControl : UIElement
    {
        sealed class TabNode { public string Title; public UIElement Content; public TabNode Next; }
        TabNode _head, _tail;
        int _count;
        public int SelectedIndex = 0;
        public Font Font;
        public int TabH = 18;
        public TabControl(Font f) { Font = f; }
        public void AddTab(string title, UIElement content) { TabNode n = new TabNode(); n.Title = title; n.Content = content; if (_tail == null) { _head = n; _tail = n; } else { _tail.Next = n; _tail = n; } _count++; }
        public int Count => _count;
        UIElement SelectedContent() { int i = 0; for (TabNode n = _head; n != null; n = n.Next) { if (i == SelectedIndex) return n.Content; i++; } return null; }
        public override void Measure(int aw, int ah) { for (TabNode n = _head; n != null; n = n.Next) n.Content.Measure(aw, ah - TabH); DesiredW = Width >= 0 ? Width : aw; DesiredH = Height >= 0 ? Height : ah; }
        public override void Arrange(int x, int y, int w, int h) { X = x; Y = y; W = w; H = h; for (TabNode n = _head; n != null; n = n.Next) n.Content.Arrange(x + 2, y + TabH + 2, w - 4, h - TabH - 4); }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            int tx = X, i = 0;
            for (TabNode n = _head; n != null; n = n.Next)
            {
                int tw = Font.Measure(n.Title) + 16;
                Color bg = i == SelectedIndex ? new Color(0xFF3C5A8C) : new Color(0xFF262A34);
                g.FillRectangle(bg, tx, Y, tw, TabH);
                g.DrawRectangle(new Color(0xFF50505A), tx, Y, tw, TabH);
                g.DrawString(Font, n.Title, Foreground, tx + 8, Y + (TabH - Font.CharH) / 2);
                tx += tw; i++;
            }
            g.FillRectangle(new Color(0xFF1C2028), X, Y + TabH, W, H - TabH);
            g.DrawRectangle(new Color(0xFF50505A), X, Y + TabH, W, H - TabH);
            UIElement c = SelectedContent();
            if (c != null) c.Render(g);
        }
        public override UIElement HitTest(int px, int py)
        {
            if (!Visible || px < X || px >= X + W || py < Y || py >= Y + H) return null;
            if (py < Y + TabH) return this;
            UIElement c = SelectedContent();
            if (c != null) { UIElement h = c.HitTest(px, py); if (h != null) return h; }
            return this;
        }
        public override void MouseDown(int mx, int my)
        {
            if (my >= Y + TabH) return;
            int tx = X, i = 0;
            for (TabNode n = _head; n != null; n = n.Next) { int tw = Font.Measure(n.Title) + 16; if (mx >= tx && mx < tx + tw) { SelectedIndex = i; return; } tx += tw; i++; }
        }
    }

    /// <summary>A node in a <see cref="TreeView"/> (children are a linked list).</summary>
    public sealed class TreeNode
    {
        public string Text;
        public bool Expanded;
        sealed class Kid { public TreeNode N; public Kid Next; }
        Kid _head, _tail;
        int _n;
        public TreeNode(string t) { Text = t; }
        public TreeNode Add(TreeNode c) { Kid k = new Kid(); k.N = c; if (_tail == null) { _head = k; _tail = k; } else { _tail.Next = k; _tail = k; } _n++; return c; }
        public TreeNode AddChild(string t) => Add(new TreeNode(t));
        public int ChildCount => _n;
        public TreeNode ChildAt(int i) { Kid k = _head; while (i-- > 0 && k != null) k = k.Next; return k == null ? null : k.N; }
        public bool HasChildren => _n > 0;
    }

    /// <summary>A collapsible hierarchical tree.</summary>
    public class TreeView : UIElement
    {
        public TreeNode Root2;
        public Font Font;
        public int RowHeight = 14;
        public int Indent = 16;
        public TreeNode Selected;
        public TreeView(Font f) { Font = f; Root2 = new TreeNode(""); }
        public TreeNode AddRoot(string t) => Root2.AddChild(t);

        int CountVisible(TreeNode n) { int c = 0; for (int i = 0; i < n.ChildCount; i++) { TreeNode k = n.ChildAt(i); c++; if (k.Expanded) c += CountVisible(k); } return c; }
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : 170; DesiredH = Height >= 0 ? Height : CountVisible(Root2) * RowHeight + 4; }
        int RenderNode(Graphics g, TreeNode n, int depth, int y)
        {
            int x = X + 4 + depth * Indent;
            if (n == Selected) g.FillRectangle(new Color(0xFF3C5A8C), X + 1, y, W - 2, RowHeight);
            if (n.HasChildren) g.DrawString(Font, n.Expanded ? "-" : "+", Foreground, x, y + (RowHeight - Font.CharH) / 2);
            g.DrawString(Font, n.Text, Foreground, x + 12, y + (RowHeight - Font.CharH) / 2);
            y += RowHeight;
            if (n.Expanded) for (int i = 0; i < n.ChildCount; i++) y = RenderNode(g, n.ChildAt(i), depth + 1, y);
            return y;
        }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRectangle(new Color(0xFF181C24), X, Y, W, H);
            g.DrawRectangle(new Color(0xFF50505A), X, Y, W, H);
            int y = Y + 2;
            for (int i = 0; i < Root2.ChildCount; i++) y = RenderNode(g, Root2.ChildAt(i), 0, y);
        }
        TreeNode _rowNode; int _rowDepth;
        void Walk(TreeNode n, int depth, int[] c)
        {
            if (_rowNode != null) return;
            if (c[0] == 0) { _rowNode = n; _rowDepth = depth; return; }
            c[0]--;
            if (n.Expanded) for (int i = 0; i < n.ChildCount; i++) { Walk(n.ChildAt(i), depth + 1, c); if (_rowNode != null) return; }
        }
        public override void MouseDown(int mx, int my)
        {
            int row = (my - Y - 2) / RowHeight; if (row < 0) return;
            _rowNode = null; int[] c = new int[1]; c[0] = row;
            for (int i = 0; i < Root2.ChildCount; i++) { Walk(Root2.ChildAt(i), 0, c); if (_rowNode != null) break; }
            if (_rowNode == null) return;
            int expX = X + 4 + _rowDepth * Indent;
            if (_rowNode.HasChildren && mx >= expX && mx < expX + 12) _rowNode.Expanded = !_rowNode.Expanded;
            else Selected = _rowNode;
        }
    }

    /// <summary>Clips a taller content element and scrolls it vertically.</summary>
    public class ScrollViewer : UIElement
    {
        public UIElement Content;
        public int Offset;
        public int BarW = 8;
        public void SetContent(UIElement c) { Content = c; }
        int ContentH => Content != null ? Content.DesiredH : 0;
        int MaxOffset() { int m = ContentH - H; return m > 0 ? m : 0; }
        public override void Measure(int aw, int ah) { if (Content != null) Content.Measure(aw - BarW, 1000000); DesiredW = Width >= 0 ? Width : aw; DesiredH = Height >= 0 ? Height : ah; }
        public override void Arrange(int x, int y, int w, int h)
        {
            X = x; Y = y; W = w; H = h;
            int max = MaxOffset(); if (Offset > max) Offset = max; if (Offset < 0) Offset = 0;
            if (Content != null) Content.Arrange(x, y - Offset, w - BarW, ContentH);
        }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRectangle(Background.A > 0 ? Background : new Color(0xFF141820), X, Y, W, H);
            if (Content != null) { g.SetClip(X, Y, W - BarW, H); Content.Render(g); g.ResetClip(); }
            int trackX = X + W - BarW;
            g.FillRectangle(new Color(0xFF20242E), trackX, Y, BarW, H);
            int ch = ContentH > 0 ? ContentH : 1;
            int thumbH = H * H / (ch > H ? ch : H); if (thumbH < 10) thumbH = 10; if (thumbH > H) thumbH = H;
            int max = MaxOffset();
            int thumbY = Y + (max > 0 ? Offset * (H - thumbH) / max : 0);
            g.FillRectangle(new Color(0xFF5A5A6E), trackX, thumbY, BarW, thumbH);
        }
        public override UIElement HitTest(int px, int py)
        {
            if (!Visible || px < X || px >= X + W || py < Y || py >= Y + H) return null;
            if (px >= X + W - BarW) return this;
            if (Content != null) { UIElement h = Content.HitTest(px, py); if (h != null) return h; }
            return this;
        }
        public override void MouseDown(int mx, int my)
        {
            if (mx >= X + W - BarW) { int max = MaxOffset(); Offset = H > 0 ? (my - Y) * max / H : 0; if (Offset < 0) Offset = 0; if (Offset > max) Offset = max; Arrange(X, Y, W, H); }
        }
        public void ScrollBy(int d) { Offset += d; Arrange(X, Y, W, H); }
    }

    /// <summary>A simple tabular data grid with a header row and selectable rows.</summary>
    public class DataGrid : UIElement
    {
        sealed class ColNode { public string Name; public int Width; public ColNode Next; }
        /// <summary>One data row; append cells with <see cref="Cell"/>.</summary>
        public sealed class Row
        {
            sealed class CellNode { public string Text; public CellNode Next; }
            CellNode _h, _t;
            public Row Cell(string s) { CellNode c = new CellNode(); c.Text = s; if (_t == null) { _h = c; _t = c; } else { _t.Next = c; _t = c; } return this; }
            internal string At(int i) { CellNode c = _h; while (i-- > 0 && c != null) c = c.Next; return c == null ? "" : c.Text; }
            internal Row Next;
        }
        ColNode _ch, _ct; int _ncol;
        Row _rh, _rt; int _nrow;
        public int SelectedRow = -1;
        public Font Font;
        public int RowHeight = 14;
        public DataGrid(Font f) { Font = f; }
        public void AddColumn(string name, int w) { ColNode c = new ColNode(); c.Name = name; c.Width = w; if (_ct == null) { _ch = c; _ct = c; } else { _ct.Next = c; _ct = c; } _ncol++; }
        public Row AddRow() { Row r = new Row(); if (_rt == null) { _rh = r; _rt = r; } else { _rt.Next = r; _rt = r; } _nrow++; return r; }
        public int RowCount => _nrow;
        int TotalW() { int w = 0; for (ColNode c = _ch; c != null; c = c.Next) w += c.Width; return w; }
        public override void Measure(int aw, int ah) { DesiredW = Width >= 0 ? Width : TotalW(); DesiredH = Height >= 0 ? Height : (_nrow + 1) * RowHeight + 2; }
        public override void Render(Graphics g)
        {
            if (!Visible) return;
            g.FillRectangle(new Color(0xFF181C24), X, Y, W, H);
            // header
            g.FillRectangle(new Color(0xFF2A3040), X, Y, W, RowHeight);
            int cx = X;
            for (ColNode c = _ch; c != null; c = c.Next) { g.DrawString(Font, c.Name, new Color(0xFFC8D2E6), cx + 3, Y + (RowHeight - Font.CharH) / 2); cx += c.Width; g.FillRectangle(new Color(0xFF3A4050), cx - 1, Y, 1, H); }
            // rows
            int ry = Y + RowHeight, ri = 0;
            for (Row r = _rh; r != null; r = r.Next)
            {
                if (ri == SelectedRow) g.FillRectangle(new Color(0xFF3C5A8C), X, ry, W, RowHeight);
                int col = 0; int xx = X;
                for (ColNode c = _ch; c != null; c = c.Next) { g.DrawString(Font, r.At(col), Foreground, xx + 3, ry + (RowHeight - Font.CharH) / 2); xx += c.Width; col++; }
                ry += RowHeight; ri++;
            }
            g.DrawRectangle(new Color(0xFF50505A), X, Y, W, H);
        }
        public override void MouseDown(int mx, int my)
        {
            int row = (my - Y) / RowHeight - 1; // -1 for the header
            if (row >= 0 && row < _nrow) SelectedRow = row;
        }
    }

    /// <summary>Hosts a UI tree: runs layout, renders to a Bitmap, blits to a window.</summary>
    public sealed class UIHost
    {
        public UIElement Root;
        public Bitmap Surface;
        Window _win;
        Graphics _g;
        public UIHost(string title, int w, int h)
        {
            Surface = new Bitmap(w, h);
            _g = new Graphics(Surface);
            _win = Window.Create(title, w, h);
        }
        public void Layout()
        {
            if (Root == null) return;
            Root.Measure(Surface.Width, Surface.Height);
            Root.Arrange(0, 0, Surface.Width, Surface.Height);
        }
        public void Render(Color clear)
        {
            _g.Clear(clear);
            if (Root != null) { Root.Render(_g); WalkPopupRender(Root); }
        }
        void WalkPopupRender(UIElement e)
        {
            e.RenderPopup(_g);
            for (int i = 0; i < e.ChildCount; i++) WalkPopupRender(e.Child(i));
        }
        UIElement WalkPopupHit(UIElement e, int px, int py)
        {
            for (int i = 0; i < e.ChildCount; i++) { UIElement r = WalkPopupHit(e.Child(i), px, py); if (r != null) return r; }
            return e.PopupHitTest(px, py);
        }
        public void Present() { _win.Blit(Surface); _win.Present(); }
        public UIElement HitTest(int x, int y) { return Root == null ? null : Root.HitTest(x, y); }

        // Input routing: hover tracking + press/capture/click.
        UIElement _hover, _capture;
        bool _wasDown;
        public void Mouse(int mx, int my, bool down)
        {
            // Popups (open dropdowns) get first shot, then the regular tree.
            UIElement hit = Root == null ? null : WalkPopupHit(Root, mx, my);
            if (hit == null) hit = HitTest(mx, my);
            if (hit != _hover)
            {
                if (_hover != null) _hover.MouseLeave();
                if (hit != null) hit.MouseEnter();
                _hover = hit;
            }
            if (down && !_wasDown) { _capture = hit; if (hit != null) hit.MouseDown(mx, my); }
            else if (!down && _wasDown) { if (_capture != null) _capture.MouseUp(mx, my); _capture = null; }
            else if (down && _capture != null) _capture.MouseMove(mx, my);
            _wasDown = down;
        }
    }
}
