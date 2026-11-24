/* Buitenzorg C runtime: POSIX threads (see include/pthread.h).
 *
 * A thread = a kernel thread of this process (THREAD_CREATE) on an mmap'd
 * stack; its control block sits at the stack's low end. Blocking objects are
 * futex words: FUTEX_WAIT sleeps in the scheduler while the word holds the
 * expected value, FUTEX_WAKE wakes sleepers of this process only. */
#include <bz.h>
#include <pthread.h>
#include <sched.h>
#include <errno.h>
#include <string.h>
#include <stdlib.h>
#include <time.h>

struct bz_thread {
    void *(*fn)(void *);
    void *arg;
    void *ret;
    uint64_t tid;
    void *base;
    size_t size;
    int detached;
    volatile int done;
    void *specific[PTHREAD_KEYS_MAX];
};

#define MAX_THREADS 128
static struct bz_thread *threads[MAX_THREADS];
static struct bz_thread main_thread;
static volatile uint32_t table_lock;
static void (*key_dtor[PTHREAD_KEYS_MAX])(void *);
static volatile uint32_t key_used[PTHREAD_KEYS_MAX];

static void spin_lock(volatile uint32_t *l)
{
    int n = 0;
    while (__atomic_exchange_n(l, 1, __ATOMIC_ACQUIRE)) {
        if (++n > 64) { bz_yield(); n = 0; }
    }
}
static void spin_unlock(volatile uint32_t *l) { __atomic_store_n(l, 0, __ATOMIC_RELEASE); }

static void futex_wait(volatile uint32_t *w, uint32_t expected) { bz_syscall(BZ_FUTEX_WAIT, (uint64_t)w, expected, 0); }
/* 0 = woken or changed, 1 = timed out */
static int futex_wait_ns(volatile uint32_t *w, uint32_t expected, uint64_t ns)
{
    return (int)bz_syscall(BZ_FUTEX_WAIT_TIMEOUT, (uint64_t)w, expected, ns ? ns : 1);
}
static void futex_wake(volatile uint32_t *w, uint64_t n) { bz_syscall(BZ_FUTEX_WAKE, (uint64_t)w, n, 0); }

static struct bz_thread *self(void)
{
    uint64_t tid = bz_syscall(BZ_THREAD_SELF, 0, 0, 0);
    spin_lock(&table_lock);
    struct bz_thread *t = 0;
    for (int i = 0; i < MAX_THREADS; i++)
        if (threads[i] && threads[i]->tid == tid) { t = threads[i]; break; }
    spin_unlock(&table_lock);
    if (!t) {
        /* the main thread (or one this runtime did not start) */
        if (!main_thread.tid) main_thread.tid = tid;
        t = &main_thread;
    }
    return t;
}

static void run_destructors(struct bz_thread *t)
{
    for (int k = 0; k < PTHREAD_KEYS_MAX; k++) {
        void *v = t->specific[k];
        if (v && key_used[k] && key_dtor[k]) { t->specific[k] = 0; key_dtor[k](v); }
    }
}

static void forget(struct bz_thread *t)
{
    spin_lock(&table_lock);
    for (int i = 0; i < MAX_THREADS; i++)
        if (threads[i] == t) threads[i] = 0;
    spin_unlock(&table_lock);
}

__attribute__((noreturn)) static void finish(struct bz_thread *t, void *ret)
{
    t->ret = ret;
    run_destructors(t);
    __atomic_store_n(&t->done, 1, __ATOMIC_RELEASE);
    for (;;) bz_syscall(BZ_THREAD_EXIT, 0, 0, 0);
}

__attribute__((noreturn, used)) void bz_pthread_body(struct bz_thread *t)
{
    /* the creator stores the id too, after THREAD_CREATE returns: store it
     * first so self() finds this thread from its first instruction */
    __atomic_store_n(&t->tid, bz_syscall(BZ_THREAD_SELF, 0, 0, 0), __ATOMIC_RELEASE);
    finish(t, t->fn(t->arg));
}

