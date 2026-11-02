// Buitenzorg.Devices (native) — board I/O for freestanding C# apps (bflat
// --stdlib:zero): GPIO, I2C, SPI, serial ports, the built-in LEDs and the
// board's sensors, over the HW_* syscalls. The same API as the managed
// Buitenzorg.Devices in BZAPP.DLL, so code moves between the two paths.
//
// On a Raspberry Pi 4 every call reaches the hardware; on any other machine
// Board.IsAvailable is false and the calls fail harmlessly (NOSYS).
//
// zerolib rules kept here: no static reference fields, no delegates, no
// stores into object[]; byte[] buffers are passed with `fixed`.

using System;
using System.Runtime.InteropServices;

namespace Buitenzorg.Devices
{
    internal static unsafe class Hw
    {
        [DllImport("*")] internal static extern ulong bz_hw_board(ulong query, ulong arg);
        [DllImport("*")] internal static extern ulong bz_hw_gpio(ulong op, ulong pin, ulong value);
        [DllImport("*")] internal static extern ulong bz_hw_led(ulong led, ulong on);
        [DllImport("*")] internal static extern ulong bz_hw_i2c(byte* t);
        [DllImport("*")] internal static extern ulong bz_hw_spi(byte* t);
        [DllImport("*")] internal static extern ulong bz_hw_uart(ulong op, byte* t);

        internal const ulong NoSys = 0xFFFF_FFFF_FFFF_FFFFUL;
        internal const ulong ErrLow = 0xFFFF_FFFF_FFFF_FF00UL;

        /// <summary>Fill a 48-byte HwTransfer at <paramref name="t"/>.</summary>
        internal static void Transfer(byte* t, uint bus, uint addr, uint hz, uint mode, byte* tx, int txLen, byte* rx, int rxLen)
        {
            uint* u = (uint*)t;
            u[0] = bus; u[1] = addr; u[2] = hz; u[3] = mode;
            ulong* q = (ulong*)(t + 16);
            q[0] = (ulong)tx; q[1] = (ulong)(txLen < 0 ? 0 : txLen);
            q[2] = (ulong)rx; q[3] = (ulong)(rxLen < 0 ? 0 : rxLen);
        }
    }

    /// <summary>Board information and built-in sensors.</summary>
    public static class Board
    {
        /// <summary>Firmware clock ids for <see cref="ClockHz"/>.</summary>
        public const int ClockArm = 3, ClockCore = 4, ClockV3d = 5, ClockEmmc2 = 12;

        static long Q(ulong query, ulong arg)
        {
            ulong v = Hw.bz_hw_board(query, arg);
            return v >= Hw.ErrLow ? -1 : (long)v;
        }

        /// <summary>True on a board with I/O support (a Raspberry Pi 4).</summary>
        public static bool IsAvailable => Q(0, 0) == 1;
        /// <summary>SoC temperature in milli-degrees Celsius (-1 = unknown).</summary>
        public static int TemperatureMilliC => (int)Q(1, 0);
        /// <summary>The firmware's throttling temperature, milli-degrees Celsius.</summary>
        public static int MaxTemperatureMilliC => (int)Q(2, 0);
        /// <summary>A clock rate in Hz (see the Clock* ids).</summary>
        public static long ClockHz(int id) => Q(3, (ulong)id);
        /// <summary>A voltage in microvolts: 1 core, 2-4 SDRAM.</summary>
        public static long VoltageMicroV(int id) => Q(4, (ulong)id);
        /// <summary>Firmware throttle flags (bit 0 under-voltage, 1 capped, 2 throttled, 3 soft limit; +16 = has occurred).</summary>
        public static int Throttled => (int)Q(5, 0);
        public static int Revision => (int)Q(6, 0);
        public static long Serial => Q(7, 0);
        public static long MemoryBytes => Q(8, 0);
        public static int GpioCount => (int)Q(9, 0);
    }

    public enum PinMode { Input = 0, Output = 1, Alt0 = 2, Alt1 = 3, Alt2 = 4, Alt3 = 5, Alt4 = 6, Alt5 = 7 }
    public enum PullMode { None = 0, Up = 1, Down = 2 }

    /// <summary>GPIO pins (BCM numbering). Header pins 0-27 except 14/15, and 42 (the activity LED).</summary>
    public sealed class GpioController
    {
        public bool SetPinMode(int pin, PinMode mode) => Hw.bz_hw_gpio(0, (ulong)pin, (ulong)mode) == 0;
        public bool OpenPin(int pin, PinMode mode) => SetPinMode(pin, mode);
        public bool Write(int pin, bool high) => Hw.bz_hw_gpio(1, (ulong)pin, high ? 1UL : 0UL) == 0;
        public bool Read(int pin) => Hw.bz_hw_gpio(2, (ulong)pin, 0) == 1;
        public bool SetPull(int pin, PullMode pull) => Hw.bz_hw_gpio(3, (ulong)pin, (ulong)pull) == 0;
        public PinMode GetPinMode(int pin)
        {
            ulong v = Hw.bz_hw_gpio(4, (ulong)pin, 0);
            return v > 7 ? PinMode.Input : (PinMode)(int)v;
        }
    }

