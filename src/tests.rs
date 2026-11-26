//! Mixer tests with a manual output (no audio device needed).

use crate::*;

const RATE: u32 = 48000;

/// a constant signal: easy to check levels with
fn dc(level: f32, seconds: f32) -> Wav {
    Wav::from_samples(vec![level; (seconds * RATE as f32) as usize], 1, RATE)
}

fn render(m: &mut Mixer, seconds: f32) -> Vec<f32> {
    let mut out = vec![0.0; (seconds * RATE as f32) as usize * 2];
    m.render(&mut out);
    out
}

fn last(out: &[f32]) -> (f32, f32) {
    (out[out.len() - 2], out[out.len() - 1])
}

#[test]
fn plays_and_ends() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let h = a.play(&dc(0.5, 0.1));
    assert!(a.is_playing(h));
    let out = render(&mut m, 0.05);
    assert_eq!(out[0], 0.5, "sounds start at full level (no ramp-in)");
    assert_eq!(last(&out), (0.5, 0.5));
    let out = render(&mut m, 0.1);
    assert_eq!(last(&out), (0.0, 0.0));
    assert!(!a.is_playing(h));
    assert_eq!(a.active_voices(), 0);
}

#[test]
fn volume_pan_and_buses() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let bus = a.create_bus().unwrap();
    let h = a.play_with(
        &dc(0.5, 1.0),
        PlayOptions::new()
            .volume(0.5)
            .pan(-1.0)
            .bus(bus)
            .looping(true),
    );
    assert_eq!(last(&render(&mut m, 0.02)), (0.25, 0.0));
    a.set_bus_volume(bus, 0.5);
    a.set_pan(h, 0.0);
    // ramps over one block, then settles
    assert_eq!(last(&render(&mut m, 0.02)), (0.125, 0.125));
    a.set_master_volume(2.0);
    assert_eq!(last(&render(&mut m, 0.02)), (0.25, 0.25));
    a.set_bus_paused(bus, true);
    assert_eq!(last(&render(&mut m, 0.02)), (0.0, 0.0));
    a.set_bus_paused(bus, false);
    assert_eq!(last(&render(&mut m, 0.02)), (0.25, 0.25));
    a.stop_bus(bus);
    render(&mut m, 0.02);
    assert!(!a.is_playing(h));
}

#[test]
fn fades_and_scheduled_stop() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let h = a.play_with(&dc(1.0, 1.0), PlayOptions::new().looping(true));
    a.fade_volume(h, 0.0, 0.5);
    a.stop_after(h, 0.5);
    let out = render(&mut m, 0.25);
    let (l, _) = last(&out);
    assert!((l - 0.5).abs() < 0.02, "half way through the fade: {l}");
    assert!(a.is_playing(h));
    let out = render(&mut m, 0.3);
    assert_eq!(last(&out), (0.0, 0.0));
    assert!(!a.is_playing(h));
    // the level never jumps up during the fade
    assert!(out
        .chunks(2)
        .map(|c| c[0])
        .collect::<Vec<_>>()
        .windows(2)
        .all(|w| w[1] <= w[0] + 1e-6));
}

#[test]
fn stop_and_pause_do_not_click() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let h = a.play_with(&dc(1.0, 1.0), PlayOptions::new().looping(true));
    render(&mut m, 0.01);
    a.set_paused(h, true);
    let out = render(&mut m, 0.02);
    let steps: f32 = out
        .chunks(2)
        .map(|c| c[0])
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(
        steps < 0.02,
        "pausing should ramp down, largest step {steps}"
    );
    assert_eq!(last(&out), (0.0, 0.0));
    assert!(a.is_playing(h), "a paused sound is still alive");
    a.set_paused(h, false);
    let out = render(&mut m, 0.02);
    assert_eq!(last(&out), (1.0, 1.0));
    a.stop(h);
    let out = render(&mut m, 0.02);
    let steps: f32 = out
        .chunks(2)
        .map(|c| c[0])
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(
        steps < 0.02,
        "stopping should ramp down, largest step {steps}"
    );
    assert!(!a.is_playing(h));
}

