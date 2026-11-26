// featheraudio for Expo apps. Every call goes straight to the native engine and returns at
// once (the engine queues the command for its audio thread).
//
//   import * as FA from "feather-audio";
//   FA.init();
//   const pew = FA.load(asset.localUri!);          // WAV / OGG / MP3 / MOD / IT / S3M / XM
//   FA.play(pew);
//   const music = FA.createBus();
//   const bgm = FA.play(FA.stream(songUri), { bus: music, loop: true, protect: true });
//   FA.setReverb(FA.bus(FA.MAIN_BUS), 0, { roomSize: 0.7 });
import { requireNativeModule } from "expo-modules-core";

const N = requireNativeModule("FeatherAudio");

/** A loaded sound (from `load` / `stream`). */
export type SourceId = number;
/** A playing sound. Stays valid after it ends (calls then do nothing). */
export type Handle = number;
/** What an effect goes on: a sound's handle, or `bus(n)`. */
export type EffectTarget = number;
export type Vec3 = [number, number, number];

export const MAIN_BUS = 0;
export const MAX_EFFECTS = 4;

export enum Attenuation {
  None = 0,
  /** min / (min + rolloff * (d - min)): the default */
  Inverse = 1,
  /** silent at maxDistance (rolloff 1) */
  Linear = 2,
  Exponential = 3,
}

export type PlayOptions = {
  volume?: number;
  /** -1 left .. 0 centre .. 1 right (2D sounds) */
  pan?: number;
  /** 1 = normal; also changes the pitch */
  speed?: number;
  loop?: boolean;
  /** start paused */
  paused?: boolean;
  bus?: number;
  /** never replaced when all voices are in use (music) */
  protect?: boolean;
  /** seconds before it starts (sample-accurate) */
  delay?: number;
  /** makes it a 3D sound */
  position?: Vec3;
  velocity?: Vec3;
  minDistance?: number;
  maxDistance?: number;
  attenuation?: Attenuation;
  rolloff?: number;
  /** 0 = off, 1 = real world (default) */
  doppler?: number;
};

/** Effect parameters (see the native docs): 0 is WET on every effect. */
export const Param = {
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
} as const;

export enum FilterKind {
  LowPass = 0,
  HighPass = 1,
  BandPass = 2,
  Notch = 3,
}

/** An effect target for a bus. */
export function bus(n: number): EffectTarget {
  return 0x80000000 + n;
}

const path = (uri: string) => (uri.startsWith("file://") ? decodeURIComponent(uri.slice(7)) : uri);

// ---- engine ----

/** Starts the engine and its audio output. Safe to call more than once. */
export function init(): boolean {
  return N.init();
}
/** Stops the output and frees everything (sounds, sources). */
export function shutdown(): void {
  N.shutdown();
}
/** Seconds of audio produced so far: the engine's clock. */
export function time(): number {
  return N.time();
}
export function sampleRate(): number {
  return N.sampleRate();
}

// ---- sources ----

/** Loads a file into memory: WAV / OGG / MP3 (sound effects) or a tracker module. Throws if
 *  the file cannot be read or its format is unknown. */
export function load(uri: string): SourceId {
  return N.load(path(uri));
}
/** OGG / MP3 decoded while playing (music: uses little memory). */
export function stream(uri: string): SourceId {
  return N.stream(path(uri));
}
/** Frees a source. Sounds playing from it keep playing. */
export function unload(source: SourceId): void {
  N.unload(source);
}
/** Length in seconds, or -1 if not known (streams, modules). */
export function duration(source: SourceId): number {
  return N.duration(source);
}

// ---- playing ----

export function play(source: SourceId, options: PlayOptions = {}): Handle {
  return N.play(source, options);
}
/** Plays at a moment in game time (seconds): rapid fire keeps even spacing. */
export function playClocked(time: number, source: SourceId, options: PlayOptions = {}): Handle {
  return N.playClocked(time, source, options);
}
export const stop = (h: Handle): void => N.stop(h);
export const stopAfter = (h: Handle, seconds: number): void => N.stopAfter(h, seconds);
export const setVolume = (h: Handle, volume: number): void => N.setVolume(h, volume);
export const fadeVolume = (h: Handle, to: number, seconds: number): void => N.fadeVolume(h, to, seconds);
export const setPan = (h: Handle, pan: number): void => N.setPan(h, pan);
export const setSpeed = (h: Handle, speed: number): void => N.setSpeed(h, speed);
export const setPaused = (h: Handle, paused: boolean): void => N.setPaused(h, paused);
export const setLooping = (h: Handle, loop: boolean): void => N.setLooping(h, loop);
export const setProtected = (h: Handle, protect: boolean): void => N.setProtected(h, protect);
export const isPlaying = (h: Handle): boolean => N.isPlaying(h);
export const activeVoices = (): number => N.activeVoices();
/** default 32, at most 256 */
export const setMaxVoices = (n: number): void => N.setMaxVoices(n);
export const stopAll = (): void => N.stopAll();
export const setMasterVolume = (volume: number): void => N.setMasterVolume(volume);

// ---- buses ----

/** A new bus (1..15), or -1 when all are in use. Buses flow into the main bus. */
export const createBus = (): number => N.createBus();
export const setBusVolume = (b: number, volume: number): void => N.setBusVolume(b, volume);
export const fadeBusVolume = (b: number, to: number, seconds: number): void => N.fadeBusVolume(b, to, seconds);
export const setBusPaused = (b: number, paused: boolean): void => N.setBusPaused(b, paused);
export const stopBus = (b: number): void => N.stopBus(b);

// ---- effects ----

export function setFilter(target: EffectTarget, slot: number, kind: FilterKind, cutoff: number, resonance = 0.707): void {
  N.setFilter(target, slot, kind, cutoff, resonance);
}
export function setEcho(target: EffectTarget, slot: number, o: { delay: number; feedback: number; damping?: number; maxDelay?: number }): void {
  N.setEcho(target, slot, o.delay, o.feedback, o.damping ?? 0, o.maxDelay ?? Math.max(1, o.delay));
}
export function setReverb(target: EffectTarget, slot: number, o: { roomSize?: number; damping?: number; width?: number; dry?: number } = {}): void {
  N.setReverb(target, slot, o.roomSize ?? 0.5, o.damping ?? 0.5, o.width ?? 1, o.dry ?? 1);
}
export const removeEffect = (target: EffectTarget, slot: number): void => N.removeEffect(target, slot);
export const setEffectParam = (target: EffectTarget, slot: number, param: number, value: number): void =>
  N.setEffectParam(target, slot, param, value);
/** Fades from the value last set for this parameter (jumps if none was set). */
export const fadeEffectParam = (target: EffectTarget, slot: number, param: number, value: number, seconds: number): void =>
  N.fadeEffectParam(target, slot, param, value, seconds);

// ---- 3D ----

/** The listener (camera). Default: origin, facing -Z, +Y up (+X is right). */
export const setListener = (position: Vec3, forward: Vec3, up: Vec3 = [0, 1, 0]): void => N.setListener(position, forward, up);
export const setListenerVelocity = (v: Vec3): void => N.setListenerVelocity(v);
export const setSpeedOfSound = (speed: number): void => N.setSpeedOfSound(speed);
export const setPosition = (h: Handle, p: Vec3): void => N.setPosition(h, p[0], p[1], p[2]);
export const setVelocity = (h: Handle, v: Vec3): void => N.setVelocity(h, v[0], v[1], v[2]);
