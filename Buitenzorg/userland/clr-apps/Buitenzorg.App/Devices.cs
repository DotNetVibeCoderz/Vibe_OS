// Buitenzorg.Devices -- board I/O for managed C# apps: GPIO, I2C, SPI, serial
// ports, the built-in LEDs and the board's sensors (Raspberry Pi 4), over the
// HW_* syscalls. The same API as the native bzhw.cs, shaped after .NET IoT
// (System.Device.Gpio / I2c / Spi) so the code reads familiar.
//
// Three situations, one API:
//   * a Raspberry Pi 4 running Buitenzorg: the calls reach the hardware;
//   * another machine running Buitenzorg: Board.IsAvailable is false and the
//     calls fail harmlessly (the kernel answers NOSYS);
//   * off the device (`dotnet App.dll` on a PC): a simulated board answers
//     (Board.IsSimulated), so apps and their self-tests run anywhere.

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace Buitenzorg.Devices;

internal static class HwNative
{
    [DllImport("buitenzorg")] internal static extern long HwBoard(int query, int arg);
    [DllImport("buitenzorg")] internal static extern long HwGpio(int op, int pin, int value);
    [DllImport("buitenzorg")] internal static extern int HwLed(int led, int on);
    [DllImport("buitenzorg")] internal static extern int HwI2c(int bus, int address, int speedHz, byte[]? tx, int txLen, byte[]? rx, int rxLen);
    [DllImport("buitenzorg")] internal static extern int HwSpi(int bus, int chipSelect, int speedHz, int mode, byte[]? tx, byte[]? rx, int len);
    [DllImport("buitenzorg")] internal static extern int HwUart(int op, int port, int baud, byte[]? buffer, int len);
}

/// <summary>The board simulated off the device: plausible sensors, GPIO pins
/// that remember what was written, a TMP102-like temperature sensor at I2C
/// 0x48, SPI and serial loopback.</summary>
internal static class SimBoard
{
    internal static bool Active;
    static readonly Dictionary<int, int> Modes = new();
    static readonly Dictionary<int, bool> Levels = new();
    static readonly Queue<byte>[] Serial = { new(), new(), new(), new() };
    static long _start = -1;

    // Only off the device do the natives fail to bind; on Buitenzorg the host
    // registers them all, so this (and the simulation) is never reached there.
    internal static bool Fallback(Exception e)
    {
        if (e is DllNotFoundException || e is EntryPointNotFoundException)
        {
            Active = true;
            if (_start < 0) _start = Environment.TickCount64;
            return true;
        }
        return false;
    }

    static double Seconds => (Environment.TickCount64 - _start) / 1000.0;

    internal static long Board(int q, int arg) => q switch
    {
        0 => 1,
        1 => 47_000 + (long)(3000 * Math.Sin(Seconds / 4)),
        2 => 85_000,
        3 => arg == 3 ? 1_500_000_000 : arg == 4 ? 500_000_000 : 100_000_000,
        4 => arg == 1 ? 850_000 : 1_100_000,
        5 => 0,
        6 => 0xD03114,
        7 => 0x1000_0000_B2B2_B2B2,
        8 => 948L * 1024 * 1024,
        9 => 58,
        _ => -1,
    };

    internal static long Gpio(int op, int pin, int value)
    {
        if (pin < 0 || pin > 57) return -1;
        switch (op)
        {
            case 0: Modes[pin] = value; return 0;
            case 1: Levels[pin] = value != 0; return 0;
            case 2: return Levels.TryGetValue(pin, out var l) && l ? 1 : 0;
            case 3: return 0;
            case 4: return Modes.TryGetValue(pin, out var m) ? m : 0;
            default: return -1;
        }
    }

    internal static int I2c(int bus, int addr, byte[]? tx, int txLen, byte[]? rx, int rxLen)
    {
        if (bus != 1 || addr != 0x48) return 1; // no acknowledge
        if (rx != null)
        {
            // TMP102: 12-bit temperature, 0.0625 C per step, MSB first
            int raw = (int)(24.5 / 0.0625) << 4;
            if (rxLen > 0) rx[0] = (byte)(raw >> 8);
            if (rxLen > 1) rx[1] = (byte)raw;
        }
        return 0;
    }

    internal static int Spi(byte[]? tx, byte[]? rx, int len)
    {
        for (int i = 0; i < len && rx != null && i < rx.Length; i++) rx[i] = tx != null && i < tx.Length ? tx[i] : (byte)0;
        return 0;
    }

    internal static int Uart(int op, int port, byte[]? buf, int len)
    {
        if (port < 2 || port > 5) return -1;
        var q = Serial[port - 2];
        switch (op)
        {
            case 0: q.Clear(); return 0;
            case 1: for (int i = 0; i < len && buf != null; i++) q.Enqueue(buf[i]); return len;
            case 2: { int n = 0; while (n < len && q.Count > 0 && buf != null) buf[n++] = q.Dequeue(); return n; }
            case 3: return 0;
            default: return -1;
        }
    }
}

