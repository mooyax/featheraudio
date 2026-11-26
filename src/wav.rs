//! In-memory samples, loaded from WAV / OGG / MP3 files or created from raw samples.

use crate::source::{Instance, Source};
use crate::Error;
use std::sync::Arc;

/// A sound held in memory, decoded once: loaded from a WAV, OGG Vorbis (`ogg` feature) or MP3
/// (`mp3` feature) file, or created from samples. Best for sound effects; for long music use
/// [`Stream`](crate::Stream), which decodes while playing.
///
/// Cloning is cheap (the samples are shared).
#[derive(Clone)]
pub struct Wav {
    /// mono, or interleaved stereo
    data: Arc<[f32]>,
    channels: usize,
    rate: u32,
    frames: usize,
    loop_start: usize,
}

impl Wav {
    /// Loads a sound file, telling the format from its contents:
    /// - WAV: 8 / 16 / 24 / 32-bit integer or 32 / 64-bit float PCM
    /// - OGG Vorbis, with the `ogg` feature
    /// - MP3, with the `mp3` feature
    ///
    /// More than two channels are reduced to the front left and right.
    pub fn from_bytes(bytes: &[u8]) -> Result<Wav, Error> {
        if crate::decode::detect(bytes) != crate::decode::Format::Wav {
            let (samples, channels, rate) = crate::decode::decode_all(bytes)?;
            return Ok(Wav::from_samples(samples, channels, rate));
        }
        let (fmt, data) = parse_riff(bytes)?;
        let samples = decode(&fmt, data)?;
        let keep = fmt.channels.min(2) as usize;
        let samples = if fmt.channels as usize == keep {
            samples
        } else {
            samples.chunks_exact(fmt.channels as usize).flat_map(|f| f[..keep].to_vec()).collect()
        };
        Ok(Wav::from_samples(samples, keep, fmt.rate))
    }

    /// Creates a sound from samples (-1.0..1.0): mono, or interleaved stereo with `channels` = 2.
    pub fn from_samples(samples: Vec<f32>, channels: usize, rate: u32) -> Wav {
        assert!(channels == 1 || channels == 2, "channels must be 1 or 2");
        let frames = samples.len() / channels;
        Wav { data: samples.into(), channels, rate: rate.max(1), frames, loop_start: 0 }
    }

    /// Length in seconds.
    pub fn duration(&self) -> f64 {
        self.frames as f64 / self.rate as f64
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Where a looping sound starts over, in seconds from the beginning (default 0).
    pub fn set_loop_point(&mut self, seconds: f64) {
        self.loop_start = ((seconds * self.rate as f64) as usize).min(self.frames.saturating_sub(1));
    }
}

impl Source for Wav {
    fn instance(&self, rate: u32) -> Box<dyn Instance> {
        Box::new(WavInstance {
            wav: self.clone(),
            pos: 0.0,
            base_step: self.rate as f64 / rate as f64,
            step: self.rate as f64 / rate as f64,
            looping: false,
        })
    }
}

struct WavInstance {
    wav: Wav,
    /// position in frames
    pos: f64,
    base_step: f64,
    step: f64,
    looping: bool,
}

impl Instance for WavInstance {
    fn render(&mut self, out: &mut [f32]) -> usize {
        let w = &self.wav;
        let (frames, ch, d) = (w.frames, w.channels, &w.data[..]);
        if frames == 0 {
            return 0;
        }
        let loop_len = (frames - w.loop_start) as f64;
        let mut done = 0;
        for o in out.chunks_exact_mut(2) {
            if self.pos >= frames as f64 {
                if !self.looping || loop_len <= 0.0 {
                    break;
                }
                self.pos = w.loop_start as f64 + (self.pos - frames as f64) % loop_len;
            }
            let i = self.pos as usize;
            let fr = (self.pos - i as f64) as f32;
            let j = if i + 1 < frames {
                i + 1
            } else if self.looping {
                w.loop_start
            } else {
                i
            };
            if ch == 1 {
                let s = d[i] + (d[j] - d[i]) * fr;
                o[0] = s;
                o[1] = s;
            } else {
                o[0] = d[i * 2] + (d[j * 2] - d[i * 2]) * fr;
                o[1] = d[i * 2 + 1] + (d[j * 2 + 1] - d[i * 2 + 1]) * fr;
            }
            self.pos += self.step;
            done += 1;
        }
        done
    }

    fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    fn set_speed(&mut self, speed: f32) {
        self.step = self.base_step * speed.max(0.0) as f64;
    }
}

// ---------------------------------------------------------------------------------------------
// WAV parsing

struct Fmt {
    /// 1 = integer PCM, 3 = IEEE float
    tag: u16,
    channels: u16,
    rate: u32,
    bits: u16,
}

fn u16le(d: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([d[p], d[p + 1]])
}

fn u32le(d: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

fn parse_riff(b: &[u8]) -> Result<(Fmt, &[u8]), Error> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(Error::Format("not a WAV file".into()));
    }
    let mut p = 12;
    let mut fmt = None;
    let mut data = None;
    while p + 8 <= b.len() {
        let id = &b[p..p + 4];
        let size = u32le(b, p + 4) as usize;
        let body = &b[p + 8..(p + 8 + size).min(b.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let mut tag = u16le(body, 0);
                if tag == 0xFFFE && body.len() >= 26 {
                    // WAVE_FORMAT_EXTENSIBLE: the real format is the start of the sub-format GUID
                    tag = u16le(body, 24);
                }
                fmt = Some(Fmt { tag, channels: u16le(body, 2), rate: u32le(body, 4), bits: u16le(body, 14) });
            }
            b"data" => data = Some(body),
            _ => {}
        }
        p += 8 + size + (size & 1); // chunks are padded to even sizes
    }
    let fmt = fmt.ok_or_else(|| Error::Format("WAV file without a fmt chunk".into()))?;
    let data = data.ok_or_else(|| Error::Format("WAV file without a data chunk".into()))?;
    if fmt.channels == 0 || fmt.rate == 0 {
        return Err(Error::Format("WAV file with no channels or no sample rate".into()));
    }
    Ok((fmt, data))
}

fn decode(f: &Fmt, d: &[u8]) -> Result<Vec<f32>, Error> {
    let v: Vec<f32> = match (f.tag, f.bits) {
        (1, 8) => d.iter().map(|&x| (x as f32 - 128.0) / 128.0).collect(),
        (1, 16) => d.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        (1, 24) => d.chunks_exact(3).map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8388608.0).collect(),
        (1, 32) => d.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2147483648.0).collect(),
        (3, 32) => d.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        (3, 64) => d.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap()) as f32).collect(),
        (tag, bits) => return Err(Error::Format(format!("unsupported WAV format (format {tag}, {bits} bits)"))),
    };
    let ch = f.channels as usize;
    Ok(v[..v.len() / ch * ch].to_vec())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// a minimal WAV file writer for the tests
    pub fn wav_bytes(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        let align = channels * bits / 8;
        b.extend_from_slice(&(rate * align as u32).to_le_bytes());
        b.extend_from_slice(&align.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data);
        b
    }

    #[test]
    fn formats() {
        let w = Wav::from_bytes(&wav_bytes(1, 1, 22050, 16, &[0x00, 0x40, 0x00, 0xC0])).unwrap();
        assert_eq!((w.channels(), w.sample_rate(), w.frames), (1, 22050, 2));
        assert_eq!(&w.data[..], &[0.5, -0.5]);
        let w = Wav::from_bytes(&wav_bytes(1, 2, 44100, 8, &[192, 64])).unwrap();
        assert_eq!((w.channels(), w.frames), (2, 1));
        assert_eq!(&w.data[..], &[0.5, -0.5]);
        let w = Wav::from_bytes(&wav_bytes(1, 1, 8000, 24, &[0, 0, 0x40])).unwrap();
        assert_eq!(&w.data[..], &[0.5]);
        let w = Wav::from_bytes(&wav_bytes(3, 1, 8000, 32, &0.25f32.to_le_bytes())).unwrap();
        assert_eq!(&w.data[..], &[0.25]);
        // 3 channels -> first two kept
        let w = Wav::from_bytes(&wav_bytes(1, 3, 8000, 8, &[192, 64, 128])).unwrap();
        assert_eq!((w.channels(), &w.data[..]), (2, &[0.5f32, -0.5][..]));
        assert!(Wav::from_bytes(b"not a wav file").is_err());
        assert!(Wav::from_bytes(&wav_bytes(2, 1, 8000, 4, &[0])).is_err());
    }

    #[test]
    fn resample_and_loop() {
        // 4 frames at 1000 Hz played at 2000 Hz -> 8 frames, linear interpolation in between
        let w = Wav::from_samples(vec![0.0, 1.0, 0.0, -1.0], 1, 1000);
        let mut i = w.instance(2000);
        let mut out = vec![0.0; 20];
        assert_eq!(i.render(&mut out), 8);
        let left: Vec<f32> = out.chunks(2).take(8).map(|c| c[0]).collect();
        assert_eq!(left, [0.0, 0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -1.0]);
        let mut i = w.instance(1000);
        i.set_looping(true);
        let mut out = vec![0.0; 20];
        assert_eq!(i.render(&mut out), 10);
        assert_eq!(out[8 * 2], 0.0);
        assert_eq!(out[9 * 2], 1.0);
    }
}
