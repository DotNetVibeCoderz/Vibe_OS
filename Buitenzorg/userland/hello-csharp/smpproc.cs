// Buitenzorg OS — SMP fine-grained locking test #4: processes, network,
// packages, audio.
//
// Like SMPSYS/SMPGUI/SMPFS, the kernel boot test runs one copy per CPU at once,
// twice (all syscalls behind the Big Kernel Lock, then fine-grained). Each copy
// loops for ~1.5 s:
//   PROC_SPAWN a child (this same program with argument "c", which exits at
//   once with code 42) -> PROC_WAIT for it and check the code (the child exits
//   on another CPU while we decide to sleep: a lost wake-up hangs here) ->
//   send a UDP datagram to its own port over loopback, receive it and verify
//   the payload -> PKG_LIST -> AUDIO_STAT,
// counting rounds. Exit code: 100 + rounds = correct; 1 = wrong data or exit
// code; 2 = a syscall failed. Built with bflat --stdlib:zero.

using System;
using System.Runtime.InteropServices;

unsafe class SmpProc
{
    [DllImport("*")] static extern ulong bz_proc_spawn(byte* path, byte* args);
    [DllImport("*")] static extern ulong bz_proc_wait(ulong pid);
    [DllImport("*")] static extern ulong bz_proc_args(byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_net_socket(ulong kind);
    [DllImport("*")] static extern ulong bz_net_bind(ulong handle, ulong port);
    [DllImport("*")] static extern ulong bz_net_send(ulong handle, byte* buf, ulong len);
    [DllImport("*")] static extern ulong bz_net_recv(ulong handle, byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_net_close(ulong handle);
    [DllImport("*")] static extern ulong bz_net_info(byte* outInfo);
    [DllImport("*")] static extern ulong bz_pkg_list(byte* buf, ulong max);
    [DllImport("*")] static extern ulong bz_audio_stat(byte* buf);
    [DllImport("*")] static extern ulong bz_thread_self();
    [DllImport("*")] static extern ulong bz_ticks();

    const ulong RUN_TICKS = 27; // ~1.5 s at the PIT's 18.2 Hz
    const ulong ERR = 0xFFFF_FFFF_FFFF_FFFF; // syserr::INVAL
    const int LEN = 32;

    static void Put(byte* dst, string s)
    {
        for (int i = 0; i < s.Length; i++)
            dst[i] = (byte)s[i];
        dst[s.Length] = 0;
    }

    static int Main()
    {
        byte* args = stackalloc byte[16];
        ulong an = bz_proc_args(args, 16);
        if (an == 1 && args[0] == (byte)'c')
            return 42; // the child: exit at once
        byte digit = an > 0 && an < 16 && args[0] >= (byte)'0' && args[0] <= (byte)'9' ? args[0] : (byte)'0';

        byte* self = stackalloc byte[24];
        Put(self, "/disk/SMPPROC.ELF");
        byte* childArgs = stackalloc byte[4];
        Put(childArgs, "c");

        // One UDP socket per copy, on its own port.
        ulong sock = bz_net_socket(0);
        ulong port = 7100 + (ulong)(digit - (byte)'0');
        if (sock == 0 || sock == ERR || bz_net_bind(sock, port) != 0)
            return 2;
        // Our own address (10.0.0.1 on loopback, 10.0.2.15 with a NIC).
        byte* ni = stackalloc byte[64];
        if (bz_net_info(ni) != 0)
            return 2;
        byte* pkt = stackalloc byte[16 + LEN];
        byte* back = stackalloc byte[16 + LEN];
        byte* pkgs = stackalloc byte[64 * 48];
        byte* audio = stackalloc byte[64];
        ulong id = bz_thread_self();

        long rounds = 0;
        int result = 0;
        ulong start = bz_ticks();
        while (bz_ticks() - start < RUN_TICKS)
        {
            ulong pid = bz_proc_spawn(self, childArgs);
            if (pid == 0 || pid == ERR) { result = 2; break; }
            if (bz_proc_wait(pid) != 42) { result = 1; break; }

            // Datagram to ourselves: header (own address, port) + payload.
            pkt[0] = ni[0]; pkt[1] = ni[1]; pkt[2] = ni[2]; pkt[3] = ni[3];
            pkt[4] = (byte)port; pkt[5] = (byte)(port >> 8); pkt[6] = 0; pkt[7] = 0;
            for (int i = 8; i < 16; i++) pkt[i] = 0;
            for (int i = 0; i < LEN; i++)
                pkt[16 + i] = (byte)('a' + (int)((id + (ulong)rounds + (ulong)i) % 26));
            if (bz_net_send(sock, pkt, LEN) != LEN) { result = 2; break; }
            ulong n = bz_net_recv(sock, back, LEN);
            if (n != LEN) { result = 2; break; }
            for (int i = 0; i < LEN; i++)
            {
                if (back[16 + i] != pkt[16 + i]) { result = 1; break; }
            }
            if (result != 0) break;

            ulong np = bz_pkg_list(pkgs, 64);
            if (np == 0 || np == ERR) { result = 2; break; }
            if (bz_audio_stat(audio) != 0) { result = 2; break; }
            rounds++;
        }
        bz_net_close(sock);
        return result != 0 ? result : 100 + (int)rounds;
    }
}
