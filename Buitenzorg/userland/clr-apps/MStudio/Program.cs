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
        // --list: the catalog; --export DIR: every template's entry file as DIR/Tnn.EXT
        // (screenshots and quick trials from the Terminal: run DIR/T05.JS)
        if (args.Length >= 1 && args[0] == "--list")
        {
            for (int i = 0; i < Templates.Catalog.Length; i++)
                Console.WriteLine((i + 1).ToString("00") + "  " + Templates.Catalog[i].Id + "  " + Templates.Catalog[i]);
            return 0;
        }
        if (args.Length >= 2 && args[0] == "--export")
        {
            string dir = args[1].TrimEnd('/');
            if (Files.GetInfo(dir) == null) Files.CreateDirectory(dir);
            int n = 0;
            for (int i = 0; i < Templates.Catalog.Length; i++)
            {
                var t = Templates.Catalog[i];
                string ext = t.Entry.Substring(t.Entry.LastIndexOf('.'));
                foreach (var (file, text) in t.Files)
                    if (file == t.Entry && Files.WriteAllText(dir + "/T" + (i + 1).ToString("00") + ext, text.TrimStart('\n'))) n++;
            }
            Console.WriteLine("Studio: " + n + " templates written to " + dir);
            return 0;
        }
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