/// <summary>Board information and built-in sensors.</summary>
public static class Board
{
    /// <summary>Firmware clock ids for <see cref="ClockHz"/>.</summary>
    public const int ClockArm = 3, ClockCore = 4, ClockV3d = 5, ClockEmmc2 = 12;

    static long Query(int q, int arg = 0)
    {
        if (SimBoard.Active) return SimBoard.Board(q, arg);
        try { return HwNative.HwBoard(q, arg); }
        catch (Exception e) when (SimBoard.Fallback(e)) { return SimBoard.Board(q, arg); }
    }

    /// <summary>True on a board with I/O (a Raspberry Pi 4, or the simulation).</summary>
    public static bool IsAvailable => Query(0) == 1;
    /// <summary>True when running off the device against the simulated board.</summary>
    public static bool IsSimulated { get { _ = IsAvailable; return SimBoard.Active; } }
    public static string Model => IsSimulated ? "Raspberry Pi 4 (simulasi)" : IsAvailable ? "Raspberry Pi 4 Model B" : "tanpa I/O papan";
    /// <summary>SoC temperature in degrees Celsius (NaN if unknown).</summary>
    public static double Temperature { get { long v = Query(1); return v < 0 ? double.NaN : v / 1000.0; } }
    /// <summary>Temperature at which the firmware throttles, degrees Celsius.</summary>
    public static double MaxTemperature { get { long v = Query(2); return v < 0 ? double.NaN : v / 1000.0; } }
    public static long ClockHz(int id) => Query(3, id);
    public static long ArmClockHz => ClockHz(ClockArm);
    public static long CoreClockHz => ClockHz(ClockCore);
    /// <summary>A voltage in volts: 1 core, 2-4 SDRAM (NaN if unknown).</summary>
    public static double Voltage(int id = 1) { long v = Query(4, id); return v <= 0 ? double.NaN : v / 1_000_000.0; }
    public static ThrottleState Throttled { get { long v = Query(5); return v < 0 ? 0 : (ThrottleState)v; } }
    public static int Revision => (int)Query(6);
    public static long Serial => Query(7);
    public static long MemoryBytes => Query(8);
    public static int GpioCount => (int)Query(9);
}

/// <summary>The firmware's throttle flags.</summary>
[Flags]
public enum ThrottleState
{
    None = 0,
    UnderVoltage = 1, FrequencyCapped = 2, Throttled = 4, SoftTemperatureLimit = 8,
    UnderVoltageOccurred = 1 << 16, FrequencyCappedOccurred = 1 << 17, ThrottledOccurred = 1 << 18, SoftTemperatureLimitOccurred = 1 << 19,
}

public enum PinMode { Input = 0, Output = 1, Alt0 = 2, Alt1 = 3, Alt2 = 4, Alt3 = 5, Alt4 = 6, Alt5 = 7 }
public enum PinValue { Low = 0, High = 1 }
public enum PullMode { None = 0, Up = 1, Down = 2 }

/// <summary>GPIO pins, BCM numbering. Applications may use the header pins
/// 0-27 except 14/15 (the serial console), and 42 (the activity LED).</summary>
public sealed class GpioController
{
    static long Call(int op, int pin, int value)
    {
        if (SimBoard.Active) return SimBoard.Gpio(op, pin, value);
        try { return HwNative.HwGpio(op, pin, value); }
        catch (Exception e) when (SimBoard.Fallback(e)) { return SimBoard.Gpio(op, pin, value); }
    }

    public static bool IsUsable(int pin) => (pin >= 0 && pin <= 27 && pin != 14 && pin != 15) || pin == 42;
    public bool OpenPin(int pin, PinMode mode) => SetPinMode(pin, mode);
    public bool SetPinMode(int pin, PinMode mode) => Call(0, pin, (int)mode) == 0;
    public PinMode GetPinMode(int pin) { long v = Call(4, pin, 0); return v is >= 0 and <= 7 ? (PinMode)v : PinMode.Input; }
    public bool Write(int pin, PinValue value) => Call(1, pin, (int)value) == 0;
    public bool Write(int pin, bool high) => Write(pin, high ? PinValue.High : PinValue.Low);
    public PinValue Read(int pin) => Call(2, pin, 0) == 1 ? PinValue.High : PinValue.Low;
    public bool SetPull(int pin, PullMode pull) => Call(3, pin, (int)pull) == 0;
}

/// <summary>The board's LEDs.</summary>
public static class Led
{
    static bool Set(int led, bool on)
    {
        if (SimBoard.Active) return true;
        try { return HwNative.HwLed(led, on ? 1 : 0) == 0; }
        catch (Exception e) when (SimBoard.Fallback(e)) { return true; }
    }
    /// <summary>The green activity LED.</summary>
    public static bool Activity(bool on) => Set(0, on);
    /// <summary>The red power LED.</summary>
    public static bool Power(bool on) => Set(1, on);
}

public enum BusStatus { Ok = 0, NoAcknowledge = 1, ClockStretch = 2, Timeout = 3, NotSupported = 4 }

/// <summary>A device on an I2C bus (bus 1 = header pins 3/5, SDA/SCL).</summary>
public sealed class I2cDevice
{
    public int Bus { get; }
    public int Address { get; }
    public int SpeedHz { get; set; } = 100_000;
    public BusStatus LastStatus { get; private set; }

