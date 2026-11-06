// Buitenzorg.Bcl part 3 demo (bzbcl3.cs): type casts and covariant arrays
// (shim RhTypeCast_* + RhpStelemRef), delegates/lambdas, Interlocked across
// parallel threads, TimeSpan, Guid (kernel RANDOM syscall) and generic LINQ.
// Prints MILESTONE: BCL3 OK only if every check passes.

using System;
using Buitenzorg;

interface IPet { int Legs(); }
class Animal { public int Id; public Animal(int id) { Id = id; } }
class Dog : Animal, IPet { public Dog(int id) : base(id) { } public int Legs() => 4; }
class Bird : Animal, IPet { public Bird(int id) : base(id) { } public int Legs() => 2; }
class Rock { }

class Person
{
    public string Name; public int Age; public int City;
    public Person(string n, int a, int c) { Name = n; Age = a; City = c; }
}

static class Bcl3
{
    static long Counter;
    static int Fails;
    static readonly char[] Buf = new char[64];

    static void Check(bool ok, string what)
    {
        BzDebug.Write(ok ? "  ok   " : "  GAGAL ");
        Console.WriteLine(what);
        if (!ok) Fails++;
    }

    static bool Same(char[] b, int len, string s)
    {
        if (len != s.Length) return false;
        for (int i = 0; i < len; i++) if (b[i] != s[i]) return false;
        return true;
    }

    static void Worker(ulong rounds)
    {
        for (ulong i = 0; i < rounds; i++) BzInterlocked.Increment(ref Counter);
    }

    static object Opaque(int i, object[] items) => items[i]; // keeps the JIT from knowing the type

