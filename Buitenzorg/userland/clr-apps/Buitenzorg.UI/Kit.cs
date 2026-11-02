// Buitenzorg.UI -- the shared pieces of the managed apps' look: the app
// header (eyebrow + title + glyph tile), stat cards, and the self-test helper.

namespace Buitenzorg.UI;

/// <summary>A rounded tile with a glyph in it: app icons, list leading icons.</summary>
public sealed class GlyphTile : Control
{
    public IconKind Kind { get; set; }
    public int Tint { get; set; } = 0x7CC4A4;
    public int TileSize { get; set; } = 36;

    public GlyphTile(IconKind kind, int tint, int size = 36)
    {
        Kind = kind; Tint = tint; TileSize = size;
        IsHitTestVisible = false;
    }

    protected override Size MeasureOverride(Size a) => new Size(TileSize, TileSize);

    public override void Render(DrawingContext dc)
    {
        var b = Bounds;
        var r = new Rect(b.X + (b.Width - TileSize) / 2, b.Y + (b.Height - TileSize) / 2, TileSize, TileSize);
        dc.FillGradient(r, TileSize / 4, Color.Lighten(Tint, 40), Color.Darken(Tint, 30));
        // a highlight along the top edge, like light on a leaf
        dc.BlendRoundRect(new Rect(r.X + 3, r.Y + 2, r.Width - 6, r.Height / 3), TileSize / 6, 0xFFFFFF, 40);
        int g = TileSize / 2;
        Icon.Draw(dc, Kind, new Rect(r.X + (TileSize - g) / 2, r.Y + (TileSize - g) / 2, g, g), 0x0B1A12);
    }
}

public static class Kit
{
    /// <summary>The header strip of an app: a glyph tile, an eyebrow (what kind of app) and the title.</summary>
    public static Control Header(string eyebrow, string title, IconKind glyph, int tint, Control? trailing = null)
    {
        var g = new Grid { ColumnDefinitions = "Auto,*,Auto", ColumnSpacing = 12, Margin = new Thickness(0, 0, 0, 12) };
        g.Children.Add(new GlyphTile(glyph, tint, 40).Col(0));
        var text = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        text.Children.Add(new TextBlock(eyebrow.ToUpperInvariant()).WithClass("eyebrow"));
        text.Children.Add(new TextBlock(title).WithClass("h1"));
        g.Children.Add(text.Col(1));
        if (trailing != null)
        {
            trailing.VerticalAlignment = VerticalAlignment.Center;
            g.Children.Add(trailing.Col(2));
        }
        return g;
    }

    /// <summary>A small card: an eyebrow label over a value (bindable through <paramref name="value"/>).</summary>
    public static Border Stat(string label, TextBlock value, Control? extra = null)
    {
        var s = new StackPanel { Spacing = 4 };
        s.Children.Add(new TextBlock(label.ToUpperInvariant()).WithClass("faint"));
        value.Classes.Add("h2");
        s.Children.Add(value);
        if (extra != null) s.Children.Add(extra);
        var b = new Border { Child = s, Padding = new Thickness(12, 10) };
        b.Classes.Add("card");
        return b;
    }
}

/// <summary>Self-test bookkeeping: prints each check, remembers failures.</summary>
public sealed class Checks
{
    readonly string _app;
    public int Passed { get; private set; }
    public int Failed { get; private set; }
    public Checks(string app) { _app = app; }

    public void That(bool ok, string what)
    {
        if (ok) Passed++;
        else { Failed++; Console.WriteLine(_app + ": FAIL " + what); }
    }

    public void Equal(string expected, string actual, string what) =>
        That(expected == actual, what + " (expected '" + expected + "', got '" + actual + "')");

    public bool Ok => Failed == 0;
    public override string ToString() => _app + ": " + Passed + " checks, " + Failed + " failed";
}