#[test]
fn voice_limit_replaces_the_oldest_unprotected() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    a.set_max_voices(2);
    let s = dc(0.1, 1.0);
    let music = a.play_with(&s, PlayOptions::new().protected(true));
    let first = a.play(&s);
    render(&mut m, 0.01);
    let second = a.play(&s);
    render(&mut m, 0.01);
    assert!(a.is_playing(music));
    assert!(
        !a.is_playing(first),
        "the oldest unprotected voice is replaced"
    );
    assert!(a.is_playing(second));
    a.set_protected(second, true);
    let third = a.play(&s);
    render(&mut m, 0.01);
    assert!(
        !a.is_playing(third),
        "nothing to replace: the new sound is dropped"
    );
    assert_eq!(a.active_voices(), 2);
}

#[test]
fn many_sounds_mix_and_clip() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let s = dc(0.3, 0.1);
    for _ in 0..5 {
        a.play(&s);
    }
    assert_eq!(
        last(&render(&mut m, 0.01)),
        (1.0, 1.0),
        "the sum 1.5 is clipped to 1.0"
    );
    let h = a.play_with(&dc(0.5, 1.0), PlayOptions::new().speed(2.0));
    render(&mut m, 0.6);
    assert!(
        !a.is_playing(h),
        "double speed: a 1 s sound ends after 0.5 s"
    );
}

#[cfg(feature = "mod")]
#[test]
fn tracker_music_on_a_bus() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let song = Tracker::from_bytes(&crate::tracker::tests::tiny_mod()).unwrap();
    let music = a.create_bus().unwrap();
    let h = a.play_with(&song, PlayOptions::new().bus(music).protected(true));
    let out = render(&mut m, 0.5);
    assert!(out.iter().any(|x| x.abs() > 0.05));
    a.fade_bus_volume(music, 0.0, 0.2);
    let out = render(&mut m, 0.3);
    assert!(
        out[out.len() - 200..].iter().all(|x| x.abs() < 1e-6),
        "faded out"
    );
    assert!(
        a.is_playing(h),
        "the song keeps playing (silently) until stopped"
    );
}

#[test]
fn delay_and_engine_time() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    assert_eq!(a.time(), 0.0);
    a.play_with(&dc(0.5, 1.0), PlayOptions::new().delay(0.01));
    let out = render(&mut m, 0.02);
    let start = out.chunks(2).position(|c| c[0] != 0.0).unwrap();
    assert_eq!(start, 480, "starts exactly 10 ms (480 frames) late");
    assert!((a.time() - 0.02).abs() < 1e-9);
}

#[test]
fn play_clocked_spreads_sounds_within_a_buffer() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let s = dc(0.1, 1.0);
    // three shots 5 ms apart in game time, all requested before the next buffer
    a.play_clocked(10.000, &s);
    a.play_clocked(10.005, &s);
    a.play_clocked(10.010, &s);
    let out = render(&mut m, 0.02);
    let level = |f: usize| (out[f * 2] * 10.0).round() as i32;
    assert_eq!(
        (level(0), level(239), level(240), level(479), level(480)),
        (1, 1, 2, 2, 3)
    );
    // in the next buffer a new clocked sound starts right away again
    a.play_clocked(10.5, &s);
    let out = render(&mut m, 0.01);
    assert_eq!((out[0] * 10.0).round() as i32, 4);
}

#[cfg(any(feature = "ogg", feature = "mp3"))]
fn sine_check(w: &Wav, rate: u32) {
    assert_eq!(w.sample_rate(), rate);
    assert!(
        (w.duration() - 0.25).abs() < 0.03,
        "duration {}",
        w.duration()
    );
    // 440 Hz: about 2 * 440 * 0.25 = 220 zero crossings
    let (a, mut m) = Audio::with_manual_output(rate);
    a.play(w);
    let mut out = vec![0.0; (rate as usize / 5) * 2];
    m.render(&mut out);
    let left: Vec<f32> = out.chunks(2).map(|c| c[0]).collect();
    let crossings = left
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    assert!(
        (170..=180).contains(&crossings),
        "crossings in 0.2 s: {crossings}"
    );
    let rms = (left.iter().map(|x| x * x).sum::<f32>() / left.len() as f32).sqrt();
    assert!((rms - 0.088).abs() < 0.01, "rms {rms}");
}

