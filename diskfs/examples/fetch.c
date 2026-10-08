/* fetch HOST PORT PATH: a minimal HTTP client using BSD sockets.
 *     cc -run fetch.c 127.0.0.1 8080 /       (with `httpd -p 8080 &`)
 */
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: fetch HOST PORT PATH\n");
        return 2;
    }
    struct addrinfo hints = {0}, *ai;
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    int err = getaddrinfo(argv[1], argv[2], &hints, &ai);
    if (err) {
        fprintf(stderr, "fetch: %s: %s\n", argv[1], gai_strerror(err));
        return 1;
    }
    int s = socket(ai->ai_family, ai->ai_socktype, 0);
    if (s < 0 || connect(s, ai->ai_addr, ai->ai_addrlen) < 0) {
        perror("fetch: connect");
        return 1;
    }
    freeaddrinfo(ai);
    char req[512];
    int n = snprintf(req, sizeof req, "GET %s HTTP/1.0\r\nHost: %s\r\n\r\n", argv[3], argv[1]);
    send(s, req, n, 0);
    char buf[4096];
    int in_body = 0, len;
    char *body;
    while ((len = recv(s, buf, sizeof buf - 1, 0)) > 0) {
        buf[len] = 0;
        if (!in_body && (body = strstr(buf, "\r\n\r\n"))) {
            in_body = 1;
            fputs(body + 4, stdout);
        } else if (in_body) {
            fwrite(buf, 1, len, stdout);
        }
    }
    close(s);
    return 0;
}
