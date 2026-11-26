/*
 * featheraudio — C API
 * A feather-light audio engine for games: fire-and-forget sounds, buses, fades, effects,
 * simple 3D sound and tracker music (MOD / IT / S3M / XM).
 * https://github.com/mooyax/featheraudio
 *
 * Link with libfeatheraudio.a (or the shared library) built from capi/.
 * On Linux also link: -lm -ldl -lpthread.
 *
 * Threads: every function except fa_audio_render may be called from any thread.
 * fa_audio_render is called from one audio thread at a time.
 *
 * Numbers:
 *   - a playing sound is identified by a handle (uint32_t, never 0). Handles stay valid after
 *     the sound ends (calls on them then do nothing) and are never reused.
 *   - buses are numbers: 0 is the main bus, fa_create_bus returns 1..15.
 *   - effects go on a sound (pass its handle) or a bus (pass FA_BUS(n)).
 */
#ifndef FEATHERAUDIO_H
#define FEATHERAUDIO_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FaAudio FaAudio;
typedef struct FaSource FaSource;

/* ---------- 3D ---------- */

#define FA_ATTENUATION_NONE        0
#define FA_ATTENUATION_INVERSE     1 /* default: min / (min + rolloff * (d - min)) */
#define FA_ATTENUATION_LINEAR      2 /* to silence (rolloff 1) at max_distance */
#define FA_ATTENUATION_EXPONENTIAL 3 /* (d / min) ^ -rolloff */

typedef struct FaSpatial {
    float position[3];
    float velocity[3];    /* units per second, for the Doppler shift */
    float min_distance;   /* full volume up to here (default 1) */
    float max_distance;   /* no quieter beyond here (default 1000) */
    int32_t attenuation;  /* FA_ATTENUATION_* */
    float rolloff;        /* default 1 */
    float doppler;        /* 0 = off, 1 = real world (default) */
} FaSpatial;

/* ---------- play options ---------- */

typedef struct FaPlayOptions {
    float volume;               /* default 1 */
    float pan;                  /* -1 left .. 0 centre .. 1 right */
    float speed;                /* 1 = normal; also changes the pitch */
    int32_t looping;
    int32_t paused;             /* start paused */
    int32_t protect;            /* never replaced when all voices are in use (music) */
    uint32_t bus;               /* 0 = main */
    float delay;                /* seconds before it starts (sample-accurate) */
    const FaSpatial *spatial;   /* NULL = a normal 2D sound; copied by fa_play */
} FaPlayOptions;

/* Options with the defaults: volume 1, centred, speed 1, no loop, main bus, 2D. */
FaPlayOptions fa_play_options_default(void);
/* A 3D position with the defaults (inverse distance 1..1000, rolloff 1, Doppler 1). */
FaSpatial fa_spatial_default(float x, float y, float z);

/* Message of the last error on this thread ("" if none). */
const char *fa_last_error(void);

/* ---------- engine ---------- */

/* Opens the default output device (only when built with the "cpal" feature, desktop).
   NULL on failure (see fa_last_error). */
FaAudio *fa_audio_create(void);
/* No device: you call fa_audio_render from your own audio callback (mobile, web, offline). */
FaAudio *fa_audio_create_manual(uint32_t sample_rate);
void fa_audio_destroy(FaAudio *audio);
uint32_t fa_audio_sample_rate(const FaAudio *audio);
/* Fills `frames` interleaved stereo float frames (L R L R ..., 2 * frames floats).
   Manual engines only; returns 0 otherwise. Never blocks on the game thread and never
   allocates (except while streaming OGG / MP3). */
int32_t fa_audio_render(const FaAudio *audio, float *out, uint32_t frames);
/* Seconds of audio rendered so far (the engine's clock). */
double fa_time(const FaAudio *audio);

/* ---------- sources ---------- */

/* WAV / OGG / MP3 decoded into memory (sound effects), or a tracker module (MOD / IT / S3M /
   XM: like the others, played once to the end of the song unless played with looping). The
   data is copied. NULL if the format is unknown. */
FaSource *fa_source_load(const void *data, size_t len);
/* OGG / MP3 decoded while playing (music). The data is copied. */
FaSource *fa_source_stream(const void *data, size_t len);
/* Your own samples: `frames * channels` floats, 1 or 2 channels, interleaved. Copied. */
FaSource *fa_source_from_samples(const float *samples, uint32_t frames, uint32_t channels, uint32_t sample_rate);
/* Length in seconds, or -1 if not known (streams, modules). */
double fa_source_duration(const FaSource *source);
/* Sounds that are playing keep playing. */
void fa_source_destroy(FaSource *source);

/* ---------- playing ---------- */

/* Plays a sound and returns its handle (0 on bad arguments). `options` may be NULL. */
uint32_t fa_play(const FaAudio *audio, const FaSource *source, const FaPlayOptions *options);
/* Plays at a moment in game time (seconds): sounds started during one audio buffer are
   spread out by their time differences, so rapid fire stays even. */
uint32_t fa_play_clocked(const FaAudio *audio, double time, const FaSource *source, const FaPlayOptions *options);

