// Buitenzorg OS — board I/O sample (native C#, bflat --stdlib:zero + bzhw.cs).
//
// On a Raspberry Pi 4: prints the board's built-in sensors (SoC temperature,
// clocks, core voltage, throttling), blinks the green activity LED, drives a
// GPIO pin and reads it back, scans I2C bus 1, and runs an SPI transfer (with
// a jumper between MOSI GPIO 10 and MISO GPIO 9 the bytes come back: loopback).
// Elsewhere it reports that the machine has no board I/O and exits 0.

using System;
using Buitenzorg;
using Buitenzorg.Devices;

class HwDemo
{
    static void Line(BzStringBuilder sb) { sb.AppendLine(); sb.Print(); sb.Clear(); }

    static void MilliC(BzStringBuilder sb, int mc)
    {
        if (mc < 0) { sb.Append("?"); return; }
        sb.Append(mc / 1000).Append('.').Append((mc % 1000) / 100).Append(" C");
    }

    static int Main()
    {
        var sb = new BzStringBuilder();
        if (!Board.IsAvailable)
        {
            Console.WriteLine("HwDemo: mesin ini tidak punya I/O papan (bukan Raspberry Pi 4).");
            return 0;
        }

        // --- built-in sensors
        sb.Append("HwDemo: Raspberry Pi 4, revisi ").AppendHex(Board.Revision).Append(", RAM ")
          .AppendLong(Board.MemoryBytes / (1024 * 1024)).Append(" MiB, ").Append(Board.GpioCount).Append(" GPIO");
        Line(sb);
        sb.Append("  suhu SoC ");
        MilliC(sb, Board.TemperatureMilliC);
        sb.Append(" (batas ");
        MilliC(sb, Board.MaxTemperatureMilliC);
        sb.Append(")");
        Line(sb);
        sb.Append("  clock ARM ").AppendLong(Board.ClockHz(Board.ClockArm) / 1_000_000).Append(" MHz, core ")
          .AppendLong(Board.ClockHz(Board.ClockCore) / 1_000_000).Append(" MHz; tegangan core ")
          .AppendLong(Board.VoltageMicroV(1) / 1000).Append(" mV");
        Line(sb);
        int thr = Board.Throttled;
        sb.Append("  throttling: ").AppendHex(thr < 0 ? 0 : thr);
        if (thr > 0 && (thr & 1) != 0) sb.Append(" (tegangan kurang!)");
        Line(sb);

        // --- LED: blink the green activity LED three times
        for (int i = 0; i < 3; i++)
        {
            Led.Activity(true); Wait(150);
            Led.Activity(false); Wait(150);
        }
        Console.WriteLine("  LED aktivitas (hijau) berkedip 3x");

        // --- GPIO: drive GPIO 17 (header pin 11) and read it back
        var gpio = new GpioController();
        gpio.OpenPin(17, PinMode.Output);
        gpio.Write(17, true);
        bool hi = gpio.Read(17);
        gpio.Write(17, false);
        bool lo = gpio.Read(17);
        gpio.SetPinMode(17, PinMode.Input);
        bool gpioOk = hi && !lo;
        Console.WriteLine(gpioOk ? "  GPIO 17: tulis tinggi/rendah terbaca kembali" : "  GPIO 17: pembacaan balik GAGAL");

        // --- I2C: scan bus 1 (header pins 3/5)
        sb.Append("  I2C bus 1:");
        int found = 0;
        bool answering = true;
        for (int a = 0x08; a < 0x78; a++)
        {
            var d = new I2cDevice(1, a);
            int v = d.ReadByte();
            if (v >= 0) { sb.Append(" ").AppendHex(a); found++; }
            else if (d.LastStatus == BusStatus.Timeout || d.LastStatus == BusStatus.NotSupported) { answering = false; break; }
        }
        if (!answering) sb.Append(" bus tidak menjawab");
        else if (found == 0) sb.Append(" tidak ada perangkat");
        Line(sb);

        // --- SPI0: 4 bytes; with MOSI-MISO jumpered they come back
        var spi = new SpiDevice(0, 0, 1_000_000, 0);
        byte[] tx = new byte[4]; byte[] rx = new byte[4];
        tx[0] = 0xB2; tx[1] = 0x5A; tx[2] = 0x01; tx[3] = 0xFE;
        bool spiOk = spi.TransferFullDuplex(tx, rx, 4);
        bool loop = rx[0] == tx[0] && rx[1] == tx[1] && rx[2] == tx[2] && rx[3] == tx[3];
        Console.WriteLine(!spiOk ? "  SPI0: transfer GAGAL" : loop ? "  SPI0: loopback MOSI-MISO cocok" : "  SPI0: transfer selesai (tanpa jumper loopback)");

        if (gpioOk && spiOk)
        {
            Console.WriteLine("MILESTONE: HWDEMO OK");
            return 0;
        }
        return 1;
    }

    static void Wait(int ms)
    {
        long t0 = BzStopwatchless.Now();
        while (BzStopwatchless.Now() - t0 < ms) { }
    }
}

/// <summary>Milliseconds from the kernel tick counter (18.2 Hz) is too coarse;
/// the monotonic nanosecond clock is used instead.</summary>
static class BzStopwatchless
{
    [System.Runtime.InteropServices.DllImport("*")] static extern ulong bz_clock_nanos();
    public static long Now() => (long)(bz_clock_nanos() / 1_000_000);
}
