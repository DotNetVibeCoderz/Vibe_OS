// The first managed C# app on Buitenzorg: an ordinary `dotnet build` output,
// interpreted by RustCLR (CLRHOST.ELF) with a tracing garbage collector.
//
// Every check below uses something the *native* path (bflat --stdlib:zero)
// cannot do -- static reference fields, delegates/lambdas, object[] stores,
// string concatenation, LINQ, exceptions, a GC that reclaims -- which is the
// point of having the managed path next to it.

namespace HelloClr;

interface IShape
{
    double Area();
    string Name { get; }
}

sealed class Rect(double w, double h) : IShape
{
    public double Area() => w * h;
    public string Name => "rect";
}

sealed class Circle(double r) : IShape
{
    public double Area() => 3.0 * r * r; // integer-exact on purpose
    public string Name => "circle";
}

class Node
{
    public Node? Next;
    public int[] Payload = new int[16];
}

static class Program
{
    // A static reference field: garbage under zerolib, normal here.
    static readonly List<string> Log = new();
    static int checks;
    static int failures;

    static void Check(string what, bool ok)
    {
        checks++;
        if (!ok)
        {
            failures++;
            Console.WriteLine("  FAILED: " + what);
        }
        Log.Add(what);
    }

    static int Main(string[] args)
    {
        Console.WriteLine("Hello from managed C# on Buitenzorg (RustCLR)!");

        // Strings: concatenation, interpolation, formatting.
        string os = "Buitenzorg";
        string s = $"{os} v{1}.{0} " + "managed";
        Check("string interpolation + concat", s == "Buitenzorg v1.0 managed");
        Check("string methods", s.ToUpper().StartsWith("BUITENZORG") && s.Split(' ').Length == 3);

        // object[] element stores (stelem.ref) -- faults under zerolib.
        object[] boxes = new object[4];
        for (int i = 0; i < boxes.Length; i++) boxes[i] = i % 2 == 0 ? (object)i : (object)("n" + i);
        Check("object[] stores + boxing", (int)boxes[2] == 2 && (string)boxes[3] == "n3");

        // Interfaces, virtual dispatch, generics.
        var shapes = new List<IShape> { new Rect(2, 3), new Circle(2), new Rect(1, 1) };
        double total = 0;
        foreach (var sh in shapes) total += sh.Area();
        Check("interface dispatch", total == 6 + 12 + 1);

        // Delegates + lambdas + LINQ (a method group cached in a static faults
        // under zerolib).
        Func<int, int> square = x => x * x;
        var evens = Enumerable.Range(1, 20).Where(x => x % 2 == 0).Select(square).ToList();
        Check("LINQ + lambdas", evens.Count == 10 && evens.Sum() == 1540 && evens.Max() == 400);
        var byName = shapes.GroupBy(x => x.Name).ToDictionary(g => g.Key, g => g.Count());
        Check("GroupBy + Dictionary", byName["rect"] == 2 && byName["circle"] == 1);

        // Exceptions: throw / catch / finally.
        int stage = 0;
        try
        {
            stage = 1;
            int[] a = new int[2];
            a[5] = 1;
        }
        catch (IndexOutOfRangeException)
        {
            stage = 2;
        }
        finally
        {
            stage *= 10;
        }
        Check("exceptions + finally", stage == 20);

        // The collector: ~4 MiB of short-lived garbage must be reclaimed.
        long before = GC.CollectionCount(0);
        Node? keep = null;
        for (int i = 0; i < 20000; i++)
        {
            var n = new Node { Next = i % 1000 == 0 ? keep : null };
            if (i % 1000 == 0) keep = n;
        }
        GC.Collect();
        int chain = 0;
        for (var n = keep; n != null; n = n.Next) chain++;
        Check("GC collected garbage, kept the live chain", GC.CollectionCount(0) > before && chain == 20);

        // Arguments from the host.
        Check("arguments", args.Length == 0 || args[0].Length > 0);

        Console.WriteLine($"HelloClr: {checks - failures}/{checks} checks passed ({Log.Count} logged)");
        return failures == 0 ? 0 : 1;
    }
}
