/* Draws on the frame buffer directly: fbdemo [seconds]
 * (the display server does the same, with windows on top). */
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

#define FBIOGET_VSCREENINFO 0x4600

int main(int argc, char **argv) {
    int fd = open("/dev/fb0", O_RDWR);
    if (fd < 0) {
        perror("/dev/fb0");
        return 1;
    }
    unsigned info[40];
    ioctl(fd, FBIOGET_VSCREENINFO, info);
    unsigned w = info[0], h = info[1];
    unsigned *fb = mmap(NULL, w * h * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (fb == MAP_FAILED) {
        perror("mmap");
        return 1;
    }
    for (unsigned y = 0; y < h; y++)
        for (unsigned x = 0; x < w; x++)
            fb[y * w + x] = ((x * 255 / w) << 16) | ((y * 255 / h) << 8) | 0x60;
    for (unsigned i = 0; i < h && i < w; i++)
        fb[i * w + i] = 0xFFFFFF;
    sleep(argc > 1 ? atoi(argv[1]) : 3);
    printf("frame buffer %ux%u\n", w, h);
    return 0;
}
