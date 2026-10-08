/* Conway's Game of Life in the terminal. Usage: life [generations] */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define W 60
#define H 20

static char grid[H][W], next[H][W];

static int neighbours(int y, int x) {
    int n = 0;
    for (int dy = -1; dy <= 1; dy++)
        for (int dx = -1; dx <= 1; dx++)
            if ((dy || dx) && grid[(y + dy + H) % H][(x + dx + W) % W])
                n++;
    return n;
}

int main(int argc, char **argv) {
    int generations = argc > 1 ? atoi(argv[1]) : 50;
    /* A glider and an R-pentomino. */
    grid[1][2] = grid[2][3] = grid[3][1] = grid[3][2] = grid[3][3] = 1;
    grid[10][30] = grid[10][31] = grid[11][29] = grid[11][30] = grid[12][30] = 1;
    for (int g = 0; g < generations; g++) {
        printf("\x1b[H\x1b[2J");
        int alive = 0;
        for (int y = 0; y < H; y++) {
            for (int x = 0; x < W; x++) {
                putchar(grid[y][x] ? '#' : '.');
                alive += grid[y][x];
            }
            putchar('\n');
        }
        printf("generation %d, %d alive\n", g, alive);
        fflush(stdout);
        for (int y = 0; y < H; y++)
            for (int x = 0; x < W; x++) {
                int n = neighbours(y, x);
                next[y][x] = n == 3 || (n == 2 && grid[y][x]);
            }
        memcpy(grid, next, sizeof grid);
        usleep(100000);
    }
    return 0;
}
