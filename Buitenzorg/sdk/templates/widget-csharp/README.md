# __APPNAME__ -- a desktop widget in C#

Created by `bz new widget-csharp`. A window whose title starts with
`widget:` is docked on the desktop's widget board: no title bar, not in the
taskbar, left alone by "tidy windows". This one shows the time and date.

```
dotnet build
dotnet bin/Debug/net10.0/__NAME83__.dll --selftest
bz deploy bin/Debug/net10.0/__NAME83__.dll
```

Keep a widget quiet: every redraw recomposes the desktop, so refresh at most
once a second and only change a `TextBlock` when its text really changed.
