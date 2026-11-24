/* Buitenzorg C runtime: a small libc for ported C programs (DOOM first).
 *
 * - memory: a heap in 4 MiB chunks from MMAP (blocks with a size header;
 *   freed blocks go on size-class free lists and are reused)
 * - files: fopen reads the whole file into memory (FS_OP STAT + FS_READ);
 *   files opened for writing are kept in memory and written with FS_WRITE
 *   when flushed / closed. Relative paths are taken from /disk.
 *   stdout / stderr go to DEBUG_WRITE, line by line.
 * - printf family (d i u x X o c s p %, flags, width, precision, h hh l ll
 *   z j t, f e g), sscanf (d i u x s c f), strto*, qsort, ctype, math basics.
 * - time: CLOCK_NANOS (monotonic) and CLOCK_RTC (wall time).
 */
#include <bz.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <errno.h>
#include <math.h>
#include <time.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <signal.h>
#include <locale.h>
#include <dirent.h>

int errno;

/* ---- memory ------------------------------------------------------------- */

#define CHUNK (4u << 20)
#define CLASSES 48

struct block { size_t size; struct block *next; };
static char *heap_ptr, *heap_end;
static struct block *free_lists[CLASSES];

static int size_class(size_t n)
{
    int c = 0;
    size_t s = 16;
    while (s < n && c < CLASSES - 1) { s <<= 1; c++; }
    return c;
}

static void *map(size_t n)
{
    uint64_t r = bz_syscall(BZ_MMAP, n, 3 /* read | write */, 0);
    return BZ_IS_ERROR(r) ? NULL : (void *)r;
}

/* threads (pthread.c) share the heap: one lock around it */
static volatile uint32_t heap_lock;
static void heap_take(void) { int n = 0; while (__atomic_exchange_n(&heap_lock, 1, __ATOMIC_ACQUIRE)) if (++n > 64) { bz_yield(); n = 0; } }
static void heap_give(void) { __atomic_store_n(&heap_lock, 0, __ATOMIC_RELEASE); }
static void *malloc_locked(size_t n);

void *malloc(size_t n)
{
    heap_take();
    void *p = malloc_locked(n);
    heap_give();
    return p;
}

static void *malloc_locked(size_t n)
{
    if (n == 0) n = 1;
    int c = size_class(n + 16);
    size_t size = (size_t)16 << c;
    if (free_lists[c]) {
        struct block *b = free_lists[c];
        free_lists[c] = b->next;
        return (char *)b + 16;
    }
    if (size > CHUNK / 2) {
        struct block *b = map(size);
        if (!b) { errno = ENOMEM; return NULL; }
        b->size = size;
        return (char *)b + 16;
    }
    if (!heap_ptr || (size_t)(heap_end - heap_ptr) < size) {
        heap_ptr = map(CHUNK);
        if (!heap_ptr) { errno = ENOMEM; return NULL; }
        heap_end = heap_ptr + CHUNK;
    }
    struct block *b = (struct block *)heap_ptr;
    heap_ptr += size;
    b->size = size;
    return (char *)b + 16;
}

void free(void *p)
{
    if (!p) return;
    struct block *b = (struct block *)((char *)p - 16);
    int c = size_class(b->size);
    heap_take();
    b->next = free_lists[c];
    free_lists[c] = b;
    heap_give();
}

void *calloc(size_t n, size_t size)
{
    size_t t = n * size;
    void *p = malloc(t);
    if (p) memset(p, 0, t);
    return p;
}

void *realloc(void *p, size_t n)
{
    if (!p) return malloc(n);
    struct block *b = (struct block *)((char *)p - 16);
    if (b->size - 16 >= n) return p;
    void *q = malloc(n);
    if (!q) return NULL;
    memcpy(q, p, b->size - 16);
    free(p);
    return q;
}

/* ---- strings ----------------------------------------------------------------- */

