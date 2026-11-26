//! Effects: filters, echo and reverb, on a single sound or on a whole bus.
//!
//! ```
//! # #[cfg(feature = "effects")] {
//! use featheraudio::{Audio, Bus, effects::{self, Filter, Reverb}};
//!
//! let (audio, _mixer) = Audio::with_manual_output(48000);
//! // reverb on everything
//! audio.set_effect(Bus::MAIN, 0, Reverb::new().room_size(0.8));
//! // muffle the music bus (e.g. when the pause menu opens), smoothly
//! let music = audio.create_bus().unwrap();
//! audio.set_effect(music, 0, Filter::low_pass(20000.0));
//! audio.fade_effect_param(music, 0, Filter::CUTOFF, 800.0, 0.3);
//! // and back to normal
//! audio.fade_effect_param(music, 0, effects::WET, 0.0, 0.3);
//! # }
//! ```
//!
//! Every sound and every bus has [`MAX_EFFECTS`] slots, run in order. Parameters are numbers:
//! [`WET`] (0 = off, 1 = full effect, default 1) works on every effect; the others are
//! constants on each effect type ([`Filter::CUTOFF`], [`Echo::FEEDBACK`]...). Changes and fades
//! take effect at the next block of 256 frames.
//!
//! Effects on a bus keep running after the sounds stop, until their tail (an echo, the reverb)
//! has died away. Effects on a sound stop with the sound.

/// Effect slots per sound and per bus.
pub const MAX_EFFECTS: usize = 4;

/// Parameter 0 of every effect: how much of the effect to hear, 0 (bypassed) .. 1 (full,
/// the default). Handled by the engine, so your own effects get it for free.
pub const WET: u32 = 0;

/// An audio effect. Implement it for your own effects; the built-in ones are [`Filter`],
/// [`Echo`] and [`Reverb`].
///
/// [`prepare`](Effect::prepare) runs on the game side before the effect is sent to the audio
/// thread, so allocate there. [`process`](Effect::process) and
/// [`set_param`](Effect::set_param) run on the audio thread: no allocation, no locks.
pub trait Effect: Send {
    /// Called once with the output sample rate before the effect is used. Allocate buffers
    /// here.
    fn prepare(&mut self, rate: u32) {
        let _ = rate;
    }

    /// Processes interleaved stereo frames (`L R L R ...`) in place. Return the full effect;
    /// the engine mixes it with the input according to [`WET`].
    fn process(&mut self, buf: &mut [f32]);

    /// Changes a parameter (1 and up; 0 is [`WET`], handled by the engine).
    fn set_param(&mut self, param: u32, value: f32) {
        let _ = (param, value);
    }

    /// Seconds the effect keeps sounding after its input goes silent (echo, reverb).
    /// A bus stops running its effects once its sounds have been silent this long.
    fn tail(&self) -> f32 {
        0.0
    }
}

/// What an effect is put on: a playing sound or a bus. Made from a [`Handle`](crate::Handle)
/// or a [`Bus`](crate::Bus), so you can pass either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectTarget {
    Voice(crate::Handle),
    Bus(crate::Bus),
}

impl From<crate::Handle> for EffectTarget {
    fn from(h: crate::Handle) -> Self {
        EffectTarget::Voice(h)
    }
}

impl From<crate::Bus> for EffectTarget {
    fn from(b: crate::Bus) -> Self {
        EffectTarget::Bus(b)
    }
}

#[cfg(feature = "effects")]
pub use builtin::*;

#[cfg(feature = "effects")]
mod builtin {
    use super::Effect;
    use std::f32::consts::PI;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Kind {
        LowPass,
        HighPass,
        BandPass,
        Notch,
    }

    /// A resonant filter (biquad, as in the RBJ audio EQ cookbook): low-pass, high-pass,
    /// band-pass or notch, 12 dB per octave.
    pub struct Filter {
        kind: Kind,
        cutoff: f32,
        q: f32,
        rate: f32,
        dirty: bool,
        /// b0 b1 b2 a1 a2, normalised by a0
        c: [f32; 5],
        /// transposed direct form II state, per channel
        z: [[f32; 2]; 2],
    }

