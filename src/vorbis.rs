//! Ogg Vorbis decoder: a port of stb_vorbis 1.25 by Sean Barrett and contributors
//! (https://github.com/nothings/stb; public domain, or MIT at your option),
//! reading from memory. Only what featheraudio needs: open, decode frame by frame, go back to
//! the start. The output is bit-identical to stb_vorbis (compiled without -ffast-math).
//!
//! The structure follows the C code closely, so it can be compared with it; pointers became
//! indices, and every array access is checked, so broken or hostile files end the stream
//! instead of reading out of bounds.

// (the dB table is copied digit for digit from the specification, as stb_vorbis does)
#![allow(clippy::too_many_arguments, clippy::needless_range_loop, clippy::excessive_precision)]

use std::f64::consts::PI;
use std::sync::Arc;

const MAX_CHANNELS: usize = 16;
const FAST_HUFFMAN_LENGTH: i32 = 10;
const FAST_TABLE_SIZE: usize = 1 << FAST_HUFFMAN_LENGTH;
const FAST_MASK: u32 = FAST_TABLE_SIZE as u32 - 1;
const NO_CODE: u8 = 255;
const EOP: i32 = -1;
const INVALID_BITS: i32 = -1;
const CODEBOOK_LIMIT: i32 = 1 << 18;

const PAGEFLAG_CONTINUED: u8 = 1;
const PAGEFLAG_FIRST: u8 = 2;
const PAGEFLAG_LAST: u8 = 4;

const PACKET_ID: i32 = 1;
const PACKET_COMMENT: i32 = 3;
const PACKET_SETUP: i32 = 5;

// ---------------------------------------------------------------------------------------------
// small helpers

fn bit_reverse(n: u32) -> u32 {
    n.reverse_bits()
}

/// stb_vorbis's ilog: ilog(1) = 1, ilog(2) = 2, ilog(4) = 3; 0 for 0 and negatives
fn ilog(n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        32 - (n as u32).leading_zeros() as i32
    }
}

fn float32_unpack(x: u32) -> f32 {
    let mantissa = x & 0x1f_ffff;
    let sign = x & 0x8000_0000;
    let exp = ((x & 0x7fe0_0000) >> 21) as i32;
    let res = if sign != 0 {
        -(mantissa as f64)
    } else {
        mantissa as f64
    };
    // ldexp((float)res, exp - 788): exact, the power of two is a normal double
    ((res as f32) as f64 * f64::from_bits(((exp - 788 + 1023) as u64) << 52)) as f32
}

fn lookup1_values(entries: i32, dim: i32) -> i32 {
    let entriesd = entries as f64;
    let mut r = (entriesd.ln() / dim as f64).exp().floor() as i32;
    if ((r as f64 + 1.0).powf(dim as f64)).floor() <= entriesd {
        r += 1;
    }
    if (r as f64 + 1.0).powf(dim as f64) <= entriesd {
        return -1;
    }
    if ((r as f64).powf(dim as f64)).floor() > entriesd {
        return -1;
    }
    r
}

static INVERSE_DB_TABLE: [f32; 256] = [
    1.0649863e-07,
    1.1341951e-07,
    1.2079015e-07,
    1.2863978e-07,
    1.3699951e-07,
    1.4590251e-07,
    1.5538408e-07,
    1.6548181e-07,
    1.7623575e-07,
    1.8768855e-07,
    1.9988561e-07,
    2.1287530e-07,
    2.2670913e-07,
    2.4144197e-07,
    2.5713223e-07,
    2.7384213e-07,
    2.9163793e-07,
    3.1059021e-07,
    3.3077411e-07,
    3.5226968e-07,
    3.7516214e-07,
    3.9954229e-07,
    4.2550680e-07,
    4.5315863e-07,
    4.8260743e-07,
    5.1396998e-07,
    5.4737065e-07,
    5.8294187e-07,
    6.2082472e-07,
    6.6116941e-07,
    7.0413592e-07,
    7.4989464e-07,
    7.9862701e-07,
    8.5052630e-07,
    9.0579828e-07,
    9.6466216e-07,
    1.0273513e-06,
    1.0941144e-06,
    1.1652161e-06,
    1.2409384e-06,
    1.3215816e-06,
    1.4074654e-06,
    1.4989305e-06,
    1.5963394e-06,
    1.7000785e-06,
    1.8105592e-06,
    1.9282195e-06,
    2.0535261e-06,
    2.1869758e-06,
    2.3290978e-06,
    2.4804557e-06,
    2.6416497e-06,
    2.8133190e-06,
    2.9961443e-06,
    3.1908506e-06,
    3.3982101e-06,
    3.6190449e-06,
    3.8542308e-06,
    4.1047004e-06,
    4.3714470e-06,
    4.6555282e-06,
    4.9580707e-06,
    5.2802740e-06,
    5.6234160e-06,
    5.9888572e-06,
    6.3780469e-06,
    6.7925283e-06,
    7.2339451e-06,
    7.7040476e-06,
    8.2047000e-06,
    8.7378876e-06,
    9.3057248e-06,
    9.9104632e-06,
    1.0554501e-05,
    1.1240392e-05,
    1.1970856e-05,
    1.2748789e-05,
    1.3577278e-05,
    1.4459606e-05,
    1.5399272e-05,
    1.6400004e-05,
    1.7465768e-05,
    1.8600792e-05,
    1.9809576e-05,
    2.1096914e-05,
    2.2467911e-05,
    2.3928002e-05,
    2.5482978e-05,
    2.7139006e-05,
    2.8902651e-05,
    3.0780908e-05,
    3.2781225e-05,
    3.4911534e-05,
    3.7180282e-05,
    3.9596466e-05,
    4.2169667e-05,
    4.4910090e-05,
    4.7828601e-05,
    5.0936773e-05,
    5.4246931e-05,
    5.7772202e-05,
    6.1526565e-05,
    6.5524908e-05,
    6.9783085e-05,
    7.4317983e-05,
    7.9147585e-05,
    8.4291040e-05,
    8.9768747e-05,
    9.5602426e-05,
    0.00010181521,
    0.00010843174,
    0.00011547824,
    0.00012298267,
    0.00013097477,
    0.00013948625,
    0.00014855085,
    0.00015820453,
    0.00016848555,
    0.00017943469,
    0.00019109536,
    0.00020351382,
    0.00021673929,
    0.00023082423,
    0.00024582449,
    0.00026179955,
    0.00027881276,
    0.00029693158,
    0.00031622787,
    0.00033677814,
    0.00035866388,
    0.00038197188,
    0.00040679456,
    0.00043323036,
    0.00046138411,
    0.00049136745,
    0.00052329927,
    0.00055730621,
    0.00059352311,
    0.00063209358,
    0.00067317058,
    0.00071691700,
    0.00076350630,
    0.00081312324,
    0.00086596457,
    0.00092223983,
    0.00098217216,
    0.0010459992,
    0.0011139742,
    0.0011863665,
    0.0012634633,
    0.0013455702,
    0.0014330129,
    0.0015261382,
    0.0016253153,
    0.0017309374,
    0.0018434235,
    0.0019632195,
    0.0020908006,
    0.0022266726,
    0.0023713743,
    0.0025254795,
    0.0026895994,
    0.0028643847,
    0.0030505286,
    0.0032487691,
    0.0034598925,
    0.0036847358,
    0.0039241906,
    0.0041792066,
    0.0044507950,
    0.0047400328,
    0.0050480668,
    0.0053761186,
    0.0057254891,
    0.0060975636,
    0.0064938176,
    0.0069158225,
    0.0073652516,
    0.0078438871,
    0.0083536271,
    0.0088964928,
    0.009474637,
    0.010090352,
    0.010746080,
    0.011444421,
    0.012188144,
    0.012980198,
    0.013823725,
    0.014722068,
    0.015678791,
    0.016697687,
    0.017782797,
    0.018938423,
    0.020169149,
    0.021479854,
    0.022875735,
    0.024362330,
    0.025945531,
    0.027631618,
    0.029427276,
    0.031339626,
    0.033376252,
    0.035545228,
    0.037855157,
    0.040315199,
    0.042935108,
    0.045725273,
    0.048696758,
    0.051861348,
    0.055231591,
    0.058820850,
    0.062643361,
    0.066714279,
    0.071049749,
    0.075666962,
    0.080584227,
    0.085821044,
    0.091398179,
    0.097337747,
    0.10366330,
    0.11039993,
    0.11757434,
    0.12521498,
    0.13335215,
    0.14201813,
    0.15124727,
    0.16107617,
    0.17154380,
    0.18269168,
    0.19456402,
    0.20720788,
    0.22067342,
    0.23501402,
    0.25028656,
    0.26655159,
    0.28387361,
    0.30232132,
    0.32196786,
    0.34289114,
    0.36517414,
    0.38890521,
    0.41417847,
    0.44109412,
    0.46975890,
    0.50028648,
    0.53279791,
    0.56742212,
    0.60429640,
    0.64356699,
    0.68538959,
    0.72993007,
    0.77736504,
    0.82788260,
    0.88168307,
    0.9389798,
    1.0,
];

// ---------------------------------------------------------------------------------------------
// setup structures

struct Codebook {
    dimensions: i32,
    entries: i32,
    codeword_lengths: Vec<u8>,
    minimum_value: f32,
    delta_value: f32,
    value_bits: u8,
    lookup_type: u8,
    sequence_p: bool,
    sparse: bool,
    lookup_values: u32,
    multiplicands: Vec<f32>,
    /// None once a sparse codebook is set up (stb frees it)
    codewords: Option<Vec<u32>>,
    fast_huffman: Box<[i32; FAST_TABLE_SIZE]>,
    sorted_codewords: Option<Vec<u32>>,
    /// stb's sorted_values with the extra slot at the front: index x is at [x + 1], [0] = -1
    sorted_values: Vec<i32>,
    sorted_entries: i32,
}

impl Default for Codebook {
    fn default() -> Self {
        Codebook {
            dimensions: 0,
            entries: 0,
            codeword_lengths: Vec::new(),
            minimum_value: 0.0,
            delta_value: 0.0,
            value_bits: 0,
            lookup_type: 0,
            sequence_p: false,
            sparse: false,
            lookup_values: 0,
            multiplicands: Vec::new(),
            codewords: None,
            fast_huffman: Box::new([-1; FAST_TABLE_SIZE]),
            sorted_codewords: None,
            sorted_values: Vec::new(),
            sorted_entries: 0,
        }
    }
}

impl Codebook {
    /// sorted_values[x] for x >= -1
    fn sorted_value(&self, x: i32) -> i32 {
        self.sorted_values
            .get((x + 1) as usize)
            .copied()
            .unwrap_or(-1)
    }
}

