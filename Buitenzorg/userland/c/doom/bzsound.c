/* Buitenzorg sound + music modules for doomgeneric (Gravicode Studios).
 *
 * doomgeneric's own modules need SDL_mixer; this file supplies
 * DG_sound_module and DG_music_module instead (built with FEATURE_SOUND):
 *   - sound effects: the WAD's DMX lumps (8-bit unsigned PCM, usually
 *     11025 Hz) mixed on up to 16 channels with DOOM's volume/separation;
 *   - music: the MUS or MIDI lumps (Freedoom ships MIDI) played by a small synthesizer (a timbre per
 *     General MIDI family, ADSR envelopes, a one-pole filter, noise drums on
 *     channel 15) -- not an OPL emulation, so it sounds like DOOM's tunes on
 *     a different instrument.
 * Both are mixed into one 22050 Hz stereo stream that goes to the kernel's
 * mixer through AUDIO_WRITE (62). Mixing happens when the engine calls
 * I_UpdateSound / the music Poll (once per frame): the stream is topped up to
 * ~120 ms ahead. No sound device (AUDIO_WRITE says NOSYS) = silent, as before.
 *
 * Our own code, not part of doomgeneric; it only uses the engine's headers. */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#include "bz.h"
#include "doomtype.h"
#include "i_sound.h"
#include "w_wad.h"
#include "z_zone.h"

#define RATE 22050
#define FORMAT ((uint64_t)RATE | (2ull << 20) | (16ull << 24))
#define AHEAD_MS 120
#define MAX_FRAMES 4096
#define SFX_CHANNELS 16
#define VOICES 32

/* engine globals the SDL modules normally define */
int use_libsamplerate = 0;
float libsamplerate_scale = 0.65f;
void I_InitTimidityConfig(void) {}

static int device_ok = -1; /* -1 unknown, 0 none, 1 streaming */
static int sfx_prefix = 1;
static int32_t mixbuf[MAX_FRAMES * 2];
static int16_t outbuf[MAX_FRAMES * 2];

/* ------------------------------------------------------------ sound effects */

typedef struct {
    const uint8_t *data;
    uint32_t len;
} sfx_data_t;

typedef struct {
    const uint8_t *data;
    uint32_t len;       /* samples */
    uint32_t pos;       /* 16.16 */
    uint32_t step;      /* 16.16 */
    int left, right;    /* 0..255 */
    sfxinfo_t *sfx;
    int active;
} sfx_chan_t;

static sfx_chan_t chans[SFX_CHANNELS];

static void lump_name(sfxinfo_t *sfx, char *buf)
{
    if (sfx->link) sfx = sfx->link;
    if (sfx_prefix) snprintf(buf, 9, "ds%s", sfx->name);
    else snprintf(buf, 9, "%s", sfx->name);
}

static sfx_data_t *load_sfx(sfxinfo_t *sfx, uint32_t *rate)
{
    if (sfx->link) sfx = sfx->link;
    if (sfx->lumpnum < 0) return NULL;
    sfx_data_t *d = sfx->driver_data;
    const uint8_t *lump = W_CacheLumpNum(sfx->lumpnum, PU_STATIC);
    uint32_t lumplen = W_LumpLength(sfx->lumpnum);
    if (lumplen < 8 || lump[0] != 3 || lump[1] != 0) return NULL;
    *rate = lump[2] | (lump[3] << 8);
    if (d) return d;
    uint32_t length = lump[4] | (lump[5] << 8) | (lump[6] << 16) | ((uint32_t)lump[7] << 24);
    if (length > lumplen - 8 || length <= 48) return NULL;
    d = malloc(sizeof *d);
    if (!d) return NULL;
    /* DMX skips the first and last 16 bytes of the sample data */
    d->data = lump + 8 + 16;
    d->len = length - 32;
    sfx->driver_data = d;
    return d;
}

static void set_params(sfx_chan_t *c, int vol, int sep)
{
    int l = ((254 - sep) * vol) / 127, r = (sep * vol) / 127;
    c->left = l < 0 ? 0 : l > 255 ? 255 : l;
    c->right = r < 0 ? 0 : r > 255 ? 255 : r;
}

