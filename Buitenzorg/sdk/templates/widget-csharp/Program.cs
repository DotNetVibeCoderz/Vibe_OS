// __APPNAME__ -- a desktop widget made with bz new widget-csharp.
// A window whose title starts with "widget:" is docked on the desktop's
// widget board (no title bar, not in the taskbar, not tiled). This one shows
// the time and the date and refreshes once a second -- keep widgets quiet:
// every redraw recomposes the desktop.

using Buitenzorg.UI;

namespace __NAME83__;

public sealed class ClockWidget
{
    public readonly Window W;
    public readonly TextBlock Time = new TextBlock("--:--");
    public readonly TextBlock Date = new TextBlock("");
    readonly DispatcherTimer _timer = new DispatcherTimer(1000);

    public ClockWidget(bool headless = false)
    {
        W = new Window("widget:__APPNAME__", 220, 96, headless);
        var root = new StackPanel { Orientation = Orientation.Vertical, Margin = new Thickness(14), Spacing = 4 };
        root.Children.Add(Time.WithClass("h1"));
        root.Children.Add(Date.WithClass("faint"));
        W.Content = root;
        Refresh(DateTime.Now);
        _timer.Tick += () => Refresh(DateTime.Now);
        if (!headless) _timer.Start();
    }

    static readonly string[] Days = { "Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu" };

    public void Refresh(DateTime now)
    {
        string t = Two(now.Hour) + ":" + Two(now.Minute);
        if (t != Time.Text) Time.Text = t; // only redraw when it changed
        Date.Text = Days[(int)now.DayOfWeek] + ", " + now.Day + "/" + now.Month + "/" + now.Year;
    }

    static string Two(int v) => v < 10 ? "0" + v : v.ToString();

    public static Checks SelfTest()
    {
        var c = new Checks("__APPNAME__");
        var w = new ClockWidget(headless: true);
        c.That(w.W.GetValue(Window.TitleProperty).StartsWith("widget:"), "docked on the widget board");
        w.Refresh(new DateTime(2026, 11, 22, 7, 5, 0));
        c.That(w.Time.Text == "07:05" && w.Date.Text.StartsWith("Minggu"), "time and day: " + w.Time.Text + " " + w.Date.Text);
        w.W.RenderFrame();
        c.That(w.W.LastFrameOps > 1, "a frame drew " + w.W.LastFrameOps + " ops");
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
            var c = ClockWidget.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        app.Run(new ClockWidget().W);
        return 0;
    }
}
