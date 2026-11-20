/* Buitenzorg C runtime (userland/c): time.h -- the subset ported programs need. */
#ifndef BZ_TIME_H
#define BZ_TIME_H
#include <sys/types.h>
typedef long clock_t;
#define CLOCKS_PER_SEC 1000000
struct tm { int tm_sec, tm_min, tm_hour, tm_mday, tm_mon, tm_year, tm_wday, tm_yday, tm_isdst; };
time_t time(time_t *t);
clock_t clock(void);
struct tm *localtime(const time_t *t);
size_t strftime(char *s, size_t n, const char *fmt, const struct tm *tm);
#endif