/* ------------------------------------------------------------------- music */

/* timbre per General MIDI family (program / 8): waveform, envelope (ms),
 * sustain (0..256), filter (0..256 = open) and level */
enum { W_SINE, W_PIANO, W_ORGAN, W_SAW, W_SQUARE, W_TRI, W_DIST, W_NOISE };
typedef struct {
    uint8_t wave;
    uint16_t attack, decay;
    uint16_t sustain;
    uint16_t release;
    uint16_t filter;
    uint16_t level;
} timbre_t;

static const timbre_t timbres[16] = {
    {W_PIANO, 4, 1400, 0, 200, 200, 230},    /* piano */
    {W_SINE, 2, 600, 0, 200, 256, 220},      /* chromatic percussion */
    {W_ORGAN, 10, 80, 230, 80, 180, 170},    /* organ */
    {W_TRI, 3, 900, 60, 150, 200, 230},      /* guitar (29/30: distortion, below) */
    {W_TRI, 4, 400, 150, 90, 150, 255},      /* bass */
    {W_SAW, 90, 300, 210, 250, 70, 150},     /* strings */
    {W_SAW, 120, 300, 210, 300, 70, 140},    /* ensemble */
    {W_SAW, 35, 200, 190, 120, 110, 160},    /* brass */
    {W_SQUARE, 25, 200, 190, 100, 90, 140},  /* reed */
    {W_SINE, 30, 200, 210, 120, 256, 200},   /* pipe */
    {W_SQUARE, 5, 200, 200, 100, 120, 130},  /* synth lead */
    {W_SAW, 250, 400, 200, 500, 60, 140},    /* synth pad */
    {W_SINE, 40, 600, 120, 300, 256, 170},   /* synth effects */
    {W_TRI, 3, 700, 0, 150, 220, 220},       /* ethnic */
    {W_SINE, 2, 300, 0, 100, 256, 230},      /* percussive */
    {W_NOISE, 5, 400, 0, 100, 120, 120},     /* sound effects */
};
static const timbre_t distortion = {W_DIST, 3, 500, 200, 120, 90, 150};

typedef struct {
    int active;
    int chan, note;
    const timbre_t *t;
    uint32_t phase, inc;
    int32_t env;        /* 0 .. 1 << 24 */
    int stage;          /* 0 attack, 1 decay, 2 sustain, 3 release */
    int32_t a_rate, d_rate, r_rate, sus;
    int amp;            /* 0..127 */
    int32_t lp;
    /* drums */
    int drum;
    uint32_t inc_end;
    uint32_t noise;
    int32_t hp;
    uint32_t age;
} voice_t;

typedef struct {
    int instrument, volume, pan, bend, last_vel;
} mchan_t;

static voice_t voices[VOICES];
static mchan_t mchans[16];
static int16_t sine[1024];
static uint32_t note_inc[128];
static int32_t bend_mul[256]; /* 16.16 factor */
static const uint8_t *song;
static uint32_t song_len, song_start, song_pos;
static int song_playing, song_paused, song_looping;
static int music_vol = 100;
static uint32_t tick_acc, wait_ticks; /* samples * 140 accumulated */
static uint32_t voice_clock;

/* a MIDI song, flattened at registration: every channel event of every
 * track, sorted, with its time in microseconds (tempo map applied) */
typedef struct {
    uint64_t us;
    uint8_t status, a, b;
} mev_t;

typedef struct {
    const uint8_t *data;
    uint32_t len;
    mev_t *ev;      /* NULL = MUS */
    uint32_t nev;
} song_t;

static song_t *cur_midi;
static uint32_t midi_next;
static uint64_t midi_samples; /* samples played since the song started */

