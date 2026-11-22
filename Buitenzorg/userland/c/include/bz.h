/* Buitenzorg system calls for C programs (the numbers of kernel/abi, ABI v1).
 * x86-64: SYSCALL, rax = number, rdi/rsi/rdx = arguments; the kernel also
 * uses r8/r9/r10 and does not restore the argument registers (both rules
 * learnt the hard way, see CLAUDE.md). ARM64: SVC #0, x8 = number, x0-x2.
 * RISC-V: ECALL, a7 = number, a0-a2. */
#ifndef BZ_H
#define BZ_H

#include <stdint.h>
#include <stddef.h>

enum {
    BZ_DEBUG_WRITE = 1, BZ_EXIT = 2, BZ_YIELD = 3, BZ_TICKS = 4, BZ_FB_INFO = 5,
    BZ_WIN_CREATE = 6, BZ_WIN_CMD = 7, BZ_WIN_PRESENT = 8, BZ_KEY_READ = 9,
    BZ_MMAP = 13, BZ_MPROTECT = 14, BZ_MUNMAP = 15,
    BZ_FS_READ = 30, BZ_IS_INTERACTIVE = 31, BZ_FS_WRITE = 32, BZ_CLOCK_RTC = 33,
    BZ_PROC_ARGS = 42, BZ_CLOCK_NANOS = 44, BZ_WIN_EVENT = 45,
    BZ_FS_OP = 54, BZ_KEY_EVENT = 59, BZ_KEY_STATE = 60, BZ_SLEEP_UNTIL = 61,
    BZ_AUDIO_WRITE = 62, BZ_IDLE_INHIBIT = 66, BZ_FS_READ_AT = 70, BZ_WIN_CTL = 74,
};

/* WIN_CTL operations: a0 = op | window << 8 */
enum { BZ_WIN_FULLSCREEN = 0, BZ_WIN_SURFACE = 1, BZ_WIN_SURFACE_PRESENT = 2, BZ_WIN_VSYNC_WAIT = 3, BZ_WIN_VSYNC_INFO = 4, BZ_WIN_SURFACE_SIZE = 5 };
enum { BZ_FS_MKDIR = 0, BZ_FS_REMOVE = 1, BZ_FS_RENAME = 2, BZ_FS_STAT = 3 };
enum { BZ_DRAW_FILL_RECT = 0, BZ_DRAW_TEXT = 1, BZ_DRAW_CLEAR = 2, BZ_DRAW_BLIT = 7 };
#define BZ_WIN_CLIENT_SIZE (1ull << 63)
/* errors come back in the top of the range (u64::MAX - n) */
#define BZ_IS_ERROR(v) ((uint64_t)(v) >= (uint64_t)-4096)

struct bz_draw_cmd {
    uint64_t op;
    int32_t x, y, w, h;
    uint32_t color, pad;
    uint64_t text_ptr, text_len;
};

static inline uint64_t bz_syscall(uint64_t nr, uint64_t a0, uint64_t a1, uint64_t a2)
{
#if defined(__x86_64__)
    uint64_t ret;
    __asm__ volatile("syscall"
                     : "=a"(ret), "+D"(a0), "+S"(a1), "+d"(a2)
                     : "a"(nr)
                     : "rcx", "r8", "r9", "r10", "r11", "memory");
    return ret;
#elif defined(__aarch64__)
    register uint64_t x8 __asm__("x8") = nr;
    register uint64_t x0 __asm__("x0") = a0;
    register uint64_t x1 __asm__("x1") = a1;
    register uint64_t x2 __asm__("x2") = a2;
    __asm__ volatile("svc #0" : "+r"(x0), "+r"(x1), "+r"(x2) : "r"(x8) : "memory");
    return x0;
#elif defined(__riscv)
    register uint64_t a7 __asm__("a7") = nr;
    register uint64_t r0 __asm__("a0") = a0;
    register uint64_t r1 __asm__("a1") = a1;
    register uint64_t r2 __asm__("a2") = a2;
    __asm__ volatile("ecall" : "+r"(r0), "+r"(r1), "+r"(r2) : "r"(a7) : "memory");
    return r0;
#else
#error "unsupported architecture"
#endif
}

static inline void bz_write(const char *s, size_t n) { bz_syscall(BZ_DEBUG_WRITE, (uint64_t)s, n, 0); }
static inline uint64_t bz_nanos(void) { return bz_syscall(BZ_CLOCK_NANOS, 0, 0, 0); }
static inline void bz_yield(void) { bz_syscall(BZ_YIELD, 0, 0, 0); }
__attribute__((noreturn)) void bz_exit(int code);

#endif
