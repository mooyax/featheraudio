// featheraudio for Expo (iOS): the JavaScript API (`FeatherAudio`), the C engine
// (featheraudio.h, in FeatherAudio.xcframework) and its output through AVAudioEngine.
//
// The engine runs with manual output: an AVAudioSourceNode asks it for every block of sound
// on the audio thread (fa_audio_render never locks against the game side and never
// allocates, except while streaming OGG / MP3).
// The audio session (AVAudioSession) is left to the app, so it can share it with other audio
// modules; set its category from the app if needed.
import AVFoundation
import ExpoModulesCore

struct FAPlayOptions: Record {
  @Field var volume: Double = 1
  @Field var pan: Double = 0
  @Field var speed: Double = 1
  @Field var loop: Bool = false
  @Field var paused: Bool = false
  @Field var bus: Int = 0
  @Field var protect: Bool = false
  @Field var delay: Double = 0
  @Field var position: [Double]? = nil
  @Field var velocity: [Double]? = nil
  @Field var minDistance: Double = 1
  @Field var maxDistance: Double = 1000
  @Field var attenuation: Int = 1
  @Field var rolloff: Double = 1
  @Field var doppler: Double = 1
}

private func v3(_ a: [Double]?) -> (Float, Float, Float) {
  guard let a = a, a.count >= 3 else { return (0, 0, 0) }
  return (Float(a[0]), Float(a[1]), Float(a[2]))
}

final class FAEngine {
  static let shared = FAEngine()

  private(set) var audio: OpaquePointer?
  private var engine: AVAudioEngine?
  private var node: AVAudioSourceNode?
  private var sources: [Int: OpaquePointer] = [:]
  private var nextSource = 1
  private let lock = NSLock()
  /// interleaved scratch for the render callback (never resized while running)
  private var scratch = [Float](repeating: 0, count: 8192 * 2)

