//! Audio sources and their playing instances.
//!
//! A [`Source`] holds what can be shared (decoded samples, a module file); every call to
//! [`Audio::play`](crate::Audio::play) creates a new [`Instance`] from it, so one source can
//! play any number of times, even on top of itself.

/// Something that can be played: a sample, a tracker module, a generator.
pub trait Source: Send + Sync {
    /// Creates a new playing instance that renders at `rate` Hz.
    fn instance(&self, rate: u32) -> Box<dyn Instance>;
}

/// One playing copy of a [`Source`]. Runs on the audio thread.
pub trait Instance: Send {
    /// Writes interleaved stereo frames (`L R L R ...`, -1.0..1.0) into `out`, overwriting it,
    /// and returns the number of frames written. Fewer frames than `out` holds means the
    /// instance has finished.
    fn render(&mut self, out: &mut [f32]) -> usize;

    /// Loop at the end instead of finishing. Sources that always loop may ignore it.
    fn set_looping(&mut self, _looping: bool) {}

    /// Playback speed (1.0 = normal), changing the pitch as well. Sources may ignore it.
    fn set_speed(&mut self, _speed: f32) {}
}
