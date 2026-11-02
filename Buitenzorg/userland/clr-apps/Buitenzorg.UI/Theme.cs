// Buitenzorg.UI -- the "Kebun" theme (Avalonia's FluentTheme counterpart):
// design tokens for a dark (forest floor) and a light (morning glasshouse)
// variant, and the default styles of every control. The signature: leaf-green
// accents, and an amber "sunfleck" focus ring -- light falling through the
// canopy -- marking where the keyboard is.

namespace Buitenzorg.UI;

public static class Theme
{
    // ---- tokens (current variant)
    public static int Bg, BgBottom, Surface, SurfaceHi, SurfaceLo, Stroke, StrokeSoft;
    public static int Text, TextMuted, TextFaint;
    public static int Accent, AccentHi, AccentLo, OnAccent;
    public static int Sun, Danger, Info, FocusRing, Overlay;
    public static ThemeVariant Variant { get; private set; }

    static readonly List<Style> _themeStyles = new List<Style>();

    public static void Use(ThemeVariant v)
    {
        Variant = v;
        if (v == ThemeVariant.Dark)
        {
            Bg = 0x0F2219; BgBottom = 0x0A1811;
            Surface = 0x16301F; SurfaceHi = 0x1E4130; SurfaceLo = 0x0C1D14;
            Stroke = 0x2E5B45; StrokeSoft = 0x204234;
            Text = 0xE9F2EB; TextMuted = 0x9DBBA9; TextFaint = 0x6A8A78;
            Accent = 0x7CC4A4; AccentHi = 0x9BD8BC; AccentLo = 0x55A381; OnAccent = 0x0A1C13;
            Sun = 0xE9B44C; Danger = 0xE5735F; Info = 0x7FB2D9; FocusRing = 0xE9B44C; Overlay = 0x000000;
        }
        else
        {
            Bg = 0xF3F7F0; BgBottom = 0xE4EDE1;
            Surface = 0xFFFFFF; SurfaceHi = 0xF6FAF4; SurfaceLo = 0xE8F0E5;
            Stroke = 0xC4D6C9; StrokeSoft = 0xD9E5DC;
            Text = 0x15271D; TextMuted = 0x4D6A59; TextFaint = 0x84A090;
            Accent = 0x2F8A63; AccentHi = 0x3FA176; AccentLo = 0x22704F; OnAccent = 0xFFFFFF;
            Sun = 0xC98A12; Danger = 0xC4462F; Info = 0x2F6FA8; FocusRing = 0xC98A12; Overlay = 0x15271D;
        }
    }

    internal static bool IsThemeStyle(Style s) => _themeStyles.Contains(s);

    static BoxShadow Elevation(int level) =>
        Variant == ThemeVariant.Dark ? new BoxShadow(0, 2 * level, 4 * level, 0x000000, 70 + 20 * level)
                                     : new BoxShadow(0, level + 1, 3 * level + 2, 0x2A4535, 30 + 10 * level);

