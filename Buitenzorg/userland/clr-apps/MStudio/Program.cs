// Buitenzorg Studio (MSTUDIO.DLL, `run mstudio`): see Studio.cs.

using Buitenzorg.UI;
using Files = Buitenzorg.App.Files;

namespace MStudio;

public static class Program
{
    /// <summary>Projects persist on /disk when it is there.</summary>
    public static string ProjectRoot()
    {
        foreach (var m in Files.List("")) if (m.Name == "disk") return "/disk/PROYEK";
        return Files.ScratchDirectory + "/PROYEK";
    }

    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = Studio.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var st = new Studio(ProjectRoot(), headless: false);
        // a first start: one example project so the editor is not empty
        if (st.Entries.Count == 0) st.CreateProject(Templates.Find("js-halo")!, "HALO");
        app.Run(st.W);
        return 0;
    }
}
