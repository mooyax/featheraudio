//! Tracker music: MOD (ProTracker), IT and S3M (Impulse / Scream Tracker) and XM
//! (FastTracker 2), each behind its own Cargo feature (`mod`, `it`, `xm`).
//!
//! The engines are public so that players and tools can reach their state (patterns,
//! channels, positions); games normally only need [`Tracker`].
//!
//! The engines are derived from pt2-clone and ft2-clone (Olav Sørensen) and use formulas and
//! tables from OpenMPT; they are under the BSD 3-Clause license (see LICENSE-BSD).

#[cfg(feature = "it")]
// ported code is kept close to the C originals (table precision, index loops)
#[allow(clippy::all)]
pub mod it;
#[cfg(feature = "mod")]
#[allow(clippy::all)]
pub mod protracker;
#[cfg(feature = "xm")]
#[allow(clippy::all)]
pub mod xm;

use crate::source::{Instance, Source};
use crate::Error;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    #[cfg(feature = "xm")]
    Xm,
    #[cfg(feature = "it")]
    It,
    #[cfg(feature = "mod")]
    Mod,
}

/// A tracker module, ready to play: MOD (ProTracker engine), IT and S3M (IT engine) or XM
/// (FastTracker 2 engine), as far as the matching features are enabled.
///
/// Like the other sources it plays once, to the end of the song (where it would start to
/// repeat), unless played with `looping` (background music). Cloning is cheap.
#[derive(Clone)]
pub struct Tracker {
    data: Arc<[u8]>,
    kind: Kind,
    title: String,
    stereo_separation: u8,
}

impl Tracker {
    /// Loads a module file. Fails if the format is not supported (or its feature is disabled)
    /// or the file is broken.
    pub fn from_bytes(bytes: &[u8]) -> Result<Tracker, Error> {
        let (kind, title) = Self::probe(bytes)?;
        Ok(Tracker { data: bytes.into(), kind, title, stereo_separation: 100 })
    }

    fn probe(d: &[u8]) -> Result<(Kind, String), Error> {
        #[cfg(feature = "xm")]
        if xm::format::detect(d) {
            let m = xm::format::load(d).map_err(|e| Error::Format(format!("XM: {e:?}")))?;
            return Ok((Kind::Xm, m.title.trim().to_string()));
        }
        #[cfg(feature = "it")]
        if it::format::detect(d) || it::s3m::detect(d) {
            let m = it::ItModule::load_any(d).map_err(|e| Error::Format(format!("IT/S3M: {e:?}")))?;
            return Ok((Kind::It, m.title.trim().to_string()));
        }
        #[cfg(feature = "mod")]
        if protracker::module::detect_mod31(d) {
            let m = protracker::Module::load(d, protracker::module::DEFAULT_MAX_SAMPLE_LENGTH).map_err(|e| Error::Format(format!("MOD: {e:?}")))?;
            let end = m.name.iter().position(|&c| c == 0).unwrap_or(m.name.len());
            return Ok((Kind::Mod, String::from_utf8_lossy(&m.name[..end]).trim().to_string()));
        }
        let _ = d;
        Err(Error::Format("not a supported tracker module (or its feature is disabled)".into()))
    }

    /// The song title stored in the module.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The format: "MOD", "IT/S3M" or "XM".
    pub fn format(&self) -> &'static str {
        match self.kind {
            #[cfg(feature = "xm")]
            Kind::Xm => "XM",
            #[cfg(feature = "it")]
            Kind::It => "IT/S3M",
            #[cfg(feature = "mod")]
            Kind::Mod => "MOD",
        }
    }

    /// Stereo separation of MOD files in percent, for new instances (0 = mono, 100 = hard
    /// Amiga panning, the default; about 20 sounds closer to an Amiga with headphones).
    /// IT, S3M and XM use the panning stored in the song.
    pub fn set_stereo_separation(&mut self, percent: u8) {
        self.stereo_separation = percent.min(100);
    }
}

