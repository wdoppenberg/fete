//! Small numeric helpers, and a deterministic generator to seed the flock with.
//!
//! The generator is hand-rolled rather than pulled from `rand` for one reason:
//! the simulation has to be reproducible from a single `u64`, so that
//! [`FeteGlobals::seed`](fete_core::prelude::FeteGlobals) alone decides what
//! this appearance of the visual looks like.

use crate::config::WORLD_SIZE;

/// xorshift64\*. Fast, tiny, and good enough for scattering boids.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    /// Box–Muller produces normals in pairs; this is the one being held back.
    spare: Option<f32>,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Any non-zero state will do, but a raw small seed gives a poor first
        // few outputs, so it is mixed first.
        let state = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407)
            | 1;
        Self { state, spare: None }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0.0..1.0`.
    pub fn unit(&mut self) -> f32 {
        // Top 24 bits: exactly the mantissa an f32 can hold, so the result is
        // uniform over representable values rather than subtly biased.
        ((self.next_u64() >> 40) as f32) / ((1u32 << 24) as f32)
    }

    /// Uniform in `min..max`.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
    }

    /// Standard normal, by Box–Muller.
    pub fn normal(&mut self) -> f32 {
        if let Some(spare) = self.spare.take() {
            return spare;
        }
        // `unit()` can return exactly zero and `ln(0)` is not a number, so the
        // draw is nudged off the floor rather than rejected in a loop.
        let u1 = self.unit().max(f32::MIN_POSITIVE);
        let u2 = self.unit();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f32::consts::TAU * u2;
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }

    /// A coin flip.
    pub fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }
}

/// Shortest signed offset between two coordinates on the wrapped world.
pub fn wrap_delta(v: f32) -> f32 {
    if v > WORLD_SIZE {
        v - 2.0 * WORLD_SIZE
    } else if v < -WORLD_SIZE {
        v + 2.0 * WORLD_SIZE
    } else {
        v
    }
}

/// Bring a position back inside the world.
pub fn wrap_pos(v: f32) -> f32 {
    wrap_delta(v)
}

/// `smoothstep`, matching the GLSL definition.
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Fractional part, always positive. Hue arithmetic lives on this.
pub fn wrap1(x: f32) -> f32 {
    x - x.floor()
}

/// Normalise in place, falling back to straight down for a zero vector — the
/// same fallback the original used, and the reason empty flow cells all point
/// the same way.
pub fn normalize2(x: &mut f32, y: &mut f32) {
    let magnitude = x.hypot(*y);
    if magnitude > 1e-6 {
        *x /= magnitude;
        *y /= magnitude;
    } else {
        *x = 0.0;
        *y = -1.0;
    }
}

/// Hold a velocity between a minimum and a maximum speed.
pub fn clamp_speed(vx: &mut f32, vy: &mut f32, min: f32, max: f32) {
    let speed = vx.hypot(*vy);
    if speed < min && speed > 0.0 {
        *vx *= min / speed;
        *vy *= min / speed;
    } else if speed > max {
        *vx *= max / speed;
        *vy *= max / speed;
    }
}

/// HSV to linear RGB. The original worked in HSV because its palette is built
/// from hue relationships; this is the same conversion.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let c = v * s;
    let m = v - c;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());

    let (r, g, b) = match (h * 6.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    [r + m, g + m, b + m]
}

/// Hue of an RGB colour, in turns.
pub fn rgb_hue(r: f32, g: f32, b: f32) -> f32 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if (max - min).abs() < 1e-6 {
        return 0.0;
    }
    let delta = max - min;
    let h = if max == r {
        (g - b) / delta
    } else if max == g {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    wrap1(h / 6.0)
}
