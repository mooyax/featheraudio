//! MP3 decoder (MPEG-1/2/2.5 Layer I, II and III): a port of minimp3 by lieff and
//! contributors (<https://github.com/lieff/minimp3>, CC0 / public domain), with the file
//! handling of its minimp3_ex.h (ID3 / APE tags skipped, gapless playback from the LAME /
//! Xing tag: encoder delay and padding removed).
//!
//! The output is bit-identical to minimp3 built without SIMD (`MINIMP3_NO_SIMD`) and with
//! float output, through `mp3dec_load_buf`. (minimp3's SSE / NEON code adds a few numbers in
//! a different order, so it can differ from both in the last bit.)
//!
//! The structure follows the C code; pointers became indices, and every access is checked,
//! so broken or hostile files end the stream instead of reading out of bounds.

// the constants are minimp3's, as written there
#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::excessive_precision,
    clippy::approx_constant
)]

use super::mp3_tables::*;
use std::sync::Arc;

const MAX_FREE_FORMAT_FRAME_SIZE: i32 = 2304;
const MAX_FRAME_SYNC_MATCHES: i32 = 10;
const MAX_L3_FRAME_PAYLOAD_BYTES: usize = MAX_FREE_FORMAT_FRAME_SIZE as usize;
const MAX_BITRESERVOIR_BYTES: usize = 511;
const SHORT_BLOCK_TYPE: u8 = 2;
const STOP_BLOCK_TYPE: u8 = 3;
const MODE_MONO: u8 = 3;
const MODE_JOINT_STEREO: u8 = 1;
const HDR_SIZE: i32 = 4;
const MAX_SCFI: i32 = (255 - 4 - 210 + 3) & !3; // BITS_DEQUANTIZER_OUT = -1
pub(crate) const MAX_SAMPLES_PER_FRAME: usize = 1152 * 2;
/// where the scale factors start in Dec::grbuf
const GRBUF: usize = 2 * 576;

// ---------------------------------------------------------------------------------------------
// frame header (4 bytes)

fn is_mono(h: &[u8]) -> bool {
    h[3] & 0xC0 == 0xC0
}
fn is_ms_stereo(h: &[u8]) -> bool {
    h[3] & 0xE0 == 0x60
}
fn is_free_format(h: &[u8]) -> bool {
    h[2] & 0xF0 == 0
}
fn is_crc(h: &[u8]) -> bool {
    h[1] & 1 == 0
}
fn test_padding(h: &[u8]) -> bool {
    h[2] & 0x2 != 0
}
fn test_mpeg1(h: &[u8]) -> bool {
    h[1] & 0x8 != 0
}
fn test_not_mpeg25(h: &[u8]) -> bool {
    h[1] & 0x10 != 0
}
fn test_i_stereo(h: &[u8]) -> bool {
    h[3] & 0x10 != 0
}
fn test_ms_stereo(h: &[u8]) -> bool {
    h[3] & 0x20 != 0
}
fn stereo_mode(h: &[u8]) -> u8 {
    (h[3] >> 6) & 3
}
fn stereo_mode_ext(h: &[u8]) -> u8 {
    (h[3] >> 4) & 3
}
fn layer(h: &[u8]) -> u8 {
    (h[1] >> 1) & 3
}
fn bitrate_index(h: &[u8]) -> u8 {
    h[2] >> 4
}
fn sample_rate_index(h: &[u8]) -> u8 {
    (h[2] >> 2) & 3
}
fn my_sample_rate(h: &[u8]) -> i32 {
    sample_rate_index(h) as i32 + (((h[1] >> 3) & 1) as i32 + ((h[1] >> 4) & 1) as i32) * 3
}
fn is_frame_576(h: &[u8]) -> bool {
    h[1] & 14 == 2
}
fn is_layer_1(h: &[u8]) -> bool {
    h[1] & 6 == 6
}

fn hdr_valid(h: &[u8]) -> bool {
    h.len() >= 4
        && h[0] == 0xff
        && ((h[1] & 0xF0) == 0xf0 || (h[1] & 0xFE) == 0xe2)
        && layer(h) != 0
        && bitrate_index(h) != 15
        && sample_rate_index(h) != 3
}

fn hdr_compare(h1: &[u8], h2: &[u8]) -> bool {
    hdr_valid(h2)
        && ((h1[1] ^ h2[1]) & 0xFE) == 0
        && ((h1[2] ^ h2[2]) & 0x0C) == 0
        && is_free_format(h1) == is_free_format(h2)
}

fn hdr_bitrate_kbps(h: &[u8]) -> u32 {
    let i =
        (test_mpeg1(h) as usize) * 45 + (layer(h) as usize - 1) * 15 + bitrate_index(h) as usize;
    2 * HALFRATE[i] as u32
}

fn hdr_sample_rate_hz(h: &[u8]) -> u32 {
    const HZ: [u32; 3] = [44100, 48000, 32000];
    HZ[sample_rate_index(h) as usize] >> (!test_mpeg1(h)) as u32 >> (!test_not_mpeg25(h)) as u32
}

fn hdr_frame_samples(h: &[u8]) -> u32 {
    if is_layer_1(h) {
        384
    } else {
        1152 >> is_frame_576(h) as u32
    }
}

fn hdr_frame_bytes(h: &[u8], free_format_size: i32) -> i32 {
    let mut frame_bytes =
        (hdr_frame_samples(h) * hdr_bitrate_kbps(h) * 125 / hdr_sample_rate_hz(h)) as i32;
    if is_layer_1(h) {
        frame_bytes &= !3;
    }
    if frame_bytes != 0 {
        frame_bytes
    } else {
        free_format_size
    }
}

fn hdr_padding(h: &[u8]) -> i32 {
    if test_padding(h) {
        if is_layer_1(h) {
            4
        } else {
            1
        }
    } else {
        0
    }
}

// ---------------------------------------------------------------------------------------------
// bit reader

#[derive(Clone, Copy, Default)]
struct Bs {
    pos: i32,
    limit: i32,
}

impl Bs {
    fn new(bytes: i32) -> Bs {
        Bs {
            pos: 0,
            limit: bytes * 8,
        }
    }
}

fn byte(buf: &[u8], i: usize) -> u32 {
    buf.get(i).copied().unwrap_or(0) as u32
}

fn get_bits(buf: &[u8], bs: &mut Bs, n: i32) -> u32 {
    let s = (bs.pos & 7) as u32;
    let mut shl = n + s as i32;
    let mut p = (bs.pos >> 3) as usize;
    bs.pos += n;
    if bs.pos > bs.limit {
        return 0;
    }
    let mut next = byte(buf, p) & (255 >> s);
    p += 1;
    let mut cache = 0u32;
    loop {
        shl -= 8;
        if shl <= 0 {
            break;
        }
        cache |= next << shl;
        next = byte(buf, p);
        p += 1;
    }
    cache | (next >> -shl)
}

// ---------------------------------------------------------------------------------------------
// Layer I / II

struct L12ScaleInfo {
    scf: [f32; 3 * 64],
    total_bands: u8,
    stereo_bands: u8,
    bitalloc: [u8; 64],
    scfcod: [u8; 64],
}

/// (tab_offset, code_tab_width, band_count) triples
fn l12_subband_alloc_table(hdr: &[u8], sci: &mut L12ScaleInfo) -> &'static [u8] {
    let mode = stereo_mode(hdr);
    let stereo_bands: i32 = if mode == MODE_MONO {
        0
    } else if mode == MODE_JOINT_STEREO {
        ((stereo_mode_ext(hdr) as i32) << 2) + 4
    } else {
        32
    };
    let (alloc, nbands): (&'static [u8], i32);
    if is_layer_1(hdr) {
        alloc = &G_ALLOC_L1;
        nbands = 32;
    } else if !test_mpeg1(hdr) {
        alloc = &G_ALLOC_L2M2;
        nbands = 30;
    } else {
        let sample_rate_idx = sample_rate_index(hdr);
        let mut kbps = hdr_bitrate_kbps(hdr) >> (mode != MODE_MONO) as u32;
        if kbps == 0 {
            kbps = 192; // free format
        }
        if kbps < 56 {
            alloc = &G_ALLOC_L2M1_LOWRATE;
            nbands = if sample_rate_idx == 2 { 12 } else { 8 };
        } else if kbps >= 96 && sample_rate_idx != 1 {
            alloc = &G_ALLOC_L2M1;
            nbands = 30;
        } else {
            alloc = &G_ALLOC_L2M1;
            nbands = 27;
        }
    }
    sci.total_bands = nbands as u8;
    sci.stereo_bands = stereo_bands.min(nbands) as u8;
    alloc
}

