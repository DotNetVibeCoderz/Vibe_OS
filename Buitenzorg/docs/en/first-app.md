# First App — Getting Started (English)

This document explains how to create and run a simple application for Buitenzorg using the SDK templates.

Create a new console app

1. Scaffold:
   dotnet run --project sdk\bz -- new console-csharp MyApp

2. Build the host-side project and package the userland binary using bflat (the SDK templates contain build instructions). The scripts folder provides scripts to build sample apps automatically.

Accessing system services

- The runtime exposes a stable syscall ABI and managed wrappers that let your app interact with windowing, drawing, filesystem, networking (UDP), audio and packages.
- See docs/abi.md and runtime/Buitenzorg.Runtime/Sys for the exact APIs and struct layouts.

Examples

- UI: use the DrawCmd-based windowing API to create simple windows and draw text/rectangles.
- File I/O: use the FS_* syscalls (wrapper libraries provide higher-level streams).

Testing

Use the HostSyscalls backend (dotnet run --project runtime/samples/HelloBuitenzorg) to run and iterate your app on the host before packaging for the emulator.