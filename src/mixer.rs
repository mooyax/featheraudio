//! The mixer: runs on the audio thread, takes commands from [`Audio`](crate::Audio) through a
//! queue and never locks. Finished instances are sent back to be dropped on the game side, so
//! the audio thread does not free memory either.

use crate::source::Instance;
use crate::{Bus, Handle, PlayOptions};
use std::sync::mpsc::{Receiver, Sender};

/// frames mixed per step
const BLOCK: usize = 256;
/// buses, including the main bus 0
pub(crate) const MAX_BUSES: usize = 16;
/// upper limit for [`Audio::set_max_voices`](crate::Audio::set_max_voices)
pub(crate) const VOICE_CAPACITY: usize = 256;
/// stopping or pausing fades out over this many seconds, to avoid clicks
const DECLICK_SECONDS: f32 = 0.005;

pub(crate) enum Command {
    Play { handle: Handle, instance: Box<dyn Instance>, opts: PlayOptions },
    Voice(Handle, VoiceCmd),
    Bus(Bus, BusCmd),
    MasterVolume(f32),
    MaxVoices(usize),
    StopAll,
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

/// Sent back to [`Audio`](crate::Audio): the voice has ended (finished, stopped or was
/// replaced by a newer one); the instance is dropped there.
pub(crate) struct Ended(pub Handle, #[allow(dead_code)] pub Box<dyn Instance>);

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
        Fader { value: v, target: v, left: 0 }
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
}

struct BusState {
    volume: Fader,
    paused: bool,
    gain: f32,
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
    bus_buf: Vec<f32>,
}

/// Balance pan law: centre keeps both channels at full level (so stereo sources are not
/// changed), -1 / +1 silences the other side.
fn pan_gains(pan: f32) -> [f32; 2] {
    let p = pan.clamp(-1.0, 1.0);
    [(1.0 - p).min(1.0), (1.0 + p).min(1.0)]
}

impl Mixer {
    pub(crate) fn new(rate: u32, commands: Receiver<Command>, ended: Sender<Ended>) -> Mixer {
        Mixer {
            rate,
            commands,
            ended,
            voices: Vec::with_capacity(VOICE_CAPACITY),
            max_voices: 32,
            serial: 0,
            buses: (0..MAX_BUSES).map(|_| BusState { volume: Fader::new(1.0), paused: false, gain: 1.0 }).collect(),
            master: Fader::new(1.0),
            master_gain: 1.0,
            tmp: vec![0.0; BLOCK * 2],
            bus_buf: vec![0.0; MAX_BUSES * BLOCK * 2],
        }
    }

    /// The output sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn frames(&self, secs: f32) -> u64 {
        (secs.max(0.0) as f64 * self.rate as f64) as u64
    }

    fn end(&mut self, i: usize) {
        let v = self.voices.swap_remove(i);
        let _ = self.ended.send(Ended(v.handle, v.inst));
    }

    fn find(&self, h: Handle) -> Option<usize> {
        self.voices.iter().position(|v| v.handle == h)
    }

    fn apply(&mut self, cmd: Command) {
        let declick = self.frames(DECLICK_SECONDS).max(1);
        match cmd {
            Command::Play { handle, mut instance, opts } => {
                if self.voices.len() >= self.max_voices {
                    // replace the oldest voice that is not protected
                    let oldest = self.voices.iter().enumerate().filter(|(_, v)| !v.protected).min_by_key(|(_, v)| v.serial).map(|(i, _)| i);
                    match oldest {
                        Some(i) => self.end(i),
                        None => {
                            let _ = self.ended.send(Ended(handle, instance));
                            return;
                        }
                    }
                }
                instance.set_looping(opts.looping);
                instance.set_speed(opts.speed);
                let bus = (opts.bus.0 as usize).min(MAX_BUSES - 1);
                let pg = pan_gains(opts.pan);
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
                    gain: if opts.paused { [0.0; 2] } else { [pg[0] * opts.volume, pg[1] * opts.volume] },
                    serial: self.serial,
                });
            }
            Command::Voice(h, c) => {
                let Some(i) = self.find(h) else { return };
                let v = &mut self.voices[i];
                match c {
                    VoiceCmd::Volume(x) => v.volume.set(x),
                    VoiceCmd::Pan(x) => v.pan = x,
                    VoiceCmd::Speed(x) => v.inst.set_speed(x),
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
                        let n = (secs.max(0.0) as f64 * self.rate as f64) as u64;
                        v.volume.fade(to, n);
                    }
                    VoiceCmd::StopAfter(secs) => {
                        let n = (secs.max(0.0) as f64 * self.rate as f64) as u64;
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
    }

    fn render_block(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        let len = n * 2;
        let declick = self.frames(DECLICK_SECONDS).max(1);
        self.bus_buf.fill(0.0);
        let mut used = [false; MAX_BUSES];

        let mut i = 0;
        while i < self.voices.len() {
            let bus_paused = self.buses[self.voices[i].bus].paused;
            let v = &mut self.voices[i];
            if v.paused || bus_paused {
                i += 1;
                continue;
            }
            let got = v.inst.render(&mut self.tmp[..len]);
            // volume and pan at the end of this block, ramped from the previous gains
            v.volume.step(n as u64);
            let pg = pan_gains(v.pan);
            let target = [pg[0] * v.volume.value, pg[1] * v.volume.value];
            if got > 0 {
                let inv = 1.0 / got as f32;
                let (dl, dr) = ((target[0] - v.gain[0]) * inv, (target[1] - v.gain[1]) * inv);
                let (mut gl, mut gr) = (v.gain[0], v.gain[1]);
                let base = v.bus * BLOCK * 2;
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
            let mut finished = got < n;
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

        // buses -> master
        out.fill(0.0);
        for (b, bus) in self.buses.iter_mut().enumerate() {
            bus.volume.step(n as u64);
            let target = bus.volume.value;
            if used[b] {
                let src = &self.bus_buf[b * BLOCK * 2..b * BLOCK * 2 + len];
                let d = (target - bus.gain) / n as f32;
                let mut g = bus.gain;
                for (o, s) in out.chunks_exact_mut(2).zip(src.chunks_exact(2)) {
                    g += d;
                    o[0] += s[0] * g;
                    o[1] += s[1] * g;
                }
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
