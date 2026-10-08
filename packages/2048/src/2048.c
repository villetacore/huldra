/* 2048: slide tiles with the arrow keys (or WASD); q quits. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

static int board[4][4];
static long score;
static struct termios saved;

static void restore(void) {
    tcsetattr(0, TCSANOW, &saved);
    printf("\x1b[?25h\n");
}

static void add_tile(void) {
    int free[16], n = 0;
    for (int i = 0; i < 16; i++)
        if (!board[i / 4][i % 4])
            free[n++] = i;
    if (n) {
        int c = free[rand() % n];
        board[c / 4][c % 4] = rand() % 10 ? 2 : 4;
    }
}

static const char *color(int v) {
    switch (v) {
    case 2: return "\x1b[47;30m";
    case 4: return "\x1b[43;30m";
    case 8: return "\x1b[41;97m";
    case 16: return "\x1b[45;97m";
    case 32: return "\x1b[44;97m";
    case 64: return "\x1b[46;30m";
    case 128: return "\x1b[42;30m";
    case 256: return "\x1b[101;97m";
    case 512: return "\x1b[105;97m";
    case 1024: return "\x1b[104;97m";
    default: return "\x1b[103;30m";
    }
}

static void draw(const char *msg) {
    printf("\x1b[H\x1b[2J2048  score %ld\r\n\r\n", score);
    for (int y = 0; y < 4; y++) {
        for (int x = 0; x < 4; x++) {
            int v = board[y][x];
            if (v)
                printf("%s%6d \x1b[0m", color(v), v);
            else
                printf("     . ");
        }
        printf("\r\n\r\n");
    }
    printf("arrows/WASD move, q quits  %s\r\n", msg);
    fflush(stdout);
}

/* Slides one row to the left; returns whether anything moved. */
static int slide(int *row) {
    int out[4] = {0}, n = 0, moved = 0, merged = 0;
    for (int i = 0; i < 4; i++) {
        if (!row[i])
            continue;
        if (n && out[n - 1] == row[i] && !merged) {
            out[n - 1] *= 2;
            score += out[n - 1];
            merged = 1;
        } else {
            out[n++] = row[i];
            merged = 0;
        }
    }
    for (int i = 0; i < 4; i++) {
        if (out[i] != row[i])
            moved = 1;
        row[i] = out[i];
    }
    return moved;
}

/* dir: 0 left, 1 right, 2 up, 3 down */
static int move(int dir) {
    int moved = 0;
    for (int k = 0; k < 4; k++) {
        int row[4];
        for (int i = 0; i < 4; i++) {
            int j = dir == 1 || dir == 3 ? 3 - i : i;
            row[i] = dir < 2 ? board[k][j] : board[j][k];
        }
        moved |= slide(row);
        for (int i = 0; i < 4; i++) {
            int j = dir == 1 || dir == 3 ? 3 - i : i;
            if (dir < 2)
                board[k][j] = row[i];
            else
                board[j][k] = row[i];
        }
    }
    return moved;
}

static int can_move(void) {
    for (int y = 0; y < 4; y++)
        for (int x = 0; x < 4; x++) {
            if (!board[y][x])
                return 1;
            if (x < 3 && board[y][x] == board[y][x + 1])
                return 1;
            if (y < 3 && board[y][x] == board[y + 1][x])
                return 1;
        }
    return 0;
}

int main(void) {
    srand((unsigned)time(NULL) ^ (unsigned)getpid());
    tcgetattr(0, &saved);
    struct termios raw = saved;
    raw.c_lflag &= ~(ICANON | ECHO);
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    tcsetattr(0, TCSANOW, &raw);
    atexit(restore);
    printf("\x1b[?25l");
    add_tile();
    add_tile();
    draw("");
    for (;;) {
        int c = getchar(), dir = -1;
        if (c == 'q' || c == EOF)
            break;
        if (c == 27 && getchar() == '[') {
            switch (getchar()) {
            case 'A': dir = 2; break;
            case 'B': dir = 3; break;
            case 'C': dir = 1; break;
            case 'D': dir = 0; break;
            }
        }
        switch (c) {
        case 'a': dir = 0; break;
        case 'd': dir = 1; break;
        case 'w': dir = 2; break;
        case 's': dir = 3; break;
        }
        if (dir >= 0 && move(dir))
            add_tile();
        if (!can_move()) {
            draw("game over!");
            break;
        }
        draw("");
    }
    return 0;
}
