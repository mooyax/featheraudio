//! The master limiter: keeps the mix under full scale when many sounds play at once, instead
//! of cutting off the tops of the waves (which sounds like crackling).
//!
//! It looks ahead a little (1.5 ms): the gain starts going down smoothly before a peak arrives,
//! so the output never goes over the threshold, and comes back up over 0.15 s. While the mix
//! stays under the threshold, the sound passes through unchanged (only 1.5 ms later).
//!
//! How: for every frame, the gain the frame needs (threshold / its peak) is followed down at
//! once and back up slowly (release); the lowest of those over the look-ahead window, averaged
//! over the window again, is the gain used for the frame leaving the delay line. Each frame's
//! gain is then at most what it needs, and changes smoothly.

/// output stays at or under this (-0.2 dB)
const THRESHOLD: f32 = 0.977;
const LOOKAHEAD_SECONDS: f32 = 0.0015;
const RELEASE_SECONDS: f32 = 0.15;
/// look-ahead frames at up to 192 kHz, + 1
const MAX_WINDOW: usize = 320;

pub(crate) struct Limiter {
    /// frames in the look-ahead window (the output is this many frames late)
    window: usize,
    release: f32,
    /// the release-smoothed gain
    env: f32,
    /// delay line of stereo frames
    delay: Box<[[f32; 2]; MAX_WINDOW]>,
    /// sliding minimum of env over the window: a ring of (frame number, value), increasing
    min_q: Box<[(u64, f32); MAX_WINDOW]>,
    min_head: usize,
    min_len: usize,
    /// moving average of those minimums over the window
    avg: Box<[f32; MAX_WINDOW]>,
    avg_sum: f64,
    n: u64,
}

impl Limiter {
    pub fn new(rate: u32) -> Limiter {
        let window = ((LOOKAHEAD_SECONDS * rate as f32).round() as usize).clamp(1, MAX_WINDOW - 1);
        Limiter {
            window,
            release: 1.0 - (-1.0 / (RELEASE_SECONDS * rate as f32)).exp(),
            env: 1.0,
            delay: Box::new([[0.0; 2]; MAX_WINDOW]),
            min_q: Box::new([(0, 1.0); MAX_WINDOW]),
            min_head: 0,
            min_len: 0,
            avg: Box::new([1.0; MAX_WINDOW]),
            avg_sum: window as f64,
            n: 0,
        }
    }

    /// Back to silence and full gain (when switched on or off).
    pub fn reset(&mut self) {
        self.env = 1.0;
        self.delay.fill([0.0; 2]);
        self.min_head = 0;
        self.min_len = 0;
        self.avg.fill(1.0);
        self.avg_sum = self.window as f64;
        self.n = 0;
    }

    /// Limits interleaved stereo frames in place.
    pub fn process(&mut self, out: &mut [f32]) {
        let w = self.window;
        for o in out.chunks_exact_mut(2) {
            let slot = (self.n % w as u64) as usize;
            // the gain this frame needs, followed down at once and up slowly
            let peak = o[0].abs().max(o[1].abs());
            let need = if peak > THRESHOLD {
                THRESHOLD / peak
            } else {
                1.0
            };
            self.env = if need < self.env {
                need
            } else {
                self.env + (need - self.env) * self.release
            };
            // sliding minimum over the last w + 1 frames
            while self.min_len > 0 {
                let back = (self.min_head + self.min_len - 1) % MAX_WINDOW;
                if self.min_q[back].1 >= self.env {
                    self.min_len -= 1;
                } else {
                    break;
                }
            }
            self.min_q[(self.min_head + self.min_len) % MAX_WINDOW] = (self.n, self.env);
            self.min_len += 1;
            if self.min_q[self.min_head].0 + (w as u64) < self.n {
                self.min_head = (self.min_head + 1) % MAX_WINDOW;
                self.min_len -= 1;
            }
            let min = self.min_q[self.min_head].1;
            // averaged over the window
            self.avg_sum += min as f64 - self.avg[slot] as f64;
            self.avg[slot] = min;
            let g = (self.avg_sum / w as f64) as f32;
            // the frame leaving the delay line (w frames ago) takes this gain: every minimum
            // averaged into it covered that frame
            let [l, r] = self.delay[slot];
            self.delay[slot] = [o[0], o[1]];
            o[0] = l * g;
            o[1] = r * g;
            self.n += 1;
        }
    }
}