static void synth_init(void)
{
    static int done;
    if (done) return;
    done = 1;
    for (int i = 0; i < 1024; i++) sine[i] = (int16_t)(32767.0 * sin(2.0 * M_PI * i / 1024.0));
    for (int n = 0; n < 128; n++) {
        double f = 440.0 * pow(2.0, (n - 69) / 12.0);
        note_inc[n] = (uint32_t)(f * 4294967296.0 / RATE);
    }
    for (int b = 0; b < 256; b++) bend_mul[b] = (int32_t)(65536.0 * pow(2.0, (b - 128) / 64.0 / 6.0));
}

static int32_t rate_for(int ms) /* envelope steps of 1 << 24 per ms */
{
    if (ms < 1) ms = 1;
    return (int32_t)((1 << 24) / ((int64_t)ms * RATE / 1000 + 1)) + 1;
}

static void reset_channels(void)
{
    for (int i = 0; i < 16; i++) mchans[i] = (mchan_t){0, 100, 64, 128, 100};
}

static voice_t *new_voice(void)
{
    voice_t *best = &voices[0];
    for (int i = 0; i < VOICES; i++) {
        voice_t *v = &voices[i];
        if (!v->active) return v;
        /* steal a released voice first, else the oldest */
        int vr = v->stage == 3, br = best->stage == 3;
        if ((vr && !br) || (vr == br && v->age < best->age)) best = v;
    }
    return best;
}

static uint32_t chan_inc(int ch, int note)
{
    uint64_t inc = note_inc[note & 127];
    return (uint32_t)((inc * (uint32_t)bend_mul[mchans[ch].bend & 255]) >> 16);
}

static void note_on(int ch, int note, int vel)
{
    voice_t *v = new_voice();
    memset(v, 0, sizeof *v);
    v->active = 1;
    v->chan = ch;
    v->note = note;
    v->amp = vel;
    v->age = ++voice_clock;
    v->noise = 0x12345u + voice_clock * 2654435761u;
    if (ch == 15) {
        /* percussion: the note picks the drum */
        v->drum = note;
        static const timbre_t drum_t = {W_SINE, 1, 200, 0, 30, 256, 255};
        v->t = &drum_t;
        int decay = 150;
        uint32_t f0 = 0, f1 = 0;
        switch (note) {
        case 35: case 36: f0 = note_inc[43]; f1 = note_inc[28]; decay = 260; break;   /* kick */
        case 41: case 43: f0 = note_inc[50]; f1 = note_inc[43]; decay = 300; break;   /* low toms */
        case 45: case 47: f0 = note_inc[55]; f1 = note_inc[48]; decay = 280; break;
        case 48: case 50: f0 = note_inc[60]; f1 = note_inc[53]; decay = 260; break;   /* high toms */
        case 38: case 40: f0 = note_inc[54]; f1 = note_inc[50]; decay = 170; break;   /* snare */
        case 42: case 44: decay = 45; break;                                          /* closed hat */
        case 46: decay = 260; break;                                                  /* open hat */
        case 49: case 52: case 55: case 57: decay = 900; break;                       /* crash */
        case 51: case 53: case 59: decay = 500; break;                                /* ride */
        case 37: case 39: decay = 70; break;                                          /* stick, clap */
        default: decay = 120; break;
        }
        v->inc = f0;
        v->inc_end = f1;
        v->a_rate = 1 << 24;
        v->d_rate = rate_for(decay);
        v->r_rate = rate_for(decay);
        v->sus = 0;
        v->env = 1 << 24;
        v->stage = 1;
        return;
    }
    int prog = mchans[ch].instrument & 127;
    v->t = (prog == 29 || prog == 30) ? &distortion : &timbres[prog / 8];
    v->inc = chan_inc(ch, note);
    v->a_rate = rate_for(v->t->attack);
    v->d_rate = rate_for(v->t->decay);
    v->r_rate = rate_for(v->t->release);
    v->sus = (int32_t)v->t->sustain << 16;
}

static void note_off(int ch, int note)
{
    for (int i = 0; i < VOICES; i++) {
        voice_t *v = &voices[i];
        if (v->active && v->chan == ch && v->note == note && v->stage != 3 && ch != 15) v->stage = 3;
    }
}

