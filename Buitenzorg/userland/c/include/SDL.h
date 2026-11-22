/* Buitenzorg's SDL2 subset (userland/c/src/sdl.c): one window, a streaming
 * texture shown with a BLIT, keyboard (and gamepad) events, timers. Enough
 * for programs written against SDL2's 2D renderer API, such as doomgeneric's
 * SDL backend. Values follow SDL2 so source code compiles unchanged. */
#ifndef BZ_SDL_H
#define BZ_SDL_H
#include <stdint.h>

typedef uint8_t Uint8;
typedef uint16_t Uint16;
typedef uint32_t Uint32;
typedef int32_t Sint32;
typedef uint64_t Uint64;
typedef int32_t SDL_Keycode;

typedef struct SDL_Window SDL_Window;
typedef struct SDL_Renderer SDL_Renderer;
typedef struct SDL_Texture SDL_Texture;
typedef struct SDL_Rect { int x, y, w, h; } SDL_Rect;

#define SDL_INIT_TIMER 0x01u
#define SDL_INIT_AUDIO 0x10u
#define SDL_INIT_VIDEO 0x20u
#define SDL_INIT_EVENTS 0x4000u
#define SDL_INIT_EVERYTHING 0xFFFFu
#define SDL_WINDOWPOS_UNDEFINED 0x1FFF0000
#define SDL_WINDOWPOS_CENTERED 0x2FFF0000
#define SDL_WINDOW_SHOWN 0x04u
#define SDL_WINDOW_RESIZABLE 0x20u
#define SDL_WINDOW_FULLSCREEN 0x01u
#define SDL_WINDOW_FULLSCREEN_DESKTOP 0x1001u
#define SDL_RENDERER_SOFTWARE 0x01u
#define SDL_RENDERER_ACCELERATED 0x02u
#define SDL_RENDERER_PRESENTVSYNC 0x04u
#define SDL_PIXELFORMAT_RGB888 0x16161804u
#define SDL_PIXELFORMAT_ARGB8888 0x16362004u
#define SDL_TEXTUREACCESS_STATIC 0
#define SDL_TEXTUREACCESS_STREAMING 1
#define SDL_TEXTUREACCESS_TARGET 2

enum { SDL_QUIT = 0x100, SDL_KEYDOWN = 0x300, SDL_KEYUP = 0x301 };

typedef struct SDL_Keysym { uint32_t scancode; SDL_Keycode sym; Uint16 mod; Uint32 unused; } SDL_Keysym;
typedef struct SDL_KeyboardEvent { Uint32 type, timestamp, windowID; Uint8 state, repeat, padding2, padding3; SDL_Keysym keysym; } SDL_KeyboardEvent;
typedef union SDL_Event { Uint32 type; SDL_KeyboardEvent key; Uint8 padding[56]; } SDL_Event;

#define SDLK_SCANCODE_MASK (1 << 30)
#define SDL_K(x) ((x) | SDLK_SCANCODE_MASK)
enum {
    SDLK_UNKNOWN = 0, SDLK_RETURN = '\r', SDLK_ESCAPE = 27, SDLK_BACKSPACE = 8, SDLK_TAB = '\t', SDLK_SPACE = ' ',
    SDLK_PLUS = '+', SDLK_COMMA = ',', SDLK_MINUS = '-', SDLK_PERIOD = '.', SDLK_SLASH = '/', SDLK_EQUALS = '=',
    SDLK_F1 = SDL_K(58), SDLK_F2 = SDL_K(59), SDLK_F3 = SDL_K(60), SDLK_F4 = SDL_K(61), SDLK_F5 = SDL_K(62), SDLK_F6 = SDL_K(63),
    SDLK_F7 = SDL_K(64), SDLK_F8 = SDL_K(65), SDLK_F9 = SDL_K(66), SDLK_F10 = SDL_K(67), SDLK_F11 = SDL_K(68), SDLK_F12 = SDL_K(69),
    SDLK_PAUSE = SDL_K(72), SDLK_RIGHT = SDL_K(79), SDLK_LEFT = SDL_K(80), SDLK_DOWN = SDL_K(81), SDLK_UP = SDL_K(82),
    SDLK_LCTRL = SDL_K(224), SDLK_LSHIFT = SDL_K(225), SDLK_LALT = SDL_K(226), SDLK_RCTRL = SDL_K(228), SDLK_RSHIFT = SDL_K(229), SDLK_RALT = SDL_K(230),
};

int SDL_Init(Uint32 flags);
void SDL_Quit(void);
const char *SDL_GetError(void);
SDL_Window *SDL_CreateWindow(const char *title, int x, int y, int w, int h, Uint32 flags);
void SDL_DestroyWindow(SDL_Window *w);
void SDL_SetWindowTitle(SDL_Window *w, const char *title);
SDL_Renderer *SDL_CreateRenderer(SDL_Window *w, int index, Uint32 flags);
void SDL_DestroyRenderer(SDL_Renderer *r);
int SDL_RenderClear(SDL_Renderer *r);
int SDL_RenderCopy(SDL_Renderer *r, SDL_Texture *t, const SDL_Rect *src, const SDL_Rect *dst);
void SDL_RenderPresent(SDL_Renderer *r);
int SDL_SetWindowFullscreen(SDL_Window *w, Uint32 flags);
void SDL_GetWindowSize(SDL_Window *w, int *width, int *height);
int SDL_RenderSetVSync(SDL_Renderer *r, int vsync);
SDL_Texture *SDL_CreateTexture(SDL_Renderer *r, Uint32 format, int access, int w, int h);
void SDL_DestroyTexture(SDL_Texture *t);
int SDL_UpdateTexture(SDL_Texture *t, const SDL_Rect *rect, const void *pixels, int pitch);
int SDL_PollEvent(SDL_Event *e);
Uint32 SDL_GetTicks(void);
Uint64 SDL_GetTicks64(void);
void SDL_Delay(Uint32 ms);
int SDL_ShowCursor(int toggle);

#endif
