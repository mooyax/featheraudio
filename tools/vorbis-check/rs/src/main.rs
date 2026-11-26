//! Writes "channels rate\n" and then every sample (interleaved f32, little endian), like cref.c.
use std::io::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).unwrap();
    let rounds = a.get(3).map_or(1, |r| r.parse().unwrap());
    let mut o = std::fs::File::create(&a[2]).unwrap();
    match featheraudio::ogg_decode_raw(&data, rounds) {
        None => o.write_all(b"ERR\n").unwrap(),
        Some((ch, rate, s)) => {
            writeln!(o, "{ch} {rate}").unwrap();
            let b: Vec<u8> = s.iter().flat_map(|x| x.to_le_bytes()).collect();
            o.write_all(&b).unwrap();
        }
    }
}