    impl Filter {
        /// Parameter: the cutoff (or centre) frequency in Hz.
        pub const CUTOFF: u32 = 1;
        /// Parameter: the resonance (Q). 0.707 is flat; higher values ring at the cutoff.
        pub const RESONANCE: u32 = 2;

        fn new(kind: Kind, cutoff: f32) -> Filter {
            Filter {
                kind,
                cutoff,
                q: std::f32::consts::FRAC_1_SQRT_2,
                rate: 0.0,
                dirty: true,
                c: [1.0, 0.0, 0.0, 0.0, 0.0],
                z: [[0.0; 2]; 2],
            }
        }

        /// Lets through what is below `cutoff` Hz: muffled, far away, under water.
        pub fn low_pass(cutoff: f32) -> Filter {
            Filter::new(Kind::LowPass, cutoff)
        }

        /// Lets through what is above `cutoff` Hz: thin, a radio, a telephone.
        pub fn high_pass(cutoff: f32) -> Filter {
            Filter::new(Kind::HighPass, cutoff)
        }

        /// Lets through a band around `centre` Hz (its width set by the resonance).
        pub fn band_pass(centre: f32) -> Filter {
            Filter::new(Kind::BandPass, centre)
        }

        /// Removes a band around `centre` Hz.
        pub fn notch(centre: f32) -> Filter {
            Filter::new(Kind::Notch, centre)
        }

        /// Sets the resonance (Q); see [`Filter::RESONANCE`].
        pub fn resonance(mut self, q: f32) -> Filter {
            self.q = q;
            self
        }

        fn update(&mut self) {
            self.dirty = false;
            if self.rate <= 0.0 {
                return;
            }
            let f = self.cutoff.clamp(10.0, self.rate * 0.49);
            let w = 2.0 * PI * f / self.rate;
            let (sin, cos) = w.sin_cos();
            let alpha = sin / (2.0 * self.q.max(0.05));
            let (b0, b1, b2) = match self.kind {
                Kind::LowPass => ((1.0 - cos) * 0.5, 1.0 - cos, (1.0 - cos) * 0.5),
                Kind::HighPass => ((1.0 + cos) * 0.5, -(1.0 + cos), (1.0 + cos) * 0.5),
                Kind::BandPass => (alpha, 0.0, -alpha),
                Kind::Notch => (1.0, -2.0 * cos, 1.0),
            };
            let a0 = 1.0 + alpha;
            self.c = [
                b0 / a0,
                b1 / a0,
                b2 / a0,
                -2.0 * cos / a0,
                (1.0 - alpha) / a0,
            ];
        }
    }

    impl Effect for Filter {
        fn prepare(&mut self, rate: u32) {
            self.rate = rate as f32;
            self.update();
        }

        fn process(&mut self, buf: &mut [f32]) {
            if self.dirty {
                self.update();
            }
            let [b0, b1, b2, a1, a2] = self.c;
            for fr in buf.chunks_exact_mut(2) {
                for (x, z) in fr.iter_mut().zip(self.z.iter_mut()) {
                    let y = b0 * *x + z[0];
                    z[0] = b1 * *x - a1 * y + z[1];
                    z[1] = b2 * *x - a2 * y;
                    *x = y;
                }
            }
            // keep the state out of the denormal range once the input has gone quiet
            for z in self.z.iter_mut().flatten() {
                if z.abs() < 1e-20 {
                    *z = 0.0;
                }
            }
        }

        fn set_param(&mut self, param: u32, value: f32) {
            match param {
                Filter::CUTOFF => self.cutoff = value,
                Filter::RESONANCE => self.q = value,
                _ => return,
            }
            self.dirty = true;
        }
    }

    /// An echo: the sound repeats every `delay` seconds, each repeat `feedback` times as loud,
    /// optionally duller each time ([`Echo::DAMPING`]).
    pub struct Echo {
        delay: f32,
        feedback: f32,
        damping: f32,
        max_delay: f32,
        /// interleaved stereo ring buffer
        buf: Vec<f32>,
        pos: usize,
        lp: [f32; 2],
        rate: f32,
    }

    impl Echo {
        /// Parameter: seconds between repeats (up to the maximum given to
        /// [`max_delay`](Echo::max_delay)).
        pub const DELAY: u32 = 1;
        /// Parameter: level of each repeat relative to the one before, 0 .. 1.
        pub const FEEDBACK: u32 = 2;
        /// Parameter: how much duller each repeat gets, 0 (not at all) .. 1.
        pub const DAMPING: u32 = 3;