void *memcpy(void *d, const void *s, size_t n) { char *a = d; const char *b = s; while (n--) *a++ = *b++; return d; }
void *memmove(void *d, const void *s, size_t n)
{
    char *a = d; const char *b = s;
    if (a < b) while (n--) *a++ = *b++;
    else { a += n; b += n; while (n--) *--a = *--b; }
    return d;
}
void *memset(void *d, int c, size_t n) { unsigned char *a = d; while (n--) *a++ = (unsigned char)c; return d; }
int memcmp(const void *x, const void *y, size_t n)
{
    const unsigned char *a = x, *b = y;
    for (; n; n--, a++, b++) if (*a != *b) return *a - *b;
    return 0;
}
void *memchr(const void *s, int c, size_t n)
{
    const unsigned char *a = s;
    for (; n; n--, a++) if (*a == (unsigned char)c) return (void *)a;
    return NULL;
}
size_t strlen(const char *s) { size_t n = 0; while (s[n]) n++; return n; }
size_t strnlen(const char *s, size_t m) { size_t n = 0; while (n < m && s[n]) n++; return n; }
char *strcpy(char *d, const char *s) { char *r = d; while ((*d++ = *s++)); return r; }
char *strncpy(char *d, const char *s, size_t n)
{
    size_t i = 0;
    for (; i < n && s[i]; i++) d[i] = s[i];
    for (; i < n; i++) d[i] = 0;
    return d;
}
char *strcat(char *d, const char *s) { strcpy(d + strlen(d), s); return d; }
char *strncat(char *d, const char *s, size_t n)
{
    char *e = d + strlen(d);
    while (n-- && *s) *e++ = *s++;
    *e = 0;
    return d;
}
int strcmp(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}
int strncmp(const char *a, const char *b, size_t n)
{
    for (; n; n--, a++, b++) {
        if (*a != *b) return (unsigned char)*a - (unsigned char)*b;
        if (!*a) return 0;
    }
    return 0;
}
int strcoll(const char *a, const char *b) { return strcmp(a, b); }
int strcasecmp(const char *a, const char *b)
{
    while (*a && tolower((unsigned char)*a) == tolower((unsigned char)*b)) { a++; b++; }
    return tolower((unsigned char)*a) - tolower((unsigned char)*b);
}
int strncasecmp(const char *a, const char *b, size_t n)
{
    for (; n; n--, a++, b++) {
        int x = tolower((unsigned char)*a), y = tolower((unsigned char)*b);
        if (x != y) return x - y;
        if (!x) return 0;
    }
    return 0;
}
char *strchr(const char *s, int c)
{
    for (;; s++) {
        if (*s == (char)c) return (char *)s;
        if (!*s) return NULL;
    }
}
char *strrchr(const char *s, int c)
{
    const char *r = NULL;
    for (;; s++) {
        if (*s == (char)c) r = s;
        if (!*s) return (char *)r;
    }
}
char *strstr(const char *h, const char *n)
{
    size_t l = strlen(n);
    if (!l) return (char *)h;
    for (; *h; h++) if (!strncmp(h, n, l)) return (char *)h;
    return NULL;
}
char *strdup(const char *s) { size_t n = strlen(s) + 1; char *d = malloc(n); if (d) memcpy(d, s, n); return d; }
char *strndup(const char *s, size_t m) { size_t n = strnlen(s, m); char *d = malloc(n + 1); if (d) { memcpy(d, s, n); d[n] = 0; } return d; }
size_t strspn(const char *s, const char *a) { size_t n = 0; while (s[n] && strchr(a, s[n])) n++; return n; }
size_t strcspn(const char *s, const char *r) { size_t n = 0; while (s[n] && !strchr(r, s[n])) n++; return n; }
char *strpbrk(const char *s, const char *a) { s += strcspn(s, a); return *s ? (char *)s : NULL; }
char *strtok(char *s, const char *d)
{
    static char *last;
    if (!s) s = last;
    if (!s) return NULL;
    s += strspn(s, d);
    if (!*s) { last = NULL; return NULL; }
    char *e = s + strcspn(s, d);
    if (*e) { *e = 0; last = e + 1; } else last = NULL;
    return s;
}
char *strerror(int e)
{
    switch (e) {
    case ENOENT: return "no such file";
    case ENOMEM: return "out of memory";
    case EINVAL: return "invalid argument";
    case EACCES: return "permission denied";
    default: return "error";
    }
}

/* ---- ctype --------------------------------------------------------------------- */

