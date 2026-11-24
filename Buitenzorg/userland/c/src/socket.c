/* Buitenzorg C runtime: BSD sockets, name lookup and poll (see
 * include/sys/socket.h). TCP goes through TCP_OP (the kernel's handles are
 * the process's own), UDP through NET_SOCKET/BIND/SEND/RECV, names through
 * DNS_RESOLVE. Every kernel call is non-blocking: blocking calls here poll
 * with a yield between tries (a TCP connect gives up after 15 s). The kernel
 * speaks IPv4 addresses as a << 24 | b << 16 | c << 8 | d; sockaddr_in keeps
 * network byte order, hence the ntohl/htonl at the boundary. */
#include <bz.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <poll.h>
#include <errno.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

enum { TCP_CONNECT = 0, TCP_SEND = 1, TCP_RECV = 2, TCP_STATE = 3, TCP_CLOSE = 4, TCP_LISTEN = 5, TCP_ACCEPT = 6 };
enum { ST_CONNECTING = 0, ST_OPEN = 1, ST_PEER_CLOSED = 2, ST_CLOSED = 3 };
enum { K_FREE = 0, K_TCP, K_LISTEN, K_UDP };

#define FIRST_FD 64
#define MAX_SOCKS 64
#define BUF 4096
#define UDP_MAX 1024

struct sock {
    int kind;
    int nonblock;
    uint64_t handle;
    uint16_t port;          /* local, host order */
    uint32_t peer_ip;       /* host order */
    uint16_t peer_port;
    int bound, connected;
    int rd_shut;
    /* bytes taken from the kernel ahead of recv (poll had to look) */
    unsigned char *buf;
    size_t have, at;
    /* one UDP datagram read ahead by poll */
    int udp_have;
    unsigned char udp[16 + UDP_MAX];
};

static struct sock socks[MAX_SOCKS];
static volatile uint32_t lock;
static uint16_t next_port = 49152;

static void L(void) { while (__atomic_exchange_n(&lock, 1, __ATOMIC_ACQUIRE)) bz_yield(); }
static void U(void) { __atomic_store_n(&lock, 0, __ATOMIC_RELEASE); }

static struct sock *get(int fd)
{
    if (fd < FIRST_FD || fd >= FIRST_FD + MAX_SOCKS) return 0;
    struct sock *s = &socks[fd - FIRST_FD];
    return s->kind == K_FREE ? 0 : s;
}

static uint64_t tcp(uint64_t op, uint64_t handle, uint64_t a1, uint64_t a2)
{
    return bz_syscall(BZ_TCP_OP, op | handle << 8, a1, a2);
}

static uint32_t local_ip(void)
{
    uint64_t info[16] = { 0 };
    if (bz_syscall(BZ_NET_INFO, (uint64_t)info, 0, 0) != 0) return 0;
    unsigned char *a = (unsigned char *)info;
    return (uint32_t)a[0] << 24 | (uint32_t)a[1] << 16 | (uint32_t)a[2] << 8 | a[3];
}

/* 127.x and 0.0.0.0 mean this machine: the kernel's loopback is its own address */
static uint32_t route(uint32_t ip)
{
    if ((ip >> 24) == 127 || ip == 0) {
        uint32_t me = local_ip();
        if (me) return me;
    }
    return ip;
}

int socket(int domain, int type, int protocol)
{
    (void)protocol;
    if (domain != AF_INET) { errno = EAFNOSUPPORT; return -1; }
    int base = type & 0xF;
    if (base != SOCK_STREAM && base != SOCK_DGRAM) { errno = EPROTONOSUPPORT; return -1; }
    L();
    int fd = -1;
    for (int i = 0; i < MAX_SOCKS; i++)
        if (socks[i].kind == K_FREE) { fd = FIRST_FD + i; break; }
    if (fd < 0) { U(); errno = EMFILE; return -1; }
    struct sock *s = &socks[fd - FIRST_FD];
    memset(s, 0, sizeof *s);
    s->kind = base == SOCK_STREAM ? K_TCP : K_UDP;
    s->nonblock = (type & SOCK_NONBLOCK) != 0;
    U();
    if (s->kind == K_UDP) {
        uint64_t h = bz_syscall(BZ_NET_SOCKET, 0, 0, 0);
        if (h == 0 || BZ_IS_ERROR(h)) { s->kind = K_FREE; errno = ENOSYS; return -1; }
        s->handle = h;
    }
    return fd;
}

static int fail(int e) { errno = e; return -1; }

