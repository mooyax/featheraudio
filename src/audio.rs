//! The game-side half of the engine.

use crate::effects::{Effect, EffectTarget};
use crate::mixer::{
    BusCmd, Clock, Command, EffectAt, EffectCmd, Ended, ListenerCmd, Mixer, VoiceCmd, MAX_BUSES,
};
use crate::source::Source;
use crate::spatial::Spatial;
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Identifies one playing sound. Stays valid after the sound ends (calls on it then do
/// nothing), and is never reused.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Handle(pub(crate) u32);

impl Handle {
    /// The handle as a number (never 0), for passing through C, JavaScript and the like.
    pub fn to_raw(self) -> u32 {
        self.0
    }
    /// A handle from [`to_raw`](Self::to_raw). A number that was never a handle is harmless:
    /// calls on it do nothing.
    pub fn from_raw(raw: u32) -> Handle {
        Handle(raw)
    }
}

/// A group of voices with its own volume, fades and pause, e.g. music, effects, UI.
/// [`Bus::MAIN`] always exists; [`Audio::create_bus`] makes more (up to 15).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Bus(pub(crate) u16);

impl Bus {
    /// The bus sounds play on unless told otherwise.
    pub const MAIN: Bus = Bus(0);

    /// The bus as a number (0 = [`Bus::MAIN`], 1..15 = created buses).
    pub fn to_raw(self) -> u16 {
        self.0
    }
    /// A bus from [`to_raw`](Self::to_raw). Numbers above 15 mean the last bus.
    pub fn from_raw(raw: u16) -> Bus {
        Bus(raw)
    }
}

/// How to play a sound. `PlayOptions::new()` is: volume 1, centred, normal speed, no loop,
/// not paused, on [`Bus::MAIN`], not protected, no delay, not 3D.
#[derive(Clone, Copy, Debug)]
pub struct PlayOptions {
    pub volume: f32,
    /// -1 (left) .. 0 (centre) .. 1 (right)
    pub pan: f32,
    /// playback speed; also changes the pitch (1 = normal)
    pub speed: f32,
    pub looping: bool,
    /// start paused (resume with [`Audio::set_paused`])
    pub paused: bool,
    pub bus: Bus,
    /// protected voices are never replaced when all voices are in use (use it for music)
    pub protected: bool,
    /// seconds of silence before the sound starts (sample-accurate)
    pub delay: f32,
    /// makes it a 3D sound: volume, pan and pitch follow its position relative to the
    /// listener ([`Audio::set_listener`])
    pub spatial: Option<Spatial>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        PlayOptions {
            volume: 1.0,
            pan: 0.0,
            speed: 1.0,
            looping: false,
            paused: false,
            bus: Bus::MAIN,
            protected: false,
            delay: 0.0,
            spatial: None,
        }
    }
}

impl PlayOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn volume(mut self, v: f32) -> Self {
        self.volume = v;
        self
    }
    pub fn pan(mut self, p: f32) -> Self {
        self.pan = p;
        self
    }
    pub fn speed(mut self, s: f32) -> Self {
        self.speed = s;
        self
    }
    pub fn looping(mut self, l: bool) -> Self {
        self.looping = l;
        self
    }
    pub fn paused(mut self, p: bool) -> Self {
        self.paused = p;
        self
    }
    pub fn bus(mut self, b: Bus) -> Self {
        self.bus = b;
        self
    }
    pub fn protected(mut self, p: bool) -> Self {
        self.protected = p;
        self
    }
    pub fn delay(mut self, seconds: f32) -> Self {
        self.delay = seconds;
        self
    }
    /// Plays the sound in 3D; see [`Spatial`].
    pub fn spatial(mut self, s: Spatial) -> Self {
        self.spatial = Some(s);
        self
    }
}

struct Front {
    commands: Sender<Command>,
    ended: Receiver<Ended>,
    next_handle: u32,
    next_bus: u16,
    /// voices that have been started and not reported as ended
    alive: HashSet<Handle>,
    /// play_clocked: (mixer render count, game time) of the first clocked sound in the
    /// current audio buffer
    clocked: Option<(u64, f64)>,
}