impl Source for Tracker {
    fn instance(&self, rate: u32) -> Box<dyn Instance> {
        // the file was checked in from_bytes, so loading it again cannot fail
        let d = &self.data[..];
        let engine: Option<Engine> = match self.kind {
            #[cfg(feature = "xm")]
            Kind::Xm => xm::format::load(d).ok().map(|m| {
                let mut p = xm::XmPlayer::new(m, rate);
                p.play_from(0);
                Engine::Xm(Box::new(p))
            }),
            #[cfg(feature = "it")]
            Kind::It => it::ItModule::load_any(d).ok().map(|m| {
                let mut p = it::ItPlayer::new(m, rate);
                p.play_from(0);
                Engine::It(Box::new(p))
            }),
            #[cfg(feature = "mod")]
            Kind::Mod => protracker::Module::load(d, protracker::module::DEFAULT_MAX_SAMPLE_LENGTH).ok().map(|m| {
                let cfg = protracker::EngineConfig { output_rate: rate, stereo_separation: self.stereo_separation, ..Default::default() };
                let mut e = protracker::Engine::new(m, cfg);
                e.play_song();
                Engine::Mod(Box::new(e), vec![0; 4096])
            }),
        };
        match engine {
            Some(engine) => {
                let mut i = TrackerInstance { engine, fade: (rate as f32 * END_FADE_SECONDS) as usize, fade_left: None };
                // like the other sources: once through, unless asked to loop
                i.set_looping(false);
                Box::new(i)
            }
            None => Box::new(Silent),
        }
    }
}

/// after the end of a song that does not loop, the notes still sounding fade out this fast
const END_FADE_SECONDS: f32 = 0.02;

struct Silent;

impl Instance for Silent {
    fn render(&mut self, _out: &mut [f32]) -> usize {
        0
    }
}

enum Engine {
    #[cfg(feature = "xm")]
    Xm(Box<xm::XmPlayer>),
    #[cfg(feature = "it")]
    It(Box<it::ItPlayer>),
    /// with the 16-bit output of the engine, converted to float
    #[cfg(feature = "mod")]
    Mod(Box<protracker::Engine>, Vec<i16>),
}

impl Engine {
    /// Fills `out`; returns the frames before the end of the song (see the engines' render).
    fn render(&mut self, out: &mut [f32]) -> usize {
        match self {
            #[cfg(feature = "xm")]
            Engine::Xm(p) => p.render_f32(out),
            #[cfg(feature = "it")]
            Engine::It(p) => p.render_f32(out),
            #[cfg(feature = "mod")]
            Engine::Mod(e, buf) => {
                // in pieces of at most the scratch size: no allocation on the audio thread
                let mut before_end = 0;
                let mut ended = e.ended;
                for chunk in out.chunks_mut(buf.len()) {
                    let b = &mut buf[..chunk.len()];
                    let n = e.render(b);
                    if !ended {
                        before_end += n;
                        ended = n < chunk.len() / 2;
                    }
                    for (o, &s) in chunk.iter_mut().zip(b.iter()) {
                        *o = s as f32 / 32768.0;
                    }
                }
                before_end
            }
        }
    }

    fn set_stop_at_end(&mut self, on: bool) {
        match self {
            #[cfg(feature = "xm")]
            Engine::Xm(p) => p.set_stop_at_end(on),
            #[cfg(feature = "it")]
            Engine::It(p) => p.set_stop_at_end(on),
            #[cfg(feature = "mod")]
            Engine::Mod(e, _) => e.set_stop_at_end(on),
        }
    }
}

struct TrackerInstance {
    engine: Engine,
    fade: usize,
    /// after the end of the song: frames of the fade-out still to go
    fade_left: Option<usize>,
}

impl Instance for TrackerInstance {
    fn render(&mut self, out: &mut [f32]) -> usize {
        let frames = out.len() / 2;
        if self.fade_left == Some(0) {
            return 0;
        }
        let n = self.engine.render(out);
        let start = match self.fade_left {
            Some(_) => 0,
            None if n < frames => {
                self.fade_left = Some(self.fade);
                n
            }
            None => return frames,
        };
        // the rest of the buffer holds the notes still sounding: fade them out
        let left = self.fade_left.unwrap_or(0);
        let k = left.min(frames - start);
        for (j, f) in out[start * 2..(start + k) * 2].chunks_exact_mut(2).enumerate() {
            let g = (left - j) as f32 / (self.fade.max(1) as f32);
            f[0] *= g;
            f[1] *= g;
        }
        self.fade_left = Some(left - k);
        start + k
    }

    fn set_looping(&mut self, looping: bool) {
        self.engine.set_stop_at_end(!looping);
    }
}

#[cfg(all(test, feature = "mod"))]
pub(crate) mod tests {
    use super::*;