static void udp_autobind(struct sock *s)
{
    if (s->bound) return;
    for (int tries = 0; tries < 64; tries++) {
        uint16_t p = next_port++;
        if (next_port < 49152) next_port = 49152;
        if (bz_syscall(BZ_NET_BIND, s->handle, p, 0) == 0) { s->port = p; s->bound = 1; return; }
    }
}

int bind(int fd, const struct sockaddr *a, socklen_t n)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (n < sizeof(struct sockaddr_in) || a->sa_family != AF_INET) return fail(EINVAL);
    const struct sockaddr_in *in = (const struct sockaddr_in *)a;
    uint16_t port = ntohs(in->sin_port);
    if (s->kind == K_UDP) {
        if (port == 0) { udp_autobind(s); return 0; }
        if (bz_syscall(BZ_NET_BIND, s->handle, port, 0) != 0) return fail(EADDRINUSE);
    }
    s->port = port;
    s->bound = 1;
    return 0;
}

int listen(int fd, int backlog)
{
    (void)backlog;
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (s->kind != K_TCP || !s->port) return fail(EINVAL);
    uint64_t h = tcp(TCP_LISTEN, 0, 0, s->port);
    if (BZ_IS_ERROR(h) || h == 0) return fail(EADDRINUSE);
    s->handle = h;
    s->kind = K_LISTEN;
    return 0;
}

int accept(int fd, struct sockaddr *a, socklen_t *n)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (s->kind != K_LISTEN) return fail(EINVAL);
    uint64_t h;
    for (;;) {
        h = tcp(TCP_ACCEPT, s->handle, 0, 0);
        if (BZ_IS_ERROR(h)) return fail(EINVAL);
        if (h) break;
        if (s->nonblock) return fail(EAGAIN);
        bz_yield();
    }
    int nfd = socket(AF_INET, SOCK_STREAM, 0);
    if (nfd < 0) { tcp(TCP_CLOSE, h, 0, 0); return -1; }
    struct sock *c = get(nfd);
    c->handle = h;
    c->connected = 1;
    c->port = s->port;
    if (a && n && *n >= sizeof(struct sockaddr_in)) {
        struct sockaddr_in *in = (struct sockaddr_in *)a;
        memset(in, 0, sizeof *in);
        in->sin_family = AF_INET; /* the kernel does not report the peer */
        *n = sizeof *in;
    }
    return nfd;
}

int connect(int fd, const struct sockaddr *a, socklen_t n)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (n < sizeof(struct sockaddr_in) || a->sa_family != AF_INET) return fail(EAFNOSUPPORT);
    const struct sockaddr_in *in = (const struct sockaddr_in *)a;
    uint32_t ip = route(ntohl(in->sin_addr.s_addr));
    uint16_t port = ntohs(in->sin_port);
    s->peer_ip = ip;
    s->peer_port = port;
    if (s->kind == K_UDP) { udp_autobind(s); s->connected = 1; return 0; }
    if (s->connected) return fail(EISCONN);
    uint64_t h = tcp(TCP_CONNECT, 0, ip, port);
    if (BZ_IS_ERROR(h) || h == 0) return fail(ECONNREFUSED);
    s->handle = h;
    uint64_t deadline = bz_nanos() + 15000000000ull;
    for (;;) {
        uint64_t st = tcp(TCP_STATE, h, 0, 0);
        if (st == ST_OPEN || st == ST_PEER_CLOSED) break;
        if (st == ST_CLOSED || BZ_IS_ERROR(st)) { tcp(TCP_CLOSE, h, 0, 0); s->handle = 0; return fail(ECONNREFUSED); }
        if (bz_nanos() > deadline) { tcp(TCP_CLOSE, h, 0, 0); s->handle = 0; return fail(ETIMEDOUT); }
        bz_yield();
    }
    s->connected = 1;
    return 0;
}

/* pull what the kernel holds into the read-ahead buffer; returns bytes now
 * buffered, 0 at end of stream, -1 = nothing yet */
static long tcp_fill(struct sock *s)
{
    if (s->have > s->at) return (long)(s->have - s->at);
    if (!s->buf) { s->buf = malloc(BUF); if (!s->buf) return -1; }
    uint64_t r = tcp(TCP_RECV, s->handle, (uint64_t)s->buf, BUF);
    if (BZ_IS_ERROR(r)) return 0;
    if (r) { s->have = r; s->at = 0; return (long)r; }
    uint64_t st = tcp(TCP_STATE, s->handle, 0, 0);
    if (st == ST_PEER_CLOSED || st == ST_CLOSED || BZ_IS_ERROR(st)) return 0;
    return -1;
}

