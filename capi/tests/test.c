/* C API test: cc -std=c99 -Wall -Wextra -Werror -Iinclude tests/test.c target/release/libfeatheraudio.a -lm -ldl -lpthread */
#include "featheraudio.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define RATE 48000
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "FAILED line %d: %s\n", __LINE__, #c); exit(1); } } while (0)

static float buf[RATE * 2];

static void render(FaAudio *a, float seconds) {
    CHECK(fa_audio_render(a, buf, (uint32_t)(seconds * RATE)));
}

static float last_l(float seconds) { return buf[(int)(seconds * RATE) * 2 - 2]; }
static float last_r(float seconds) { return buf[(int)(seconds * RATE) * 2 - 1]; }

static float rms(int frames) {
    double s = 0;
    for (int i = 0; i < frames * 2; i++) s += buf[i] * buf[i];
    return (float)sqrt(s / (frames * 2));
}

static void *read_file(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    *len = (size_t)ftell(f);
    fseek(f, 0, SEEK_SET);
    void *p = malloc(*len);
    CHECK(fread(p, 1, *len, f) == *len);
    fclose(f);
    return p;
}

int main(int argc, char **argv) {
    const char *data_dir = argc > 1 ? argv[1] : "../tests/data";
    FaAudio *a = fa_audio_create_manual(RATE);
    CHECK(a && fa_audio_sample_rate(a) == RATE);
    /* exact samples below: without the limiter's 1.5 ms look-ahead */
    fa_set_limiter(a, 0);

    /* a constant 0.5 for 0.1 s */
    float dc[4800];
    for (int i = 0; i < 4800; i++) dc[i] = 0.5f;
    FaSource *s = fa_source_from_samples(dc, 4800, 1, RATE);
    CHECK(s && fabs(fa_source_duration(s) - 0.1) < 1e-9);

    uint32_t h = fa_play(a, s, NULL);
    CHECK(h != 0 && fa_is_playing(a, h));
    render(a, 0.05f);
    CHECK(buf[0] == 0.5f && last_l(0.05f) == 0.5f);
    render(a, 0.1f);
    CHECK(last_l(0.1f) == 0.0f && !fa_is_playing(a, h) && fa_active_voices(a) == 0);

    /* options, buses, pan */
    int32_t bus = fa_create_bus(a);
    CHECK(bus == 1);
    FaPlayOptions o = fa_play_options_default();
    o.volume = 0.5f; o.pan = -1.0f; o.looping = 1; o.bus = (uint32_t)bus;
    h = fa_play(a, s, &o);
    render(a, 0.02f);
    CHECK(last_l(0.02f) == 0.25f && last_r(0.02f) == 0.0f);
    fa_set_bus_volume(a, (uint32_t)bus, 0.5f);
    fa_set_pan(a, h, 0.0f);
    render(a, 0.02f);
    CHECK(last_l(0.02f) == 0.125f && last_r(0.02f) == 0.125f);
    fa_stop(a, h);
    render(a, 0.02f);
    CHECK(!fa_is_playing(a, h));
    fa_set_bus_volume(a, (uint32_t)bus, 1.0f);

    /* 3D: 4 units in front = a quarter (inverse distance), then to the right */
    FaSpatial sp = fa_spatial_default(0, 0, -4);
    sp.doppler = 0;
    o = fa_play_options_default();
    o.looping = 1; o.spatial = &sp;
    h = fa_play(a, s, &o);
    render(a, 0.02f);
    CHECK(fabsf(last_l(0.02f) - 0.125f) < 1e-6f && last_l(0.02f) == last_r(0.02f));
    fa_set_position(a, h, 2, 0, 0);
    render(a, 0.02f);
    CHECK(fabsf(last_l(0.02f)) < 1e-6f && fabsf(last_r(0.02f) - 0.25f * sqrtf(2.0f)) < 1e-5f);
    float pos[3] = {0, 0, 0}, fwd[3] = {1, 0, 0}, up[3] = {0, 1, 0};
    fa_set_listener(a, pos, fwd, up); /* now it is in front */
    render(a, 0.02f);
    CHECK(fabsf(last_l(0.02f) - 0.25f) < 1e-5f && fabsf(last_r(0.02f) - 0.25f) < 1e-5f);
    fa_stop(a, h);
    render(a, 0.02f);

    /* effects: a low-pass on a bus removes an 8 kHz tone; WET 0 brings it back */
    float tone[RATE];
    for (int i = 0; i < RATE; i++) tone[i] = 0.5f * sinf(i * 8000.0f * 6.2831853f / RATE);
    FaSource *t = fa_source_from_samples(tone, RATE, 1, RATE);
    o = fa_play_options_default();
    o.looping = 1; o.bus = (uint32_t)bus;
    h = fa_play(a, t, &o);
    fa_set_filter(a, FA_BUS(bus), 0, FA_FILTER_LOW_PASS, 500.0f, 0.707f);
    render(a, 0.05f);
    render(a, 0.1f);
    CHECK(rms(4800) < 0.005f);
    fa_set_effect_param(a, FA_BUS(bus), 0, FA_WET, 0.0f);
    render(a, 0.1f);
    CHECK(fabsf(rms(4800) - 0.5f / sqrtf(2.0f)) < 0.01f);
    fa_remove_effect(a, FA_BUS(bus), 0);
    fa_stop_all(a);
    render(a, 0.05f);

    /* echo on the main bus: a click repeats 0.1 s later at half level */
    float click = 1.0f;
    FaSource *c = fa_source_from_samples(&click, 1, 1, RATE);
    fa_set_echo(a, FA_BUS(FA_BUS_MAIN), 0, 0.1f, 0.5f, 0.0f, 1.0f);
    fa_play(a, c, NULL);
    render(a, 0.25f);
    CHECK(buf[0] == 1.0f && buf[4800 * 2] == 0.5f && buf[9600 * 2] == 0.25f);
    fa_remove_effect(a, FA_BUS(FA_BUS_MAIN), 0);
    render(a, 0.05f);

    /* reverb: a click rings on */
    fa_set_reverb(a, FA_BUS(FA_BUS_MAIN), 0, 0.5f, 0.5f, 1.0f, 1.0f);
    fa_play(a, c, NULL);
    render(a, 0.3f);
    CHECK(rms(14400) > 1e-3f);
    fa_remove_effect(a, FA_BUS(FA_BUS_MAIN), 0);

    /* files: OGG and MP3 into memory and streamed */
    const char *names[] = {"tone.ogg", "tone.mp3"};
    for (int k = 0; k < 2; k++) {
        char path[512];
        snprintf(path, sizeof path, "%s/%s", data_dir, names[k]);
        size_t len;
        void *bytes = read_file(path, &len);
        CHECK(bytes);
        FaSource *w = fa_source_load(bytes, len);
        FaSource *st = fa_source_stream(bytes, len);
        free(bytes); /* the data was copied */
        CHECK(w && st && fa_source_duration(w) > 0.2 && fa_source_duration(st) < 0);
        fa_stop_all(a);
        render(a, 0.05f);
        fa_play(a, st, NULL);
        render(a, 0.1f);
        CHECK(rms(4800) > 0.05f);
        fa_source_destroy(w);
        fa_source_destroy(st); /* still playing: fine */
        render(a, 0.1f);
    }

    /* a tracker module (tests/data/song.mod, a 4-channel ProTracker module) */
    {
        char path[512];
        snprintf(path, sizeof path, "%s/song.mod", data_dir);
        size_t len;
        void *bytes = read_file(path, &len);
        CHECK(bytes);
        FaSource *t = fa_source_load(bytes, len);
        free(bytes);
        CHECK(t && fa_source_duration(t) < 0);
        fa_stop_all(a);
        render(a, 0.05f);
        uint32_t h = fa_play(a, t, NULL);
        render(a, 0.5f);
        CHECK(fa_is_playing(a, h) && rms(RATE / 2) > 0.05f);
        fa_stop_all(a);
        fa_source_destroy(t);
        render(a, 0.1f);
    }

    /* errors */
    CHECK(fa_source_load("nope", 4) == NULL && strlen(fa_last_error()) > 0);
    CHECK(fa_play(a, NULL, NULL) == 0);
    CHECK(fa_play(NULL, s, NULL) == 0);
    fa_stop(NULL, 1);

    /* clocked: two sounds 5 ms apart in one buffer start 240 frames apart */
    fa_stop_all(a);
    render(a, 0.05f);
    fa_play_clocked(a, 10.0, c, NULL);
    fa_play_clocked(a, 10.005, c, NULL);
    render(a, 0.01f);
    CHECK(buf[0] == 1.0f && buf[240 * 2] == 1.0f && buf[2] == 0.0f);
    CHECK(fa_time(a) > 1.0);

    fa_source_destroy(s);
    fa_source_destroy(t);
    fa_source_destroy(c);
    fa_audio_destroy(a);
    printf("C API: all checks passed\n");
    return 0;
}
