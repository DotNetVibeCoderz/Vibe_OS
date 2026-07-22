// "Hello from C#!" — the roadmap v0.4 "Tunas" milestone program.
// Runs today against the host-simulation syscall backend; the same code will
// run unmodified on Buitenzorg OS once the managed runtime boots on bare metal.

using Buitenzorg.Runtime.Sys;

BzSys.DebugWrite("Hello from C#!\n");
BzSys.DebugWrite($"Buitenzorg syscall ABI v{BzSys.AbiVersion()} — backend: " +
                 $"{(BzSys.IsOnBuitenzorg ? "native kernel" : "host simulation")}\n");
BzSys.DebugWrite($"ticks since boot: {BzSys.Ticks()}\n");