        /// An echo repeating every `delay` seconds, each repeat `feedback` (0 .. 1) times as
        /// loud as the previous one.
        pub fn new(delay: f32, feedback: f32) -> Echo {
            Echo {
                delay,
                feedback,
                damping: 0.0,
                max_delay: delay.max(1.0),
                buf: Vec::new(),
                pos: 0,
                lp: [0.0; 2],
                rate: 0.0,
            }
        }

        /// Makes each repeat duller; see [`Echo::DAMPING`].
        pub fn damping(mut self, d: f32) -> Echo {
            self.damping = d;
            self
        }

        /// The longest delay [`Echo::DELAY`] can be set to later (default: the larger of the
        /// starting delay and 1 second). Sets the memory used.
        pub fn max_delay(mut self, seconds: f32) -> Echo {
            self.max_delay = seconds.max(self.delay);
            self
        }
    }

    impl Effect for Echo {
        fn prepare(&mut self, rate: u32) {
            self.rate = rate as f32;
            let frames = (self.max_delay * self.rate).ceil() as usize + 1;
            self.buf = vec![0.0; frames * 2];
            self.pos = 0;
        }

        fn process(&mut self, buf: &mut [f32]) {
            let frames = self.buf.len() / 2;
            if frames < 2 {
                return;
            }
            let d = ((self.delay * self.rate).round() as usize).clamp(1, frames - 1);
            let fb = self.feedback.clamp(0.0, 1.0);
            let k = 1.0 - self.damping.clamp(0.0, 0.99);
            for fr in buf.chunks_exact_mut(2) {
                let rd = (self.pos + frames - d) % frames;
                for (c, (x, lp)) in fr.iter_mut().zip(self.lp.iter_mut()).enumerate() {
                    let delayed = self.buf[rd * 2 + c];
                    *lp += k * (delayed - *lp);
                    let mut y = *x + fb * *lp;
                    if y.abs() < 1e-20 {
                        y = 0.0;
                    }
                    self.buf[self.pos * 2 + c] = y;
                    *x = y;
                }
                self.pos = (self.pos + 1) % frames;
            }
        }

        fn set_param(&mut self, param: u32, value: f32) {
            match param {
                Echo::DELAY => self.delay = value.max(0.0),
                Echo::FEEDBACK => self.feedback = value,
                Echo::DAMPING => self.damping = value,
                _ => {}
            }
        }

        fn tail(&self) -> f32 {
            let fb = self.feedback.clamp(0.0, 0.999);
            // repeats until 60 dB down
            if fb <= 0.0 {
                self.delay
            } else {
                self.delay * (1.0 + (0.001f32).ln() / fb.ln())
            }
        }
    }

    struct Comb {
        buf: Vec<f32>,
        pos: usize,
        store: f32,
    }

    impl Comb {
        #[inline]
        fn run(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
            let out = self.buf[self.pos];
            self.store = out * (1.0 - damp) + self.store * damp;
            if self.store.abs() < 1e-20 {
                self.store = 0.0;
            }
            self.buf[self.pos] = x + self.store * feedback;
            self.pos += 1;
            if self.pos == self.buf.len() {
                self.pos = 0;
            }
            out
        }
    }

    struct Allpass {
        buf: Vec<f32>,
        pos: usize,
    }

    impl Allpass {
        #[inline]
        fn run(&mut self, x: f32) -> f32 {
            let b = self.buf[self.pos];
            let mut s = x + b * 0.5;
            if s.abs() < 1e-20 {
                s = 0.0;
            }
            self.buf[self.pos] = s;
            self.pos += 1;
            if self.pos == self.buf.len() {
                self.pos = 0;
            }
            b - x
        }
    }

    // Freeverb's delay lengths, in samples at 44.1 kHz; the right channel is 23 longer
    const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
    const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
    const SPREAD: usize = 23;

    /// A room reverb (Freeverb: 8 comb and 4 all-pass filters per channel). Uses about
    /// 100 KB of memory at 48 kHz.
    pub struct Reverb {
        room: f32,
        damping: f32,
        width: f32,
        dry: f32,
        combs: Vec<[Comb; 2]>,
        allpasses: Vec<[Allpass; 2]>,
    }