static void all_off(int ch, int hard)
{
    for (int i = 0; i < VOICES; i++)
        if (voices[i].active && (ch < 0 || voices[i].chan == ch)) {
            if (hard) voices[i].active = 0;
            else voices[i].stage = 3;
        }
}

static inline int32_t wave(voice_t *v)
{
    uint32_t p = v->phase;
    switch (v->t->wave) {
    case W_SINE: return sine[p >> 22];
    case W_PIANO: return (sine[p >> 22] * 3 + sine[(p << 1) >> 22] + sine[(p * 3) >> 22] / 2) / 4;
    case W_ORGAN: return (sine[p >> 22] * 2 + sine[(p << 1) >> 22] + sine[(p << 2) >> 22] / 2) * 2 / 7;
    case W_SAW: return (int32_t)(p >> 16) - 32768;
    case W_SQUARE: return (p & 0x80000000u) ? 24000 : -24000;
    case W_TRI: {
        int32_t s = (int32_t)(p >> 15);
        return (s < 65536 ? s : 131071 - s) - 32768;
    }
    case W_DIST: { /* two detuned saws, driven into a clipper */
        int32_t s = ((int32_t)(p >> 16) - 32768) + ((int32_t)((p + (p >> 7)) >> 16) - 32768);
        s = s * 3 / 2;
        if (s > 30000) s = 30000 + (s - 30000) / 8;
        if (s < -30000) s = -30000 + (s + 30000) / 8;
        return s > 32767 ? 32767 : s < -32767 ? -32767 : s;
    }
    default: {
        v->noise = v->noise * 1664525u + 1013904223u;
        return (int32_t)(v->noise >> 16) - 32768;
    }
    }
}

static inline int32_t drum_sample(voice_t *v)
{
    v->noise = v->noise * 1664525u + 1013904223u;
    int32_t n = (int32_t)(v->noise >> 16) - 32768;
    int32_t hp = n - v->hp; /* bright noise for hats / cymbals */
    v->hp = n;
    int32_t tone = 0;
    if (v->inc) {
        tone = sine[v->phase >> 22];
        v->phase += v->inc;
        if (v->inc > v->inc_end) v->inc -= (v->inc - v->inc_end) / 600 + 1;
    }
    switch (v->drum) {
    case 35: case 36: case 41: case 43: case 45: case 47: case 48: case 50: return tone;
    case 38: case 40: return tone / 2 + n / 2;
    case 42: case 44: case 46: case 49: case 51: case 52: case 53: case 55: case 57: case 59:
        return hp / 3;
    default: return n / 2;
    }
}

static void mix_music(int32_t *out, int frames)
{
    for (int i = 0; i < VOICES; i++) {
        voice_t *v = &voices[i];
        if (!v->active) continue;
        mchan_t *c = &mchans[v->chan];
        int amp = v->amp * c->volume / 127 * music_vol / 127 * v->t->level / 256; /* 0..127 */
        int pan = c->pan; /* 0..127 */
        int lg = amp * (127 - pan) / 64, rg = amp * pan / 64;
        if (lg > 127) lg = 127;
        if (rg > 127) rg = 127;
        int filt = v->t->filter;
        if (v->chan != 15) v->inc = chan_inc(v->chan, v->note);
        for (int f = 0; f < frames; f++) {
            /* envelope */
            switch (v->stage) {
            case 0:
                v->env += v->a_rate;
                if (v->env >= 1 << 24) { v->env = 1 << 24; v->stage = 1; }
                break;
            case 1:
                v->env -= v->d_rate;
                if (v->env <= v->sus) { v->env = v->sus; v->stage = 2; }
                break;
            case 2:
                break;
            default:
                v->env -= v->r_rate;
                break;
            }
            if (v->env <= 0) { v->active = 0; break; }
            int32_t s;
            if (v->drum) s = drum_sample(v);
            else {
                s = wave(v);
                v->phase += v->inc;
            }
            v->lp += ((s - v->lp) * filt) >> 8;
            int32_t e = (int32_t)(((int64_t)v->lp * (v->env >> 8)) >> 16);
            out[f * 2] += e * lg >> 9;
            out[f * 2 + 1] += e * rg >> 9;
        }
    }
}

