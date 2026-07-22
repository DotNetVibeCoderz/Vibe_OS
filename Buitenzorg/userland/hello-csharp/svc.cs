// Buitenzorg OS — v0.5 "Dahan": a C# program that runs as a system *service*
// (a ring-3 process launched by the init manager). Compiled the same way as
// hello.cs (bflat --stdlib:zero + bzstart shim), it does a little "service"
// work and exits, demonstrating a managed process running under the kernel.

using System;

class Service
{
    static void Main()
    {
        Console.WriteLine("[svc-csharp] service starting (ring 3, managed process)");
        for (int i = 1; i <= 3; i++)
        {
            Console.WriteLine("[svc-csharp] serving request:");
            Console.WriteLine(i);
        }
        Console.WriteLine("[svc-csharp] work complete, exiting cleanly");
    }
}
