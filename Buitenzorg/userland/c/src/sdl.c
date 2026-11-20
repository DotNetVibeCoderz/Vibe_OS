/* Buitenzorg's SDL2 subset: a window (WIN_CREATE with the client size), one
 * texture per renderer shown with a single BLIT draw op + WIN_PRESENT, key
 * events from KEY_EVENT (PC set-1 scancodes, `| 0x100` for E0 keys, gamepad
 * buttons from 0x200) translated to SDL2 keycodes, SDL_GetTicks from
 * CLOCK_NANOS, SDL_Delay from SLEEP_UNTIL.
 *
 * Self-tests: with `-bzframes N` on the command line the program ends after
 * N presented frames and reports what the last picture held. */
#include <bz.h>
#include <SDL.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct SDL_Window { uint64_t id; int w, h; };
struct SDL_Renderer { SDL_Window *win; SDL_Texture *last; };
struct SDL_Texture { int w, h; uint32_t *px; };

extern int bz_argc;
extern char **bz_argv;
static long frame_limit = -1, frames;
static uint64_t t0;

int SDL_Init(Uint32 flags)
{
    (void)flags;
    t0 = bz_nanos();
    for (int i = 1; i + 1 < bz_argc; i++)
        if (!strcmp(bz_argv[i], "-bzframes")) frame_limit = atol(bz_argv[i + 1]);
    bz_syscall(BZ_IDLE_INHIBIT, 1, 0, 0); /* a game: no screensaver */
    return 0;
}
void SDL_Quit(void) { bz_syscall(BZ_IDLE_INHIBIT, 0, 0, 0); }
const char *SDL_GetError(void) { return ""; }
int SDL_ShowCursor(int t) { (void)t; return 0; }

SDL_Window *SDL_CreateWindow(const char *title, int x, int y, int w, int h, Uint32 flags)
{
    (void)x; (void)y; (void)flags;
    if (!t0) SDL_Init(0);
    SDL_Window *win = calloc(1, sizeof *win);
    if (!win) return NULL;
    size_t n = strlen(title);
    if (n > 120) n = 120;
    win->id = bz_syscall(BZ_WIN_CREATE, (uint64_t)title, n ? n : 1, BZ_WIN_CLIENT_SIZE | (uint64_t)w << 32 | (uint64_t)h);
    win->w = w;
    win->h = h;
    if (win->id == 0) { free(win); return NULL; }
    return win;
}
void SDL_DestroyWindow(SDL_Window *w) { free(w); }
void SDL_SetWindowTitle(SDL_Window *w, const char *t) { (void)w; (void)t; }
SDL_Renderer *SDL_CreateRenderer(SDL_Window *w, int index, Uint32 flags)
{
    (void)index; (void)flags;
    SDL_Renderer *r = calloc(1, sizeof *r);
    if (r) r->win = w;
    return r;
}
void SDL_DestroyRenderer(SDL_Renderer *r) { free(r); }
int SDL_RenderClear(SDL_Renderer *r) { (void)r; return 0; }
SDL_Texture *SDL_CreateTexture(SDL_Renderer *r, Uint32 format, int access, int w, int h)
{
    (void)r; (void)format; (void)access;
    SDL_Texture *t = calloc(1, sizeof *t);
    if (!t) return NULL;
    t->w = w;
    t->h = h;
    t->px = calloc((size_t)w * h, 4);
    return t;
}
void SDL_DestroyTexture(SDL_Texture *t) { if (t) { free(t->px); free(t); } }
int SDL_UpdateTexture(SDL_Texture *t, const SDL_Rect *rect, const void *pixels, int pitch)
{
    int x0 = rect ? rect->x : 0, y0 = rect ? rect->y : 0, w = rect ? rect->w : t->w, h = rect ? rect->h : t->h;
    for (int y = 0; y < h; y++) memcpy(t->px + (size_t)(y0 + y) * t->w + x0, (const char *)pixels + (size_t)y * pitch, (size_t)w * 4);
    return 0;
}
int SDL_RenderCopy(SDL_Renderer *r, SDL_Texture *t, const SDL_Rect *src, const SDL_Rect *dst)
{
    (void)src; (void)dst;
    r->last = t;
    return 0;
}

static void report_and_exit(SDL_Texture *t)
{
    /* what the last picture held: distinct colours (sampled) and lit pixels */
    uint32_t seen[64];
    int distinct = 0, lit = 0, total = t->w * t->h;
    for (int i = 0; i < total; i += 37) {
        uint32_t c = t->px[i] & 0xFFFFFF;
        if (c > 0x101010) lit++;
        int k = 0;
        while (k < distinct && seen[k] != c) k++;
        if (k == distinct && distinct < 64) seen[distinct++] = c;
    }
    int samples = (total + 36) / 37;
    uint64_t ms = (bz_nanos() - t0) / 1000000;
    printf("[sdl] %ld frames in %llu ms; last frame: %d+ colours, %d%% lit\n", frames, (unsigned long long)ms, distinct, lit * 100 / (samples ? samples : 1));
    if (distinct >= 16 && lit * 4 > samples) printf("MILESTONE: SDL FRAMES OK (%ld frames, %d+ colours)\n", frames, distinct);
    exit(distinct >= 16 ? 0 : 1);
}

