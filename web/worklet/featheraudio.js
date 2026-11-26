// featheraudio for web pages and games: the engine runs in an AudioWorklet (see
// featheraudio-processor.js); this is the page side. The calls are the same as the Expo
// module's (play, setPosition, setReverb...): they return at once, and are sent to the audio
// thread in one batch per task.
//
//   <script src="featheraudio.js"></script>
//   const fa = await FeatherAudio.create(audioContext, { base: "featheraudio/" });
//   fa.node.connect(audioContext.destination);       // or into your own gain node
//   const pew = fa.fromAudioBuffer(buffer);          // or fa.load(arrayBufferOfAFile)
//   fa.play(pew, { position: [3, 0, -2] });          // 3D: x right, y up, -z ahead
//
// (Works as a classic script, setting globalThis.FeatherAudio, and as a CommonJS module.)
(function (root) {
  "use strict";
  const BUS_FLAG = 0x80000000;

  class FeatherAudio {
    /** Starts the engine in `context`. `base` is where featheraudio.wasm and
     *  featheraudio-processor.js are (or give `wasmUrl` / `processorUrl`). */
    static async create(context, opts = {}) {
      const base = opts.base ?? "";
      const wasmUrl = opts.wasmUrl ?? base + "featheraudio.wasm";
      const processorUrl = opts.processorUrl ?? base + "featheraudio-processor.js";
      const [bytes] = await Promise.all([fetch(wasmUrl).then((r) => {
        if (!r.ok) throw new Error(`featheraudio: ${wasmUrl}: ${r.status}`);
        return r.arrayBuffer();
      }), context.audioWorklet.addModule(processorUrl)]);
      const node = new AudioWorkletNode(context, "featheraudio", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2] });
      const fa = new FeatherAudio(context, node);
      await new Promise((resolve, reject) => {
        fa._ready = resolve;
        fa._fail = reject;
        node.port.postMessage({ wasm: bytes }, [bytes]);
      });
      return fa;
    }

    constructor(context, node) {
      this.context = context;
      /** the AudioWorkletNode: connect it where the engine's sound should go */
      this.node = node;
      this.MAIN_BUS = 0;
      this._batch = [];
      this._transfer = [];
      this._ids = 0;
      this._buses = 0;
      this._playing = new Set();
      this._time = 0;
      this._voices = 0;
      this.onSourceError = (id, message) => console.warn(`featheraudio: source ${id}: ${message}`);
      node.port.onmessage = (e) => {
        const m = e.data;
        if (m.ready) {
          this._rate = m.rate;
          this._ready && this._ready();
        } else if (m.error) {
          this._fail && this._fail(new Error(m.error));
        } else if (m.sourceError !== undefined) {
          this.onSourceError(m.sourceError, m.message);
        }
        if (m.time !== undefined) {
          this._time = m.time;
          this._voices = m.voices;
        }
        if (m.ended) for (const h of m.ended) this._playing.delete(h);
      };
    }

    _send(cmd, transfer) {
      if (this._batch.length === 0) queueMicrotask(() => this._flush());
      this._batch.push(cmd);
      if (transfer) this._transfer.push(transfer);
    }
    _flush() {
      const b = this._batch,
        t = this._transfer;
      this._batch = [];
      this._transfer = [];
      if (b.length) this.node.port.postMessage(b, t);
    }

    // ---- engine ----

    /** seconds of audio made so far (the engine's clock; updated every 50 ms) */
    time() {
      return this._time;
    }
    sampleRate() {
      return this._rate;
    }
    activeVoices() {
      return this._voices;
    }

    // ---- sources ----

    /** A whole file in memory (ArrayBuffer or typed array): WAV / OGG / MP3, or a tracker
     *  module (MOD / IT / S3M / XM). The bytes are handed over to the audio thread. */
    load(bytes) {
      return this._source("load", bytes);
    }
    /** OGG / MP3 decoded while playing (music). */
    stream(bytes) {
      return this._source("stream", bytes);
    }
    _source(op, bytes) {
      const id = ++this._ids;
      const copy = bytes instanceof ArrayBuffer ? bytes.slice(0) : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
      this._send([op, id, copy], copy);
      return id;
    }
    /** Your own samples: interleaved floats, 1 or 2 channels. */
    fromSamples(samples, channels, rate) {
      const id = ++this._ids;
      const copy = new Float32Array(samples);
      this._send(["samples", id, copy, channels, rate], copy.buffer);
      return id;
    }
    /** A Web Audio AudioBuffer (from decodeAudioData or an OfflineAudioContext): its first
     *  two channels. */
    fromAudioBuffer(buffer) {
      const ch = Math.min(2, buffer.numberOfChannels),
        n = buffer.length;
      const s = new Float32Array(n * ch);
      for (let c = 0; c < ch; c++) {
        const d = buffer.getChannelData(c);
        for (let i = 0; i < n; i++) s[i * ch + c] = d[i];
      }
      const id = ++this._ids;
      this._send(["samples", id, s, ch, buffer.sampleRate], s.buffer);
      return id;
    }
    unload(source) {
      this._send(["unload", source]);
    }

    // ---- playing ----

    /** Plays a source; returns a handle. Options: volume, pan, speed, loop, paused, protect,
     *  bus, delay (s), and for 3D: position [x, y, z], velocity, minDistance, maxDistance,
     *  attenuation (0 none, 1 inverse, 2 linear, 3 exponential), rolloff, doppler. */
    play(source, options = {}) {
      const h = ++this._ids;
      this._playing.add(h);
      this._send(["play", h, source, options]);
      return h;
    }
    stop(h) {
      this._send(["stop", h]);
    }
    stopAfter(h, seconds) {
      this._send(["stopAfter", h, seconds]);
    }
    setVolume(h, v) {
      this._send(["volume", h, v]);
    }
    fadeVolume(h, to, seconds) {
      this._send(["fadeVolume", h, to, seconds]);
    }
    setPan(h, pan) {
      this._send(["pan", h, pan]);
    }
    setSpeed(h, speed) {
      this._send(["speed", h, speed]);
    }
    setPaused(h, paused) {
      this._send(["paused", h, paused]);
    }
    setLooping(h, loop) {
      this._send(["looping", h, loop]);
    }
    setProtected(h, protect) {
      this._send(["protect", h, protect]);
    }
    /** false once the sound has ended (known within about 50 ms) */
    isPlaying(h) {
      return this._playing.has(h);
    }
    setMaxVoices(n) {
      this._send(["maxVoices", n]);
    }
    /** the master limiter (on by default) */
    setLimiter(on) {
      this._send(["limiter", on]);
    }
    stopAll() {
      this._send(["stopAll"]);
    }
    setMasterVolume(v) {
      this._send(["master", v]);
    }

    // ---- buses ----

    /** A new bus (it flows into the main bus); returns its number. */
    createBus() {
      const b = ++this._buses;
      this._send(["createBus", b]);
      return b;
    }
    setBusVolume(b, v) {
      this._send(["busVolume", b, v]);
    }
    fadeBusVolume(b, to, seconds) {
      this._send(["fadeBusVolume", b, to, seconds]);
    }
    setBusPaused(b, paused) {
      this._send(["busPaused", b, paused]);
    }
    stopBus(b) {
      this._send(["stopBus", b]);
    }

    // ---- effects: on a sound (its handle) or a bus (bus(n)) ----

    bus(n) {
      return BUS_FLAG + n;
    }
    /** kind: 0 low-pass, 1 high-pass, 2 band-pass, 3 notch */
    setFilter(target, slot, kind, cutoff, resonance = 0.707) {
      this._send(["filter", target, slot, kind, cutoff, resonance]);
    }
    setEcho(target, slot, o) {
      this._send(["echo", target, slot, o.delay, o.feedback, o.damping ?? 0, o.maxDelay ?? Math.max(1, o.delay)]);
    }
    setReverb(target, slot, o = {}) {
      this._send(["reverb", target, slot, o.roomSize ?? 0.5, o.damping ?? 0.5, o.width ?? 1, o.dry ?? 1]);
    }
    removeEffect(target, slot) {
      this._send(["removeEffect", target, slot]);
    }
    setEffectParam(target, slot, param, value) {
      this._send(["effectParam", target, slot, param, value]);
    }
    fadeEffectParam(target, slot, param, value, seconds) {
      this._send(["fadeEffectParam", target, slot, param, value, seconds]);
    }

    // ---- 3D ----

    /** The listener: default at the origin, facing -Z, +Y up (+X is right). */
    setListener(position, forward = [0, 0, -1], up = [0, 1, 0]) {
      this._send(["listener", position, forward, up]);
    }
    setListenerVelocity(v) {
      this._send(["listenerVelocity", v]);
    }
    setSpeedOfSound(speed) {
      this._send(["speedOfSound", speed]);
    }
    setPosition(h, p) {
      this._send(["position", h, p[0], p[1], p[2]]);
    }
    setVelocity(h, v) {
      this._send(["velocity", h, v[0], v[1], v[2]]);
    }
  }

  /** effect parameters: 0 is WET on every effect */
  FeatherAudio.Param = {
    WET: 0,
    FILTER_CUTOFF: 1,
    FILTER_RESONANCE: 2,
    ECHO_DELAY: 1,
    ECHO_FEEDBACK: 2,
    ECHO_DAMPING: 3,
    REVERB_ROOM_SIZE: 1,
    REVERB_DAMPING: 2,
    REVERB_WIDTH: 3,
    REVERB_DRY: 4,
  };
  FeatherAudio.FilterKind = { LowPass: 0, HighPass: 1, BandPass: 2, Notch: 3 };

  if (typeof module === "object" && module.exports) module.exports = FeatherAudio;
  else root.FeatherAudio = FeatherAudio;
})(typeof globalThis !== "undefined" ? globalThis : this);
