// featheraudio in an AudioWorklet: the engine (the C API built as WebAssembly,
// featheraudio.wasm) runs on the browser's audio thread and renders every block there. The
// page talks to it through featheraudio.js, which sends batches of commands over the port.
//
// Ids: the page numbers sources, sounds (handles) and buses itself, so its calls return at
// once; this side maps them to the engine's. Every 50 ms it reports the engine clock, the
// number of voices and the sounds that have ended.

const OPT_SIZE = 36; // FaPlayOptions on wasm32
const SPATIAL_SIZE = 44; // FaSpatial on wasm32
const BUS_FLAG = 0x80000000;

class FeatherAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.x = null;
    this.queue = [];
    this.sources = new Map(); // page id -> FaSource*
    this.handles = new Map(); // page handle -> engine handle
    this.buses = new Map([[0, 0]]); // page bus -> engine bus
    this.frames = 0;
    this.port.onmessage = (e) => this.message(e.data);
  }

  message(m) {
    if (m.wasm) {
      WebAssembly.instantiate(m.wasm, {}).then(({ instance }) => {
        const x = (this.x = instance.exports);
        this.audio = x.fa_audio_create_manual(sampleRate);
        this.out = x.fa_alloc(128 * 2 * 4);
        this.outFrames = 128;
        this.opt = x.fa_alloc(OPT_SIZE);
        this.spatial = x.fa_alloc(SPATIAL_SIZE);
        this.vec = x.fa_alloc(36);
        this.port.postMessage({ ready: true, rate: sampleRate });
        for (const b of this.queue) this.run(b);
        this.queue = [];
      }, (err) => this.port.postMessage({ error: String(err) }));
    } else if (!this.x) {
      this.queue.push(m);
    } else {
      this.run(m);
    }
  }

  // ---- memory helpers (views are made fresh: the memory can grow) ----

  f32(ptr, n) {
    return new Float32Array(this.x.memory.buffer, ptr, n);
  }
  dv() {
    return new DataView(this.x.memory.buffer);
  }
  copyIn(bytes) {
    const p = this.x.fa_alloc(bytes.byteLength);
    new Uint8Array(this.x.memory.buffer, p, bytes.byteLength).set(new Uint8Array(bytes.buffer || bytes, bytes.byteOffset || 0, bytes.byteLength));
    return p;
  }
  lastError() {
    const p = this.x.fa_last_error();
    const m = new Uint8Array(this.x.memory.buffer);
    let s = "";
    for (let i = p; m[i] && i < p + 500; i++) s += String.fromCharCode(m[i]);
    return s;
  }
  target(t) {
    if (t >= BUS_FLAG) return (BUS_FLAG | (this.buses.get(t - BUS_FLAG) ?? 0)) >>> 0;
    return this.handles.get(t) ?? 0;
  }
  h(h) {
    return this.handles.get(h) ?? 0;
  }
  options(o) {
    const d = this.dv(),
      p = this.opt;
    d.setFloat32(p, o.volume ?? 1, true);
    d.setFloat32(p + 4, o.pan ?? 0, true);
    d.setFloat32(p + 8, o.speed ?? 1, true);
    d.setInt32(p + 12, o.loop ? 1 : 0, true);
    d.setInt32(p + 16, o.paused ? 1 : 0, true);
    d.setInt32(p + 20, o.protect ? 1 : 0, true);
    d.setUint32(p + 24, this.buses.get(o.bus ?? 0) ?? 0, true);
    d.setFloat32(p + 28, o.delay ?? 0, true);
    let sp = 0;
    if (o.position) {
      sp = this.spatial;
      const v = o.velocity || [0, 0, 0];
      [o.position[0], o.position[1], o.position[2], v[0], v[1], v[2]].forEach((x, i) => d.setFloat32(sp + i * 4, x, true));
      d.setFloat32(sp + 24, o.minDistance ?? 1, true);
      d.setFloat32(sp + 28, o.maxDistance ?? 1000, true);
      d.setInt32(sp + 32, o.attenuation ?? 1, true);
      d.setFloat32(sp + 36, o.rolloff ?? 1, true);
      d.setFloat32(sp + 40, o.doppler ?? 1, true);
    }
    d.setUint32(p + 32, sp, true);
    return p;
  }
  vec3(a, b, c) {
    const v = this.f32(this.vec, 9);
    v.set(a, 0);
    if (b) v.set(b, 3);
    if (c) v.set(c, 6);
    return this.vec;
  }

  // ---- commands ----

  run(batch) {
    const x = this.x,
      a = this.audio;
    for (const c of batch) {
      switch (c[0]) {
        case "load":
        case "stream": {
          const p = this.copyIn(c[2]);
          const s = c[0] === "load" ? x.fa_source_load(p, c[2].byteLength) : x.fa_source_stream(p, c[2].byteLength);
          x.fa_free(p, c[2].byteLength);
          if (s) this.sources.set(c[1], s);
          else this.port.postMessage({ sourceError: c[1], message: this.lastError() });
          break;
        }
        case "samples": {
          // [op, id, Float32Array interleaved, channels, rate]
          const bytes = c[2].length * 4;
          const p = x.fa_alloc(bytes);
          this.f32(p, c[2].length).set(c[2]);
          const s = x.fa_source_from_samples(p, c[2].length / c[3], c[3], c[4]);
          x.fa_free(p, bytes);
          if (s) this.sources.set(c[1], s);
          break;
        }
        case "unload": {
          const s = this.sources.get(c[1]);
          if (s) x.fa_source_destroy(s);
          this.sources.delete(c[1]);
          break;
        }
        case "play": {
          // [op, handle, source, options]
          const s = this.sources.get(c[2]);
          const h = s ? x.fa_play(a, s, this.options(c[3] || {})) : 0;
          this.handles.set(c[1], h);
          if (!h) this.port.postMessage({ ended: [c[1]] });
          break;
        }
        case "stop":
          x.fa_stop(a, this.h(c[1]));
          break;
        case "stopAfter":
          x.fa_stop_after(a, this.h(c[1]), c[2]);
          break;
        case "volume":
          x.fa_set_volume(a, this.h(c[1]), c[2]);
          break;
        case "fadeVolume":
          x.fa_fade_volume(a, this.h(c[1]), c[2], c[3]);
          break;
        case "pan":
          x.fa_set_pan(a, this.h(c[1]), c[2]);
          break;
        case "speed":
          x.fa_set_speed(a, this.h(c[1]), c[2]);
          break;
        case "paused":
          x.fa_set_paused(a, this.h(c[1]), c[2] ? 1 : 0);
          break;
        case "looping":
          x.fa_set_looping(a, this.h(c[1]), c[2] ? 1 : 0);
          break;
        case "protect":
          x.fa_set_protected(a, this.h(c[1]), c[2] ? 1 : 0);
          break;
        case "maxVoices":
          x.fa_set_max_voices(a, c[1]);
          break;
        case "limiter":
          x.fa_set_limiter(a, c[1] ? 1 : 0);
          break;
        case "stopAll":
          x.fa_stop_all(a);
          break;
        case "master":
          x.fa_set_master_volume(a, c[1]);
          break;
        case "createBus": {
          const b = x.fa_create_bus(a);
          if (b >= 0) this.buses.set(c[1], b);
          break;
        }
        case "busVolume":
          x.fa_set_bus_volume(a, this.buses.get(c[1]) ?? 0, c[2]);
          break;
        case "fadeBusVolume":
          x.fa_fade_bus_volume(a, this.buses.get(c[1]) ?? 0, c[2], c[3]);
          break;
        case "busPaused":
          x.fa_set_bus_paused(a, this.buses.get(c[1]) ?? 0, c[2] ? 1 : 0);
          break;
        case "stopBus":
          x.fa_stop_bus(a, this.buses.get(c[1]) ?? 0);
          break;
        case "filter":
          x.fa_set_filter(a, this.target(c[1]), c[2], c[3], c[4], c[5]);
          break;
        case "echo":
          x.fa_set_echo(a, this.target(c[1]), c[2], c[3], c[4], c[5], c[6]);
          break;
        case "reverb":
          x.fa_set_reverb(a, this.target(c[1]), c[2], c[3], c[4], c[5], c[6]);
          break;
        case "removeEffect":
          x.fa_remove_effect(a, this.target(c[1]), c[2]);
          break;
        case "effectParam":
          x.fa_set_effect_param(a, this.target(c[1]), c[2], c[3], c[4]);
          break;
        case "fadeEffectParam":
          x.fa_fade_effect_param(a, this.target(c[1]), c[2], c[3], c[4], c[5]);
          break;
        case "listener": {
          const p = this.vec3(c[1], c[2], c[3]);
          x.fa_set_listener(a, p, p + 12, p + 24);
          break;
        }
        case "listenerVelocity":
          x.fa_set_listener_velocity(a, this.vec3(c[1]));
          break;
        case "speedOfSound":
          x.fa_set_speed_of_sound(a, c[1]);
          break;
        case "position":
          x.fa_set_position(a, this.h(c[1]), c[2], c[3], c[4]);
          break;
        case "velocity":
          x.fa_set_velocity(a, this.h(c[1]), c[2], c[3], c[4]);
          break;
      }
    }
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    if (!this.x || !out || out.length === 0) return true;
    const n = out[0].length;
    const x = this.x;
    if (n > this.outFrames) {
      x.fa_free(this.out, this.outFrames * 8);
      this.out = x.fa_alloc(n * 8);
      this.outFrames = n;
    }
    x.fa_audio_render(this.audio, this.out, n);
    const s = this.f32(this.out, n * 2);
    const l = out[0],
      r = out[1] || out[0];
    for (let i = 0; i < n; i++) {
      l[i] = s[i * 2];
      r[i] = s[i * 2 + 1];
    }
    // report every 50 ms: the clock, the voices, and which sounds have ended
    this.frames += n;
    if (this.frames >= sampleRate * 0.05) {
      this.frames = 0;
      const ended = [];
      for (const [ph, eh] of this.handles) {
        if (!x.fa_is_playing(this.audio, eh)) {
          ended.push(ph);
          this.handles.delete(ph);
        }
      }
      this.port.postMessage({ time: x.fa_time(this.audio), voices: x.fa_active_voices(this.audio), ended });
    }
    return true;
  }
}

registerProcessor("featheraudio", FeatherAudioProcessor);
