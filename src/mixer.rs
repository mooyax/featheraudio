//! The mixer: runs on the audio thread, takes commands from [`Audio`](crate::Audio) through a
//! queue and never locks. Finished instances are sent back to be dropped on the game side, so
//! the audio thread does not free memory either.

use crate::effects::{Effect, MAX_EFFECTS, WET};
use crate::source::Instance;
use crate::spatial::{self, Listener, Spatial};
use crate::{Bus, Handle, PlayOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// frames mixed per step
const BLOCK: usize = 256;
/// buses, including the main bus 0
pub(crate) const MAX_BUSES: usize = 16;
/// upper limit for [`Audio::set_max_voices`](crate::Audio::set_max_voices)
pub(crate) const VOICE_CAPACITY: usize = 256;
/// stopping or pausing fades out over this many seconds, to avoid clicks
const DECLICK_SECONDS: f32 = 0.005;

pub(crate) enum Command {
    /// `delay`: frames of silence before the sound starts
    Play {
        handle: Handle,
        instance: Box<dyn Instance>,
        opts: PlayOptions,
        delay: u64,
    },
    Voice(Handle, VoiceCmd),
    Bus(Bus, BusCmd),
    Effect(EffectAt, usize, EffectCmd),
    Listener(ListenerCmd),
    MasterVolume(f32),
    MaxVoices(usize),
    StopAll,
}

#[derive(Clone, Copy)]
pub(crate) enum EffectAt {
    Voice(Handle),
    Bus(usize),
}

pub(crate) enum EffectCmd {
    /// put an effect in the slot (already prepared for the output rate)
    Set(Box<dyn Effect>),
    Remove,
    Param(u32, f32),
    /// fade a parameter to a value over seconds
    Fade(u32, f32, f32),
}

pub(crate) enum ListenerCmd {
    /// position, forward, up
    Place([f32; 3], [f32; 3], [f32; 3]),
    Velocity([f32; 3]),
    SpeedOfSound(f32),
}

pub(crate) enum VoiceCmd {
    Volume(f32),
    Pan(f32),
    Speed(f32),
    Paused(bool),
    Looping(bool),
    Protected(bool),
    /// fade the volume to `to` over `secs`
    Fade(f32, f32),
    Spatial(Spatial),
    Position([f32; 3]),
    Velocity([f32; 3]),
    /// stop after `secs` (with a short fade-out)
    StopAfter(f32),
    Stop,
}

pub(crate) enum BusCmd {
    Volume(f32),
    Fade(f32, f32),
    Paused(bool),
    Stop,
}

/// How far the mixer has got, readable from the game side.
#[derive(Default)]
pub(crate) struct Clock {
    /// calls to [`Mixer::render`]
    pub renders: AtomicU64,
    /// frames rendered in total
    pub frames: AtomicU64,
}

/// Sent back to [`Audio`](crate::Audio) to be dropped there, so the audio thread never frees
/// memory.
#[allow(dead_code)]
pub(crate) enum Ended {
    /// the voice has ended (finished, stopped or was replaced by a newer one)
    Voice(Handle, Box<dyn Instance>),
    /// an effect that was replaced or removed, or sent to a voice that had already ended
    Effect(Box<dyn Effect>),
}

/// A volume that can fade linearly towards a target over a number of frames.
#[derive(Clone, Copy)]
struct Fader {
    value: f32,
    target: f32,
    /// frames left until the target is reached (0 = not fading)
    left: u64,
}

impl Fader {
    fn new(v: f32) -> Self {
        Fader {
            value: v,
            target: v,
            left: 0,
        }
    }

    fn set(&mut self, v: f32) {
        *self = Fader::new(v);
    }

    fn fade(&mut self, to: f32, frames: u64) {
        if frames == 0 {
            self.set(to);
        } else {
            self.target = to;
            self.left = frames;
        }
    }

    /// advance by `n` frames
    fn step(&mut self, n: u64) {
        if self.left == 0 {
            return;
        }
        if n >= self.left {
            self.set(self.target);
        } else {
            self.value += (self.target - self.value) * (n as f32 / self.left as f32);
            self.left -= n;
        }
    }
}

/// One effect in a chain, with its wet level and parameter fades.
struct Slot {
    fx: Box<dyn Effect>,
    wet: Fader,
    /// wet level at the end of the last block
    wet_gain: f32,
    /// parameters being faded (id, fader); `NO_PARAM` marks a free entry
    fades: [(u32, Fader); 4],
}

const NO_PARAM: u32 = u32::MAX;

type Chain = [Option<Slot>; MAX_EFFECTS];

fn empty_chain() -> Chain {
    [const { None }; MAX_EFFECTS]
}

fn has_effects(c: &Chain) -> bool {
    c.iter().any(|s| s.is_some())
}

/// Applies a command to a chain. Removed effects are returned to be dropped on the game side.
fn chain_cmd(chain: &mut Chain, slot: usize, cmd: EffectCmd, rate: u32) -> Option<Box<dyn Effect>> {
    let Some(entry) = chain.get_mut(slot) else {
        return match cmd {
            EffectCmd::Set(fx) => Some(fx),
            _ => None,
        };
    };
    match cmd {
        EffectCmd::Set(fx) => entry
            .replace(Slot {
                fx,
                wet: Fader::new(1.0),
                wet_gain: 1.0,
                fades: [(NO_PARAM, Fader::new(0.0)); 4],
            })
            .map(|s| s.fx),
        EffectCmd::Remove => entry.take().map(|s| s.fx),
        EffectCmd::Param(p, v) => {
            if let Some(s) = entry {
                if p == WET {
                    // a set jumps (no ramp)
                    s.wet.set(v);
                    s.wet_gain = v;
                } else {
                    // remember the value (cancelling any fade), so a later fade starts from it
                    if let Some(i) = s
                        .fades
                        .iter()
                        .position(|f| f.0 == p)
                        .or_else(|| s.fades.iter().position(|f| f.0 == NO_PARAM))
                    {
                        s.fades[i] = (p, Fader::new(v));
                    }
                    s.fx.set_param(p, v);
                }
            }
            None
        }
        EffectCmd::Fade(p, v, secs) => {
            if let Some(s) = entry {
                let n = (secs.max(0.0) as f64 * rate as f64).round() as u64;
                if p == WET {
                    s.wet.fade(v, n);
                } else {
                    // the current value is not known to the engine: the fade starts from the
                    // value last set by a fade or set_param, or jumps if there was none
                    let i = s
                        .fades
                        .iter()
                        .position(|f| f.0 == p)
                        .or_else(|| s.fades.iter().position(|f| f.0 == NO_PARAM))
                        .unwrap_or(0);
                    let mut f = if s.fades[i].0 == p {
                        s.fades[i].1
                    } else {
                        Fader::new(v)
                    };
                    f.fade(v, n);
                    s.fades[i] = (p, f);
                    s.fx.set_param(p, f.value);
                }
            }
            None
        }
    }
}

/// Runs a chain over `buf` (interleaved stereo) for a block of `n` frames. `dry` is scratch
/// space at least as long as `buf`.
fn run_chain(chain: &mut Chain, buf: &mut [f32], dry: &mut [f32], n: u64) {
    for s in chain.iter_mut().flatten() {
        for (p, f) in s.fades.iter_mut() {
            // finished fades stay in the list (left == 0) so a later fade starts from their value
            if *p != NO_PARAM && f.left > 0 {
                f.step(n);
                s.fx.set_param(*p, f.value);
            }
        }
        s.wet.step(n);
        let (from, to) = (s.wet_gain, s.wet.value);
        s.wet_gain = to;
        if from >= 1.0 && to >= 1.0 {
            s.fx.process(buf);
        } else if from <= 0.0 && to <= 0.0 {
            // bypassed
        } else {
            let dry = &mut dry[..buf.len()];
            dry.copy_from_slice(buf);
            s.fx.process(buf);
            let frames = (buf.len() / 2).max(1) as f32;
            let d = (to - from) / frames;
            let mut w = from;
            for (o, x) in buf.chunks_exact_mut(2).zip(dry.chunks_exact(2)) {
                w += d;
                o[0] = x[0] + (o[0] - x[0]) * w;
                o[1] = x[1] + (o[1] - x[1]) * w;
            }
        }
    }
}

fn chain_tail(c: &Chain) -> f32 {
    c.iter().flatten().map(|s| s.fx.tail()).fold(0.0, f32::max)
}

struct Voice {
    handle: Handle,
    inst: Box<dyn Instance>,
    volume: Fader,
    pan: f32,
    bus: usize,
    paused: bool,
    /// fading out before pausing: frames left, and the volume to restore on resume
    pausing: Option<(u64, Fader)>,
    protected: bool,
    /// frames until the voice stops (counting down), if a stop is scheduled
    stop_in: Option<u64>,
    /// gains used at the end of the last block, ramped from towards the new ones
    gain: [f32; 2],
    /// play order, for replacing the oldest voice when all are in use
    serial: u64,
    /// frames to wait before starting
    delay: u64,
    /// playback speed set by the game (the Doppler shift is applied on top)
    speed: f32,
    /// speed last given to the instance
    applied_speed: f32,
    spatial: Option<Spatial>,
    effects: Chain,
}

struct BusState {
    volume: Fader,
    paused: bool,
    gain: f32,
    effects: Chain,
    /// frames since sound last reached the bus (to stop running effects once their tail ends)
    quiet: u64,
}

/// The mixing side of the engine.
///
/// With the `cpal` feature, [`Audio::new`](crate::Audio::new) runs it on the audio thread for
/// you. With [`Audio::with_manual_output`](crate::Audio::with_manual_output) you get it to call
/// [`Mixer::render`] yourself: from your own audio callback (Web Audio, AVAudioEngine,
/// AudioTrack...) or offline.
pub struct Mixer {
    rate: u32,
    commands: Receiver<Command>,
    ended: Sender<Ended>,
    voices: Vec<Voice>,
    max_voices: usize,
    serial: u64,
    buses: Vec<BusState>,
    master: Fader,
    master_gain: f32,
    tmp: Vec<f32>,
    dry: Vec<f32>,
    bus_buf: Vec<f32>,
    listener: Listener,
    clock: Arc<Clock>,
}

/// Balance pan law: centre keeps both channels at full level (so stereo sources are not
/// changed), -1 / +1 silences the other side.
fn pan_gains(pan: f32) -> [f32; 2] {
    let p = pan.clamp(-1.0, 1.0);
    [(1.0 - p).min(1.0), (1.0 + p).min(1.0)]
}

impl Mixer {
    pub(crate) fn new(
        rate: u32,
        commands: Receiver<Command>,
        ended: Sender<Ended>,
        clock: Arc<Clock>,
    ) -> Mixer {
        Mixer {
            rate,
            commands,
            ended,
            voices: Vec::with_capacity(VOICE_CAPACITY),
            max_voices: 32,
            serial: 0,
            buses: (0..MAX_BUSES)
                .map(|_| BusState {
                    volume: Fader::new(1.0),
                    paused: false,
                    gain: 1.0,
                    effects: empty_chain(),
                    quiet: u64::MAX,
                })
                .collect(),
            master: Fader::new(1.0),
            master_gain: 1.0,
            tmp: vec![0.0; BLOCK * 2],
            dry: vec![0.0; BLOCK * 2],
            listener: Listener::default(),
            bus_buf: vec![0.0; MAX_BUSES * BLOCK * 2],
            clock,
        }
    }

    /// The output sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn frames(&self, secs: f32) -> u64 {
        (secs.max(0.0) as f64 * self.rate as f64).round() as u64
    }

    fn end(&mut self, i: usize) {
        let v = self.voices.swap_remove(i);
        for s in v.effects.into_iter().flatten() {
            let _ = self.ended.send(Ended::Effect(s.fx));
        }
        let _ = self.ended.send(Ended::Voice(v.handle, v.inst));
    }

    fn find(&self, h: Handle) -> Option<usize> {
        self.voices.iter().position(|v| v.handle == h)
    }

    fn apply(&mut self, cmd: Command) {
        let declick = self.frames(DECLICK_SECONDS).max(1);
        match cmd {
            Command::Play {
                handle,
                mut instance,
                opts,
                delay,
            } => {
                if self.voices.len() >= self.max_voices {
                    // replace the oldest voice that is not protected
                    let oldest = self
                        .voices
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| !v.protected)
                        .min_by_key(|(_, v)| v.serial)
                        .map(|(i, _)| i);
                    match oldest {
                        Some(i) => self.end(i),
                        None => {
                            let _ = self.ended.send(Ended::Voice(handle, instance));
                            return;
                        }
                    }
                }
                instance.set_looping(opts.looping);
                instance.set_speed(opts.speed);
                let bus = (opts.bus.0 as usize).min(MAX_BUSES - 1);
                let (pg, dist) = match &opts.spatial {
                    Some(sp) => {
                        let a = spatial::apply(sp, &self.listener);
                        if a.doppler != 1.0 {
                            instance.set_speed(opts.speed * a.doppler);
                        }
                        (a.pan, a.gain)
                    }
                    None => (pan_gains(opts.pan), 1.0),
                };
                let g = opts.volume * dist;
                self.serial += 1;
                self.voices.push(Voice {
                    handle,
                    inst: instance,
                    volume: Fader::new(opts.volume),
                    pan: opts.pan,
                    bus,
                    paused: opts.paused,
                    pausing: None,
                    protected: opts.protected,
                    stop_in: None,
                    // start at full gain: no ramp-in, so attacks stay sharp
                    gain: if opts.paused {
                        [0.0; 2]
                    } else {
                        [pg[0] * g, pg[1] * g]
                    },
                    serial: self.serial,
                    delay,
                    speed: opts.speed,
                    applied_speed: opts.speed,
                    spatial: opts.spatial,
                    effects: empty_chain(),
                });
            }
            Command::Voice(h, c) => {
                let Some(i) = self.find(h) else { return };
                let v = &mut self.voices[i];
                match c {
                    VoiceCmd::Volume(x) => v.volume.set(x),
                    VoiceCmd::Pan(x) => v.pan = x,
                    VoiceCmd::Speed(x) => {
                        v.speed = x;
                        if v.spatial.is_none() {
                            v.applied_speed = x;
                            v.inst.set_speed(x);
                        }
                    }
                    VoiceCmd::Spatial(sp) => v.spatial = Some(sp),
                    VoiceCmd::Position(p) => {
                        if let Some(sp) = v.spatial.as_mut() {
                            sp.position = p;
                        }
                    }
                    VoiceCmd::Velocity(x) => {
                        if let Some(sp) = v.spatial.as_mut() {
                            sp.velocity = x;
                        }
                    }
                    VoiceCmd::Looping(x) => v.inst.set_looping(x),
                    VoiceCmd::Protected(x) => v.protected = x,
                    VoiceCmd::Paused(true) => {
                        if !v.paused && v.pausing.is_none() {
                            v.pausing = Some((declick, v.volume));
                            v.volume.fade(0.0, declick);
                        }
                    }
                    VoiceCmd::Paused(false) => {
                        if let Some((_, restore)) = v.pausing.take() {
                            v.volume = restore;
                        }
                        // the gain is 0 while paused, so playback ramps back in
                        v.paused = false;
                    }
                    VoiceCmd::Fade(to, secs) => {
                        let n = (secs.max(0.0) as f64 * self.rate as f64).round() as u64;
                        v.volume.fade(to, n);
                    }
                    VoiceCmd::StopAfter(secs) => {
                        let n = (secs.max(0.0) as f64 * self.rate as f64).round() as u64;
                        v.stop_in = Some(n.max(declick));
                    }
                    VoiceCmd::Stop => {
                        if v.paused {
                            self.end(i);
                        } else {
                            v.stop_in = Some(declick);
                            v.volume.fade(0.0, declick);
                        }
                    }
                }
            }
            Command::Bus(b, c) => {
                let bi = (b.0 as usize).min(MAX_BUSES - 1);
                match c {
                    BusCmd::Volume(x) => self.buses[bi].volume.set(x),
                    BusCmd::Fade(to, secs) => {
                        let n = self.frames(secs);
                        self.buses[bi].volume.fade(to, n);
                    }
                    BusCmd::Paused(p) => self.buses[bi].paused = p,
                    BusCmd::Stop => {
                        for v in self.voices.iter_mut().filter(|v| v.bus == bi) {
                            v.stop_in = Some(declick);
                            v.volume.fade(0.0, declick);
                        }
                    }
                }
            }
            Command::Effect(at, slot, c) => {
                let chain = match at {
                    EffectAt::Voice(h) => self.find(h).map(|i| &mut self.voices[i].effects),
                    EffectAt::Bus(b) => Some(&mut self.buses[b.min(MAX_BUSES - 1)].effects),
                };
                let old = match chain {
                    Some(chain) => chain_cmd(chain, slot, c, self.rate),
                    // the voice has already ended: send the effect back to be dropped
                    None => match c {
                        EffectCmd::Set(fx) => Some(fx),
                        _ => None,
                    },
                };
                if let Some(fx) = old {
                    let _ = self.ended.send(Ended::Effect(fx));
                }
            }
            Command::Listener(c) => match c {
                ListenerCmd::Place(p, f, u) => {
                    self.listener.position = p;
                    self.listener.forward = f;
                    self.listener.up = u;
                }
                ListenerCmd::Velocity(v) => self.listener.velocity = v,
                ListenerCmd::SpeedOfSound(c) => self.listener.speed_of_sound = c,
            },
            Command::MasterVolume(x) => self.master.set(x),
            Command::MaxVoices(n) => self.max_voices = n.clamp(1, VOICE_CAPACITY),
            Command::StopAll => {
                for v in self.voices.iter_mut() {
                    v.stop_in = Some(declick);
                    v.volume.fade(0.0, declick);
                }
            }
        }
    }

    /// Fills `out` with interleaved stereo frames (`L R L R ...`), overwriting it.
    /// Real-time safe: no locks, no allocation (finished sounds are freed on the game side).
    pub fn render(&mut self, out: &mut [f32]) {
        while let Ok(c) = self.commands.try_recv() {
            self.apply(c);
        }
        for chunk in out.chunks_mut(BLOCK * 2) {
            self.render_block(chunk);
        }
        self.clock
            .frames
            .fetch_add((out.len() / 2) as u64, Ordering::Release);
        self.clock.renders.fetch_add(1, Ordering::Release);
    }

    fn render_block(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        let len = n * 2;
        let declick = self.frames(DECLICK_SECONDS).max(1);
        for b in 0..MAX_BUSES {
            self.bus_buf[b * BLOCK * 2..b * BLOCK * 2 + len].fill(0.0);
        }
        let mut used = [false; MAX_BUSES];

        let mut i = 0;
        while i < self.voices.len() {
            let bus_paused = self.buses[self.voices[i].bus].paused;
            let v = &mut self.voices[i];
            if v.paused || bus_paused {
                i += 1;
                continue;
            }
            // a delayed start: silence for the first `delay` frames
            if v.delay >= n as u64 {
                v.delay -= n as u64;
                i += 1;
                continue;
            }
            let skip = v.delay as usize;
            v.delay = 0;
            let want = n - skip;
            // 3D: distance, direction and Doppler for this block
            let (pg, dist) = match &v.spatial {
                Some(sp) => {
                    let a = spatial::apply(sp, &self.listener);
                    let speed = v.speed * a.doppler;
                    if (speed - v.applied_speed).abs() > 1e-4 * v.applied_speed.abs() {
                        v.inst.set_speed(speed);
                        v.applied_speed = speed;
                    }
                    (a.pan, a.gain)
                }
                None => (pan_gains(v.pan), 1.0),
            };
            let got = v.inst.render(&mut self.tmp[..want * 2]);
            if got > 0 && has_effects(&v.effects) {
                run_chain(
                    &mut v.effects,
                    &mut self.tmp[..got * 2],
                    &mut self.dry,
                    n as u64,
                );
            }
            // volume and pan at the end of this block, ramped from the previous gains
            v.volume.step(n as u64);
            let g = v.volume.value * dist;
            let target = [pg[0] * g, pg[1] * g];
            if got > 0 {
                let inv = 1.0 / got as f32;
                let (dl, dr) = ((target[0] - v.gain[0]) * inv, (target[1] - v.gain[1]) * inv);
                let (mut gl, mut gr) = (v.gain[0], v.gain[1]);
                let base = v.bus * BLOCK * 2 + skip * 2;
                let dst = &mut self.bus_buf[base..base + got * 2];
                for (d, s) in dst.chunks_exact_mut(2).zip(self.tmp.chunks_exact(2)) {
                    gl += dl;
                    gr += dr;
                    d[0] += s[0] * gl;
                    d[1] += s[1] * gr;
                }
                used[v.bus] = true;
            }
            v.gain = target;
            if let Some((left, restore)) = v.pausing {
                if left <= n as u64 {
                    v.paused = true;
                    v.pausing = None;
                    v.volume = restore;
                    v.gain = [0.0; 2];
                } else {
                    v.pausing = Some((left - n as u64, restore));
                }
            }
            let mut finished = got < want;
            if let Some(left) = v.stop_in.as_mut() {
                if *left <= n as u64 {
                    finished = true;
                } else {
                    *left -= n as u64;
                    // fade out over the last few milliseconds before a scheduled stop
                    if *left <= declick && v.volume.target != 0.0 {
                        let l = *left;
                        v.volume.fade(0.0, l);
                    }
                }
            }
            if finished {
                self.end(i);
            } else {
                i += 1;
            }
        }

        // buses -> main bus -> output. The other buses flow into the main bus, so effects and
        // volume on the main bus apply to everything.
        let rate = self.rate as f32;
        for b in (0..MAX_BUSES).rev() {
            let bus = &mut self.buses[b];
            bus.volume.step(n as u64);
            let target = bus.volume.value;
            let mut active = used[b];
            if has_effects(&bus.effects) {
                if active {
                    bus.quiet = 0;
                } else if (bus.quiet as f32) < chain_tail(&bus.effects) * rate + BLOCK as f32 {
                    // no new sound, but an echo or reverb is still ringing
                    bus.quiet = bus.quiet.saturating_add(n as u64);
                    active = true;
                }
                if active {
                    let src = &mut self.bus_buf[b * BLOCK * 2..b * BLOCK * 2 + len];
                    run_chain(&mut bus.effects, src, &mut self.dry, n as u64);
                }
            }
            if active {
                let d = (target - bus.gain) / n as f32;
                let mut g = bus.gain;
                let (main, rest) = self.bus_buf.split_at_mut(BLOCK * 2);
                if b == 0 {
                    for (o, s) in out.chunks_exact_mut(2).zip(main.chunks_exact(2)) {
                        g += d;
                        o[0] = s[0] * g;
                        o[1] = s[1] * g;
                    }
                } else {
                    let src = &rest[(b - 1) * BLOCK * 2..(b - 1) * BLOCK * 2 + len];
                    for (o, s) in main[..len].chunks_exact_mut(2).zip(src.chunks_exact(2)) {
                        g += d;
                        o[0] += s[0] * g;
                        o[1] += s[1] * g;
                    }
                    used[0] = true;
                }
            } else if b == 0 {
                out.fill(0.0);
            }
            bus.gain = target;
        }
        self.master.step(n as u64);
        let target = self.master.value;
        let d = (target - self.master_gain) / n as f32;
        let mut g = self.master_gain;
        for o in out.chunks_exact_mut(2) {
            g += d;
            o[0] = (o[0] * g).clamp(-1.0, 1.0);
            o[1] = (o[1] * g).clamp(-1.0, 1.0);
        }
        self.master_gain = target;
    }
}
