# __APPNAME__ -- local AI app (C#, managed)

Created by `bz new ai-csharp`. A managed C# app (an ordinary `dotnet build`
assembly, run on the device by RustCLR) that uses **Buitenzorg.AI**, the OS's
built-in AI. Everything runs on the machine; nothing goes to the network.

- `LanguageModel.Load()` opens `/disk/STORIES.GGU` (stories260K, a small model
  that writes short children's stories in English) on the GPU compute backend
  when the machine has one, else on the CPU. Any llama-architecture GGUF
  (F32/F16/Q8_0/Q4_0) can be passed as a path.
- `Begin(prompt, maxTokens).Next()` gives the answer piece by piece. Call it
  from a `DispatcherTimer` so the text appears while it is written;
  `Generate(...)` returns it all at once, and `Stream(...)` is the
  `IEnumerable<string>` form.
- `Vision.Edges` / `Grayscale` / `Histogram` / `Sharpness` work on `int[]`
  ARGB pixels.

## Build, test, run

```powershell
dotnet build -c Release
dotnet bin/Release/net10.0/__NAME83__.dll --selftest   # on the PC a simulated model answers
bz deploy bin/Release/net10.0/__NAME83__.dll --as __NAME83__.DLL   # to a device (docs/bz-link.md)
```

On the device: `run __NAME83__`. The assembly name must stay an 8.3
upper-case name, because it is the file name on `/disk`.

Guide: `docs/ai-compute.md`. A full example is the **Asisten AI** app
(`userland/clr-apps/MAsisten`).