int isdigit(int c) { return c >= '0' && c <= '9'; }
int isupper(int c) { return c >= 'A' && c <= 'Z'; }
int islower(int c) { return c >= 'a' && c <= 'z'; }
int isalpha(int c) { return isupper(c) || islower(c); }
int isalnum(int c) { return isalpha(c) || isdigit(c); }
int isxdigit(int c) { return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
int isspace(int c) { return c == ' ' || (c >= '\t' && c <= '\r'); }
int isprint(int c) { return c >= 0x20 && c < 0x7f; }
int isgraph(int c) { return c > 0x20 && c < 0x7f; }
int iscntrl(int c) { return (c >= 0 && c < 0x20) || c == 0x7f; }
int ispunct(int c) { return isgraph(c) && !isalnum(c); }
int toupper(int c) { return islower(c) ? c - 32 : c; }
int tolower(int c) { return isupper(c) ? c + 32 : c; }

/* ---- numbers --------------------------------------------------------------------- */

unsigned long long strtoull(const char *s, char **end, int base)
{
    const char *p = s;
    while (isspace((unsigned char)*p)) p++;
    if (*p == '+') p++;
    if ((base == 0 || base == 16) && p[0] == '0' && (p[1] == 'x' || p[1] == 'X')) { p += 2; base = 16; }
    else if (base == 0 && p[0] == '0') base = 8;
    else if (base == 0) base = 10;
    unsigned long long v = 0;
    const char *start = p;
    for (;; p++) {
        int d;
        if (isdigit((unsigned char)*p)) d = *p - '0';
        else if (isalpha((unsigned char)*p)) d = tolower((unsigned char)*p) - 'a' + 10;
        else break;
        if (d >= base) break;
        v = v * base + d;
    }
    if (end) *end = (char *)(p == start ? s : p);
    return v;
}
long long strtoll(const char *s, char **end, int base)
{
    const char *p = s;
    while (isspace((unsigned char)*p)) p++;
    int neg = 0;
    if (*p == '-') { neg = 1; p++; }
    char *e;
    unsigned long long v = strtoull(p, &e, base);
    if (end) *end = e == p ? (char *)s : e;
    return neg ? -(long long)v : (long long)v;
}
long strtol(const char *s, char **end, int base) { return (long)strtoll(s, end, base); }
unsigned long strtoul(const char *s, char **end, int base) { return (unsigned long)strtoull(s, end, base); }
int atoi(const char *s) { return (int)strtol(s, NULL, 10); }
long atol(const char *s) { return strtol(s, NULL, 10); }
double strtod(const char *s, char **end)
{
    const char *p = s;
    while (isspace((unsigned char)*p)) p++;
    int neg = 0;
    if (*p == '-' || *p == '+') neg = *p++ == '-';
    double v = 0;
    int any = 0;
    while (isdigit((unsigned char)*p)) { v = v * 10 + (*p++ - '0'); any = 1; }
    if (*p == '.') {
        double f = 0.1;
        p++;
        while (isdigit((unsigned char)*p)) { v += (*p++ - '0') * f; f *= 0.1; any = 1; }
    }
    if (any && (*p == 'e' || *p == 'E')) {
        const char *q = p + 1;
        int en = 0, ex = 0;
        if (*q == '-' || *q == '+') en = *q++ == '-';
        if (isdigit((unsigned char)*q)) {
            while (isdigit((unsigned char)*q)) ex = ex * 10 + (*q++ - '0');
            while (ex--) v = en ? v / 10 : v * 10;
            p = q;
        }
    }
    if (end) *end = (char *)(any ? p : s);
    return neg ? -v : v;
}
float strtof(const char *s, char **end) { return (float)strtod(s, end); }
double atof(const char *s) { return strtod(s, NULL); }
int abs(int x) { return x < 0 ? -x : x; }
long labs(long x) { return x < 0 ? -x : x; }
long long llabs(long long x) { return x < 0 ? -x : x; }

static unsigned long rand_state = 1;
int rand(void) { rand_state = rand_state * 1103515245 + 12345; return (int)((rand_state >> 16) & RAND_MAX); }
void srand(unsigned s) { rand_state = s; }

void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *))
{
    /* insertion sort for small arrays, else a simple quicksort */
    char *a = base;
    char tmp[256];
    if (size > sizeof tmp) return;
    if (n < 2) return;
    if (n < 12) {
        for (size_t i = 1; i < n; i++)
            for (size_t j = i; j > 0 && cmp(a + (j - 1) * size, a + j * size) > 0; j--) {
                memcpy(tmp, a + j * size, size);
                memcpy(a + j * size, a + (j - 1) * size, size);
                memcpy(a + (j - 1) * size, tmp, size);
            }
        return;
    }
    char *pivot = a + (n / 2) * size;
    memcpy(tmp, pivot, size);
    memcpy(pivot, a + (n - 1) * size, size);
    memcpy(a + (n - 1) * size, tmp, size);
    size_t store = 0;
    for (size_t i = 0; i + 1 < n; i++) {
        if (cmp(a + i * size, a + (n - 1) * size) < 0) {
            memcpy(tmp, a + i * size, size);
            memcpy(a + i * size, a + store * size, size);
            memcpy(a + store * size, tmp, size);
            store++;
        }
    }
    memcpy(tmp, a + store * size, size);
    memcpy(a + store * size, a + (n - 1) * size, size);
    memcpy(a + (n - 1) * size, tmp, size);
    qsort(a, store, size, cmp);
    qsort(a + (store + 1) * size, n - store - 1, size, cmp);
}

void *bsearch(const void *key, const void *base, size_t n, size_t size, int (*cmp)(const void *, const void *))
{
    const char *a = base;
    size_t lo = 0, hi = n;
    while (lo < hi) {
        size_t mid = (lo + hi) / 2;
        int c = cmp(key, a + mid * size);
        if (c == 0) return (void *)(a + mid * size);
        if (c < 0) hi = mid; else lo = mid + 1;
    }
    return NULL;
}

char *getenv(const char *name) { (void)name; return NULL; }
int system(const char *cmd) { (void)cmd; return -1; }
char *setlocale(int c, const char *l) { (void)c; (void)l; return "C"; }
sighandler_t signal(int sig, sighandler_t h) { (void)sig; (void)h; return SIG_DFL; }

/* ---- exit ------------------------------------------------------------------------- */

static void (*at_exit[16])(void);
static int at_exit_n;
int atexit(void (*f)(void)) { if (at_exit_n < 16) { at_exit[at_exit_n++] = f; return 0; } return -1; }
void bz_flush_all(void);
__attribute__((noreturn)) void exit(int code)
{
    while (at_exit_n > 0) at_exit[--at_exit_n]();
    bz_flush_all();
    bz_exit(code);
}
__attribute__((noreturn)) void abort(void) { bz_write("abort\n", 6); bz_exit(134); }
__attribute__((noreturn)) void bz_exit(int code)
{
    for (;;) bz_syscall(BZ_EXIT, (uint64_t)code, 0, 0);
}
void bz_assert_fail(const char *expr, const char *file, int line)
{
    fprintf(stderr, "assertion failed: %s (%s:%d)\n", expr, file, line);
    abort();
}

/* ---- time ------------------------------------------------------------------------- */

struct bz_rtc { uint64_t year, month, day, hour, minute, second; }; /* RtcTime */

