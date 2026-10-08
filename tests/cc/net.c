/* Sockets through glibc: a TCP echo server in a child process, a client,
   UDP, and a ping socket. Built with gcc -static and run on Huldra. */
#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static int failed;
/* Also compiled by hcc: see tests/cc. */
#define CHECK(c)                                          \
    do {                                                  \
        if (!(c)) {                                       \
            printf("FAIL line %d: %s\n", __LINE__, #c);   \
            failed++;                                     \
        }                                                 \
    } while (0)

int main(void) {
    struct sockaddr_in addr = {0};
    addr.sin_family = AF_INET;
    addr.sin_port = htons(7777);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

    int l = socket(AF_INET, SOCK_STREAM, 0);
    int one = 1;
    CHECK(setsockopt(l, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one) == 0);
    CHECK(bind(l, (struct sockaddr *)&addr, sizeof addr) == 0);
    CHECK(listen(l, 4) == 0);
    pid_t pid = fork();
    if (pid == 0) {
        int c = accept(l, NULL, NULL);
        char buf[256];
        ssize_t n;
        while ((n = read(c, buf, sizeof buf)) > 0)
            write(c, buf, n);
        close(c);
        _exit(0);
    }
    close(l);
    int s = socket(AF_INET, SOCK_STREAM, 0);
    CHECK(connect(s, (struct sockaddr *)&addr, sizeof addr) == 0);
    struct sockaddr_in peer;
    socklen_t plen = sizeof peer;
    CHECK(getpeername(s, (struct sockaddr *)&peer, &plen) == 0 && ntohs(peer.sin_port) == 7777);
    const char *msg = "echo through the kernel";
    CHECK(send(s, msg, strlen(msg), 0) == (ssize_t)strlen(msg));
    shutdown(s, SHUT_WR);
    char got[64] = {0};
    size_t total = 0;
    ssize_t n;
    while ((n = recv(s, got + total, sizeof got - 1 - total, 0)) > 0)
        total += n;
    CHECK(strcmp(got, msg) == 0);
    close(s);
    int status;
    waitpid(pid, &status, 0);
    CHECK(WIFEXITED(status));

    int u1 = socket(AF_INET, SOCK_DGRAM, 0), u2 = socket(AF_INET, SOCK_DGRAM, 0);
    addr.sin_port = htons(7778);
    CHECK(bind(u1, (struct sockaddr *)&addr, sizeof addr) == 0);
    CHECK(sendto(u2, "dgram", 5, 0, (struct sockaddr *)&addr, sizeof addr) == 5);
    struct sockaddr_in from;
    socklen_t flen = sizeof from;
    CHECK(recvfrom(u1, got, sizeof got, 0, (struct sockaddr *)&from, &flen) == 5 && memcmp(got, "dgram", 5) == 0);
    CHECK(from.sin_addr.s_addr == htonl(INADDR_LOOPBACK));

    addr.sin_port = htons(1);
    int r = socket(AF_INET, SOCK_STREAM, 0);
    CHECK(connect(r, (struct sockaddr *)&addr, sizeof addr) == -1);
    printf("net: %d failed (inet_ntoa %s)\n", failed, inet_ntoa(from.sin_addr));
    return failed != 0;
}