ssize_t send(int fd, const void *p, size_t n, int flags)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (s->kind == K_UDP) return sendto(fd, p, n, flags, 0, 0);
    if (s->kind != K_TCP || !s->connected) return fail(ENOTCONN);
    size_t done = 0;
    while (done < n) {
        uint64_t r = tcp(TCP_SEND, s->handle, (uint64_t)((const char *)p + done), n - done);
        if (BZ_IS_ERROR(r)) return done ? (ssize_t)done : fail(EPIPE);
        if (r == 0) {
            uint64_t st = tcp(TCP_STATE, s->handle, 0, 0);
            if (st == ST_CLOSED) return done ? (ssize_t)done : fail(EPIPE);
            if ((flags & MSG_DONTWAIT) || s->nonblock) return done ? (ssize_t)done : fail(EAGAIN);
            bz_yield();
            continue;
        }
        done += r;
    }
    return (ssize_t)done;
}

ssize_t recv(int fd, void *p, size_t n, int flags)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (s->kind == K_UDP) return recvfrom(fd, p, n, flags, 0, 0);
    if (s->kind != K_TCP || !s->connected) return fail(ENOTCONN);
    if (s->rd_shut || n == 0) return 0;
    long got;
    while ((got = tcp_fill(s)) < 0) {
        if ((flags & MSG_DONTWAIT) || s->nonblock) return fail(EAGAIN);
        bz_yield();
    }
    if (got == 0) return 0;
    size_t k = (size_t)got < n ? (size_t)got : n;
    memcpy(p, s->buf + s->at, k);
    if (!(flags & MSG_PEEK)) s->at += k;
    return (ssize_t)k;
}

ssize_t sendto(int fd, const void *p, size_t n, int flags, const struct sockaddr *a, socklen_t an)
{
    (void)flags;
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (s->kind != K_UDP) return a ? fail(EISCONN) : send(fd, p, n, flags);
    uint32_t ip = s->peer_ip;
    uint16_t port = s->peer_port;
    if (a) {
        if (an < sizeof(struct sockaddr_in)) return fail(EINVAL);
        const struct sockaddr_in *in = (const struct sockaddr_in *)a;
        ip = route(ntohl(in->sin_addr.s_addr));
        port = ntohs(in->sin_port);
    } else if (!s->connected) {
        return fail(ENOTCONN);
    }
    if (n > UDP_MAX) return fail(EINVAL);
    udp_autobind(s);
    unsigned char pkt[16 + UDP_MAX];
    pkt[0] = ip >> 24; pkt[1] = ip >> 16; pkt[2] = ip >> 8; pkt[3] = ip;
    uint32_t port32 = port;
    memcpy(pkt + 4, &port32, 4);
    uint64_t len = n;
    memcpy(pkt + 8, &len, 8);
    memcpy(pkt + 16, p, n);
    uint64_t r = bz_syscall(BZ_NET_SEND, s->handle, (uint64_t)pkt, n);
    if (BZ_IS_ERROR(r)) return fail(EINVAL);
    return (ssize_t)r;
}

/* one datagram into s->udp; 1 = there is one */
static int udp_fill(struct sock *s)
{
    if (s->udp_have) return 1;
    uint64_t r = bz_syscall(BZ_NET_RECV, s->handle, (uint64_t)s->udp, UDP_MAX);
    if (BZ_IS_ERROR(r) || r == 0) return 0;
    uint64_t len = r;
    memcpy(s->udp + 8, &len, 8);
    s->udp_have = 1;
    return 1;
}

ssize_t recvfrom(int fd, void *p, size_t n, int flags, struct sockaddr *a, socklen_t *an)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (s->kind != K_UDP) return recv(fd, p, n, flags);
    if (!s->bound) return fail(EINVAL);
    while (!udp_fill(s)) {
        if ((flags & MSG_DONTWAIT) || s->nonblock) return fail(EAGAIN);
        bz_yield();
    }
    uint64_t len;
    memcpy(&len, s->udp + 8, 8);
    size_t k = len < n ? (size_t)len : n;
    memcpy(p, s->udp + 16, k);
    if (a && an && *an >= sizeof(struct sockaddr_in)) {
        struct sockaddr_in *in = (struct sockaddr_in *)a;
        memset(in, 0, sizeof *in);
        in->sin_family = AF_INET;
        uint32_t ip = (uint32_t)s->udp[0] << 24 | (uint32_t)s->udp[1] << 16 | (uint32_t)s->udp[2] << 8 | s->udp[3];
        uint32_t port;
        memcpy(&port, s->udp + 4, 4);
        in->sin_addr.s_addr = htonl(ip);
        in->sin_port = htons((uint16_t)port);
        *an = sizeof *in;
    }
    if (!(flags & MSG_PEEK)) s->udp_have = 0;
    return (ssize_t)k;
}

