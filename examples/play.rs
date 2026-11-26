//! Plays sound files on the default output device.
//!
//!     cargo run --release --example play -- sound.wav
//!     cargo run --release --example play -- song.xm
//!
//! WAV files play once; tracker modules (MOD / IT / S3M / XM) loop until you press Enter.

use featheraudio::{Audio, PlayOptions, Source, Wav};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: play <file.wav | file.mod/.it/.s3m/.xm>")?;
    let bytes = std::fs::read(&path)?;
    let audio = Audio::new()?;
    println!("output: {} Hz", audio.sample_rate());

    let (source, is_music): (Box<dyn Source>, bool) = match Wav::from_bytes(&bytes) {
        Ok(w) => {
            println!("{path}: {:.2} s, {} ch, {} Hz", w.duration(), w.channels(), w.sample_rate());
            (Box::new(w), false)
        }
        Err(_) => load_tracker(&bytes, &path)?,
    };
    let h = audio.play_with(source.as_ref(), PlayOptions::new().protected(is_music));
    if is_music {
        println!("playing, press Enter to fade out and quit");
        let mut s = String::new();
        std::io::stdin().read_line(&mut s)?;
        audio.fade_volume(h, 0.0, 1.0);
        audio.stop_after(h, 1.0);
    }
    while audio.is_playing(h) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

#[cfg(any(feature = "mod", feature = "it", feature = "xm"))]
fn load_tracker(bytes: &[u8], path: &str) -> Result<(Box<dyn Source>, bool), Box<dyn std::error::Error>> {
    let t = featheraudio::Tracker::from_bytes(bytes)?;
    println!("{path}: \"{}\" [{}]", t.title(), t.format());
    Ok((Box::new(t), true))
}

#[cfg(not(any(feature = "mod", feature = "it", feature = "xm")))]
fn load_tracker(_: &[u8], path: &str) -> Result<(Box<dyn Source>, bool), Box<dyn std::error::Error>> {
    Err(format!("{path}: not a WAV file (tracker modules need the `tracker` feature)").into())
}
