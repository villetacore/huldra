/* snake: the classic game in a window. Arrows (or WASD) steer, Space
 * pauses, R restarts. Written in C against gui.h. */
#include <gui.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

#define COLS 28
#define ROWS 20
#define CELL 18
#define TOP 28

static int sx[COLS * ROWS], sy[COLS * ROWS];
static int len, dir, next_dir, food_x, food_y, score, best, dead, paused;
static unsigned win;

static void place_food(void) {
    for (;;) {
        food_x = rand() % COLS;
        food_y = rand() % ROWS;
        int clash = 0;
        for (int i = 0; i < len; i++)
            if (sx[i] == food_x && sy[i] == food_y)
                clash = 1;
        if (!clash)
            return;
    }
}

static void reset(void) {
    len = 4;
    for (int i = 0; i < len; i++) {
        sx[i] = COLS / 2 - i;
        sy[i] = ROWS / 2;
    }
    dir = next_dir = 0;
    score = 0;
    dead = paused = 0;
    place_food();
}

static void cell(int x, int y, unsigned color) {
    gui_fill(win, x * CELL + 1, TOP + y * CELL + 1, CELL - 2, CELL - 2, color);
}

static void draw_all(void) {
    char line[96];
    gui_fill(win, 0, 0, COLS * CELL, TOP, 0x1E2228);
    snprintf(line, sizeof line, "Score %d   Best %d   %s", score, best, dead ? "Game over - R to restart" : paused ? "Paused" : "");
    gui_text(win, 8, 6, 0xE8E8E8, 0, line);
    gui_fill(win, 0, TOP, COLS * CELL, ROWS * CELL, 0x2B3A1E);
    cell(food_x, food_y, 0xE53935);
    for (int i = len - 1; i >= 0; i--)
        cell(sx[i], sy[i], i == 0 ? 0xB5E655 : 0x7CB342);
    gui_flush();
}

static void step(void) {
    static const int dx[] = {1, 0, -1, 0}, dy[] = {0, 1, 0, -1};
    if (dead || paused)
        return;
    if ((next_dir + 2) % 4 != dir)
        dir = next_dir;
    int nx = sx[0] + dx[dir], ny = sy[0] + dy[dir];
    if (nx < 0 || ny < 0 || nx >= COLS || ny >= ROWS) {
        dead = 1;
    }
    for (int i = 0; i < len - 1 && !dead; i++)
        if (sx[i] == nx && sy[i] == ny)
            dead = 1;
    if (dead) {
        if (score > best)
            best = score;
        draw_all();
        return;
    }
    int ate = nx == food_x && ny == food_y;
    int tail_x = sx[len - 1], tail_y = sy[len - 1];
    for (int i = len - 1; i > 0; i--) {
        sx[i] = sx[i - 1];
        sy[i] = sy[i - 1];
    }
    sx[0] = nx;
    sy[0] = ny;
    if (ate) {
        sx[len] = tail_x;
        sy[len] = tail_y;
        len++;
        score += 10;
        place_food();
        draw_all();
        return;
    }
    /* Only the changed cells. */
    gui_fill(win, tail_x * CELL, TOP + tail_y * CELL, CELL, CELL, 0x2B3A1E);
    cell(sx[1], sy[1], 0x7CB342);
    cell(sx[0], sy[0], 0xB5E655);
    gui_flush();
}

int main(void) {
    if (gui_connect() != 0) {
        fprintf(stderr, "snake: no display (start one with startgui)\n");
        return 1;
    }
    srand((unsigned)time(NULL) ^ (unsigned)getpid());
    win = gui_window(0, 0, COLS * CELL, TOP + ROWS * CELL, "Snake");
    reset();
    struct gui_event e;
    clock_t last = clock();
    for (;;) {
        int r = gui_next_event(&e, 30);
        if (r < 0)
            return 0;
        if (r > 0) {
            if (e.type == GUI_EXPOSE)
                draw_all();
            if (e.type == GUI_CLOSE)
                return 0;
            if (e.type == GUI_KEY && e.pressed) {
                switch (e.code) {
                case GUI_KEY_RIGHT: next_dir = 0; break;
                case GUI_KEY_DOWN: next_dir = 1; break;
                case GUI_KEY_LEFT: next_dir = 2; break;
                case GUI_KEY_UP: next_dir = 3; break;
                }
                switch (e.ch) {
                case 'd': next_dir = 0; break;
                case 's': next_dir = 1; break;
                case 'a': next_dir = 2; break;
                case 'w': next_dir = 3; break;
                case ' ': paused = !paused; draw_all(); break;
                case 'r': reset(); draw_all(); break;
                }
            }
        }
        clock_t now = clock();
        long period = 120000 - (len > 30 ? 30 : len) * 2000; /* speeds up */
        if (now - last >= period) {
            last = now;
            step();
        }
    }
}
