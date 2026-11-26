// JNI bridge between FeatherAudioModule.kt (object Native) and the featheraudio C API.
#include <jni.h>
#include <math.h>
#include <stdint.h>
#include "featheraudio.h"

#define FN(name) Java_expo_modules_featheraudio_Native_##name
#define A(a) ((const FaAudio *)(intptr_t)(a))
#define S(s) ((FaSource *)(intptr_t)(s))

JNIEXPORT jlong JNICALL FN(create)(JNIEnv *env, jclass c, jint rate) {
  (void)env; (void)c;
  return (jlong)(intptr_t)fa_audio_create_manual((uint32_t)rate);
}
JNIEXPORT void JNICALL FN(destroy)(JNIEnv *env, jclass c, jlong a) {
  (void)env; (void)c;
  fa_audio_destroy((FaAudio *)(intptr_t)a);
}
JNIEXPORT void JNICALL FN(render)(JNIEnv *env, jclass c, jlong a, jfloatArray out, jint frames) {
  (void)c;
  // critical: no copy, and the audio thread does not allocate
  float *buf = (float *)(*env)->GetPrimitiveArrayCritical(env, out, NULL);
  if (!buf) return;
  fa_audio_render(A(a), buf, (uint32_t)frames);
  (*env)->ReleasePrimitiveArrayCritical(env, out, buf, 0);
}
JNIEXPORT jdouble JNICALL FN(time)(JNIEnv *env, jclass c, jlong a) { (void)env; (void)c; return fa_time(A(a)); }
JNIEXPORT jint JNICALL FN(sampleRate)(JNIEnv *env, jclass c, jlong a) { (void)env; (void)c; return (jint)fa_audio_sample_rate(A(a)); }
JNIEXPORT jstring JNICALL FN(lastError)(JNIEnv *env, jclass c) { (void)c; return (*env)->NewStringUTF(env, fa_last_error()); }

static jlong source(JNIEnv *env, jbyteArray data, int stream) {
  jsize len = (*env)->GetArrayLength(env, data);
  jbyte *bytes = (*env)->GetByteArrayElements(env, data, NULL);
  if (!bytes) return 0;
  // the C API copies the data, so it can be released at once
  FaSource *s = stream ? fa_source_stream(bytes, (size_t)len) : fa_source_load(bytes, (size_t)len);
  (*env)->ReleaseByteArrayElements(env, data, bytes, JNI_ABORT);
  return (jlong)(intptr_t)s;
}
JNIEXPORT jlong JNICALL FN(sourceLoad)(JNIEnv *env, jclass c, jbyteArray d) { (void)c; return source(env, d, 0); }
JNIEXPORT jlong JNICALL FN(sourceStream)(JNIEnv *env, jclass c, jbyteArray d) { (void)c; return source(env, d, 1); }
JNIEXPORT jdouble JNICALL FN(sourceDuration)(JNIEnv *env, jclass c, jlong s) { (void)env; (void)c; return fa_source_duration(S(s)); }
JNIEXPORT void JNICALL FN(sourceDestroy)(JNIEnv *env, jclass c, jlong s) { (void)env; (void)c; fa_source_destroy(S(s)); }

JNIEXPORT jint JNICALL FN(play)(JNIEnv *env, jclass c, jlong a, jlong s, jdouble clocked, jfloat volume, jfloat pan,
                                jfloat speed, jboolean loop, jboolean paused, jboolean protect, jint bus, jfloat delay,
                                jfloatArray spatial, jint attenuation) {
  (void)c;
  FaPlayOptions o = fa_play_options_default();
  o.volume = volume; o.pan = pan; o.speed = speed;
  o.looping = loop; o.paused = paused; o.protect = protect;
  o.bus = bus < 0 ? 0u : (uint32_t)bus; o.delay = delay;
  FaSpatial sp;
  if (spatial && (*env)->GetArrayLength(env, spatial) >= 10) {
    float v[10];
    (*env)->GetFloatArrayRegion(env, spatial, 0, 10, v);
    sp = fa_spatial_default(v[0], v[1], v[2]);
    sp.velocity[0] = v[3]; sp.velocity[1] = v[4]; sp.velocity[2] = v[5];
    sp.min_distance = v[6]; sp.max_distance = v[7]; sp.rolloff = v[8]; sp.doppler = v[9];
    sp.attenuation = attenuation;
    o.spatial = &sp;
  }
  uint32_t h = isnan(clocked) ? fa_play(A(a), S(s), &o) : fa_play_clocked(A(a), clocked, S(s), &o);
  return (jint)h;
}

#define VOICE0(name, fn) \
  JNIEXPORT void JNICALL FN(name)(JNIEnv *env, jclass c, jlong a, jint h) { (void)env; (void)c; fn(A(a), (uint32_t)h); }
#define VOICE1(name, fn, T) \
  JNIEXPORT void JNICALL FN(name)(JNIEnv *env, jclass c, jlong a, jint h, T x) { (void)env; (void)c; fn(A(a), (uint32_t)h, x); }
#define VOICE2(name, fn) \
  JNIEXPORT void JNICALL FN(name)(JNIEnv *env, jclass c, jlong a, jint h, jfloat x, jfloat y) { (void)env; (void)c; fn(A(a), (uint32_t)h, x, y); }