impl Front {
    /// takes in the voices the mixer has finished with (and drops their instances here)
    fn collect(&mut self) {
        while let Ok(e) = self.ended.try_recv() {
            if let Ended::Voice(h, ..) = e {
                self.alive.remove(&h);
            }
        }
    }
}

/// The audio engine, as seen from the game. Cheap to call from any thread (`Send + Sync`);
/// every call only queues a command for the audio thread.
pub struct Audio {
    front: Mutex<Front>,
    clock: Arc<Clock>,
    rate: u32,
    #[cfg(feature = "cpal")]
    _output: Option<crate::audio::cpal_output::Output>,
}

impl Audio {
    /// Opens the default audio output device and starts mixing on its audio thread.
    #[cfg(feature = "cpal")]
    pub fn new() -> Result<Audio, crate::Error> {
        let (cmd_tx, cmd_rx) = channel();
        let (end_tx, end_rx) = channel();
        let clock = Arc::new(Clock::default());
        let c = clock.clone();
        let (output, rate) =
            cpal_output::Output::start(move |rate| Mixer::new(rate, cmd_rx, end_tx, c))?;
        Ok(Audio {
            front: Mutex::new(Front::new(cmd_tx, end_rx)),
            clock,
            rate,
            _output: Some(output),
        })
    }

    /// Creates the engine without opening a device: you get the [`Mixer`] and call
    /// [`Mixer::render`] yourself, from your own audio callback or offline.
    pub fn with_manual_output(rate: u32) -> (Audio, Mixer) {
        let (cmd_tx, cmd_rx) = channel();
        let (end_tx, end_rx) = channel();
        let clock = Arc::new(Clock::default());
        let audio = Audio {
            front: Mutex::new(Front::new(cmd_tx, end_rx)),
            clock: clock.clone(),
            rate,
            #[cfg(feature = "cpal")]
            _output: None,
        };
        (audio, Mixer::new(rate, cmd_rx, end_tx, clock))
    }

