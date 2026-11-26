//! C API for featheraudio. The header is `include/featheraudio.h`; every function here is
//! declared there with its documentation.
//!
//! Threading: every function except `fa_audio_render` may be called from any thread.
//! `fa_audio_render` is called from one audio thread at a time.

#![allow(clippy::missing_safety_doc)]

use featheraudio::{Attenuation, Audio, Bus, Handle, Mixer, PlayOptions, Source, Spatial, Wav};
use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::sync::Mutex;

pub struct FaAudio {
    audio: Audio,
    /// only with manual output (fa_audio_create_manual)
    mixer: Option<Mutex<Mixer>>,
}

pub struct FaSource {
    src: Box<dyn Source>,
    duration: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FaSpatial {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub min_distance: f32,
    pub max_distance: f32,
    /// 0 none, 1 inverse, 2 linear, 3 exponential
    pub attenuation: i32,
    pub rolloff: f32,
    pub doppler: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FaPlayOptions {
    pub volume: f32,
    pub pan: f32,
    pub speed: f32,
    pub looping: i32,
    pub paused: i32,
    pub protect: i32,
    pub bus: u32,
    pub delay: f32,
    /// NULL for a normal (2D) sound
    pub spatial: *const FaSpatial,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(msg: impl ToString) {
    let s = CString::new(msg.to_string().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = s);
}

/// The message of the last error on this thread (empty if none). Valid until the next call
/// that fails on this thread.
#[no_mangle]
pub extern "C" fn fa_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

// ---------- engine ----------

#[cfg(feature = "cpal")]
#[no_mangle]
pub extern "C" fn fa_audio_create() -> *mut FaAudio {
    match Audio::new() {
        Ok(audio) => Box::into_raw(Box::new(FaAudio { audio, mixer: None })),
        Err(e) => {
            set_error(e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn fa_audio_create_manual(sample_rate: u32) -> *mut FaAudio {
    if sample_rate == 0 {
        set_error("sample rate is 0");
        return std::ptr::null_mut();
    }
    let (audio, mixer) = Audio::with_manual_output(sample_rate);
    Box::into_raw(Box::new(FaAudio { audio, mixer: Some(Mutex::new(mixer)) }))
}

#[no_mangle]
pub unsafe extern "C" fn fa_audio_destroy(a: *mut FaAudio) {
    if !a.is_null() {
        drop(Box::from_raw(a));
    }
}

unsafe fn audio<'a>(a: *const FaAudio) -> Option<&'a FaAudio> {
    a.as_ref()
}

#[no_mangle]
pub unsafe extern "C" fn fa_audio_sample_rate(a: *const FaAudio) -> u32 {
    audio(a).map_or(0, |a| a.audio.sample_rate())
}

#[no_mangle]
pub unsafe extern "C" fn fa_audio_render(a: *const FaAudio, out: *mut f32, frames: u32) -> i32 {
    let Some(a) = audio(a) else { return 0 };
    let Some(m) = &a.mixer else { return 0 };
    if out.is_null() {
        return 0;
    }
    let buf = std::slice::from_raw_parts_mut(out, frames as usize * 2);
    let mut m = m.lock().unwrap_or_else(|e| e.into_inner());
    m.render(buf);
    1
}

#[no_mangle]
pub unsafe extern "C" fn fa_time(a: *const FaAudio) -> f64 {
    audio(a).map_or(0.0, |a| a.audio.time())
}

// ---------- sources ----------

fn new_source(src: Box<dyn Source>, duration: f64) -> *mut FaSource {
    Box::into_raw(Box::new(FaSource { src, duration }))
}

unsafe fn bytes<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() || len == 0 {
        set_error("no data");
        None
    } else {
        Some(std::slice::from_raw_parts(data, len))
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_source_load(data: *const u8, len: usize) -> *mut FaSource {
    let Some(b) = bytes(data, len) else { return std::ptr::null_mut() };
    let wav_err = match Wav::from_bytes(b) {
        Ok(w) => {
            let d = w.duration();
            return new_source(Box::new(w), d);
        }
        Err(e) => e,
    };
    #[cfg(any(feature = "mod", feature = "it", feature = "xm"))]
    if let Ok(t) = featheraudio::Tracker::from_bytes(b) {
        return new_source(Box::new(t), -1.0);
    }
    set_error(wav_err);
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn fa_source_stream(data: *const u8, len: usize) -> *mut FaSource {
    let Some(_b) = bytes(data, len) else { return std::ptr::null_mut() };
    #[cfg(any(feature = "ogg", feature = "mp3"))]
    {
        match featheraudio::Stream::from_vec(_b.to_vec()) {
            Ok(s) => new_source(Box::new(s), -1.0),
            Err(e) => {
                set_error(e);
                std::ptr::null_mut()
            }
        }
    }
    #[cfg(not(any(feature = "ogg", feature = "mp3")))]
    {
        set_error("streaming needs the ogg or mp3 feature");
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_source_from_samples(samples: *const f32, frames: u32, channels: u32, sample_rate: u32) -> *mut FaSource {
    if samples.is_null() || frames == 0 || !(1..=2).contains(&channels) || sample_rate == 0 {
        set_error("bad samples");
        return std::ptr::null_mut();
    }
    let v = std::slice::from_raw_parts(samples, frames as usize * channels as usize).to_vec();
    let w = Wav::from_samples(v, channels as usize, sample_rate);
    let d = w.duration();
    new_source(Box::new(w), d)
}

#[no_mangle]
pub unsafe extern "C" fn fa_source_duration(s: *const FaSource) -> f64 {
    s.as_ref().map_or(-1.0, |s| s.duration)
}

#[no_mangle]
pub unsafe extern "C" fn fa_source_destroy(s: *mut FaSource) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}

// ---------- playing ----------

#[no_mangle]
pub extern "C" fn fa_play_options_default() -> FaPlayOptions {
    FaPlayOptions { volume: 1.0, pan: 0.0, speed: 1.0, looping: 0, paused: 0, protect: 0, bus: 0, delay: 0.0, spatial: std::ptr::null() }
}

#[no_mangle]
pub extern "C" fn fa_spatial_default(x: f32, y: f32, z: f32) -> FaSpatial {
    to_c_spatial(&Spatial::at([x, y, z]))
}

fn to_c_spatial(s: &Spatial) -> FaSpatial {
    FaSpatial {
        position: s.position,
        velocity: s.velocity,
        min_distance: s.min_distance,
        max_distance: s.max_distance,
        attenuation: match s.attenuation {
            Attenuation::None => 0,
            Attenuation::Inverse => 1,
            Attenuation::Linear => 2,
            Attenuation::Exponential => 3,
        },
        rolloff: s.rolloff,
        doppler: s.doppler,
    }
}

fn from_c_spatial(s: &FaSpatial) -> Spatial {
    Spatial {
        position: s.position,
        velocity: s.velocity,
        min_distance: s.min_distance,
        max_distance: s.max_distance,
        attenuation: match s.attenuation {
            0 => Attenuation::None,
            2 => Attenuation::Linear,
            3 => Attenuation::Exponential,
            _ => Attenuation::Inverse,
        },
        rolloff: s.rolloff,
        doppler: s.doppler,
    }
}

unsafe fn options(o: *const FaPlayOptions) -> PlayOptions {
    let Some(o) = o.as_ref() else { return PlayOptions::default() };
    PlayOptions {
        volume: o.volume,
        pan: o.pan,
        speed: o.speed,
        looping: o.looping != 0,
        paused: o.paused != 0,
        bus: Bus::from_raw(o.bus.min(u16::MAX as u32) as u16),
        protected: o.protect != 0,
        delay: o.delay,
        spatial: o.spatial.as_ref().map(from_c_spatial),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_play(a: *const FaAudio, s: *const FaSource, opts: *const FaPlayOptions) -> u32 {
    match (audio(a), s.as_ref()) {
        (Some(a), Some(s)) => a.audio.play_with(s.src.as_ref(), options(opts)).to_raw(),
        _ => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_play_clocked(a: *const FaAudio, time: f64, s: *const FaSource, opts: *const FaPlayOptions) -> u32 {
    match (audio(a), s.as_ref()) {
        (Some(a), Some(s)) => a.audio.play_clocked_with(time, s.src.as_ref(), options(opts)).to_raw(),
        _ => 0,
    }
}

macro_rules! voice_fns {
    ($($name:ident($($arg:ident: $ty:ty),*) => $method:ident;)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $name(a: *const FaAudio, h: u32 $(, $arg: $ty)*) {
                if let Some(a) = audio(a) {
                    a.audio.$method(Handle::from_raw(h) $(, $arg)*);
                }
            }
        )*
    };
}

voice_fns! {
    fa_stop() => stop;
    fa_stop_after(seconds: f32) => stop_after;
    fa_set_volume(volume: f32) => set_volume;
    fa_fade_volume(to: f32, seconds: f32) => fade_volume;
    fa_set_pan(pan: f32) => set_pan;
    fa_set_speed(speed: f32) => set_speed;
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_paused(a: *const FaAudio, h: u32, paused: i32) {
    if let Some(a) = audio(a) {
        a.audio.set_paused(Handle::from_raw(h), paused != 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_looping(a: *const FaAudio, h: u32, looping: i32) {
    if let Some(a) = audio(a) {
        a.audio.set_looping(Handle::from_raw(h), looping != 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_protected(a: *const FaAudio, h: u32, protect: i32) {
    if let Some(a) = audio(a) {
        a.audio.set_protected(Handle::from_raw(h), protect != 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_is_playing(a: *const FaAudio, h: u32) -> i32 {
    audio(a).is_some_and(|a| a.audio.is_playing(Handle::from_raw(h))) as i32
}

#[no_mangle]
pub unsafe extern "C" fn fa_active_voices(a: *const FaAudio) -> u32 {
    audio(a).map_or(0, |a| a.audio.active_voices() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_max_voices(a: *const FaAudio, n: u32) {
    if let Some(a) = audio(a) {
        a.audio.set_max_voices(n as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_limiter(a: *const FaAudio, on: i32) {
    if let Some(a) = audio(a) {
        a.audio.set_limiter(on != 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_stop_all(a: *const FaAudio) {
    if let Some(a) = audio(a) {
        a.audio.stop_all();
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_master_volume(a: *const FaAudio, volume: f32) {
    if let Some(a) = audio(a) {
        a.audio.set_master_volume(volume);
    }
}

// ---------- buses ----------

#[no_mangle]
pub unsafe extern "C" fn fa_create_bus(a: *const FaAudio) -> i32 {
    audio(a).and_then(|a| a.audio.create_bus()).map_or(-1, |b| b.to_raw() as i32)
}

fn bus(b: u32) -> Bus {
    Bus::from_raw(b.min(u16::MAX as u32) as u16)
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_bus_volume(a: *const FaAudio, b: u32, volume: f32) {
    if let Some(a) = audio(a) {
        a.audio.set_bus_volume(bus(b), volume);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_fade_bus_volume(a: *const FaAudio, b: u32, to: f32, seconds: f32) {
    if let Some(a) = audio(a) {
        a.audio.fade_bus_volume(bus(b), to, seconds);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_bus_paused(a: *const FaAudio, b: u32, paused: i32) {
    if let Some(a) = audio(a) {
        a.audio.set_bus_paused(bus(b), paused != 0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_stop_bus(a: *const FaAudio, b: u32) {
    if let Some(a) = audio(a) {
        a.audio.stop_bus(bus(b));
    }
}

// ---------- effects ----------

const BUS_FLAG: u32 = 0x8000_0000;

fn target(t: u32) -> featheraudio::effects::EffectTarget {
    if t & BUS_FLAG != 0 {
        bus(t & !BUS_FLAG).into()
    } else {
        Handle::from_raw(t).into()
    }
}

#[cfg(feature = "effects")]
#[no_mangle]
pub unsafe extern "C" fn fa_set_filter(a: *const FaAudio, t: u32, slot: u32, kind: i32, cutoff: f32, resonance: f32) {
    use featheraudio::effects::Filter;
    let Some(a) = audio(a) else { return };
    let f = match kind {
        1 => Filter::high_pass(cutoff),
        2 => Filter::band_pass(cutoff),
        3 => Filter::notch(cutoff),
        _ => Filter::low_pass(cutoff),
    };
    a.audio.set_effect(target(t), slot as usize, f.resonance(resonance));
}

#[cfg(feature = "effects")]
#[no_mangle]
pub unsafe extern "C" fn fa_set_echo(a: *const FaAudio, t: u32, slot: u32, delay: f32, feedback: f32, damping: f32, max_delay: f32) {
    use featheraudio::effects::Echo;
    let Some(a) = audio(a) else { return };
    a.audio.set_effect(target(t), slot as usize, Echo::new(delay, feedback).damping(damping).max_delay(max_delay));
}

#[cfg(feature = "effects")]
#[no_mangle]
pub unsafe extern "C" fn fa_set_reverb(a: *const FaAudio, t: u32, slot: u32, room_size: f32, damping: f32, width: f32, dry: f32) {
    use featheraudio::effects::Reverb;
    let Some(a) = audio(a) else { return };
    a.audio.set_effect(target(t), slot as usize, Reverb::new().room_size(room_size).damping(damping).width(width).dry(dry));
}

#[no_mangle]
pub unsafe extern "C" fn fa_remove_effect(a: *const FaAudio, t: u32, slot: u32) {
    if let Some(a) = audio(a) {
        a.audio.remove_effect(target(t), slot as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_effect_param(a: *const FaAudio, t: u32, slot: u32, param: u32, value: f32) {
    if let Some(a) = audio(a) {
        a.audio.set_effect_param(target(t), slot as usize, param, value);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_fade_effect_param(a: *const FaAudio, t: u32, slot: u32, param: u32, value: f32, seconds: f32) {
    if let Some(a) = audio(a) {
        a.audio.fade_effect_param(target(t), slot as usize, param, value, seconds);
    }
}

// ---------- 3D ----------

unsafe fn vec3(p: *const f32) -> Option<[f32; 3]> {
    p.as_ref().map(|_| [*p, *p.add(1), *p.add(2)])
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_listener(a: *const FaAudio, position: *const f32, forward: *const f32, up: *const f32) {
    if let (Some(a), Some(p), Some(f), Some(u)) = (audio(a), vec3(position), vec3(forward), vec3(up)) {
        a.audio.set_listener(p, f, u);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_listener_velocity(a: *const FaAudio, velocity: *const f32) {
    if let (Some(a), Some(v)) = (audio(a), vec3(velocity)) {
        a.audio.set_listener_velocity(v);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_speed_of_sound(a: *const FaAudio, speed: f32) {
    if let Some(a) = audio(a) {
        a.audio.set_speed_of_sound(speed);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_spatial(a: *const FaAudio, h: u32, s: *const FaSpatial) {
    if let (Some(a), Some(s)) = (audio(a), s.as_ref()) {
        a.audio.set_spatial(Handle::from_raw(h), from_c_spatial(s));
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_position(a: *const FaAudio, h: u32, x: f32, y: f32, z: f32) {
    if let Some(a) = audio(a) {
        a.audio.set_position(Handle::from_raw(h), [x, y, z]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn fa_set_velocity(a: *const FaAudio, h: u32, x: f32, y: f32, z: f32) {
    if let Some(a) = audio(a) {
        a.audio.set_velocity(Handle::from_raw(h), [x, y, z]);
    }
}