    /// A tiny 4-channel ProTracker module: one square-wave sample, one note on channel 1.
    pub fn tiny_mod() -> Vec<u8> {
        let mut d = vec![0u8; 1084 + 1024];
        d[0..4].copy_from_slice(b"test");
        // sample 1: 32 words, volume 64, loop over the whole sample
        let s = 20;
        d[s..s + 6].copy_from_slice(b"square");
        d[s + 22..s + 24].copy_from_slice(&32u16.to_be_bytes());
        d[s + 25] = 64;
        d[s + 26..s + 28].copy_from_slice(&0u16.to_be_bytes());
        d[s + 28..s + 30].copy_from_slice(&32u16.to_be_bytes());
        for i in 1..31 {
            d[20 + i * 30 + 29] = 1; // empty samples: replen 1
        }
        d[950] = 1; // song length
        d[951] = 127;
        d[1080..1084].copy_from_slice(b"M.K.");
        // pattern 0, row 0, channel 0: sample 1, period 428 (C-2)
        d[1084] = (428 >> 8) as u8;
        d[1085] = (428 & 0xFF) as u8;
        d[1086] = 0x10;
        let mut b = d;
        b.extend((0..64).map(|i| if i < 32 { 100u8 } else { 156u8 }));
        b
    }

    #[test]
    fn plays_a_module() {
        let t = Tracker::from_bytes(&tiny_mod()).unwrap();
        assert_eq!((t.title(), t.format()), ("test", "MOD"));
        let mut i = t.instance(48000);
        let mut out = vec![0.0; 4800 * 2];
        assert_eq!(i.render(&mut out), 4800);
        assert!(out.iter().any(|x| x.abs() > 0.05), "module should not be silent");
        assert!(Tracker::from_bytes(b"definitely not a module").is_err());
    }
}

#[cfg(all(test, feature = "mod", feature = "it", feature = "xm"))]
mod end_tests {
    use super::*;

    /// Frames until the instance finishes (or `max` seconds), at 48 kHz.
    fn length(file: &str, looping: bool, max: f32) -> usize {
        let data = std::fs::read(format!("{}/tests/data/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut i = Tracker::from_bytes(&data).unwrap().instance(48000);
        i.set_looping(looping);
        let mut out = vec![0.0; 960 * 2];
        let mut total = 0;
        while total < (max * 48000.0) as usize {
            let n = i.render(&mut out);
            total += n;
            if n < 960 {
                break;
            }
        }
        total
    }

    // the song in tests/data/loop-end.* (made by make-modules.py): 40 rows of 0.12 s with a
    // pattern loop inside (XM: 36, FastTracker 2 plays its loop once less, as libopenmpt does),
    // then 20 ms of fade-out
    #[test]
    fn songs_end_where_they_would_repeat() {
        for (f, secs) in [("loop-end.mod", 4.82), ("loop-end.it", 4.82), ("loop-end.xm", 4.34)] {
            let n = length(f, false, 20.0) as f32 / 48000.0;
            assert!((n - secs).abs() < 0.002, "{f}: {n} s, not {secs}");
        }
    }

    #[test]
    fn looping_songs_go_on() {
        for f in ["loop-end.mod", "loop-end.it", "loop-end.xm"] {
            assert_eq!(length(f, true, 12.0), 12 * 48000, "{f}");
        }
    }

    #[test]
    fn the_end_fades_out_and_play_from_the_start_again() {
        for f in ["loop-end.mod", "loop-end.it", "loop-end.xm"] {
            let data = std::fs::read(format!("{}/tests/data/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
            let t = Tracker::from_bytes(&data).unwrap();
            let mut i = t.instance(48000);
            let mut out = vec![0.0; 48000 * 2 * 6];
            let n = i.render(&mut out);
            let s = &out[..n * 2];
            // sound right up to the fade, which ends near silence
            let before = &s[s.len() - 960 * 2 * 2..s.len() - 960 * 2];
            assert!(before.iter().any(|x| x.abs() > 0.05), "{f}: sound before the end");
            assert!(s[s.len() - 2..].iter().all(|x| x.abs() < 0.01), "{f}: faded out");
            assert_eq!(i.render(&mut out[..1920]), 0, "{f}: finished");
            // a new instance starts over
            let mut j = t.instance(48000);
            assert_eq!(j.render(&mut out), n, "{f}: the same length again");
        }
    }

    #[test]
    fn a_song_played_once_ends_its_voice() {
        let data = std::fs::read(format!("{}/tests/data/loop-end.mod", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let t = Tracker::from_bytes(&data).unwrap();
        let (a, mut m) = crate::Audio::with_manual_output(48000);
        let once = a.play(&t);
        let looped = a.play_with(&t, crate::PlayOptions::new().looping(true));
        let mut out = vec![0.0; 48000 * 2 * 6];
        m.render(&mut out);
        assert!(!a.is_playing(once), "ends after 4.8 s");
        assert!(a.is_playing(looped), "loops");
        // looping can be turned off while it plays: it then stops at the end of the song
        a.set_looping(looped, false);
        let mut out = vec![0.0; 48000 * 2 * 6];
        m.render(&mut out);
        assert!(!a.is_playing(looped), "stopped at the next end");
    }
}
