//! Plays a sound file on the default output device.
//!
//!     cargo run --release --example play -- sound.wav
//!     cargo run --release --example play -- music.ogg     (or .mp3: streamed while playing)
//!     cargo run --release --example play -- song.xm       (MOD / IT / S3M / XM: to the end of the song)
//!
//! Press Enter to fade out and stop.

use featheraudio::{Audio, PlayOptions, Source, Wav};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: play <file.wav | .ogg | .mp3 | .mod | .it | .s3m | .xm>")?;
    let bytes = std::fs::read(&path)?;
    let audio = Audio::new()?;
    println!("output: {} Hz", audio.sample_rate());

    let source: Box<dyn Source> = if bytes.starts_with(b"RIFF") {
        let w = Wav::from_bytes(&bytes)?;
        println!("{path}: WAV, {:.2} s, {} ch, {} Hz", w.duration(), w.channels(), w.sample_rate());
        Box::new(w)
    } else if let Some(s) = stream(&bytes) {
        println!("{path}: streamed, {} ch, {} Hz", s.0, s.1);
        s.2
    } else {
        tracker(&bytes, &path)?
    };
    let h = audio.play_with(source.as_ref(), PlayOptions::new().protected(true));

    // wait for Enter (or the end of the sound) on another thread
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
        let _ = tx.send(());
    });
    println!("playing, press Enter to stop");
    while audio.is_playing(h) {
        if rx.try_recv().is_ok() {
            audio.fade_volume(h, 0.0, 1.0);
            audio.stop_after(h, 1.0);
            while audio.is_playing(h) {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(any(feature = "ogg", feature = "mp3"))]
fn stream(bytes: &[u8]) -> Option<(usize, u32, Box<dyn Source>)> {
    let s = featheraudio::Stream::from_bytes(bytes).ok()?;
    Some((s.channels(), s.sample_rate(), Box::new(s)))
}

#[cfg(not(any(feature = "ogg", feature = "mp3")))]
fn stream(_: &[u8]) -> Option<(usize, u32, Box<dyn Source>)> {
    None
}

#[cfg(any(feature = "mod", feature = "it", feature = "xm"))]
fn tracker(bytes: &[u8], path: &str) -> Result<Box<dyn Source>, Box<dyn std::error::Error>> {
    let t = featheraudio::Tracker::from_bytes(bytes)?;
    println!("{path}: \"{}\" [{}]", t.title(), t.format());
    Ok(Box::new(t))
}

#[cfg(not(any(feature = "mod", feature = "it", feature = "xm")))]
fn tracker(_: &[u8], path: &str) -> Result<Box<dyn Source>, Box<dyn std::error::Error>> {
    Err(format!("{path}: unsupported format (or its feature is disabled)").into())
}
