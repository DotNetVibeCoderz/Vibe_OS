// Buitenzorg OS — v0.4 "Tunas" milestone program.
//
// Compiled with bflat (NativeAOT/ILC) using --stdlib:zero --os:linux: no GC,
// no JIT, just C# compiled ahead-of-time to a static ELF. zerolib's Console
// uses the Linux write/exit syscall ABI, which the Buitenzorg kernel maps
// onto its own syscall table (kernel/bzkernel/src/usermode.rs).
//
// Build: scripts/build-hello-csharp.ps1 (or .sh) → hello.elf

using System;

class Program
{
    static void Main()
    {
        Console.WriteLine("Hello from C#!");
        Console.WriteLine("Buitenzorg OS: C# di ring 3, kernel Rust di ring 0.");
        Console.WriteLine("v0.4 'Tunas' -- benih telah bertunas.");
    }
}
