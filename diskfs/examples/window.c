/* A window from C (run it in the graphical session):
 *     cc -run /usr/share/huldra/examples/window.c
 * Click to drop circles, type to see key events, close the window to quit.
 */
#include <gui.h>
#include <stdio.h>

static void redraw(unsigned w, int width, int height, const char *status) {
    for (int y = 0; y < height; y += 4) {
        int shade = 40 + y * 80 / height;
        gui_fill(w, 0, y, width, 4, GUI_RGB(20, shade / 2, shade));
    }
    gui_text(w, 12, 12, 0xFFFFFF, 0, "Hello from C on the Huldra display server!");
    gui_text(w, 12, 36, 0xA0C8FF, 0, "Click anywhere; press keys.");
    gui_fill(w, 0, height - 24, width, 24, 0x202020);
    gui_text(w, 8, height - 20, 0xFFD27A, 0, status);
    gui_flush();
}

int main(void) {
    if (gui_connect() != 0) {
        fprintf(stderr, "window: no display (start one with startgui)\n");
        return 1;
    }
    int width = 420, height = 260;
    unsigned w = gui_window(160, 120, width, height, "C window");
    char status[128] = "ready";
    struct gui_event e;
    while (gui_next_event(&e, -1) > 0) {
        switch (e.type) {
        case GUI_EXPOSE:
            width = e.w;
            height = e.h;
            redraw(w, width, height, status);
            break;
        case GUI_BUTTON:
            if (e.pressed && e.button == GUI_LEFT) {
                gui_circle(w, e.x, e.y, 12, GUI_RGB(240, 160, 64));
                snprintf(status, sizeof status, "click at %d,%d", e.x, e.y);
                gui_fill(w, 0, height - 24, width, 24, 0x202020);
                gui_text(w, 8, height - 20, 0xFFD27A, 0, status);
                gui_flush();
            }
            break;
        case GUI_KEY:
            if (e.pressed) {
                snprintf(status, sizeof status, "key code %#x char '%c'", e.code, e.ch >= 32 && e.ch < 127 ? (int)e.ch : ' ');
                gui_fill(w, 0, height - 24, width, 24, 0x202020);
                gui_text(w, 8, height - 20, 0xFFD27A, 0, status);
                gui_flush();
            }
            break;
        case GUI_CLOSE:
            return 0;
        }
    }
    return 0;
}