static uint32_t read_delay(void)
{
    uint32_t d = 0;
    while (song_pos < song_len) {
        uint8_t b = song[song_pos++];
        d = (d << 7) | (b & 127);
        if (!(b & 128)) break;
    }
    return d;
}

/* run MUS events until the next delay */
static void run_events(void)
{
    while (song_playing && wait_ticks == 0) {
        if (song_pos >= song_len) { song_playing = 0; break; }
        uint8_t ev = song[song_pos++];
        int ch = ev & 15, type = (ev >> 4) & 7, last = ev & 128;
        uint8_t a = song_pos < song_len ? song[song_pos] : 0;
        switch (type) {
        case 0: song_pos++; note_off(ch, a & 127); break;
        case 1:
            song_pos++;
            if (a & 128) mchans[ch].last_vel = song_pos < song_len ? song[song_pos++] & 127 : 100;
            note_on(ch, a & 127, mchans[ch].last_vel);
            break;
        case 2: song_pos++; mchans[ch].bend = a; break;
        case 3:
            song_pos++;
            if (a == 10) all_off(ch, 1);
            else if (a == 11) all_off(ch, 0);
            else if (a == 14) { mchans[ch].volume = 100; mchans[ch].pan = 64; mchans[ch].bend = 128; }
            break;
        case 4: {
            song_pos++;
            uint8_t val = song_pos < song_len ? song[song_pos++] & 127 : 0;
            if (a == 0) mchans[ch].instrument = val;
            else if (a == 3) mchans[ch].volume = val;
            else if (a == 4) mchans[ch].pan = val;
            break;
        }
        case 5: break; /* end of measure */
        case 6:        /* end of score */
            if (song_looping) {
                song_pos = song_start;
                all_off(-1, 0);
                continue;
            }
            song_playing = 0;
            all_off(-1, 0);
            return;
        default: break;
        }
        if (last) wait_ticks = read_delay();
    }
}

static void midi_event(const mev_t *e)
{
    int ch = e->status & 15, type = e->status >> 4;
    /* MIDI's percussion channel is 9; the synth's (MUS) is 15 */
    if (ch == 9) ch = 15;
    else if (ch == 15) ch = 9;
    switch (type) {
    case 8: note_off(ch, e->a); break;
    case 9:
        if (e->b) note_on(ch, e->a, e->b);
        else note_off(ch, e->a);
        break;
    case 11:
        if (e->a == 7) mchans[ch].volume = e->b;
        else if (e->a == 10) mchans[ch].pan = e->b;
        else if (e->a == 120) all_off(ch, 1);
        else if (e->a == 123) all_off(ch, 0);
        else if (e->a == 121) { mchans[ch].volume = 100; mchans[ch].pan = 64; mchans[ch].bend = 128; }
        break;
    case 12: mchans[ch].instrument = e->a; break;
    case 14: mchans[ch].bend = ((e->b << 7 | e->a) >> 6) & 255; break;
    default: break;
    }
}

static void render_midi(int32_t *out, int frames)
{
    song_t *s = cur_midi;
    while (frames > 0) {
        int n = frames;
        if (song_playing && !song_paused) {
            while (midi_next < s->nev && s->ev[midi_next].us * RATE / 1000000 <= midi_samples)
                midi_event(&s->ev[midi_next++]);
            if (midi_next >= s->nev) {
                all_off(-1, 0);
                if (song_looping && s->nev) {
                    midi_next = 0;
                    midi_samples = 0;
                    reset_channels();
                    continue;
                }
                song_playing = 0;
            } else {
                uint64_t at = s->ev[midi_next].us * RATE / 1000000;
                if (at - midi_samples < (uint64_t)n) n = (int)(at - midi_samples);
                if (n < 1) n = 1;
            }
        }
        mix_music(out, n);
        out += n * 2;
        frames -= n;
        if (song_playing && !song_paused) midi_samples += (uint64_t)n;
    }
}

