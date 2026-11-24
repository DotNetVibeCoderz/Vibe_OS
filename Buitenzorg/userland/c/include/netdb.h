/* Buitenzorg C runtime (userland/c): netdb.h -- name lookup through the
 * kernel's DNS client (DNS_RESOLVE: A records, cached). */
#ifndef BZ_NETDB_H
#define BZ_NETDB_H
#include <netinet/in.h>
struct hostent { char *h_name; char **h_aliases; int h_addrtype; int h_length; char **h_addr_list; };
#define h_addr h_addr_list[0]
struct addrinfo {
    int ai_flags, ai_family, ai_socktype, ai_protocol;
    socklen_t ai_addrlen;
    struct sockaddr *ai_addr;
    char *ai_canonname;
    struct addrinfo *ai_next;
};
#define AI_PASSIVE 1
#define AI_CANONNAME 2
#define AI_NUMERICHOST 4
#define EAI_NONAME (-2)
#define EAI_FAMILY (-6)
#define EAI_SERVICE (-8)
#define EAI_MEMORY (-10)
extern int h_errno;
#define HOST_NOT_FOUND 1
struct hostent *gethostbyname(const char *name);
int getaddrinfo(const char *node, const char *service, const struct addrinfo *hints, struct addrinfo **res);
void freeaddrinfo(struct addrinfo *res);
const char *gai_strerror(int e);
#endif
