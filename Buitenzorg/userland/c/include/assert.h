/* Buitenzorg C runtime (userland/c): assert.h -- the subset ported programs need. */
#ifndef BZ_ASSERT_H
#define BZ_ASSERT_H
void bz_assert_fail(const char *expr, const char *file, int line);
#ifdef NDEBUG
#define assert(x) ((void)0)
#else
#define assert(x) ((x) ? (void)0 : bz_assert_fail(#x, __FILE__, __LINE__))
#endif
#endif
