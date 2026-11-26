// featheraudio for Expo (Android): the JavaScript API (`FeatherAudio`), the C engine (through
// JNI, libfeatheraudio_jni.so) and its output through AudioTrack.
//
// The engine runs with manual output: a thread renders blocks with fa_audio_render and
// writes them to the AudioTrack. Audio focus is left to the app, so it can share the output
// with other audio modules.
package expo.modules.featheraudio

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.records.Field
import expo.modules.kotlin.records.Record
import java.io.File

class PlayOptions : Record {
  @Field val volume: Double = 1.0
  @Field val pan: Double = 0.0
  @Field val speed: Double = 1.0
  @Field val loop: Boolean = false
  @Field val paused: Boolean = false
  @Field val bus: Int = 0
  @Field val protect: Boolean = false
  @Field val delay: Double = 0.0
  @Field val position: List<Double>? = null
  @Field val velocity: List<Double>? = null
  @Field val minDistance: Double = 1.0
  @Field val maxDistance: Double = 1000.0
  @Field val attenuation: Int = 1
  @Field val rolloff: Double = 1.0
  @Field val doppler: Double = 1.0
}

class FeatherAudioException(message: String) : CodedException(message)

/** A handle or effect target (up to 0x8000000F for buses) as the C side's uint32. */
private fun u(x: Double): Int = x.toLong().toInt()

private fun f3(l: List<Double>?, d: FloatArray): FloatArray =
  if (l != null && l.size >= 3) floatArrayOf(l[0].toFloat(), l[1].toFloat(), l[2].toFloat()) else d

class FeatherAudioModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("FeatherAudio")

    Function("init") { Engine.start() }
    Function("shutdown") { Engine.shutdown() }
    Function("time") { Native.time(Engine.audio) }
    Function("sampleRate") { Native.sampleRate(Engine.audio) }

    Function("load") { path: String ->
      val s = Native.sourceLoad(read(path))
      if (s == 0L) throw FeatherAudioException(Native.lastError())
      Engine.addSource(s)
    }
    Function("stream") { path: String ->
      val s = Native.sourceStream(read(path))
      if (s == 0L) throw FeatherAudioException(Native.lastError())
      Engine.addSource(s)
    }
    Function("unload") { id: Int -> Engine.removeSource(id) }
    Function("duration") { id: Int -> Native.sourceDuration(Engine.source(id)) }

    Function("play") { src: Int, o: PlayOptions -> Engine.play(src, o, Double.NaN) }
    Function("playClocked") { t: Double, src: Int, o: PlayOptions -> Engine.play(src, o, t) }
    Function("stop") { h: Double -> Native.stop(Engine.audio, u(h)) }
    Function("stopAfter") { h: Double, s: Double -> Native.stopAfter(Engine.audio, u(h), s.toFloat()) }
    Function("setVolume") { h: Double, v: Double -> Native.setVolume(Engine.audio, u(h), v.toFloat()) }
    Function("fadeVolume") { h: Double, to: Double, s: Double -> Native.fadeVolume(Engine.audio, u(h), to.toFloat(), s.toFloat()) }
    Function("setPan") { h: Double, v: Double -> Native.setPan(Engine.audio, u(h), v.toFloat()) }
    Function("setSpeed") { h: Double, v: Double -> Native.setSpeed(Engine.audio, u(h), v.toFloat()) }
    Function("setPaused") { h: Double, v: Boolean -> Native.setPaused(Engine.audio, u(h), v) }
    Function("setLooping") { h: Double, v: Boolean -> Native.setLooping(Engine.audio, u(h), v) }
    Function("setProtected") { h: Double, v: Boolean -> Native.setProtected(Engine.audio, u(h), v) }
    Function("isPlaying") { h: Double -> Native.isPlaying(Engine.audio, u(h)) }
    Function("activeVoices") { Native.activeVoices(Engine.audio) }
    Function("setMaxVoices") { n: Int -> Native.setMaxVoices(Engine.audio, maxOf(1, n)) }
    Function("stopAll") { Native.stopAll(Engine.audio) }
    Function("setMasterVolume") { v: Double -> Native.setMasterVolume(Engine.audio, v.toFloat()) }

    Function("createBus") { Native.createBus(Engine.audio) }
    Function("setBusVolume") { b: Int, v: Double -> Native.setBusVolume(Engine.audio, b, v.toFloat()) }
    Function("fadeBusVolume") { b: Int, to: Double, s: Double -> Native.fadeBusVolume(Engine.audio, b, to.toFloat(), s.toFloat()) }
    Function("setBusPaused") { b: Int, v: Boolean -> Native.setBusPaused(Engine.audio, b, v) }
    Function("stopBus") { b: Int -> Native.stopBus(Engine.audio, b) }

    Function("setFilter") { t: Double, slot: Int, kind: Int, cutoff: Double, q: Double ->
      Native.setFilter(Engine.audio, u(t), slot, kind, cutoff.toFloat(), q.toFloat())
    }
    Function("setEcho") { t: Double, slot: Int, delay: Double, fb: Double, damp: Double, maxDelay: Double ->
      Native.setEcho(Engine.audio, u(t), slot, delay.toFloat(), fb.toFloat(), damp.toFloat(), maxDelay.toFloat())
    }
    Function("setReverb") { t: Double, slot: Int, room: Double, damp: Double, width: Double, dry: Double ->
      Native.setReverb(Engine.audio, u(t), slot, room.toFloat(), damp.toFloat(), width.toFloat(), dry.toFloat())
    }
    Function("removeEffect") { t: Double, slot: Int -> Native.removeEffect(Engine.audio, u(t), slot) }
    Function("setEffectParam") { t: Double, slot: Int, p: Int, v: Double ->
      Native.setEffectParam(Engine.audio, u(t), slot, p, v.toFloat())
    }
    Function("fadeEffectParam") { t: Double, slot: Int, p: Int, v: Double, s: Double ->
      Native.fadeEffectParam(Engine.audio, u(t), slot, p, v.toFloat(), s.toFloat())
    }

    Function("setListener") { p: List<Double>, f: List<Double>, up: List<Double> ->
      Native.setListener(Engine.audio, f3(p, floatArrayOf(0f, 0f, 0f)), f3(f, floatArrayOf(0f, 0f, -1f)), f3(up, floatArrayOf(0f, 1f, 0f)))
    }
    Function("setListenerVelocity") { v: List<Double> -> Native.setListenerVelocity(Engine.audio, f3(v, floatArrayOf(0f, 0f, 0f))) }
    Function("setSpeedOfSound") { s: Double -> Native.setSpeedOfSound(Engine.audio, s.toFloat()) }
    Function("setPosition") { h: Double, x: Double, y: Double, z: Double -> Native.setPosition(Engine.audio, u(h), x.toFloat(), y.toFloat(), z.toFloat()) }
    Function("setVelocity") { h: Double, x: Double, y: Double, z: Double -> Native.setVelocity(Engine.audio, u(h), x.toFloat(), y.toFloat(), z.toFloat()) }
  }

  private fun read(path: String): ByteArray =
    try { File(path.removePrefix("file://")).readBytes() } catch (e: Exception) { throw FeatherAudioException("cannot read $path") }
}

