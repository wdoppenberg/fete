//! What the heavy boids leave behind them.
//!
//! Fifty-six remembered positions per boid, drawn as the same soft disc as the
//! boid itself, tapering in both size and opacity. The taper length is not
//! fixed: it is driven by that boid's oscillator, so a disc at the top of its
//! pulse drags a long tail and one at the bottom is nearly bare. That coupling
//! is most of what makes the field look alive rather than merely animated.

use crate::config::*;
use crate::flock::Boid;
use crate::render::{MeshBuf, emit_firefly};

/// A ring buffer of past positions, newest at `head`.
#[derive(Debug)]
pub struct Trails {
    points: Vec<[f32; 2]>,
    head: usize,
    /// How many frames have been recorded, so a fresh visual does not draw a
    /// tail of stale positions on its first frames.
    filled: usize,
}

impl Default for Trails {
    fn default() -> Self {
        Self::new()
    }
}

impl Trails {
    pub fn new() -> Self {
        Self {
            points: vec![[0.0; 2]; N_HEAVY * TRAIL_LEN],
            head: 0,
            filled: 0,
        }
    }

    /// Forget everything and restart from the flock's current positions.
    pub fn reset(&mut self, heavy: &[Boid]) {
        for (i, boid) in heavy.iter().enumerate() {
            for k in 0..TRAIL_LEN {
                self.points[i * TRAIL_LEN + k] = [boid.x, boid.y];
            }
        }
        self.head = 0;
        self.filled = 1;
    }

    /// Record one frame.
    pub fn push(&mut self, heavy: &[Boid]) {
        self.head = (self.head + 1) % TRAIL_LEN;
        self.filled = (self.filled + 1).min(TRAIL_LEN);
        for (i, boid) in heavy.iter().enumerate() {
            self.points[i * TRAIL_LEN + self.head] = [boid.x, boid.y];
        }
    }

    /// Draw every tail. `length_scale` is the knob, scaling the taper the
    /// oscillator sets.
    ///
    /// Colour comes from the boid, not from history — the original recoloured
    /// the whole trail every frame as the palette morphed, so a tail is always
    /// the same hue as the disc in front of it.
    pub fn emit(
        &self,
        out: &mut MeshBuf,
        colors: &[[f32; 3]],
        pulses: &[f32],
        time: f32,
        length_scale: f32,
    ) {
        for (i, color) in colors.iter().enumerate().take(N_HEAVY) {
            let pulse = pulses.get(i).copied().unwrap_or(1.0);

            // Map the oscillator onto how much of the buffer is visible.
            let normalised = ((pulse - (1.0 - KURA_AMP)) / (2.0 * KURA_AMP)).clamp(0.0, 1.0);
            let fraction =
                TRAIL_LEN_MIN_FRAC + (TRAIL_LEN_MAX_FRAC - TRAIL_LEN_MIN_FRAC) * normalised;
            let denominator = (fraction * length_scale * TRAIL_LEN as f32).max(1.0);

            let visible = self.filled.min(TRAIL_LEN);
            for age in 0..visible {
                let alpha = if age == 0 {
                    1.0
                } else {
                    let t = (age as f32 / denominator).clamp(0.0, 1.0);
                    (1.0 - t).powf(TRAIL_FADE_GAMMA)
                };
                // Past the end of the taper there is nothing left to draw, and
                // nothing further back can be brighter.
                if alpha < 0.006 {
                    break;
                }

                let slot = (self.head + TRAIL_LEN - age) % TRAIL_LEN;
                let point = self.points[i * TRAIL_LEN + slot];
                emit_firefly(
                    out, point[0], point[1], 0.0, alpha, pulse, *color, TRAIL_PT, time,
                );
            }
        }
    }
}
