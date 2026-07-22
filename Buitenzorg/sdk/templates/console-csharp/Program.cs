// Console System App template (requirements.md §13.2):
// stdin/stdout, arguments, and the syscall/sys-info API.

using Buitenzorg.Runtime.Sys;

if (args is ["--ticks"])
{
    Console.WriteLine(BzSys.Ticks());
    return;
}

Console.WriteLine($"Halo dari Buitenzorg! (ABI v{BzSys.AbiVersion()})");
Console.WriteLine(BzSys.IsOnBuitenzorg
    ? "running on Buitenzorg OS"
    : "running on the host simulation backend");
