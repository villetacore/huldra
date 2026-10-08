/* sl: a steam locomotive runs across the terminal. */
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

static const char *engine[] = {
    "      ====        ________                ___________ ",
    "  _D _|  |_______/        \\__I_I_____===__|_________| ",
    "   |(_)---  |   H\\________/ |   |        =|___ ___|   ",
    "   /     |  |   H  |  |     |   |         ||_| |_||   ",
    "  |      |  |   H  |__--------------------| [___] |   ",
    "  | ________|___H__/__|_____/[][]~\\_______|       |   ",
    "  |/ |   |-----------I_____I [][] []  D   |=======|__ ",
};

static const char *wheels[2][3] = {
    {"__/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__ ",
     " |/-=|___|=    ||    ||    ||    |_____/~\\___/        ",
     "  \\_/      \\O=====O=====O=====O_/      \\_/            "},
    {"__/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__ ",
     " |/-=|___|=O=====O=====O=====O   |_____/~\\___/        ",
     "  \\_/      \\__/  \\__/  \\__/  \\__/      \\_/            "},
};

static void put_clipped(int row, int col, const char *s, int width) {
    int len = (int)strlen(s);
    for (int i = 0; i < len; i++) {
        int x = col + i;
        if (x >= 0 && x < width) {
            printf("\x1b[%d;%dH%c", row, x + 1, s[i]);
        }
    }
}

int main(void) {
    struct winsize ws;
    int width = 80, height = 25;
    if (ioctl(1, TIOCGWINSZ, &ws) == 0 && ws.ws_col) {
        width = ws.ws_col;
        height = ws.ws_row;
    }
    int top = height / 2 - 5;
    if (top < 1)
        top = 1;
    printf("\x1b[?25l");
    for (int x = width; x > -60; x -= 2) {
        printf("\x1b[2J");
        int smoke = (x / 2) % 2;
        put_clipped(top - 1, x + 6 + smoke, smoke ? "(  )   ( )" : " ( )  (  ) ", width);
        for (int i = 0; i < 7; i++)
            put_clipped(top + i, x, engine[i], width);
        for (int i = 0; i < 3; i++)
            put_clipped(top + 7 + i, x, wheels[(x / 2) & 1][i], width);
        fflush(stdout);
        usleep(40000);
    }
    printf("\x1b[2J\x1b[H\x1b[?25h");
    return 0;
}
