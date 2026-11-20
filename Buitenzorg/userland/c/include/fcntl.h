/* Buitenzorg C runtime (userland/c): fcntl.h -- the subset ported programs need. */
#ifndef BZ_FCNTL_H
#define BZ_FCNTL_H
#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 0100
#define O_TRUNC 01000
#define O_BINARY 0
int open(const char *path, int flags, ...);
#endif
