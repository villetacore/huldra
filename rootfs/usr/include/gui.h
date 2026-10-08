/* gui.h: windows for C programs on the Huldra display server.
 *
 *     gui_connect();
 *     unsigned w = gui_window(100, 100, 320, 200, "Hello");
 *     struct gui_event e;
 *     while (gui_next_event(&e, -1) > 0) {
 *         if (e.type == GUI_EXPOSE) {
 *             gui_fill(w, 0, 0, e.w, e.h, 0x203040);
 *             gui_text(w, 10, 10, 0xFFFFFF, 0, "Hello, world");
 *             gui_flush();
 *         }
 *         if (e.type == GUI_CLOSE) break;
 *     }
 *
 * Colors are 0xRRGGBB. Text uses the 8x16 VGA font. Link nothing: the
 * library is part of the C library (/usr/lib/hcc/libc.c).
 */
#ifndef _GUI_H
#define _GUI_H

#define GUI_FONT_W 8
#define GUI_FONT_H 16

enum {
    GUI_EXPOSE = 2,     /* w, h: redraw everything */
    GUI_CONFIGURE = 3,  /* x, y, w, h */
    GUI_KEY = 4,        /* code, pressed, mods, ch */
    GUI_BUTTON = 5,     /* x, y, button, pressed, mods */
    GUI_MOTION = 6,     /* x, y, buttons */
    GUI_FOCUS = 7,      /* pressed = focused */
    GUI_CLOSE = 9,      /* the user closed the window */
};

/* Buttons and modifiers. */
#define GUI_LEFT 1
#define GUI_RIGHT 2
#define GUI_MIDDLE 4
#define GUI_WHEEL_UP 8
#define GUI_WHEEL_DOWN 16
#define GUI_SHIFT 1
#define GUI_CTRL 2
#define GUI_ALT 4

/* Key codes for keys without a character. */
#define GUI_KEY_ESC 0x01
#define GUI_KEY_ENTER 0x1C
#define GUI_KEY_BACKSPACE 0x0E
#define GUI_KEY_TAB 0x0F
#define GUI_KEY_UP 0x148
#define GUI_KEY_DOWN 0x150
#define GUI_KEY_LEFT 0x14B
#define GUI_KEY_RIGHT 0x14D

struct gui_event {
    int type;
    unsigned window;
    int x, y, w, h;
    int code, pressed, mods, button, buttons;
    unsigned ch;
};

/* Connects to $DISPLAY (default :0); 0 on success. */
int gui_connect(void);
int gui_screen_width(void);
int gui_screen_height(void);
/* Creates and shows a window; returns its id. */
unsigned gui_window(int x, int y, int w, int h, const char *title);
void gui_title(unsigned win, const char *title);
void gui_destroy(unsigned win);
void gui_fill(unsigned win, int x, int y, int w, int h, unsigned color);
/* bg 0 (or GUI_TRANSPARENT) draws only the letters. */
void gui_text(unsigned win, int x, int y, unsigned fg, unsigned bg, const char *text);
void gui_line(unsigned win, int x0, int y0, int x1, int y1, unsigned color);
void gui_circle(unsigned win, int x, int y, int r, unsigned color);
/* w * h pixels, 0xRRGGBB each. */
void gui_image(unsigned win, int x, int y, int w, int h, const unsigned *pixels);
void gui_flush(void);
/* Waits up to timeout_ms (-1: forever): 1 with an event, 0 on timeout,
   -1 when the display went away. */
int gui_next_event(struct gui_event *e, int timeout_ms);
int gui_fd(void);

#define GUI_TRANSPARENT 0
#define GUI_RGB(r, g, b) (((unsigned)(r) << 16) | ((unsigned)(g) << 8) | (unsigned)(b))

#endif