    public static List<Style> BuildStyles()
    {
        _themeStyles.Clear();
        var all = new List<Style>
        {
            new Style("Window") { { Control.BackgroundProperty, Bg }, { Control.BackgroundBottomProperty, BgBottom }, { Control.ForegroundProperty, Text } },

            // text
            new Style("TextBlock.muted, Label.muted") { { Control.ForegroundProperty, TextMuted } },
            new Style("TextBlock.faint") { { Control.ForegroundProperty, TextFaint } },
            new Style("TextBlock.accent") { { Control.ForegroundProperty, Accent } },
            new Style("TextBlock.sun") { { Control.ForegroundProperty, Sun } },
            new Style("TextBlock.h1") { { Control.FontSizeProperty, 24 } },
            new Style("TextBlock.h2, TextBlock.strong") { { Control.FontWeightProperty, FontWeight.Bold } },
            new Style("TextBlock.eyebrow") { { Control.ForegroundProperty, Accent }, { Control.FontWeightProperty, FontWeight.Bold } },

            // surfaces
            new Style("Border.card") { { Control.BackgroundProperty, Surface }, { Control.CornerRadiusProperty, 12 }, { Control.BorderBrushProperty, StrokeSoft },
                { Control.BorderThicknessProperty, 1 }, { Control.PaddingProperty, new Thickness(14) }, { Control.BoxShadowProperty, Elevation(1) } },
            new Style("Border.inset") { { Control.BackgroundProperty, SurfaceLo }, { Control.CornerRadiusProperty, 10 }, { Control.PaddingProperty, new Thickness(10) } },
            new Style("Border.toolbar") { { Control.BackgroundProperty, Surface }, { Control.BorderBrushProperty, StrokeSoft }, { Control.PaddingProperty, new Thickness(10, 6) } },
            new Style("Separator") { { Control.BackgroundProperty, StrokeSoft }, { Control.MarginProperty, new Thickness(0, 6) } },

            // buttons
            new Style("Button, RepeatButton, ToggleButton, DropDownButton, SplitButton, CalendarDayButton") {
                { Control.BackgroundProperty, SurfaceHi }, { Control.BackgroundBottomProperty, Surface }, { Control.BorderBrushProperty, Stroke },
                { Control.BorderThicknessProperty, 1 }, { Control.CornerRadiusProperty, 8 }, { Control.PaddingProperty, new Thickness(14, 7) },
                { Control.HorizontalAlignmentProperty, HorizontalAlignment.Left }, { Control.VerticalAlignmentProperty, VerticalAlignment.Center } },
            new Style("Button:pointerover, RepeatButton:pointerover, ToggleButton:pointerover, DropDownButton:pointerover, SplitButton:pointerover, CalendarDayButton:pointerover") {
                { Control.BackgroundProperty, Color.Mix(SurfaceHi, Accent, 40) }, { Control.BorderBrushProperty, Color.Mix(Stroke, Accent, 90) } },
            new Style("Button:pressed, RepeatButton:pressed, ToggleButton:pressed, DropDownButton:pressed") {
                { Control.BackgroundProperty, SurfaceLo }, { Control.BackgroundBottomProperty, Surface } },
            new Style("Button:disabled, RepeatButton:disabled, ToggleButton:disabled, DropDownButton:disabled") { { Control.ForegroundProperty, TextFaint },
                { Control.BackgroundProperty, Surface }, { Control.BorderBrushProperty, StrokeSoft } },
            new Style(":is(Button).accent, ToggleButton:checked") { { Control.BackgroundProperty, AccentHi }, { Control.BackgroundBottomProperty, Accent },
                { Control.ForegroundProperty, OnAccent }, { Control.BorderBrushProperty, AccentLo } },
            new Style(":is(Button).accent:pointerover, ToggleButton:checked:pointerover") { { Control.BackgroundProperty, Color.Lighten(AccentHi, 40) } },
            new Style(":is(Button).accent:pressed, ToggleButton:checked:pressed") { { Control.BackgroundProperty, Accent }, { Control.BackgroundBottomProperty, AccentLo } },
            new Style(":is(Button).sun") { { Control.BackgroundProperty, Color.Lighten(Sun, 30) }, { Control.BackgroundBottomProperty, Sun },
                { Control.ForegroundProperty, 0x2A1C04 }, { Control.BorderBrushProperty, Color.Darken(Sun, 40) } },
            new Style(":is(Button).danger") { { Control.ForegroundProperty, Danger } },
            new Style(":is(Button).danger:pointerover") { { Control.BackgroundProperty, Color.Mix(SurfaceHi, Danger, 50) } },
            new Style(":is(Button).subtle, :is(Button).link") { { Control.BackgroundProperty, Color.Transparent }, { Control.BackgroundBottomProperty, Color.Transparent },
                { Control.BorderThicknessProperty, 0 } },
            new Style(":is(Button).subtle:pointerover") { { Control.BackgroundProperty, SurfaceHi } },
            new Style(":is(Button).subtle:pressed") { { Control.BackgroundProperty, SurfaceLo } },
            new Style("HyperlinkButton") { { Control.ForegroundProperty, Accent }, { Control.PaddingProperty, new Thickness(2) },
                { Control.HorizontalAlignmentProperty, HorizontalAlignment.Left } },
            new Style("HyperlinkButton:pointerover") { { Control.ForegroundProperty, AccentHi } },

            // toggles
            new Style("CheckBox, RadioButton, ToggleSwitch") { { Control.PaddingProperty, new Thickness(0, 2) }, { Control.HorizontalAlignmentProperty, HorizontalAlignment.Left },
                { Control.BorderBrushProperty, Stroke }, { Control.BackgroundProperty, SurfaceLo } },
            new Style("CheckBox:pointerover, RadioButton:pointerover, ToggleSwitch:pointerover") { { Control.BorderBrushProperty, Accent } },
            new Style("CheckBox:disabled, RadioButton:disabled, ToggleSwitch:disabled") { { Control.ForegroundProperty, TextFaint } },

            // text input
            new Style("TextBox, AutoCompleteBox, NumericUpDown, CalendarDatePicker, ComboBox") {
                { Control.BackgroundProperty, SurfaceLo }, { Control.BorderBrushProperty, Stroke }, { Control.BorderThicknessProperty, 1 },
                { Control.CornerRadiusProperty, 7 }, { Control.PaddingProperty, new Thickness(9, 6) } },
            new Style("TextBox:pointerover, ComboBox:pointerover, CalendarDatePicker:pointerover") { { Control.BorderBrushProperty, Color.Mix(Stroke, Accent, 120) } },
            new Style("TextBox:focus, ComboBox:focus, CalendarDatePicker:focus, AutoCompleteBox:focus, NumericUpDown:focus") { { Control.BorderBrushProperty, Accent } },
            new Style("TextBox:disabled, ComboBox:disabled") { { Control.ForegroundProperty, TextFaint } },
            new Style("NumericUpDown TextBox, AutoCompleteBox TextBox") { { Control.BorderThicknessProperty, 0 }, { Control.BackgroundProperty, Color.Transparent },
                { Control.PaddingProperty, new Thickness(0) } },

            // range
            new Style("Slider") { { Control.BackgroundProperty, StrokeSoft }, { Control.HeightProperty, 24 } },
            new Style("ProgressBar") { { Control.BackgroundProperty, SurfaceLo }, { Control.HeightProperty, 8 }, { Control.CornerRadiusProperty, 4 } },

            // items
            new Style("ListBox, TreeView, DataGrid") { { Control.BackgroundProperty, SurfaceLo }, { Control.CornerRadiusProperty, 10 },
                { Control.BorderBrushProperty, StrokeSoft }, { Control.BorderThicknessProperty, 1 }, { Control.PaddingProperty, new Thickness(4) } },
            new Style("ListBoxItem, ComboBoxItem, TreeViewItemHeader, MenuItem") { { Control.PaddingProperty, new Thickness(10, 6) }, { Control.CornerRadiusProperty, 6 } },
            new Style("ListBoxItem:pointerover, ComboBoxItem:pointerover, TreeViewItemHeader:pointerover, MenuItem:pointerover, DataGridRow:pointerover") {
                { Control.BackgroundProperty, Surface } },
            new Style("ListBoxItem:selected, ComboBoxItem:selected, TreeViewItemHeader:selected, DataGridRow:selected") {
                { Control.BackgroundProperty, SurfaceHi } },
            new Style("MenuItem:disabled") { { Control.ForegroundProperty, TextFaint } },
            new Style("TabItem") { { Control.PaddingProperty, new Thickness(12, 8) }, { Control.ForegroundProperty, TextMuted } },
            new Style("TabItem:pointerover") { { Control.ForegroundProperty, Text } },
            new Style("TabItem:selected") { { Control.ForegroundProperty, Text }, { Control.FontWeightProperty, FontWeight.Bold } },
            new Style("DataGridColumnHeader") { { Control.ForegroundProperty, TextMuted }, { Control.FontWeightProperty, FontWeight.Bold },
                { Control.PaddingProperty, new Thickness(8, 6) } },
            new Style("DataGridColumnHeader:pointerover") { { Control.ForegroundProperty, Text } },
            new Style("DataGridRow") { { Control.CornerRadiusProperty, 5 } },

            // overlays
            new Style("FlyoutPresenter, MenuFlyoutPresenter, ContextMenu, ComboBoxDropDown, MenuDropDown") {
                { Control.BackgroundProperty, SurfaceHi }, { Control.CornerRadiusProperty, 10 }, { Control.BorderBrushProperty, Stroke },
                { Control.BorderThicknessProperty, 1 }, { Control.PaddingProperty, new Thickness(4) }, { Control.BoxShadowProperty, Elevation(3) } },
            new Style("FlyoutPresenter") { { Control.PaddingProperty, new Thickness(12) } },
            new Style("ToolTip") { { Control.BackgroundProperty, Variant == ThemeVariant.Dark ? 0x061009 : 0x1E3327 },
                { Control.ForegroundProperty, 0xE9F2EB }, { Control.CornerRadiusProperty, 6 }, { Control.PaddingProperty, new Thickness(8, 4) },
                { Control.BoxShadowProperty, Elevation(2) } },
            new Style("NotificationCard") { { Control.BackgroundProperty, SurfaceHi }, { Control.CornerRadiusProperty, 10 }, { Control.BorderBrushProperty, Stroke },
                { Control.BorderThicknessProperty, 1 }, { Control.PaddingProperty, new Thickness(12, 10) }, { Control.BoxShadowProperty, Elevation(3) },
                { Control.WidthProperty, 260 } },
            new Style("Menu") { { Control.BackgroundProperty, Surface }, { Control.PaddingProperty, new Thickness(4, 2) } },

            // containers
            new Style("Expander") { { Control.BackgroundProperty, Surface }, { Control.CornerRadiusProperty, 10 }, { Control.BorderBrushProperty, StrokeSoft },
                { Control.BorderThicknessProperty, 1 } },
            new Style("SplitView") { { Control.BackgroundProperty, Color.Transparent } },
            new Style("GridSplitter") { { Control.BackgroundProperty, StrokeSoft } },
            new Style("GridSplitter:pointerover, GridSplitter:pressed") { { Control.BackgroundProperty, Accent } },
            new Style("Calendar") { { Control.BackgroundProperty, Surface }, { Control.CornerRadiusProperty, 10 }, { Control.PaddingProperty, new Thickness(8) } },
            new Style("CalendarDayButton") { { Control.CornerRadiusProperty, 14 }, { Control.HorizontalAlignmentProperty, HorizontalAlignment.Stretch },
                { Control.PaddingProperty, new Thickness(0, 4) } },
            new Style("CalendarDayButton:pointerover") { { Control.BackgroundProperty, SurfaceHi } },
            new Style("CalendarDayButton.today") { { Control.BorderBrushProperty, Sun }, { Control.BorderThicknessProperty, 1 } },
            new Style("CalendarDayButton.other") { { Control.ForegroundProperty, TextFaint } },
            new Style("CalendarDayButton:selected") { { Control.BackgroundProperty, Accent }, { Control.ForegroundProperty, OnAccent } },
        };
        _themeStyles.AddRange(all);
        return all;
    }
}
