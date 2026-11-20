/* Buitenzorg C runtime (userland/c): errno.h -- the subset ported programs need. */
#ifndef BZ_ERRNO_H
#define BZ_ERRNO_H
extern int errno;
#define ENOENT 2
#define EIO 5
#define ENOMEM 12
#define EACCES 13
#define EEXIST 17
#define ENOTDIR 20
#define EISDIR 21
#define ENOSPC 28
#define EPERM 1
#define EAGAIN 11
#define EBADF 9
#define EINVAL 22
#define ERANGE 34
#endif
