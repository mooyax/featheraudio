//! How much CPU the effects and 3D take: mixes 10 s of 32 looping voices at 48 kHz, with and
//! without effects.
use featheraudio::effects::{Echo, Filter, Reverb};
use featheraudio::{Audio, Bus, PlayOptions, Spatial, Wav};

fn run(label: &str, setup: impl Fn(&Audio, &[featheraudio::Handle])) {
    let rate = 48000;
    let (a, mut m) = Audio::with_manual_output(rate);
    let tone = Wav::from_samples(
        (0..rate).map(|i| (i as f32 * 0.05).sin() * 0.02).collect(),
        1,
        rate,
    );
    a.set_max_voices(64);
    let hs: Vec<_> = (0..32)
        .map(|i| {
            a.play_with(
                &tone,
                PlayOptions::new()
                    .looping(true)
                    .speed(0.5 + i as f32 / 32.0),
            )
        })
        .collect();
    setup(&a, &hs);
    let mut buf = vec![0.0f32; 1024];
    let t = std::time::Instant::now();
    for _ in 0..(rate as usize * 10 / 512) {
        m.render(&mut buf);
    }
    let s = t.elapsed().as_secs_f64();
    println!("{label:44} {:6.2}% of one core", s / 10.0 * 100.0);
}

fn main() {
    run("32 voices", |_, _| {});
    run("32 voices + reverb on MAIN", |a, _| {
        a.set_effect(Bus::MAIN, 0, Reverb::new())
    });
    run("32 voices + echo on MAIN", |a, _| {
        a.set_effect(Bus::MAIN, 0, Echo::new(0.3, 0.5))
    });
    run("32 voices, each with a low-pass filter", |a, hs| {
        for h in hs {
            a.set_effect(*h, 0, Filter::low_pass(2000.0));
        }
    });
    run("32 voices, all 3D (moving, with Doppler)", |a, hs| {
        for (i, h) in hs.iter().enumerate() {
            a.set_spatial(
                *h,
                Spatial::at([i as f32, 0.0, -5.0]).velocity([3.0, 0.0, 0.0]),
            );
        }
    });
}
