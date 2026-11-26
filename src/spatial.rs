//! Simple 3D sound: distance attenuation, left/right panning from the listener's point of view,
//! and Doppler shift. Positions are `[x, y, z]` in any unit you like (metres suit the default
//! speed of sound). The listener starts at the origin, facing -Z with +Y up (so +X is right),
//! like OpenAL and SoLoud.

/// How a sound gets quieter with distance. `min` and `max` are the sound's
/// [`Spatial::distance`]; closer than `min` it plays at full volume, and beyond `max` it gets
/// no quieter.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Attenuation {
    /// No change with distance.
    None,
    /// `min / (min + rolloff * (d - min))`: halves at twice `min` (rolloff 1). Natural; the
    /// default.
    Inverse,
    /// Falls in a straight line to `1 - rolloff` at `max` (rolloff 1: silent at `max`).
    Linear,
    /// `(d / min) ^ -rolloff`.
    Exponential,
}

/// Where a 3D sound is and how it behaves. Give it to
/// [`PlayOptions::spatial`](crate::PlayOptions::spatial), then move it with
/// [`Audio::set_position`](crate::Audio::set_position).
///
/// A 3D sound's pan comes from its position, so [`PlayOptions::pan`](crate::PlayOptions::pan)
/// is not used. Mono sources work best.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spatial {
    pub position: [f32; 3],
    /// units per second; only used for the Doppler shift
    pub velocity: [f32; 3],
    /// full volume up to this distance (default 1)
    pub min_distance: f32,
    /// no quieter beyond this distance (default 1000)
    pub max_distance: f32,
    pub attenuation: Attenuation,
    /// how quickly the sound fades with distance (default 1)
    pub rolloff: f32,
    /// strength of the Doppler shift, 0 (off) .. 1 (real world, the default)
    pub doppler: f32,
}

impl Spatial {
    /// A sound at `position`, with the defaults: inverse-distance attenuation from 1 to 1000,
    /// rolloff 1, real-world Doppler, not moving.
    pub fn at(position: [f32; 3]) -> Spatial {
        Spatial {
            position,
            velocity: [0.0; 3],
            min_distance: 1.0,
            max_distance: 1000.0,
            attenuation: Attenuation::Inverse,
            rolloff: 1.0,
            doppler: 1.0,
        }
    }

    pub fn velocity(mut self, v: [f32; 3]) -> Spatial {
        self.velocity = v;
        self
    }

    /// Full volume up to `min`, no quieter beyond `max`.
    pub fn distance(mut self, min: f32, max: f32) -> Spatial {
        self.min_distance = min;
        self.max_distance = max;
        self
    }

    pub fn attenuation(mut self, model: Attenuation, rolloff: f32) -> Spatial {
        self.attenuation = model;
        self.rolloff = rolloff;
        self
    }

    pub fn doppler(mut self, factor: f32) -> Spatial {
        self.doppler = factor;
        self
    }

    /// The volume factor at distance `d`.
    pub fn gain(&self, d: f32) -> f32 {
        let min = self.min_distance.max(1e-6);
        let max = self.max_distance.max(min);
        let d = d.clamp(min, max);
        let r = self.rolloff.max(0.0);
        let g = match self.attenuation {
            Attenuation::None => 1.0,
            Attenuation::Inverse => min / (min + r * (d - min)),
            Attenuation::Linear => {
                if max > min {
                    1.0 - r * (d - min) / (max - min)
                } else {
                    1.0
                }
            }
            Attenuation::Exponential => (d / min).powf(-r),
        };
        g.clamp(0.0, 1.0)
    }
}

/// The listener: the player's ears, usually the camera.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Listener {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub velocity: [f32; 3],
    pub speed_of_sound: f32,
}

impl Default for Listener {
    fn default() -> Self {
        Listener {
            position: [0.0; 3],
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
            velocity: [0.0; 3],
            speed_of_sound: 343.0,
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// What the mixer applies to a 3D voice this block.
pub(crate) struct Applied {
    pub gain: f32,
    /// left and right gains from the direction (equal power, 1 / 1 straight ahead)
    pub pan: [f32; 2],
    /// pitch factor from the Doppler shift
    pub doppler: f32,
}

pub(crate) fn apply(s: &Spatial, l: &Listener) -> Applied {
    let to = sub(s.position, l.position);
    let d = len(to);
    let gain = s.gain(d);

    let right = cross(l.forward, l.up);
    let rl = len(right);
    let mut p = if d > 1e-6 && rl > 1e-6 {
        dot(to, right) / (d * rl)
    } else {
        0.0
    };
    // inside the minimum distance the sound gradually moves to the centre (no jump when it
    // passes through the listener)
    if s.min_distance > 0.0 {
        p *= (d / s.min_distance).min(1.0);
    }
    let p = p.clamp(-1.0, 1.0);
    let pan = [(1.0 - p).sqrt(), (1.0 + p).sqrt()];

    let mut doppler = 1.0;
    if s.doppler > 0.0 && d > 1e-6 {
        // OpenAL's formula, along the line from the source to the listener
        let c = l.speed_of_sound.max(1.0);
        let lim = c / s.doppler * 0.99;
        let sl = [-to[0] / d, -to[1] / d, -to[2] / d];
        let vls = dot(sl, l.velocity).min(lim);
        let vss = dot(sl, s.velocity).min(lim);
        doppler = ((c - s.doppler * vls) / (c - s.doppler * vss)).clamp(0.1, 10.0);
    }
    Applied { gain, pan, doppler }
}