    impl Default for Reverb {
        fn default() -> Self {
            Reverb::new()
        }
    }

    impl Reverb {
        /// Parameter: the size of the room, 0 (small) .. 1 (huge, very long tail). Default 0.5.
        pub const ROOM_SIZE: u32 = 1;
        /// Parameter: how quickly the high frequencies die away, 0 (bright) .. 1 (dull).
        /// Default 0.5.
        pub const DAMPING: u32 = 2;
        /// Parameter: stereo width of the reverb, 0 (mono) .. 1. Default 1.
        pub const WIDTH: u32 = 3;
        /// Parameter: level of the original sound in the output, 0 .. 1. Default 1; set 0 on a
        /// bus you use as a reverb send.
        pub const DRY: u32 = 4;

        pub fn new() -> Reverb {
            Reverb {
                room: 0.5,
                damping: 0.5,
                width: 1.0,
                dry: 1.0,
                combs: Vec::new(),
                allpasses: Vec::new(),
            }
        }

        /// See [`Reverb::ROOM_SIZE`].
        pub fn room_size(mut self, r: f32) -> Reverb {
            self.room = r;
            self
        }

        /// See [`Reverb::DAMPING`].
        pub fn damping(mut self, d: f32) -> Reverb {
            self.damping = d;
            self
        }

        /// See [`Reverb::WIDTH`].
        pub fn width(mut self, w: f32) -> Reverb {
            self.width = w;
            self
        }

        /// See [`Reverb::DRY`].
        pub fn dry(mut self, d: f32) -> Reverb {
            self.dry = d;
            self
        }

        fn feedback(&self) -> f32 {
            self.room.clamp(0.0, 1.0) * 0.28 + 0.7
        }
    }

    impl Effect for Reverb {
        fn prepare(&mut self, rate: u32) {
            let scale = rate as f32 / 44100.0;
            let len = |n: usize| ((n as f32 * scale) as usize).max(1);
            self.combs = COMBS
                .iter()
                .map(|&n| {
                    [0, SPREAD].map(|s| Comb {
                        buf: vec![0.0; len(n + s)],
                        pos: 0,
                        store: 0.0,
                    })
                })
                .collect();
            self.allpasses = ALLPASSES
                .iter()
                .map(|&n| {
                    [0, SPREAD].map(|s| Allpass {
                        buf: vec![0.0; len(n + s)],
                        pos: 0,
                    })
                })
                .collect();
        }

        fn process(&mut self, buf: &mut [f32]) {
            if self.combs.is_empty() {
                return;
            }
            let fb = self.feedback();
            let damp = self.damping.clamp(0.0, 1.0) * 0.4;
            let w = self.width.clamp(0.0, 1.0);
            let (wet1, wet2) = (w * 0.5 + 0.5, (1.0 - w) * 0.5);
            let dry = self.dry;
            for fr in buf.chunks_exact_mut(2) {
                let input = (fr[0] + fr[1]) * 0.015;
                let mut out = [0.0f32; 2];
                for c in self.combs.iter_mut() {
                    out[0] += c[0].run(input, fb, damp);
                    out[1] += c[1].run(input, fb, damp);
                }
                for a in self.allpasses.iter_mut() {
                    out[0] = a[0].run(out[0]);
                    out[1] = a[1].run(out[1]);
                }
                let (l, r) = (out[0] * wet1 + out[1] * wet2, out[1] * wet1 + out[0] * wet2);
                fr[0] = fr[0] * dry + l;
                fr[1] = fr[1] * dry + r;
            }
        }

        fn set_param(&mut self, param: u32, value: f32) {
            match param {
                Reverb::ROOM_SIZE => self.room = value,
                Reverb::DAMPING => self.damping = value,
                Reverb::WIDTH => self.width = value,
                Reverb::DRY => self.dry = value,
                _ => {}
            }
        }

        fn tail(&self) -> f32 {
            // the longest comb decays by `feedback` per pass; until 60 dB down
            let fb = self.feedback().min(0.999);
            let longest = (COMBS[7] + SPREAD) as f32 / 44100.0;
            longest * (0.001f32).ln() / fb.ln()
        }
    }
}
