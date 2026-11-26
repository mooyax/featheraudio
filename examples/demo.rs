//! Renders a short demo offline (no audio device needed) and writes it to `demo.wav`:
//! background music on a music bus, "pew" sound effects panned around on an effects bus,
//! a quieter stretch for the music, and a fade-out at the end.
//!
//!     cargo run --release --example demo -- [song.xm]
//!
//! Without a module file, the music is a generated drone.

use featheraudio::{Audio, PlayOptions, Source, Wav};

const RATE: u32 = 48000;

/// a falling square-wave "pew"
fn pew() -> Wav {
    let n = (RATE as f32 * 0.18) as usize;
    let mut phase = 0.0f32;
    let s = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            phase += (1400.0 - 1100.0 * t) / RATE as f32;
            let sq = if phase.fract() < 0.5 { 1.0 } else { -1.0 };
            sq * 0.35 * (1.0 - t) * (1.0 - t)
        })
        .collect();
    Wav::from_samples(s, 1, RATE)
}

/// two slowly beating sine waves, stereo, looping cleanly after 2 seconds
fn drone() -> Wav {
    let n = RATE as usize * 2;
    let mut s = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let a = (t * 110.0 * std::f32::consts::TAU).sin();
        let b = (t * 165.0 * std::f32::consts::TAU).sin();
        s.push(0.25 * a + 0.1 * b);
        s.push(0.1 * a + 0.25 * b);
    }
    Wav::from_samples(s, 2, RATE)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let music: Box<dyn Source> = match std::env::args().nth(1) {
        Some(p) => tracker(&std::fs::read(p)?)?,
        None => Box::new(drone()),
    };
    let (audio, mut mixer) = Audio::with_manual_output(RATE);
    let music_bus = audio.create_bus().unwrap();
    let sfx_bus = audio.create_bus().unwrap();

    let bgm = audio.play_with(music.as_ref(), PlayOptions::new().bus(music_bus).looping(true).protected(true).volume(0.0));
    audio.fade_volume(bgm, 0.8, 1.0); // fade in

    let pew = pew();
    let mut out = Vec::new();
    let mut block = vec![0.0f32; RATE as usize / 10 * 2]; // 0.1 s
    for step in 0..120 {
        // 12 seconds
        let t = step as f32 / 10.0;
        if (2.0..8.0).contains(&t) && step % 4 == 0 {
            // pews sweeping from left to right, slightly higher each time
            let k = (t - 2.0) / 6.0;
            audio.play_with(&pew, PlayOptions::new().bus(sfx_bus).pan(k * 2.0 - 1.0).speed(1.0 + k * 0.5));
        }
        if step == 40 {
            audio.fade_bus_volume(music_bus, 0.4, 0.5); // duck the music under the effects
        }
        if step == 80 {
            audio.fade_bus_volume(music_bus, 1.0, 0.5);
        }
        if step == 100 {
            audio.fade_volume(bgm, 0.0, 2.0);
            audio.stop_after(bgm, 2.0);
        }
        mixer.render(&mut block);
        out.extend_from_slice(&block);
    }
    write_wav("demo.wav", &out)?;
    println!("wrote demo.wav ({:.1} s)", out.len() as f32 / 2.0 / RATE as f32);
    Ok(())
}

#[cfg(any(feature = "mod", feature = "it", feature = "xm"))]
fn tracker(bytes: &[u8]) -> Result<Box<dyn Source>, Box<dyn std::error::Error>> {
    Ok(Box::new(featheraudio::Tracker::from_bytes(bytes)?))
}

#[cfg(not(any(feature = "mod", feature = "it", feature = "xm")))]
fn tracker(_: &[u8]) -> Result<Box<dyn Source>, Box<dyn std::error::Error>> {
    Err("tracker modules need the `tracker` feature".into())
}

fn write_wav(path: &str, s: &[f32]) -> std::io::Result<()> {
    let data: Vec<u8> = s.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::with_capacity(44 + data.len());
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(path, b)
}
