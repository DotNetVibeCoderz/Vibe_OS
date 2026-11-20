/* Buitenzorg C runtime (userland/c): strings.h -- the subset ported programs need. */
#ifndef BZ_STRINGS_H
#define BZ_STRINGS_H
#include <stddef.h>
int strcasecmp(const char *a, const char *b);
int strncasecmp(const char *a, const char *b, size_t n);
#endif
