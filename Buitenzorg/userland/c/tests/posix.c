/* POSIX.ELF -- the C runtime's threads and sockets, end to end on the device:
 * pthread mutex/cond/once/keys/rwlock/barrier with real parallel threads, a
 * TCP echo server thread + client over the kernel's TCP, UDP between two
 * sockets, name and address helpers. Prints MILESTONE: POSIX OK. */
#include <pthread.h>
#include <sched.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <poll.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <time.h>

static int checks, failed;
static void check(int ok, const char *what)
{
    checks++;
    if (!ok) { failed++; printf("POSIX: FAIL %s\n", what); }
}

/* ---- threads ------------------------------------------------------------ */

static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
static long counter;
static void *bump(void *arg)
{
    long n = (long)arg;
    for (long i = 0; i < n; i++) {
        pthread_mutex_lock(&mu);
        long v = counter;
        if ((i & 63) == 0) sched_yield(); /* hold the lock across a switch */
        counter = v + 1;
        pthread_mutex_unlock(&mu);
    }
    return (void *)(n * 2);
}

#define QN 64
static int queue[QN], qhead, qtail, produced_sum, consumed_sum;
static pthread_mutex_t qm = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t not_empty = PTHREAD_COND_INITIALIZER, not_full = PTHREAD_COND_INITIALIZER;
static void *producer(void *arg)
{
    (void)arg;
    for (int i = 1; i <= 500; i++) {
        pthread_mutex_lock(&qm);
        while ((qtail + 1) % QN == qhead) pthread_cond_wait(&not_full, &qm);
        queue[qtail] = i;
        qtail = (qtail + 1) % QN;
        produced_sum += i;
        pthread_cond_signal(&not_empty);
        pthread_mutex_unlock(&qm);
    }
    return 0;
}
static void *consumer(void *arg)
{
    (void)arg;
    for (int i = 0; i < 500; i++) {
        pthread_mutex_lock(&qm);
        while (qhead == qtail) pthread_cond_wait(&not_empty, &qm);
        consumed_sum += queue[qhead];
        qhead = (qhead + 1) % QN;
        pthread_cond_signal(&not_full);
        pthread_mutex_unlock(&qm);
    }
    return 0;
}

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int once_runs;
static void init_once(void) { once_runs++; }
static pthread_key_t key;
static int dtor_runs;
static void dtor(void *v) { (void)v; __atomic_fetch_add(&dtor_runs, 1, __ATOMIC_SEQ_CST); }
static pthread_barrier_t bar;
static int before_barrier, after_ok;
static void *keyed(void *arg)
{
    pthread_once(&once, init_once);
    pthread_setspecific(key, arg);
    sched_yield();
    int mine = pthread_getspecific(key) == arg;
    __atomic_fetch_add(&before_barrier, 1, __ATOMIC_SEQ_CST);
    pthread_barrier_wait(&bar);
    if (__atomic_load_n(&before_barrier, __ATOMIC_SEQ_CST) == 4 && mine) __atomic_fetch_add(&after_ok, 1, __ATOMIC_SEQ_CST);
    return 0;
}