    static unsafe int Main()
    {
        Console.WriteLine("Bcl3: cast/array kovarian, delegate, Interlocked, TimeSpan, Guid, LINQ generik...");

        // ---- casts, is/as, pattern matching, covariant arrays ----
        object[] things = new object[5];
        things[0] = new Dog(1);
        things[1] = new Bird(2);
        things[2] = new Rock();
        things[3] = "teks";
        things[4] = new Dog(5);
        int animals = 0, pets = 0, legs = 0, dogs = 0;
        for (int i = 0; i < things.Length; i++)
        {
            object o = Opaque(i, things);
            if (o is Animal) animals++;
            IPet p = o as IPet;
            if (p != null) { pets++; legs += p.Legs(); }
            if (o is Dog d && d.Id > 0) dogs++;
        }
        Check(animals == 3 && pets == 3 && legs == 10 && dogs == 2, "is / as / pola 'is Dog d' pada object[] campuran");
        Animal a = (Animal)Opaque(1, things);
        Check(a.Id == 2, "cast (Animal) dari object");
        IPet[] petArr = new IPet[2];
        petArr[0] = (IPet)Opaque(0, things);
        petArr[1] = (Bird)Opaque(1, things);
        Animal[] zoo = new Animal[2];
        zoo[0] = new Dog(7); zoo[1] = new Bird(8);
        object[] covariant = new string[2];
        covariant[0] = "a";
        Check(petArr[0].Legs() + petArr[1].Legs() == 6 && zoo[1].Id == 8 && covariant.Length == 2, "array interface, array kelas dasar, kovariansi string[] -> object[]");
        int kind = 0;
        switch (Opaque(2, things))
        {
            case Dog: kind = 1; break;
            case Rock: kind = 2; break;
        }
        Check(kind == 2, "switch berdasarkan tipe");

        // ---- delegates and lambdas ----
        int offset = 10;
        Func<int, int> add = x => x + offset;
        Predicate<int> even = x => x % 2 == 0;
        Comparison<int> desc = (x, y) => y - x;
        int acc = 0;
        Action<int> collect = x => acc += x;
        collect(add(5)); collect(add(1));
        Check(acc == 26 && even(4) && !even(3) && desc(1, 2) > 0, "Func / Action / Predicate / Comparison dengan closure");

        // ---- Interlocked across parallel threads ----
        Counter = 0;
        BzTask t1 = BzTask.Run(&Worker, 20000);
        BzTask t2 = BzTask.Run(&Worker, 20000);
        Worker(20000);
        bool joined = t1 != null && t2 != null && t1.Wait() && t2.Wait();
        Check(joined && Counter == 60000, "Interlocked.Increment dari 3 thread paralel = 60000");
        int v = 5;
        int old = BzInterlocked.CompareExchange(ref v, 9, 5);
        int old2 = BzInterlocked.CompareExchange(ref v, 1, 5);
        int ex = BzInterlocked.Exchange(ref v, 3);
        Check(old == 5 && old2 == 9 && ex == 9 && v == 3 && BzInterlocked.Add(ref v, 4) == 7 && BzInterlocked.Decrement(ref v) == 6, "CompareExchange / Exchange / Add / Decrement");

        // ---- TimeSpan ----
        BzTimeSpan ts = new BzTimeSpan(1, 2, 3, 4, 5);
        int n = ts.Format(Buf, 0);
        bool fmt = Same(Buf, n, "1.02:03:04.0050000");
        BzTimeSpan back;
        bool parsed = BzTimeSpan.TryParse(Buf, n, out back) && back == ts;
        BzTimeSpan neg = BzTimeSpan.FromMilliseconds(-1500);
        int n2 = neg.Format(Buf, 0);
        bool negFmt = Same(Buf, n2, "-00:00:01.5000000");
        BzTimeSpan ninety = BzTimeSpan.FromMinutes(90);
        Check(fmt && parsed && negFmt && ninety.Hours == 1 && ninety.Minutes == 30 && ninety.TotalSeconds == 5400
              && (ninety + BzTimeSpan.FromMinutes(30)).TotalHours == 2 && ninety > BzTimeSpan.FromHours(1),
              "TimeSpan: komponen, format \"c\", parse, aritmetika");
        Con.Write(Buf, n2); Console.WriteLine("");

        // ---- Guid ----
        BzGuid g1 = BzGuid.NewGuid(), g2 = BzGuid.NewGuid();
        int gn = g1.Format(Buf, 0);
        BzGuid g1b;
        bool roundTrip = BzGuid.TryParse(Buf, 0, gn, out g1b) && g1b == g1;
        BzDebug.Write("  guid: "); Con.Write(Buf, gn); Console.WriteLine("");
        char[] known = new char[36];
        string ks = "0f8fad5b-d9cb-469f-a165-70867728950e";
        for (int i = 0; i < 36; i++) known[i] = ks[i];
        BzGuid kg;
        bool kp = BzGuid.TryParse(known, 0, 36, out kg);
        int kn = kg.Format(Buf, 0);
        Check(!g1.IsEmpty && g1 != g2 && g1.Version == 4 && (g1[8] & 0xC0) == 0x80 && gn == 36 && roundTrip
              && kp && kg[0] == 0x5b && kg[3] == 0x0f && kg[4] == 0xcb && kg[7] == 0x46 && Same(Buf, kn, ks),
              "Guid: v4 acak dari kernel, format \"D\", parse, urutan byte .NET");

        // ---- generic LINQ ----
        var people = new BzList<Person>();
        people.Add(new Person("Sari", 34, 1));
        people.Add(new Person("Budi", 17, 2));
        people.Add(new Person("Ayu", 34, 2));
        people.Add(new Person("Dewi", 25, 1));
        people.Add(new Person("Eko", 12, 3));
        people.Add(new Person("Fajar", 25, 2));
        var adults = people.Where(p => p.Age >= 18);
        var byAge = adults.OrderBy(p => (long)p.Age);              // stable: Dewi before Fajar, Sari before Ayu
        var byAgeDescName = adults.OrderBy(BzEnumerable.By<Person>(p => -p.Age).ThenBy(p => p.Name[0]));
        var groups = people.GroupBy(p => p.City);
        var ages = people.Select(p => p.Age);
        var distinctAges = ages.Distinct();
        long total = people.Sum(p => p.Age);
        Person oldest = people.MaxBy(p => p.Age, null);
        Person youngest = people.MinBy(p => p.Age, null);
        var names = byAge.Select(p => p.Name);
        var firstTwo = names.Take(2);
        bool stable = (object)byAge[0].Name == (object)"Dewi" && (object)byAge[1].Name == (object)"Fajar"
                      && (object)byAge[2].Name == (object)"Sari" && (object)byAge[3].Name == (object)"Ayu";
        bool desc2 = (object)byAgeDescName[0].Name == (object)"Ayu" && (object)byAgeDescName[1].Name == (object)"Sari"
                      && (object)byAgeDescName[2].Name == (object)"Dewi";
        bool grouped = groups.Count == 3 && groups[0].Key == 1 && groups[0].Count == 2 && groups[1].Count == 3 && groups[2].Count == 1
                       && (object)groups[1].Items[1].Name == (object)"Ayu";
        int sumZip = ages.Zip(ages.Reverse(), (x, y) => x + y).Aggregate(0, (s, x) => s + x);
        Check(adults.Count == 4 && stable && desc2, "Where + OrderBy stabil + OrderBy..ThenBy");
        Check(grouped && distinctAges.Count == 4 && total == 147 && (object)oldest.Name == (object)"Sari"
              && (object)youngest.Name == (object)"Eko" && firstTwo.Count == 2 && sumZip == 294
              && people.Any(p => p.Age < 13) && !people.All(p => p.Age > 13) && people.Count(p => p.City == 2) == 3,
              "GroupBy / Distinct / Sum / MinBy / MaxBy / Zip / Aggregate / Any / All / Count");

        Console.WriteLine(Fails == 0 ? "MILESTONE: BCL3 OK" : "MILESTONE: BCL3 FAIL");
        return Fails == 0 ? 0 : 1;
    }
}