static void render_music(int32_t *out, int frames)
{
    if (cur_midi) { render_midi(out, frames); return; }
    while (frames > 0) {
        if (song_playing && !song_paused) run_events();
        /* frames until the next MUS tick (140 per second) */
        int n = frames;
        if (song_playing && !song_paused) {
            uint32_t left = (RATE - tick_acc + 139) / 140;
            if (left == 0) left = 1;
            if ((uint32_t)n > left) n = (int)left;
        }
        mix_music(out, n);
        out += n * 2;
        frames -= n;
        if (song_playing && !song_paused) {
            tick_acc += (uint32_t)n * 140;
            while (tick_acc >= RATE) {
                tick_acc -= RATE;
                if (wait_ticks) wait_ticks--;
            }
        }
    }
}

/* ------------------------------------------------------------------- mixing */

static void pump(void)
{
    if (device_ok == 0) return;
    uint64_t queued = bz_syscall(BZ_AUDIO_WRITE, 0, 0, FORMAT);
    if (BZ_IS_ERROR(queued)) { device_ok = 0; return; }
    device_ok = 1;
    if (queued >= AHEAD_MS) return;
    int frames = (int)((AHEAD_MS + 30 - queued) * RATE / 1000);
    if (frames > MAX_FRAMES) frames = MAX_FRAMES;
    memset(mixbuf, 0, (size_t)frames * 2 * sizeof(int32_t));
    for (int i = 0; i < SFX_CHANNELS; i++) {
        sfx_chan_t *c = &chans[i];
        if (!c->active) continue;
        for (int f = 0; f < frames; f++) {
            uint32_t idx = c->pos >> 16;
            if (idx >= c->len) { c->active = 0; break; }
            int32_t s = ((int32_t)c->data[idx] - 128) << 8;
            mixbuf[f * 2] += s * c->left >> 9;
            mixbuf[f * 2 + 1] += s * c->right >> 9;
            c->pos += c->step;
        }
    }
    render_music(mixbuf, frames);
    for (int i = 0; i < frames * 2; i++) {
        int32_t s = mixbuf[i];
        outbuf[i] = (int16_t)(s > 32767 ? 32767 : s < -32768 ? -32768 : s);
    }
    bz_syscall(BZ_AUDIO_WRITE, (uint64_t)outbuf, (uint64_t)frames * 4, FORMAT);
}

/* ------------------------------------------------------------ sound module */

static boolean S_Init(boolean use_prefix)
{
    sfx_prefix = use_prefix;
    memset(chans, 0, sizeof chans);
    synth_init();
    uint64_t q = bz_syscall(BZ_AUDIO_WRITE, 0, 0, FORMAT);
    device_ok = BZ_IS_ERROR(q) ? 0 : 1;
    printf("bzsound: %s\n", device_ok ? "streaming 22050 Hz stereo through the kernel mixer" : "no sound device (AUDIO_WRITE unavailable)");
    return device_ok ? true : false;
}

static void S_Shutdown(void) { memset(chans, 0, sizeof chans); }

static int S_GetSfxLumpNum(sfxinfo_t *sfx)
{
    char name[9];
    lump_name(sfx, name);
    return W_CheckNumForName(name);
}

static void S_Update(void) { pump(); }

static void S_UpdateParams(int ch, int vol, int sep)
{
    if (ch >= 0 && ch < SFX_CHANNELS) set_params(&chans[ch], vol, sep);
}

static int S_Start(sfxinfo_t *sfx, int ch, int vol, int sep)
{
    if (ch < 0 || ch >= SFX_CHANNELS) return -1;
    uint32_t rate = 11025;
    sfx_data_t *d = load_sfx(sfx, &rate);
    if (!d) return -1;
    sfx_chan_t *c = &chans[ch];
    c->data = d->data;
    c->len = d->len;
    c->pos = 0;
    c->step = (uint32_t)(((uint64_t)(rate ? rate : 11025) << 16) / RATE);
    c->sfx = sfx;
    set_params(c, vol, sep);
    c->active = 1;
    return ch;
}

