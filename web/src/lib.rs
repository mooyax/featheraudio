//! featheraudio in the browser: the engine with a small API for the demo page. The page's
//! Web Audio callback calls [`Demo::render`]; everything else is called from the UI.

use featheraudio::effects::{Echo, Filter, Reverb, WET};
use featheraudio::{Audio, Bus, Handle, Mixer, PlayOptions, Source, Spatial, Wav};
use std::f32::consts::TAU;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Demo {
    audio: Audio,
    mixer: Mixer,
    rate: u32,
    music_bus: Bus,
    sfx_bus: Bus,
    sources: Vec<Box<dyn Source>>,
    pew: Wav,
    chime: Wav,
    engine: Wav,
    music: Wav,
    /// interleaved scratch for render
    buf: Vec<f32>,
    music_handle: Option<Handle>,
}

fn wav(samples: Vec<f32>, rate: u32) -> Wav {
    Wav::from_samples(samples, 1, rate)
}

/// a falling square-wave "pew"
fn pew(rate: u32) -> Wav {
    let n = (rate as f32 * 0.18) as usize;
    let mut ph = 0.0f32;
    wav(
        (0..n)
            .map(|i| {
                let t = i as f32 / n as f32;
                ph += (1400.0 - 1100.0 * t) / rate as f32;
                let sq = if ph.fract() < 0.5 { 1.0 } else { -1.0 };
                sq * 0.3 * (1.0 - t) * (1.0 - t)
            })
            .collect(),
        rate,
    )
}

/// a bell-like chime: a few inharmonic partials with an exponential decay
fn chime(rate: u32) -> Wav {
    let n = (rate as f32 * 1.2) as usize;
    let partials = [(1.0, 1.0), (2.76, 0.5), (5.4, 0.25), (8.93, 0.12)];
    wav(
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let s: f32 = partials.iter().map(|(m, a)| a * (t * 880.0 * m * TAU).sin() * (-t * 3.0 * m).exp()).sum();
                s * 0.35 * (t * 400.0).min(1.0)
            })
            .collect(),
        rate,
    )
}

/// an engine buzz: a looping sawtooth with a wobble (mono, for 3D)
fn engine(rate: u32) -> Wav {
    let n = rate as usize;
    wav(
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let saw = (t * 110.0).fract() * 2.0 - 1.0;
                let saw2 = (t * 55.0).fract() * 2.0 - 1.0;
                (saw * 0.6 + saw2 * 0.4) * 0.35 * (1.0 + 0.25 * (t * 10.0 * TAU).sin())
            })
            .collect(),
        rate,
    )
}

/// a gentle looping arpeggio (8 seconds, stereo), so there is music without a file
fn music(rate: u32) -> Wav {
    let bpm = 120.0;
    let step = 60.0 / bpm / 4.0; // 16th notes
    let chords: [[f32; 4]; 4] = [[57.0, 60.0, 64.0, 69.0], [53.0, 57.0, 60.0, 65.0], [48.0, 52.0, 55.0, 60.0], [55.0, 59.0, 62.0, 67.0]];
    let pattern = [0, 1, 2, 3, 2, 1, 0, 2, 0, 1, 2, 3, 2, 3, 1, 2];
    let total = (step * 16.0 * 4.0 * rate as f32) as usize;
    let mut out = vec![0.0f32; total * 2];
    let hz = |m: f32| 440.0 * 2f32.powf((m - 69.0) / 12.0);
    for (c, chord) in chords.iter().enumerate() {
        // pad: the chord's root and fifth, soft
        let start = (c as f32 * step * 16.0 * rate as f32) as usize;
        let len = (step * 16.0 * rate as f32) as usize;
        for i in 0..len {
            let t = i as f32 / rate as f32;
            let env = (t * 4.0).min(1.0) * (1.0 - i as f32 / len as f32).powf(0.3);
            let s = ((t * hz(chord[0] - 12.0) * TAU).sin() + 0.5 * (t * hz(chord[2] - 12.0) * TAU).sin()) * 0.08 * env;
            out[(start + i) * 2] += s;
            out[(start + i) * 2 + 1] += s;
        }
        // arpeggio: plucked triangle waves, alternating left and right
        for (k, &p) in pattern.iter().enumerate() {
            let s0 = start + (k as f32 * step * rate as f32) as usize;
            let f = hz(chord[p] + 12.0);
            let n = (step * 2.5 * rate as f32) as usize;
            let pan = if k % 2 == 0 { 0.7 } else { 1.0 };
            for i in 0..n.min(total - s0) {
                let t = i as f32 / rate as f32;
                let tri = 1.0 - 4.0 * ((t * f).fract() - 0.5).abs();
                let s = tri * 0.12 * (-t * 9.0).exp();
                out[(s0 + i) * 2] += s * pan;
                out[(s0 + i) * 2 + 1] += s * (1.7 - pan);
            }
        }
    }
    Wav::from_samples(out, 2, rate)
}

