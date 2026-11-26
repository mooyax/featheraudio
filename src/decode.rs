//! Compressed audio formats: OGG Vorbis (our port of stb_vorbis) and MP3 (nanomp3), behind the `ogg` and
//! `mp3` features. Both decode from memory, frame by frame, so they can be streamed.

use crate::Error;
use std::sync::Arc;

/// The container / codec of a sound file, from its first bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Wav,
    Ogg,
    Mp3,
    Unknown,
}

pub(crate) fn detect(b: &[u8]) -> Format {
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        Format::Wav
    } else if b.starts_with(b"OggS") {
        Format::Ogg
    } else if b.starts_with(b"ID3") || (b.len() >= 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0) {
        Format::Mp3
    } else {
        Format::Unknown
    }
}

/// A decoder that produces interleaved samples, a packet / frame at a time.
pub(crate) trait Decoder: Send {
    fn channels(&self) -> usize;
    fn rate(&self) -> u32;
    /// The two source channels used for left and right (both 0 for mono).
    fn stereo_map(&self) -> (usize, usize) {
        match self.channels() {
            1 => (0, 0),
            _ => (0, 1),
        }
    }
    /// Appends the next decoded samples (interleaved, `channels()` per frame) to `out`.
    /// Returns false at the end of the stream (or on an error that ends it).
    fn decode(&mut self, out: &mut Vec<f32>) -> bool;
    /// Goes back to the start. Returns false if that is not possible.
    fn rewind(&mut self) -> bool;
}

/// Opens a decoder for OGG Vorbis or MP3 data.
#[allow(unused_variables)]
pub(crate) fn open(data: Arc<[u8]>) -> Result<Box<dyn Decoder>, Error> {
    match detect(&data) {
        #[cfg(feature = "ogg")]
        Format::Ogg => Ok(Box::new(OggDecoder::new(data)?)),
        #[cfg(feature = "mp3")]
        Format::Mp3 => Ok(Box::new(Mp3Decoder::new(data)?)),
        #[cfg(not(feature = "ogg"))]
        Format::Ogg => Err(Error::Format("OGG files need the `ogg` feature".into())),
        #[cfg(not(feature = "mp3"))]
        Format::Mp3 => Err(Error::Format("MP3 files need the `mp3` feature".into())),
        Format::Wav => Err(Error::Format("WAV files are loaded with Wav::from_bytes".into())),
        Format::Unknown => Err(Error::Format("not a WAV, OGG or MP3 file".into())),
    }
}

/// Decodes a whole OGG / MP3 file into stereo-or-mono samples: (samples, channels, rate).
pub(crate) fn decode_all(data: &[u8]) -> Result<(Vec<f32>, usize, u32), Error> {
    let mut d = open(data.into())?;
    let (ch, rate, (l, r)) = (d.channels(), d.rate(), d.stereo_map());
    let mut all = Vec::new();
    while d.decode(&mut all) {}
    if all.is_empty() {
        return Err(Error::Format("no audio in the file".into()));
    }
    Ok(match ch {
        1 => (all, 1, rate),
        2 => (all, 2, rate),
        _ => (all.chunks_exact(ch).flat_map(|f| [f[l], f[r]]).collect(), 2, rate),
    })
}

// ---------------------------------------------------------------------------------------------

#[cfg(feature = "ogg")]
struct OggDecoder {
    v: crate::vorbis::Vorbis,
}

#[cfg(feature = "ogg")]
impl OggDecoder {
    fn new(data: Arc<[u8]>) -> Result<Self, Error> {
        crate::vorbis::Vorbis::new(data).map(|v| OggDecoder { v }).ok_or_else(|| Error::Format("OGG: not a Vorbis stream, or broken".into()))
    }
}

#[cfg(feature = "ogg")]
impl Decoder for OggDecoder {
    fn channels(&self) -> usize {
        self.v.channels
    }

    fn rate(&self) -> u32 {
        self.v.sample_rate
    }

    fn stereo_map(&self) -> (usize, usize) {
        // Vorbis channel order: 3 ch and more start with left, centre, right
        match self.channels() {
            1 => (0, 0),
            2 => (0, 1),
            _ => (0, 2),
        }
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> bool {
        // frames can be empty (the first one, and at block size changes); skip those
        loop {
            match self.v.next_frame() {
                Some(0) => continue,
                Some(n) => {
                    let ch = self.v.channels;
                    let start = out.len();
                    out.resize(start + n * ch, 0.0);
                    for c in 0..ch {
                        for (i, &x) in self.v.channel(c).iter().enumerate() {
                            out[start + i * ch + c] = x;
                        }
                    }
                    return true;
                }
                None => return false,
            }
        }
    }

    fn rewind(&mut self) -> bool {
        self.v.rewind()
    }
}

/// For tests against stb_vorbis: decodes all channels, unchanged and interleaved, `rounds`
/// times (going back to the start in between): (channels, rate, samples). Not part of the API.
#[cfg(feature = "ogg")]
#[doc(hidden)]
pub fn ogg_decode_raw(data: &[u8], rounds: usize) -> Option<(usize, u32, Vec<f32>)> {
    let mut d = OggDecoder::new(data.into()).ok()?;
    let mut out = Vec::new();
    for r in 0..rounds {
        if r > 0 && !d.rewind() {
            break;
        }
        while d.decode(&mut out) {}
    }
    Some((d.channels(), d.rate(), out))
}

#[cfg(feature = "mp3")]
struct Mp3Decoder {
    reader: nanomp3::Reader<std::io::Cursor<Arc<[u8]>>, f32>,
    channels: usize,
    rate: u32,
}

#[cfg(feature = "mp3")]
impl Mp3Decoder {
    fn new(data: Arc<[u8]>) -> Result<Self, Error> {
        let reader = nanomp3::Reader::<_, f32>::new(std::io::Cursor::new(data)).map_err(|e| Error::Format(format!("MP3: {e:?}")))?;
        let channels = reader.channels().map(|c| c.num() as usize).ok_or_else(|| Error::Format("MP3: no audio".into()))?;
        let rate = reader.sample_rate();
        Ok(Mp3Decoder { reader, channels, rate })
    }
}

#[cfg(feature = "mp3")]
impl Decoder for Mp3Decoder {
    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> bool {
        match self.reader.read_frame() {
            Ok(Some(s)) if !s.is_empty() => {
                out.extend_from_slice(s);
                true
            }
            _ => false,
        }
    }

    fn rewind(&mut self) -> bool {
        self.reader.seek(0).is_ok()
    }
}

/// For benchmarks against stb_vorbis: decodes every frame and only counts the samples.
/// Not part of the API.
#[cfg(feature = "ogg")]
#[doc(hidden)]
pub fn ogg_decode_count(data: &[u8]) -> usize {
    let Some(mut v) = crate::vorbis::Vorbis::new(data.into()) else { return 0 };
    let mut n = 0;
    while let Some(len) = v.next_frame() {
        n += len;
    }
    n
}