static void S_Stop(int ch)
{
    if (ch >= 0 && ch < SFX_CHANNELS) chans[ch].active = 0;
}

static boolean S_IsPlaying(int ch)
{
    return ch >= 0 && ch < SFX_CHANNELS && chans[ch].active;
}

static void S_Cache(sfxinfo_t *sounds, int n) { (void)sounds; (void)n; }

static snddevice_t devices[] = {
    SNDDEVICE_SB, SNDDEVICE_PAS, SNDDEVICE_GUS, SNDDEVICE_WAVEBLASTER,
    SNDDEVICE_SOUNDCANVAS, SNDDEVICE_GENMIDI, SNDDEVICE_AWE32, SNDDEVICE_ADLIB,
};

sound_module_t DG_sound_module = {
    devices, sizeof devices / sizeof devices[0],
    S_Init, S_Shutdown, S_GetSfxLumpNum, S_Update, S_UpdateParams,
    S_Start, S_Stop, S_IsPlaying, S_Cache,
};

/* ------------------------------------------------------------ music module */

static boolean M_Init(void) { synth_init(); reset_channels(); return true; }
static void M_Shutdown(void) { song_playing = 0; all_off(-1, 1); }
static void M_SetVolume(int v) { music_vol = v < 0 ? 0 : v > 127 ? 127 : v; }
static void M_Pause(void) { song_paused = 1; all_off(-1, 0); }
static void M_Resume(void) { song_paused = 0; }

static uint32_t be32(const uint8_t *p) { return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }

static uint32_t read_vlq(const uint8_t *d, uint32_t *p, uint32_t end)
{
    uint32_t v = 0;
    while (*p < end) {
        uint8_t b = d[(*p)++];
        v = v << 7 | (b & 127);
        if (!(b & 128)) break;
    }
    return v;
}

typedef struct { uint64_t tick; uint32_t seq; uint32_t tempo; uint8_t status, a, b; } raw_t;

static int raw_cmp(const void *x, const void *y)
{
    const raw_t *a = x, *b = y;
    if (a->tick != b->tick) return a->tick < b->tick ? -1 : 1;
    return a->seq < b->seq ? -1 : a->seq > b->seq;
}

/* Standard MIDI File (format 0/1) -> a sorted event list in microseconds */
static int parse_midi(song_t *s, const uint8_t *d, uint32_t len)
{
    if (len < 14 || be32(d + 4) < 6) return 0;
    uint32_t ntracks = d[10] << 8 | d[11], division = d[12] << 8 | d[13];
    if (division & 0x8000 || division == 0) return 0; /* SMPTE time: not supported */
    uint32_t cap = 1024, n = 0, seq = 0;
    raw_t *raw = malloc(cap * sizeof *raw);
    if (!raw) return 0;
    uint32_t pos = 8 + be32(d + 4);
    for (uint32_t t = 0; t < ntracks && pos + 8 <= len; t++) {
        uint32_t tlen = be32(d + pos + 4);
        uint32_t p = pos + 8, end = tlen > len - p ? len : p + tlen;
        int is_track = memcmp(d + pos, "MTrk", 4) == 0;
        pos = end;
        if (!is_track) continue;
        uint64_t tick = 0;
        uint8_t running = 0;
        while (p < end) {
            tick += read_vlq(d, &p, end);
            if (p >= end) break;
            uint8_t st = d[p];
            if (st & 0x80) p++;
            else st = running;
            raw_t r = {tick, seq++, 0, st, 0, 0};
            if (st == 0xFF) {
                if (p >= end) break;
                uint8_t type = d[p++];
                uint32_t l = read_vlq(d, &p, end);
                if (type == 0x51 && l == 3 && p + 3 <= end)
                    r.tempo = (uint32_t)d[p] << 16 | d[p + 1] << 8 | d[p + 2];
                p += l;
                if (type == 0x2F) break;
                if (!r.tempo) continue;
            } else if (st == 0xF0 || st == 0xF7) {
                p += read_vlq(d, &p, end);
                continue;
            } else if (st >= 0x80) {
                running = st;
                int two = (st >> 4) != 12 && (st >> 4) != 13;
                if (p + (two ? 2 : 1) > end) break;
                r.a = d[p++] & 127;
                if (two) r.b = d[p++] & 127;
                if ((st >> 4) == 13 || (st >> 4) == 10) continue; /* pressure: ignored */
            } else {
                break; /* a data byte without a running status */
            }
            if (n == cap) {
                raw_t *g = realloc(raw, (cap *= 2) * sizeof *raw);
                if (!g) { free(raw); return 0; }
                raw = g;
            }
            raw[n++] = r;
        }
    }
    qsort(raw, n, sizeof *raw, raw_cmp);
    s->ev = malloc((n ? n : 1) * sizeof *s->ev);
    if (!s->ev) { free(raw); return 0; }
    uint64_t us = 0, last_tick = 0;
    uint32_t tempo = 500000, m = 0;
    for (uint32_t i = 0; i < n; i++) {
        us += (raw[i].tick - last_tick) * tempo / division;
        last_tick = raw[i].tick;
        if (raw[i].tempo) { tempo = raw[i].tempo; continue; }
        s->ev[m++] = (mev_t){us, raw[i].status, raw[i].a, raw[i].b};
    }
    s->nev = m;
    free(raw);
    return 1;
}