fn l12_read_scalefactors(
    buf: &[u8],
    bs: &mut Bs,
    pba: &[u8],
    scfcod: &[u8],
    bands: usize,
    scf: &mut [f32],
) {
    let mut o = 0usize;
    for i in 0..bands {
        let mut s = 0f32;
        let ba = pba[i] as i32;
        let mask = if ba != 0 {
            4 + ((19 >> scfcod[i]) & 3)
        } else {
            0
        };
        let mut m = 4;
        while m != 0 {
            if mask & m != 0 {
                let b = get_bits(buf, bs, 6) as i32;
                let idx = (ba * 3 - 6 + b % 3) as usize;
                s = G_DEQ_L12.get(idx).copied().unwrap_or(0.0) * ((1i32 << 21) >> (b / 3)) as f32;
            }
            scf[o] = s;
            o += 1;
            m >>= 1;
        }
    }
}

fn l12_read_scale_info(hdr: &[u8], buf: &[u8], bs: &mut Bs, sci: &mut L12ScaleInfo) {
    let alloc = l12_subband_alloc_table(hdr, sci);
    let (mut k, mut ba_bits, mut ai) = (0i32, 0i32, 0usize);
    let mut tab = 0usize;
    for i in 0..sci.total_bands as i32 {
        if i == k {
            k += alloc[ai + 2] as i32;
            ba_bits = alloc[ai + 1] as i32;
            tab = alloc[ai] as usize;
            ai += 3;
        }
        let mut ba = G_BITALLOC_CODE_TAB[tab + get_bits(buf, bs, ba_bits) as usize];
        sci.bitalloc[2 * i as usize] = ba;
        if i < sci.stereo_bands as i32 {
            ba = G_BITALLOC_CODE_TAB[tab + get_bits(buf, bs, ba_bits) as usize];
        }
        sci.bitalloc[2 * i as usize + 1] = if sci.stereo_bands != 0 { ba } else { 0 };
    }
    for i in 0..2 * sci.total_bands as usize {
        sci.scfcod[i] = if sci.bitalloc[i] != 0 {
            if is_layer_1(hdr) {
                2
            } else {
                get_bits(buf, bs, 2) as u8
            }
        } else {
            6
        };
    }
    let (bitalloc, scfcod) = (sci.bitalloc, sci.scfcod);
    l12_read_scalefactors(
        buf,
        bs,
        &bitalloc,
        &scfcod,
        sci.total_bands as usize * 2,
        &mut sci.scf,
    );
    for i in sci.stereo_bands as usize..sci.total_bands as usize {
        sci.bitalloc[2 * i + 1] = 0;
    }
}

fn l12_dequantize_granule(
    grbuf: &mut [f32],
    start: usize,
    buf: &[u8],
    bs: &mut Bs,
    sci: &L12ScaleInfo,
    group_size: i32,
) -> i32 {
    let mut choff: isize = 576;
    for j in 0..4 {
        let mut dst = (start + (group_size * j) as usize) as isize;
        for i in 0..2 * sci.total_bands as usize {
            let ba = sci.bitalloc[i] as i32;
            if ba != 0 {
                if ba < 17 {
                    let half = (1 << (ba - 1)) - 1;
                    for k in 0..group_size {
                        let v = (get_bits(buf, bs, ba) as i32 - half) as f32;
                        if let Some(d) = grbuf.get_mut((dst + k as isize) as usize) {
                            *d = v;
                        }
                    }
                } else {
                    let m: u32 = (2 << (ba - 17)) + 1; // 3, 5, 9
                    let mut code = get_bits(buf, bs, (m + 2 - (m >> 3)) as i32); // 5, 7, 10
                    for k in 0..group_size {
                        let v = ((code % m).wrapping_sub(m / 2)) as i32 as f32;
                        if let Some(d) = grbuf.get_mut((dst + k as isize) as usize) {
                            *d = v;
                        }
                        code /= m;
                    }
                }
            }
            dst += choff;
            choff = 18 - choff;
        }
    }
    group_size * 4
}

