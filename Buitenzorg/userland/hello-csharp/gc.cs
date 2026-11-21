// Buitenzorg OS -- native C# (zerolib): a garbage collector that reclaims.
//
// The ring-3 heap used to be a bump allocator: every `new` stayed until the
// program exited. The startup shim now has a mark-sweep collector (bzgc.rs).
// This app allocates far more than it ever keeps -- 128 MiB of garbage in
// small, medium and large objects -- while keeping live data reachable only
// from a static field, locals and other objects, then checks that the live
// data is intact, that the committed memory stayed small, and that
// collections ran and freed memory. Prints MILESTONE: GC OK.

using System;
using System.Runtime.InteropServices;

class Node
{
    public int Value;
    public Node Next;
    public int[] Payload;
}

class Holder
{
    public Node Head;
    public object Other;
}

static unsafe class GcTest
{
    static Holder s_holder;
    static int s_failures;

    [DllImport("*")] static extern void bz_gc_stats(ulong* outp);
    [DllImport("*")] static extern void bz_heap_stats(ulong* outp);
    [DllImport("*")] static extern ulong bz_gc_collect();

    static ulong Gc(int i) { ulong* s = stackalloc ulong[8]; bz_gc_stats(s); return s[i]; }
    static ulong Heap(int i) { ulong* s = stackalloc ulong[5]; bz_heap_stats(s); return s[i]; }

    static void Check(bool ok, string what)
    {
        if (!ok) { s_failures++; Console.WriteLine(what); }
    }

    static void Num(string label, ulong v)
    {
        for (int i = 0; i < label.Length; i++) Console.Write(label[i]);
        char* b = stackalloc char[24];
        int n = 0;
        if (v == 0) b[n++] = '0';
        while (v > 0) { b[n++] = (char)('0' + (int)(v % 10)); v /= 10; }
        while (n > 0) Console.Write(b[--n]);
        Console.WriteLine("");
    }

    static Node Build(int n)
    {
        Node head = null;
        for (int i = n - 1; i >= 0; i--)
        {
            var node = new Node { Value = i, Next = head, Payload = new int[4] };
            node.Payload[0] = i * 3;
            head = node;
            // garbage between the live nodes, so live and dead objects share blocks
            var junk = new int[(i % 7) * 8 + 1];
            junk[0] = i;
        }
        return head;
    }

    static bool Verify(Node head, int n)
    {
        int i = 0;
        for (Node p = head; p != null; p = p.Next, i++)
        {
            if (p.Value != i || p.Payload == null || p.Payload.Length != 4 || p.Payload[0] != i * 3) return false;
        }
        return i == n;
    }

    static int Main()
    {
        Console.WriteLine("GC: mengalokasikan 128 MiB sampah...");

        // live data: one list from a static field, one from a local
        s_holder = new Holder();
        s_holder.Head = Build(5000);
        Node local = Build(3000);
        byte[] bigLive = new byte[300000];
        bigLive[123456] = 42;
        s_holder.Other = bigLive;

        ulong collectionsBefore = Gc(0);
        ulong peakCommitted = 0;
        long garbage = 0;
        const long Total = 128L << 20;
        int round = 0;
        while (garbage < Total)
        {
            // small objects
            for (int i = 0; i < 2000; i++)
            {
                var n = new Node { Value = i };
                n.Payload = new int[6];
                garbage += 24 + 48;
            }
            // medium arrays
            for (int i = 0; i < 64; i++)
            {
                var a = new long[(i % 16) * 64 + 100];
                a[0] = i;
                garbage += a.Length * 8;
            }
            // a large array (whole blocks)
            var big = new byte[200000 + (round % 5) * 70000];
            big[big.Length - 1] = 1;
            garbage += big.Length;
            ulong c = Heap(1);
            if (c > peakCommitted) peakCommitted = c;
            round++;
        }

        // an explicit collection, then the live data again
        bool ran = bz_gc_collect() != 0;
        bool live = Verify(s_holder.Head, 5000) && Verify(local, 3000);
        bool bigOk = s_holder.Other == (object)bigLive && bigLive[123456] == 42 && bigLive.Length == 300000;

        ulong collections = Gc(0) - collectionsBefore;
        ulong freed = Gc(2);
        ulong committed = Heap(1);
        Num("GC: rounds ", (ulong)round);
        Num("GC: collections ", collections);
        Num("GC: freed MiB ", freed >> 20);
        Num("GC: peak committed KiB ", peakCommitted >> 10);
        Num("GC: live KiB ", Gc(1) >> 10);
        Num("GC: last pause us ", Gc(3));

        Check(ran, "FAIL: explicit collection did not run");
        Check(live, "FAIL: live linked lists damaged");
        Check(bigOk, "FAIL: live large array damaged");
        Check(collections >= 4, "FAIL: too few collections");
        Check(freed >= (ulong)(Total / 2), "FAIL: freed too little");
        // without reclaiming, committed would exceed 128 MiB
        Check(peakCommitted < (48UL << 20), "FAIL: committed memory grew with the garbage");
        Check(committed <= peakCommitted + (1UL << 20), "FAIL: committed after collect");

        // after dropping the static list, it is reclaimed too
        ulong liveBefore = Gc(1);
        s_holder.Head = null;
        bz_gc_collect();
        Check(Gc(1) + 100000 < liveBefore, "FAIL: dropped list not reclaimed");
        Check(Verify(local, 3000), "FAIL: local list after second collect");

        if (s_failures == 0)
        {
            Console.WriteLine("MILESTONE: GC OK (128 MiB of garbage in small/medium/large objects reclaimed, committed memory bounded, live lists + large array intact, dropped static list reclaimed)");
            return 0;
        }
        return 1;
    }
}
