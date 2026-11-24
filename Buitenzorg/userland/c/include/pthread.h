/* Buitenzorg C runtime (userland/c): pthread.h -- POSIX threads over the
 * kernel's THREAD_CREATE / THREAD_JOIN / FUTEX_* syscalls. Threads of one
 * program run in parallel on several CPUs. Mutexes and condition variables
 * are futex words (they block in the scheduler, no spinning). Not here:
 * cancellation, signals, process-shared objects, priorities. errno is one
 * global (not per thread). */
#ifndef BZ_PTHREAD_H
#define BZ_PTHREAD_H
#include <stddef.h>
#include <stdint.h>
#include <time.h>

typedef uint64_t pthread_t;
typedef struct { size_t stacksize; int detached; } pthread_attr_t;
typedef struct { volatile uint32_t state; int type; uint64_t owner; int count; } pthread_mutex_t;
typedef struct { int type; } pthread_mutexattr_t;
typedef struct { volatile uint32_t seq; } pthread_cond_t;
typedef struct { int unused; } pthread_condattr_t;
typedef struct { volatile uint32_t state; } pthread_once_t;
typedef unsigned pthread_key_t;
typedef struct { pthread_mutex_t lock; pthread_cond_t cond; int readers; int writer; } pthread_rwlock_t;
typedef struct { int unused; } pthread_rwlockattr_t;
typedef struct { pthread_mutex_t lock; pthread_cond_t cond; unsigned count, waiting, cycle; } pthread_barrier_t;
typedef struct { int unused; } pthread_barrierattr_t;
typedef volatile uint32_t pthread_spinlock_t;

#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_RECURSIVE 1
#define PTHREAD_MUTEX_ERRORCHECK 2
#define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL
#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1
#define PTHREAD_MUTEX_INITIALIZER { 0, 0, 0, 0 }
#define PTHREAD_RECURSIVE_MUTEX_INITIALIZER { 0, PTHREAD_MUTEX_RECURSIVE, 0, 0 }
#define PTHREAD_COND_INITIALIZER { 0 }
#define PTHREAD_ONCE_INIT { 0 }
#define PTHREAD_RWLOCK_INITIALIZER { PTHREAD_MUTEX_INITIALIZER, PTHREAD_COND_INITIALIZER, 0, 0 }
#define PTHREAD_BARRIER_SERIAL_THREAD (-1)
#define PTHREAD_KEYS_MAX 32
#define PTHREAD_STACK_MIN 16384

int pthread_create(pthread_t *t, const pthread_attr_t *attr, void *(*fn)(void *), void *arg);
int pthread_join(pthread_t t, void **ret);
int pthread_detach(pthread_t t);
__attribute__((noreturn)) void pthread_exit(void *ret);
pthread_t pthread_self(void);
int pthread_equal(pthread_t a, pthread_t b);
int pthread_attr_init(pthread_attr_t *a);
int pthread_attr_destroy(pthread_attr_t *a);
int pthread_attr_setstacksize(pthread_attr_t *a, size_t n);
int pthread_attr_getstacksize(const pthread_attr_t *a, size_t *n);
int pthread_attr_setdetachstate(pthread_attr_t *a, int s);

int pthread_mutexattr_init(pthread_mutexattr_t *a);
int pthread_mutexattr_settype(pthread_mutexattr_t *a, int type);
int pthread_mutexattr_destroy(pthread_mutexattr_t *a);
int pthread_mutex_init(pthread_mutex_t *m, const pthread_mutexattr_t *a);
int pthread_mutex_destroy(pthread_mutex_t *m);
int pthread_mutex_lock(pthread_mutex_t *m);
int pthread_mutex_trylock(pthread_mutex_t *m);
int pthread_mutex_unlock(pthread_mutex_t *m);

int pthread_cond_init(pthread_cond_t *c, const pthread_condattr_t *a);
int pthread_cond_destroy(pthread_cond_t *c);
int pthread_cond_wait(pthread_cond_t *c, pthread_mutex_t *m);
int pthread_cond_timedwait(pthread_cond_t *c, pthread_mutex_t *m, const struct timespec *abstime);
int pthread_cond_signal(pthread_cond_t *c);
int pthread_cond_broadcast(pthread_cond_t *c);

int pthread_once(pthread_once_t *o, void (*fn)(void));
int pthread_key_create(pthread_key_t *k, void (*destructor)(void *));
int pthread_key_delete(pthread_key_t k);
void *pthread_getspecific(pthread_key_t k);
int pthread_setspecific(pthread_key_t k, const void *v);

int pthread_rwlock_init(pthread_rwlock_t *l, const pthread_rwlockattr_t *a);
int pthread_rwlock_destroy(pthread_rwlock_t *l);
int pthread_rwlock_rdlock(pthread_rwlock_t *l);
int pthread_rwlock_wrlock(pthread_rwlock_t *l);
int pthread_rwlock_unlock(pthread_rwlock_t *l);

int pthread_barrier_init(pthread_barrier_t *b, const pthread_barrierattr_t *a, unsigned count);
int pthread_barrier_destroy(pthread_barrier_t *b);
int pthread_barrier_wait(pthread_barrier_t *b);

int pthread_spin_init(pthread_spinlock_t *s, int shared);
int pthread_spin_destroy(pthread_spinlock_t *s);
int pthread_spin_lock(pthread_spinlock_t *s);
int pthread_spin_trylock(pthread_spinlock_t *s);
int pthread_spin_unlock(pthread_spinlock_t *s);
#endif
