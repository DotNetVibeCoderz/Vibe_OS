# Raspberry Pi board I/O: GPIO, I2C, SPI, serial, LEDs, sensors

Buitenzorg apps on a Raspberry Pi 4 can use the header pins, the I2C and SPI
buses, the extra serial ports, the two LEDs on the board, and the sensors the
board firmware reports. The same API exists for both kinds of C# app:

| | Managed apps (RustCLR, `dotnet build`) | Native apps (bflat, `--stdlib:zero`) |
|---|---|---|
| Library | `Buitenzorg.Devices` in `BZAPP.DLL` (reference `Buitenzorg.App`) | `userland/hello-csharp/bzhw.cs` (add it to the build) |
| Sample | **Lab Perangkat** (`mhwlab`, in *Managed Apps*) | **hwdemo** (`run hwdemo`, ARM64) |

On another machine `Board.IsAvailable` is false and every call fails without
harm, so one program runs anywhere. A managed app started with `dotnet` on a
PC talks to a **simulated board** (`Board.IsSimulated`): plausible sensor
values, GPIO pins that remember what you wrote, a temperature sensor at I2C
address 0x48, and SPI / serial loopback. Use it to develop without the Pi.

## What's on the board

The Pi 4 has no sensor chips for applications on its I2C buses. What it has
built in, and what this library reads, is:

| Built in | API |
|---|---|
| SoC temperature, and the temperature at which it slows down | `Board.Temperature`, `Board.MaxTemperature` |
| Clock rates (ARM cores, VPU core, ...) | `Board.ArmClockHz`, `Board.CoreClockHz`, `Board.ClockHz(id)` |
| Core and SDRAM voltages | `Board.Voltage(1)` (core), `2`-`4` (SDRAM) |
| Under-voltage and throttling, now and since boot | `Board.Throttled` (`ThrottleState` flags) |
| Board revision, serial number, memory | `Board.Revision`, `Board.Serial`, `Board.MemoryBytes` |
| Green activity LED, red power LED | `Led.Activity(on)`, `Led.Power(on)` |

A low supply voltage shows up as `ThrottleState.UnderVoltage`: use a 5 V / 3 A
power supply.

## Pins

BCM GPIO numbers (not header pin numbers):

| Use | GPIO | Header pin |
|---|---|---|
| General I/O | 0-27, except 14 and 15 | see the Pi pinout |
| I2C bus 1: SDA, SCL | 2, 3 | 3, 5 |
| I2C bus 0: SDA, SCL | 0, 1 | 27, 28 (the HAT EEPROM bus) |
| SPI0: MOSI, MISO, SCLK, CE0, CE1 | 10, 9, 11, 8, 7 | 19, 21, 23, 24, 26 |
| Serial port 2 / 3 / 4 / 5: TX, RX | 0/1, 4/5, 8/9, 12/13 | |
| **Not available**: the serial console | 14, 15 | 8, 10 |

GPIO 42 is the activity LED (use `Led.Activity`). A bus switches its pins to
the bus function the first time it is used, so don't use the same pins as GPIO
at the same time.

**The pins are 3.3 V. 5 V on any GPIO pin can damage the Pi.**

## Managed (Buitenzorg.Devices)

```csharp
using Buitenzorg.Devices;

if (!Board.IsAvailable) { Console.WriteLine("no board I/O here"); return; }
Console.WriteLine($"SoC {Board.Temperature:0.0} C, ARM {Board.ArmClockHz / 1_000_000} MHz");

Led.Activity(true);                          // green LED on

var gpio = new GpioController();
gpio.OpenPin(17, PinMode.Output);            // header pin 11
gpio.Write(17, PinValue.High);
gpio.OpenPin(27, PinMode.Input);
gpio.SetPull(27, PullMode.Up);               // a button to ground on GPIO 27
bool pressed = gpio.Read(27) == PinValue.Low;

foreach (int a in I2cDevice.Scan(1)) Console.WriteLine($"I2C device at 0x{a:X2}");
var sensor = I2cDevice.Create(1, 0x48);      // e.g. a TMP102
byte[]? raw = sensor.ReadRegister(0x00, 2);

var spi = new SpiDevice(0, 0, clockHz: 1_000_000, mode: 0);
var rx = new byte[4];
spi.TransferFullDuplex(new byte[] { 1, 2, 3, 4 }, rx);

using var port = new SerialPort(3, 115200);  // TX GPIO 4, RX GPIO 5
port.Open();
port.Write("hello\r\n");
int n = port.Read(new byte[64]);             // does not wait
```

## Native (bzhw.cs)

The same classes, with zerolib's rules: byte arrays and lengths instead of
strings, no exceptions.

```csharp
using Buitenzorg.Devices;

if (!Board.IsAvailable) return 0;
int mc = Board.TemperatureMilliC;            // milli-degrees Celsius
Led.Activity(true);
var gpio = new GpioController();
gpio.OpenPin(17, PinMode.Output);
gpio.Write(17, true);
var dev = new I2cDevice(1, 0x48);
int b = dev.ReadByte();                      // -1 if nothing answered (dev.LastStatus)
```

Build it with `bzhw.cs` (and `bzbcl.cs` for text output) in the source list, as
`hwdemo` does in `scripts/build-hello-csharp-arm64.*`.

## Trying the buses without a sensor

- **SPI loopback:** a jumper wire between GPIO 10 (MOSI, pin 19) and GPIO 9
  (MISO, pin 21). The bytes sent come back. *Uji SPI* in Lab Perangkat says so.
- **Serial loopback:** a jumper between GPIO 4 (TX of port 3, pin 7) and GPIO 5
  (RX, pin 29). *Uji UART3* sends ten bytes and reads them back.
- **I2C:** any I2C module on pins 3/5 (with 3V3 and GND); *Pindai I2C* lists the
  addresses that answer.

## Limits

- Everything is polled: no interrupts on GPIO edges, no DMA. Fine for sensors
  and LEDs; not for high-speed streams.
- I2C writes and reads are separate transfers (a stop between them, no repeated
  start). Most sensors accept this; a few need a repeated start.
- Only I2C buses 0 and 1, and SPI0, are available; no PWM yet.
- Under QEMU `raspi4b` the sensors report fixed values (25 C) and no device
  answers on the buses.

## Underneath

The kernel driver is `kernel/bzkernel/src/platform/virt/rpi_io.rs`, behind the
syscalls `HW_BOARD`, `HW_GPIO`, `HW_I2C`, `HW_SPI`, `HW_UART` and `HW_LED`
(numbers 47-52, `HwTransfer` struct: see `docs/abi.md`). Sensor values come from
the VideoCore firmware through the mailbox.
