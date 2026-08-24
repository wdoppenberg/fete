//! One phase oscillator per heavy boid, weakly coupled to the ones it is
//! touching.
//!
//! This is what makes the discs breathe. Each oscillator runs at its own
//! natural frequency and only couples to neighbours that are *very* close —
//! and only when there are one or two of them, never a crowd. That restraint
//! is the whole point: full Kuramoto coupling synchronises, and a screen of
//! two hundred discs all pulsing together reads as a strobe. Coupling this
//! weak produces momentary agreements between passing pairs instead, which is
//! the effect worth having.
//!
//! Time here is measured in beats scaled to the reference tempo, so the
//! breathing belongs to the track rather than to the wall clock.

use crate::config::*;
use crate::flock::{Boid, Grid};
use crate::math::{Rng, wrap_delta};

#[derive(Debug, Default)]
pub struct Kuramoto {
    theta: Vec<f32>,
    /// Natural frequency, radians per second at the reference tempo.
    omega: Vec<f32>,
    /// Per-oscillator amplitude, so they do not all swing equally far.
    amplitude: Vec<f32>,
    pulse: Vec<f32>,
}

impl Kuramoto {
    pub fn new(count: usize, rng: &mut Rng) -> Self {
        let mut system = Self {
            theta: vec![0.0; count],
            omega: vec![0.0; count],
            amplitude: vec![1.0; count],
            pulse: vec![1.0; count],
        };
        for i in 0..count {
            system.theta[i] = rng.range(0.0, std::f32::consts::TAU);
            let freq = rng.range(KURA_FREQ_MIN, KURA_FREQ_MAX);
            system.omega[i] = std::f32::consts::TAU * freq;
            system.amplitude[i] = 1.0 + rng.range(-AMP_JITTER, AMP_JITTER);
        }
        system
    }

    /// Size multiplier for oscillator `i`, around `1.0`.
    pub fn pulse(&self, i: usize) -> f32 {
        self.pulse.get(i).copied().unwrap_or(1.0)
    }

    pub fn pulses(&self) -> &[f32] {
        &self.pulse
    }

    /// Advance every oscillator. `dt` is in reference-tempo seconds.
    pub fn update(&mut self, flock: &[Boid], grid: &Grid, dt: f32, rng: &mut Rng) {
        if flock.len() != self.theta.len() {
            return;
        }
        let radius2 = KURA_COUPLING_RADIUS * KURA_COUPLING_RADIUS;

        for i in 0..self.theta.len() {
            let boid = flock[i];
            let (cgx, cgy) = Grid::cell_of(boid.x, boid.y);

            let mut sin_sum = 0.0f32;
            let mut cos_sum = 0.0f32;
            let mut weight_sum = 0.0f32;
            let mut neighbours = 0usize;

            for gx in (cgx - 1)..=(cgx + 1) {
                for gy in (cgy - 1)..=(cgy + 1) {
                    for &j in grid.at(gx, gy) {
                        let j = j as usize;
                        if j == i {
                            continue;
                        }
                        let dx = wrap_delta(flock[j].x - boid.x);
                        let dy = wrap_delta(flock[j].y - boid.y);
                        let d2 = dx * dx + dy * dy;
                        if d2 >= radius2 {
                            continue;
                        }
                        let dist = d2.sqrt();
                        // Half the nominal radius, and an exponential weight on
                        // top: contact, not proximity.
                        if dist < KURA_COUPLING_RADIUS * 0.5 {
                            let w = (-5.0 * dist / KURA_COUPLING_RADIUS).exp();
                            sin_sum += w * self.theta[j].sin();
                            cos_sum += w * self.theta[j].cos();
                            weight_sum += w;
                            neighbours += 1;
                        }
                    }
                }
            }

            // A crowd does not couple at all. Only a pair or a trio does.
            let coupling = if neighbours > 0 && neighbours <= 2 {
                KURA_K_BASE
            } else {
                0.0
            };

            let term = self.theta[i].cos() * sin_sum - self.theta[i].sin() * cos_sum;
            let mut dtheta = self.omega[i];
            if weight_sum > 0.0 {
                dtheta += coupling * (term / weight_sum);
            }

            // Phase noise, scaled as a Wiener increment so the diffusion rate
            // does not depend on the frame rate.
            let noise = KURA_NOISE_STD * rng.normal() * dt.max(0.0).sqrt();
            self.theta[i] = (self.theta[i] + dtheta * dt + noise).rem_euclid(std::f32::consts::TAU);

            // A spatial term folded into the phase. Two boids sitting on the
            // same phase but a few units apart still read differently, which
            // breaks up the banding a pure phase field would produce.
            let spatial = boid.x * 0.1 + boid.y * 0.1;
            self.pulse[i] = 1.0 + (KURA_AMP * self.amplitude[i]) * (self.theta[i] + spatial).sin();
        }
    }
}
