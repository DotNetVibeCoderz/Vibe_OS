/* Buitenzorg C runtime (userland/c): dirent.h -- the subset ported programs need. */
#ifndef BZ_DIRENT_H
#define BZ_DIRENT_H
struct dirent { char d_name[256]; };
typedef struct bz_dir DIR;
DIR *opendir(const char *path);
struct dirent *readdir(DIR *d);
int closedir(DIR *d);
#endif