  private init() {
    NotificationCenter.default.addObserver(self, selector: #selector(restart), name: .AVAudioEngineConfigurationChange, object: nil)
    NotificationCenter.default.addObserver(self, selector: #selector(restart), name: AVAudioSession.interruptionNotification, object: nil)
  }

  func start() -> Bool {
    if audio != nil { return true }
    let e = AVAudioEngine()
    let outFormat = e.outputNode.outputFormat(forBus: 0)
    let rate = outFormat.sampleRate > 0 ? outFormat.sampleRate : 48000
    guard let a = fa_audio_create_manual(UInt32(rate)) else { return false }
    let fmt = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2)!
    let n = AVAudioSourceNode(format: fmt) { [unowned self] _, _, frames, abl -> OSStatus in
      self.render(frames: Int(frames), abl: abl)
      return noErr
    }
    e.attach(n)
    e.connect(n, to: e.mainMixerNode, format: fmt)
    audio = a
    engine = e
    node = n
    e.prepare()
    do { try e.start() } catch { return false }
    return true
  }

  private func render(frames: Int, abl: UnsafeMutablePointer<AudioBufferList>) {
    let list = UnsafeMutableAudioBufferListPointer(abl)
    guard list.count >= 1, let l = list[0].mData?.assumingMemoryBound(to: Float.self) else { return }
    let r = list.count > 1 ? list[1].mData?.assumingMemoryBound(to: Float.self) ?? l : l
    var done = 0
    scratch.withUnsafeMutableBufferPointer { buf in
      let cap = buf.count / 2
      while done < frames {
        let n = min(cap, frames - done)
        fa_audio_render(audio, buf.baseAddress, UInt32(n))
        for i in 0..<n {
          l[done + i] = buf[i * 2]
          r[done + i] = buf[i * 2 + 1]
        }
        done += n
      }
    }
  }

  func shutdown() {
    engine?.stop()
    engine = nil
    node = nil
    lock.lock()
    for (_, s) in sources { fa_source_destroy(s) }
    sources.removeAll()
    lock.unlock()
    if let a = audio { fa_audio_destroy(a) }
    audio = nil
  }

  @objc private func restart() {
    DispatchQueue.main.async {
      if let e = self.engine, !e.isRunning {
        e.prepare()
        try? e.start()
      }
    }
  }

  // ---- sources ----

  func addSource(_ s: OpaquePointer) -> Int {
    lock.lock(); defer { lock.unlock() }
    let id = nextSource
    nextSource += 1
    sources[id] = s
    return id
  }

  func source(_ id: Int) -> OpaquePointer? {
    lock.lock(); defer { lock.unlock() }
    return sources[id]
  }

  func removeSource(_ id: Int) {
    lock.lock()
    let s = sources.removeValue(forKey: id)
    lock.unlock()
    if let s = s { fa_source_destroy(s) }
  }

  func play(_ src: Int, _ o: FAPlayOptions, clocked: Double?) -> Int {
    guard let a = audio, let s = source(src) else { return 0 }
    var opts = fa_play_options_default()
    opts.volume = Float(o.volume)
    opts.pan = Float(o.pan)
    opts.speed = Float(o.speed)
    opts.looping = o.loop ? 1 : 0
    opts.paused = o.paused ? 1 : 0
    opts.protect = o.protect ? 1 : 0
    opts.bus = UInt32(max(0, o.bus))
    opts.delay = Float(o.delay)
    let play = { (p: UnsafePointer<FaPlayOptions>) -> UInt32 in
      if let t = clocked { return fa_play_clocked(a, t, s, p) }
      return fa_play(a, s, p)
    }
    if let pos = o.position {
      let (x, y, z) = v3(pos)
      var sp = fa_spatial_default(x, y, z)
      let (vx, vy, vz) = v3(o.velocity)
      sp.velocity = (vx, vy, vz)
      sp.min_distance = Float(o.minDistance)
      sp.max_distance = Float(o.maxDistance)
      sp.attenuation = Int32(o.attenuation)
      sp.rolloff = Float(o.rolloff)
      sp.doppler = Float(o.doppler)
      return withUnsafePointer(to: &sp) { spp in
        opts.spatial = spp
        return Int(withUnsafePointer(to: &opts) { play($0) })
      }
    }
    return Int(withUnsafePointer(to: &opts) { play($0) })
  }
}

private func readFile(_ path: String) throws -> Data {
  guard let d = FileManager.default.contents(atPath: path) else {
    throw Exception(name: "FeatherAudioError", description: "cannot read \(path)")
  }
  return d
}

private func lastError() -> String {
  String(cString: fa_last_error())
}

public class FeatherAudioModule: Module {
  public func definition() -> ModuleDefinition {
    Name("FeatherAudio")

    let E = FAEngine.shared
    let h = { (x: Double) in UInt32(truncatingIfNeeded: Int64(x)) }

    Function("init") { () -> Bool in E.start() }
    Function("shutdown") { E.shutdown() }
    Function("time") { () -> Double in fa_time(E.audio) }
    Function("sampleRate") { () -> Int in Int(fa_audio_sample_rate(E.audio)) }

    Function("load") { (path: String) throws -> Int in
      let d = try readFile(path)
      let s = d.withUnsafeBytes { fa_source_load($0.baseAddress, d.count) }
      guard let s = s else { throw Exception(name: "FeatherAudioError", description: lastError()) }
      return E.addSource(s)
    }
    Function("stream") { (path: String) throws -> Int in
      let d = try readFile(path)
      let s = d.withUnsafeBytes { fa_source_stream($0.baseAddress, d.count) }
      guard let s = s else { throw Exception(name: "FeatherAudioError", description: lastError()) }
      return E.addSource(s)
    }
    Function("unload") { (id: Int) in E.removeSource(id) }
    Function("duration") { (id: Int) -> Double in fa_source_duration(E.source(id)) }

    Function("play") { (src: Int, o: FAPlayOptions) -> Int in E.play(src, o, clocked: nil) }
    Function("playClocked") { (t: Double, src: Int, o: FAPlayOptions) -> Int in E.play(src, o, clocked: t) }
    Function("stop") { (x: Double) in fa_stop(E.audio, h(x)) }
    Function("stopAfter") { (x: Double, s: Double) in fa_stop_after(E.audio, h(x), Float(s)) }
    Function("setVolume") { (x: Double, v: Double) in fa_set_volume(E.audio, h(x), Float(v)) }
    Function("fadeVolume") { (x: Double, to: Double, s: Double) in fa_fade_volume(E.audio, h(x), Float(to), Float(s)) }
    Function("setPan") { (x: Double, v: Double) in fa_set_pan(E.audio, h(x), Float(v)) }
    Function("setSpeed") { (x: Double, v: Double) in fa_set_speed(E.audio, h(x), Float(v)) }
    Function("setPaused") { (x: Double, v: Bool) in fa_set_paused(E.audio, h(x), v ? 1 : 0) }
    Function("setLooping") { (x: Double, v: Bool) in fa_set_looping(E.audio, h(x), v ? 1 : 0) }
    Function("setProtected") { (x: Double, v: Bool) in fa_set_protected(E.audio, h(x), v ? 1 : 0) }
    Function("isPlaying") { (x: Double) -> Bool in fa_is_playing(E.audio, h(x)) != 0 }
    Function("activeVoices") { () -> Int in Int(fa_active_voices(E.audio)) }
    Function("setMaxVoices") { (n: Int) in fa_set_max_voices(E.audio, UInt32(max(1, n))) }
    Function("stopAll") { fa_stop_all(E.audio) }
    Function("setMasterVolume") { (v: Double) in fa_set_master_volume(E.audio, Float(v)) }

    Function("createBus") { () -> Int in Int(fa_create_bus(E.audio)) }
    Function("setBusVolume") { (b: Int, v: Double) in fa_set_bus_volume(E.audio, UInt32(max(0, b)), Float(v)) }
    Function("fadeBusVolume") { (b: Int, to: Double, s: Double) in fa_fade_bus_volume(E.audio, UInt32(max(0, b)), Float(to), Float(s)) }
    Function("setBusPaused") { (b: Int, v: Bool) in fa_set_bus_paused(E.audio, UInt32(max(0, b)), v ? 1 : 0) }
    Function("stopBus") { (b: Int) in fa_stop_bus(E.audio, UInt32(max(0, b))) }

    Function("setFilter") { (t: Double, slot: Int, kind: Int, cutoff: Double, q: Double) in
      fa_set_filter(E.audio, h(t), UInt32(max(0, slot)), Int32(kind), Float(cutoff), Float(q))
    }
    Function("setEcho") { (t: Double, slot: Int, delay: Double, fb: Double, damp: Double, maxDelay: Double) in
      fa_set_echo(E.audio, h(t), UInt32(max(0, slot)), Float(delay), Float(fb), Float(damp), Float(maxDelay))
    }
    Function("setReverb") { (t: Double, slot: Int, room: Double, damp: Double, width: Double, dry: Double) in
      fa_set_reverb(E.audio, h(t), UInt32(max(0, slot)), Float(room), Float(damp), Float(width), Float(dry))
    }
    Function("removeEffect") { (t: Double, slot: Int) in fa_remove_effect(E.audio, h(t), UInt32(max(0, slot))) }
    Function("setEffectParam") { (t: Double, slot: Int, p: Int, v: Double) in
      fa_set_effect_param(E.audio, h(t), UInt32(max(0, slot)), UInt32(max(0, p)), Float(v))
    }
    Function("fadeEffectParam") { (t: Double, slot: Int, p: Int, v: Double, s: Double) in
      fa_fade_effect_param(E.audio, h(t), UInt32(max(0, slot)), UInt32(max(0, p)), Float(v), Float(s))
    }

    Function("setListener") { (p: [Double], f: [Double], u: [Double]) in
      var pp = [Float](p.prefix(3).map(Float.init)) + [0, 0, 0]
      var ff = [Float](f.prefix(3).map(Float.init)) + [0, 0, -1]
      var uu = [Float](u.prefix(3).map(Float.init)) + [0, 1, 0]
      fa_set_listener(E.audio, &pp, &ff, &uu)
    }
    Function("setListenerVelocity") { (v: [Double]) in
      var vv = [Float](v.prefix(3).map(Float.init)) + [0, 0, 0]
      fa_set_listener_velocity(E.audio, &vv)
    }
    Function("setSpeedOfSound") { (s: Double) in fa_set_speed_of_sound(E.audio, Float(s)) }
    Function("setPosition") { (x: Double, a: Double, b: Double, c: Double) in fa_set_position(E.audio, h(x), Float(a), Float(b), Float(c)) }
    Function("setVelocity") { (x: Double, a: Double, b: Double, c: Double) in fa_set_velocity(E.audio, h(x), Float(a), Float(b), Float(c)) }
  }
}
