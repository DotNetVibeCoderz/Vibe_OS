/* Buitenzorg C runtime (userland/c): math.h -- the subset ported programs need. */
#ifndef BZ_MATH_H
#define BZ_MATH_H
double fabs(double x);
float fabsf(float x);
double floor(double x);
double ceil(double x);
double sqrt(double x);
double pow(double x, double y);
double sin(double x);
double cos(double x);
double tan(double x);
double atan(double x);
double atan2(double y, double x);
double exp(double x);
double log(double x);
double log10(double x);
double fmod(double x, double y);
double round(double x);
double trunc(double x);
#define M_PI 3.14159265358979323846
#define HUGE_VAL __builtin_huge_val()
#define INFINITY __builtin_inff()
#define NAN __builtin_nanf("")
#endif