#[cfg(feature = "ogg")]
#[test]
fn loads_ogg() {
    let w = Wav::from_bytes(include_bytes!("../tests/data/tone.ogg")).unwrap();
    assert_eq!(w.channels(), 1);
    sine_check(&w, 22050);
    let s = Wav::from_bytes(include_bytes!("../tests/data/stereo.ogg")).unwrap();
    assert_eq!((s.channels(), s.sample_rate()), (2, 44100));
}

#[cfg(feature = "mp3")]
#[test]
fn loads_mp3() {
    let w = Wav::from_bytes(include_bytes!("../tests/data/tone.mp3")).unwrap();
    assert_eq!(w.channels(), 1);
    sine_check(&w, 22050);
}

#[cfg(any(feature = "ogg", feature = "mp3"))]
fn stream_matches_wav(bytes: &[u8]) {
    // streamed and fully decoded give the same samples
    let w = Wav::from_bytes(bytes).unwrap();
    let s = Stream::from_bytes(bytes).unwrap();
    let mut wi = w.instance(RATE);
    let mut si = s.instance(RATE);
    let mut a = vec![0.0; RATE as usize];
    let mut b = vec![0.0; RATE as usize];
    let (na, nb) = (wi.render(&mut a), si.render(&mut b));
    assert_eq!(na, nb, "same length");
    assert!(na > RATE as usize / 5);
    let maxd = a
        .iter()
        .zip(&b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max);
    assert!(maxd < 1e-5, "max difference {maxd}");
    // looping keeps going
    let mut si = s.instance(RATE);
    si.set_looping(true);
    let mut big = vec![0.0; RATE as usize * 2];
    assert_eq!(
        si.render(&mut big),
        RATE as usize,
        "a looping stream fills the whole second"
    );
    assert!(
        big[big.len() - 4000..].iter().any(|x| x.abs() > 0.01),
        "still sounding after the loop"
    );
}

#[cfg(feature = "ogg")]
#[test]
fn streams_ogg() {
    stream_matches_wav(include_bytes!("../tests/data/tone.ogg"));
    stream_matches_wav(include_bytes!("../tests/data/stereo.ogg"));
}

#[cfg(feature = "mp3")]
#[test]
fn streams_mp3() {
    stream_matches_wav(include_bytes!("../tests/data/tone.mp3"));
}

#[test]
fn unsupported_formats_are_errors() {
    assert!(Wav::from_bytes(b"\x00\x01\x02\x03 garbage").is_err());
    #[cfg(any(feature = "ogg", feature = "mp3"))]
    assert!(Stream::from_bytes(b"OggS but not really").is_err());
}

fn sine(freq: f32, seconds: f32) -> Wav {
    let n = (seconds * RATE as f32) as usize;
    Wav::from_samples(
        (0..n)
            .map(|i| (i as f32 * freq * std::f32::consts::TAU / RATE as f32).sin() * 0.5)
            .collect(),
        1,
        RATE,
    )
}

#[cfg_attr(not(feature = "effects"), allow(dead_code))]
fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// a single sample of 1.0
#[cfg_attr(not(feature = "effects"), allow(dead_code))]
fn click() -> Wav {
    Wav::from_samples(vec![1.0], 1, RATE)
}

#[cfg(feature = "effects")]
mod fx {
    use super::*;
    use crate::effects::{self, Echo, Filter, Reverb};

    /// level (dB) of a looping sine through an effect on its voice, relative to the input
    fn through(freq: f32, f: Filter) -> f32 {
        let (a, mut m) = Audio::with_manual_output(RATE);
        // quiet enough that resonance peaks do not clip
        let h = a.play_with(
            &sine(freq, 1.0),
            PlayOptions::new().looping(true).volume(0.1),
        );
        a.set_effect(h, 0, f);
        render(&mut m, 0.1);
        let out = render(&mut m, 0.2);
        20.0 * (rms(&out) / (0.05 / 2f32.sqrt())).log10()
    }