fn l12_apply_scf_384(sci: &L12ScaleInfo, scf: &[f32], dst: &mut [f32]) {
    let sb = sci.stereo_bands as usize * 18;
    let n = (sci.total_bands as usize).saturating_sub(sci.stereo_bands as usize) * 18;
    dst.copy_within(sb..sb + n, 576 + sb);
    for i in 0..sci.total_bands as usize {
        let (d, s) = (i * 18, i * 6);
        for k in 0..12 {
            dst[d + k] *= scf[s];
            dst[d + k + 576] *= scf[s + 3];
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Layer III

#[derive(Clone, Copy)]
struct GrInfo {
    sfbtab: &'static [u8],
    part_23_length: u16,
    big_values: u16,
    scalefac_compress: u16,
    global_gain: u8,
    block_type: u8,
    mixed_block_flag: u8,
    n_long_sfb: u8,
    n_short_sfb: u8,
    table_select: [u8; 3],
    region_count: [u8; 3],
    subblock_gain: [u8; 3],
    preflag: u8,
    scalefac_scale: u8,
    count1_table: u8,
    scfsi: u8,
}

impl Default for GrInfo {
    fn default() -> Self {
        GrInfo {
            sfbtab: &G_SCF_LONG[..23],
            part_23_length: 0,
            big_values: 0,
            scalefac_compress: 0,
            global_gain: 0,
            block_type: 0,
            mixed_block_flag: 0,
            n_long_sfb: 0,
            n_short_sfb: 0,
            table_select: [0; 3],
            region_count: [0; 3],
            subblock_gain: [0; 3],
            preflag: 0,
            scalefac_scale: 0,
            count1_table: 0,
            scfsi: 0,
        }
    }
}

fn l3_read_side_info(buf: &[u8], bs: &mut Bs, gr: &mut [GrInfo; 4], hdr: &[u8]) -> i32 {
    let mut scfsi: u32 = 0;
    let main_data_begin: i32;
    let mut part_23_sum = 0i32;
    let mut gr_count = if is_mono(hdr) { 1 } else { 2 };
    let mut sr_idx = my_sample_rate(hdr);
    sr_idx -= (sr_idx != 0) as i32;
    let sr = sr_idx as usize;

    if test_mpeg1(hdr) {
        gr_count *= 2;
        main_data_begin = get_bits(buf, bs, 9) as i32;
        scfsi = get_bits(buf, bs, 7 + gr_count);
    } else {
        main_data_begin = (get_bits(buf, bs, 8 + gr_count) >> gr_count) as i32;
    }

    for g in gr.iter_mut().take(gr_count as usize) {
        if is_mono(hdr) {
            scfsi <<= 4;
        }
        g.part_23_length = get_bits(buf, bs, 12) as u16;
        part_23_sum += g.part_23_length as i32;
        g.big_values = get_bits(buf, bs, 9) as u16;
        if g.big_values > 288 {
            return -1;
        }
        g.global_gain = get_bits(buf, bs, 8) as u8;
        g.scalefac_compress = get_bits(buf, bs, if test_mpeg1(hdr) { 4 } else { 9 }) as u16;
        g.sfbtab = &G_SCF_LONG[sr * 23..sr * 23 + 23];
        g.n_long_sfb = 22;
        g.n_short_sfb = 0;
        let tables: u32;
        if get_bits(buf, bs, 1) != 0 {
            g.block_type = get_bits(buf, bs, 2) as u8;
            if g.block_type == 0 {
                return -1;
            }
            g.mixed_block_flag = get_bits(buf, bs, 1) as u8;
            g.region_count[0] = 7;
            g.region_count[1] = 255;
            if g.block_type == SHORT_BLOCK_TYPE {
                scfsi &= 0x0F0F;
                if g.mixed_block_flag == 0 {
                    g.region_count[0] = 8;
                    g.sfbtab = &G_SCF_SHORT[sr * 40..sr * 40 + 40];
                    g.n_long_sfb = 0;
                    g.n_short_sfb = 39;
                } else {
                    g.sfbtab = &G_SCF_MIXED[sr * 40..sr * 40 + 40];
                    g.n_long_sfb = if test_mpeg1(hdr) { 8 } else { 6 };
                    g.n_short_sfb = 30;
                }
            }
            tables = get_bits(buf, bs, 10) << 5;
            g.subblock_gain[0] = get_bits(buf, bs, 3) as u8;
            g.subblock_gain[1] = get_bits(buf, bs, 3) as u8;
            g.subblock_gain[2] = get_bits(buf, bs, 3) as u8;
        } else {
            g.block_type = 0;
            g.mixed_block_flag = 0;
            tables = get_bits(buf, bs, 15);
            g.region_count[0] = get_bits(buf, bs, 4) as u8;
            g.region_count[1] = get_bits(buf, bs, 3) as u8;
            g.region_count[2] = 255;
        }
        g.table_select[0] = (tables >> 10) as u8;
        g.table_select[1] = ((tables >> 5) & 31) as u8;
        g.table_select[2] = (tables & 31) as u8;
        g.preflag = if test_mpeg1(hdr) {
            get_bits(buf, bs, 1) as u8
        } else {
            (g.scalefac_compress >= 500) as u8
        };
        g.scalefac_scale = get_bits(buf, bs, 1) as u8;
        g.count1_table = get_bits(buf, bs, 1) as u8;
        g.scfsi = ((scfsi >> 12) & 15) as u8;
        scfsi <<= 4;
    }

    if part_23_sum + bs.pos > bs.limit + main_data_begin * 8 {
        return -1;
    }
    main_data_begin
}

fn l3_read_scalefactors(
    scf: &mut [u8; 40],
    ist_pos: &mut [u8],
    scf_size: &[u8; 4],
    scf_count: &[u8],
    buf: &[u8],
    bs: &mut Bs,
    mut scfsi: i32,
) {
    let mut o = 0usize;
    let mut i = 0;
    while i < 4 && scf_count.get(i).is_some_and(|&c| c != 0) {
        let cnt = scf_count[i] as usize;
        let end = (o + cnt).min(39);
        if scfsi & 8 != 0 {
            scf[o..end].copy_from_slice(&ist_pos[o..end]);
        } else {
            let bits = scf_size[i] as i32;
            if bits == 0 {
                scf[o..end].fill(0);
                ist_pos[o..end].fill(0);
            } else {
                let max_scf = if scfsi < 0 { (1 << bits) - 1 } else { -1 };
                for k in o..o + cnt {
                    let s = get_bits(buf, bs, bits) as i32;
                    if k < 39 {
                        ist_pos[k] = if s == max_scf { 255 } else { s as u8 };
                        scf[k] = s as u8;
                    }
                }
            }
        }
        o += cnt;
        i += 1;
        scfsi = scfsi.wrapping_mul(2);
    }
    for k in o..o + 3 {
        if let Some(x) = scf.get_mut(k) {
            *x = 0;
        }
    }
}

fn l3_ldexp_q2(mut y: f32, mut exp_q2: i32) -> f32 {
    loop {
        let e = exp_q2.min(30 * 4);
        y *= G_EXPFRAC[(e & 3) as usize] * ((1i32 << 30) >> (e >> 2)) as f32;
        exp_q2 -= e;
        if exp_q2 <= 0 {
            break;
        }
    }
    y
}

fn l3_decode_scalefactors(
    hdr: &[u8],
    ist_pos: &mut [u8],
    buf: &[u8],
    bs: &mut Bs,
    gr: &GrInfo,
    scf: &mut [f32; 40],
    ch: usize,
) {
    let row = (gr.n_short_sfb != 0) as usize + (gr.n_long_sfb == 0) as usize;
    let mut part_off = row * 28;
    let mut scf_size = [0u8; 4];
    let mut iscf = [0u8; 40];
    let scf_shift = gr.scalefac_scale as i32 + 1;
    let mut scfsi = gr.scfsi as i32;

    if test_mpeg1(hdr) {
        let part = G_SCFC_DECODE[gr.scalefac_compress as usize & 15];
        scf_size[0] = part >> 2;
        scf_size[1] = part >> 2;
        scf_size[2] = part & 3;
        scf_size[3] = part & 3;
    } else {
        let ist = (test_i_stereo(hdr) && ch != 0) as i32;
        let mut sfc = (gr.scalefac_compress >> ist) as i32;
        let mut k = ist * 3 * 4;
        while sfc >= 0 {
            let mut modprod = 1i32;
            for i in (0..4).rev() {
                let m = G_MOD.get((k + i) as usize).copied().unwrap_or(1) as i32;
                scf_size[i as usize] = (sfc / modprod % m) as u8;
                modprod *= m;
            }
            sfc -= modprod;
            k += 4;
        }
        part_off += k as usize;
        scfsi = -16;
    }
    let counts = G_SCF_PARTITIONS
        .get(part_off..(part_off + 4).min(G_SCF_PARTITIONS.len()))
        .unwrap_or(&[]);
    l3_read_scalefactors(&mut iscf, ist_pos, &scf_size, counts, buf, bs, scfsi);

    if gr.n_short_sfb != 0 {
        let sh = 3 - scf_shift;
        let mut i = 0usize;
        while i < gr.n_short_sfb as usize {
            let b = gr.n_long_sfb as usize + i;
            iscf[b] = iscf[b].wrapping_add(gr.subblock_gain[0] << sh);
            iscf[b + 1] = iscf[b + 1].wrapping_add(gr.subblock_gain[1] << sh);
            iscf[b + 2] = iscf[b + 2].wrapping_add(gr.subblock_gain[2] << sh);
            i += 3;
        }
    } else if gr.preflag != 0 {
        for i in 0..10 {
            iscf[11 + i] = iscf[11 + i].wrapping_add(G_PREAMP[i]);
        }
    }

    let gain_exp = gr.global_gain as i32 - 4 - 210 - if is_ms_stereo(hdr) { 2 } else { 0 };
    let gain = l3_ldexp_q2((1 << (MAX_SCFI / 4)) as f32, MAX_SCFI - gain_exp);
    for i in 0..(gr.n_long_sfb as usize + gr.n_short_sfb as usize).min(40) {
        scf[i] = l3_ldexp_q2(gain, (iscf[i] as i32) << scf_shift);
    }
}

fn l3_pow_43(mut x: i32) -> f32 {
    let mut mult = 256i32;
    if x < 129 {
        return G_POW43.get((16 + x) as usize).copied().unwrap_or(0.0);
    }
    if x < 1024 {
        mult = 16;
        x <<= 3;
    }
    let sign = (2 * x) & 64;
    let frac = ((x & 63) - sign) as f32 / ((x & !63) + sign) as f32;
    G_POW43
        .get((16 + ((x + sign) >> 6)) as usize)
        .copied()
        .unwrap_or(0.0)
        * (1.0 + frac * ((4.0f32 / 3.0) + frac * (2.0f32 / 9.0)))
        * mult as f32
}

/// Huffman decoding of one granule of one channel into `dst` (576 values).
fn l3_huffman(
    dst: &mut [f32],
    buf: &[u8],
    bs: &mut Bs,
    gr: &GrInfo,
    scf: &[f32; 40],
    layer3gr_limit: i32,
) {
    let mut one = 0f32;
    let mut ireg = 0usize;
    let mut big_val_cnt = gr.big_values as i32;
    let sfb = gr.sfbtab;
    let mut sfbi = 0usize;
    let mut scfi = 0usize;
    let mut next = (bs.pos / 8) as usize;
    let mut cache: u32 =
        ((((byte(buf, next) * 256 + byte(buf, next + 1)) * 256 + byte(buf, next + 2)) * 256)
            + byte(buf, next + 3))
        .wrapping_shl((bs.pos & 7) as u32);
    let mut sh: i32 = (bs.pos & 7) - 8;
    next += 4;
    let mut d = 0usize;

    macro_rules! peek {
        ($n:expr) => {
            cache.wrapping_shr(32u32.wrapping_sub($n as u32))
        };
    }
    macro_rules! flush {
        ($n:expr) => {{
            cache = cache.wrapping_shl($n as u32);
            sh += $n as i32;
        }};
    }
    macro_rules! check {
        () => {
            while sh >= 0 {
                cache |= byte(buf, next).wrapping_shl(sh as u32);
                next += 1;
                sh -= 8;
            }
        };
    }
    let sfb_at = |i: usize| sfb.get(i).copied().unwrap_or(0) as i32;
    let tab = |i: i32| TABS.get(i.max(0) as usize).copied().unwrap_or(0) as i32;

    while big_val_cnt > 0 {
        let tab_num = gr.table_select[ireg.min(2)] as usize;
        let mut sfb_cnt = gr.region_count[ireg.min(2)] as i32;
        ireg += 1;
        let base = TABINDEX[tab_num] as i32;
        let linbits = G_LINBITS[tab_num] as i32;
        loop {
            let np = sfb_at(sfbi) / 2;
            sfbi += 1;
            if np == 0 {
                // (not possible with the real tables: their bands add up to 576)
                big_val_cnt = 0;
                break;
            }
            let mut pairs = big_val_cnt.min(np);
            one = scf.get(scfi).copied().unwrap_or(0.0);
            scfi += 1;
            loop {
                let mut w = 5;
                let mut leaf = tab(base + peek!(w) as i32);
                while leaf < 0 {
                    flush!(w);
                    w = leaf & 7;
                    leaf = tab(base + peek!(w) as i32 - (leaf >> 3));
                }
                flush!(leaf >> 8);
                for _ in 0..2 {
                    let mut lsb = leaf & 0x0F;
                    let v;
                    if linbits != 0 && lsb == 15 {
                        lsb += peek!(linbits) as i32;
                        flush!(linbits);
                        check!();
                        v = one * l3_pow_43(lsb) * (if (cache as i32) < 0 { -1.0 } else { 1.0 });
                    } else {
                        v = G_POW43[(16 + lsb - 16 * (cache >> 31) as i32) as usize] * one;
                    }
                    if let Some(x) = dst.get_mut(d) {
                        *x = v;
                    }
                    d += 1;
                    flush!(if lsb != 0 { 1 } else { 0 });
                    leaf >>= 4;
                }
                check!();
                pairs -= 1;
                if pairs == 0 {
                    break;
                }
            }
            big_val_cnt -= np;
            if big_val_cnt <= 0 {
                break;
            }
            sfb_cnt -= 1;
            if sfb_cnt < 0 {
                break;
            }
        }
    }

    let mut np = 1 - big_val_cnt;
    loop {
        let cb: &[u8] = if gr.count1_table != 0 { &TAB33 } else { &TAB32 };
        let mut leaf = cb[peek!(4) as usize] as i32;
        if leaf & 8 == 0 {
            let idx = (leaf >> 3) as u32
                + (cache << 4).wrapping_shr(32u32.wrapping_sub((leaf & 3) as u32));
            leaf = cb.get(idx as usize).copied().unwrap_or(0) as i32;
        }
        flush!(leaf & 7);
        let bspos = next as i32 * 8 - 24 + sh;
        if bspos > layer3gr_limit {
            break;
        }
        macro_rules! reload {
            () => {
                np -= 1;
                if np == 0 {
                    np = sfb_at(sfbi) / 2;
                    sfbi += 1;
                    if np == 0 {
                        break;
                    }
                    one = scf.get(scfi).copied().unwrap_or(0.0);
                    scfi += 1;
                }
            };
        }
        macro_rules! deq {
            ($s:expr) => {
                if leaf & (128 >> $s) != 0 {
                    if let Some(x) = dst.get_mut(d + $s) {
                        *x = if (cache as i32) < 0 { -one } else { one };
                    }
                    flush!(1);
                }
            };
        }
        reload!();
        deq!(0);
        deq!(1);
        reload!();
        deq!(2);
        deq!(3);
        check!();
        d += 4;
    }
    bs.pos = layer3gr_limit;
}

fn l3_midside_stereo(g: &mut [f32], left: usize, n: usize) {
    for i in left..left + n {
        let (a, b) = (g[i], g[i + 576]);
        g[i] = a + b;
        g[i + 576] = a - b;
    }
}

fn l3_stereo_top_band(g: &[f32], sfb: &[u8], nbands: usize, max_band: &mut [i32; 3]) {
    *max_band = [-1; 3];
    let mut right = 576usize;
    for i in 0..nbands.min(sfb.len()) {
        let mut k = 0;
        while k < sfb[i] as usize {
            if g[right + k] != 0.0 || g[right + k + 1] != 0.0 {
                max_band[i % 3] = i as i32;
                break;
            }
            k += 2;
        }
        right += sfb[i] as usize;
    }
}

fn l3_stereo_process(
    g: &mut [f32],
    ist_pos: &[u8],
    sfb: &[u8],
    hdr: &[u8],
    max_band: &[i32; 3],
    mpeg2_sh: i32,
) {
    let mpeg1 = test_mpeg1(hdr);
    let max_pos: u32 = if mpeg1 { 7 } else { 64 };
    let mut left = 0usize;
    let mut i = 0usize;
    while i < sfb.len() && sfb[i] != 0 {
        let n = sfb[i] as usize;
        let ipos = ist_pos.get(i).copied().unwrap_or(0) as u32;
        if i as i32 > max_band[i % 3] && ipos < max_pos {
            let s = if test_ms_stereo(hdr) {
                1.41421356f32
            } else {
                1.0
            };
            let (mut kl, mut kr);
            if mpeg1 {
                kl = G_PAN[2 * ipos as usize];
                kr = G_PAN[2 * ipos as usize + 1];
            } else {
                kl = 1.0;
                kr = l3_ldexp_q2(1.0, (((ipos + 1) >> 1) << mpeg2_sh) as i32);
                if ipos & 1 != 0 {
                    kl = kr;
                    kr = 1.0;
                }
            }
            let (kl, kr) = (kl * s, kr * s);
            for j in left..left + n {
                g[j + 576] = g[j] * kr;
                g[j] *= kl;
            }
        } else if test_ms_stereo(hdr) {
            l3_midside_stereo(g, left, n);
        }
        left += n;
        i += 1;
    }
}

fn l3_intensity_stereo(g: &mut [f32], ist_pos: &mut [u8], gr: &[GrInfo], hdr: &[u8]) {
    let mut max_band = [0i32; 3];
    let n_sfb = gr[0].n_long_sfb as i32 + gr[0].n_short_sfb as i32;
    let max_blocks = if gr[0].n_short_sfb != 0 { 3 } else { 1 };
    l3_stereo_top_band(g, gr[0].sfbtab, n_sfb as usize, &mut max_band);
    if gr[0].n_long_sfb != 0 {
        let m = max_band[0].max(max_band[1]).max(max_band[2]);
        max_band = [m; 3];
    }
    for i in 0..max_blocks {
        let default_pos = if test_mpeg1(hdr) { 3 } else { 0 };
        let itop = n_sfb - max_blocks + i;
        let prev = itop - max_blocks;
        let v = if max_band[i as usize] >= prev {
            default_pos
        } else {
            ist_pos.get(prev as usize).copied().unwrap_or(0)
        };
        if let Some(x) = ist_pos.get_mut(itop as usize) {
            *x = v;
        }
    }
    l3_stereo_process(
        g,
        ist_pos,
        gr[0].sfbtab,
        hdr,
        &max_band,
        (gr[1].scalefac_compress & 1) as i32,
    );
}

fn l3_reorder(g: &mut [f32], start: usize, scratch: &mut [f32], sfb: &[u8]) {
    let mut src = start;
    let mut d = 0usize;
    let mut si = 0usize;
    while si < sfb.len() && sfb[si] != 0 {
        let len = sfb[si] as usize;
        for _ in 0..len {
            scratch[d] = g[src];
            scratch[d + 1] = g[src + len];
            scratch[d + 2] = g[src + 2 * len];
            d += 3;
            src += 1;
        }
        si += 3;
        src += 2 * len;
    }
    g[start..start + d].copy_from_slice(&scratch[..d]);
}

fn l3_antialias(g: &mut [f32], mut nbands: i32) {
    let mut b = 0usize;
    while nbands > 0 {
        for i in 0..8 {
            let u = g[b + 18 + i];
            let d = g[b + 17 - i];
            g[b + 18 + i] = u * G_AA[i] - d * G_AA[8 + i];
            g[b + 17 - i] = u * G_AA[8 + i] + d * G_AA[i];
        }
        nbands -= 1;
        b += 18;
    }
}

fn l3_dct3_9(y: &mut [f32; 9]) {
    let (mut s0, mut s2, mut s4, mut s6, mut s8) = (y[0], y[2], y[4], y[6], y[8]);
    let mut t0 = s0 + s6 * 0.5;
    s0 -= s6;
    let mut t4 = (s4 + s2) * 0.93969262;
    let mut t2 = (s8 + s2) * 0.76604444;
    s6 = (s4 - s8) * 0.17364818;
    s4 += s8 - s2;

    s2 = s0 - s4 * 0.5;
    y[4] = s4 + s0;
    s8 = t0 - t2 + s6;
    s0 = t0 - t4 + t2;
    s4 = t0 + t4 - s6;

    let (mut s1, mut s3, mut s5, mut s7) = (y[1], y[3], y[5], y[7]);

    s3 *= 0.86602540;
    t0 = (s5 + s1) * 0.98480775;
    t4 = (s5 - s7) * 0.34202014;
    t2 = (s1 + s7) * 0.64278761;
    s1 = (s1 - s5 - s7) * 0.86602540;

    s5 = t0 - s3 - t2;
    s7 = t4 - s3 - t0;
    s3 = t4 + s3 - t2;

    y[0] = s4 - s7;
    y[1] = s2 + s1;
    y[2] = s0 - s3;
    y[3] = s8 + s5;
    y[5] = s8 - s5;
    y[6] = s0 + s3;
    y[7] = s2 - s1;
    y[8] = s4 + s7;
}

fn l3_imdct36(
    g: &mut [f32],
    mut gb: usize,
    overlap: &mut [f32],
    mut ov: usize,
    window: &[f32],
    nbands: usize,
) {
    for _ in 0..nbands {
        let mut co = [0f32; 9];
        let mut si = [0f32; 9];
        co[0] = -g[gb];
        si[0] = g[gb + 17];
        for i in 0..4 {
            si[8 - 2 * i] = g[gb + 4 * i + 1] - g[gb + 4 * i + 2];
            co[1 + 2 * i] = g[gb + 4 * i + 1] + g[gb + 4 * i + 2];
            si[7 - 2 * i] = g[gb + 4 * i + 4] - g[gb + 4 * i + 3];
            co[2 + 2 * i] = -(g[gb + 4 * i + 3] + g[gb + 4 * i + 4]);
        }
        l3_dct3_9(&mut co);
        l3_dct3_9(&mut si);
        si[1] = -si[1];
        si[3] = -si[3];
        si[5] = -si[5];
        si[7] = -si[7];
        for i in 0..9 {
            let ovl = overlap[ov + i];
            let sum = co[i] * G_TWID9[9 + i] + si[i] * G_TWID9[i];
            overlap[ov + i] = co[i] * G_TWID9[i] - si[i] * G_TWID9[9 + i];
            g[gb + i] = ovl * window[i] - sum * window[9 + i];
            g[gb + 17 - i] = ovl * window[9 + i] + sum * window[i];
        }
        gb += 18;
        ov += 9;
    }
}

fn l3_idct3(x0: f32, x1: f32, x2: f32, dst: &mut [f32; 3]) {
    let m1 = x1 * 0.86602540;
    let a1 = x0 - x2 * 0.5;
    dst[1] = x0 + x2;
    dst[0] = a1 + m1;
    dst[2] = a1 - m1;
}

/// x: 18 inputs read at x[0], x[3], ...; writes 6 outputs to `dst` and updates overlap[3..6]
/// in `ovl` (the 3 floats at overlap + 6).
fn l3_imdct12(x: &[f32], dst: &mut [f32], ovl3: &mut [f32]) {
    let mut co = [0f32; 3];
    let mut si = [0f32; 3];
    l3_idct3(-x[0], x[6] + x[3], x[12] + x[9], &mut co);
    l3_idct3(x[15], x[12] - x[9], x[6] - x[3], &mut si);
    si[1] = -si[1];
    for i in 0..3 {
        let ovl = ovl3[i];
        let sum = co[i] * G_TWID3[3 + i] + si[i] * G_TWID3[i];
        ovl3[i] = co[i] * G_TWID3[i] - si[i] * G_TWID3[3 + i];
        dst[i] = ovl * G_TWID3[2 - i] - sum * G_TWID3[5 - i];
        dst[5 - i] = ovl * G_TWID3[5 - i] + sum * G_TWID3[2 - i];
    }
}

fn l3_imdct_short(
    g: &mut [f32],
    mut gb: usize,
    overlap: &mut [f32],
    mut ov: usize,
    mut nbands: i32,
) {
    while nbands > 0 {
        let mut tmp = [0f32; 18];
        tmp.copy_from_slice(&g[gb..gb + 18]);
        g[gb..gb + 6].copy_from_slice(&overlap[ov..ov + 6]);
        let mut o6 = [overlap[ov + 6], overlap[ov + 7], overlap[ov + 8]];
        l3_imdct12(&tmp, &mut g[gb + 6..gb + 12], &mut o6);
        l3_imdct12(&tmp[1..], &mut g[gb + 12..gb + 18], &mut o6);
        let mut out = [0f32; 6];
        l3_imdct12(&tmp[2..], &mut out, &mut o6);
        overlap[ov..ov + 6].copy_from_slice(&out);
        overlap[ov + 6..ov + 9].copy_from_slice(&o6);
        nbands -= 1;
        ov += 9;
        gb += 18;
    }
}

fn l3_change_sign(g: &mut [f32]) {
    let mut b = 18usize;
    for _ in 0..16 {
        let mut i = 1;
        while i < 18 {
            g[b + i] = -g[b + i];
            i += 2;
        }
        b += 36;
    }
}

fn l3_imdct_gr(g: &mut [f32], overlap: &mut [f32], block_type: u8, n_long_bands: usize) {
    let (mut gb, mut ov) = (0usize, 0usize);
    if n_long_bands != 0 {
        l3_imdct36(g, 0, overlap, 0, &G_MDCT_WINDOW[..18], n_long_bands);
        gb = 18 * n_long_bands;
        ov = 9 * n_long_bands;
    }
    if block_type == SHORT_BLOCK_TYPE {
        l3_imdct_short(g, gb, overlap, ov, 32 - n_long_bands as i32);
    } else {
        let w = if block_type == STOP_BLOCK_TYPE {
            &G_MDCT_WINDOW[18..]
        } else {
            &G_MDCT_WINDOW[..18]
        };
        l3_imdct36(g, gb, overlap, ov, w, 32 - n_long_bands);
    }
}

// ---------------------------------------------------------------------------------------------
// synthesis

fn dct_ii(g: &mut [f32], n: usize) {
    for k in 0..n {
        let mut t = [[0f32; 8]; 4];
        let mut y = k;
        for i in 0..8 {
            let x0 = g[y + i * 18];
            let x1 = g[y + (15 - i) * 18];
            let x2 = g[y + (16 + i) * 18];
            let x3 = g[y + (31 - i) * 18];
            let t0 = x0 + x3;
            let t1 = x1 + x2;
            let t2 = (x1 - x2) * G_SEC[3 * i];
            let t3 = (x0 - x3) * G_SEC[3 * i + 1];
            t[0][i] = t0 + t1;
            t[1][i] = (t0 - t1) * G_SEC[3 * i + 2];
            t[2][i] = t3 + t2;
            t[3][i] = (t3 - t2) * G_SEC[3 * i + 2];
        }
        for x in t.iter_mut() {
            let (mut x0, mut x1, mut x2, mut x3, mut x4, mut x5, mut x6, mut x7) =
                (x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]);
            let mut xt = x0 - x7;
            x0 += x7;
            x7 = x1 - x6;
            x1 += x6;
            x6 = x2 - x5;
            x2 += x5;
            x5 = x3 - x4;
            x3 += x4;
            x4 = x0 - x3;
            x0 += x3;
            x3 = x1 - x2;
            x1 += x2;
            x[0] = x0 + x1;
            x[4] = (x0 - x1) * 0.70710677;
            x5 += x6;
            x6 = (x6 + x7) * 0.70710677;
            x7 += xt;
            x3 = (x3 + x4) * 0.70710677;
            x5 -= x7 * 0.198912367; // rotate by PI/8
            x7 += x5 * 0.382683432;
            x5 -= x7 * 0.198912367;
            x0 = xt - x6;
            xt += x6;
            x[1] = (xt + x7) * 0.50979561;
            x[2] = (x4 + x3) * 0.54119611;
            x[3] = (x0 - x5) * 0.60134488;
            x[5] = (x0 + x5) * 0.89997619;
            x[6] = (x4 - x3) * 1.30656302;
            x[7] = (xt - x7) * 2.56291556;
        }
        for i in 0..7 {
            g[y] = t[0][i];
            g[y + 18] = t[2][i] + t[3][i] + t[3][i + 1];
            g[y + 2 * 18] = t[1][i] + t[1][i + 1];
            g[y + 3 * 18] = t[2][i + 1] + t[3][i] + t[3][i + 1];
            y += 4 * 18;
        }
        g[y] = t[0][7];
        g[y + 18] = t[2][7] + t[3][7];
        g[y + 2 * 18] = t[1][7];
        g[y + 3 * 18] = t[3][7];
    }
}

#[inline]
fn scale_pcm(sample: f32) -> f32 {
    sample * (1.0 / 32768.0)
}

fn synth_pair(pcm: &mut [f32], p: usize, nch: usize, lins: &[f32], z: usize) {
    let zz = |i: usize| lins[z + i];
    let mut a = (zz(14 * 64) - zz(0)) * 29.0;
    a += (zz(64) + zz(13 * 64)) * 213.0;
    a += (zz(12 * 64) - zz(2 * 64)) * 459.0;
    a += (zz(3 * 64) + zz(11 * 64)) * 2037.0;
    a += (zz(10 * 64) - zz(4 * 64)) * 5153.0;
    a += (zz(5 * 64) + zz(9 * 64)) * 6574.0;
    a += (zz(8 * 64) - zz(6 * 64)) * 37489.0;
    a += zz(7 * 64) * 75038.0;
    pcm[p] = scale_pcm(a);

    let z = z + 2;
    let zz = |i: usize| lins[z + i];
    let mut a = zz(14 * 64) * 104.0;
    a += zz(12 * 64) * 1567.0;
    a += zz(10 * 64) * 9727.0;
    a += zz(8 * 64) * 64019.0;
    a += zz(6 * 64) * -9975.0;
    a += zz(4 * 64) * -45.0;
    a += zz(2 * 64) * 146.0;
    a += zz(0) * -5.0;
    pcm[p + 16 * nch] = scale_pcm(a);
}

/// xl: index of the left channel's subband samples in `g`; dstl: index in `pcm`; lins: base
/// index in `lin`.
fn synth(
    g: &[f32],
    xl: usize,
    pcm: &mut [f32],
    dstl: usize,
    nch: usize,
    lin: &mut [f32],
    lins: usize,
) {
    let xr = xl + 576 * (nch - 1);
    let dstr = dstl + (nch - 1);
    let zlin = lins + 15 * 64;

    lin[zlin + 4 * 15] = g[xl + 18 * 16];
    lin[zlin + 4 * 15 + 1] = g[xr + 18 * 16];
    lin[zlin + 4 * 15 + 2] = g[xl];
    lin[zlin + 4 * 15 + 3] = g[xr];

    lin[zlin + 4 * 31] = g[xl + 1 + 18 * 16];
    lin[zlin + 4 * 31 + 1] = g[xr + 1 + 18 * 16];
    lin[zlin + 4 * 31 + 2] = g[xl + 1];
    lin[zlin + 4 * 31 + 3] = g[xr + 1];

    synth_pair(pcm, dstr, nch, lin, lins + 4 * 15 + 1);
    synth_pair(pcm, dstr + 32 * nch, nch, lin, lins + 4 * 15 + 64 + 1);
    synth_pair(pcm, dstl, nch, lin, lins + 4 * 15);
    synth_pair(pcm, dstl + 32 * nch, nch, lin, lins + 4 * 15 + 64);

    for i in (0..15usize).rev() {
        let zi = zlin + 4 * i;
        lin[zi] = g[xl + 18 * (31 - i)];
        lin[zi + 1] = g[xr + 18 * (31 - i)];
        lin[zi + 2] = g[xl + 1 + 18 * (31 - i)];
        lin[zi + 3] = g[xr + 1 + 18 * (31 - i)];
        lin[zi + 64] = g[xl + 1 + 18 * (1 + i)];
        lin[zi + 64 + 1] = g[xr + 1 + 18 * (1 + i)];
        lin[zi - 64 + 2] = g[xl + 18 * (1 + i)];
        lin[zi - 64 + 3] = g[xr + 18 * (1 + i)];

        // minimp3's S0(0) S2(1) S1(2) S2(3) S1(4) S2(5) S1(6) S2(7), one lane at a time (the
        // four lanes are independent, and each adds up in the same order as in minimp3)
        // zlin[4 * i - 15 * 64 ..= 4 * i + 3], so every offset below is a constant
        let win: &[f32; 15 * 64 + 4] = lin[zi - 15 * 64..zi + 4].try_into().unwrap();
        // the 16 window values for this i (i goes down from 14)
        let wt: &[f32; 16] = G_WIN[(14 - i) * 16..(15 - i) * 16].try_into().unwrap();
        let mut a = [0f32; 4];
        let mut b = [0f32; 4];
        for j in 0..4 {
            // vz = zlin[4 * i - k * 64], vy = zlin[4 * i - (15 - k) * 64]
            let z = |k: usize| win[(15 - k) * 64 + j];
            let y = |k: usize| win[k * 64 + j];
            let (mut aj, mut bj) = (z(0) * wt[0] - y(0) * wt[1], z(0) * wt[1] + y(0) * wt[0]);
            for k in 1..8 {
                let (w0, w1, z, y) = (wt[2 * k], wt[2 * k + 1], z(k), y(k));
                bj += z * w1 + y * w0;
                if k & 1 == 1 {
                    aj += y * w1 - z * w0;
                } else {
                    aj += z * w0 - y * w1;
                }
            }
            a[j] = aj;
            b[j] = bj;
        }

        pcm[dstr + (15 - i) * nch] = scale_pcm(a[1]);
        pcm[dstr + (17 + i) * nch] = scale_pcm(b[1]);
        pcm[dstl + (15 - i) * nch] = scale_pcm(a[0]);
        pcm[dstl + (17 + i) * nch] = scale_pcm(b[0]);
        pcm[dstr + (47 - i) * nch] = scale_pcm(a[3]);
        pcm[dstr + (49 + i) * nch] = scale_pcm(b[3]);
        pcm[dstl + (47 - i) * nch] = scale_pcm(a[2]);
        pcm[dstl + (49 + i) * nch] = scale_pcm(b[2]);
    }
}

fn synth_granule(
    qmf_state: &mut [f32; 15 * 64],
    g: &mut [f32],
    nbands: usize,
    nch: usize,
    pcm: &mut [f32],
    pcm_off: usize,
    lin: &mut [f32],
) {
    for i in 0..nch {
        dct_ii(&mut g[576 * i..], nbands);
    }
    lin[..15 * 64].copy_from_slice(qmf_state);
    lin[15 * 64..15 * 64 + 64 * nbands].fill(0.0);
    let mut i = 0;
    while i < nbands {
        synth(g, i, pcm, pcm_off + 32 * nch * i, nch, lin, i * 64);
        i += 2;
    }
    if nch == 1 {
        let mut i = 0;
        while i < 15 * 64 {
            qmf_state[i] = lin[nbands * 64 + i];
            i += 2;
        }
    } else {
        qmf_state.copy_from_slice(&lin[nbands * 64..nbands * 64 + 15 * 64]);
    }
}

// ---------------------------------------------------------------------------------------------
// finding frames

fn hdr_is_null_or_tag(h: &[u8], bytes_left: i32) -> bool {
    (h[0] == 0 && h[1] == 0 && h[2] == 0 && h[3] == 0) || (bytes_left == 128 && &h[..3] == b"TAG")
}

fn match_frame(d: &[u8], mp3_bytes: i32, frame_bytes: i32) -> bool {
    let mut i = 0i32;
    let mut nmatch = 0;
    while nmatch < MAX_FRAME_SYNC_MATCHES {
        let h = &d[i as usize..];
        i += hdr_frame_bytes(h, frame_bytes) + hdr_padding(h);
        if i + HDR_SIZE > mp3_bytes {
            return nmatch > 0;
        }
        let h2 = &d[i as usize..];
        if hdr_is_null_or_tag(h2, mp3_bytes - i) {
            return nmatch > 0;
        }
        if !hdr_compare(d, h2) {
            return false;
        }
        nmatch += 1;
    }
    true
}

/// (offset of the frame, its length with padding, or 0 if none)
fn find_frame(d: &[u8], free_format_bytes: &mut i32) -> (i32, i32) {
    let mp3_bytes = d.len() as i32;
    let mut i = 0i32;
    while i < mp3_bytes - HDR_SIZE {
        let m = &d[i as usize..];
        if hdr_valid(m) {
            let mut frame_bytes = hdr_frame_bytes(m, *free_format_bytes);
            let mut frame_and_padding = frame_bytes + hdr_padding(m);
            let mut k = HDR_SIZE;
            while frame_bytes == 0
                && k < MAX_FREE_FORMAT_FRAME_SIZE
                && i + 2 * k < mp3_bytes - HDR_SIZE
            {
                if hdr_compare(m, &m[k as usize..]) {
                    let fb = k - hdr_padding(m);
                    let nextfb = fb + hdr_padding(&m[k as usize..]);
                    if i + k + nextfb + HDR_SIZE > mp3_bytes
                        || !hdr_compare(m, &m[(k + nextfb) as usize..])
                    {
                        k += 1;
                        continue;
                    }
                    frame_and_padding = k;
                    frame_bytes = fb;
                    *free_format_bytes = fb;
                }
                k += 1;
            }
            if (frame_bytes != 0
                && i + frame_and_padding <= mp3_bytes
                && match_frame(m, mp3_bytes - i, frame_bytes))
                || (i == 0 && frame_and_padding == mp3_bytes)
            {
                return (i, frame_and_padding);
            }
            *free_format_bytes = 0;
        }
        i += 1;
    }
    (mp3_bytes, 0)
}

// ---------------------------------------------------------------------------------------------
// the frame decoder (mp3dec_t + mp3dec_decode_frame)

#[derive(Default, Clone, Copy)]
pub(crate) struct FrameInfo {
    pub frame_bytes: i32,
    pub channels: i32,
    pub hz: i32,
    pub layer: i32,
}

struct Dec {
    mdct_overlap: [[f32; 9 * 32]; 2],
    qmf_state: [f32; 15 * 64],
    reserv: i32,
    free_format_bytes: i32,
    header: [u8; 4],
    reserv_buf: [u8; MAX_BITRESERVOIR_BYTES],
    // scratch (mp3dec_scratch_t)
    maindata: Vec<u8>,
    gr_info: [GrInfo; 4],
    /// grbuf[2][576] and then scf[40], in one block as in minimp3: with mixed blocks at 8 kHz
    /// the reordering of a channel runs 24 values past its 576 (into the next channel, or into
    /// the scale factors), and minimp3's output depends on that
    grbuf: Vec<f32>,
    syn: Vec<f32>,
    ist_pos: [[u8; 39]; 2],
}

impl Dec {
    fn new() -> Box<Dec> {
        Box::new(Dec {
            mdct_overlap: [[0.0; 9 * 32]; 2],
            qmf_state: [0.0; 15 * 64],
            reserv: 0,
            free_format_bytes: 0,
            header: [0; 4],
            reserv_buf: [0; MAX_BITRESERVOIR_BYTES],
            // + 8: the Huffman decoder reads a few bytes ahead. On broken files it can also use
            // a few bits past the end of the main data; those bytes are left over from the
            // frame before, as in minimp3 (where they are on the stack), not cleared, so the
            // output stays the same as minimp3's there too.
            maindata: vec![0; MAX_BITRESERVOIR_BYTES + MAX_L3_FRAME_PAYLOAD_BYTES + 8],
            gr_info: [GrInfo::default(); 4],
            grbuf: vec![0.0; GRBUF + 40],
            syn: vec![0.0; (18 + 15) * 2 * 32],
            ist_pos: [[0; 39]; 2],
        })
    }

    /// memset(dec, 0)
    fn reset(&mut self) {
        self.mdct_overlap = [[0.0; 9 * 32]; 2];
        self.qmf_state = [0.0; 15 * 64];
        self.reserv = 0;
        self.free_format_bytes = 0;
        self.header = [0; 4];
        self.reserv_buf = [0; MAX_BITRESERVOIR_BYTES];
    }

    /// Decodes the frame at the start of `mp3` (skipping garbage before it). Returns the
    /// samples per channel written to `pcm` (0 if none); `info.frame_bytes` is how far to move.
    fn decode_frame(&mut self, mp3: &[u8], pcm: &mut [f32], info: &mut FrameInfo) -> i32 {
        let mp3_bytes = mp3.len() as i32;
        let mut i = 0i32;
        let mut frame_size = 0i32;
        let mut success = 1;
        self.ist_pos = [[0; 39]; 2];

        if mp3_bytes > 4 && self.header[0] == 0xff && hdr_compare(&self.header, mp3) {
            frame_size = hdr_frame_bytes(mp3, self.free_format_bytes) + hdr_padding(mp3);
            if frame_size != mp3_bytes
                && (frame_size + HDR_SIZE > mp3_bytes
                    || !hdr_compare(mp3, &mp3[frame_size as usize..]))
            {
                frame_size = 0;
            }
        }
        if frame_size == 0 {
            self.reset();
            let (off, fs) = find_frame(mp3, &mut self.free_format_bytes);
            i = off;
            frame_size = fs;
            if frame_size == 0 || i + frame_size > mp3_bytes {
                info.frame_bytes = i;
                return 0;
            }
        }

        let hdr: [u8; 4] = mp3[i as usize..i as usize + 4].try_into().unwrap();
        self.header = hdr;
        info.frame_bytes = i + frame_size;
        info.channels = if is_mono(&hdr) { 1 } else { 2 };
        info.hz = hdr_sample_rate_hz(&hdr) as i32;
        info.layer = 4 - layer(&hdr) as i32;

        let frame = mp3
            .get((i + HDR_SIZE) as usize..(i + frame_size) as usize)
            .unwrap_or(&[]);
        let mut bs = Bs::new(frame_size - HDR_SIZE);
        if is_crc(&hdr) {
            get_bits(frame, &mut bs, 16);
        }
        let nch = info.channels as usize;

        if info.layer == 3 {
            let main_data_begin = l3_read_side_info(frame, &mut bs, &mut self.gr_info, &hdr);
            if main_data_begin < 0 || bs.pos > bs.limit {
                self.header[0] = 0;
                return 0;
            }
            // L3_restore_reservoir
            let frame_bytes = ((bs.limit - bs.pos) / 8) as usize;
            let bytes_have = self.reserv.min(main_data_begin).max(0) as usize;
            let from = (self.reserv - main_data_begin).max(0) as usize;
            self.maindata[..bytes_have].copy_from_slice(&self.reserv_buf[from..from + bytes_have]);
            let src = (bs.pos / 8) as usize;
            let n = frame_bytes
                .min(frame.len().saturating_sub(src))
                .min(self.maindata.len() - 8 - bytes_have);
            self.maindata[bytes_have..bytes_have + n].copy_from_slice(&frame[src..src + n]);
            let total = bytes_have + n;
            let mut mbs = Bs::new(total as i32);
            success = (self.reserv >= main_data_begin) as i32;

            if success != 0 {
                let granules = if test_mpeg1(&hdr) { 2 } else { 1 };
                let mut p = 0usize;
                for igr in 0..granules {
                    self.grbuf[..GRBUF].fill(0.0);
                    self.l3_decode(&mut mbs, igr * nch, nch);
                    synth_granule(
                        &mut self.qmf_state,
                        &mut self.grbuf,
                        18,
                        nch,
                        pcm,
                        p,
                        &mut self.syn,
                    );
                    p += 576 * nch;
                }
            }
            // L3_save_reservoir
            let mut pos = ((mbs.pos + 7) as u32 / 8) as i32;
            let mut remains = (mbs.limit as u32 / 8) as i32 - pos;
            if remains > MAX_BITRESERVOIR_BYTES as i32 {
                pos += remains - MAX_BITRESERVOIR_BYTES as i32;
                remains = MAX_BITRESERVOIR_BYTES as i32;
            }
            if remains > 0 {
                let (p, r) = (pos as usize, remains as usize);
                self.reserv_buf[..r].copy_from_slice(&self.maindata[p..p + r]);
            }
            self.reserv = remains;
        } else {
            let mut sci = L12ScaleInfo {
                scf: [0.0; 192],
                total_bands: 0,
                stereo_bands: 0,
                bitalloc: [0; 64],
                scfcod: [0; 64],
            };
            l12_read_scale_info(&hdr, frame, &mut bs, &mut sci);
            self.grbuf[..GRBUF].fill(0.0);
            let mut i = 0i32;
            let mut p = 0usize;
            for igr in 0..3 {
                i += l12_dequantize_granule(
                    &mut self.grbuf,
                    i as usize,
                    frame,
                    &mut bs,
                    &sci,
                    info.layer | 1,
                );
                if i == 12 {
                    i = 0;
                    let scf = sci.scf;
                    l12_apply_scf_384(&sci, &scf[igr..], &mut self.grbuf);
                    synth_granule(
                        &mut self.qmf_state,
                        &mut self.grbuf,
                        12,
                        nch,
                        pcm,
                        p,
                        &mut self.syn,
                    );
                    self.grbuf[..GRBUF].fill(0.0);
                    p += 384 * nch;
                }
                if bs.pos > bs.limit {
                    self.header[0] = 0;
                    return 0;
                }
            }
        }
        success * hdr_frame_samples(&self.header) as i32
    }

    fn l3_decode(&mut self, bs: &mut Bs, gr0: usize, nch: usize) {
        let hdr = self.header;
        for ch in 0..nch {
            let gr = self.gr_info[gr0 + ch];
            let limit = bs.pos + gr.part_23_length as i32;
            let scf: &mut [f32; 40] = (&mut self.grbuf[GRBUF..GRBUF + 40]).try_into().unwrap();
            l3_decode_scalefactors(
                &hdr,
                &mut self.ist_pos[ch],
                &self.maindata,
                bs,
                &gr,
                scf,
                ch,
            );
            let scf = *scf;
            l3_huffman(
                &mut self.grbuf[576 * ch..576 * ch + 576],
                &self.maindata,
                bs,
                &gr,
                &scf,
                limit,
            );
        }
        if test_i_stereo(&hdr) {
            let gr = [self.gr_info[gr0], self.gr_info[(gr0 + 1).min(3)]];
            l3_intensity_stereo(&mut self.grbuf, &mut self.ist_pos[1], &gr, &hdr);
        } else if is_ms_stereo(&hdr) {
            l3_midside_stereo(&mut self.grbuf, 0, 576);
        }
        for ch in 0..nch {
            let gr = self.gr_info[gr0 + ch];
            let mut aa_bands = 31;
            let n_long_bands = (if gr.mixed_block_flag != 0 { 2 } else { 0 })
                << (my_sample_rate(&hdr) == 2) as i32;
            if gr.n_short_sfb != 0 {
                aa_bands = n_long_bands - 1;
                let sfb = &gr.sfbtab[(gr.n_long_sfb as usize).min(gr.sfbtab.len())..];
                l3_reorder(
                    &mut self.grbuf,
                    576 * ch + n_long_bands as usize * 18,
                    &mut self.syn,
                    sfb,
                );
            }
            let g = &mut self.grbuf[576 * ch..576 * ch + 576];
            l3_antialias(g, aa_bands);
            l3_imdct_gr(
                g,
                &mut self.mdct_overlap[ch],
                gr.block_type,
                n_long_bands as usize,
            );
            l3_change_sign(g);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the file (minimp3_ex: mp3dec_load_buf, frame by frame)

fn skip_id3v2(b: &[u8]) -> usize {
    if b.len() >= 10
        && &b[..3] == b"ID3"
        && !((b[5] & 15) != 0
            || (b[6] & 0x80) != 0
            || (b[7] & 0x80) != 0
            || (b[8] & 0x80) != 0
            || (b[9] & 0x80) != 0)
    {
        let mut size = ((((b[6] & 0x7f) as usize) << 21)
            | (((b[7] & 0x7f) as usize) << 14)
            | (((b[8] & 0x7f) as usize) << 7)
            | (b[9] & 0x7f) as usize)
            + 10;
        if b[5] & 16 != 0 {
            size += 10; // footer
        }
        return size;
    }
    0
}

/// The size of `b` without ID3v1 (+ "TAG+") and APEv2 tags at the end.
fn skip_id3v1(b: &[u8]) -> usize {
    let mut n = b.len();
    if n >= 128 && &b[n - 128..n - 125] == b"TAG" {
        n -= 128;
        if n >= 227 && &b[n - 227..n - 223] == b"TAG+" {
            n -= 227;
        }
    }
    if n > 32 && &b[n - 32..n - 24] == b"APETAGEX" {
        n -= 32;
        let t = &b[n + 12..n + 16];
        let tag_size = u32::from_le_bytes([t[0], t[1], t[2], t[3]]) as usize;
        if n >= tag_size {
            n -= tag_size;
        }
    }
    n
}

/// The Xing / Info tag in the first frame (minimp3_ex's mp3dec_check_vbrtag).
fn check_vbrtag(frame: &[u8]) -> VbrTag {
    let fs = frame.len();
    if fs < HDR_SIZE as usize + 8 {
        return VbrTag::None;
    }
    let mut bs = Bs::new(fs as i32 - HDR_SIZE);
    let body = &frame[HDR_SIZE as usize..];
    if is_crc(frame) {
        get_bits(body, &mut bs, 16);
    }
    let mut gr = [GrInfo::default(); 4];
    if l3_read_side_info(body, &mut bs, &mut gr, frame) < 0 {
        return VbrTag::None;
    }
    let mut off = HDR_SIZE as usize + (bs.pos / 8) as usize;
    if off > fs || fs - off < 8 {
        return VbrTag::None;
    }
    let tag = &frame[off..];
    if &tag[..4] != b"Xing" && &tag[..4] != b"Info" {
        return VbrTag::None;
    }
    let flags = tag[7];
    if flags & 1 == 0 {
        return VbrTag::NoFrames;
    }
    off += 8;
    if fs - off < 4 {
        return VbrTag::None;
    }
    let t = &frame[off..];
    let frames = u32::from_be_bytes([t[0], t[1], t[2], t[3]]);
    off += 4;
    for (flag, len) in [(2u8, 4usize), (4, 100), (8, 4)] {
        if flags & flag != 0 {
            if fs - off < len {
                return VbrTag::None;
            }
            off += len;
        }
    }
    if fs - off < 1 {
        return VbrTag::None;
    }
    let (mut delay, mut padding) = (0, 0);
    if frame[off] != 0 {
        // LAME, Lavc...: encoder delay and padding
        if fs - off <= 35 {
            return VbrTag::None;
        }
        off += 21;
        let t = &frame[off..];
        delay = (((t[0] as i32) << 4) | (t[1] as i32 >> 4)) + (528 + 1);
        padding = ((((t[1] & 0xF) as i32) << 8) | t[2] as i32) - (528 + 1);
    }
    VbrTag::Frames(frames, delay, padding)
}

enum VbrTag {
    /// no tag (or a broken one): the frame is audio
    None,
    /// a tag without a frame count (-1 in minimp3): skipped, nothing trimmed
    NoFrames,
    /// frames, encoder delay, padding (delay = padding = 0 without a LAME extension)
    Frames(u32, i32, i32),
}

/// An MP3 stream decoding from memory, frame by frame, with the encoder delay and padding
/// removed when the file has a LAME tag.
pub(crate) struct Mp3 {
    data: Arc<[u8]>,
    /// the first frame to decode (after tags and the Xing / LAME frame)
    start: usize,
    /// the end of the audio (before ID3v1 / APE tags)
    end: usize,
    pos: usize,
    dec: Box<Dec>,
    pub channels: usize,
    pub sample_rate: u32,
    layer: i32,
    /// interleaved samples still to skip at the start (encoder delay)
    delay: usize,
    to_skip: usize,
    /// total interleaved samples (0 = unknown), and how many were given out
    detected: u64,
    given: u64,
    pcm: Vec<f32>,
    ended: bool,
}

impl Mp3 {
    pub fn new(data: Arc<[u8]>) -> Option<Mp3> {
        // skip ID3v2 at the start and ID3v1 / APE at the end
        let mut start = skip_id3v2(&data).min(data.len());
        let end = start + skip_id3v1(&data[start..]);
        // find the first frame: its format, and a Xing / LAME tag
        let mut ffb = 0i32;
        let (i, frame_size) = find_frame(&data[start..end], &mut ffb);
        start += i as usize;
        if frame_size == 0 {
            return None;
        }
        let hdr = &data[start..start + 4];
        let channels = if is_mono(hdr) { 1 } else { 2 };
        let sample_rate = hdr_sample_rate_hz(hdr);
        let layer = 4 - layer(hdr) as i32;
        let samples = hdr_frame_samples(hdr) as u64 * channels as u64;
        let (mut delay, mut detected) = (0usize, 0u64);
        if layer == 3 {
            let tag = check_vbrtag(&data[start..start + frame_size as usize]);
            if let VbrTag::Frames(frames, d, p) = tag {
                let padding = p as i64 * channels as i64;
                delay = (d.max(0) as usize) * channels;
                let to_skip = d as i64 * channels as i64;
                detected = samples * frames as u64;
                if detected >= to_skip as u64 {
                    detected -= to_skip as u64;
                }
                if padding > 0 && detected >= padding as u64 {
                    detected -= padding as u64;
                }
                if detected == 0 {
                    return None;
                }
            }
            if !matches!(tag, VbrTag::None) {
                start += frame_size as usize; // the tag frame holds no audio
            }
        }
        Some(Mp3 {
            data,
            start,
            end,
            pos: start,
            dec: Dec::new(),
            channels,
            sample_rate,
            layer,
            delay,
            to_skip: delay,
            detected,
            given: 0,
            pcm: vec![0.0; MAX_SAMPLES_PER_FRAME],
            ended: false,
        })
    }

    /// Decodes the next frame with audio. Returns its interleaved samples, or None at the end.
    pub fn next_frame(&mut self) -> Option<&[f32]> {
        loop {
            if self.ended || self.pos >= self.end {
                return None;
            }
            let mut info = FrameInfo::default();
            let n = self
                .dec
                .decode_frame(&self.data[self.pos..self.end], &mut self.pcm, &mut info);
            self.pos += info.frame_bytes.max(0) as usize;
            if n > 0 {
                if info.hz as u32 != self.sample_rate
                    || info.layer != self.layer
                    || info.channels as usize != self.channels
                {
                    // the format changed in the middle: stop, like minimp3_ex
                    self.ended = true;
                    return None;
                }
                let mut len = n as usize * self.channels;
                let mut start = 0;
                if self.to_skip > 0 {
                    let skip = len.min(self.to_skip);
                    self.to_skip -= skip;
                    len -= skip;
                    start = skip;
                }
                if self.detected > 0 && self.given + len as u64 > self.detected {
                    len = self.detected.saturating_sub(self.given) as usize;
                }
                self.given += len as u64;
                if len > 0 {
                    return Some(&self.pcm[start..start + len]);
                }
                if self.detected > 0 && self.given >= self.detected {
                    self.ended = true;
                    return None;
                }
            } else if info.frame_bytes == 0 {
                return None;
            }
        }
    }

    /// Goes back to the start.
    pub fn rewind(&mut self) {
        self.pos = self.start;
        self.dec.header[0] = 0;
        self.to_skip = self.delay;
        self.given = 0;
        self.ended = false;
    }
}
