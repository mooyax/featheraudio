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
    let h = a.play_with(&dc(0.5, 1.0), PlayOptions::new().volume(0.5).pan(-1.0).bus(bus).looping(true));
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
    assert!(out.chunks(2).map(|c| c[0]).collect::<Vec<_>>().windows(2).all(|w| w[1] <= w[0] + 1e-6));
}

#[test]
fn stop_and_pause_do_not_click() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let h = a.play_with(&dc(1.0, 1.0), PlayOptions::new().looping(true));
    render(&mut m, 0.01);
    a.set_paused(h, true);
    let out = render(&mut m, 0.02);
    let steps: f32 = out.chunks(2).map(|c| c[0]).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(steps < 0.02, "pausing should ramp down, largest step {steps}");
    assert_eq!(last(&out), (0.0, 0.0));
    assert!(a.is_playing(h), "a paused sound is still alive");
    a.set_paused(h, false);
    let out = render(&mut m, 0.02);
    assert_eq!(last(&out), (1.0, 1.0));
    a.stop(h);
    let out = render(&mut m, 0.02);
    let steps: f32 = out.chunks(2).map(|c| c[0]).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(steps < 0.02, "stopping should ramp down, largest step {steps}");
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
    assert!(!a.is_playing(first), "the oldest unprotected voice is replaced");
    assert!(a.is_playing(second));
    a.set_protected(second, true);
    let third = a.play(&s);
    render(&mut m, 0.01);
    assert!(!a.is_playing(third), "nothing to replace: the new sound is dropped");
    assert_eq!(a.active_voices(), 2);
}

#[test]
fn many_sounds_mix_and_clip() {
    let (a, mut m) = Audio::with_manual_output(RATE);
    let s = dc(0.3, 0.1);
    for _ in 0..5 {
        a.play(&s);
    }
    assert_eq!(last(&render(&mut m, 0.01)), (1.0, 1.0), "the sum 1.5 is clipped to 1.0");
    let h = a.play_with(&dc(0.5, 1.0), PlayOptions::new().speed(2.0));
    render(&mut m, 0.6);
    assert!(!a.is_playing(h), "double speed: a 1 s sound ends after 0.5 s");
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
    assert!(out[out.len() - 200..].iter().all(|x| x.abs() < 1e-6), "faded out");
    assert!(a.is_playing(h), "the song keeps playing (silently) until stopped");
}
