//! Compressed audio formats: OGG Vorbis (lewton) and MP3 (nanomp3), behind the `ogg` and
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
    data: Arc<[u8]>,
    reader: lewton::inside_ogg::OggStreamReader<std::io::Cursor<Arc<[u8]>>>,
}

#[cfg(feature = "ogg")]
impl OggDecoder {
    fn new(data: Arc<[u8]>) -> Result<Self, Error> {
        let reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(data.clone())).map_err(|e| Error::Format(format!("OGG: {e}")))?;
        Ok(OggDecoder { data, reader })
    }
}

#[cfg(feature = "ogg")]
impl Decoder for OggDecoder {
    fn channels(&self) -> usize {
        self.reader.ident_hdr.audio_channels as usize
    }

    fn rate(&self) -> u32 {
        self.reader.ident_hdr.audio_sample_rate
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
        use lewton::samples::InterleavedSamples;
        match self.reader.read_dec_packet_generic::<InterleavedSamples<f32>>() {
            Ok(Some(p)) => {
                out.extend_from_slice(&p.samples);
                true
            }
            _ => false,
        }
    }

    fn rewind(&mut self) -> bool {
        match OggDecoder::new(self.data.clone()) {
            Ok(d) => {
                *self = d;
                true
            }
            Err(_) => false,
        }
    }
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