void fa_stop(const FaAudio *audio, uint32_t handle);                  /* short fade-out */
void fa_stop_after(const FaAudio *audio, uint32_t handle, float seconds);
void fa_set_volume(const FaAudio *audio, uint32_t handle, float volume);
void fa_fade_volume(const FaAudio *audio, uint32_t handle, float to, float seconds);
void fa_set_pan(const FaAudio *audio, uint32_t handle, float pan);
void fa_set_speed(const FaAudio *audio, uint32_t handle, float speed);
void fa_set_paused(const FaAudio *audio, uint32_t handle, int32_t paused);
void fa_set_looping(const FaAudio *audio, uint32_t handle, int32_t looping);
void fa_set_protected(const FaAudio *audio, uint32_t handle, int32_t protect);
int32_t fa_is_playing(const FaAudio *audio, uint32_t handle);
uint32_t fa_active_voices(const FaAudio *audio);
/* default 32, at most 256; when full, the oldest unprotected sound makes room */
void fa_set_max_voices(const FaAudio *audio, uint32_t n);
/* the master limiter, on by default: many sounds together over full scale are turned down
   smoothly instead of crackling (the output is 1.5 ms later). 0: off, no delay, clipped */
void fa_set_limiter(const FaAudio *audio, int32_t on);
void fa_stop_all(const FaAudio *audio);
void fa_set_master_volume(const FaAudio *audio, float volume);

/* ---------- buses ---------- */

#define FA_BUS_MAIN 0u

/* A new bus (1..15), or -1 when all are in use. Buses flow into the main bus. */
int32_t fa_create_bus(const FaAudio *audio);
void fa_set_bus_volume(const FaAudio *audio, uint32_t bus, float volume);
void fa_fade_bus_volume(const FaAudio *audio, uint32_t bus, float to, float seconds);
void fa_set_bus_paused(const FaAudio *audio, uint32_t bus, int32_t paused);
void fa_stop_bus(const FaAudio *audio, uint32_t bus);

/* ---------- effects ---------- */

/* Effect targets: a sound's handle, or a bus. */
#define FA_BUS(n) (0x80000000u | (uint32_t)(n))
#define FA_MAX_EFFECTS 4

/* Parameter 0 of every effect: 0 = bypassed .. 1 = full effect (default). */
#define FA_WET 0
#define FA_FILTER_CUTOFF    1  /* Hz */
#define FA_FILTER_RESONANCE 2  /* Q, 0.707 = flat */
#define FA_ECHO_DELAY       1  /* seconds */
#define FA_ECHO_FEEDBACK    2  /* 0..1 */
#define FA_ECHO_DAMPING     3  /* 0..1 */
#define FA_REVERB_ROOM_SIZE 1  /* 0..1 */
#define FA_REVERB_DAMPING   2  /* 0..1 */
#define FA_REVERB_WIDTH     3  /* 0..1 */
#define FA_REVERB_DRY       4  /* 0..1 */

#define FA_FILTER_LOW_PASS  0
#define FA_FILTER_HIGH_PASS 1
#define FA_FILTER_BAND_PASS 2
#define FA_FILTER_NOTCH     3

/* Put an effect into a slot (0..3), replacing what was there. (Built-in effects need the
   "effects" feature, on by default.) */
void fa_set_filter(const FaAudio *audio, uint32_t target, uint32_t slot, int32_t kind, float cutoff, float resonance);
/* `max_delay`: the longest FA_ECHO_DELAY allowed later (memory); at least `delay`. */
void fa_set_echo(const FaAudio *audio, uint32_t target, uint32_t slot, float delay, float feedback, float damping, float max_delay);
/* Freeverb. Defaults: room 0.5, damping 0.5, width 1, dry 1 (0 on a reverb-only bus). */
void fa_set_reverb(const FaAudio *audio, uint32_t target, uint32_t slot, float room_size, float damping, float width, float dry);
void fa_remove_effect(const FaAudio *audio, uint32_t target, uint32_t slot);
void fa_set_effect_param(const FaAudio *audio, uint32_t target, uint32_t slot, uint32_t param, float value);
/* Fades from the value last set for this parameter (jumps if none was set). */
void fa_fade_effect_param(const FaAudio *audio, uint32_t target, uint32_t slot, uint32_t param, float value, float seconds);

/* ---------- 3D ---------- */

/* The listener (camera): position, facing direction and up, each float[3].
   Default: origin, facing -Z, +Y up (+X is right). */
void fa_set_listener(const FaAudio *audio, const float *position, const float *forward, const float *up);
void fa_set_listener_velocity(const FaAudio *audio, const float *velocity);
void fa_set_speed_of_sound(const FaAudio *audio, float speed); /* default 343 */
/* Makes a playing sound 3D, or changes all of its 3D settings. */
void fa_set_spatial(const FaAudio *audio, uint32_t handle, const FaSpatial *spatial);
void fa_set_position(const FaAudio *audio, uint32_t handle, float x, float y, float z);
void fa_set_velocity(const FaAudio *audio, uint32_t handle, float x, float y, float z);

#ifdef __cplusplus
}
#endif

#endif /* FEATHERAUDIO_H */
