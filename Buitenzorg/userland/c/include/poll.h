/* Buitenzorg C runtime (userland/c): poll.h -- for socket descriptors. */
#ifndef BZ_POLL_H
#define BZ_POLL_H
struct pollfd { int fd; short events, revents; };
typedef unsigned long nfds_t;
#define POLLIN 0x001
#define POLLOUT 0x004
#define POLLERR 0x008
#define POLLHUP 0x010
#define POLLNVAL 0x020
int poll(struct pollfd *fds, nfds_t n, int timeout_ms);
#endif
