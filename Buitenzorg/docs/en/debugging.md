# Debugging the Kernel (English)

This guide covers common debugging workflows for Buitenzorg kernel development using QEMU and GDB.

Quick GDB flow

1. Start QEMU as a GDB server (suspend CPU at start):
   export QEMU_EXTRA='-s -S'
   cargo run --release -p bzimage -- --run

2. Attach gdb from another terminal:
   gdb kernel/target/x86_64-unknown-none/release/bzkernel
   (gdb) target remote :1234

Kernel symbols

The built kernel image with symbols is located under kernel/target/x86_64-unknown-none/release/bzkernel. Use the same build type as the one run by bzimage.

Logging & serial

QEMU’s serial output provides kernel logs and milestone markers used in smoke tests. Use serial + framebuffer together to inspect both headless and graphical behavior.

Profiling

The repo includes simple profiling support (timestamp zones). See docs in kernel source for how to enable zone profiling and gather traces.