    /// The output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn front(&self) -> std::sync::MutexGuard<'_, Front> {
        self.front.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, c: Command) {
        let _ = self.front().commands.send(c);
    }

    fn voice(&self, h: Handle, c: VoiceCmd) {
        self.send(Command::Voice(h, c));
    }

    /// Plays a sound with the default options and returns its handle. Fire and forget: the
    /// voice cleans itself up when it ends.
    pub fn play(&self, source: &dyn Source) -> Handle {
        self.play_with(source, PlayOptions::default())
    }

    /// Plays a sound with the given options.
    pub fn play_with(&self, source: &dyn Source, opts: PlayOptions) -> Handle {
        let mut f = self.front();
        self.start(&mut f, source, opts, 0)
    }

    fn start(
        &self,
        f: &mut Front,
        source: &dyn Source,
        opts: PlayOptions,
        extra_delay: u64,
    ) -> Handle {
        // the instance is created (and allocated) here, on the game side
        let instance = source.instance(self.rate);
        f.collect();
        f.next_handle = f.next_handle.wrapping_add(1).max(1);
        let handle = Handle(f.next_handle);
        f.alive.insert(handle);
        let delay = (opts.delay.max(0.0) as f64 * self.rate as f64).round() as u64 + extra_delay;
        let _ = f.commands.send(Command::Play {
            handle,
            instance,
            opts,
            delay,
        });
        handle
    }

    /// Plays a sound at a moment in game time (seconds, any clock you like, e.g. your physics
    /// time). Several sounds started during one audio buffer are spread out by the
    /// differences between their times, instead of all starting together at the next buffer:
    /// rapid fire stays evenly spaced whatever the buffer size. Like SoLoud's `playClocked`.
    pub fn play_clocked(&self, time: f64, source: &dyn Source) -> Handle {
        self.play_clocked_with(time, source, PlayOptions::default())
    }

    /// [`play_clocked`](Self::play_clocked) with options.
    pub fn play_clocked_with(&self, time: f64, source: &dyn Source, opts: PlayOptions) -> Handle {
        let renders = self.clock.renders.load(Ordering::Acquire);
        let mut f = self.front();
        let offset = match f.clocked {
            // a later sound in the same buffer: start it as much later as its time is
            Some((r, t0)) if r == renders && time >= t0 => {
                // at most a quarter of a second, in case the game stalled
                ((time - t0) * self.rate as f64)
                    .round()
                    .min(self.rate as f64 / 4.0) as u64
            }
            _ => {
                f.clocked = Some((renders, time));
                0
            }
        };
        self.start(&mut f, source, opts, offset)
    }

    /// Seconds of audio the mixer has produced so far: the engine's clock. Advances in steps
    /// of one audio buffer. Combine with [`PlayOptions::delay`] to start sounds at exact
    /// moments, e.g. on the beat.
    pub fn time(&self) -> f64 {
        self.clock.frames.load(Ordering::Acquire) as f64 / self.rate as f64
    }

    /// Stops a sound (with a few milliseconds of fade-out, to avoid a click).
    pub fn stop(&self, h: Handle) {
        self.voice(h, VoiceCmd::Stop);
    }

    /// Stops a sound after `seconds`. Combine with [`fade_volume`](Self::fade_volume) to fade out.
    pub fn stop_after(&self, h: Handle, seconds: f32) {
        self.voice(h, VoiceCmd::StopAfter(seconds));
    }

    pub fn set_volume(&self, h: Handle, volume: f32) {
        self.voice(h, VoiceCmd::Volume(volume));
    }

    /// Changes the volume smoothly to `to` over `seconds`.
    pub fn fade_volume(&self, h: Handle, to: f32, seconds: f32) {
        self.voice(h, VoiceCmd::Fade(to, seconds));
    }

    /// -1 (left) .. 0 (centre) .. 1 (right)
    pub fn set_pan(&self, h: Handle, pan: f32) {
        self.voice(h, VoiceCmd::Pan(pan));
    }

    /// Playback speed (1 = normal); also changes the pitch.
    pub fn set_speed(&self, h: Handle, speed: f32) {
        self.voice(h, VoiceCmd::Speed(speed));
    }

    pub fn set_paused(&self, h: Handle, paused: bool) {
        self.voice(h, VoiceCmd::Paused(paused));
    }

    pub fn set_looping(&self, h: Handle, looping: bool) {
        self.voice(h, VoiceCmd::Looping(looping));
    }

    /// Protected voices are never replaced when all voices are in use.
    pub fn set_protected(&self, h: Handle, protected: bool) {
        self.voice(h, VoiceCmd::Protected(protected));
    }

    /// True while the sound has not ended. Because commands reach the audio thread a moment
    /// later, a sound that has just been stopped can still report true briefly.
    pub fn is_playing(&self, h: Handle) -> bool {
        let mut f = self.front();
        f.collect();
        f.alive.contains(&h)
    }

    /// Number of sounds that have not ended.
    pub fn active_voices(&self) -> usize {
        let mut f = self.front();
        f.collect();
        f.alive.len()
    }

    /// How many sounds may play at once (default 32, at most 256). When a new sound starts and
    /// all voices are in use, the oldest one that is not protected is replaced.
    pub fn set_max_voices(&self, n: usize) {
        self.send(Command::MaxVoices(n));
    }

    /// Creates a new bus, or returns `None` when all 15 extra buses are in use.
    pub fn create_bus(&self) -> Option<Bus> {
        let mut f = self.front();
        if (f.next_bus as usize) < MAX_BUSES - 1 {
            f.next_bus += 1;
            Some(Bus(f.next_bus))
        } else {
            None
        }
    }

    pub fn set_bus_volume(&self, b: Bus, volume: f32) {
        self.send(Command::Bus(b, BusCmd::Volume(volume)));
    }

    pub fn fade_bus_volume(&self, b: Bus, to: f32, seconds: f32) {
        self.send(Command::Bus(b, BusCmd::Fade(to, seconds)));
    }

    /// Pauses or resumes every sound on the bus.
    pub fn set_bus_paused(&self, b: Bus, paused: bool) {
        self.send(Command::Bus(b, BusCmd::Paused(paused)));
    }

    /// Stops every sound on the bus.
    pub fn stop_bus(&self, b: Bus) {
        self.send(Command::Bus(b, BusCmd::Stop));
    }

    /// Puts an effect in one of the [`MAX_EFFECTS`](crate::effects::MAX_EFFECTS) slots of a
    /// sound (pass its [`Handle`]) or a bus (pass the [`Bus`]), replacing what was there.
    /// Effects run in slot order. See [`effects`](crate::effects).
    pub fn set_effect(
        &self,
        target: impl Into<EffectTarget>,
        slot: usize,
        mut effect: impl Effect + 'static,
    ) {
        // buffers are allocated here, on the game side
        effect.prepare(self.rate);
        self.effect(target.into(), slot, EffectCmd::Set(Box::new(effect)));
    }

    /// Removes the effect in a slot.
    pub fn remove_effect(&self, target: impl Into<EffectTarget>, slot: usize) {
        self.effect(target.into(), slot, EffectCmd::Remove);
    }

    /// Changes a parameter of the effect in a slot: [`effects::WET`](crate::effects::WET) or
    /// one of the effect's own (e.g. [`Filter::CUTOFF`](crate::effects::Filter::CUTOFF)).
    pub fn set_effect_param(
        &self,
        target: impl Into<EffectTarget>,
        slot: usize,
        param: u32,
        value: f32,
    ) {
        self.effect(target.into(), slot, EffectCmd::Param(param, value));
    }

    /// Changes a parameter smoothly to `value` over `seconds`. The fade starts from the value
    /// last given to this parameter with `set_effect_param` or a fade; if there was none, the
    /// parameter jumps to `value` (the engine does not know effects' starting values).
    pub fn fade_effect_param(
        &self,
        target: impl Into<EffectTarget>,
        slot: usize,
        param: u32,
        value: f32,
        seconds: f32,
    ) {
        self.effect(target.into(), slot, EffectCmd::Fade(param, value, seconds));
    }

    fn effect(&self, t: EffectTarget, slot: usize, c: EffectCmd) {
        let at = match t {
            EffectTarget::Voice(h) => EffectAt::Voice(h),
            EffectTarget::Bus(b) => EffectAt::Bus(b.0 as usize),
        };
        self.send(Command::Effect(at, slot, c));
    }

    /// Places the listener (the player's ears, usually the camera) for 3D sounds: its
    /// position, the direction it faces and its up direction. Default: at the origin, facing
    /// -Z, +Y up.
    pub fn set_listener(&self, position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
        self.send(Command::Listener(ListenerCmd::Place(position, forward, up)));
    }

    /// The listener's velocity in units per second, for the Doppler shift.
    pub fn set_listener_velocity(&self, velocity: [f32; 3]) {
        self.send(Command::Listener(ListenerCmd::Velocity(velocity)));
    }

    /// Units per second (default 343: metres in air). Affects only the Doppler shift.
    pub fn set_speed_of_sound(&self, speed: f32) {
        self.send(Command::Listener(ListenerCmd::SpeedOfSound(speed)));
    }

    /// Makes a playing sound 3D, or changes all of its 3D settings at once.
    pub fn set_spatial(&self, h: Handle, s: Spatial) {
        self.voice(h, VoiceCmd::Spatial(s));
    }

    /// Moves a 3D sound. Call it every frame for moving objects.
    pub fn set_position(&self, h: Handle, position: [f32; 3]) {
        self.voice(h, VoiceCmd::Position(position));
    }

    /// A 3D sound's velocity in units per second, for the Doppler shift.
    pub fn set_velocity(&self, h: Handle, velocity: [f32; 3]) {
        self.voice(h, VoiceCmd::Velocity(velocity));
    }

    pub fn set_master_volume(&self, volume: f32) {
        self.send(Command::MasterVolume(volume));
    }

    pub fn stop_all(&self) {
        self.send(Command::StopAll);
    }
}