time_t time(time_t *t)
{
    struct bz_rtc r = { 0 };
    bz_syscall(BZ_CLOCK_RTC, (uint64_t)&r, 0, 0);
    /* days since 1970 (civil from date, Hinnant) */
    int y = (int)r.year, m = (int)r.month, d = (int)r.day;
    y -= m <= 2;
    int era = (y >= 0 ? y : y - 399) / 400;
    unsigned yoe = (unsigned)(y - era * 400);
    unsigned doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    unsigned doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    long days = era * 146097L + (long)doe - 719468;
    time_t v = days * 86400 + (long)(r.hour * 3600 + r.minute * 60 + r.second);
    if (t) *t = v;
    return v;
}
clock_t clock(void) { return (clock_t)(bz_nanos() / 1000); }
static struct tm tm_static;
struct tm *localtime(const time_t *t)
{
    long s = (long)*t;
    long days = s / 86400, rem = s % 86400;
    tm_static.tm_hour = rem / 3600; tm_static.tm_min = rem % 3600 / 60; tm_static.tm_sec = rem % 60;
    tm_static.tm_wday = (int)((days + 4) % 7);
    long z = days + 719468, era = z / 146097, doe = z - era * 146097;
    long yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long doy = doe - (365 * yoe + yoe / 4 - yoe / 100), mp = (5 * doy + 2) / 153;
    tm_static.tm_mday = (int)(doy - (153 * mp + 2) / 5 + 1);
    tm_static.tm_mon = (int)(mp < 10 ? mp + 2 : mp - 10);
    tm_static.tm_year = (int)(yoe + era * 400 + (tm_static.tm_mon < 2)) - 1900;
    return &tm_static;
}
size_t strftime(char *s, size_t n, const char *fmt, const struct tm *tm)
{
    return (size_t)snprintf(s, n, "%04d-%02d-%02d %02d:%02d", tm->tm_year + 1900, tm->tm_mon + 1, tm->tm_mday, tm->tm_hour, tm->tm_min) + 0 * (fmt != 0);
}
int gettimeofday(struct timeval *tv, void *tz)
{
    (void)tz;
    uint64_t ns = bz_nanos();
    tv->tv_sec = (time_t)(ns / 1000000000ull);
    tv->tv_usec = (suseconds_t)(ns / 1000 % 1000000);
    return 0;
}
int usleep(unsigned us)
{
    uint64_t until = bz_nanos() + (uint64_t)us * 1000;
    bz_syscall(BZ_SLEEP_UNTIL, until, 0, 0);
    return 0;
}
unsigned sleep(unsigned s) { usleep(s * 1000000u); return 0; }

/* ---- math ---------------------------------------------------------------------- */

double fabs(double x) { return x < 0 ? -x : x; }
float fabsf(float x) { return x < 0 ? -x : x; }
double floor(double x) { double t = (double)(long long)x; return t > x ? t - 1 : t; }
double ceil(double x) { double t = (double)(long long)x; return t < x ? t + 1 : t; }
double trunc(double x) { return (double)(long long)x; }
double round(double x) { return x < 0 ? -floor(-x + 0.5) : floor(x + 0.5); }
double fmod(double x, double y) { return y == 0 ? 0 : x - trunc(x / y) * y; }
double sqrt(double x)
{
    if (x <= 0) return 0;
    double r = x > 1 ? x : 1;
    for (int i = 0; i < 60; i++) r = 0.5 * (r + x / r);
    return r;
}
double exp(double x)
{
    /* e^x = 2^k * e^r with |r| <= ln2/2 */
    int k = (int)(x / 0.6931471805599453 + (x < 0 ? -0.5 : 0.5));
    double r = x - k * 0.6931471805599453, t = 1, s = 1;
    for (int i = 1; i < 20; i++) { t *= r / i; s += t; }
    while (k > 0) { s *= 2; k--; }
    while (k < 0) { s /= 2; k++; }
    return s;
}
double log(double x)
{
    if (x <= 0) return -1e308;
    int k = 0;
    while (x > 2) { x /= 2; k++; }
    while (x < 1) { x *= 2; k--; }
    double y = (x - 1) / (x + 1), y2 = y * y, s = 0, t = y;
    for (int i = 1; i < 40; i += 2) { s += t / i; t *= y2; }
    return 2 * s + k * 0.6931471805599453;
}
double log10(double x) { return log(x) / 2.302585092994046; }
double pow(double x, double y)
{
    if (y == (long long)y) {
        long long n = (long long)y;
        double r = 1, b = x;
        int neg = n < 0;
        if (neg) n = -n;
        while (n) { if (n & 1) r *= b; b *= b; n >>= 1; }
        return neg ? 1 / r : r;
    }
    return x <= 0 ? 0 : exp(y * log(x));
}
double sin(double x)
{
    x = fmod(x, 2 * M_PI);
    double t = x, s = x;
    for (int i = 1; i < 12; i++) { t *= -x * x / ((2 * i) * (2 * i + 1)); s += t; }
    return s;
}
double cos(double x) { return sin(x + M_PI / 2); }
double tan(double x) { double c = cos(x); return c == 0 ? 1e308 : sin(x) / c; }
double atan(double x)
{
    if (x > 1) return M_PI / 2 - atan(1 / x);
    if (x < -1) return -M_PI / 2 - atan(1 / x);
    /* atan(x) = 2 atan(x / (1 + sqrt(1 + x^2))) brings |x| under 0.42 */
    double y = x / (1 + sqrt(1 + x * x)), t = y, s = y;
    for (int i = 1; i < 30; i++) { t *= -y * y; s += t / (2 * i + 1); }
    return 2 * s;
}
double atan2(double y, double x)
{
    if (x > 0) return atan(y / x);
    if (x < 0) return y >= 0 ? atan(y / x) + M_PI : atan(y / x) - M_PI;
    return y > 0 ? M_PI / 2 : y < 0 ? -M_PI / 2 : 0;
}

