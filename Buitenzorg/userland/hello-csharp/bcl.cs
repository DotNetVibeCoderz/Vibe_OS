// Buitenzorg OS — v0.15 "Matang" increment 6: Buitenzorg.Bcl demo.
//
// Uses the hand-written .NET-style library (bzbcl.cs) that sits on the working
// managed heap: a generic list, LINQ-style Where/Select/Sum, a StringBuilder,
// BitConverter, and Base64 — real, usable collections/text/encoding in ring-3
// C# (not the official CoreLib, but functional and on the path). Built with
// bflat --stdlib:zero together with bzbcl.cs.

using System;
using Buitenzorg;

class BclDemo
{
    static bool IsEven(int x) => x % 2 == 0;
    static bool IsOdd(int x) => x % 2 != 0;
    static bool IsNonNeg(int x) => x >= 0;
    static int Square(int x) => x * x;
    static int Add(int a, int b) => a + b;

    static unsafe void Main()
    {
        Console.WriteLine("Bcl: menguji List/LINQ/SB/BitConv/Base64 + Math/Random/Convert/Str/RefList/Hex...");

        // Generic list + LINQ: sum of squares of the even numbers in 0..9.
        // (Function pointers avoid the delegate-caching GC static.)
        BzList<int> list = new BzList<int>();
        for (int i = 0; i < 10; i++) list.Add(i);
        BzList<int> evens = BzLinq.Where(list, &IsEven);        // 0,2,4,6,8
        BzList<int> squares = BzLinq.Select(evens, &Square);    // 0,4,16,36,64
        int sum = BzLinq.Sum(squares);                          // 120
        bool linqOk = sum == 120 && list.Count == 10 && evens.Count == 5;

        // StringBuilder: build and print dynamic text (numbers included).
        BzStringBuilder sb = new BzStringBuilder();
        sb.Append("  list.Count=").Append(list.Count)
          .Append(" evens=").Append(evens.Count)
          .Append(" sumOfSquares=").Append(sum).Append('\n');
        sb.Print();
        bool sbOk = sb.Length > 0;

        // BitConverter round-trip.
        byte[] bytes = BzBitConverter.GetBytes(0x1234_5678);
        int back = BzBitConverter.ToInt32(bytes, 0);
        bool bcOk = back == 0x1234_5678;

        // Base64 encode of those 4 bytes (LE: 78 56 34 12) -> "eFY0Eg==".
        int b64len;
        char[] b64 = BzBase64.Encode(bytes, out b64len);
        const string expected = "eFY0Eg==";
        bool b64Ok = b64len == expected.Length;
        for (int i = 0; i < b64len && b64Ok; i++) if (b64[i] != expected[i]) b64Ok = false;

        BzStringBuilder sb2 = new BzStringBuilder();
        sb2.Append("  base64(").AppendHex(0x1234_5678).Append(")=");
        for (int i = 0; i < b64len; i++) sb2.Append(b64[i]);
        sb2.Append('\n');
        sb2.Print();

        // More LINQ over the same list.
        int evenCount = BzLinq.Count(list, &IsEven);   // 5
        bool anyOdd = BzLinq.Any(list, &IsOdd);         // true
        bool allNonNeg = BzLinq.All(list, &IsNonNeg);   // true
        int mx = BzLinq.Max(list);                       // 9
        int mn = BzLinq.Min(list);                       // 0
        bool linqOk2 = evenCount == 5 && anyOdd && allNonNeg && mx == 9 && mn == 0;

        // Stack (LIFO).
        BzStack<int> st = new BzStack<int>();
        st.Push(1); st.Push(2); st.Push(3);
        bool stackOk = st.Count == 3 && st.Pop() == 3 && st.Pop() == 2 && st.Pop() == 1 && st.Count == 0;

        // Queue (FIFO).
        BzQueue<int> q = new BzQueue<int>();
        for (int i = 1; i <= 5; i++) q.Enqueue(i);
        int qsum = 0, prev = 0; bool qorder = true;
        while (q.Count > 0) { int x = q.Dequeue(); if (x != prev + 1) qorder = false; prev = x; qsum += x; }
        bool queueOk = qsum == 15 && qorder;

        // Dictionary (int-keyed map), including overwrite and a miss.
        BzIntMap<int> map = new BzIntMap<int>();
        map.Set(10, 100); map.Set(20, 200); map.Set(10, 111);
        int mv1, mv2, mv3;
        bool mapOk = map.TryGet(10, out mv1) && mv1 == 111
                     && map.TryGet(20, out mv2) && mv2 == 200
                     && !map.TryGet(30, out mv3) && map.Count == 2;

        // HashSet (int).
        BzIntSet set = new BzIntSet();
        set.Add(5); set.Add(5); set.Add(7);
        bool setOk = set.Count == 2 && set.Contains(5) && set.Contains(7) && !set.Contains(9);

        // String-keyed dictionary (overwrite + miss).
        BzStrMap<int> smap = new BzStrMap<int>();
        smap.Set("one", 1); smap.Set("two", 2); smap.Set("one", 11);
        int sv1, sv2, sv3;
        bool smapOk = smap.TryGet("one", out sv1) && sv1 == 11
                      && smap.TryGet("two", out sv2) && sv2 == 2
                      && !smap.TryGet("three", out sv3) && smap.Count == 2;

        // Quicksort.
        BzList<int> toSort = new BzList<int>();
        int[] raw = new int[7];
        raw[0] = 5; raw[1] = 3; raw[2] = 8; raw[3] = 1; raw[4] = 9; raw[5] = 2; raw[6] = 7;
        for (int i = 0; i < raw.Length; i++) toSort.Add(raw[i]);
        BzSort.Sort(toSort);
        bool sortOk = toSort[0] == 1 && toSort[toSort.Count - 1] == 9;
        for (int i = 1; i < toSort.Count && sortOk; i++) if (toSort[i - 1] > toSort[i]) sortOk = false;

        // Contains / IndexOf / Reverse.
        bool findOk = BzLinq.Contains(list, 7) && BzLinq.IndexOf(list, 7) == 7 && !BzLinq.Contains(list, 99);
        BzList<int> rev = BzLinq.Reverse(list);
        bool revOk = rev[0] == 9 && rev[9] == 0;

        // AppendLong.
        BzStringBuilder sbL = new BzStringBuilder();
        sbL.AppendLong(9_876_543_210L);
        bool longOk = sbL.Length == 10;

        BzStringBuilder sb3 = new BzStringBuilder();
        sb3.Append("  count(even)=").Append(evenCount)
           .Append(" max=").Append(mx).Append(" min=").Append(mn)
           .Append(" stack/queue/map=")
           .Append(stackOk ? 1 : 0).Append(queueOk ? 1 : 0).Append(mapOk ? 1 : 0)
           .Append(" set/strmap/sort/find/rev/long=")
           .Append(setOk ? 1 : 0).Append(smapOk ? 1 : 0).Append(sortOk ? 1 : 0)
           .Append(findOk ? 1 : 0).Append(revOk ? 1 : 0).Append(longOk ? 1 : 0)
           .Append(" big=").AppendLong(9_876_543_210L)
           .Append('\n');
        sb3.Print();

        // --- v0.16 BCL additions: Math / Random / Convert / Str / RefList / Hex ---
        bool mathOk = BzMath.Abs(-7) == 7 && BzMath.Clamp(15, 0, 10) == 10 && BzMath.Gcd(24, 36) == 12
                      && BzMath.Pow(2, 10) == 1024 && BzMath.ISqrt(10000) == 100 && BzMath.Sign(-3) == -1;

        BzRandom rng = new BzRandom(12345);
        bool rngOk = true;
        for (int i = 0; i < 50; i++) { int r = rng.NextRange(10, 20); if (r < 10 || r >= 20) rngOk = false; }
        bool rngDet = new BzRandom(1).Next() == new BzRandom(1).Next() && new BzRandom(7).Next() >= 0;

        long pv = BzConvert.ParseLong("-12345", out bool okp);
        int ph = BzConvert.ParseHex("1A2b", out bool okh);
        char[] nb = new char[24]; int nl = BzConvert.LongToChars(-98765, nb);
        bool convOk = okp && pv == -12345 && okh && ph == 0x1A2B && nl == 6 && nb[0] == '-' && nb[1] == '9';

        bool strOk = BzStr.Equals("HALO", "HALO") && !BzStr.Equals("HALO", "halo")
                     && BzStr.StartsWith("BUITENZORG", "BUIT") && BzStr.EndsWith("BUITENZORG", "ZORG")
                     && BzStr.IndexOf("A-B-C", '-') == 1 && BzStr.Count("A-B-C", '-') == 2
                     && BzStr.Upper('a') == 'A' && BzStr.Lower('Z') == 'z' && BzStr.IsDigit('5') && !BzStr.IsAlpha('5');

        BzRefList<char[]> rl = new BzRefList<char[]>();
        rl.Add(new char[] { 'a', 'b' }); rl.Add(new char[] { 'c' });
        bool refOk = rl.Count == 2 && rl.Get(0)[0] == 'a' && rl.Get(1)[0] == 'c';

        int hl; char[] hx = BzHex.Encode(BzBitConverter.GetBytes(0x1234_5678), out hl); // LE 78 56 34 12
        bool hexOk = hl == 8 && hx[0] == '7' && hx[1] == '8' && hx[2] == '5' && hx[3] == '6';

        bool linq3 = BzLinq.First(list) == 0 && BzLinq.Last(list) == 9
                     && BzLinq.Take(list, 3).Count == 3 && BzLinq.Skip(list, 7).Count == 3
                     && BzLinq.Average(list) == 4 && BzLinq.Aggregate(list, 0, &Add) == 45;

        BzStringBuilder sb4 = new BzStringBuilder();
        sb4.Append("x").AppendLine().Append("y"); int clr = sb4.Length; sb4.Clear();
        bool sbAddOk = clr == 3 && sb4.Length == 0;

        BzStringBuilder sb5 = new BzStringBuilder();
        sb5.Append("  math/rng/conv/str/ref/hex/linq3/sb=")
           .Append(mathOk ? 1 : 0).Append(rngOk && rngDet ? 1 : 0).Append(convOk ? 1 : 0)
           .Append(strOk ? 1 : 0).Append(refOk ? 1 : 0).Append(hexOk ? 1 : 0)
           .Append(linq3 ? 1 : 0).Append(sbAddOk ? 1 : 0).Append('\n');
        sb5.Print();

        bool addOk = mathOk && rngOk && rngDet && convOk && strOk && refOk && hexOk && linq3 && sbAddOk;

        if (linqOk && sbOk && bcOk && b64Ok && linqOk2 && stackOk && queueOk && mapOk
            && setOk && smapOk && sortOk && findOk && revOk && longOk && addOk)
            Console.WriteLine("MILESTONE: BCL OK");
        else
            Console.WriteLine("Bcl: verifikasi gagal (koleksi/linq/sort/sb/math/rng/conv/str/ref/hex)");
    }
}