int shutdown(int fd, int how)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (how == SHUT_RD || how == SHUT_RDWR) s->rd_shut = 1;
    return 0; /* the kernel has no half-close: SHUT_WR waits for close() */
}

int setsockopt(int fd, int level, int name, const void *v, socklen_t n)
{
    (void)level; (void)name; (void)v; (void)n;
    return get(fd) ? 0 : fail(ENOTSOCK);
}

int getsockopt(int fd, int level, int name, void *v, socklen_t *n)
{
    (void)level;
    if (!get(fd)) return fail(ENOTSOCK);
    if (v && n && *n >= sizeof(int)) { *(int *)v = name == SO_ERROR ? 0 : 0; *n = sizeof(int); }
    return 0;
}

static int addr_out(struct sockaddr *a, socklen_t *n, uint32_t ip, uint16_t port)
{
    if (!a || !n || *n < sizeof(struct sockaddr_in)) return fail(EINVAL);
    struct sockaddr_in *in = (struct sockaddr_in *)a;
    memset(in, 0, sizeof *in);
    in->sin_family = AF_INET;
    in->sin_addr.s_addr = htonl(ip);
    in->sin_port = htons(port);
    *n = sizeof *in;
    return 0;
}

int getsockname(int fd, struct sockaddr *a, socklen_t *n)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    return addr_out(a, n, local_ip(), s->port);
}

int getpeername(int fd, struct sockaddr *a, socklen_t *n)
{
    struct sock *s = get(fd);
    if (!s) return fail(ENOTSOCK);
    if (!s->connected) return fail(ENOTCONN);
    return addr_out(a, n, s->peer_ip, s->peer_port);
}

/* hooks for libc's close / read / write on descriptors >= 64 */
int bz_sock_close(int fd)
{
    struct sock *s = get(fd);
    if (!s) return fail(EBADF);
    if (s->kind == K_UDP) bz_syscall(BZ_NET_CLOSE, s->handle, 0, 0);
    else if (s->handle) tcp(TCP_CLOSE, s->handle, 0, 0);
    free(s->buf);
    L();
    s->kind = K_FREE;
    U();
    return 0;
}
ssize_t bz_sock_read(int fd, void *p, size_t n) { return recv(fd, p, n, 0); }
ssize_t bz_sock_write(int fd, const void *p, size_t n) { return send(fd, p, n, 0); }

int poll(struct pollfd *fds, nfds_t n, int timeout_ms)
{
    uint64_t deadline = timeout_ms < 0 ? ~0ull : bz_nanos() + (uint64_t)timeout_ms * 1000000ull;
    for (;;) {
        int ready = 0;
        for (nfds_t i = 0; i < n; i++) {
            struct pollfd *f = &fds[i];
            f->revents = 0;
            struct sock *s = get(f->fd);
            if (!s) {
                if (f->fd == 1 || f->fd == 2) f->revents = f->events & POLLOUT;
                else if (f->fd >= 0) f->revents = POLLNVAL;
            } else if (s->kind == K_LISTEN) {
                /* a pending connection cannot be peeked without taking it */
                if (f->events & POLLIN) f->revents |= POLLIN;
            } else if (s->kind == K_UDP) {
                if ((f->events & POLLIN) && s->bound && udp_fill(s)) f->revents |= POLLIN;
                if (f->events & POLLOUT) f->revents |= POLLOUT;
            } else if (s->connected) {
                if (f->events & POLLIN) {
                    long got = tcp_fill(s);
                    if (got > 0) f->revents |= POLLIN;
                    else if (got == 0) f->revents |= POLLIN | POLLHUP;
                }
                if (f->events & POLLOUT) f->revents |= POLLOUT;
            }
            if (f->revents) ready++;
        }
        if (ready || timeout_ms == 0 || bz_nanos() >= deadline) return ready;
        bz_yield();
    }
}

/* ---- addresses and names ---------------------------------------------------- */

int inet_aton(const char *str, struct in_addr *a)
{
    uint32_t v = 0;
    for (int part = 0; part < 4; part++) {
        if (*str < '0' || *str > '9') return 0;
        unsigned x = 0;
        while (*str >= '0' && *str <= '9') { x = x * 10 + (unsigned)(*str++ - '0'); if (x > 255) return 0; }
        v = v << 8 | x;
        if (part < 3) { if (*str != '.') return 0; str++; }
    }
    if (*str) return 0;
    a->s_addr = htonl(v);
    return 1;
}