/* ---- files ------------------------------------------------------------------------ */

struct bz_file {
    char path[160];
    unsigned char *buf;
    size_t len, cap, pos;
    int writing, dirty, eof, err, console, unget;
};

static struct bz_file f_stdin = { .console = 0, .eof = 1, .unget = -1 };
static struct bz_file f_stdout = { .console = 1, .writing = 1, .unget = -1 };
static struct bz_file f_stderr = { .console = 1, .writing = 1, .unget = -1 };
FILE *stdin = &f_stdin, *stdout = &f_stdout, *stderr = &f_stderr;

#define MAX_OPEN 32
static FILE *open_files[MAX_OPEN];

static void full_path(char *out, size_t n, const char *path)
{
    if (path[0] == '/') snprintf(out, n, "%s", path);
    else if (path[0] == '.' && path[1] == '/') snprintf(out, n, "/disk/%s", path + 2);
    else snprintf(out, n, "/disk/%s", path);
    /* FAT has 8.3 names: a "hidden" Unix name (.default.cfg) loses its dots */
    char *name = strrchr(out, '/');
    if (name && name[1] == '.' && name[2] && name[2] != '.') {
        char *q = name + 1;
        while (*q == '.') q++;
        memmove(name + 1, q, strlen(q) + 1);
    }
}

static long file_size(const char *path)
{
    uint64_t out[4] = { 0 };
    uint64_t r = bz_syscall(BZ_FS_OP, BZ_FS_STAT, (uint64_t)path, (uint64_t)out);
    if (BZ_IS_ERROR(r)) return -1;
    if (out[1]) return -2; /* a folder */
    return (long)out[0];
}

static int reserve(FILE *f, size_t n)
{
    if (n <= f->cap) return 0;
    size_t c = f->cap ? f->cap : 4096;
    while (c < n) c *= 2;
    unsigned char *b = realloc(f->buf, c);
    if (!b) return -1;
    f->buf = b;
    f->cap = c;
    return 0;
}

FILE *fopen(const char *path, const char *mode)
{
    FILE *f = calloc(1, sizeof *f);
    if (!f) return NULL;
    full_path(f->path, sizeof f->path, path);
    f->unget = -1;
    int write = strchr(mode, 'w') || strchr(mode, 'a');
    int plus = strchr(mode, '+') != NULL;
    long size = file_size(f->path);
    if (size == -2 || (!write && size < 0)) { free(f); errno = ENOENT; return NULL; }
    if (!strchr(mode, 'w') && size > 0) {
        if (reserve(f, (size_t)size + 1)) { free(f); errno = ENOMEM; return NULL; }
        uint64_t got = 0;
        if (size <= (1 << 20)) {
            got = bz_syscall(BZ_FS_READ, (uint64_t)f->path, (uint64_t)f->buf, (uint64_t)size);
            if (BZ_IS_ERROR(got)) got = 0;
        } else {
            /* large files (game data) in 1 MiB pieces: the kernel holds one piece at a time */
            while (got < (uint64_t)size) {
                uint64_t want = (uint64_t)size - got;
                if (want > (1u << 20)) want = 1u << 20;
                uint64_t r = bz_syscall(BZ_FS_READ_AT, (uint64_t)f->path, (uint64_t)(f->buf + got), got << 24 | want);
                if (BZ_IS_ERROR(r) || r == 0) {
                    fprintf(stderr, "[libc] reading %s stopped at %llu of %ld bytes (%llx)\n", f->path, (unsigned long long)got, size, (unsigned long long)r);
                    break;
                }
                got += r;
            }
        }
        f->len = (size_t)got;
    }
    f->writing = write || plus;
    if (strchr(mode, 'a')) f->pos = f->len;
    if (strchr(mode, 'w')) f->dirty = 1; /* creates / truncates on close */
    for (int i = 0; i < MAX_OPEN; i++) if (!open_files[i]) { open_files[i] = f; break; }
    return f;
}

static void console_out(FILE *f, const void *p, size_t n)
{
    (void)f;
    bz_write(p, n);
}

int fflush(FILE *f)
{
    if (!f) { bz_flush_all(); return 0; }
    if (f->console || !f->writing || !f->dirty) return 0;
    uint64_t r = bz_syscall(BZ_FS_WRITE, (uint64_t)f->path, (uint64_t)(f->buf ? f->buf : (unsigned char *)""), f->len);
    f->dirty = 0;
    if (BZ_IS_ERROR(r) || (f->len && r == 0)) { f->err = 1; return EOF; }
    return 0;
}

void bz_flush_all(void)
{
    for (int i = 0; i < MAX_OPEN; i++) if (open_files[i]) fflush(open_files[i]);
}

int fclose(FILE *f)
{
    if (!f || f->console) return 0;
    int r = fflush(f);
    for (int i = 0; i < MAX_OPEN; i++) if (open_files[i] == f) open_files[i] = NULL;
    free(f->buf);
    free(f);
    return r;
}

size_t fread(void *p, size_t size, size_t n, FILE *f)
{
    if (!f || f->console || size == 0) return 0;
    size_t want = size * n, have = f->pos < f->len ? f->len - f->pos : 0;
    if (want > have) { want = have - have % size; f->eof = 1; }
    memcpy(p, f->buf + f->pos, want);
    f->pos += want;
    return want / size;
}