#[derive(Default, Clone)]
struct Floor1 {
    partitions: u8,
    partition_class_list: [u8; 32],
    class_dimensions: [u8; 16],
    class_subclasses: [u8; 16],
    class_masterbooks: [u8; 16],
    subclass_books: [[i16; 8]; 16],
    xlist: Vec<u16>,
    sorted_order: Vec<u8>,
    neighbors: Vec<[u8; 2]>,
    floor1_multiplier: u8,
    values: i32,
}

struct Residue {
    begin: u32,
    end: u32,
    part_size: u32,
    classbook: u8,
    /// classdata[q * classwords + i]
    classdata: Vec<u8>,
    residue_books: Vec<[i16; 8]>,
}

#[derive(Clone, Copy, Default)]
struct MappingChannel {
    magnitude: u8,
    angle: u8,
    mux: u8,
}

struct Mapping {
    chan: Vec<MappingChannel>,
    coupling_steps: u16,
    submaps: u8,
    submap_floor: [u8; 16],
    submap_residue: [u8; 16],
}

#[derive(Clone, Copy, Default)]
struct Mode {
    blockflag: u8,
    mapping: u8,
}

// ---------------------------------------------------------------------------------------------
// the bit / packet / page reader

struct Reader {
    data: Arc<[u8]>,
    pos: usize,
    eof: bool,
    error: bool,

    page_flag: u8,
    segment_count: i32,
    segments: [u8; 255],
    next_seg: i32,
    last_seg: bool,
    last_seg_which: i32,
    bytes_in_seg: u8,
    acc: u32,
    valid_bits: i32,
    end_seg_with_known_loc: i32,
    known_loc_for_packet: u64,
}

impl Reader {
    fn err(&mut self) -> bool {
        self.error = true;
        false
    }

    fn get8(&mut self) -> u8 {
        match self.data.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                b
            }
            None => {
                self.eof = true;
                0
            }
        }
    }

    fn get32(&mut self) -> u32 {
        let mut x = self.get8() as u32;
        x = x.wrapping_add((self.get8() as u32) << 8);
        x = x.wrapping_add((self.get8() as u32) << 16);
        x.wrapping_add((self.get8() as u32) << 24)
    }

    fn get64(&mut self) -> u64 {
        let mut x = 0u64;
        for sh in (0..64).step_by(8) {
            x = x.wrapping_add((self.get8() as u64) << sh);
        }
        x
    }

    fn getn(&mut self, out: &mut [u8]) -> bool {
        let end = self.pos + out.len();
        if end > self.data.len() {
            self.eof = true;
            return false;
        }
        out.copy_from_slice(&self.data[self.pos..end]);
        self.pos = end;
        true
    }

    fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n);
        if self.pos >= self.data.len() {
            self.eof = true;
        }
    }

    fn set_offset(&mut self, loc: usize) {
        self.eof = false;
        if loc >= self.data.len() {
            self.pos = self.data.len();
            self.eof = true;
        } else {
            self.pos = loc;
        }
    }

    fn capture_pattern(&mut self) -> bool {
        self.get8() == 0x4f && self.get8() == 0x67 && self.get8() == 0x67 && self.get8() == 0x53
    }

    fn start_page_no_capturepattern(&mut self) -> bool {
        if self.get8() != 0 {
            return self.err();
        }
        self.page_flag = self.get8();
        let loc = self.get64();
        if loc >= 1u64 << 50 {
            return self.err();
        }
        self.get32(); // serial number
        self.get32(); // page sequence number
        self.get32(); // crc
        self.segment_count = self.get8() as i32;
        let mut seg = [0u8; 255];
        let n = self.segment_count as usize;
        if !self.getn(&mut seg[..n]) {
            return self.err();
        }
        self.segments[..n].copy_from_slice(&seg[..n]);
        self.end_seg_with_known_loc = -2;
        if loc != u64::MAX {
            let mut i = self.segment_count - 1;
            while i >= 0 {
                if self.segments[i as usize] < 255 {
                    break;
                }
                i -= 1;
            }
            if i >= 0 {
                self.end_seg_with_known_loc = i;
                self.known_loc_for_packet = loc;
            }
        }
        self.next_seg = 0;
        true
    }

    fn start_page(&mut self) -> bool {
        if !self.capture_pattern() {
            return self.err();
        }
        self.start_page_no_capturepattern()
    }

    fn start_packet(&mut self) -> bool {
        while self.next_seg == -1 {
            if !self.start_page() {
                return false;
            }
            if self.page_flag & PAGEFLAG_CONTINUED != 0 {
                return self.err();
            }
        }
        self.last_seg = false;
        self.valid_bits = 0;
        self.bytes_in_seg = 0;
        true
    }

    fn maybe_start_packet(&mut self) -> bool {
        if self.next_seg == -1 {
            let x = self.get8();
            if self.eof {
                return false; // EOF at page boundary is not an error
            }
            if x != 0x4f || self.get8() != 0x67 || self.get8() != 0x67 || self.get8() != 0x53 {
                return self.err();
            }
            if !self.start_page_no_capturepattern() {
                return false;
            }
            if self.page_flag & PAGEFLAG_CONTINUED != 0 {
                self.last_seg = false;
                self.bytes_in_seg = 0;
                return self.err();
            }
        }
        self.start_packet()
    }

    fn next_segment(&mut self) -> i32 {
        if self.last_seg {
            return 0;
        }
        if self.next_seg == -1 {
            self.last_seg_which = self.segment_count - 1;
            if !self.start_page() {
                self.last_seg = true;
                return 0;
            }
            if self.page_flag & PAGEFLAG_CONTINUED == 0 {
                self.err();
                return 0;
            }
        }
        let len = self.segments[self.next_seg as usize];
        self.next_seg += 1;
        if len < 255 {
            self.last_seg = true;
            self.last_seg_which = self.next_seg - 1;
        }
        if self.next_seg >= self.segment_count {
            self.next_seg = -1;
        }
        self.bytes_in_seg = len;
        len as i32
    }

    fn get8_packet_raw(&mut self) -> i32 {
        if self.bytes_in_seg == 0 && (self.last_seg || self.next_segment() == 0) {
            return EOP;
        }
        self.bytes_in_seg -= 1;
        self.get8() as i32
    }

    fn get8_packet(&mut self) -> i32 {
        let x = self.get8_packet_raw();
        self.valid_bits = 0;
        x
    }

    fn get32_packet(&mut self) -> i32 {
        let mut x = 0u32;
        for sh in (0..32).step_by(8) {
            x |= (self.get8_packet() as u32) << sh;
        }
        x as i32
    }

    fn flush_packet(&mut self) {
        while self.get8_packet_raw() != EOP {}
    }

    fn get_bits(&mut self, n: i32) -> u32 {
        if self.valid_bits < 0 {
            return 0;
        }
        if self.valid_bits < n {
            if n > 24 {
                let z = self.get_bits(24);
                return z.wrapping_add(self.get_bits(n - 24) << 24);
            }
            if self.valid_bits == 0 {
                self.acc = 0;
            }
            while self.valid_bits < n {
                let p = self.get8_packet_raw();
                if p == EOP {
                    self.valid_bits = INVALID_BITS;
                    return 0;
                }
                self.acc = self.acc.wrapping_add((p as u32) << self.valid_bits);
                self.valid_bits += 8;
            }
        }
        // (n is at most 32; shifts by 32 behave as on x86, like the C code there)
        let z = self.acc & 1u32.wrapping_shl(n as u32).wrapping_sub(1);
        self.acc = self.acc.wrapping_shr(n as u32);
        self.valid_bits -= n;
        z
    }

    fn prep_huffman(&mut self) {
        if self.valid_bits <= 24 {
            if self.valid_bits == 0 {
                self.acc = 0;
            }
            loop {
                if self.last_seg && self.bytes_in_seg == 0 {
                    return;
                }
                let z = self.get8_packet_raw();
                if z == EOP {
                    return;
                }
                self.acc = self.acc.wrapping_add((z as u32) << self.valid_bits);
                self.valid_bits += 8;
                if self.valid_bits > 24 {
                    break;
                }
            }
        }
    }

    // ---- huffman decoding ----

    fn shift_acc(&mut self, len: i32) {
        self.acc = if len >= 32 { 0 } else { self.acc >> len };
    }

    fn decode_scalar_raw(&mut self, c: &Codebook) -> i32 {
        self.prep_huffman();
        if c.codewords.is_none() && c.sorted_codewords.is_none() {
            return -1;
        }
        let binary = if c.entries > 8 {
            c.sorted_codewords.is_some()
        } else {
            c.codewords.is_none()
        };
        if binary {
            let sc = c.sorted_codewords.as_deref().unwrap_or(&[]);
            let code = bit_reverse(self.acc);
            let (mut x, mut n) = (0i32, c.sorted_entries);
            while n > 1 {
                let m = x + (n >> 1);
                if sc.get(m as usize).is_some_and(|&v| v <= code) {
                    x = m;
                    n -= n >> 1;
                } else {
                    n >>= 1;
                }
            }
            if !c.sparse {
                x = c.sorted_value(x);
            }
            let Some(&len) = c.codeword_lengths.get(x as usize) else {
                self.valid_bits = 0;
                return -1;
            };
            let len = len as i32;
            if self.valid_bits >= len {
                self.shift_acc(len);
                self.valid_bits -= len;
                return x;
            }
            self.valid_bits = 0;
            return -1;
        }
        // small: linear search
        let cw = c.codewords.as_deref().unwrap_or(&[]);
        for i in 0..c.entries as usize {
            let (Some(&l), Some(&w)) = (c.codeword_lengths.get(i), cw.get(i)) else {
                break;
            };
            if l == NO_CODE {
                continue;
            }
            let mask = (2u32 << (l as u32 - 1)).wrapping_sub(1);
            if w == self.acc & mask {
                if self.valid_bits >= l as i32 {
                    self.shift_acc(l as i32);
                    self.valid_bits -= l as i32;
                    return i as i32;
                }
                self.valid_bits = 0;
                return -1;
            }
        }
        self.err();
        self.valid_bits = 0;
        -1
    }

    /// DECODE_RAW
    #[inline]
    fn decode_raw(&mut self, c: &Codebook) -> i32 {
        if self.valid_bits < FAST_HUFFMAN_LENGTH {
            self.prep_huffman();
        }
        let v = c.fast_huffman[(self.acc & FAST_MASK) as usize];
        if v >= 0 {
            let cl = c.codeword_lengths[v as usize] as i32;
            self.shift_acc(cl);
            self.valid_bits -= cl;
            if self.valid_bits < 0 {
                self.valid_bits = 0;
                return -1;
            }
            v
        } else {
            self.decode_scalar_raw(c)
        }
    }

    /// DECODE: also maps sparse sorted indices to symbols
    #[inline]
    fn decode(&mut self, c: &Codebook) -> i32 {
        let v = self.decode_raw(c);
        if c.sparse {
            c.sorted_value(v)
        } else {
            v
        }
    }

    fn codebook_decode_start(&mut self, c: &Codebook) -> i32 {
        let mut z = -1;
        if c.lookup_type == 0 {
            self.err();
        } else {
            z = self.decode_raw(c);
            if z < 0 {
                if self.bytes_in_seg == 0 && self.last_seg {
                    return z;
                }
                self.err();
            }
        }
        z
    }
}