in_addr_t inet_addr(const char *s)
{
    struct in_addr a;
    return inet_aton(s, &a) ? a.s_addr : INADDR_NONE;
}

int inet_pton(int af, const char *s, void *dst)
{
    if (af != AF_INET) { errno = EAFNOSUPPORT; return -1; }
    return inet_aton(s, (struct in_addr *)dst);
}

const char *inet_ntop(int af, const void *src, char *dst, socklen_t n)
{
    if (af != AF_INET) { errno = EAFNOSUPPORT; return 0; }
    uint32_t v = ntohl(((const struct in_addr *)src)->s_addr);
    char tmp[16];
    snprintf(tmp, sizeof tmp, "%u.%u.%u.%u", v >> 24, (v >> 16) & 255, (v >> 8) & 255, v & 255);
    if (strlen(tmp) + 1 > n) { errno = ENOSPC; return 0; }
    strcpy(dst, tmp);
    return dst;
}

char *inet_ntoa(struct in_addr a)
{
    static char buf[16];
    inet_ntop(AF_INET, &a, buf, sizeof buf);
    return buf;
}

int h_errno;

static int resolve(const char *name, uint32_t *ip)
{
    struct in_addr a;
    if (inet_aton(name, &a)) { *ip = ntohl(a.s_addr); return 1; }
    uint64_t r = bz_syscall(BZ_DNS_RESOLVE, (uint64_t)name, 0, 0);
    if (BZ_IS_ERROR(r) || (uint32_t)r == 0) return 0;
    *ip = (uint32_t)r;
    return 1;
}

struct hostent *gethostbyname(const char *name)
{
    static struct hostent h;
    static struct in_addr addr;
    static char *list[2];
    static char hname[256];
    uint32_t ip;
    if (!resolve(name, &ip)) { h_errno = HOST_NOT_FOUND; return 0; }
    addr.s_addr = htonl(ip);
    list[0] = (char *)&addr;
    list[1] = 0;
    strncpy(hname, name, sizeof hname - 1);
    h.h_name = hname;
    h.h_aliases = list + 1;
    h.h_addrtype = AF_INET;
    h.h_length = 4;
    h.h_addr_list = list;
    return &h;
}

int getaddrinfo(const char *node, const char *service, const struct addrinfo *hints, struct addrinfo **res)
{
    if (hints && hints->ai_family != AF_UNSPEC && hints->ai_family != AF_INET) return EAI_FAMILY;
    uint32_t ip = 0;
    if (node) {
        struct in_addr a;
        if (inet_aton(node, &a)) ip = ntohl(a.s_addr);
        else if ((hints && (hints->ai_flags & AI_NUMERICHOST)) || !resolve(node, &ip)) return EAI_NONAME;
    } else if (!(hints && (hints->ai_flags & AI_PASSIVE))) {
        ip = 0x7f000001;
    }
    unsigned port = 0;
    if (service) {
        char *end;
        port = (unsigned)strtol(service, &end, 10);
        if (*end || port > 65535) {
            if (!strcmp(service, "http")) port = 80;
            else if (!strcmp(service, "https")) port = 443;
            else return EAI_SERVICE;
        }
    }
    struct addrinfo *ai = calloc(1, sizeof *ai + sizeof(struct sockaddr_in));
    if (!ai) return EAI_MEMORY;
    struct sockaddr_in *in = (struct sockaddr_in *)(ai + 1);
    in->sin_family = AF_INET;
    in->sin_port = htons((uint16_t)port);
    in->sin_addr.s_addr = htonl(ip);
    ai->ai_family = AF_INET;
    ai->ai_socktype = hints && hints->ai_socktype ? hints->ai_socktype : SOCK_STREAM;
    ai->ai_protocol = ai->ai_socktype == SOCK_DGRAM ? IPPROTO_UDP : IPPROTO_TCP;
    ai->ai_addrlen = sizeof *in;
    ai->ai_addr = (struct sockaddr *)in;
    *res = ai;
    return 0;
}

void freeaddrinfo(struct addrinfo *res)
{
    while (res) { struct addrinfo *n = res->ai_next; free(res); res = n; }
}

const char *gai_strerror(int e)
{
    switch (e) {
    case EAI_NONAME: return "name not found";
    case EAI_FAMILY: return "address family not supported";
    case EAI_SERVICE: return "service not known";
    case EAI_MEMORY: return "out of memory";
    default: return "unknown error";
    }
}