    #[test]
    fn low_pass_and_high_pass() {
        let lp = |f| through(f, Filter::low_pass(1000.0));
        let hp = |f| through(f, Filter::high_pass(1000.0));
        assert!(lp(100.0).abs() < 0.1, "LP passes the lows: {}", lp(100.0));
        assert!(
            (lp(1000.0) + 3.0).abs() < 0.2,
            "LP is -3 dB at the cutoff: {}",
            lp(1000.0)
        );
        assert!(
            lp(8000.0) < -34.0,
            "LP cuts 3 octaves up by ~36 dB: {}",
            lp(8000.0)
        );
        assert!(
            hp(10000.0).abs() < 0.1,
            "HP passes the highs: {}",
            hp(10000.0)
        );
        assert!(hp(125.0) < -34.0, "HP cuts 3 octaves down: {}", hp(125.0));
        assert!(through(1000.0, Filter::band_pass(1000.0)).abs() < 0.2);
        assert!(through(1000.0, Filter::notch(1000.0)) < -30.0);
        // resonance boosts the cutoff
        let res = through(1000.0, Filter::low_pass(1000.0).resonance(4.0));
        assert!(
            (res - 12.0).abs() < 0.3,
            "resonance boosts the cutoff by Q: {res}"
        );
    }

    #[test]
    fn wet_zero_is_untouched_and_fades_are_smooth() {
        let (a, mut m) = Audio::with_manual_output(RATE);
        let (b, mut m2) = Audio::with_manual_output(RATE);
        let s = sine(5000.0, 1.0);
        let h = a.play_with(&s, PlayOptions::new().looping(true));
        b.play_with(&s, PlayOptions::new().looping(true));
        a.set_effect(h, 0, Filter::low_pass(200.0));
        a.set_effect_param(h, 0, effects::WET, 0.0);
        assert!(
            render(&mut m, 0.05) == render(&mut m2, 0.05),
            "WET 0 bypasses the effect"
        );
        // fade the filter in, then its cutoff down: the level drops, step by step
        a.fade_effect_param(h, 0, effects::WET, 1.0, 0.05);
        a.set_effect_param(h, 0, Filter::CUTOFF, 20000.0);
        a.fade_effect_param(h, 0, Filter::CUTOFF, 200.0, 0.2);
        let out = render(&mut m, 0.3);
        // 10 ms pieces
        let block = |i: usize| rms(&out[i * 960..i * 960 + 960]);
        assert!(
            block(0) > 0.3 && block(29) < 0.01,
            "{} {}",
            block(0),
            block(29)
        );
        for i in 1..30 {
            assert!(block(i) <= block(i - 1) * 1.05, "no jumps up at block {i}");
        }
    }

    #[test]
    fn echo_repeats_and_keeps_ringing_after_the_sound() {
        let (a, mut m) = Audio::with_manual_output(RATE);
        a.set_effect(Bus::MAIN, 0, Echo::new(0.1, 0.5));
        let h = a.play(&click());
        let out = render(&mut m, 0.35);
        assert!(!a.is_playing(h), "the click has ended, the echo goes on");
        let at = |s: f32| out[(s * RATE as f32) as usize * 2];
        assert_eq!(
            (at(0.0), at(0.1), at(0.2), at(0.3)),
            (1.0, 0.5, 0.25, 0.125)
        );
        assert_eq!(
            out.iter().filter(|v| **v != 0.0).count(),
            8,
            "only the repeats, in both channels"
        );
        // damping makes each repeat duller (spread out, lower peak)
        let (a, mut m) = Audio::with_manual_output(RATE);
        a.set_effect(Bus::MAIN, 0, Echo::new(0.1, 0.5).damping(0.5));
        a.play(&click());
        let out = render(&mut m, 0.15);
        let peak = out[4800 * 2..].iter().fold(0f32, |p, v| p.max(v.abs()));
        assert!(peak < 0.5 && peak > 0.1, "{peak}");
    }

    #[test]
    fn reverb_rings_then_goes_quiet() {
        let (a, mut m) = Audio::with_manual_output(RATE);
        a.set_effect(Bus::MAIN, 0, Reverb::new().room_size(0.5));
        a.play(&click());
        let early = render(&mut m, 0.3);
        assert!(rms(&early[4800..]) > 1e-3, "it rings");
        let diff: f32 = early[4800..].chunks(2).map(|c| (c[0] - c[1]).abs()).sum();
        assert!(diff > 1.0, "left and right differ: the reverb is wide");
        let later = render(&mut m, 1.0);
        assert!(rms(&later) < rms(&early[4800..]) * 0.1, "and dies away");
        render(&mut m, 2.0);
        assert!(
            render(&mut m, 0.1).iter().all(|v| *v == 0.0),
            "the bus stops running it after the tail"
        );
        // dry 0: only the reverb, no direct sound
        let (a, mut m) = Audio::with_manual_output(RATE);
        a.set_effect(Bus::MAIN, 0, Reverb::new().dry(0.0));
        a.play(&click());
        assert_eq!(render(&mut m, 0.01)[0], 0.0);
    }

