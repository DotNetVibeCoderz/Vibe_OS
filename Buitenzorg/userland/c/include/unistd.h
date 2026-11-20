/* Buitenzorg C runtime (userland/c): unistd.h -- the subset ported programs need. */
#ifndef BZ_UNISTD_H
#define BZ_UNISTD_H
#include <stddef.h>
#include <sys/types.h>
int usleep(unsigned usec);
unsigned sleep(unsigned sec);
int access(const char *path, int mode);
int isatty(int fd);
int close(int fd);
ssize_t read(int fd, void *p, size_t n);
ssize_t write(int fd, const void *p, size_t n);
char *getcwd(char *buf, size_t n);
int chdir(const char *path);
#define F_OK 0
#define R_OK 4
#define W_OK 2
#define X_OK 1
#endif
