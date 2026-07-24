# Buitenzorg OS - GDB init for kernel debugging.
#
# Loaded by scripts/debug-kernel.ps1 / .sh, which start QEMU paused with a GDB
# stub on :1234 and point GDB at the kernel ELF (with symbols). You can also use
# it by hand:
#
#   gdb -x scripts/debug-kernel.gdb \
#       kernel/target/x86_64-unknown-none/release/bzkernel
#
# then, inside gdb:  target remote :1234   (the wrapper scripts do this for you)

set pagination off
set disassembly-flavor att
set architecture i386:x86-64

# The bootloader maps the kernel to a high virtual address; symbols in the ELF
# are already at those addresses, so no offset fixup is needed for a PIE static
# link at a fixed base. If a build ever relocates, re-point with:
#   add-symbol-file <elf> <text-addr>

define bz-break-main
  # Break at the kernel entry so you land right after the bootloader handoff.
  break kernel_main
  echo \n[bz] breakpoint set at kernel_main - type 'continue'\n
end

define bz-faults
  # Break on the fault handlers, so a crash stops in the debugger instead of
  # scrolling a rodata dump past you.
  break page_fault_handler
  break double_fault_handler
  break general_protection_handler
  echo \n[bz] breakpoints set on page/double/GP fault handlers\n
end

define bz-regs
  info registers rax rbx rcx rdx rsi rdi rbp rsp rip
end

define bz-help
  echo \nBuitenzorg GDB helpers:\n
  echo "  bz-break-main   break at kernel_main\n"
  echo "  bz-faults       break on the fault handlers\n"
  echo "  bz-regs         compact register dump\n"
  echo "  (standard gdb:  break <fn> | continue | stepi | bt | x/i $pip)\n\n"
end

echo \n=== Buitenzorg kernel debugger ===\n
bz-help
