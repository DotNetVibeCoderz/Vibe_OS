# __APPNAME__ -- a 2D game in C#

Created by `bz new game-csharp`. Catch the falling leaves with the basket
(arrow keys or A/D); three missed leaves end the game, Enter starts again.

* **Rules** (`Catch`) are plain C# with no UI, so they are easy to test.
* **The view** (`CatchView : GameView`) gets `Update()` 60 times a second and
  reads the keys held from `Input` (raw key events, so several keys at once
  work); `Draw()` paints with the `DrawingContext` in playfield units
  (`SRect`, `Sx`, `Sy` scale them to the window).
* `Sfx.Score()` / `Sfx.Lose()` play tones (silent off the device).

Build and test on the PC, then deploy:

```
dotnet build
dotnet bin/Debug/net10.0/__NAME83__.dll --selftest
bz deploy bin/Debug/net10.0/__NAME83__.dll
```

On the device: `run __NAME83__` (or `bz deploy` runs it). Guide: `docs/games.md`.