fn codebook_decode(r: &mut Reader, c: &Codebook, out: &mut [f32], off: usize, len: i32) -> bool {
    let z = r.codebook_decode_start(c);
    if z < 0 {
        return false;
    }
    let len = len.min(c.dimensions);
    let z = (z * c.dimensions) as usize;
    let mut last = 0f32;
    for i in 0..len.max(0) as usize {
        let Some(&m) = c.multiplicands.get(z + i) else {
            return true;
        };
        let val = m + last;
        if let Some(o) = out.get_mut(off + i) {
            *o += val;
        }
        if c.sequence_p {
            last = val + c.minimum_value;
        }
    }
    true
}

fn codebook_decode_step(
    r: &mut Reader,
    c: &Codebook,
    out: &mut [f32],
    off: usize,
    len: i32,
    step: i32,
) -> bool {
    let z = r.codebook_decode_start(c);
    let mut last = 0f32;
    if z < 0 {
        return false;
    }
    let len = len.min(c.dimensions);
    let z = (z * c.dimensions) as usize;
    for i in 0..len.max(0) as usize {
        let Some(&m) = c.multiplicands.get(z + i) else {
            return true;
        };
        let val = m + last;
        if let Some(o) = out.get_mut(off + i * step as usize) {
            *o += val;
        }
        if c.sequence_p {
            last = val;
        }
    }
    true
}

/// residue type 2: decode into the channels interleaved. `outputs[ch]` is None for channels
/// that are not decoded.
fn codebook_decode_deinterleave_repeat(
    r: &mut Reader,
    c: &Codebook,
    bufs: &mut [Vec<f32>],
    outputs: &[Option<usize>],
    ch: i32,
    c_inter: &mut i32,
    p_inter: &mut i32,
    len: i32,
    mut total_decode: i32,
) -> bool {
    let mut effective = c.dimensions;
    if c.lookup_type == 0 {
        return r.err();
    }
    while total_decode > 0 {
        let mut last = 0f32;
        let z = r.decode_raw(c);
        if z < 0 {
            if r.bytes_in_seg == 0 && r.last_seg {
                return false;
            }
            return r.err();
        }
        // stop short at the end of the buffers
        if *c_inter + *p_inter * ch + effective > len * ch {
            effective = len * ch - *p_inter * ch - *c_inter;
        }
        let z = (z * c.dimensions) as usize;
        for i in 0..effective.max(0) as usize {
            let val = c.multiplicands.get(z + i).copied().unwrap_or(0.0) + last;
            if let Some(Some(b)) = outputs.get(*c_inter as usize) {
                if let Some(o) = bufs[*b].get_mut(*p_inter as usize) {
                    *o += val;
                }
            }
            *c_inter += 1;
            if *c_inter == ch {
                *c_inter = 0;
                *p_inter += 1;
            }
            if c.sequence_p {
                last = val;
            }
        }
        total_decode -= effective;
    }
    true
}

