// __APPNAME__ -- the OS's built-in AI from C#, made with bz new.
// Buitenzorg.AI (in BZAPP.DLL): LanguageModel.Load() opens /disk/STORIES.GGU
// (a small story model) on the GPU compute backend when there is one, else
// the CPU; Begin(...).Next() yields the answer piece by piece. Vision.Edges /
// Grayscale / Histogram work on int[] ARGB pixels. Nothing leaves the machine.
// On the PC (dotnet ... --selftest) a simulated model answers.

using Buitenzorg.AI;
using Buitenzorg.UI;

namespace __NAME83__;

public sealed class AiWindow
{
    public readonly Window W;
    public readonly LanguageModel? Model;
    public readonly TextBox Prompt = new TextBox { Watermark = "Once upon a time" };
    public readonly TextBlock Answer = new TextBlock("") { TextWrapping = TextWrapping.Wrap };
    readonly TextBlock _status = new TextBlock("");
    readonly DispatcherTimer _timer = new DispatcherTimer(30);
    Generation? _gen;

    public AiWindow(bool headless = false)
    {
        W = new Window("__APPNAME__", 520, 380, headless);
        Model = LanguageModel.Load();
        _status.Text = Model == null
            ? "Model tidak ada: " + LanguageModel.LastError
            : $"{Model.Parameters / 1000} ribu parameter di {Model.Backend}";

        var write = new Button(new TextBlock("Tulis"), _ => Start()).WithClass("accent");
        var root = new DockPanel { Margin = new Thickness(20) };
        root.Children.Add(new TextBlock("__APPNAME__").WithClass("eyebrow").DockTo(Dock.Top));
        root.Children.Add(Prompt.DockTo(Dock.Top));
        root.Children.Add(write.DockTo(Dock.Top));
        root.Children.Add(_status.WithClass("faint").DockTo(Dock.Bottom));
        root.Children.Add(new ScrollViewer { Content = Answer, Margin = new Thickness(0, 10, 0, 0) });
        W.Content = root;

        // one piece per tick: the text appears while it is written
        _timer.Tick += () => Step();
        Application.Current?.Apply(W);
        W.UpdateLayout();
    }

    public void Start()
    {
        if (Model == null) return;
        string p = Prompt.Text.Trim().Length == 0 ? "Once upon a time" : Prompt.Text.Trim();
        Answer.Text = p;
        _gen = Model.Begin(p, maxTokens: 64);
        _timer.Start();
    }

    public bool Step()
    {
        var piece = _gen?.Next();
        if (piece == null)
        {
            _timer.Stop();
            if (_gen != null && Model != null)
                _status.Text = $"{Model.LastTokens} token, {(int)Model.LastTokensPerSecond} token/detik di {Model.Backend}";
            _gen = null;
            return false;
        }
        Answer.Text += piece;
        return true;
    }

    public static Checks SelfTest()
    {
        var c = new Checks("__APPNAME__");
        var a = new AiWindow(headless: true);
        c.That(a.Model != null, "model loaded: " + (a.Model?.Backend ?? LanguageModel.LastError));
        a.Prompt.Text = "Once upon a time";
        a.Start();
        while (a.Step()) { }
        c.That(a.Answer.Text.Length > "Once upon a time".Length, "answer: " + a.Answer.Text);
        // vision: a white square on black -> bright edges on its border
        var px = new int[16 * 16];
        for (int y = 4; y < 12; y++) for (int x = 4; x < 12; x++) px[y * 16 + x] = 0xFFFFFF;
        var e = Vision.Edges(px, 16, 16);
        c.That((e[4 * 16 + 8] & 0xFF) > 100 && (e[8 * 16 + 8] & 0xFF) == 0, "Sobel edges");
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
            var c = AiWindow.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        app.Run(new AiWindow().W);
        return 0;
    }
}