/* the kernel starts a thread at its entry with the stack top in sp and the
 * argument in the first argument register; align, then call the body */
#if defined(__x86_64__)
__asm__(".text\n.globl bz_pthread_entry\nbz_pthread_entry:\n  and $-16, %rsp\n  xor %ebp, %ebp\n  call bz_pthread_body\n  ud2\n");
#elif defined(__aarch64__)
__asm__(".text\n.globl bz_pthread_entry\nbz_pthread_entry:\n  mov x9, sp\n  and x9, x9, #-16\n  mov sp, x9\n  mov x29, #0\n  bl bz_pthread_body\n  brk #0\n");
#elif defined(__riscv)
__asm__(".text\n.globl bz_pthread_entry\nbz_pthread_entry:\n  andi sp, sp, -16\n  li s0, 0\n  call bz_pthread_body\n  ebreak\n");
#endif
void bz_pthread_entry(void);

static void release(struct bz_thread *t)
{
    forget(t);
    bz_syscall(BZ_MUNMAP, (uint64_t)t->base, t->size, 0);
}

/* stacks of detached threads that finished */
static void reap_detached(void)
{
    for (int i = 0; i < MAX_THREADS; i++) {
        spin_lock(&table_lock);
        struct bz_thread *t = threads[i];
        int dead = t && t->detached && __atomic_load_n(&t->done, __ATOMIC_ACQUIRE);
        if (dead) threads[i] = 0;
        spin_unlock(&table_lock);
        if (dead) {
            bz_syscall(BZ_THREAD_JOIN, t->tid, 0, 0);
            bz_syscall(BZ_MUNMAP, (uint64_t)t->base, t->size, 0);
        }
    }
}

int pthread_create(pthread_t *out, const pthread_attr_t *attr, void *(*fn)(void *), void *arg)
{
    reap_detached();
    size_t size = attr && attr->stacksize ? attr->stacksize : 256 * 1024;
    size = (size + 4095) & ~(size_t)4095;
    uint64_t base = bz_syscall(BZ_MMAP, size, 3, 0);
    if (BZ_IS_ERROR(base) || !base) return EAGAIN;
    struct bz_thread *t = (struct bz_thread *)base;
    memset(t, 0, sizeof *t);
    t->fn = fn;
    t->arg = arg;
    t->base = (void *)base;
    t->size = size;
    t->detached = attr && attr->detached;
    spin_lock(&table_lock);
    int slot = -1;
    for (int i = 0; i < MAX_THREADS; i++)
        if (!threads[i]) { slot = i; break; }
    if (slot >= 0) threads[slot] = t;
    spin_unlock(&table_lock);
    if (slot < 0) {
        bz_syscall(BZ_MUNMAP, base, size, 0);
        return EAGAIN;
    }
    uint64_t tid = bz_syscall(BZ_THREAD_CREATE, (uint64_t)bz_pthread_entry, (uint64_t)t, base + size);
    if (BZ_IS_ERROR(tid)) {
        forget(t);
        bz_syscall(BZ_MUNMAP, base, size, 0);
        return EAGAIN;
    }
    __atomic_store_n(&t->tid, tid, __ATOMIC_RELEASE);
    *out = (pthread_t)t;
    return 0;
}

int pthread_join(pthread_t th, void **ret)
{
    struct bz_thread *t = (struct bz_thread *)th;
    if (!t || t == &main_thread || t->detached) return EINVAL;
    while (!__atomic_load_n(&t->tid, __ATOMIC_ACQUIRE)) bz_yield();
    void *r = 0;
    bz_syscall(BZ_THREAD_JOIN, t->tid, 0, 0);
    while (!__atomic_load_n(&t->done, __ATOMIC_ACQUIRE)) bz_yield();
    r = t->ret;
    release(t);
    if (ret) *ret = r;
    return 0;
}

int pthread_detach(pthread_t th)
{
    struct bz_thread *t = (struct bz_thread *)th;
    if (!t || t == &main_thread) return EINVAL;
    t->detached = 1;
    return 0;
}

