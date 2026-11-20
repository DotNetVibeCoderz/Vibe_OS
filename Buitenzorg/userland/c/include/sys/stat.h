/* Buitenzorg C runtime (userland/c): sys/stat.h -- the subset ported programs need. */
#ifndef BZ_SYS_STAT_H
#define BZ_SYS_STAT_H
#include <sys/types.h>
struct stat { off_t st_size; mode_t st_mode; time_t st_mtime; };
#define S_IFDIR 0040000
#define S_IFREG 0100000
#define S_ISDIR(m) (((m) & 0170000) == S_IFDIR)
#define S_ISREG(m) (((m) & 0170000) == S_IFREG)
int stat(const char *path, struct stat *st);
int mkdir(const char *path, mode_t mode);
#endif
