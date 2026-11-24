/* Buitenzorg C runtime (userland/c): sys/socket.h -- BSD sockets over the
 * kernel's TCP_OP (TCP) and NET_* (UDP) syscalls. IPv4 only (AF_INET).
 * Socket descriptors start at 64 and work with read/write/close/poll. */
#ifndef BZ_SYS_SOCKET_H
#define BZ_SYS_SOCKET_H
#include <sys/types.h>
typedef unsigned int socklen_t;
typedef unsigned short sa_family_t;
struct sockaddr { sa_family_t sa_family; char sa_data[14]; };
struct sockaddr_storage { sa_family_t ss_family; char ss_data[126]; };
#define AF_UNSPEC 0
#define AF_INET 2
#define AF_INET6 10
#define PF_INET AF_INET
#define SOCK_STREAM 1
#define SOCK_DGRAM 2
#define SOCK_NONBLOCK 04000
#define SOCK_CLOEXEC 02000000
#define SOL_SOCKET 1
#define SO_REUSEADDR 2
#define SO_ERROR 4
#define SO_KEEPALIVE 9
#define SO_RCVTIMEO 20
#define SO_SNDTIMEO 21
#define MSG_PEEK 0x02
#define MSG_DONTWAIT 0x40
#define MSG_NOSIGNAL 0x4000
#define SHUT_RD 0
#define SHUT_WR 1
#define SHUT_RDWR 2
#define SOMAXCONN 16
int socket(int domain, int type, int protocol);
int bind(int fd, const struct sockaddr *a, socklen_t n);
int listen(int fd, int backlog);
int accept(int fd, struct sockaddr *a, socklen_t *n);
int connect(int fd, const struct sockaddr *a, socklen_t n);
ssize_t send(int fd, const void *p, size_t n, int flags);
ssize_t recv(int fd, void *p, size_t n, int flags);
ssize_t sendto(int fd, const void *p, size_t n, int flags, const struct sockaddr *a, socklen_t an);
ssize_t recvfrom(int fd, void *p, size_t n, int flags, struct sockaddr *a, socklen_t *an);
int shutdown(int fd, int how);
int setsockopt(int fd, int level, int name, const void *v, socklen_t n);
int getsockopt(int fd, int level, int name, void *v, socklen_t *n);
int getsockname(int fd, struct sockaddr *a, socklen_t *n);
int getpeername(int fd, struct sockaddr *a, socklen_t *n);
#endif
