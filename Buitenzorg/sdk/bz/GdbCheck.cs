// `bz gdb-check`: drive the device's GDB stub (kernel/bzkernel/src/gdbstub.rs)
// with the packets GDB sends, over TCP -- the test that runs where no GDB is
// installed (the smoke test's [bzlink] step).
//
//   bz gdb-check [--app crashme] [--func Crash__Level2]
//                [--tcp 127.0.0.1:1235] [--device-port 1234]
//
// It starts `gdbserver PORT APP` through bz-link, connects to the stub (QEMU:
// `hostfwd=tcp::1235-:1234`), and checks: the entry stop, registers (`g`, `p`),
// `monitor sym`, a breakpoint (`Z0`) hit by `c`, memory (`m`) with the
// breakpoint hidden, a step (`s`), `z0`, and `k`.

using System.Net.Sockets;
using System.Text;

static class GdbCheck
{
    sealed class Rsp : IDisposable
    {
        readonly TcpClient _c;
        readonly NetworkStream _s;
        public Rsp(string host, int port)
        {
            _c = new TcpClient();
            _c.Connect(host, port);
            _c.ReceiveTimeout = 60000;
            _s = _c.GetStream();
        }

        public string Call(string data)
        {
            byte sum = 0;
            foreach (var b in Encoding.ASCII.GetBytes(data)) sum += b;
            var p = Encoding.ASCII.GetBytes($"${data}#{sum:x2}");
            _s.Write(p);
            // skip acks, read $...#cs, ack it
            int ch;
            do { ch = _s.ReadByte(); if (ch < 0) throw new IOException("stub closed the connection"); } while (ch != '$');
            var sb = new StringBuilder();
            while ((ch = _s.ReadByte()) != '#')
            {
                if (ch < 0) throw new IOException("stub closed the connection");
                sb.Append((char)ch);
            }
            _s.ReadByte();
            _s.ReadByte();
            _s.WriteByte((byte)'+');
            return sb.ToString();
        }

        /// <summary>Send raw bytes (Ctrl+C is a bare 0x03) and read the next packet.</summary>
        public string CallRaw(byte[] raw)
        {
            _s.Write(raw);
            int ch;
            do { ch = _s.ReadByte(); if (ch < 0) throw new IOException("stub closed the connection"); } while (ch != '$');
            var sb = new StringBuilder();
            while ((ch = _s.ReadByte()) != '#')
            {
                if (ch < 0) throw new IOException("stub closed the connection");
                sb.Append((char)ch);
            }
            _s.ReadByte();
            _s.ReadByte();
            _s.WriteByte((byte)'+');
            return sb.ToString();
        }

        public void Send(string data)
        {
            byte sum = 0;
            foreach (var b in Encoding.ASCII.GetBytes(data)) sum += b;
            _s.Write(Encoding.ASCII.GetBytes($"${data}#{sum:x2}"));
        }

        public void Dispose() => _c.Dispose();
    }

    static ulong Le(string hex, int index, int size = 8)
    {
        ulong v = 0;
        for (int i = 0; i < size; i++) v |= Convert.ToUInt64(hex.Substring((index * 16) + 2 * i, 2), 16) << (8 * i);
        return v;
    }