internal object Engine {
  @Volatile var audio = 0L
    private set
  private val sources = HashMap<Int, Long>()
  private var nextSource = 1
  private var track: AudioTrack? = null
  private var thread: Thread? = null
  @Volatile private var running = false

  @Synchronized fun start(): Boolean {
    if (audio != 0L) return true
    val rate = AudioTrack.getNativeOutputSampleRate(AudioManager.STREAM_MUSIC).takeIf { it > 0 } ?: 48000
    val a = Native.create(rate)
    if (a == 0L) return false
    val minBytes = AudioTrack.getMinBufferSize(rate, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_FLOAT)
    val frames = 256
    val t = AudioTrack.Builder()
      .setAudioAttributes(AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_GAME)
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .build())
      .setAudioFormat(AudioFormat.Builder()
        .setSampleRate(rate)
        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
        .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
        .build())
      // small buffer for low latency, but never below what the device needs
      .setBufferSizeInBytes(maxOf(minBytes, frames * 8 * 4))
      .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
      .setTransferMode(AudioTrack.MODE_STREAM)
      .build()
    t.play()
    audio = a
    track = t
    running = true
    thread = Thread({
      android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_AUDIO)
      val buf = FloatArray(frames * 2)
      while (running) {
        Native.render(a, buf, frames)
        t.write(buf, 0, frames * 2, AudioTrack.WRITE_BLOCKING)
      }
    }, "featheraudio").apply { start() }
    return true
  }

  @Synchronized fun shutdown() {
    running = false
    thread?.join(500)
    thread = null
    track?.let { try { it.stop() } catch (_: Exception) {}; it.release() }
    track = null
    sources.values.forEach { Native.sourceDestroy(it) }
    sources.clear()
    if (audio != 0L) Native.destroy(audio)
    audio = 0L
  }

  @Synchronized fun addSource(s: Long): Int {
    val id = nextSource++
    sources[id] = s
    return id
  }

  @Synchronized fun source(id: Int): Long = sources[id] ?: 0L

  @Synchronized fun removeSource(id: Int) {
    sources.remove(id)?.let { Native.sourceDestroy(it) }
  }

  fun play(src: Int, o: PlayOptions, clocked: Double): Int {
    val s = source(src)
    if (audio == 0L || s == 0L) return 0
    val pos = o.position
    val spatial = if (pos != null && pos.size >= 3) {
      floatArrayOf(
        pos[0].toFloat(), pos[1].toFloat(), pos[2].toFloat(),
        *f3(o.velocity, floatArrayOf(0f, 0f, 0f)),
        o.minDistance.toFloat(), o.maxDistance.toFloat(), o.rolloff.toFloat(), o.doppler.toFloat()
      )
    } else null
    return Native.play(
      audio, s, clocked, o.volume.toFloat(), o.pan.toFloat(), o.speed.toFloat(),
      o.loop, o.paused, o.protect, o.bus, o.delay.toFloat(), spatial, o.attenuation
    )
  }
}

