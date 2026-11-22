# __APPNAME__ -- a 2D game in a JavaScript UI script

Created by `bz new game-js`. Catch the falling leaves with the basket (arrow
keys or A/D); three missed leaves end the game, Enter starts again.

A **UI script** opens a window, so it runs as its own process:
`run /disk/__NAME83__.JS` (VS Code's Run and Studio's F5 do this for any
script that calls `window(`). `bz deploy main.js --as __NAME83__.JS --watch`
reloads it on the device every time you save.

Game functions (the same in TypeScript, Python and C# script -- `Ui.KeyDown`, ...):

| function | what it does |
|---|---|
| `keydown(name)` | is the key held now (`"left"`, `"a"`, `"space"`, `"enter"`, ...) |
| `sprite(x, y, rows, scale, color[, color2])` | a pixel picture: rows split by `|`, `#` = color, `o` = color2 |
| `hit(x1, y1, w1, h1, x2, y2, w2, h2)` / `near(x1, y1, x2, y2, r)` | collisions |
| `sound(hz, ms)` | a tone |
| `random(n)`, `ticks()`, `vsync()`, `fullscreen(on)`, `notify(title, body)` | random numbers, time, the display, notifications |

The script language is small: integers, strings, functions, `if` / `while` /
`for` -- no arrays, floats or `+=`. Guide: `docs/script-ui.md`.