impl Front {
    fn new(commands: Sender<Command>, ended: Receiver<Ended>) -> Front {
        Front {
            commands,
            ended,
            next_handle: 0,
            next_bus: 0,
            alive: HashSet::new(),
            clocked: None,
        }
    }
}

#[cfg(feature = "cpal")]
pub(crate) mod cpal_output {
    //! Audio output through cpal. The stream lives on its own thread, so [`Audio`] stays
    //! `Send + Sync` on every platform.

    use super::Mixer;
    use crate::Error;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::mpsc::{channel, sync_channel, Sender};
    use std::thread::JoinHandle;

    pub(crate) struct Output {
        stop: Option<Sender<()>>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Output {
        fn drop(&mut self) {
            self.stop.take();
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Output {
        pub(crate) fn start(
            make_mixer: impl FnOnce(u32) -> Mixer + Send + 'static,
        ) -> Result<(Output, u32), Error> {
            let (ready_tx, ready_rx) = sync_channel::<Result<u32, Error>>(1);
            let (stop_tx, stop_rx) = channel::<()>();
            let thread = std::thread::Builder::new()
                .name("featheraudio".into())
                .spawn(move || {
                    let stream = match open(make_mixer) {
                        Ok((s, rate)) => {
                            let _ = ready_tx.send(Ok(rate));
                            s
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    // keep the stream alive until the Audio is dropped
                    let _ = stop_rx.recv();
                    drop(stream);
                })
                .map_err(|e| Error::Device(e.to_string()))?;
            let rate = ready_rx
                .recv()
                .map_err(|_| Error::Device("audio thread failed to start".into()))??;
            Ok((
                Output {
                    stop: Some(stop_tx),
                    thread: Some(thread),
                },
                rate,
            ))
        }
    }

    fn open(make_mixer: impl FnOnce(u32) -> Mixer) -> Result<(cpal::Stream, u32), Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error::Device("no output device".into()))?;
        let cfg = device
            .default_output_config()
            .map_err(|e| Error::Device(e.to_string()))?;
        let rate = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;
        let config: cpal::StreamConfig = cfg.config();
        let mut mixer = make_mixer(rate);
        let mut buf: Vec<f32> = vec![0.0; 8192];
        let err = |e| eprintln!("featheraudio: stream error: {e}");
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |d: &mut [f32], _| write(d, channels, &mut mixer, &mut buf, |v| v),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |d: &mut [i16], _| {
                    write(d, channels, &mut mixer, &mut buf, |v| (v * 32767.0) as i16)
                },
                err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &config,
                move |d: &mut [u16], _| {
                    write(d, channels, &mut mixer, &mut buf, |v| {
                        ((v * 32767.0) as i32 + 32768) as u16
                    })
                },
                err,
                None,
            ),
            other => {
                return Err(Error::Device(format!(
                    "unsupported sample format {other:?}"
                )))
            }
        }
        .map_err(|e| Error::Device(e.to_string()))?;
        stream.play().map_err(|e| Error::Device(e.to_string()))?;
        Ok((stream, rate))
    }

    /// renders stereo and spreads it over the device's channels (mono devices get L+R)
    fn write<T: Copy>(
        d: &mut [T],
        channels: usize,
        mixer: &mut Mixer,
        buf: &mut Vec<f32>,
        conv: impl Fn(f32) -> T,
    ) {
        let frames = d.len() / channels;
        if buf.len() < frames * 2 {
            buf.resize(frames * 2, 0.0); // only if the device asks for an unusually large block
        }
        mixer.render(&mut buf[..frames * 2]);
        for (f, frame) in d.chunks_exact_mut(channels).enumerate() {
            let (l, r) = (buf[f * 2], buf[f * 2 + 1]);
            if channels == 1 {
                frame[0] = conv((l + r) * 0.5);
                continue;
            }
            for (c, s) in frame.iter_mut().enumerate() {
                *s = conv(match c {
                    0 => l,
                    1 => r,
                    _ => 0.0,
                });
            }
        }
    }
}
