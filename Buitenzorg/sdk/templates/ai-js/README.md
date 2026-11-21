# __APPNAME__ -- local AI in a JavaScript UI script

Created by `bz new ai-js`. A **UI script**: it opens a window (`window(...)`),
so it runs in its own process with `run /disk/__NAME83__.JS`, not with
`script js` (the in-kernel interpreter has no window and no AI). VS Code's
Run and Studio's F5 do this for any script that calls `window(`.

AI functions (the OS's language model, on the machine, GPU when there is one):

| function | what it does |
|---|---|
| `aiload([file])` | open a GGUF model (default `/disk/STORIES.GGU`) |
| `aistart(prompt, tokens[, temp*1000])` + `ainext()` | stream the answer; call `ainext()` once per frame, `""` = done |
| `aigenerate(prompt, tokens[, temp*1000])` | the whole answer at once |
| `aibackend()`, `aiinfo(6)`, `aispeed()` | where it runs, parameter count, tokens per second |

The same functions exist in Python UI scripts, and as `Ai.Load()` /
`Ai.Start()` / `Ai.Next()` / ... in C# script. Guides: `docs/script-ui.md`,
`docs/ai-compute.md`.
