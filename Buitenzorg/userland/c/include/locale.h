/* Buitenzorg C runtime (userland/c): locale.h -- the subset ported programs need. */
#ifndef BZ_LOCALE_H
#define BZ_LOCALE_H
#define LC_ALL 6
char *setlocale(int category, const char *locale);
#endif
