//! # featheraudio
//!
//! A feather-light audio engine for games, in pure Rust: fire-and-forget sounds, voice
//! handles, buses, fades, effects (filters, echo, reverb), simple 3D sound and tracker music
//! (MOD / IT / S3M / XM).
//!
//! ```no_run
//! use featheraudio::{Audio, PlayOptions, Wav};
//!
//! # #[cfg(feature = "cpal")] {
//! let audio = Audio::new()?; // default output device
//! let pew = Wav::from_bytes(&std::fs::read("pew.wav")?)?;
//! audio.play(&pew); // fire and forget
//!
//! let music = audio.create_bus().unwrap();
//! # #[cfg(feature = "xm")] {
//! let song = featheraudio::Tracker::from_bytes(&std::fs::read("song.xm")?)?;
//! let bgm = audio.play_with(&song, PlayOptions::new().bus(music).volume(0.8));
//! audio.fade_volume(bgm, 0.0, 2.0); // fade out over 2 seconds
//! audio.stop_after(bgm, 2.0);
//! # }
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The engine has two halves: [`Audio`], used from the game, and [`Mixer`], which runs on the
//! audio thread. They only talk through queues, so the audio thread never waits for the game.

// `chunks_exact(2)` is kept over `as_chunks` (newer clippy suggests it) so older Rust versions
// still build the crate
#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]

mod audio;
#[cfg_attr(not(any(feature = "ogg", feature = "mp3")), allow(dead_code))]
mod decode;
pub mod effects;
mod mixer;
mod source;
pub mod spatial;
#[cfg(any(feature = "ogg", feature = "mp3"))]
mod stream;
#[cfg(any(feature = "mod", feature = "it", feature = "xm"))]
pub mod tracker;
mod wav;

pub use audio::{Audio, Bus, Handle, PlayOptions};
pub use mixer::Mixer;
pub use source::{Instance, Source};
pub use spatial::{Attenuation, Spatial};
#[cfg(any(feature = "ogg", feature = "mp3"))]
pub use stream::Stream;
#[cfg(any(feature = "mod", feature = "it", feature = "xm"))]
pub use tracker::Tracker;
pub use wav::Wav;

/// Errors from loading sounds or opening the audio device.
#[derive(Debug)]
pub enum Error {
    /// The file is not in a supported format, or is broken.
    Format(String),
    /// The audio output could not be opened.
    Device(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Format(s) => write!(f, "format error: {s}"),
            Error::Device(s) => write!(f, "audio device error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
