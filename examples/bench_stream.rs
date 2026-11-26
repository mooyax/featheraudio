//! How long it takes to stream-decode and resample a file to 48 kHz.
use featheraudio::{Source, Stream};
fn main() {
    for p in std::env::args().skip(1) {
        let s = Stream::from_vec(std::fs::read(&p).unwrap()).unwrap();
        let mut i = s.instance(48000);
        let mut buf = vec![0.0f32; 2048];
        let t = std::time::Instant::now();
        let mut frames = 0;
        loop {
            let n = i.render(&mut buf);
            frames += n;
            if n < 1024 {
                break;
            }
        }
        let secs = frames as f64 / 48000.0;
        println!("{p}: {secs:.1} s of audio in {:.3} s ({:.2}% of one core)", t.elapsed().as_secs_f64(), 100.0 * t.elapsed().as_secs_f64() / secs);
    }
}
