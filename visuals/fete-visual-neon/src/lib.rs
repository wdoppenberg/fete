//! **Neon** — a city seen from above, drifting past.
//!
//! The city is a function rather than a model: a hash over an infinite integer
//! grid decides which cells are road and how tall each block is, and rays walk
//! that grid a cell at a time. It never repeats, it has no extent to run out
//! of, and it costs nothing to store.
//!
//! The camera hovers. At street level the same city reads as a handful of large
//! flat-faced boxes; from altitude a building is a few pixels and what you see
//! is a lit street grid with traffic on it, fading into haze.
//!
//! Built for a room where the screen is atmosphere rather than the act, so the
//! camera moves slowly, the frame is mostly dark, and it reacts at half-time.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | how many windows are lit |
//! | E/D | 2 | drift speed |
//! | R/F | 3 | altitude |
//! | T/G | 4 | how far the camera looks down |
//! | Y/H | 5 | haze — how far the city is visible |
//! | U/J | 6 | colour spread |
//! | I/K | 7 | beat depth (half-time) |

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use fete_core::prelude::*;

/// Must match `NeonParams` in `neon.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct NeonParams {
    /// Distance travelled down the avenue.
    pub drift: f32,
    /// Smoothed half-time beat energy.
    pub energy: f32,
    /// Lateral position across the avenue.
    pub sway: f32,
    /// Camera height above the road.
    pub height: f32,
    /// Look direction, as small offsets from straight ahead.
    pub yaw: f32,
    pub pitch: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct Neon {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: NeonParams,
}

impl Material2d for Neon {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_neon/shaders/neon.wgsl".into()
    }
}

impl Visual for Neon {
    const ID: VisualId = "neon";
    const NAME: &'static str = "Neon City";
    const TAGS: &'static [&'static str] = &["tokyo", "raymarched", "slow", "ambient"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // One cell is one city block, so this is blocks per second. Slow — from
        // altitude the ground moves visually slower than it does at street
        // level, so this can be a little faster than a flythrough without ever
        // reading as speed.
        let speed = frame.knob_range(2, 0.3, 3.0);
        self.params.drift += speed * dt;

        // The camera is never quite still, on periods long enough that no
        // single one is perceptible. Without this the flight is a rail and
        // reads as a screensaver; with it, it reads as handheld.
        let wander =
            |period: f32, phase: f32| ((beats / period + phase) * std::f32::consts::TAU).sin();

        // Nothing is above the buildings to collide with, so the camera is free
        // to wander much further than it could down a street.
        self.params.sway = wander(43.0, 0.0) * 6.0;

        // Altitude. The floor is set well above the tallest tower (8.5) so the
        // camera never clips through one, which from above would fill the frame
        // with a single roof.
        self.params.height = frame.knob_range(3, 12.0, 30.0) + wander(67.0, 0.4) * 1.6;

        // How far down we look. Shallow shows more horizon and haze and reads
        // as a wide establishing shot; steep shows the street grid as a plan.
        let look_down = frame.knob_range(4, 0.22, 0.85);
        self.params.pitch = -look_down + wander(97.0, 0.7) * 0.03;

        // Yaw trails the sway, the way you look towards where you are drifting.
        self.params.yaw = wander(43.0, 0.15) * 0.09;

        // Half-time and heavily smoothed: a swell, not a hit.
        let target =
            (frame.clock.pulse_div(2.0, 2.0) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        let alpha = 1.0 - (-dt / 0.35).exp();
        self.params.energy += (target - self.params.energy) * alpha;
    }
}

pub struct NeonPlugin;

impl Plugin for NeonPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/neon.wgsl");
        app.add_visual::<Neon>();
    }
}