size_t fwrite(const void *p, size_t size, size_t n, FILE *f)
{
    size_t t = size * n;
    if (!f || size == 0) return 0;
    if (f->console) { console_out(f, p, t); return n; }
    if (!f->writing) { f->err = 1; return 0; }
    if (reserve(f, f->pos + t)) { f->err = 1; return 0; }
    memcpy(f->buf + f->pos, p, t);
    f->pos += t;
    if (f->pos > f->len) f->len = f->pos;
    f->dirty = 1;
    return n;
}

int fseek(FILE *f, long off, int whence)
{
    if (!f || f->console) return -1;
    long base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? (long)f->pos : (long)f->len;
    long p = base + off;
    if (p < 0) return -1;
    f->pos = (size_t)p;
    f->eof = 0;
    f->unget = -1;
    return 0;
}
long ftell(FILE *f) { return f && !f->console ? (long)f->pos : -1; }
void rewind(FILE *f) { fseek(f, 0, SEEK_SET); }
int feof(FILE *f) { return f ? f->eof : 1; }
int ferror(FILE *f) { return f ? f->err : 1; }
void clearerr(FILE *f) { if (f) { f->eof = 0; f->err = 0; } }
int fgetc(FILE *f)
{
    if (f->unget >= 0) { int c = f->unget; f->unget = -1; return c; }
    if (f->console || f->pos >= f->len) { f->eof = 1; return EOF; }
    return f->buf[f->pos++];
}
int getc(FILE *f) { return fgetc(f); }
int getchar(void) { return EOF; }
int ungetc(int c, FILE *f) { if (c == EOF) return EOF; f->unget = c; f->eof = 0; return c; }
char *fgets(char *s, int n, FILE *f)
{
    int i = 0;
    while (i < n - 1) {
        int c = fgetc(f);
        if (c == EOF) break;
        s[i++] = (char)c;
        if (c == '\n') break;
    }
    if (i == 0) return NULL;
    s[i] = 0;
    return s;
}
int fputc(int c, FILE *f) { unsigned char b = (unsigned char)c; return fwrite(&b, 1, 1, f) == 1 ? c : EOF; }
int putc(int c, FILE *f) { return fputc(c, f); }
int fputs(const char *s, FILE *f) { return fwrite(s, 1, strlen(s), f) ? 0 : EOF; }
int puts(const char *s) { fputs(s, stdout); fputc('\n', stdout); return 0; }
int putchar(int c) { return fputc(c, stdout); }
int setvbuf(FILE *f, char *b, int m, size_t s) { (void)f; (void)b; (void)m; (void)s; return 0; }
void setbuf(FILE *f, char *b) { (void)f; (void)b; }
int fileno(FILE *f) { return f == stdin ? 0 : f == stdout ? 1 : f == stderr ? 2 : 3; }
FILE *tmpfile(void) { return NULL; }

int remove(const char *path)
{
    char p[160];
    full_path(p, sizeof p, path);
    return BZ_IS_ERROR(bz_syscall(BZ_FS_OP, BZ_FS_REMOVE, (uint64_t)p, 0)) ? -1 : 0;
}
int rename(const char *from, const char *to)
{
    char a[160], b[160];
    full_path(a, sizeof a, from);
    full_path(b, sizeof b, to);
    return BZ_IS_ERROR(bz_syscall(BZ_FS_OP, BZ_FS_RENAME, (uint64_t)a, (uint64_t)b)) ? -1 : 0;
}
int mkdir(const char *path, mode_t mode)
{
    (void)mode;
    char p[160];
    full_path(p, sizeof p, path);
    if (file_size(p) == -2) { errno = EEXIST; return -1; }
    return BZ_IS_ERROR(bz_syscall(BZ_FS_OP, BZ_FS_MKDIR, (uint64_t)p, 0)) ? -1 : 0;
}
int stat(const char *path, struct stat *st)
{
    char p[160];
    full_path(p, sizeof p, path);
    uint64_t out[4] = { 0 };
    if (BZ_IS_ERROR(bz_syscall(BZ_FS_OP, BZ_FS_STAT, (uint64_t)p, (uint64_t)out))) { errno = ENOENT; return -1; }
    st->st_size = (off_t)out[0];
    st->st_mode = out[1] ? S_IFDIR : S_IFREG;
    st->st_mtime = 0;
    return 0;
}
int access(const char *path, int mode) { struct stat st; (void)mode; return stat(path, &st); }
int isatty(int fd) { (void)fd; return 0; }
/* descriptors >= 64 are sockets (socket.c) */
int bz_sock_close(int fd);
ssize_t bz_sock_read(int fd, void *p, size_t n);
ssize_t bz_sock_write(int fd, const void *p, size_t n);
int close(int fd) { return fd >= 64 ? bz_sock_close(fd) : 0; }
ssize_t read(int fd, void *p, size_t n) { if (fd >= 64) return bz_sock_read(fd, p, n); return -1; }
ssize_t write(int fd, const void *p, size_t n)
{
    if (fd >= 64) return bz_sock_write(fd, p, n);
    if (fd == 1 || fd == 2) { bz_write(p, n); return (ssize_t)n; }
    return -1;
}
char *getcwd(char *buf, size_t n) { snprintf(buf, n, "/disk"); return buf; }
int chdir(const char *path) { (void)path; return 0; }
void perror(const char *s) { fprintf(stderr, "%s: %s\n", s ? s : "", strerror(errno)); }
DIR *opendir(const char *path) { (void)path; return NULL; }
struct dirent *readdir(DIR *d) { (void)d; return NULL; }
int closedir(DIR *d) { (void)d; return 0; }