#define VOICE3(name, fn) \
  JNIEXPORT void JNICALL FN(name)(JNIEnv *env, jclass c, jlong a, jint h, jfloat x, jfloat y, jfloat z) { (void)env; (void)c; fn(A(a), (uint32_t)h, x, y, z); }

VOICE0(stop, fa_stop)
VOICE1(stopAfter, fa_stop_after, jfloat)
VOICE1(setVolume, fa_set_volume, jfloat)
VOICE2(fadeVolume, fa_fade_volume)
VOICE1(setPan, fa_set_pan, jfloat)
VOICE1(setSpeed, fa_set_speed, jfloat)
VOICE1(setPaused, fa_set_paused, jboolean)
VOICE1(setLooping, fa_set_looping, jboolean)
VOICE1(setProtected, fa_set_protected, jboolean)
VOICE3(setPosition, fa_set_position)
VOICE3(setVelocity, fa_set_velocity)

JNIEXPORT jboolean JNICALL FN(isPlaying)(JNIEnv *env, jclass c, jlong a, jint h) { (void)env; (void)c; return fa_is_playing(A(a), (uint32_t)h) != 0; }
JNIEXPORT jint JNICALL FN(activeVoices)(JNIEnv *env, jclass c, jlong a) { (void)env; (void)c; return (jint)fa_active_voices(A(a)); }
JNIEXPORT void JNICALL FN(setMaxVoices)(JNIEnv *env, jclass c, jlong a, jint n) { (void)env; (void)c; fa_set_max_voices(A(a), (uint32_t)n); }
JNIEXPORT void JNICALL FN(stopAll)(JNIEnv *env, jclass c, jlong a) { (void)env; (void)c; fa_stop_all(A(a)); }
JNIEXPORT void JNICALL FN(setMasterVolume)(JNIEnv *env, jclass c, jlong a, jfloat v) { (void)env; (void)c; fa_set_master_volume(A(a), v); }

// buses use the same shapes as voices (a number and values)
JNIEXPORT jint JNICALL FN(createBus)(JNIEnv *env, jclass c, jlong a) { (void)env; (void)c; return fa_create_bus(A(a)); }
VOICE1(setBusVolume, fa_set_bus_volume, jfloat)
VOICE2(fadeBusVolume, fa_fade_bus_volume)
VOICE1(setBusPaused, fa_set_bus_paused, jboolean)
VOICE0(stopBus, fa_stop_bus)

JNIEXPORT void JNICALL FN(setFilter)(JNIEnv *env, jclass c, jlong a, jint t, jint slot, jint kind, jfloat cutoff, jfloat q) {
  (void)env; (void)c; fa_set_filter(A(a), (uint32_t)t, (uint32_t)slot, kind, cutoff, q);
}
JNIEXPORT void JNICALL FN(setEcho)(JNIEnv *env, jclass c, jlong a, jint t, jint slot, jfloat d, jfloat fb, jfloat damp, jfloat maxd) {
  (void)env; (void)c; fa_set_echo(A(a), (uint32_t)t, (uint32_t)slot, d, fb, damp, maxd);
}
JNIEXPORT void JNICALL FN(setReverb)(JNIEnv *env, jclass c, jlong a, jint t, jint slot, jfloat room, jfloat damp, jfloat width, jfloat dry) {
  (void)env; (void)c; fa_set_reverb(A(a), (uint32_t)t, (uint32_t)slot, room, damp, width, dry);
}
JNIEXPORT void JNICALL FN(removeEffect)(JNIEnv *env, jclass c, jlong a, jint t, jint slot) {
  (void)env; (void)c; fa_remove_effect(A(a), (uint32_t)t, (uint32_t)slot);
}
JNIEXPORT void JNICALL FN(setEffectParam)(JNIEnv *env, jclass c, jlong a, jint t, jint slot, jint p, jfloat v) {
  (void)env; (void)c; fa_set_effect_param(A(a), (uint32_t)t, (uint32_t)slot, (uint32_t)p, v);
}
JNIEXPORT void JNICALL FN(fadeEffectParam)(JNIEnv *env, jclass c, jlong a, jint t, jint slot, jint p, jfloat v, jfloat s) {
  (void)env; (void)c; fa_fade_effect_param(A(a), (uint32_t)t, (uint32_t)slot, (uint32_t)p, v, s);
}

static void vec3(JNIEnv *env, jfloatArray arr, float out[3]) {
  out[0] = out[1] = out[2] = 0;
  if (arr && (*env)->GetArrayLength(env, arr) >= 3) (*env)->GetFloatArrayRegion(env, arr, 0, 3, out);
}
JNIEXPORT void JNICALL FN(setListener)(JNIEnv *env, jclass c, jlong a, jfloatArray p, jfloatArray f, jfloatArray u) {
  (void)c;
  float pp[3], ff[3], uu[3];
  vec3(env, p, pp); vec3(env, f, ff); vec3(env, u, uu);
  fa_set_listener(A(a), pp, ff, uu);
}
JNIEXPORT void JNICALL FN(setListenerVelocity)(JNIEnv *env, jclass c, jlong a, jfloatArray v) {
  (void)c;
  float vv[3];
  vec3(env, v, vv);
  fa_set_listener_velocity(A(a), vv);
}
JNIEXPORT void JNICALL FN(setSpeedOfSound)(JNIEnv *env, jclass c, jlong a, jfloat s) { (void)env; (void)c; fa_set_speed_of_sound(A(a), s); }