static void test_threads(void)
{
    pthread_t t[4];
    void *ret[4];
    for (int i = 0; i < 4; i++) check(pthread_create(&t[i], 0, bump, (void *)2000L) == 0, "pthread_create");
    for (int i = 0; i < 4; i++) check(pthread_join(t[i], &ret[i]) == 0 && ret[i] == (void *)4000L, "pthread_join returns the value");
    check(counter == 8000, "4 threads x 2000 increments under a mutex = 8000");

    pthread_t p, c;
    pthread_create(&p, 0, producer, 0);
    pthread_create(&c, 0, consumer, 0);
    pthread_join(p, 0);
    pthread_join(c, 0);
    check(produced_sum == 125250 && consumed_sum == 125250, "condition variables: producer/consumer through a 64-slot queue");

    pthread_mutex_t r;
    pthread_mutexattr_t ra;
    pthread_mutexattr_init(&ra);
    pthread_mutexattr_settype(&ra, PTHREAD_MUTEX_RECURSIVE);
    pthread_mutex_init(&r, &ra);
    int rok = pthread_mutex_lock(&r) == 0 && pthread_mutex_lock(&r) == 0 && pthread_mutex_unlock(&r) == 0 && pthread_mutex_trylock(&r) == 0;
    pthread_mutex_unlock(&r);
    pthread_mutex_unlock(&r);
    check(rok && r.state == 0, "recursive mutex");
    check(pthread_mutex_trylock(&mu) == 0 && pthread_mutex_trylock(&mu) == EBUSY && pthread_mutex_unlock(&mu) == 0, "trylock");

    pthread_key_create(&key, dtor);
    pthread_barrier_init(&bar, 0, 4);
    pthread_t k[4];
    for (long i = 0; i < 4; i++) pthread_create(&k[i], 0, keyed, (void *)(i + 1));
    for (int i = 0; i < 4; i++) pthread_join(k[i], 0);
    check(once_runs == 1, "pthread_once ran once");
    check(after_ok == 4, "per-thread keys + a barrier of 4");
    check(dtor_runs == 4, "key destructors ran at thread exit");

    pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
    pthread_mutex_t cm = PTHREAD_MUTEX_INITIALIZER;
    struct timespec t0, dl;
    clock_gettime(CLOCK_REALTIME, &t0);
    dl = t0;
    dl.tv_nsec += 100 * 1000000;
    if (dl.tv_nsec >= 1000000000) { dl.tv_sec++; dl.tv_nsec -= 1000000000; }
    pthread_mutex_lock(&cm);
    int tr = pthread_cond_timedwait(&cv, &cm, &dl);
    pthread_mutex_unlock(&cm);
    struct timespec t1;
    clock_gettime(CLOCK_REALTIME, &t1);
    long ms = (long)(t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
    check(tr == ETIMEDOUT && ms >= 80 && ms < 2000, "pthread_cond_timedwait times out after ~100 ms");
}

/* ---- sockets ---------------------------------------------------------------- */

#define PORT 7344
#define BYTES 20000
static volatile int server_ready;
static void *echo_server(void *arg)
{
    (void)arg;
    int s = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons(PORT);
    a.sin_addr.s_addr = htonl(INADDR_ANY);
    if (bind(s, (struct sockaddr *)&a, sizeof a) || listen(s, 4)) { server_ready = -1; return 0; }
    server_ready = 1;
    int c = accept(s, 0, 0);
    char buf[1500];
    long total = 0;
    for (;;) {
        ssize_t n = recv(c, buf, sizeof buf, 0);
        if (n <= 0) break;
        send(c, buf, (size_t)n, 0);
        total += n;
        if (total >= BYTES) break;
    }
    close(c);
    close(s);
    return (void *)total;
}

static void test_sockets(void)
{
    struct in_addr ia;
    check(inet_aton("10.0.2.15", &ia) && ntohl(ia.s_addr) == 0x0A00020F && !strcmp(inet_ntoa(ia), "10.0.2.15"), "inet_aton / inet_ntoa");
    check(inet_addr("300.1.1.1") == INADDR_NONE, "inet_addr rejects a bad address");
    struct addrinfo *ai = 0, hints;
    memset(&hints, 0, sizeof hints);
    hints.ai_socktype = SOCK_DGRAM;
    check(getaddrinfo("192.168.1.9", "53", &hints, &ai) == 0 && ai && ntohs(((struct sockaddr_in *)ai->ai_addr)->sin_port) == 53, "getaddrinfo numeric");
    freeaddrinfo(ai);

    pthread_t srv;
    pthread_create(&srv, 0, echo_server, 0);
    while (!server_ready) sched_yield();
    check(server_ready == 1, "TCP: bind + listen");
    int c = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons(PORT);
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    int conn = connect(c, (struct sockaddr *)&a, sizeof a);
    check(conn == 0, "TCP: connect to the echo server");
    unsigned char *out = malloc(BYTES), *in = malloc(BYTES);
    for (int i = 0; i < BYTES; i++) out[i] = (unsigned char)(i * 7 + 3);
    long got = 0;
    if (conn == 0) {
        check(send(c, out, BYTES, 0) == BYTES, "TCP: send 20000 bytes");
        while (got < BYTES) {
            struct pollfd pf = { c, POLLIN, 0 };
            if (poll(&pf, 1, 5000) <= 0) break;
            ssize_t n = read(c, in + got, BYTES - got);
            if (n <= 0) break;
            got += n;
        }
    }
    void *served = 0;
    pthread_join(srv, &served);
    check(got == BYTES && memcmp(in, out, BYTES) == 0 && (long)served == BYTES, "TCP: 20000 bytes echoed back intact (poll + read)");
    close(c);

    int u1 = socket(AF_INET, SOCK_DGRAM, 0), u2 = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in b1, b2;
    memset(&b1, 0, sizeof b1);
    b1.sin_family = AF_INET;
    b1.sin_port = htons(7345);
    b2 = b1;
    b2.sin_port = htons(7346);
    check(bind(u1, (struct sockaddr *)&b1, sizeof b1) == 0 && bind(u2, (struct sockaddr *)&b2, sizeof b2) == 0, "UDP: bind two sockets");
    b2.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    const char msg[] = "halo dari soket UDP";
    check(sendto(u1, msg, sizeof msg, 0, (struct sockaddr *)&b2, sizeof b2) == (ssize_t)sizeof msg, "UDP: sendto");
    char rbuf[64];
    struct sockaddr_in from;
    socklen_t fl = sizeof from;
    struct pollfd up = { u2, POLLIN, 0 };
    int pr = poll(&up, 1, 3000);
    ssize_t rn = pr > 0 ? recvfrom(u2, rbuf, sizeof rbuf, 0, (struct sockaddr *)&from, &fl) : -1;
    check(rn == (ssize_t)sizeof msg && !memcmp(rbuf, msg, sizeof msg) && ntohs(from.sin_port) == 7345, "UDP: recvfrom gets the datagram and its sender port");
    check(recvfrom(u2, rbuf, sizeof rbuf, MSG_DONTWAIT, 0, 0) == -1 && errno == EAGAIN, "UDP: MSG_DONTWAIT on an empty socket = EAGAIN");
    close(u1);
    close(u2);
}

int main(void)
{
    uint64_t t0 = 0;
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    t0 = (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
    test_threads();
    test_sockets();
    clock_gettime(CLOCK_MONOTONIC, &ts);
    uint64_t ms = (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000 - t0;
    printf("POSIX: %d checks, %d failed (%llu ms)\n", checks, failed, (unsigned long long)ms);
    if (failed == 0)
        printf("MILESTONE: POSIX OK (pthreads: mutex, cond, once, keys, barrier, timedwait; sockets: TCP echo 20000 bytes, UDP, poll, getaddrinfo; %d checks)\n", checks);
    return failed ? 1 : 0;
}