/* ---- printf ------------------------------------------------------------------------ */

struct out { char *buf; size_t cap, len; FILE *file; char line[256]; size_t ll; };

static void out_char(struct out *o, char c)
{
    if (o->file) {
        o->line[o->ll++] = c;
        if (o->ll == sizeof o->line || c == '\n') { fwrite(o->line, 1, o->ll, o->file); o->ll = 0; }
    } else if (o->len + 1 < o->cap) {
        o->buf[o->len] = c;
    }
    o->len++;
}

static void out_str(struct out *o, const char *s, size_t n, int width, int left, char pad)
{
    int fill = width > (int)n ? width - (int)n : 0;
    if (!left) while (fill-- > 0) out_char(o, pad);
    for (size_t i = 0; i < n; i++) out_char(o, s[i]);
    if (left) while (fill-- > 0) out_char(o, ' ');
}

static int fmt_float(char *b, double v, int prec, char kind)
{
    int n = 0;
    if (v != v) { memcpy(b, "nan", 3); return 3; }
    if (v < 0) { b[n++] = '-'; v = -v; }
    if (prec < 0) prec = 6;
    int ex = 0;
    if (kind == 'e' || (kind == 'g' && v != 0 && (v < 1e-4 || v >= 1e15))) {
        while (v >= 10) { v /= 10; ex++; }
        while (v != 0 && v < 1) { v *= 10; ex--; }
        kind = 'e';
    }
    double r = 0.5;
    for (int i = 0; i < prec; i++) r /= 10;
    v += r;
    if (v >= 1e18) { memcpy(b + n, "inf", 3); return n + 3; }
    unsigned long long ip = (unsigned long long)v;
    double fp = v - (double)ip;
    char tmp[24];
    int t = 0;
    do { tmp[t++] = (char)('0' + ip % 10); ip /= 10; } while (ip);
    while (t) b[n++] = tmp[--t];
    if (prec > 0) {
        b[n++] = '.';
        for (int i = 0; i < prec && n < 60; i++) { fp *= 10; int d = (int)fp; b[n++] = (char)('0' + d); fp -= d; }
    }
    if (kind == 'e') n += snprintf(b + n, 8, "e%+03d", ex);
    return n;
}

static int format(struct out *o, const char *fmt, va_list ap)
{
    for (const char *p = fmt; *p; p++) {
        if (*p != '%') { out_char(o, *p); continue; }
        p++;
        int left = 0, plus = 0, space = 0, alt = 0;
        char pad = ' ';
        for (;; p++) {
            if (*p == '-') left = 1;
            else if (*p == '0') pad = '0';
            else if (*p == '+') plus = 1;
            else if (*p == ' ') space = 1;
            else if (*p == '#') alt = 1;
            else break;
        }
        int width = 0, prec = -1;
        if (*p == '*') { width = va_arg(ap, int); if (width < 0) { left = 1; width = -width; } p++; }
        else while (isdigit((unsigned char)*p)) width = width * 10 + (*p++ - '0');
        if (*p == '.') {
            p++;
            prec = 0;
            if (*p == '*') { prec = va_arg(ap, int); p++; }
            else while (isdigit((unsigned char)*p)) prec = prec * 10 + (*p++ - '0');
        }
        int lng = 0;
        for (;; p++) {
            if (*p == 'l') lng++;
            else if (*p == 'h') lng = lng > 0 ? lng : -1;
            else if (*p == 'z' || *p == 'j' || *p == 't') lng = 2;
            else break;
        }
        char b[72];
        int n = 0;
        switch (*p) {
        case 'd': case 'i': {
            long long v = lng >= 2 ? va_arg(ap, long long) : lng == 1 ? va_arg(ap, long) : va_arg(ap, int);
            unsigned long long u = v < 0 ? -(unsigned long long)v : (unsigned long long)v;
            char t[24];
            int k = 0;
            do { t[k++] = (char)('0' + u % 10); u /= 10; } while (u);
            while (k < prec) t[k++] = '0';
            if (v < 0) b[n++] = '-'; else if (plus) b[n++] = '+'; else if (space) b[n++] = ' ';
            while (k) b[n++] = t[--k];
            if (pad == '0' && !left && (b[0] == '-' || b[0] == '+') && width > n) {
                out_char(o, b[0]);
                out_str(o, b + 1, (size_t)n - 1, width - 1, 0, '0');
            } else out_str(o, b, (size_t)n, width, left, prec >= 0 ? ' ' : pad);
            break;
        }
        case 'u': case 'x': case 'X': case 'o': case 'p': {
            unsigned long long u;
            if (*p == 'p') { u = (unsigned long long)(uintptr_t)va_arg(ap, void *); alt = 1; }
            else u = lng >= 2 ? va_arg(ap, unsigned long long) : lng == 1 ? va_arg(ap, unsigned long) : va_arg(ap, unsigned int);
            if (lng == -1 && *p != 'p') u &= 0xFFFF;
            int base = *p == 'u' ? 10 : *p == 'o' ? 8 : 16;
            const char *dig = *p == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
            char t[24];
            int k = 0;
            do { t[k++] = dig[u % base]; u /= base; } while (u);
            while (k < prec) t[k++] = '0';
            if (alt && base == 16) { b[n++] = '0'; b[n++] = *p == 'X' ? 'X' : 'x'; }
            while (k) b[n++] = t[--k];
            out_str(o, b, (size_t)n, width, left, pad);
            break;
        }
        case 'c': b[0] = (char)va_arg(ap, int); out_str(o, b, 1, width, left, ' '); break;
        case 's': {
            const char *s = va_arg(ap, const char *);
            if (!s) s = "(null)";
            size_t l = prec >= 0 ? strnlen(s, (size_t)prec) : strlen(s);
            out_str(o, s, l, width, left, ' ');
            break;
        }
        case 'f': case 'F': case 'e': case 'g': case 'G': {
            double v = va_arg(ap, double);
            n = fmt_float(b, v, prec, (char)tolower((unsigned char)*p));
            out_str(o, b, (size_t)n, width, left, pad);
            break;
        }
        case '%': out_char(o, '%'); break;
        case 0: p--; break;
        default: out_char(o, '%'); out_char(o, *p); break;
        }
    }
    if (o->file && o->ll) { fwrite(o->line, 1, o->ll, o->file); o->ll = 0; }
    if (!o->file && o->cap) o->buf[o->len < o->cap ? o->len : o->cap - 1] = 0;
    return (int)o->len;
}

