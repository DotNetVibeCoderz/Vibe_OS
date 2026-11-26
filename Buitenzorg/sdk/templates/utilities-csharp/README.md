# __APPNAME__ -- a system utility in C#

Created by `bz new utilities-csharp`. Three tabs over Buitenzorg.App's system
access, refreshed every two seconds:

* **Proses** -- `Processes.List()`; *Akhiri proses* calls `Processes.Kill(pid)`
  (ending another program needs the `system` permission in `app.manifest`).
* **Sistem** -- `SystemInfo`: uptime, kernel heap (with a bar), memory, tasks.
* **Disk** -- files and bytes per top-level folder of a mount
  (`Files.List` + `Files.Size`, three folder levels deep).

Off the device the same code runs against sample processes and an in-memory
file system, so `--selftest` works on the PC:

```
dotnet build
dotnet bin/Debug/net10.0/__NAME83__.dll --selftest
bz deploy bin/Debug/net10.0/__NAME83__.dll
```

On the device: `run __NAME83__`.
