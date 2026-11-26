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
/// The song loops at its end, which suits background music. Cloning is cheap.
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
        let inst: Option<Box<dyn Instance>> = match self.kind {
            #[cfg(feature = "xm")]
            Kind::Xm => xm::format::load(d).ok().map(|m| {
                let mut p = xm::XmPlayer::new(m, rate);
                p.play_from(0);
                Box::new(XmInstance(p)) as Box<dyn Instance>
            }),
            #[cfg(feature = "it")]
            Kind::It => it::ItModule::load_any(d).ok().map(|m| {
                let mut p = it::ItPlayer::new(m, rate);
                p.play_from(0);
                Box::new(ItInstance(p)) as Box<dyn Instance>
            }),
            #[cfg(feature = "mod")]
            Kind::Mod => protracker::Module::load(d, protracker::module::DEFAULT_MAX_SAMPLE_LENGTH).ok().map(|m| {
                let cfg = protracker::EngineConfig { output_rate: rate, stereo_separation: self.stereo_separation, ..Default::default() };
                let mut e = protracker::Engine::new(m, cfg);
                e.play_song();
                Box::new(ModInstance { engine: e, buf: vec![0; 4096] }) as Box<dyn Instance>
            }),
        };
        inst.unwrap_or_else(|| Box::new(Silent))
    }
}

struct Silent;

impl Instance for Silent {
    fn render(&mut self, _out: &mut [f32]) -> usize {
        0
    }
}

#[cfg(feature = "xm")]
struct XmInstance(xm::XmPlayer);

#[cfg(feature = "xm")]
impl Instance for XmInstance {
    fn render(&mut self, out: &mut [f32]) -> usize {
        self.0.render_f32(out);
        out.len() / 2
    }
}

#[cfg(feature = "it")]
struct ItInstance(it::ItPlayer);

#[cfg(feature = "it")]
impl Instance for ItInstance {
    fn render(&mut self, out: &mut [f32]) -> usize {
        self.0.render_f32(out);
        out.len() / 2
    }
}

#[cfg(feature = "mod")]
struct ModInstance {
    engine: protracker::Engine,
    /// 16-bit output of the engine, converted to float
    buf: Vec<i16>,
}

#[cfg(feature = "mod")]
impl Instance for ModInstance {
    fn render(&mut self, out: &mut [f32]) -> usize {
        // in pieces of at most the scratch size: no allocation on the audio thread
        for chunk in out.chunks_mut(self.buf.len()) {
            let b = &mut self.buf[..chunk.len()];
            self.engine.render(b);
            for (o, &s) in chunk.iter_mut().zip(b.iter()) {
                *o = s as f32 / 32768.0;
            }
        }
        out.len() / 2
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
