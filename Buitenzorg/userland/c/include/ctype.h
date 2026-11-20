/* Buitenzorg C runtime (userland/c): ctype.h -- the subset ported programs need. */
#ifndef BZ_CTYPE_H
#define BZ_CTYPE_H
int isalnum(int c);
int isalpha(int c);
int isdigit(int c);
int isxdigit(int c);
int isspace(int c);
int isupper(int c);
int islower(int c);
int isprint(int c);
int ispunct(int c);
int iscntrl(int c);
int isgraph(int c);
int toupper(int c);
int tolower(int c);
#endif
