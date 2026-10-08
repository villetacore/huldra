/* pthreads test, built with the host gcc -static. */
#include <pthread.h>
#include <stdio.h>
#include <unistd.h>

#define THREADS 8
#define ITERATIONS 10000

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static long counter = 0;

static void *worker(void *arg) {
    long id = (long)arg;
    for (int i = 0; i < ITERATIONS; i++) {
        pthread_mutex_lock(&lock);
        counter++;
        pthread_mutex_unlock(&lock);
    }
    return (void *)(id * 2);
}

int main(void) {
    pthread_t t[THREADS];
    for (long i = 0; i < THREADS; i++) pthread_create(&t[i], NULL, worker, (void *)i);
    long sum = 0;
    for (int i = 0; i < THREADS; i++) {
        void *ret;
        pthread_join(t[i], &ret);
        sum += (long)ret;
    }
    printf("threads: counter=%ld (expected %d), sum=%ld, pid=%d\n", counter, THREADS * ITERATIONS, sum, getpid());
    return counter != THREADS * ITERATIONS;
}
