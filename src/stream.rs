//! Streamed sounds: OGG / MP3 decoded a little at a time while playing.

use crate::decode::{self, Decoder};
use crate::source::{Instance, Source};
use crate::Error;
use std::sync::Arc;

/// A compressed sound (OGG Vorbis or MP3) that is decoded while it plays, instead of all at
/// once. Use it for music and other long sounds: only the compressed file stays in memory
/// (a minute of decoded stereo audio would take about 23 MB).
///
/// Decoding happens on the audio thread, a packet / frame at a time; unlike the rest of the
/// mixer it allocates small buffers there. Cloning is cheap (the file data is shared).
#[derive(Clone)]
pub struct Stream {
    data: Arc<[u8]>,
    channels: usize,
    rate: u32,
}

impl Stream {
    /// Opens an OGG Vorbis (`ogg` feature) or MP3 (`mp3` feature) file held in memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Stream, Error> {
        Self::from_shared(bytes.into())
    }

    /// Like [`from_bytes`](Self::from_bytes), without copying the data.
    pub fn from_vec(bytes: Vec<u8>) -> Result<Stream, Error> {
        Self::from_shared(bytes.into())
    }

    fn from_shared(data: Arc<[u8]>) -> Result<Stream, Error> {
        let d = decode::open(data.clone())?;
        Ok(Stream { channels: d.channels(), rate: d.rate(), data })
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl Source for Stream {
    fn instance(&self, rate: u32) -> Box<dyn Instance> {
        match decode::open(self.data.clone()) {
            Ok(dec) => {
                let step = dec.rate() as f64 / rate as f64;
                Box::new(StreamInstance {
                    map: dec.stereo_map(),
                    channels: dec.channels(),
                    dec,
                    buf: Vec::with_capacity(COMPACT_AT * 4),
                    scratch: Vec::with_capacity(8192),
                    pos: 0.0,
                    base_step: step,
                    step,
                    looping: false,
                    eof: false,
                })
            }
            Err(_) => Box::new(Ended),
        }
    }
}

struct Ended;

impl Instance for Ended {
    fn render(&mut self, _out: &mut [f32]) -> usize {
        0
    }
}

/// drop frames that have been played once this many have piled up in the buffer
const COMPACT_AT: usize = 8192;

struct StreamInstance {
    dec: Box<dyn Decoder>,
    channels: usize,
    map: (usize, usize),
    /// decoded stereo frames, interleaved
    buf: Vec<f32>,
    scratch: Vec<f32>,
    /// position in `buf`, in frames
    pos: f64,
    base_step: f64,
    step: f64,
    looping: bool,
    eof: bool,
}

impl StreamInstance {
    fn frames(&self) -> usize {
        self.buf.len() / 2
    }

    /// decodes more frames into `buf`; false when there is nothing more
    fn fill(&mut self) -> bool {
        loop {
            self.scratch.clear();
            if self.dec.decode(&mut self.scratch) {
                let (ch, (l, r)) = (self.channels.max(1), self.map);
                for f in self.scratch.chunks_exact(ch) {
                    self.buf.push(f[l]);
                    self.buf.push(f[r]);
                }
                if !self.scratch.is_empty() {
                    return true;
                }
            } else if self.looping && self.dec.rewind() {
                continue;
            } else {
                self.eof = true;
                return false;
            }
        }
    }
}

impl Instance for StreamInstance {
    fn render(&mut self, out: &mut [f32]) -> usize {
        let mut done = 0;
        for o in out.chunks_exact_mut(2) {
            let i = self.pos as usize;
            // need frames i and i + 1 for the interpolation
            while i + 1 >= self.frames() && !self.eof {
                if !self.fill() {
                    break;
                }
            }
            if i >= self.frames() {
                break; // the end
            }
            let j = (i + 1).min(self.frames() - 1);
            let fr = (self.pos - i as f64) as f32;
            o[0] = self.buf[i * 2] + (self.buf[j * 2] - self.buf[i * 2]) * fr;
            o[1] = self.buf[i * 2 + 1] + (self.buf[j * 2 + 1] - self.buf[i * 2 + 1]) * fr;
            self.pos += self.step;
            done += 1;
        }
        // forget what has been played
        let played = self.pos as usize;
        if played >= COMPACT_AT {
            let n = played.min(self.frames());
            self.buf.drain(..n * 2);
            self.pos -= n as f64;
        }
        done
    }

    fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
        if looping && self.eof {
            // the decoder reached the end already; start it over for the loop
            self.eof = !self.dec.rewind();
        }
    }

    fn set_speed(&mut self, speed: f32) {
        self.step = self.base_step * speed.max(0.0) as f64;
    }
}