    public static int Check(IDebugDevice dev, string[] args)
    {
        string app = "crashme", func = "Crash__Level2", tcp = "127.0.0.1:1235";
        int devPort = 1234;
        for (int i = 0; i + 1 < args.Length; i++)
            switch (args[i])
            {
                case "--app": app = args[i + 1]; break;
                case "--func": func = args[i + 1]; break;
                case "--tcp": tcp = args[i + 1]; break;
                case "--device-port": int.TryParse(args[i + 1], out devPort); break;
            }
        int checks = 0, failed = 0;
        void Expect(bool ok, string what) { checks++; if (!ok) failed++; Console.WriteLine((ok ? "  ok   " : "  FAIL ") + what); }
        if (!dev.Exec("ls /disk").Output.ToUpperInvariant().Contains(app.ToUpperInvariant() + ".ELF"))
        {
            Console.WriteLine($"bz gdb-check: skipped (/disk/{app.ToUpperInvariant()}.ELF is not on the device)");
            Console.WriteLine("bz gdb-check: 0 checks, 0 failed");
            return 0;
        }
        try
        {
            var started = dev.Exec($"gdbserver {devPort} {app}").Output;
            var m = System.Text.RegularExpressions.Regex.Match(started, @"stopped \(entry\) at (0x[0-9a-f]+)");
            Expect(m.Success, "gdbserver: " + started.Trim());
            ulong entry = m.Success ? Convert.ToUInt64(m.Groups[1].Value, 16) : 0;
            var hp = tcp.Split(':');
            using var g = new Rsp(hp[0], int.Parse(hp[1]));
            Expect(g.Call("qSupported:multiprocess+;swbreak+;hwbreak+").Contains("PacketSize="), "qSupported");
            Expect(g.Call("?") == "S05", "? -> S05 (stopped)");
            g.Call("Hg0");
            var regs = g.Call("g");
            // x86-64 / RISC-V: pc is register 16 / 32; ARM64: 32
            int pcIndex = regs.Length / 16 >= 33 ? 32 : 16;
            Expect(regs.Length >= 17 * 16 && Le(regs, pcIndex) == entry, $"g: {regs.Length / 2} bytes, pc = entry {entry:x}");
            Expect(g.Call($"p{pcIndex:x}") == regs.Substring(pcIndex * 16, 16), "p (one register) agrees with g");
            var symHex = g.Call("qRcmd," + Convert.ToHexStringLower(Encoding.ASCII.GetBytes("sym " + func)));
            string symText = Encoding.ASCII.GetString(Convert.FromHexString(symHex)).Trim();
            ulong addr = symText.StartsWith("0x") ? Convert.ToUInt64(symText, 16) : 0;
            Expect(addr != 0, $"monitor sym {func} = {symText}");
            Expect(g.Call($"Z0,{addr:x},1") == "OK", "Z0 (breakpoint)");
            Expect(g.Call("c") == "S05", "c -> S05 (the breakpoint)");
            regs = g.Call("g");
            Expect(Le(regs, pcIndex) == addr, $"pc at {func}");
            var mem = g.Call($"m{addr:x},4");
            Expect(mem.Length == 8 && !mem.StartsWith("cc"), $"m: {mem} (the program's bytes, not the breakpoint)");
            Expect(g.Call("s") == "S05", "s -> S05 (one step)");
            regs = g.Call("g");
            Expect(Le(regs, pcIndex) != addr, "pc moved after the step");
            Expect(g.Call($"z0,{addr:x},1") == "OK", "z0 (breakpoint removed)");
            // M: write 8 bytes on the stack, read them back, put the old ones back
            int spIndex = pcIndex == 16 ? 7 : regs.Length % 16 != 0 ? 31 : 2; // x86 rsp; ARM64 sp (cpsr makes the packet odd); RISC-V x2
            ulong sp = Le(regs, spIndex);
            var before = g.Call($"m{sp:x},8");
            Expect(g.Call($"M{sp:x},8:1122334455667788") == "OK" && g.Call($"m{sp:x},8") == "1122334455667788", $"M: wrote 8 bytes at sp {sp:x} and read them back");
            Expect(g.Call($"M{sp:x},8:{before}") == "OK", "M: old bytes restored");
            g.Send("k");
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        // the session ends; the stub kills a program it leaves
        Thread.Sleep(1500);

        // Ctrl+C: a program that spins, continued, then interrupted
        try
        {
            var started = dev.Exec($"gdbserver {devPort} {app} spin").Output;
            Expect(started.Contains("stopped (entry)"), "gdbserver ... spin (a program that runs ~1.5 s)");
            var hp = tcp.Split(':');
            using var g = new Rsp(hp[0], int.Parse(hp[1]));
            g.Call("qSupported");
            g.Send("c");
            Thread.Sleep(400);
            var raw = new byte[] { 0x03 };
            var reply = g.CallRaw(raw);
            Expect(reply == "S02", $"Ctrl+C while it runs -> {reply} (SIGINT)");
            var bt = Encoding.ASCII.GetString(Convert.FromHexString(g.Call("qRcmd," + Convert.ToHexStringLower("bt"u8.ToArray()))));
            Expect(bt.Contains("Hot") || bt.Contains("Cold") || bt.Contains("clock"), "stopped inside the spin: " + bt.Trim().Split('\n')[0]);
            g.Send("k");
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        Thread.Sleep(1500);

        // kgdb: the kernel, read-only
        try
        {
            var started = dev.Exec($"kgdb {devPort}").Output;
            var m = System.Text.RegularExpressions.Regex.Match(started, @"offset (0x[0-9a-f]+)");
            Expect(m.Success, "kgdb: " + started.Trim());
            var hp = tcp.Split(':');
            using var g = new Rsp(hp[0], int.Parse(hp[1]));
            g.Call("qSupported");
            var off = g.Call("qOffsets");
            Expect(off.StartsWith("Text="), "qOffsets: " + off);
            ulong text = Convert.ToUInt64(off[5..off.IndexOf(';')], 16);
            var low = g.Call("m1000,4");
            Expect(low == "E14", "user-half addresses refused (" + low + ")");
            if (text != 0)
            {
                var head = g.Call($"m{text:x},4");
                Expect(head == "7f454c46", $"the kernel image at {text:x} starts with the ELF magic ({head})");
            }
            Expect(g.Call("M0,1:00") == "E01", "kgdb does not write");
            g.Send("D");
        }
        catch (Exception e) { failed++; Console.WriteLine("  FAIL " + e.Message); }
        Thread.Sleep(1000);
        Console.WriteLine($"bz gdb-check: {checks} checks, {failed} failed");
        return failed == 0 ? 0 : 1;
    }
}