    public I2cDevice(int bus, int address) { Bus = bus; Address = address; }
    public static I2cDevice Create(int bus, int address) => new(bus, address);

    BusStatus Run(byte[]? tx, int txLen, byte[]? rx, int rxLen)
    {
        int r;
        if (SimBoard.Active) r = SimBoard.I2c(Bus, Address, tx, txLen, rx, rxLen);
        else
        {
            try { r = HwNative.HwI2c(Bus, Address, SpeedHz, tx, txLen, rx, rxLen); }
            catch (Exception e) when (SimBoard.Fallback(e)) { r = SimBoard.I2c(Bus, Address, tx, txLen, rx, rxLen); }
        }
        LastStatus = r is >= 0 and <= 3 ? (BusStatus)r : BusStatus.NotSupported;
        return LastStatus;
    }

    public bool Write(byte[] data) => Run(data, data.Length, null, 0) == BusStatus.Ok;
    public bool Read(byte[] buffer) => Run(null, 0, buffer, buffer.Length) == BusStatus.Ok;
    public bool WriteRead(byte[] write, byte[] read) => Run(write, write.Length, read, read.Length) == BusStatus.Ok;
    public bool WriteByte(byte value) => Write(new[] { value });
    public int ReadByte() { var b = new byte[1]; return Read(b) ? b[0] : -1; }
    /// <summary>Write a register index, read <paramref name="count"/> bytes back (null on failure).</summary>
    public byte[]? ReadRegister(byte register, int count = 1)
    {
        var buf = new byte[count];
        return WriteRead(new[] { register }, buf) ? buf : null;
    }

    /// <summary>Addresses that acknowledge on <paramref name="bus"/> (0x08-0x77).</summary>
    public static List<int> Scan(int bus)
    {
        var found = new List<int>();
        for (int a = 0x08; a < 0x78; a++)
        {
            var d = new I2cDevice(bus, a);
            if (d.ReadByte() >= 0) found.Add(a);
            else if (d.LastStatus is BusStatus.Timeout or BusStatus.NotSupported) break;
        }
        return found;
    }
}

/// <summary>A device on SPI0 (chip select 0 = GPIO 8, 1 = GPIO 7; MOSI 10, MISO 9, SCLK 11).</summary>
public sealed class SpiDevice
{
    public int Bus { get; }
    public int ChipSelect { get; }
    public int ClockHz { get; set; }
    public int Mode { get; set; }
    public BusStatus LastStatus { get; private set; }

    public SpiDevice(int bus, int chipSelect, int clockHz = 1_000_000, int mode = 0)
    { Bus = bus; ChipSelect = chipSelect; ClockHz = clockHz; Mode = mode; }

    /// <summary>Send <paramref name="write"/> while receiving <paramref name="read"/> (same length).</summary>
    public bool TransferFullDuplex(byte[] write, byte[] read)
    {
        int len = Math.Max(write.Length, read.Length), r;
        if (SimBoard.Active) r = SimBoard.Spi(write, read, len);
        else
        {
            try { r = HwNative.HwSpi(Bus, ChipSelect, ClockHz, Mode, write, read, len); }
            catch (Exception e) when (SimBoard.Fallback(e)) { r = SimBoard.Spi(write, read, len); }
        }
        LastStatus = r is >= 0 and <= 3 ? (BusStatus)r : BusStatus.NotSupported;
        return LastStatus == BusStatus.Ok;
    }
    public bool Write(byte[] data) => TransferFullDuplex(data, new byte[data.Length]);
}

/// <summary>One of the Pi 4's extra serial ports (2: GPIO 0/1, 3: 4/5, 4: 8/9, 5: 12/13; TX/RX).</summary>
public sealed class SerialPort : IDisposable
{
    public int Port { get; }
    public int BaudRate { get; }
    public bool IsOpen { get; private set; }

    public SerialPort(int port, int baudRate = 115200) { Port = port; BaudRate = baudRate; }

    int Op(int op, byte[]? buf, int len)
    {
        if (SimBoard.Active) return SimBoard.Uart(op, Port, buf, len);
        try { return HwNative.HwUart(op, Port, BaudRate, buf, len); }
        catch (Exception e) when (SimBoard.Fallback(e)) { return SimBoard.Uart(op, Port, buf, len); }
    }

    public bool Open() { IsOpen = Op(0, null, 0) == 0; return IsOpen; }
    public void Close() { Op(3, null, 0); IsOpen = false; }
    public void Dispose() => Close();
    public int Write(byte[] data) => Op(1, data, data.Length);
    /// <summary>Send ASCII text (other characters become '?').</summary>
    public int Write(string text)
    {
        var b = new byte[text.Length];
        for (int i = 0; i < text.Length; i++) b[i] = text[i] < 128 ? (byte)text[i] : (byte)'?';
        return Write(b);
    }
    /// <summary>Bytes received so far (does not wait).</summary>
    public int Read(byte[] buffer) => Op(2, buffer, buffer.Length);
}