static void *M_Register(void *data, int len)
{
    const uint8_t *d = data;
    song_t *s = calloc(1, sizeof *s);
    if (!s) return NULL;
    s->data = d;
    s->len = (uint32_t)len;
    if (len >= 16 && memcmp(d, "MUS\x1a", 4) == 0) return s;
    if (len >= 14 && memcmp(d, "MThd", 4) == 0 && parse_midi(s, d, (uint32_t)len)) {
        printf("bzsound: MIDI song, %u events\n", s->nev);
        return s;
    }
    printf("bzsound: unknown song format\n");
    free(s);
    return NULL;
}

static void M_Unregister(void *h)
{
    song_t *s = h;
    if (s && (song == s->data || cur_midi == s)) { song_playing = 0; song = NULL; cur_midi = NULL; all_off(-1, 1); }
    if (s) free(s->ev);
    free(s);
}

static void M_Play(void *h, boolean looping)
{
    song_t *s = h;
    all_off(-1, 1);
    song_playing = 0;
    cur_midi = NULL;
    if (!s) return;
    if (s->ev) {
        cur_midi = s;
        song = s->data;
        midi_next = 0;
        midi_samples = 0;
        song_looping = looping;
        song_paused = 0;
        reset_channels();
        song_playing = 1;
        return;
    }
    const uint8_t *d = s->data;
    uint32_t start = d[6] | (d[7] << 8);
    uint32_t score = d[4] | (d[5] << 8);
    if (start >= s->len) return;
    song = d;
    song_start = start;
    song_len = start + score <= s->len ? start + score : s->len;
    song_pos = start;
    song_looping = looping;
    song_paused = 0;
    tick_acc = 0;
    wait_ticks = 0;
    reset_channels();
    song_playing = 1;
}

static void M_Stop(void) { song_playing = 0; all_off(-1, 0); }
static boolean M_IsPlaying(void) { return song_playing; }
static void M_Poll(void) { pump(); }

static snddevice_t music_devices[] = {
    SNDDEVICE_PAS, SNDDEVICE_GUS, SNDDEVICE_WAVEBLASTER, SNDDEVICE_SOUNDCANVAS,
    SNDDEVICE_GENMIDI, SNDDEVICE_AWE32, SNDDEVICE_ADLIB, SNDDEVICE_SB,
};

music_module_t DG_music_module = {
    music_devices, sizeof music_devices / sizeof music_devices[0],
    M_Init, M_Shutdown, M_SetVolume, M_Pause, M_Resume, M_Register,
    M_Unregister, M_Play, M_Stop, M_IsPlaying, M_Poll,
};
