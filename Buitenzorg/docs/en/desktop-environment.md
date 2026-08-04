# Desktop Environment (English)

Overview

The desktop environment provides the shell, theme engine, window manager and a collection of default apps. It is implemented in C# as a userland service and leverages the windowing syscalls for rendering and event handling.

Features

- Multi-desktop workspaces, start menu, system tray, theme switching, and window decorations with rounded corners.
- Built-in apps: calculator, editor, 2048, clock, file manager, piano, image viewer, app store (v0.16 suite).

Customization

Themes are packaged and hot-switchable. The desktop subscribes to package manager events to display the model gallery and app store.