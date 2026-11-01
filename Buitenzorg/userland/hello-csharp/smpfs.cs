// Buitenzorg OS — SMP fine-grained locking test #3: file-system syscalls.
//
// Like SMPSYS/SMPGUI, the kernel boot test runs one copy per CPU at once,
// twice (all syscalls behind the Big Kernel Lock, then fine-grained, where the
// VFS takes only its own mount lock). Each copy loops for ~1.5 s:
//   write its own file /ram/SMPFn.TXT (n = its argument digit, stamped with the
//   round number) -> read it back and verify -> read /disk/BATANG.TXT,
// counting rounds. Reading back another copy's data, or a failed call, fails.
// Exit code: 100 + rounds = correct; 1 = wrong data; 2 = a syscall failed.
// Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

unsafe class SmpFs
{
    [DllImport("*")] static extern ulong bz_fs_write(byte* path, byte* buf, ulong len);
    [DllImport("*")] static extern ulong bz_fs_read(byte* path, byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_proc_args(byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_thread_self();
    [DllImport("*")] static extern ulong bz_ticks();

    const ulong RUN_TICKS = 27; // ~1.5 s at the PIT's 18.2 Hz
    const int LEN = 64;

    static void Put(byte* dst, string s)
    {
        for (int i = 0; i < s.Length; i++)
            dst[i] = (byte)s[i];
        dst[s.Length] = 0;
    }

    static int Main()
    {
        // The file name carries this copy's number (its argument), so copies
        // never share a file.
        byte* args = stackalloc byte[16];
        ulong an = bz_proc_args(args, 16);
        byte digit = an > 0 && an < 16 && args[0] >= (byte)'0' && args[0] <= (byte)'9' ? args[0] : (byte)'0';
        byte* path = stackalloc byte[24];
        Put(path, "/ram/SMPF0.TXT");
        path[9] = digit;
        byte* other = stackalloc byte[24];
        Put(other, "/disk/BATANG.TXT");
        byte* data = stackalloc byte[LEN];
        byte* back = stackalloc byte[LEN];
        byte* big = stackalloc byte[512];
        ulong id = bz_thread_self();

        long rounds = 0;
        ulong start = bz_ticks();
        while (bz_ticks() - start < RUN_TICKS)
        {
            for (int i = 0; i < LEN; i++)
                data[i] = (byte)('A' + (int)((id + (ulong)rounds + (ulong)i) % 26));
            if (bz_fs_write(path, data, LEN) != LEN) // returns the bytes written
                return 2;
            ulong n = bz_fs_read(path, back, LEN);
            if (n != LEN)
                return 2;
            for (int i = 0; i < LEN; i++)
            {
                if (back[i] != data[i])
                    return 1;
            }
            if (bz_fs_read(other, big, 512) == 0)
                return 2;
            rounds++;
        }
        return 100 + (int)rounds;
    }
}