void pthread_exit(void *ret)
{
    struct bz_thread *t = self();
    if (t == &main_thread) {
        run_destructors(t);
        exit(0);
    }
    finish(t, ret);
}

pthread_t pthread_self(void) { return (pthread_t)self(); }
int pthread_equal(pthread_t a, pthread_t b) { return a == b; }

int pthread_attr_init(pthread_attr_t *a) { a->stacksize = 0; a->detached = 0; return 0; }
int pthread_attr_destroy(pthread_attr_t *a) { (void)a; return 0; }
int pthread_attr_setstacksize(pthread_attr_t *a, size_t n) { if (n < PTHREAD_STACK_MIN) return EINVAL; a->stacksize = n; return 0; }
int pthread_attr_getstacksize(const pthread_attr_t *a, size_t *n) { *n = a->stacksize ? a->stacksize : 256 * 1024; return 0; }
int pthread_attr_setdetachstate(pthread_attr_t *a, int s) { a->detached = s == PTHREAD_CREATE_DETACHED; return 0; }

/* ---- mutexes: 0 free, 1 locked, 2 locked with waiters (Drepper) ---------- */

int pthread_mutexattr_init(pthread_mutexattr_t *a) { a->type = PTHREAD_MUTEX_DEFAULT; return 0; }
int pthread_mutexattr_settype(pthread_mutexattr_t *a, int type) { a->type = type; return 0; }
int pthread_mutexattr_destroy(pthread_mutexattr_t *a) { (void)a; return 0; }

int pthread_mutex_init(pthread_mutex_t *m, const pthread_mutexattr_t *a)
{
    m->state = 0;
    m->type = a ? a->type : PTHREAD_MUTEX_DEFAULT;
    m->owner = 0;
    m->count = 0;
    return 0;
}
int pthread_mutex_destroy(pthread_mutex_t *m) { return m->state ? EBUSY : 0; }

static uint64_t me(void) { return bz_syscall(BZ_THREAD_SELF, 0, 0, 0); }

