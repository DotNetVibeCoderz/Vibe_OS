// Desktop App template (requirements.md §11.2, §13.2): a window that draws
// its UI through the Buitenzorg window syscalls. Build with bflat + bzstart
// shim into a static ELF, place on /disk, and launch with `run <name>`.

using System;

unsafe class App
{
    const uint Bg = 0x141C16;
    const uint Accent = 0x4FA33F;
    const uint Text = 0xC8E9B0;

    static void Main()
    {
        Console.WriteLine("[app] starting desktop app");
        uint win = BzUi.CreateWindow("Desktop App", 360, 220);

        BzUi.Clear(win, Bg);
        BzUi.Fill(win, 0, 0, 360, 30, Accent);
        BzUi.Text(win, 10, 8, "Halo dari Desktop App!", 0x0B120B);
        BzUi.Text(win, 10, 50, "Digambar via window syscall Buitenzorg.", Text);
        BzUi.Text(win, 10, 74, "Edit app.cs untuk memulai.", Text);
        BzUi.Present(win);

        Bz_wait();
        Console.WriteLine("[app] exiting");
    }

    // Keep the window up briefly (a real app would loop on BzUi.ReadKey()).
    static void Bz_wait() => BzUi.Sleep(36);
}
