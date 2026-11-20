/* Buitenzorg C runtime (userland/c): signal.h -- the subset ported programs need. */
#ifndef BZ_SIGNAL_H
#define BZ_SIGNAL_H
typedef int sig_atomic_t;
typedef void (*sighandler_t)(int);
#define SIGINT 2
#define SIGSEGV 11
#define SIG_DFL ((sighandler_t)0)
#define SIG_IGN ((sighandler_t)1)
sighandler_t signal(int sig, sighandler_t h);
#endif
