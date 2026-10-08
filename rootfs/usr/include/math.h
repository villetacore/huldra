#ifndef _MATH_H
#define _MATH_H
#define M_E 2.7182818284590452354
#define M_LOG2E 1.4426950408889634074
#define M_LOG10E 0.43429448190325182765
#define M_LN2 0.69314718055994530942
#define M_LN10 2.30258509299404568402
#define M_PI 3.14159265358979323846
#define M_PI_2 1.57079632679489661923
#define M_PI_4 0.78539816339744830962
#define M_1_PI 0.31830988618379067154
#define M_2_PI 0.63661977236758134308
#define M_SQRT2 1.41421356237309504880
#define M_SQRT1_2 0.70710678118654752440
#define INFINITY (1.0 / 0.0)
#define HUGE_VAL INFINITY
#define HUGE_VALF INFINITY
#define NAN (0.0 / 0.0)
#define isnan(x) ((x) != (x))
#define isinf(x) ((x) == INFINITY ? 1 : (x) == -INFINITY ? -1 : 0)
#define isfinite(x) (!isnan(x) && !isinf(x))
#define signbit(x) __signbit(x)
#define fpclassify(x) __fpclassify(x)
#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4
int __signbit(double x);
int __fpclassify(double x);
double sqrt(double x);
double fabs(double x);
double floor(double x);
double ceil(double x);
double round(double x);
double trunc(double x);
double rint(double x);
double nearbyint(double x);
long lround(double x);
long lrint(double x);
double fmod(double x, double y);
double remainder(double x, double y);
double modf(double x, double *ip);
double frexp(double x, int *e);
double ldexp(double x, int e);
double scalbn(double x, int e);
double exp(double x);
double exp2(double x);
double expm1(double x);
double log(double x);
double log2(double x);
double log10(double x);
double log1p(double x);
double pow(double x, double y);
double cbrt(double x);
double hypot(double x, double y);
double sin(double x);
double cos(double x);
double tan(double x);
double asin(double x);
double acos(double x);
double atan(double x);
double atan2(double y, double x);
double sinh(double x);
double cosh(double x);
double tanh(double x);
double fmin(double a, double b);
double fmax(double a, double b);
double copysign(double x, double y);
float sqrtf(float x);
float fabsf(float x);
float floorf(float x);
float ceilf(float x);
float roundf(float x);
float powf(float x, float y);
float expf(float x);
float logf(float x);
float sinf(float x);
float cosf(float x);
float tanf(float x);
float atan2f(float y, float x);
float fmodf(float x, float y);
#define sqrt(x) __builtin_sqrt(x)
#endif
