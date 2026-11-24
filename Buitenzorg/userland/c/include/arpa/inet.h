/* Buitenzorg C runtime (userland/c): arpa/inet.h */
#ifndef BZ_ARPA_INET_H
#define BZ_ARPA_INET_H
#include <netinet/in.h>
#define INET_ADDRSTRLEN 16
in_addr_t inet_addr(const char *s);
int inet_aton(const char *s, struct in_addr *a);
char *inet_ntoa(struct in_addr a);
int inet_pton(int af, const char *s, void *dst);
const char *inet_ntop(int af, const void *src, char *dst, socklen_t n);
#endif
