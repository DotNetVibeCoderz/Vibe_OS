// Buitenzorg OS — process control from ring 3: spawn + argv + wait.
//
// Run with no arguments, this program is the parent: it starts two copies of
// itself as separate processes — once by VFS path with "child 7", once by app
// name with "child 42" — waits for each, and checks that each exit code is the
// number it was given (so the argument string travelled to the child and the
// exit code came back through PROC_WAIT). It also checks that waiting on a pid
// that never existed is refused instead of hanging.
// Run as "child N", it just returns N.
// Built with bflat --stdlib:zero + bzbcl/bzbcl2 (BzProcess).

using System;
using Buitenzorg;

class ProcTest
{
    const ulong ERR_LOW = 0xFFFF_FFFF_FFFF_FF00UL;

    static int Main()
    {
        char[] args;
        int n = BzProcess.GetArgs(out args);
        if (n >= 7 && args[0] == 'c' && args[1] == 'h' && args[2] == 'i' && args[3] == 'l'
            && args[4] == 'd' && args[5] == ' ')
            return ParseInt(args, 6, n);

        Console.WriteLine("ProcTest: induk men-spawn anak (path + nama app) lalu menunggu...");
        ulong a = BzProcess.Start("/disk/PROCTEST.ELF", "child 7");
        ulong b = BzProcess.Start("proctest", "child 42");
        if (a == 0 || b == 0)
        {
            Console.WriteLine("ProcTest: SPAWN GAGAL");
            return 1;
        }
        ulong codeA = BzProcess.WaitForExit(a);
        ulong codeB = BzProcess.WaitForExit(b);
        bool unknownRefused = BzProcess.WaitForExit(999_999) >= ERR_LOW;

        if (a != b && codeA == 7 && codeB == 42 && unknownRefused)
        {
            Console.WriteLine("MILESTONE: PROCCTL OK");
            return 0;
        }
        Console.WriteLine("ProcTest: kode exit / argumen tidak cocok");
        return 1;
    }

    static int ParseInt(char[] s, int from, int to)
    {
        int v = 0;
        for (int i = from; i < to; i++)
        {
            char c = s[i];
            if (c < '0' || c > '9') break;
            v = v * 10 + (c - '0');
        }
        return v;
    }
}
