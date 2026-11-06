// Buitenzorg OS — native C# (zerolib): static reference fields.
//
// zerolib leaves the GC statics uninitialized, so every `static` reference
// field used to read garbage (XOX's `static readonly char[] _board = new
// char[9]` faulted at a wild address), and a method-group -> delegate
// conversion (whose delegate the compiler caches in a static field) faulted
// too. bzstart.rs now does the runtime's InitializeStatics before Main:
// one object per GC static block, pre-initialized data copied in. This app
// checks each case and prints MILESTONE: STATICS OK. Built with bflat
// --stdlib:zero.

using System;

class Holder
{
    public int Value;
}

delegate int IntOp(int x);

static class Counter
{
    // a static reference field whose value needs code to run: a static
    // constructor (not something ILC can pre-compute)
    public static int[] Table = Build();

    static int[] Build()
    {
        int[] t = new int[8];
        for (int i = 0; i < t.Length; i++) t[i] = i * i;
        return t;
    }
}

static class StaticsDemo
{
    // pre-initialized by ILC (frozen object / frozen array)
    static readonly Holder s_holder = new Holder { Value = 42 };
    static readonly char[] s_board = new char[9];

    // assigned at run time
    static int[] s_numbers;
    static Holder s_later;

    static int Double(int x) => x * 2;

    static int Apply(IntOp op, int v) => op(v);

    static void Main()
    {
        Console.WriteLine("Statics: menguji field static referensi...");

        bool preinit = s_holder != null && s_holder.Value == 42;

        bool board = s_board != null && s_board.Length == 9;
        if (board)
        {
            s_board[4] = 'X';
            board = s_board[4] == 'X' && s_board[0] == '\0';
        }

        s_numbers = new int[3];
        s_numbers[2] = 7;
        s_later = new Holder();
        s_later.Value = 5;
        bool runtime = s_numbers[2] == 7 && s_later.Value == 5;

        // method group -> delegate: the compiler caches the delegate in a static field
        bool deleg = Apply(Double, 21) == 42 && Apply(Double, 4) == 8;

        bool cctor = Counter.Table != null && Counter.Table.Length == 8 && Counter.Table[3] == 9;

        // lambdas: a non-capturing one is cached in a static field like a
        // method group; a capturing one allocates a closure object
        int bonus = 3;
        bool lambdas = Apply(x => x + 1, 9) == 10 && Apply(x => x * bonus, 5) == 15;

        Console.WriteLine(preinit ? "  static ref pre-initialized: ok" : "  static ref pre-initialized: GAGAL");
        Console.WriteLine(board ? "  static readonly char[]: ok" : "  static readonly char[]: GAGAL");
        Console.WriteLine(runtime ? "  static assigned at run time: ok" : "  static assigned at run time: GAGAL");
        Console.WriteLine(deleg ? "  method group -> delegate: ok" : "  method group -> delegate: GAGAL");
        Console.WriteLine(cctor ? "  static constructor: ok" : "  static constructor: GAGAL");
        Console.WriteLine(lambdas ? "  lambdas + closure: ok" : "  lambdas + closure: GAGAL");
        if (preinit && board && runtime && deleg && cctor && lambdas)
            Console.WriteLine("MILESTONE: STATICS OK");
        else
            Console.WriteLine("Statics: verifikasi gagal");
    }
}