/** The C API through JNI (android/src/main/cpp/featheraudio_jni.c). */
internal object Native {
  init { System.loadLibrary("featheraudio_jni") }

  @JvmStatic external fun create(rate: Int): Long
  @JvmStatic external fun destroy(a: Long)
  @JvmStatic external fun render(a: Long, out: FloatArray, frames: Int)
  @JvmStatic external fun time(a: Long): Double
  @JvmStatic external fun sampleRate(a: Long): Int
  @JvmStatic external fun lastError(): String

  @JvmStatic external fun sourceLoad(data: ByteArray): Long
  @JvmStatic external fun sourceStream(data: ByteArray): Long
  @JvmStatic external fun sourceDuration(s: Long): Double
  @JvmStatic external fun sourceDestroy(s: Long)

  /** clocked: NaN for a normal play. spatial: null, or x y z vx vy vz min max rolloff doppler */
  @JvmStatic external fun play(
    a: Long, s: Long, clocked: Double, volume: Float, pan: Float, speed: Float,
    loop: Boolean, paused: Boolean, protect: Boolean, bus: Int, delay: Float,
    spatial: FloatArray?, attenuation: Int
  ): Int

  @JvmStatic external fun stop(a: Long, h: Int)
  @JvmStatic external fun stopAfter(a: Long, h: Int, s: Float)
  @JvmStatic external fun setVolume(a: Long, h: Int, v: Float)
  @JvmStatic external fun fadeVolume(a: Long, h: Int, to: Float, s: Float)
  @JvmStatic external fun setPan(a: Long, h: Int, v: Float)
  @JvmStatic external fun setSpeed(a: Long, h: Int, v: Float)
  @JvmStatic external fun setPaused(a: Long, h: Int, v: Boolean)
  @JvmStatic external fun setLooping(a: Long, h: Int, v: Boolean)
  @JvmStatic external fun setProtected(a: Long, h: Int, v: Boolean)
  @JvmStatic external fun isPlaying(a: Long, h: Int): Boolean
  @JvmStatic external fun activeVoices(a: Long): Int
  @JvmStatic external fun setMaxVoices(a: Long, n: Int)
  @JvmStatic external fun stopAll(a: Long)
  @JvmStatic external fun setMasterVolume(a: Long, v: Float)

  @JvmStatic external fun createBus(a: Long): Int
  @JvmStatic external fun setBusVolume(a: Long, b: Int, v: Float)
  @JvmStatic external fun fadeBusVolume(a: Long, b: Int, to: Float, s: Float)
  @JvmStatic external fun setBusPaused(a: Long, b: Int, v: Boolean)
  @JvmStatic external fun stopBus(a: Long, b: Int)

  @JvmStatic external fun setFilter(a: Long, t: Int, slot: Int, kind: Int, cutoff: Float, q: Float)
  @JvmStatic external fun setEcho(a: Long, t: Int, slot: Int, delay: Float, fb: Float, damp: Float, maxDelay: Float)
  @JvmStatic external fun setReverb(a: Long, t: Int, slot: Int, room: Float, damp: Float, width: Float, dry: Float)
  @JvmStatic external fun removeEffect(a: Long, t: Int, slot: Int)
  @JvmStatic external fun setEffectParam(a: Long, t: Int, slot: Int, p: Int, v: Float)
  @JvmStatic external fun fadeEffectParam(a: Long, t: Int, slot: Int, p: Int, v: Float, s: Float)

  @JvmStatic external fun setListener(a: Long, p: FloatArray, f: FloatArray, u: FloatArray)
  @JvmStatic external fun setListenerVelocity(a: Long, v: FloatArray)
  @JvmStatic external fun setSpeedOfSound(a: Long, s: Float)
  @JvmStatic external fun setPosition(a: Long, h: Int, x: Float, y: Float, z: Float)
  @JvmStatic external fun setVelocity(a: Long, h: Int, x: Float, y: Float, z: Float)
}
