// Paint -- the managed drawing app: draw with the mouse on a canvas, pick a
// colour from the palette and a brush size, undo the last stroke, clear.
// Strokes are kept as line segments and redrawn with the kernel's line op
// (no per-pixel work in the interpreter). Buitenzorg.UI.

using Buitenzorg.UI;

namespace MPaint;

public sealed class Stroke
{
    public int Color, Size;
    public readonly List<(int X, int Y)> Points = new();
}

/// <summary>The drawing surface.</summary>
public sealed class PaintCanvas : Control
{
    public readonly List<Stroke> Strokes = new();
    public int Color = 0x2E7D5B;
    public int BrushSize = 3;
    Stroke? _current;

    public PaintCanvas() { Focusable = true; }

    protected override Size MeasureOverride(Size a) => new Size(Math.Max(200, a.Width), Math.Max(150, a.Height));

    static void Thick(DrawingContext dc, int x0, int y0, int x1, int y1, int size, int color)
    {
        int r = size / 2;
        for (int d = -r; d <= r; d++)
        {
            if (Math.Abs(y1 - y0) > Math.Abs(x1 - x0)) dc.Line(x0 + d, y0, x1 + d, y1, color);
            else dc.Line(x0, y0 + d, x1, y1 + d, color);
        }
    }

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        dc.FillRoundRect(b, 8, 0xFFFFFF);
        dc.PushClip(b);
        foreach (var s in Strokes)
        {
            if (s.Points.Count == 1) dc.FillRect(new Rect(b.X + s.Points[0].X - s.Size / 2, b.Y + s.Points[0].Y - s.Size / 2, s.Size, s.Size), s.Color);
            for (int i = 1; i < s.Points.Count; i++)
                Thick(dc, b.X + s.Points[i - 1].X, b.Y + s.Points[i - 1].Y, b.X + s.Points[i].X, b.Y + s.Points[i].Y, s.Size, s.Color);
        }
        dc.PopClip();
    }

    public void Begin(int x, int y)
    {
        _current = new Stroke { Color = Color, Size = BrushSize };
        _current.Points.Add((x, y));
        Strokes.Add(_current);
        InvalidateVisual();
    }

    public void Extend(int x, int y)
    {
        if (_current == null) return;
        var last = _current.Points[^1];
        if (Math.Abs(last.X - x) + Math.Abs(last.Y - y) < 3) return; // skip tiny moves
        _current.Points.Add((x, y));
        InvalidateVisual();
    }

    public void End() => _current = null;

    public void Undo() { if (Strokes.Count > 0) { Strokes.RemoveAt(Strokes.Count - 1); InvalidateVisual(); } }
    public void ClearAll() { Strokes.Clear(); InvalidateVisual(); }

    protected override void OnPointerPressed(PointerEventArgs e) { Focus(); Begin(e.X - Bounds.X, e.Y - Bounds.Y); e.Handled = true; }
    protected override void OnPointerMoved(PointerEventArgs e) { if (e.LeftButton) Extend(e.X - Bounds.X, e.Y - Bounds.Y); base.OnPointerMoved(e); }
    protected override void OnPointerReleased(PointerEventArgs e) { End(); base.OnPointerReleased(e); }
}

public static class PaintApp
{
    public static readonly int[] Palette = { 0x1F2A24, 0x2E7D5B, 0x3E63DD, 0xE5484D, 0xF5D90A, 0xF0844B, 0xC792EA, 0x8B8D98 };

    /// <summary>A round colour swatch that selects its colour.</summary>
    public sealed class Swatch : Control
    {
        public readonly int Value;
        public bool Selected;
        readonly Action<Swatch> _pick;
        public Swatch(int value, Action<Swatch> pick) { Value = value; _pick = pick; }
        protected override Size MeasureOverride(Size a) => new Size(28, 28);
        public override void Render(DrawingContext dc)
        {
            var b = Bounds;
            if (Selected) dc.Ellipse(b, Theme.Text, true);
            dc.Ellipse(b.Inflate(Selected ? -3 : -2), Value, true);
        }
        protected override void OnPointerPressed(PointerEventArgs e) { _pick(this); e.Handled = true; }
    }

    public static (Window, PaintCanvas, List<Swatch>) Build(bool headless = false)
    {
        var w = new Window("Paint", 720, 520, headless);
        var canvas = new PaintCanvas();
        var swatches = new List<Swatch>();
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(0, 0, 0, 10) };
        foreach (var color in Palette)
        {
            var s = new Swatch(color, sw =>
            {
                foreach (var o in swatches) { o.Selected = o == sw; o.InvalidateVisual(); }
                canvas.Color = sw.Value;
            }) { Selected = color == canvas.Color };
            swatches.Add(s);
            tools.Children.Add(s);
        }
        var size = new Slider { Minimum = 1, Maximum = 15, Value = canvas.BrushSize, TickFrequency = 1, IsSnapToTickEnabled = true, Width = 120, Margin = new Thickness(12, 4, 0, 0) };
        size.ValueChanged += s => canvas.BrushSize = (int)s.Value;
        tools.Children.Add(new TextBlock("Kuas") { Margin = new Thickness(12, 6, 0, 0) });
        tools.Children.Add(size);
        var undo = new Button(new TextBlock("Urungkan"));
        undo.Click += _ => canvas.Undo();
        var clear = new Button(new TextBlock("Bersihkan"));
        clear.Click += _ => canvas.ClearAll();
        tools.Children.Add(undo);
        tools.Children.Add(clear);

        var root = new DockPanel { Margin = new Thickness(16) };
        root.Children.Add(Kit.Header("Multimedia", "Paint", IconKind.Leaf, 0xC792EA).DockTo(Dock.Top));
        root.Children.Add(tools.DockTo(Dock.Top));
        root.Children.Add(canvas);
        w.Content = root;
        Application.Current?.Apply(w);
        w.UpdateLayout();
        return (w, canvas, swatches);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Paint");
        var (w, canvas, swatches) = Build(headless: true);
        var b = canvas.Bounds;
        w.SimulatePointer(1, b.X + 20, b.Y + 20);
        w.SimulatePointer(2, b.X + 20, b.Y + 20);  // press
        w.SimulatePointer(1, b.X + 120, b.Y + 60); // drag
        w.SimulatePointer(1, b.X + 200, b.Y + 140);
        w.SimulatePointer(3, b.X + 200, b.Y + 140); // release
        c.That(canvas.Strokes.Count == 1 && canvas.Strokes[0].Points.Count >= 3, "a stroke drawn with the mouse (" + (canvas.Strokes.Count > 0 ? canvas.Strokes[0].Points.Count : 0) + " points)");
        var red = swatches.Find(s => s.Value == 0xE5484D)!;
        w.SimulateClick(red);
        c.That(canvas.Color == 0xE5484D && red.Selected, "palette picks red");
        canvas.Begin(10, 10); canvas.Extend(60, 60); canvas.End();
        c.That(canvas.Strokes.Count == 2 && canvas.Strokes[1].Color == 0xE5484D, "next stroke is red");
        canvas.Undo();
        c.That(canvas.Strokes.Count == 1, "undo removes the last stroke");
        canvas.ClearAll();
        c.That(canvas.Strokes.Count == 0, "clear");
        w.RenderFrame();
        c.That(w.LastFrameOps > 10, "frame drew " + w.LastFrameOps + " ops");
        return c;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = PaintApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _, _) = PaintApp.Build();
        app.Run(w);
        return 0;
    }
}
