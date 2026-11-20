/* Buitenzorg C runtime: program entry. `_start` moves onto a 1 MiB stack of
 * our own (the kernel gives 128 KiB), splits the argument string (PROC_ARGS)
 * into argv -- double quotes group words -- and calls main(argc, argv). */
#include <bz.h>
#include <stdlib.h>

int main(int argc, char **argv);

#define STACK_SIZE (1u << 20)
__attribute__((aligned(16))) char bz_stack[STACK_SIZE];

int bz_argc;
char **bz_argv;
static char args[4096];
static char *argv_store[64];

void bz_start_c(void)
{
    uint64_t n = bz_syscall(BZ_PROC_ARGS, (uint64_t)args, sizeof args - 1, 0);
    if (BZ_IS_ERROR(n) || n >= sizeof args) n = 0;
    args[n] = 0;
    int argc = 0;
    argv_store[argc++] = "program";
    char *p = args;
    while (*p && argc < 63) {
        while (*p == ' ') p++;
        if (!*p) break;
        char *start, *end;
        if (*p == '"') {
            start = ++p;
            while (*p && *p != '"') p++;
        } else {
            start = p;
            while (*p && *p != ' ') p++;
        }
        end = p;
        if (*p) p++;
        *end = 0;
        argv_store[argc++] = start;
    }
    argv_store[argc] = 0;
    bz_argc = argc;
    bz_argv = argv_store;
    exit(main(argc, argv_store));
}

#if defined(__x86_64__)
__asm__(".section .text._start,\"ax\",@progbits\n"
        ".globl _start\n"
        "_start:\n"
        "  lea bz_stack+0x100000(%rip), %rsp\n"
        "  and $-16, %rsp\n"
        "  xor %ebp, %ebp\n"
        "  call bz_start_c\n"
        "  ud2\n");
#elif defined(__aarch64__)
__asm__(".section .text._start,\"ax\",%progbits\n"
        ".globl _start\n"
        "_start:\n"
        "  adrp x0, bz_stack\n"
        "  add x0, x0, :lo12:bz_stack\n"
        "  mov x1, #0x100000\n"
        "  add x0, x0, x1\n"
        "  mov sp, x0\n"
        "  mov x29, #0\n"
        "  bl bz_start_c\n"
        "  brk #0\n");
#elif defined(__riscv)
__asm__(".section .text._start,\"ax\",@progbits\n"
        ".globl _start\n"
        "_start:\n"
        "  .option push\n"
        "  .option norelax\n"
        "  la sp, bz_stack\n"
        "  li t0, 0x100000\n"
        "  add sp, sp, t0\n"
        "  .option pop\n"
        "  li s0, 0\n"
        "  call bz_start_c\n"
        "  ebreak\n");
#endif