fn predict_point(x: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> i32 {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let err = dy.abs() * (x - x0);
    let off = if adx != 0 { err / adx } else { 0 };
    if dy < 0 {
        y0 - off
    } else {
        y0 + off
    }
}

fn draw_line(out: &mut [f32], x0: i32, y0: i32, mut x1: i32, y1: i32, n: i32) {
    let dy = y1 - y0;
    let adx = x1 - x0;
    if adx <= 0 {
        return;
    }
    let mut ady = dy.abs();
    let base = dy / adx;
    let sy = base + if dy < 0 { -1 } else { 1 };
    ady -= base.abs() * adx;
    let (x, mut y, mut err) = (x0, y0, 0);
    if x1 > n {
        x1 = n;
    }
    let x1 = x1.min(out.len() as i32);
    if x < x1 && x >= 0 {
        let seg = &mut out[x as usize..x1 as usize];
        seg[0] *= INVERSE_DB_TABLE[(y & 255) as usize];
        for o in &mut seg[1..] {
            err += ady;
            if err >= adx {
                err -= adx;
                y += sy;
            } else {
                y += base;
            }
            *o *= INVERSE_DB_TABLE[(y & 255) as usize];
        }
    }
}

// ---------------------------------------------------------------------------------------------
// IMDCT

/// The 8 floats ending at `top0` and the 8 ending at `top2` (top2 + 8 <= top0 - 7), as two
/// arrays: index 7 - k is e[top - k]. Lets the IMDCT loops run without bounds checks.
#[inline(always)]
fn two8(e: &mut [f32], top0: usize, top2: usize) -> (&mut [f32; 8], &mut [f32; 8]) {
    let (lo, hi) = e.split_at_mut(top0 - 7);
    let x0: &mut [f32; 8] = (&mut hi[..8]).try_into().unwrap();
    let x2: &mut [f32; 8] = (&mut lo[top2 - 7..top2 + 1]).try_into().unwrap();
    (x0, x2)
}

fn imdct_step3_iter0_loop(n: i32, e: &mut [f32], i_off: i32, k_off: i32, a: &[f32]) {
    let count = (n >> 2).max(0) as usize;
    let (mut ee0, mut ee2) = (i_off as usize, (i_off + k_off) as usize);
    let mut ai = 0usize;
    for it in 0..count {
        let (x0, x2) = two8(e, ee0, ee2);
        for s in 0..4 {
            let (h, l) = (7 - 2 * s, 6 - 2 * s);
            let (a0, a1) = (a[ai], a[ai + 1]);
            let k00_20 = x0[h] - x2[h];
            let k01_21 = x0[l] - x2[l];
            x0[h] += x2[h];
            x0[l] += x2[l];
            x2[h] = k00_20 * a0 - k01_21 * a1;
            x2[l] = k01_21 * a0 + k00_20 * a1;
            ai += 8;
        }
        if it + 1 < count {
            ee0 -= 8;
            ee2 -= 8;
        }
    }
}

fn imdct_step3_inner_r_loop(lim: i32, e: &mut [f32], d0: i32, k_off: i32, a: &[f32], k1: i32) {
    let count = (lim >> 2).max(0) as usize;
    let (mut e0, mut e2) = (d0 as usize, (d0 + k_off) as usize);
    let mut ai = 0usize;
    for it in 0..count {
        let (x0, x2) = two8(e, e0, e2);
        for s in 0..4 {
            let (h, l) = (7 - 2 * s, 6 - 2 * s);
            let (a0, a1) = (a[ai], a[ai + 1]);
            let k00_20 = x0[h] - x2[h];
            let k01_21 = x0[l] - x2[l];
            x0[h] += x2[h];
            x0[l] += x2[l];
            x2[h] = k00_20 * a0 - k01_21 * a1;
            x2[l] = k01_21 * a0 + k00_20 * a1;
            ai += k1 as usize;
        }
        if it + 1 < count {
            e0 -= 8;
            e2 -= 8;
        }
    }
}

fn imdct_step3_inner_s_loop(
    n: i32,
    e: &mut [f32],
    i_off: i32,
    k_off: i32,
    a: &[f32],
    a_off: i32,
    k0: i32,
) {
    let ao = a_off as usize;
    let aa = [
        a[0],
        a[1],
        a[ao],
        a[ao + 1],
        a[ao * 2],
        a[ao * 2 + 1],
        a[ao * 3],
        a[ao * 3 + 1],
    ];
    let count = n.max(0) as usize;
    let (mut ee0, mut ee2) = (i_off as usize, (i_off + k_off) as usize);
    for it in 0..count {
        let (x0, x2) = two8(e, ee0, ee2);
        for s in 0..4 {
            let (h, l) = (7 - 2 * s, 6 - 2 * s);
            let (a0, a1) = (aa[s * 2], aa[s * 2 + 1]);
            let k00 = x0[h] - x2[h];
            let k11 = x0[l] - x2[l];
            x0[h] += x2[h];
            x0[l] += x2[l];
            x2[h] = k00 * a0 - k11 * a1;
            x2[l] = k11 * a0 + k00 * a1;
        }
        if it + 1 < count {
            ee0 -= k0 as usize;
            ee2 -= k0 as usize;
        }
    }
}

#[inline]
fn iter_54(e: &mut [f32], z: usize) {
    let k00 = e[z] - e[z - 4];
    let y0 = e[z] + e[z - 4];
    let y2 = e[z - 2] + e[z - 6];
    let k22 = e[z - 2] - e[z - 6];
    e[z] = y0 + y2;
    e[z - 2] = y0 - y2;
    let k33 = e[z - 3] - e[z - 7];
    e[z - 4] = k00 + k33;
    e[z - 6] = k00 - k33;
    let k11 = e[z - 1] - e[z - 5];
    let y1 = e[z - 1] + e[z - 5];
    let y3 = e[z - 3] + e[z - 7];
    e[z - 1] = y1 + y3;
    e[z - 3] = y1 - y3;
    e[z - 5] = k11 - k22;
    e[z - 7] = k11 + k22;
}

fn imdct_step3_inner_s_loop_ld654(n: i32, e: &mut [f32], i_off: i32, a: &[f32], base_n: i32) {
    let a2 = a[(base_n >> 3) as usize];
    let mut z = i_off as isize;
    let base = z - 16 * n as isize;
    while z > base {
        let zz = z as usize;
        let k00 = e[zz] - e[zz - 8];
        let k11 = e[zz - 1] - e[zz - 9];
        let l00 = e[zz - 2] - e[zz - 10];
        let l11 = e[zz - 3] - e[zz - 11];
        e[zz] += e[zz - 8];
        e[zz - 1] += e[zz - 9];
        e[zz - 2] += e[zz - 10];
        e[zz - 3] += e[zz - 11];
        e[zz - 8] = k00;
        e[zz - 9] = k11;
        e[zz - 10] = (l00 + l11) * a2;
        e[zz - 11] = (l11 - l00) * a2;

        let k00 = e[zz - 4] - e[zz - 12];
        let k11 = e[zz - 5] - e[zz - 13];
        let l00 = e[zz - 6] - e[zz - 14];
        let l11 = e[zz - 7] - e[zz - 15];
        e[zz - 4] += e[zz - 12];
        e[zz - 5] += e[zz - 13];
        e[zz - 6] += e[zz - 14];
        e[zz - 7] += e[zz - 15];
        e[zz - 12] = k11;
        e[zz - 13] = -k00;
        e[zz - 14] = (l11 - l00) * a2;
        e[zz - 15] = (l00 + l11) * -a2;

        iter_54(e, zz);
        iter_54(e, zz - 8);
        z -= 16;
    }
}

struct BlockTables {
    a: Vec<f32>,
    b: Vec<f32>,
    c: Vec<f32>,
    window: Vec<f32>,
    bitrev: Vec<u16>,
}

impl BlockTables {
    fn new(n: i32) -> BlockTables {
        let (n2, n4, n8) = ((n >> 1) as usize, (n >> 2) as usize, (n >> 3) as usize);
        let nf = n as f64;
        let mut a = vec![0f32; n2];
        let mut b = vec![0f32; n2];
        let mut c = vec![0f32; n4];
        for k in 0..n4 {
            let k2 = 2 * k;
            a[k2] = (4.0 * k as f64 * PI / nf).cos() as f32;
            a[k2 + 1] = -(4.0 * k as f64 * PI / nf).sin() as f32;
            b[k2] = ((k2 + 1) as f64 * PI / nf / 2.0).cos() as f32 * 0.5;
            b[k2 + 1] = ((k2 + 1) as f64 * PI / nf / 2.0).sin() as f32 * 0.5;
        }
        for k in 0..n8 {
            let k2 = 2 * k;
            c[k2] = (2.0 * (k2 + 1) as f64 * PI / nf).cos() as f32;
            c[k2 + 1] = -(2.0 * (k2 + 1) as f64 * PI / nf).sin() as f32;
        }
        let window = (0..n2)
            .map(|i| {
                let s = ((i as f64 + 0.5) / n2 as f64 * 0.5 * PI).sin() as f32;
                (0.5 * PI * (s * s) as f64).sin() as f32
            })
            .collect();
        let ld = ilog(n) - 1;
        let bitrev = (0..n8)
            .map(|i| ((bit_reverse(i as u32) >> (32 - ld + 3)) << 2) as u16)
            .collect();
        BlockTables {
            a,
            b,
            c,
            window,
            bitrev,
        }
    }
}

fn inverse_mdct(buffer: &mut [f32], n: i32, t: &BlockTables, buf2: &mut [f32]) {
    let (n2, n4, n8) = (n >> 1, n >> 2, n >> 3);
    let a = &t.a[..];
    let n2u = n2 as usize;
    let buf2 = &mut buf2[..n2u];

    // copy and reflect spectral data, step 0
    {
        let mut d = n2 as isize - 2;
        let mut aa = 0usize;
        let mut e = 0usize;
        while e != n2u {
            let du = d as usize;
            buf2[du + 1] = buffer[e] * a[aa] - buffer[e + 2] * a[aa + 1];
            buf2[du] = buffer[e] * a[aa + 1] + buffer[e + 2] * a[aa];
            d -= 2;
            aa += 2;
            e += 4;
        }
        let mut e = n2 as isize - 3;
        while d >= 0 {
            let (du, eu) = (d as usize, e as usize);
            buf2[du + 1] = -buffer[eu + 2] * a[aa] - -buffer[eu] * a[aa + 1];
            buf2[du] = -buffer[eu + 2] * a[aa + 1] + -buffer[eu] * a[aa];
            d -= 2;
            aa += 2;
            e -= 4;
        }
    }

    // step 2: u = buffer, v = buf2
    {
        let u = &mut *buffer;
        let v = &*buf2;
        let mut aa = n2 as isize - 8;
        let (mut e0, mut e1) = (n4 as usize, 0usize);
        let (mut d0, mut d1) = (n4 as usize, 0usize);
        while aa >= 0 {
            let ai = aa as usize;
            let v41_21 = v[e0 + 1] - v[e1 + 1];
            let v40_20 = v[e0] - v[e1];
            u[d0 + 1] = v[e0 + 1] + v[e1 + 1];
            u[d0] = v[e0] + v[e1];
            u[d1 + 1] = v41_21 * a[ai + 4] - v40_20 * a[ai + 5];
            u[d1] = v40_20 * a[ai + 4] + v41_21 * a[ai + 5];

            let v41_21 = v[e0 + 3] - v[e1 + 3];
            let v40_20 = v[e0 + 2] - v[e1 + 2];
            u[d0 + 3] = v[e0 + 3] + v[e1 + 3];
            u[d0 + 2] = v[e0 + 2] + v[e1 + 2];
            u[d1 + 3] = v41_21 * a[ai] - v40_20 * a[ai + 1];
            u[d1 + 2] = v40_20 * a[ai] + v41_21 * a[ai + 1];

            aa -= 8;
            d0 += 4;
            d1 += 4;
            e0 += 4;
            e1 += 4;
        }
    }

    // step 3
    let ld = ilog(n) - 1;
    let u = &mut *buffer;
    imdct_step3_iter0_loop(n >> 4, u, n2 - 1, -(n >> 3), a);
    imdct_step3_iter0_loop(n >> 4, u, n2 - 1 - n4, -(n >> 3), a);

    for k in 0..4 {
        imdct_step3_inner_r_loop(n >> 5, u, n2 - 1 - n8 * k, -(n >> 4), a, 16);
    }

    let mut l = 2;
    while l < (ld - 3) >> 1 {
        let k0 = n >> (l + 2);
        let k0_2 = k0 >> 1;
        let lim = 1 << (l + 1);
        for i in 0..lim {
            imdct_step3_inner_r_loop(n >> (l + 4), u, n2 - 1 - k0 * i, -k0_2, a, 1 << (l + 3));
        }
        l += 1;
    }

    while l < ld - 6 {
        let k0 = n >> (l + 2);
        let k1 = 1 << (l + 3);
        let k0_2 = k0 >> 1;
        let rlim = n >> (l + 6);
        let lim = 1 << (l + 1);
        let mut a0 = 0usize;
        let mut i_off = n2 - 1;
        for _ in 0..rlim {
            imdct_step3_inner_s_loop(lim, u, i_off, -k0_2, &a[a0..], k1, k0);
            a0 += (k1 * 4) as usize;
            i_off -= 8;
        }
        l += 1;
    }

    imdct_step3_inner_s_loop_ld654(n >> 5, u, n2 - 1, a, n);

    // steps 4, 5, 6: u -> v
    {
        let v = &mut *buf2;
        let bitrev = &t.bitrev;
        let mut d0 = n4 as isize - 4;
        let mut d1 = n2 as usize - 4;
        let mut br = 0usize;
        while d0 >= 0 {
            let d0u = d0 as usize;
            let k4 = bitrev[br] as usize;
            v[d1 + 3] = u[k4];
            v[d1 + 2] = u[k4 + 1];
            v[d0u + 3] = u[k4 + 2];
            v[d0u + 2] = u[k4 + 3];
            let k4 = bitrev[br + 1] as usize;
            v[d1 + 1] = u[k4];
            v[d1] = u[k4 + 1];
            v[d0u + 1] = u[k4 + 2];
            v[d0u] = u[k4 + 3];
            d0 -= 4;
            d1 = d1.wrapping_sub(4);
            br += 2;
        }
    }

    // step 7, in place in v
    {
        let v = &mut *buf2;
        let c = &t.c;
        let (mut d, mut e) = (0usize, n2u - 4);
        let mut ci = 0usize;
        while d < e {
            let a02 = v[d] - v[e + 2];
            let a11 = v[d + 1] + v[e + 3];
            let b0 = c[ci + 1] * a02 + c[ci] * a11;
            let b1 = c[ci + 1] * a11 - c[ci] * a02;
            let b2 = v[d] + v[e + 2];
            let b3 = v[d + 1] - v[e + 3];
            v[d] = b2 + b0;
            v[d + 1] = b3 + b1;
            v[e + 2] = b2 - b0;
            v[e + 3] = b1 - b3;

            let a02 = v[d + 2] - v[e];
            let a11 = v[d + 3] + v[e + 1];
            let b0 = c[ci + 3] * a02 + c[ci + 2] * a11;
            let b1 = c[ci + 3] * a11 - c[ci + 2] * a02;
            let b2 = v[d + 2] + v[e];
            let b3 = v[d + 3] - v[e + 1];
            v[d + 2] = b2 + b0;
            v[d + 3] = b3 + b1;
            v[e] = b2 - b0;
            v[e + 1] = b1 - b3;

            ci += 4;
            d += 4;
            e -= 4;
        }
    }

    // step 8 + decode: buf2 -> buffer
    {
        let b = &t.b;
        let v = &*buf2;
        let mut bi = n2 as isize - 8;
        let mut e = n2 as isize - 8;
        let (mut d0, mut d1, mut d2, mut d3) = (0usize, n2u - 4, n2u, n as usize - 4);
        while e >= 0 {
            let (ei, bb) = (e as usize, bi as usize);
            let p3 = v[ei + 6] * b[bb + 7] - v[ei + 7] * b[bb + 6];
            let p2 = -v[ei + 6] * b[bb + 6] - v[ei + 7] * b[bb + 7];
            buffer[d0] = p3;
            buffer[d1 + 3] = -p3;
            buffer[d2] = p2;
            buffer[d3 + 3] = p2;

            let p1 = v[ei + 4] * b[bb + 5] - v[ei + 5] * b[bb + 4];
            let p0 = -v[ei + 4] * b[bb + 4] - v[ei + 5] * b[bb + 5];
            buffer[d0 + 1] = p1;
            buffer[d1 + 2] = -p1;
            buffer[d2 + 1] = p0;
            buffer[d3 + 2] = p0;

            let p3 = v[ei + 2] * b[bb + 3] - v[ei + 3] * b[bb + 2];
            let p2 = -v[ei + 2] * b[bb + 2] - v[ei + 3] * b[bb + 3];
            buffer[d0 + 2] = p3;
            buffer[d1 + 1] = -p3;
            buffer[d2 + 2] = p2;
            buffer[d3 + 1] = p2;

            let p1 = v[ei] * b[bb + 1] - v[ei + 1] * b[bb];
            let p0 = -v[ei] * b[bb] - v[ei + 1] * b[bb + 1];
            buffer[d0 + 3] = p1;
            buffer[d1] = -p1;
            buffer[d2 + 3] = p0;
            buffer[d3] = p0;

            bi -= 8;
            e -= 8;
            d0 += 4;
            d2 += 4;
            d1 = d1.wrapping_sub(4);
            d3 = d3.wrapping_sub(4);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the decoder

/// An Ogg Vorbis stream decoding from memory.
pub(crate) struct Vorbis {
    r: Reader,
    pub sample_rate: u32,
    pub channels: usize,

    blocksize: [i32; 2],
    codebooks: Vec<Codebook>,
    floor_config: Vec<Floor1>,
    residue_types: Vec<u16>,
    residue_config: Vec<Residue>,
    mapping: Vec<Mapping>,
    mode_config: Vec<Mode>,

    channel_buffers: Vec<Vec<f32>>,
    previous_window: Vec<Vec<f32>>,
    previous_length: i32,
    final_y: Vec<Vec<i16>>,
    tables: [BlockTables; 2],
    /// scratch for the IMDCT (blocksize_1 / 2)
    imdct_buf: Vec<f32>,
    /// scratch for residue decoding: classdata row (q) per channel per partition set
    part_classdata: Vec<Vec<u32>>,

    first_decode: bool,
    current_loc: i64,
    current_loc_valid: bool,
    discard_samples_deferred: i32,
    first_audio_page_offset: usize,

    channel_buffer_start: usize,
    channel_buffer_end: usize,
}

impl Vorbis {
    /// Opens a stream and decodes its headers (and the first frame, which gives no samples).
    pub fn new(data: Arc<[u8]>) -> Option<Vorbis> {
        let r = Reader {
            data,
            pos: 0,
            eof: false,
            error: false,
            page_flag: 0,
            segment_count: 0,
            segments: [0; 255],
            next_seg: 0,
            last_seg: false,
            last_seg_which: 0,
            bytes_in_seg: 0,
            acc: 0,
            valid_bits: 0,
            end_seg_with_known_loc: 0,
            known_loc_for_packet: 0,
        };
        let mut f = Vorbis {
            r,
            sample_rate: 0,
            channels: 0,
            blocksize: [0; 2],
            codebooks: Vec::new(),
            floor_config: Vec::new(),
            residue_types: Vec::new(),
            residue_config: Vec::new(),
            mapping: Vec::new(),
            mode_config: Vec::new(),
            channel_buffers: Vec::new(),
            previous_window: Vec::new(),
            previous_length: 0,
            final_y: Vec::new(),
            tables: [BlockTables::new(256), BlockTables::new(256)],
            imdct_buf: Vec::new(),
            part_classdata: Vec::new(),
            first_decode: true,
            current_loc: 0,
            current_loc_valid: false,
            discard_samples_deferred: 0,
            first_audio_page_offset: 0,
            channel_buffer_start: 0,
            channel_buffer_end: 0,
        };
        if !f.start_decoder() {
            return None;
        }
        f.pump_first_frame();
        Some(f)
    }

    /// Decodes the next frame. Returns its length in samples (often 0 for the first one), or
    /// None at the end of the stream. The samples are in [`channel`](Self::channel).
    pub fn next_frame(&mut self) -> Option<usize> {
        let Some((len, left, right)) = self.decode_packet() else {
            self.channel_buffer_start = 0;
            self.channel_buffer_end = 0;
            return None;
        };
        let len = self.finish_frame(len, left, right).max(0) as usize;
        self.channel_buffer_start = left as usize;
        self.channel_buffer_end = left as usize + len;
        Some(len)
    }

    /// The samples of channel `c` from the last [`next_frame`](Self::next_frame).
    pub fn channel(&self, c: usize) -> &[f32] {
        &self.channel_buffers[c][self.channel_buffer_start..self.channel_buffer_end]
    }

    /// Goes back to the start of the stream.
    pub fn rewind(&mut self) -> bool {
        self.r.set_offset(self.first_audio_page_offset);
        self.r.error = false;
        self.previous_length = 0;
        self.first_decode = true;
        self.r.next_seg = -1;
        self.pump_first_frame()
    }

    fn pump_first_frame(&mut self) -> bool {
        match self.decode_packet() {
            Some((len, left, right)) => {
                self.finish_frame(len, left, right);
                true
            }
            None => false,
        }
    }

    // ---- setup ----

    fn start_decoder(&mut self) -> bool {
        let r = &mut self.r;
        self.first_decode = true;
        if !r.start_page() {
            return false;
        }
        if r.page_flag & PAGEFLAG_FIRST == 0
            || r.page_flag & PAGEFLAG_LAST != 0
            || r.page_flag & PAGEFLAG_CONTINUED != 0
        {
            return r.err();
        }
        if r.segment_count != 1 || r.segments[0] != 30 {
            return r.err();
        }
        let mut header = [0u8; 6];
        if r.get8() as i32 != PACKET_ID {
            return r.err();
        }
        if !r.getn(&mut header) || &header != b"vorbis" {
            return r.err();
        }
        if r.get32() != 0 {
            return r.err();
        }
        let channels = r.get8() as usize;
        if channels == 0 || channels > MAX_CHANNELS {
            return r.err();
        }
        self.channels = channels;
        self.sample_rate = r.get32();
        if self.sample_rate == 0 {
            return r.err();
        }
        r.get32();
        r.get32();
        r.get32();
        let x = r.get8();
        let (log0, log1) = ((x & 15) as i32, (x >> 4) as i32);
        if !(8..=13).contains(&log0) || !(8..=13).contains(&log1) || log0 > log1 {
            return r.err();
        }
        self.blocksize = [1 << log0, 1 << log1];
        let max_blocksize = self.blocksize[1] as u32;
        if r.get8() & 1 == 0 {
            return r.err();
        }

        // second packet: comments (skipped)
        if !r.start_page() || !r.start_packet() {
            return false;
        }
        if r.next_segment() == 0 {
            return false;
        }
        if r.get8_packet() != PACKET_COMMENT {
            return r.err();
        }
        for h in header.iter_mut() {
            *h = r.get8_packet() as u8;
        }
        if &header != b"vorbis" {
            return r.err();
        }
        let len = r.get32_packet();
        if len < 0 || len == i32::MAX {
            return r.err();
        }
        // (reading on past the end of the packet changes nothing, so stop there)
        for _ in 0..len {
            if r.get8_packet() == EOP {
                break;
            }
        }
        let count = r.get32_packet();
        if count > i32::MAX / 8 {
            return r.err();
        }
        for _ in 0..count.max(0) {
            let len = r.get32_packet();
            if len < 0 || len == i32::MAX {
                return r.err();
            }
            for _ in 0..len {
                if r.get8_packet() == EOP {
                    break;
                }
            }
        }
        if r.get8_packet() & 1 == 0 {
            return r.err();
        }
        r.skip(r.bytes_in_seg as usize);
        r.bytes_in_seg = 0;
        loop {
            let len = r.next_segment();
            r.skip(len as usize);
            r.bytes_in_seg = 0;
            if len == 0 {
                break;
            }
        }

        // third packet: setup
        if !r.start_packet() {
            return false;
        }
        if r.get8_packet() != PACKET_SETUP {
            return r.err();
        }
        for h in header.iter_mut() {
            *h = r.get8_packet() as u8;
        }
        if &header != b"vorbis" {
            return r.err();
        }

        // codebooks
        let codebook_count = r.get_bits(8) as i32 + 1;
        if r.valid_bits < 0 {
            return r.err();
        }
        for _ in 0..codebook_count {
            match read_codebook(r) {
                Some(c) => self.codebooks.push(c),
                None => return r.err(),
            }
        }

        // time domain transfers (unused)
        let x = r.get_bits(6) + 1;
        for _ in 0..x {
            if r.get_bits(16) != 0 {
                return r.err();
            }
        }

        // floors
        let floor_count = r.get_bits(6) as i32 + 1;
        if r.valid_bits < 0 {
            return r.err();
        }
        let mut longest_floorlist = 0;
        for _ in 0..floor_count {
            let floor_type = r.get_bits(16);
            if floor_type != 1 {
                // floor 0 is not supported by stb_vorbis (and no encoder in use writes it)
                return r.err();
            }
            let mut g = Floor1 {
                partitions: r.get_bits(5) as u8,
                ..Default::default()
            };
            let mut max_class = -1i32;
            for j in 0..g.partitions as usize {
                g.partition_class_list[j] = r.get_bits(4) as u8;
                max_class = max_class.max(g.partition_class_list[j] as i32);
            }
            for j in 0..(max_class + 1).max(0) as usize {
                g.class_dimensions[j] = r.get_bits(3) as u8 + 1;
                g.class_subclasses[j] = r.get_bits(2) as u8;
                if r.valid_bits < 0 {
                    return r.err();
                }
                if g.class_subclasses[j] != 0 {
                    g.class_masterbooks[j] = r.get_bits(8) as u8;
                    if g.class_masterbooks[j] as i32 >= codebook_count {
                        return r.err();
                    }
                }
                for k in 0..1usize << g.class_subclasses[j] {
                    g.subclass_books[j][k] = r.get_bits(8) as i16 - 1;
                    if g.subclass_books[j][k] as i32 >= codebook_count {
                        return r.err();
                    }
                }
            }
            g.floor1_multiplier = r.get_bits(2) as u8 + 1;
            let rangebits = r.get_bits(4) as i32;
            g.xlist = vec![0, (1u32 << rangebits) as u16];
            for j in 0..g.partitions as usize {
                let c = g.partition_class_list[j] as usize;
                for _ in 0..g.class_dimensions[c] {
                    g.xlist.push(r.get_bits(rangebits) as u16);
                }
            }
            g.values = g.xlist.len() as i32;
            // sort the points by x (at most 250: a small insertion sort is enough)
            let mut p: Vec<(u16, u16)> = g
                .xlist
                .iter()
                .enumerate()
                .map(|(j, &x)| (x, j as u16))
                .collect();
            for i in 1..p.len() {
                let mut j = i;
                while j > 0 && p[j - 1].0 > p[j].0 {
                    p.swap(j - 1, j);
                    j -= 1;
                }
            }
            if p.windows(2).any(|w| w[0].0 == w[1].0) {
                return r.err();
            }
            g.sorted_order = p.iter().map(|q| q.1 as u8).collect();
            g.neighbors = vec![[0, 0]; g.xlist.len()];
            for j in 2..g.xlist.len() {
                let (mut low, mut high) = (0usize, 0usize);
                let (mut lo, mut hi) = (-1i32, 65536i32);
                let xn = g.xlist[j] as i32;
                for i in 0..j {
                    let xi = g.xlist[i] as i32;
                    if xi > lo && xi < xn {
                        low = i;
                        lo = xi;
                    }
                    if xi < hi && xi > xn {
                        high = i;
                        hi = xi;
                    }
                }
                g.neighbors[j] = [low as u8, high as u8];
            }
            longest_floorlist = longest_floorlist.max(g.values);
            self.floor_config.push(g);
        }

        // residues
        let residue_count = r.get_bits(6) as i32 + 1;
        if r.valid_bits < 0 {
            return r.err();
        }
        let max_vector = max_blocksize * self.channels as u32;
        for _ in 0..residue_count {
            let rtype = r.get_bits(16) as u16;
            if rtype > 2 {
                return r.err();
            }
            let begin = r.get_bits(24);
            let end = r.get_bits(24);
            if end < begin || end > max_vector {
                return r.err();
            }
            let part_size = r.get_bits(24) + 1;
            if part_size > max_vector {
                return r.err();
            }
            let classifications = r.get_bits(6) as u8 + 1;
            let classbook = r.get_bits(8) as u8;
            if r.valid_bits < 0 || classbook as i32 >= codebook_count {
                return r.err();
            }
            let mut cascade = [0u8; 64];
            for c in cascade.iter_mut().take(classifications as usize) {
                let low = r.get_bits(3) as u8;
                let high = if r.get_bits(1) != 0 {
                    r.get_bits(5) as u8
                } else {
                    0
                };
                *c = high.wrapping_mul(8).wrapping_add(low);
            }
            if r.valid_bits < 0 {
                return r.err();
            }
            let mut residue_books = vec![[0i16; 8]; classifications as usize];
            for j in 0..classifications as usize {
                for k in 0..8 {
                    if cascade[j] & (1 << k) != 0 {
                        residue_books[j][k] = r.get_bits(8) as i16;
                        if r.valid_bits < 0 || residue_books[j][k] as i32 >= codebook_count {
                            return r.err();
                        }
                    } else {
                        residue_books[j][k] = -1;
                    }
                }
            }
            let cb = &self.codebooks[classbook as usize];
            let classwords = cb.dimensions.max(0) as usize;
            let entries = cb.entries.max(0) as usize;
            let mut classdata = vec![0u8; entries * classwords];
            for j in 0..entries {
                let mut temp = j as i32;
                for k in (0..classwords).rev() {
                    classdata[j * classwords + k] = (temp % classifications as i32) as u8;
                    temp /= classifications as i32;
                }
            }
            self.residue_types.push(rtype);
            self.residue_config.push(Residue {
                begin,
                end,
                part_size,
                classbook,
                classdata,
                residue_books,
            });
        }

        // mappings
        let mapping_count = r.get_bits(6) as i32 + 1;
        if r.valid_bits < 0 {
            return r.err();
        }
        let ch = self.channels;
        for _ in 0..mapping_count {
            if r.get_bits(16) != 0 {
                return r.err();
            }
            let mut m = Mapping {
                chan: vec![MappingChannel::default(); ch],
                coupling_steps: 0,
                submaps: 1,
                submap_floor: [0; 16],
                submap_residue: [0; 16],
            };
            if r.get_bits(1) != 0 {
                m.submaps = r.get_bits(4) as u8 + 1;
            }
            if r.get_bits(1) != 0 {
                m.coupling_steps = r.get_bits(8) as u16 + 1;
                if m.coupling_steps as usize > ch {
                    return r.err();
                }
                let bits = ilog(ch as i32 - 1);
                for k in 0..m.coupling_steps as usize {
                    m.chan[k].magnitude = r.get_bits(bits) as u8;
                    m.chan[k].angle = r.get_bits(bits) as u8;
                    if r.valid_bits < 0 {
                        return r.err();
                    }
                    let (mg, an) = (m.chan[k].magnitude as usize, m.chan[k].angle as usize);
                    if mg >= ch || an >= ch || mg == an {
                        return r.err();
                    }
                }
            }
            if r.get_bits(2) != 0 {
                return r.err();
            }
            if m.submaps > 1 {
                for j in 0..ch {
                    m.chan[j].mux = r.get_bits(4) as u8;
                    if m.chan[j].mux >= m.submaps {
                        return r.err();
                    }
                }
            }
            for j in 0..m.submaps as usize {
                r.get_bits(8);
                m.submap_floor[j] = r.get_bits(8) as u8;
                m.submap_residue[j] = r.get_bits(8) as u8;
                if m.submap_floor[j] as i32 >= floor_count
                    || m.submap_residue[j] as i32 >= residue_count
                {
                    return r.err();
                }
            }
            self.mapping.push(m);
        }

        // modes
        let mode_count = r.get_bits(6) as i32 + 1;
        for _ in 0..mode_count {
            let blockflag = r.get_bits(1) as u8;
            let windowtype = r.get_bits(16);
            let transformtype = r.get_bits(16);
            let mapping = r.get_bits(8) as u8;
            if r.valid_bits < 0
                || windowtype != 0
                || transformtype != 0
                || mapping as i32 >= mapping_count
            {
                return r.err();
            }
            self.mode_config.push(Mode { blockflag, mapping });
        }

        r.flush_packet();
        self.previous_length = 0;

        let b1 = self.blocksize[1] as usize;
        self.channel_buffers = vec![vec![0.0; b1]; ch];
        self.previous_window = vec![vec![0.0; b1 / 2]; ch];
        self.final_y = vec![vec![0; longest_floorlist as usize]; ch];
        self.tables = [
            BlockTables::new(self.blocksize[0]),
            BlockTables::new(self.blocksize[1]),
        ];
        self.imdct_buf = vec![0.0; b1 / 2];

        // the largest number of partitions a residue reads
        let mut max_part_read = 0usize;
        for (i, res) in self.residue_config.iter().enumerate() {
            let actual = (self.blocksize[1] as u32
                * if self.residue_types[i] == 2 {
                    ch as u32
                } else {
                    1
                })
                / 2;
            let n_read = res.end.min(actual) - res.begin.min(actual);
            max_part_read = max_part_read.max((n_read / res.part_size) as usize);
        }
        self.part_classdata = vec![vec![0; max_part_read]; ch];

        self.first_audio_page_offset = if self.r.next_seg == -1 { self.r.pos } else { 0 };
        true
    }

    // ---- decoding ----

    /// (len, left, right)
    fn decode_packet(&mut self) -> Option<(i32, i32, i32)> {
        let (mode, mut left_start, left_end, right_start, right_end) = self.decode_initial()?;
        let len =
            self.decode_packet_rest(mode, &mut left_start, left_end, right_start, right_end)?;
        Some((len, left_start, right_start))
    }

    fn decode_initial(&mut self) -> Option<(usize, i32, i32, i32, i32)> {
        self.channel_buffer_start = 0;
        self.channel_buffer_end = 0;
        let r = &mut self.r;
        loop {
            if r.eof {
                return None;
            }
            if !r.maybe_start_packet() {
                return None;
            }
            if r.get_bits(1) != 0 {
                while r.get8_packet() != EOP {}
                continue;
            }
            break;
        }
        let i = r.get_bits(ilog(self.mode_config.len() as i32 - 1)) as usize;
        if i >= self.mode_config.len() {
            return None;
        }
        let m = self.mode_config[i];
        let (n, prev, next);
        if m.blockflag != 0 {
            n = self.blocksize[1];
            prev = r.get_bits(1);
            next = r.get_bits(1);
        } else {
            prev = 0;
            next = 0;
            n = self.blocksize[0];
        }
        let window_center = n >> 1;
        let b0 = self.blocksize[0];
        let (ls, le) = if m.blockflag != 0 && prev == 0 {
            ((n - b0) >> 2, (n + b0) >> 2)
        } else {
            (0, window_center)
        };
        let (rs, re) = if m.blockflag != 0 && next == 0 {
            ((n * 3 - b0) >> 2, (n * 3 + b0) >> 2)
        } else {
            (window_center, n)
        };
        Some((i, ls, le, rs, re))
    }

    fn do_floor(g: &Floor1, n: i32, target: &mut [f32], final_y: &[i16]) {
        let n2 = n >> 1;
        let mult = g.floor1_multiplier as i32;
        let mut lx = 0i32;
        let mut ly = final_y[0] as i32 * mult;
        for q in 1..g.values as usize {
            let j = g.sorted_order[q] as usize;
            if final_y[j] >= 0 {
                let hy = final_y[j] as i32 * mult;
                let hx = g.xlist[j] as i32;
                if lx != hx {
                    draw_line(target, lx, ly, hx, hy, n2);
                }
                lx = hx;
                ly = hy;
            }
        }
        if lx < n2 {
            for t in target.iter_mut().take(n2 as usize).skip(lx as usize) {
                *t *= INVERSE_DB_TABLE[(ly & 255) as usize];
            }
        }
    }

    fn decode_packet_rest(
        &mut self,
        mode: usize,
        left_start: &mut i32,
        _left_end: i32,
        right_start: i32,
        right_end: i32,
    ) -> Option<i32> {
        let m = self.mode_config[mode];
        let n = self.blocksize[m.blockflag as usize];
        let map = &self.mapping[m.mapping as usize];
        let n2 = n >> 1;
        let ch = self.channels;
        let mut zero_channel = [false; MAX_CHANNELS];
        let r = &mut self.r;

        // floors
        for i in 0..ch {
            let s = map.chan[i].mux as usize;
            let floor = map.submap_floor[s] as usize;
            let g = &self.floor_config[floor];
            let mut ok = false;
            if r.get_bits(1) != 0 {
                const RANGE_LIST: [i32; 4] = [256, 128, 86, 64];
                let range = RANGE_LIST[g.floor1_multiplier as usize - 1];
                let fy = &mut self.final_y[i];
                let mut offset = 2usize;
                fy[0] = r.get_bits(ilog(range) - 1) as i16;
                fy[1] = r.get_bits(ilog(range) - 1) as i16;
                for j in 0..g.partitions as usize {
                    let pclass = g.partition_class_list[j] as usize;
                    let cdim = g.class_dimensions[pclass];
                    let cbits = g.class_subclasses[pclass] as i32;
                    let csub = (1 << cbits) - 1;
                    let mut cval = 0i32;
                    if cbits != 0 {
                        cval = r.decode(&self.codebooks[g.class_masterbooks[pclass] as usize]);
                    }
                    for _ in 0..cdim {
                        let book = g.subclass_books[pclass][(cval & csub) as usize];
                        cval >>= cbits;
                        fy[offset] = if book >= 0 {
                            r.decode(&self.codebooks[book as usize]) as i16
                        } else {
                            0
                        };
                        offset += 1;
                    }
                }
                if r.valid_bits != INVALID_BITS {
                    ok = true;
                    let mut step2 = [false; 256];
                    step2[0] = true;
                    step2[1] = true;
                    for j in 2..g.values as usize {
                        let [low, high] = g.neighbors[j];
                        let (low, high) = (low as usize, high as usize);
                        let pred = predict_point(
                            g.xlist[j] as i32,
                            g.xlist[low] as i32,
                            g.xlist[high] as i32,
                            fy[low] as i32,
                            fy[high] as i32,
                        );
                        let val = fy[j] as i32;
                        let highroom = range - pred;
                        let lowroom = pred;
                        let room = if highroom < lowroom {
                            highroom * 2
                        } else {
                            lowroom * 2
                        };
                        if val != 0 {
                            step2[low] = true;
                            step2[high] = true;
                            step2[j] = true;
                            fy[j] = if val >= room {
                                if highroom > lowroom {
                                    (val - lowroom + pred) as i16
                                } else {
                                    (pred - val + highroom - 1) as i16
                                }
                            } else if val & 1 != 0 {
                                (pred - ((val + 1) >> 1)) as i16
                            } else {
                                (pred + (val >> 1)) as i16
                            };
                        } else {
                            step2[j] = false;
                            fy[j] = pred as i16;
                        }
                    }
                    for j in 0..g.values as usize {
                        if !step2[j] {
                            fy[j] = -1;
                        }
                    }
                }
            }
            if !ok {
                zero_channel[i] = true;
            }
        }

        // re-enable coupled channels
        let really_zero = zero_channel;
        for i in 0..map.coupling_steps as usize {
            let (mg, an) = (map.chan[i].magnitude as usize, map.chan[i].angle as usize);
            if !zero_channel[mg] || !zero_channel[an] {
                zero_channel[mg] = false;
                zero_channel[an] = false;
            }
        }

        // residues
        for i in 0..map.submaps as usize {
            let mut outputs: [Option<usize>; MAX_CHANNELS] = [None; MAX_CHANNELS];
            let mut do_not_decode = [false; MAX_CHANNELS];
            let mut c = 0usize;
            for j in 0..ch {
                if map.chan[j].mux as usize == i {
                    if zero_channel[j] {
                        do_not_decode[c] = true;
                        outputs[c] = None;
                    } else {
                        do_not_decode[c] = false;
                        outputs[c] = Some(j);
                    }
                    c += 1;
                }
            }
            let rn = map.submap_residue[i] as usize;
            decode_residue(
                r,
                &self.codebooks,
                &self.residue_config[rn],
                self.residue_types[rn],
                &mut self.channel_buffers,
                &outputs[..c],
                c as i32,
                n2,
                &do_not_decode,
                &mut self.part_classdata,
            );
        }

        // inverse coupling
        for i in (0..map.coupling_steps as usize).rev() {
            let (mg, an) = (map.chan[i].magnitude as usize, map.chan[i].angle as usize);
            let (m1, a1) = if mg < an {
                let (x, y) = self.channel_buffers.split_at_mut(an);
                (&mut x[mg], &mut y[0])
            } else {
                let (x, y) = self.channel_buffers.split_at_mut(mg);
                (&mut y[0], &mut x[an])
            };
            for j in 0..n2 as usize {
                let (m2, a2);
                if m1[j] > 0.0 {
                    if a1[j] > 0.0 {
                        m2 = m1[j];
                        a2 = m1[j] - a1[j];
                    } else {
                        a2 = m1[j];
                        m2 = m1[j] + a1[j];
                    }
                } else if a1[j] > 0.0 {
                    m2 = m1[j];
                    a2 = m1[j] + a1[j];
                } else {
                    a2 = m1[j];
                    m2 = m1[j] - a1[j];
                }
                m1[j] = m2;
                a1[j] = a2;
            }
        }

        // finish the floors
        for i in 0..ch {
            if really_zero[i] {
                self.channel_buffers[i][..n2 as usize].fill(0.0);
            } else {
                let s = map.chan[i].mux as usize;
                let g = &self.floor_config[map.submap_floor[s] as usize];
                Self::do_floor(g, n, &mut self.channel_buffers[i], &self.final_y[i]);
            }
        }

        // inverse MDCT
        let t = &self.tables[m.blockflag as usize];
        for i in 0..ch {
            inverse_mdct(&mut self.channel_buffers[i], n, t, &mut self.imdct_buf);
        }

        r.flush_packet();

        if self.first_decode {
            self.current_loc = -(n2 as i64);
            self.discard_samples_deferred = n - right_end;
            self.current_loc_valid = true;
            self.first_decode = false;
        } else if self.discard_samples_deferred != 0 {
            if self.discard_samples_deferred >= right_start - *left_start {
                self.discard_samples_deferred -= right_start - *left_start;
                *left_start = right_start;
            } else {
                *left_start += self.discard_samples_deferred;
                self.discard_samples_deferred = 0;
            }
        }

        // the sample number of this packet, if the page tells it
        if r.last_seg_which == r.end_seg_with_known_loc {
            if self.current_loc_valid && r.page_flag & PAGEFLAG_LAST != 0 {
                let current_end = r.known_loc_for_packet as i64;
                if current_end < self.current_loc + (right_end - *left_start) as i64 {
                    if self.previous_length < 0 {
                        return Some(0);
                    }
                    let mut len = if current_end < self.current_loc {
                        0
                    } else {
                        (current_end - self.current_loc) as i32
                    };
                    len += *left_start;
                    if len > right_end {
                        len = right_end;
                    }
                    self.current_loc += len as i64;
                    return Some(len);
                }
            }
            self.current_loc = r.known_loc_for_packet as i64 - (n2 - *left_start) as i64;
            self.current_loc_valid = true;
        }
        if self.current_loc_valid {
            self.current_loc += (right_start - *left_start) as i64;
        }
        Some(right_end)
    }

    fn finish_frame(&mut self, len: i32, left: i32, right: i32) -> i32 {
        if self.previous_length < 0 {
            return 0;
        }
        if self.previous_length != 0 {
            let n = self.previous_length;
            let w = if n * 2 == self.blocksize[0] {
                &self.tables[0].window
            } else if n * 2 == self.blocksize[1] {
                &self.tables[1].window
            } else {
                return 0;
            };
            let (n, left) = (n as usize, left as usize);
            let w = &w[..n];
            for i in 0..self.channels {
                let cb = &mut self.channel_buffers[i][left..left + n];
                let pw = &self.previous_window[i][..n];
                for ((c, p), (w0, w1)) in cb.iter_mut().zip(pw).zip(w.iter().zip(w.iter().rev())) {
                    *c = *c * w0 + p * w1;
                }
            }
        }
        let prev = self.previous_length;
        self.previous_length = len - right;
        if self.previous_length <= 0 {
            self.previous_length = -1;
        }
        let (r0, r1) = (right.max(0) as usize, len.max(0) as usize);
        if r1 > r0 {
            for i in 0..self.channels {
                self.previous_window[i][..r1 - r0]
                    .copy_from_slice(&self.channel_buffers[i][r0..r1]);
            }
        }
        if prev == 0 {
            return 0;
        }
        let right = right.min(len);
        right - left
    }
}

fn read_codebook(r: &mut Reader) -> Option<Codebook> {
    let mut c = Codebook::default();
    if r.get_bits(8) != 0x42 || r.get_bits(8) != 0x43 || r.get_bits(8) != 0x56 {
        return None;
    }
    let x = r.get_bits(8) as i32;
    c.dimensions = ((r.get_bits(8) as i32) << 8) + x;
    let x = r.get_bits(8) as i32;
    let y = r.get_bits(8) as i32;
    c.entries = ((r.get_bits(8) as i32) << 16) + (y << 8) + x;
    let ordered = r.get_bits(1) != 0;
    c.sparse = if ordered { false } else { r.get_bits(1) != 0 };
    if c.dimensions == 0 && c.entries != 0 {
        return None;
    }
    if c.entries != 0 && CODEBOOK_LIMIT / c.entries < c.dimensions {
        return None;
    }
    // stb's allocator refuses size 0, so an empty codebook is an error there too
    if r.valid_bits < 0 || c.entries == 0 {
        return None;
    }
    let entries = c.entries as usize;
    let mut lengths = vec![0u8; entries];
    let mut total = 0i32;
    if ordered {
        let mut current_entry = 0i32;
        let mut current_length = r.get_bits(5) as i32 + 1;
        while current_entry < c.entries {
            let limit = c.entries - current_entry;
            let n = r.get_bits(ilog(limit)) as i32;
            if r.valid_bits < 0 || current_length >= 32 || current_entry + n > c.entries {
                return None;
            }
            lengths[current_entry as usize..(current_entry + n) as usize]
                .fill(current_length as u8);
            current_entry += n;
            current_length += 1;
        }
    } else {
        for l in lengths.iter_mut() {
            let present = if c.sparse { r.get_bits(1) != 0 } else { true };
            if r.valid_bits < 0 {
                return None;
            }
            if present {
                *l = r.get_bits(5) as u8 + 1;
                total += 1;
                if *l == 32 {
                    return None;
                }
            } else {
                *l = NO_CODE;
            }
        }
    }
    if c.sparse && total >= c.entries >> 2 {
        c.sparse = false;
    }
    let sorted_count = if c.sparse {
        total
    } else {
        lengths
            .iter()
            .filter(|&&l| l as i32 > FAST_HUFFMAN_LENGTH && l != NO_CODE)
            .count() as i32
    };
    c.sorted_entries = sorted_count;

    // codewords (and, for sparse books, the compacted lengths and symbols)
    let mut values: Vec<u32> = Vec::new();
    let mut codewords: Vec<u32>;
    if !c.sparse {
        codewords = vec![0; entries];
        c.codeword_lengths = lengths.clone();
    } else {
        codewords = vec![0; sorted_count as usize];
        c.codeword_lengths = vec![0; sorted_count as usize];
        values = vec![0; sorted_count as usize];
    }
    if !compute_codewords(&mut c, &mut codewords, &lengths, &mut values) {
        return None;
    }
    if c.sorted_entries != 0 {
        let mut sc = vec![0u32; c.sorted_entries as usize + 1];
        let mut sv = vec![0i32; c.sorted_entries as usize + 1];
        sv[0] = -1;
        compute_sorted_huffman(&mut c, &codewords, &lengths, &values, &mut sc, &mut sv);
        c.sorted_codewords = Some(sc);
        c.sorted_values = sv;
    }
    c.codewords = if c.sparse { None } else { Some(codewords) };
    compute_accelerated_huffman(&mut c);

    c.lookup_type = r.get_bits(4) as u8;
    if c.lookup_type > 2 {
        return None;
    }
    if c.lookup_type > 0 {
        c.minimum_value = float32_unpack(r.get_bits(32));
        c.delta_value = float32_unpack(r.get_bits(32));
        c.value_bits = r.get_bits(4) as u8 + 1;
        c.sequence_p = r.get_bits(1) != 0;
        if c.lookup_type == 1 {
            let nv = lookup1_values(c.entries, c.dimensions);
            if nv < 0 {
                return None;
            }
            c.lookup_values = nv as u32;
        } else {
            c.lookup_values = (c.entries as u32).wrapping_mul(c.dimensions as u32);
        }
        if c.lookup_values == 0 {
            return None;
        }
        let mut mults = Vec::with_capacity(c.lookup_values as usize);
        for _ in 0..c.lookup_values {
            let q = r.get_bits(c.value_bits as i32);
            if r.valid_bits < 0 {
                return None;
            }
            mults.push(q as u16);
        }
        if c.lookup_type == 1 {
            // pre-expand the lookup1 multiplicands
            let mut last = 0f32;
            let sparse = c.sparse;
            if !(sparse && c.sorted_entries == 0) {
                let cblen = if sparse { c.sorted_entries } else { c.entries } as usize;
                let dims = c.dimensions as usize;
                c.multiplicands = vec![0.0; cblen * dims];
                for j in 0..cblen {
                    let z = if sparse {
                        c.sorted_value(j as i32) as u32
                    } else {
                        j as u32
                    };
                    let mut div = 1u32;
                    for k in 0..dims {
                        let off = ((z / div) % c.lookup_values) as usize;
                        let val = mults[off] as f32 * c.delta_value + c.minimum_value + last;
                        c.multiplicands[j * dims + k] = val;
                        if c.sequence_p {
                            last = val;
                        }
                        if k + 1 < dims {
                            if div > u32::MAX / c.lookup_values {
                                return None;
                            }
                            div *= c.lookup_values;
                        }
                    }
                }
                c.lookup_type = 2;
            }
        } else {
            let mut last = 0f32;
            c.multiplicands = mults
                .iter()
                .map(|&m| {
                    let val = m as f32 * c.delta_value + c.minimum_value + last;
                    if c.sequence_p {
                        last = val;
                    }
                    val
                })
                .collect();
        }
    }
    Some(c)
}

fn compute_codewords(
    c: &mut Codebook,
    codewords: &mut [u32],
    len: &[u8],
    values: &mut [u32],
) -> bool {
    let n = len.len();
    let mut available = [0u32; 32];
    let mut m = 0usize;
    let mut add = |c: &mut Codebook, code: u32, symbol: usize, count: usize, l: u8| {
        if !c.sparse {
            codewords[symbol] = code;
        } else {
            codewords[count] = code;
            c.codeword_lengths[count] = l;
            values[count] = symbol as u32;
        }
    };
    let Some(k) = len.iter().position(|&l| l < NO_CODE) else {
        return true;
    };
    add(c, 0, k, m, len[k]);
    m += 1;
    for i in 1..=len[k] as usize {
        available[i] = 1u32 << (32 - i);
    }
    for i in k + 1..n {
        let l = len[i];
        if l == NO_CODE {
            continue;
        }
        let mut z = l as usize;
        while z > 0 && available[z] == 0 {
            z -= 1;
        }
        if z == 0 {
            return false;
        }
        let res = available[z];
        available[z] = 0;
        add(c, bit_reverse(res), i, m, l);
        m += 1;
        if z != l as usize {
            for y in (z + 1..=l as usize).rev() {
                available[y] = res.wrapping_add(1u32 << (32 - y));
            }
        }
    }
    true
}

fn include_in_sort(c: &Codebook, len: u8) -> bool {
    if c.sparse {
        return true;
    }
    len != NO_CODE && len as i32 > FAST_HUFFMAN_LENGTH
}

fn compute_sorted_huffman(
    c: &mut Codebook,
    codewords: &[u32],
    lengths: &[u8],
    values: &[u32],
    sc: &mut [u32],
    sv: &mut [i32],
) {
    let se = c.sorted_entries as usize;
    if !c.sparse {
        let mut k = 0;
        for i in 0..c.entries as usize {
            if include_in_sort(c, lengths[i]) {
                sc[k] = bit_reverse(codewords[i]);
                k += 1;
            }
        }
    } else {
        for i in 0..se {
            sc[i] = bit_reverse(codewords[i]);
        }
    }
    sc[..se].sort_unstable();
    sc[se] = 0xffff_ffff;
    let len = if c.sparse { se } else { c.entries as usize };
    for i in 0..len {
        let huff_len = if c.sparse {
            lengths[values[i] as usize]
        } else {
            lengths[i]
        };
        if include_in_sort(c, huff_len) {
            let code = bit_reverse(codewords[i]);
            let (mut x, mut n) = (0usize, se);
            while n > 1 {
                let m = x + (n >> 1);
                if sc[m] <= code {
                    x = m;
                    n -= n >> 1;
                } else {
                    n >>= 1;
                }
            }
            if c.sparse {
                sv[x + 1] = values[i] as i32;
                c.codeword_lengths[x] = huff_len;
            } else {
                sv[x + 1] = i as i32;
            }
        }
    }
}

fn compute_accelerated_huffman(c: &mut Codebook) {
    c.fast_huffman.fill(-1);
    let len = if c.sparse {
        c.sorted_entries
    } else {
        c.entries
    } as usize;
    for i in 0..len {
        let l = c.codeword_lengths[i];
        if (l as i32) <= FAST_HUFFMAN_LENGTH {
            let mut z = if c.sparse {
                bit_reverse(c.sorted_codewords.as_ref().map_or(0, |s| s[i]))
            } else {
                c.codewords.as_ref().map_or(0, |w| w[i])
            };
            while (z as usize) < FAST_TABLE_SIZE {
                c.fast_huffman[z as usize] = i as i32;
                z += 1 << l;
            }
        }
    }
}

fn residue_decode(
    r: &mut Reader,
    book: &Codebook,
    target: &mut [f32],
    offset: i32,
    n: i32,
    rtype: u16,
) -> bool {
    if book.dimensions <= 0 {
        return false;
    }
    if rtype == 0 {
        let step = n / book.dimensions;
        for k in 0..step {
            if !codebook_decode_step(r, book, target, (offset + k) as usize, n - offset - k, step) {
                return false;
            }
        }
    } else {
        let (mut k, mut offset) = (0, offset);
        while k < n {
            if !codebook_decode(r, book, target, offset as usize, n - k) {
                return false;
            }
            k += book.dimensions;
            offset += book.dimensions;
        }
    }
    true
}

fn decode_residue(
    r: &mut Reader,
    books: &[Codebook],
    res: &Residue,
    rtype: u16,
    bufs: &mut [Vec<f32>],
    outputs: &[Option<usize>],
    ch: i32,
    n: i32,
    do_not_decode: &[bool],
    part_classdata: &mut [Vec<u32>],
) {
    let classbook = &books[res.classbook as usize];
    let classwords = classbook.dimensions;
    let actual_size = if rtype == 2 {
        (n * ch) as u32
    } else {
        n as u32
    };
    let limit_begin = res.begin.min(actual_size);
    let limit_end = res.end.min(actual_size);
    let n_read = (limit_end - limit_begin) as i32;
    let part_read = n_read / res.part_size as i32;
    let part_size = res.part_size as i32;
    let begin = res.begin as i32;
    let cw = classwords.max(0) as usize;

    for (i, o) in outputs.iter().enumerate() {
        if !do_not_decode[i] {
            if let Some(b) = o {
                bufs[*b][..n as usize].fill(0.0);
            }
        }
    }

    if rtype == 2 && ch != 1 {
        if do_not_decode[..ch as usize].iter().all(|&d| d) {
            return;
        }
        for pass in 0..8 {
            let (mut pcount, mut class_set) = (0i32, 0usize);
            while pcount < part_read {
                let z0 = begin + pcount * part_size;
                let (mut c_inter, mut p_inter) = if ch == 2 {
                    (z0 & 1, z0 >> 1)
                } else {
                    (z0 % ch, z0 / ch)
                };
                if pass == 0 {
                    let q = r.decode(classbook);
                    if q == EOP {
                        return;
                    }
                    if let Some(slot) = part_classdata[0].get_mut(class_set) {
                        *slot = q as u32;
                    }
                }
                let mut i = 0;
                while i < classwords && pcount < part_read {
                    let mut z = begin + pcount * part_size;
                    let q = part_classdata[0].get(class_set).copied().unwrap_or(0) as usize;
                    let c = res.classdata.get(q * cw + i as usize).copied().unwrap_or(0) as usize;
                    let b = res.residue_books.get(c).map_or(-1, |rb| rb[pass]);
                    if b >= 0 {
                        if !codebook_decode_deinterleave_repeat(
                            r,
                            &books[b as usize],
                            bufs,
                            outputs,
                            ch,
                            &mut c_inter,
                            &mut p_inter,
                            n,
                            part_size,
                        ) {
                            return;
                        }
                    } else {
                        z += part_size;
                        if ch == 2 {
                            c_inter = z & 1;
                            p_inter = z >> 1;
                        } else {
                            c_inter = z % ch;
                            p_inter = z / ch;
                        }
                    }
                    i += 1;
                    pcount += 1;
                }
                class_set += 1;
            }
        }
        return;
    }

    for pass in 0..8 {
        let (mut pcount, mut class_set) = (0i32, 0usize);
        while pcount < part_read {
            if pass == 0 {
                for j in 0..ch as usize {
                    if !do_not_decode[j] {
                        let temp = r.decode(classbook);
                        if temp == EOP {
                            return;
                        }
                        if let Some(slot) = part_classdata[j].get_mut(class_set) {
                            *slot = temp as u32;
                        }
                    }
                }
            }
            let mut i = 0;
            while i < classwords && pcount < part_read {
                for j in 0..ch as usize {
                    if !do_not_decode[j] {
                        let q = part_classdata[j].get(class_set).copied().unwrap_or(0) as usize;
                        let c =
                            res.classdata.get(q * cw + i as usize).copied().unwrap_or(0) as usize;
                        let b = res.residue_books.get(c).map_or(-1, |rb| rb[pass]);
                        if b >= 0 {
                            let Some(bi) = outputs[j] else { continue };
                            let offset = begin + pcount * part_size;
                            if !residue_decode(
                                r,
                                &books[b as usize],
                                &mut bufs[bi],
                                offset,
                                part_size,
                                rtype,
                            ) {
                                return;
                            }
                        }
                    }
                }
                i += 1;
                pcount += 1;
            }
            class_set += 1;
        }
    }
}