int vsnprintf(char *s, size_t n, const char *fmt, va_list ap) { struct out o = { .buf = s, .cap = n }; return format(&o, fmt, ap); }
int vsprintf(char *s, const char *fmt, va_list ap) { return vsnprintf(s, (size_t)-1 / 2, fmt, ap); }
int vfprintf(FILE *f, const char *fmt, va_list ap) { struct out o = { .file = f }; return format(&o, fmt, ap); }
int vprintf(const char *fmt, va_list ap) { return vfprintf(stdout, fmt, ap); }
int snprintf(char *s, size_t n, const char *fmt, ...) { va_list ap; va_start(ap, fmt); int r = vsnprintf(s, n, fmt, ap); va_end(ap); return r; }
int sprintf(char *s, const char *fmt, ...) { va_list ap; va_start(ap, fmt); int r = vsprintf(s, fmt, ap); va_end(ap); return r; }
int fprintf(FILE *f, const char *fmt, ...) { va_list ap; va_start(ap, fmt); int r = vfprintf(f, fmt, ap); va_end(ap); return r; }
int printf(const char *fmt, ...) { va_list ap; va_start(ap, fmt); int r = vfprintf(stdout, fmt, ap); va_end(ap); return r; }

/* ---- scanf ----------------------------------------------------------------------- */

static int scan(const char *s, const char *fmt, va_list ap)
{
    int count = 0;
    const char *p = s;
    for (const char *f = fmt; *f; f++) {
        if (isspace((unsigned char)*f)) { while (isspace((unsigned char)*p)) p++; continue; }
        if (*f != '%') { if (*p != *f) break; p++; continue; }
        f++;
        int skip = 0, width = 0, lng = 0;
        if (*f == '*') { skip = 1; f++; }
        while (isdigit((unsigned char)*f)) width = width * 10 + (*f++ - '0');
        while (*f == 'l' || *f == 'h') { if (*f == 'l') lng++; f++; }
        if (*f != 'c') while (isspace((unsigned char)*p)) p++;
        if (!*p) break;
        char *e;
        switch (*f) {
        case 'd': case 'i': case 'u': case 'x': case 'X': {
            long long v = *f == 'x' || *f == 'X' ? (long long)strtoull(p, &e, 16) : strtoll(p, &e, *f == 'i' ? 0 : 10);
            if (e == p) return count;
            p = e;
            if (!skip) {
                if (lng >= 2) *va_arg(ap, long long *) = v;
                else if (lng == 1) *va_arg(ap, long *) = (long)v;
                else *va_arg(ap, int *) = (int)v;
                count++;
            }
            break;
        }
        case 'f': case 'g': case 'e': {
            double v = strtod(p, &e);
            if (e == p) return count;
            p = e;
            if (!skip) { if (lng) *va_arg(ap, double *) = v; else *va_arg(ap, float *) = (float)v; count++; }
            break;
        }
        case 's': {
            char *d = skip ? NULL : va_arg(ap, char *);
            int k = 0;
            while (*p && !isspace((unsigned char)*p) && (!width || k < width)) { if (d) d[k] = *p; k++; p++; }
            if (d) { d[k] = 0; count++; }
            break;
        }
        case 'c': {
            char *d = skip ? NULL : va_arg(ap, char *);
            int k = width ? width : 1;
            for (int i = 0; i < k && *p; i++) { if (d) d[i] = *p; p++; }
            if (d) count++;
            break;
        }
        default: return count;
        }
    }
    return count;
}
int sscanf(const char *s, const char *fmt, ...) { va_list ap; va_start(ap, fmt); int r = scan(s, fmt, ap); va_end(ap); return r; }
int fscanf(FILE *f, const char *fmt, ...)
{
    char line[512];
    if (!fgets(line, sizeof line, f)) return EOF;
    va_list ap;
    va_start(ap, fmt);
    int r = scan(line, fmt, ap);
    va_end(ap);
    return r;
}