#[wasm_bindgen]
impl Demo {
    #[wasm_bindgen(constructor)]
    pub fn new(rate: u32) -> Demo {
        let (audio, mixer) = Audio::with_manual_output(rate);
        let music_bus = audio.create_bus().unwrap();
        let sfx_bus = audio.create_bus().unwrap();
        audio.set_max_voices(64);
        // slot 0 of the music bus: a low-pass, wide open until the music is muffled
        audio.set_effect(music_bus, 0, Filter::low_pass(20000.0));
        audio.set_effect_param(music_bus, 0, Filter::CUTOFF, 20000.0);
        Demo {
            audio,
            mixer,
            rate,
            music_bus,
            sfx_bus,
            sources: Vec::new(),
            pew: pew(rate),
            chime: chime(rate),
            engine: engine(rate),
            music: music(rate),
            buf: Vec::new(),
            music_handle: None,
        }
    }

    /// Renders `left.len()` frames into the two channel buffers. Returns the milliseconds it
    /// took is measured on the JS side.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        if self.buf.len() < n * 2 {
            self.buf.resize(n * 2, 0.0);
        }
        self.mixer.render(&mut self.buf[..n * 2]);
        for i in 0..n {
            left[i] = self.buf[i * 2];
            right[i] = self.buf[i * 2 + 1];
        }
    }

    pub fn active_voices(&self) -> u32 {
        self.audio.active_voices() as u32
    }

    pub fn time(&self) -> f64 {
        self.audio.time()
    }

    // ---- music ----

    /// Plays the built-in arpeggio on the music bus (replacing what was playing).
    pub fn play_builtin_music(&mut self) {
        self.stop_music();
        let h = self.audio.play_with(&self.music, PlayOptions::new().bus(self.music_bus).looping(true).protected(true).volume(0.0));
        self.audio.fade_volume(h, 1.0, 0.5);
        self.music_handle = Some(h);
    }

    /// Plays a dropped file on the music bus: OGG / MP3 streamed, WAV in memory, or a tracker
    /// module. Returns a description, or throws if the format is unknown.
    pub fn play_file(&mut self, bytes: Vec<u8>) -> Result<String, JsError> {
        let (src, what): (Box<dyn Source>, String) = match featheraudio::Stream::from_vec(bytes.clone()) {
            Ok(s) => (Box::new(s), "streamed (OGG / MP3)".into()),
            Err(_) => match Wav::from_bytes(&bytes) {
                Ok(w) => {
                    let d = w.duration();
                    (Box::new(w), format!("WAV, {d:.1} s"))
                }
                Err(_) => {
                    let t = featheraudio::Tracker::from_bytes(&bytes).map_err(|e| JsError::new(&e.to_string()))?;
                    let what = format!("{} module “{}”", t.format(), t.title().trim());
                    (Box::new(t), what)
                }
            },
        };
        self.stop_music();
        let h = self.audio.play_with(src.as_ref(), PlayOptions::new().bus(self.music_bus).looping(true).protected(true));
        self.music_handle = Some(h);
        // keep the source (only the latest few)
        self.sources.push(src);
        if self.sources.len() > 4 {
            self.sources.remove(0);
        }
        Ok(what)
    }

    pub fn stop_music(&mut self) {
        if let Some(h) = self.music_handle.take() {
            self.audio.fade_volume(h, 0.0, 0.3);
            self.audio.stop_after(h, 0.3);
        }
    }

    pub fn set_music_volume(&self, v: f32) {
        self.audio.fade_bus_volume(self.music_bus, v, 0.05);
    }

    /// Fades the music's low-pass to `hz` (20000 = open).
    pub fn muffle_music(&self, hz: f32) {
        self.audio.fade_effect_param(self.music_bus, 0, Filter::CUTOFF, hz, 0.4);
    }

    // ---- sound effects ----

    /// A "pew" on the effects bus. `pan` -1..1.
    pub fn pew(&self, pan: f32, speed: f32) -> u32 {
        self.audio.play_with(&self.pew, PlayOptions::new().bus(self.sfx_bus).pan(pan).speed(speed).volume(0.8)).to_raw()
    }

    /// A pew timed by the page's clock (seconds): rapid fire stays evenly spaced.
    pub fn pew_clocked(&self, time: f64, pan: f32) -> u32 {
        self.audio.play_clocked_with(time, &self.pew, PlayOptions::new().bus(self.sfx_bus).pan(pan).speed(1.6).volume(0.6)).to_raw()
    }

    /// A chime at a 3D position (x right, z towards the listener; the listener is at the
    /// origin facing -z).
    pub fn chime_at(&self, x: f32, z: f32, pitch: f32) -> u32 {
        self.audio
            .play_with(&self.chime, PlayOptions::new().bus(self.sfx_bus).speed(pitch).spatial(Spatial::at([x, 0.0, z]).distance(2.0, 200.0)))
            .to_raw()
    }

    /// A looping engine sound at a 3D position; move it with `set_position` / `set_velocity`.
    pub fn start_engine(&self, x: f32, z: f32) -> u32 {
        let h = self.audio.play_with(&self.engine, PlayOptions::new().bus(self.sfx_bus).looping(true).volume(0.0).spatial(Spatial::at([x, 0.0, z]).distance(3.0, 300.0)));
        self.audio.fade_volume(h, 0.9, 0.3);
        h.to_raw()
    }

    pub fn set_position(&self, h: u32, x: f32, z: f32) {
        self.audio.set_position(Handle::from_raw(h), [x, 0.0, z]);
    }

    pub fn set_velocity(&self, h: u32, vx: f32, vz: f32) {
        self.audio.set_velocity(Handle::from_raw(h), [vx, 0.0, vz]);
    }

    pub fn stop(&self, h: u32) {
        self.audio.fade_volume(Handle::from_raw(h), 0.0, 0.2);
        self.audio.stop_after(Handle::from_raw(h), 0.2);
    }

    pub fn stop_all(&mut self) {
        self.music_handle = None;
        self.audio.stop_all();
    }

    // ---- effects ----

    /// Reverb on everything (the main bus): `room` 0..1, or off.
    pub fn set_reverb(&self, on: bool, room: f32) {
        if on {
            self.audio.set_effect(Bus::MAIN, 0, Reverb::new().room_size(room).damping(0.4));
        } else {
            self.audio.remove_effect(Bus::MAIN, 0);
        }
    }

    pub fn set_reverb_room(&self, room: f32) {
        self.audio.set_effect_param(Bus::MAIN, 0, Reverb::ROOM_SIZE, room);
    }

    /// Echo on the sound effects.
    pub fn set_echo(&self, on: bool) {
        if on {
            self.audio.set_effect(self.sfx_bus, 0, Echo::new(0.25, 0.45).damping(0.3));
        } else {
            self.audio.fade_effect_param(self.sfx_bus, 0, WET, 0.0, 0.05);
        }
    }

    pub fn set_master_volume(&self, v: f32) {
        self.audio.set_master_volume(v);
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
}