    /// <summary>The board's two LEDs.</summary>
    public static class Led
    {
        public static bool Activity(bool on) => Hw.bz_hw_led(0, on ? 1UL : 0UL) == 0;
        public static bool Power(bool on) => Hw.bz_hw_led(1, on ? 1UL : 0UL) == 0;
    }

    /// <summary>Results of I2C / SPI transfers.</summary>
    public enum BusStatus { Ok = 0, NoAcknowledge = 1, ClockStretch = 2, Timeout = 3, NotSupported = 4 }

    /// <summary>One device on an I2C bus (bus 1 = header pins 3/5).</summary>
    public sealed unsafe class I2cDevice
    {
        public int Bus;
        public int Address;
        public int SpeedHz;
        public BusStatus LastStatus;

        public I2cDevice(int bus, int address) { Bus = bus; Address = address; SpeedHz = 100_000; }

        BusStatus Run(byte* tx, int txLen, byte* rx, int rxLen)
        {
            byte* t = stackalloc byte[48];
            Hw.Transfer(t, (uint)Bus, (uint)Address, (uint)SpeedHz, 0, tx, txLen, rx, rxLen);
            ulong r = Hw.bz_hw_i2c(t);
            LastStatus = r >= Hw.ErrLow ? BusStatus.NotSupported : (BusStatus)(int)r;
            return LastStatus;
        }

        public bool Write(byte[] data, int len) { fixed (byte* p = data) return Run(p, len, null, 0) == BusStatus.Ok; }
        public bool Read(byte[] buf, int len) { fixed (byte* p = buf) return Run(null, 0, p, len) == BusStatus.Ok; }
        public bool WriteRead(byte[] tx, int txLen, byte[] rx, int rxLen)
        {
            fixed (byte* a = tx) fixed (byte* b = rx) return Run(a, txLen, b, rxLen) == BusStatus.Ok;
        }
        public bool WriteByte(byte v) { byte* p = stackalloc byte[1]; p[0] = v; return Run(p, 1, null, 0) == BusStatus.Ok; }
        public int ReadByte() { byte* p = stackalloc byte[1]; return Run(null, 0, p, 1) == BusStatus.Ok ? p[0] : -1; }
        /// <summary>Read register <paramref name="reg"/> (write the index, read one byte).</summary>
        public int ReadRegister(byte reg)
        {
            byte* a = stackalloc byte[1]; byte* b = stackalloc byte[1];
            a[0] = reg;
            return Run(a, 1, b, 1) == BusStatus.Ok ? b[0] : -1;
        }

        /// <summary>Does a device acknowledge <paramref name="address"/>? (reads one byte)</summary>
        public static bool Probe(int bus, int address)
        {
            var d = new I2cDevice(bus, address);
            return d.ReadByte() >= 0;
        }
    }

    /// <summary>A device on SPI0 (chip select 0 = GPIO 8, 1 = GPIO 7).</summary>
    public sealed unsafe class SpiDevice
    {
        public int Bus;
        public int ChipSelect;
        public int ClockHz;
        public int Mode;
        public BusStatus LastStatus;

        public SpiDevice(int bus, int chipSelect, int clockHz, int mode)
        { Bus = bus; ChipSelect = chipSelect; ClockHz = clockHz; Mode = mode; }

        /// <summary>Send <paramref name="tx"/> while receiving into <paramref name="rx"/> (<paramref name="len"/> bytes).</summary>
        public bool TransferFullDuplex(byte[] tx, byte[] rx, int len)
        {
            byte* t = stackalloc byte[48];
            fixed (byte* a = tx) fixed (byte* b = rx)
            {
                Hw.Transfer(t, (uint)Bus, (uint)ChipSelect, (uint)ClockHz, (uint)Mode, a, len, b, len);
                ulong r = Hw.bz_hw_spi(t);
                LastStatus = r >= Hw.ErrLow ? BusStatus.NotSupported : (BusStatus)(int)r;
            }
            return LastStatus == BusStatus.Ok;
        }
    }

    /// <summary>One of the Pi 4's extra serial ports (2: GPIO 0/1, 3: 4/5, 4: 8/9, 5: 12/13; TX/RX).</summary>
    public sealed unsafe class SerialPort
    {
        public int Port;
        public int BaudRate;
        public bool IsOpen;

        public SerialPort(int port, int baudRate) { Port = port; BaudRate = baudRate; }

        ulong Op(ulong op, byte* tx, int txLen, byte* rx, int rxLen)
        {
            byte* t = stackalloc byte[48];
            Hw.Transfer(t, (uint)Port, 0, (uint)BaudRate, 0, tx, txLen, rx, rxLen);
            return Hw.bz_hw_uart(op, t);
        }

        public bool Open() { IsOpen = Op(0, null, 0, null, 0) == 0; return IsOpen; }
        public void Close() { Op(3, null, 0, null, 0); IsOpen = false; }
        public int Write(byte[] data, int len)
        {
            fixed (byte* p = data) { ulong r = Op(1, p, len, null, 0); return r >= Hw.ErrLow ? -1 : (int)r; }
        }
        /// <summary>Bytes received so far (does not wait), up to <paramref name="len"/>.</summary>
        public int Read(byte[] buf, int len)
        {
            fixed (byte* p = buf) { ulong r = Op(2, null, 0, p, len); return r >= Hw.ErrLow ? -1 : (int)r; }
        }
    }
}
