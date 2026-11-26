# __APPNAME__ -- Minesweeper in C#

Created by `bz new minesweeper-csharp`. A 9 x 9 field with 10 mines: open
every safe square. Numbers count the mines around a square; an empty square
opens its neighbours. Switch on *Mode bendera* (or press F) to flag a mine.
The first click is always safe.

* **Rules** (`Field`) are plain C# with no UI: mines placed after the first
  click, a flood fill with a stack instead of recursion, flags, won / lost.
* **The window** (`MinesWindow`) is a Buitenzorg.UI `Grid` of 81 buttons;
  `Refresh()` redraws them from the field after every click.
* `--selftest` checks the rules with hand-placed mines and clicks the window.

```
dotnet build
dotnet bin/Debug/net10.0/__NAME83__.dll --selftest
bz deploy bin/Debug/net10.0/__NAME83__.dll
```

On the device: `run __NAME83__`.
