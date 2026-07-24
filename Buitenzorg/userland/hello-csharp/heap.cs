// Buitenzorg OS — v0.15 "Matang" increment 4: managed heap (new/array/generics).
//
// zerolib's `new` and array allocation route through SystemNative_Malloc, which
// increment 4 makes a real growable, zeroed heap (a .bss arena that grows via
// the mmap PAL). This app allocates a large array (forcing the heap to grow
// past the .bss arena via mmap), builds a heap-object linked list, and uses a
// generic type — verifying heap allocation works in ring-3 C#. (Static
// reference fields are still unsupported — GC statics — so everything here is
// local.) Built with bflat --stdlib:zero.

using System;

class HeapDemo
{
    // Heap-allocated reference type (uses `new`, exercises AllocObject).
    class Node
    {
        public int Value;
        public Node Next;
    }

    // Generic value type (exercises generic instantiation).
    struct Pair<T>
    {
        public T A;
        public T B;
    }

    static void Main()
    {
        Console.WriteLine("Heap: menguji new / array / generics...");

        // 1) A large array (~400 KiB) forces the heap to grow past the 256 KiB
        //    .bss arena into an mmap'd chunk.
        const int N = 100000;
        int[] arr = new int[N];
        for (int i = 0; i < N; i++) arr[i] = i;
        long sum = 0;
        for (int i = 0; i < N; i++) sum += arr[i];
        bool arrOk = sum == (long)N * (N - 1) / 2;

        // Multi-growth check: allocate several 400 KiB arrays so the heap must
        // map more than one mmap chunk (crosses the 1 MiB grow boundary more
        // than once). This exercises the growable bump heap past a single chunk.
        bool multiOk = true;
        for (int k = 0; k < 4; k++)
        {
            int[] big = new int[N];
            big[0] = k; big[N - 1] = k + 7;
            if (big[0] != k || big[N - 1] != k + 7) { multiOk = false; break; }
        }

        // 2) Heap objects: a small linked list built with `new`.
        Node head = null;
        for (int i = 0; i < 5; i++)
        {
            Node n = new Node();
            n.Value = i;
            n.Next = head;
            head = n;
        }
        int count = 0, valueSum = 0;
        for (Node c = head; c != null; c = c.Next)
        {
            count++;
            valueSum += c.Value;
        }
        bool objOk = count == 5 && valueSum == (0 + 1 + 2 + 3 + 4);

        // 3) A generic instance.
        Pair<int> p = new Pair<int>();
        p.A = 3;
        p.B = 4;
        bool genOk = p.A + p.B == 7;

        if (arrOk && multiOk && objOk && genOk)
            Console.WriteLine("MILESTONE: HEAP OK");
        else
            Console.WriteLine("Heap: verifikasi gagal (array/multi/objek/generic)");
    }
}