void SDL_RenderPresent(SDL_Renderer *r)
{
    SDL_Texture *t = r->last;
    if (!t || !r->win) return;
    struct bz_draw_cmd c = { .op = BZ_DRAW_BLIT, .x = 0, .y = 0, .w = t->w, .h = t->h, .text_ptr = (uint64_t)t->px, .text_len = (uint64_t)t->w * t->h * 4 };
    bz_syscall(BZ_WIN_CMD, r->win->id, (uint64_t)&c, 0);
    bz_syscall(BZ_WIN_PRESENT, r->win->id, 0, 0);
    frames++;
    if (frame_limit > 0 && frames >= frame_limit) report_and_exit(t);
}

/* ---- events ----------------------------------------------------------------- */

static SDL_Keycode keycode(uint32_t code)
{
    static const char row1[] = "qwertyuiop", row2[] = "asdfghjkl", row3[] = "zxcvbnm";
    if (code >= 0x10 && code <= 0x19) return row1[code - 0x10];
    if (code >= 0x1E && code <= 0x26) return row2[code - 0x1E];
    if (code >= 0x2C && code <= 0x32) return row3[code - 0x2C];
    if (code >= 0x02 && code <= 0x0A) return '1' + (int)(code - 0x02);
    if (code >= 0x3B && code <= 0x44) return SDLK_F1 + (int)(code - 0x3B);
    switch (code) {
    case 0x0B: return '0';
    case 0x01: return SDLK_ESCAPE;
    case 0x0C: return SDLK_MINUS;
    case 0x0D: return SDLK_EQUALS;
    case 0x0E: return SDLK_BACKSPACE;
    case 0x0F: return SDLK_TAB;
    case 0x1C: case 0x11C: return SDLK_RETURN;
    case 0x1D: return SDLK_LCTRL;
    case 0x11D: return SDLK_RCTRL;
    case 0x2A: return SDLK_LSHIFT;
    case 0x36: return SDLK_RSHIFT;
    case 0x38: return SDLK_LALT;
    case 0x138: return SDLK_RALT;
    case 0x39: return SDLK_SPACE;
    case 0x33: return SDLK_COMMA;
    case 0x34: return SDLK_PERIOD;
    case 0x35: return SDLK_SLASH;
    case 0x57: return SDLK_F11;
    case 0x58: return SDLK_F12;
    case 0x148: return SDLK_UP;
    case 0x150: return SDLK_DOWN;
    case 0x14B: return SDLK_LEFT;
    case 0x14D: return SDLK_RIGHT;
    /* gamepad (RawKey.PadA ..): A fire, B use, X / Y strafe, L / R weapons, Select menu, Start enter */
    case 0x200: return SDLK_LCTRL;
    case 0x201: return SDLK_SPACE;
    case 0x202: return SDLK_COMMA;
    case 0x203: return SDLK_PERIOD;
    case 0x204: return '1';
    case 0x205: return '2';
    case 0x206: return SDLK_ESCAPE;
    case 0x207: return SDLK_RETURN;
    case 0x210: return SDLK_UP;
    case 0x211: return SDLK_DOWN;
    case 0x212: return SDLK_LEFT;
    case 0x213: return SDLK_RIGHT;
    default: return SDLK_UNKNOWN;
    }
}

int SDL_PollEvent(SDL_Event *e)
{
    for (;;) {
        uint64_t v = bz_syscall(BZ_KEY_EVENT, 0, 0, 0);
        if (v == 0 || BZ_IS_ERROR(v)) return 0;
        uint32_t code = (uint32_t)(v & 0xFFFF);
        int pressed = (int)((v >> 16) & 1);
        SDL_Keycode k = keycode(code);
        if (k == SDLK_UNKNOWN) continue;
        memset(e, 0, sizeof *e);
        e->type = pressed ? SDL_KEYDOWN : SDL_KEYUP;
        e->key.state = (Uint8)pressed;
        e->key.keysym.scancode = code;
        e->key.keysym.sym = k;
        return 1;
    }
}

Uint64 SDL_GetTicks64(void) { if (!t0) t0 = bz_nanos(); return (bz_nanos() - t0) / 1000000; }
Uint32 SDL_GetTicks(void) { return (Uint32)SDL_GetTicks64(); }
void SDL_Delay(Uint32 ms) { bz_syscall(BZ_SLEEP_UNTIL, bz_nanos() + (uint64_t)ms * 1000000, 0, 0); }