int pthread_mutex_trylock(pthread_mutex_t *m)
{
    if (m->type != PTHREAD_MUTEX_NORMAL && m->state && m->owner == me()) {
        if (m->type == PTHREAD_MUTEX_RECURSIVE) { m->count++; return 0; }
        return EBUSY;
    }
    uint32_t zero = 0;
    if (!__atomic_compare_exchange_n(&m->state, &zero, 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) return EBUSY;
    if (m->type != PTHREAD_MUTEX_NORMAL) { m->owner = me(); m->count = 1; }
    return 0;
}

int pthread_mutex_lock(pthread_mutex_t *m)
{
    if (m->type != PTHREAD_MUTEX_NORMAL && m->state && m->owner == me()) {
        if (m->type == PTHREAD_MUTEX_RECURSIVE) { m->count++; return 0; }
        return EDEADLK;
    }
    uint32_t c = 0;
    if (!__atomic_compare_exchange_n(&m->state, &c, 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED)) {
        if (c != 2) c = __atomic_exchange_n(&m->state, 2, __ATOMIC_ACQUIRE);
        while (c != 0) {
            futex_wait(&m->state, 2);
            c = __atomic_exchange_n(&m->state, 2, __ATOMIC_ACQUIRE);
        }
    }
    if (m->type != PTHREAD_MUTEX_NORMAL) { m->owner = me(); m->count = 1; }
    return 0;
}

int pthread_mutex_unlock(pthread_mutex_t *m)
{
    if (m->type != PTHREAD_MUTEX_NORMAL) {
        if (!m->state || m->owner != me()) return EPERM;
        if (--m->count > 0) return 0;
        m->owner = 0;
    }
    if (__atomic_fetch_sub(&m->state, 1, __ATOMIC_RELEASE) != 1) {
        __atomic_store_n(&m->state, 0, __ATOMIC_RELEASE);
        futex_wake(&m->state, 1);
    }
    return 0;
}

/* ---- condition variables: a sequence word -------------------------------- */

int pthread_cond_init(pthread_cond_t *c, const pthread_condattr_t *a) { (void)a; c->seq = 0; return 0; }
int pthread_cond_destroy(pthread_cond_t *c) { (void)c; return 0; }

/* unlock fully (a recursive mutex too), wait, relock to the same depth */
static int cond_wait_ns(pthread_cond_t *c, pthread_mutex_t *m, uint64_t ns, int timed)
{
    uint32_t seq = __atomic_load_n(&c->seq, __ATOMIC_ACQUIRE);
    int depth = m->type == PTHREAD_MUTEX_RECURSIVE ? m->count : 1;
    if (m->type == PTHREAD_MUTEX_RECURSIVE) m->count = 1;
    pthread_mutex_unlock(m);
    int timeout = 0;
    if (timed) timeout = futex_wait_ns(&c->seq, seq, ns) == 1;
    else futex_wait(&c->seq, seq);
    pthread_mutex_lock(m);
    if (m->type == PTHREAD_MUTEX_RECURSIVE) m->count = depth;
    return timeout && __atomic_load_n(&c->seq, __ATOMIC_ACQUIRE) == seq ? ETIMEDOUT : 0;
}

int pthread_cond_wait(pthread_cond_t *c, pthread_mutex_t *m) { return cond_wait_ns(c, m, 0, 0); }

int pthread_cond_timedwait(pthread_cond_t *c, pthread_mutex_t *m, const struct timespec *abstime)
{
    struct timespec now;
    clock_gettime(CLOCK_REALTIME, &now);
    int64_t ns = (int64_t)(abstime->tv_sec - now.tv_sec) * 1000000000 + (abstime->tv_nsec - now.tv_nsec);
    if (ns <= 0) return ETIMEDOUT;
    return cond_wait_ns(c, m, (uint64_t)ns, 1);
}

int pthread_cond_signal(pthread_cond_t *c) { __atomic_fetch_add(&c->seq, 1, __ATOMIC_RELEASE); futex_wake(&c->seq, 1); return 0; }
int pthread_cond_broadcast(pthread_cond_t *c) { __atomic_fetch_add(&c->seq, 1, __ATOMIC_RELEASE); futex_wake(&c->seq, 0x7fffffff); return 0; }

/* ---- once, keys -------------------------------------------------------------- */

int pthread_once(pthread_once_t *o, void (*fn)(void))
{
    /* 0 not run, 1 running, 2 done */
    uint32_t z = 0;
    if (__atomic_compare_exchange_n(&o->state, &z, 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_ACQUIRE)) {
        fn();
        __atomic_store_n(&o->state, 2, __ATOMIC_RELEASE);
        futex_wake(&o->state, 0x7fffffff);
        return 0;
    }
    while (__atomic_load_n(&o->state, __ATOMIC_ACQUIRE) != 2) futex_wait(&o->state, 1);
    return 0;
}

int pthread_key_create(pthread_key_t *k, void (*destructor)(void *))
{
    for (unsigned i = 0; i < PTHREAD_KEYS_MAX; i++) {
        uint32_t z = 0;
        if (__atomic_compare_exchange_n(&key_used[i], &z, 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_RELAXED)) {
            key_dtor[i] = destructor;
            *k = i;
            return 0;
        }
    }
    return EAGAIN;
}
int pthread_key_delete(pthread_key_t k) { if (k >= PTHREAD_KEYS_MAX) return EINVAL; key_used[k] = 0; key_dtor[k] = 0; return 0; }
void *pthread_getspecific(pthread_key_t k) { return k < PTHREAD_KEYS_MAX ? self()->specific[k] : 0; }
int pthread_setspecific(pthread_key_t k, const void *v) { if (k >= PTHREAD_KEYS_MAX) return EINVAL; self()->specific[k] = (void *)v; return 0; }

/* ---- read-write locks, barriers, spin locks ------------------------------------ */

int pthread_rwlock_init(pthread_rwlock_t *l, const pthread_rwlockattr_t *a) { (void)a; memset(l, 0, sizeof *l); return 0; }
int pthread_rwlock_destroy(pthread_rwlock_t *l) { (void)l; return 0; }
int pthread_rwlock_rdlock(pthread_rwlock_t *l)
{
    pthread_mutex_lock(&l->lock);
    while (l->writer) pthread_cond_wait(&l->cond, &l->lock);
    l->readers++;
    pthread_mutex_unlock(&l->lock);
    return 0;
}
int pthread_rwlock_wrlock(pthread_rwlock_t *l)
{
    pthread_mutex_lock(&l->lock);
    while (l->writer || l->readers) pthread_cond_wait(&l->cond, &l->lock);
    l->writer = 1;
    pthread_mutex_unlock(&l->lock);
    return 0;
}
int pthread_rwlock_unlock(pthread_rwlock_t *l)
{
    pthread_mutex_lock(&l->lock);
    if (l->writer) l->writer = 0; else if (l->readers) l->readers--;
    pthread_cond_broadcast(&l->cond);
    pthread_mutex_unlock(&l->lock);
    return 0;
}

int pthread_barrier_init(pthread_barrier_t *b, const pthread_barrierattr_t *a, unsigned count)
{
    (void)a;
    if (!count) return EINVAL;
    memset(b, 0, sizeof *b);
    b->count = count;
    return 0;
}
int pthread_barrier_destroy(pthread_barrier_t *b) { (void)b; return 0; }
int pthread_barrier_wait(pthread_barrier_t *b)
{
    pthread_mutex_lock(&b->lock);
    unsigned cycle = b->cycle;
    if (++b->waiting == b->count) {
        b->waiting = 0;
        b->cycle++;
        pthread_cond_broadcast(&b->cond);
        pthread_mutex_unlock(&b->lock);
        return PTHREAD_BARRIER_SERIAL_THREAD;
    }
    while (cycle == b->cycle) pthread_cond_wait(&b->cond, &b->lock);
    pthread_mutex_unlock(&b->lock);
    return 0;
}

int pthread_spin_init(pthread_spinlock_t *s, int shared) { (void)shared; *s = 0; return 0; }
int pthread_spin_destroy(pthread_spinlock_t *s) { (void)s; return 0; }
int pthread_spin_lock(pthread_spinlock_t *s) { spin_lock(s); return 0; }
int pthread_spin_trylock(pthread_spinlock_t *s) { return __atomic_exchange_n(s, 1, __ATOMIC_ACQUIRE) ? EBUSY : 0; }
int pthread_spin_unlock(pthread_spinlock_t *s) { spin_unlock(s); return 0; }

int sched_yield(void) { bz_yield(); return 0; }

/* ---- clocks -------------------------------------------------------------------- */

int clock_gettime(clockid_t id, struct timespec *ts)
{
    uint64_t ns = bz_nanos();
    if (id == CLOCK_REALTIME) {
        /* wall clock = the RTC's second + the monotonic clock's sub-second part
         * (kept monotonic per call by pairing both reads) */
        static volatile uint64_t base_ns;
        uint64_t b = __atomic_load_n(&base_ns, __ATOMIC_ACQUIRE);
        if (!b) {
            uint64_t now_s = (uint64_t)time(0);
            b = now_s * 1000000000ull - ns;
            __atomic_store_n(&base_ns, b, __ATOMIC_RELEASE);
        }
        ns += b;
    }
    ts->tv_sec = (time_t)(ns / 1000000000ull);
    ts->tv_nsec = (long)(ns % 1000000000ull);
    return 0;
}

int nanosleep(const struct timespec *req, struct timespec *rem)
{
    uint64_t until = bz_nanos() + (uint64_t)req->tv_sec * 1000000000ull + (uint64_t)req->tv_nsec;
    bz_syscall(BZ_SLEEP_UNTIL, until, 0, 0);
    if (rem) { rem->tv_sec = 0; rem->tv_nsec = 0; }
    return 0;
}
