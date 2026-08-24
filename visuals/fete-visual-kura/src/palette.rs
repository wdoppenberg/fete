//! Colour: three role hues, and a per-boid shade of each.
//!
//! The original generated its own palettes — a driver hue, one of six harmonic
//! schemes to place two more against it, and a fixed saturation and value per
//! role. That last part is what actually makes the picture read: the heavy
//! discs are almost fully saturated and almost fully bright, the light shoal is
//! two thirds as saturated and dimmer, and the small dust is pale. Three
//! populations at three brightnesses is the depth in the image.
//!
//! So the roles are kept exactly and only the hues are re-sourced: they come
//! from the show's own cosine palette, sampled at three points around the
//! gradient. Kura recolours with the rest of the set when the palette morphs,
//! and still looks like itself.

use fete_core::prelude::Palette;

use crate::config::*;
use crate::math::{Rng, hsv_to_rgb, rgb_hue, wrap1};

/// Saturation and value for each population.
const HEAVY_SV: [f32; 2] = [0.98, 0.99];
const LIGHT_SV: [f32; 2] = [0.62, 0.84];
const SMALL_SV: [f32; 2] = [0.35, 0.78];

/// Minimum hue separation between roles, in turns. Without it a narrow palette
/// collapses all three flocks onto one colour and the depth goes with it.
const MIN_SEPARATION_LIGHT: f32 = 0.10;
const MIN_SEPARATION_SMALL: f32 = 0.18;

#[derive(Debug, Default)]
pub struct Colors {
    pub heavy: Vec<[f32; 3]>,
    pub light: Vec<[f32; 3]>,
    pub small: Vec<[f32; 3]>,

    /// Fixed per-boid hue offsets, drawn once per activation. A boid keeps its
    /// shade for the whole run, which is why a cluster looks like a cluster of
    /// individuals rather than a gradient.
    jitter_heavy: Vec<f32>,
    jitter_light: Vec<f32>,
    jitter_small: Vec<f32>,
}

impl Colors {
    pub fn new(rng: &mut Rng) -> Self {
        let jitter = |count: usize, rng: &mut Rng| {
            (0..count)
                .map(|_| rng.range(-ANALOG_SPREAD, ANALOG_SPREAD))
                .collect::<Vec<_>>()
        };
        Self {
            heavy: vec![[1.0; 3]; N_HEAVY],
            light: vec![[1.0; 3]; N_LIGHT],
            small: vec![[1.0; 3]; N_SMALL],
            jitter_heavy: jitter(N_HEAVY, rng),
            jitter_light: jitter(N_LIGHT, rng),
            jitter_small: jitter(N_SMALL, rng),
        }
    }

    /// Recompute every boid's colour from the show palette.
    ///
    /// Cheap enough to run every frame, which is what makes a palette morph
    /// reach the flocks continuously instead of in steps.
    pub fn apply(&mut self, palette: &Palette, seed: f32, spread: f32) {
        let hue_at = |t: f32| {
            let c = palette.sample(t);
            rgb_hue(c.x.max(0.0), c.y.max(0.0), c.z.max(0.0))
        };

        let heavy_hue = hue_at(seed);
        let mut light_hue = hue_at(seed + 1.0 / 3.0);
        let mut small_hue = hue_at(seed + 2.0 / 3.0);

        light_hue = nudge_away(light_hue, heavy_hue, MIN_SEPARATION_LIGHT);
        small_hue = nudge_away(small_hue, heavy_hue, MIN_SEPARATION_SMALL);
        small_hue = nudge_away(small_hue, light_hue, MIN_SEPARATION_LIGHT * 0.8);

        fill(
            &mut self.heavy,
            &self.jitter_heavy,
            heavy_hue,
            HEAVY_SV,
            spread,
        );
        fill(
            &mut self.light,
            &self.jitter_light,
            light_hue,
            LIGHT_SV,
            spread,
        );
        fill(
            &mut self.small,
            &self.jitter_small,
            small_hue,
            SMALL_SV,
            spread,
        );
    }
}

fn fill(out: &mut [[f32; 3]], jitter: &[f32], hue: f32, sv: [f32; 2], spread: f32) {
    for (color, offset) in out.iter_mut().zip(jitter) {
        *color = hsv_to_rgb(wrap1(hue + offset * spread), sv[0], sv[1]);
    }
}

/// Push `hue` away from `from` until they are at least `minimum` apart.
fn nudge_away(hue: f32, from: f32, minimum: f32) -> f32 {
    let mut delta = hue - from;
    delta -= (delta + 0.5).floor();
    if delta.abs() < minimum {
        let sign = if delta >= 0.0 { 1.0 } else { -1.0 };
        return wrap1(from + sign * minimum);
    }
    hue
}