    #[test]
    fn buses_flow_into_main() {
        let (a, mut m) = Audio::with_manual_output(RATE);
        let music = a.create_bus().unwrap();
        a.set_effect(Bus::MAIN, 0, Filter::low_pass(200.0));
        a.play_with(
            &sine(8000.0, 1.0),
            PlayOptions::new().bus(music).looping(true),
        );
        render(&mut m, 0.05);
        assert!(
            rms(&render(&mut m, 0.1)) < 0.01,
            "an effect on MAIN applies to every bus"
        );
        a.remove_effect(Bus::MAIN, 0);
        render(&mut m, 0.01);
        assert!(rms(&render(&mut m, 0.1)) > 0.3);
        a.set_bus_volume(Bus::MAIN, 0.5);
        render(&mut m, 0.01);
        assert!(
            (rms(&render(&mut m, 0.1)) - 0.5 / 2f32.sqrt() * 0.5).abs() < 0.01,
            "and so does its volume"
        );
    }
}

mod spatial3d {
    use super::*;

    fn level(a: &Audio, m: &mut Mixer) -> (f32, f32) {
        let _ = a;
        last(&render(m, 0.02))
    }

    #[test]
    fn distance_and_direction() {
        let (a, mut m) = Audio::with_manual_output(RATE);
        let h = a.play_with(
            &dc(0.5, 1.0),
            PlayOptions::new()
                .looping(true)
                .spatial(Spatial::at([0.0, 0.0, -1.0]).doppler(0.0)),
        );
        assert_eq!(
            level(&a, &mut m),
            (0.5, 0.5),
            "in front, at the minimum distance: as in 2D"
        );
        a.set_position(h, [0.0, 0.0, -4.0]);
        let (l, r) = level(&a, &mut m);
        assert!(
            (l - 0.125).abs() < 1e-6 && l == r,
            "inverse distance: a quarter at 4x: {l}"
        );
        a.set_position(h, [2.0, 0.0, 0.0]);
        let (l, r) = level(&a, &mut m);
        assert!(
            l.abs() < 1e-6 && (r - 0.25 * 2f32.sqrt()).abs() < 1e-5,
            "to the right: {l} {r}"
        );
        // turn to face +X: now it is in front
        a.set_listener([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let (l, r) = level(&a, &mut m);
        assert!(
            (l - 0.25).abs() < 1e-5 && (r - 0.25).abs() < 1e-5,
            "{l} {r}"
        );
        // linear: silent at the maximum distance
        a.set_listener([0.0; 3], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
        a.set_spatial(
            h,
            Spatial::at([0.0, 0.0, -10.0])
                .distance(1.0, 10.0)
                .attenuation(Attenuation::Linear, 1.0),
        );
        assert_eq!(level(&a, &mut m), (0.0, 0.0));
        a.set_position(h, [0.0, 0.0, -5.5]);
        let (l, _) = level(&a, &mut m);
        assert!((l - 0.25).abs() < 1e-5, "half way: {l}");
    }

    #[test]
    fn doppler_raises_the_pitch_of_an_approaching_sound() {
        let crossings = |v: f32| {
            let (a, mut m) = Audio::with_manual_output(RATE);
            a.play_with(
                &sine(1000.0, 1.0),
                PlayOptions::new()
                    .looping(true)
                    .spatial(Spatial::at([0.0, 0.0, -100.0]).velocity([0.0, 0.0, v])),
            );
            let out = render(&mut m, 0.5);
            out.chunks(2)
                .map(|c| c[0])
                .collect::<Vec<_>>()
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count() as f32
                / 0.5
        };
        let still = crossings(0.0);
        let coming = crossings(34.3);
        let going = crossings(-34.3);
        assert!((still - 1000.0).abs() <= 2.0, "{still}");
        assert!(
            (coming / still - 343.0 / (343.0 - 34.3)).abs() < 0.01,
            "{coming}"
        );
        assert!(
            (going / still - 343.0 / (343.0 + 34.3)).abs() < 0.01,
            "{going}"
        );
    }
}
