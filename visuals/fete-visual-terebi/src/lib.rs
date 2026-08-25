//! **Terebi** — テレビ. A wall of 90s CRT sets in a dark room, each one playing
//! its own fragment of late-night Japanese television: a studio, an anime
//! impact frame, a vertical shooter, a platformer, a pseudo-3d racer, a test
//! card, and the ones showing nothing but snow.
//!
//! The wall is carved rather than tiled. A cell is cut in half along its longer
//! side up to twice, each cut decided by a hash, and every rectangle that falls
//! out is one set — unequal sizes, no neighbour lookups, nothing stored. That
//! matters more here than anywhere else in the show: a grid of identical lit
//! rectangles is the one thing a wall of televisions must never look like.
//!
//! Nine channels, each a pure function of a picture coordinate in `-1..1`. That
//! is what makes the sync work: when the wall gangs up, every set is handed the
//! position of its own glass on the wall instead of its own local coordinate,
//! and the same nine functions paint one enormous picture split across every
//! screen — each piece still bulging through its own tube. It costs one `mix`.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | how many sets are switched on |
//! | E/D | 2 | how often the sets change channel |
//! | R/F | 3 | sync — how often the whole wall shows one picture |
//! | T/G | 4 | interference — snow, tracking, lost vertical hold |
//! | Y/H | 5 | set size — a few large sets or a bank of portables |
//! | U/J | 6 | colour spread |
//! | I/K | 7 | beat depth (half-time) |

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use fete_core::prelude::*;

/// Must match `TerebiParams` in `terebi.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct TerebiParams {
    /// Lateral drift of the wall, in screen units. Bounded — see [`Terebi::animate`].
    pub sway: Vec2,
    /// The night's schedule, in programme units. Wraps at [`PROGRAMME_WRAP`].
    pub programme: f32,
    /// Smoothed half-time beat energy.
    pub energy: f32,
    /// Smoothed interference amount, so tape damage comes and goes rather than
    /// switching on.
    pub interference: f32,
    /// How far into a sync window the wall is, `0.0..1.0`.
    pub sync: f32,
    /// What this sync window does: `0.0` puts every set on the same broadcast
    /// at its own scale, `1.0` spreads one picture across the whole wall.
    pub wall_mode: f32,
    pub _pad0: f32,
}

/// Where the programme clock wraps.
///
/// The shader takes `floor(programme / dwell)` as a set's channel index, so an
/// f32 carrying hours of programme time has too little left for the fraction
/// that schedules the next cut. Wrapping costs one mass channel change every
/// twenty minutes or so, which on a wall of televisions is indistinguishable
/// from any other cut.
pub const PROGRAMME_WRAP: f32 = 512.0;

/// How often a sync window may open, in beats.
const SYNC_PERIOD: f32 = 32.0;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct Terebi {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: TerebiParams,
}

impl Material2d for Terebi {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_terebi/shaders/terebi.wgsl".into()
    }
}

impl Visual for Terebi {
    const ID: VisualId = "terebi";
    const NAME: &'static str = "Terebi";
    const TAGS: &'static [&'static str] = &["tokyo", "crt", "collage", "busy"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // Programme time. Integrated rather than `time * rate`, which are only
        // equal while the rate is constant: computed from the clock, turning the
        // cut rate up rewrites what every set has *already* shown and the whole
        // wall jumps to a different point in the schedule.
        self.params.programme += frame.knob_range(2, 0.02, 0.55) * dt;
        if self.params.programme >= PROGRAMME_WRAP {
            self.params.programme -= PROGRAMME_WRAP;
        }

        // Bounded, not integrated — same reason as Kanban. An unbounded drift
        // walks the cell coordinates into the thousands over a night and the
        // hashes that lay the wall out run out of fraction and quantise.
        let wander =
            |period: f32, phase: f32| ((beats / period + phase) * std::f32::consts::TAU).sin();
        self.params.sway = Vec2::new(wander(61.0, 0.0) * 0.05, wander(83.0, 0.37) * 0.03);

        // Glides over half a second. Snapped, every set in the frame tears at
        // once, which reads as a bug rather than as tape.
        self.params.interference = smooth(self.params.interference, frame.knob(4), dt, 0.5);

        // Sync windows. One may open every phrase; whether it does is a hash of
        // which phrase it is, so the knob sets how often the wall gangs up
        // rather than scheduling it. Held for four to twenty-four beats, drawn
        // from its own hash rather than from the one that opened the window —
        // sharing them ties how long a sync lasts to how rare it is, and a low
        // knob ends up only ever producing the shortest ones. Always closed well
        // before the next window, so the envelope is never interrupted.
        let window = (beats / SYNC_PERIOD).floor();
        let roll = hash11(window * 7.3 + frame.globals.seed * 31.0);
        let target = if roll < frame.knob(3) * 0.55 {
            let held = beats - window * SYNC_PERIOD;
            let hold = 4.0 + hash11(window * 13.9 + 3.3) * 20.0;
            (smoothstep(0.0, 1.0, held) * (1.0 - smoothstep(hold, hold + 2.0, held)))
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.params.sync = smooth(self.params.sync, target, dt, 0.10);
        // Decided with the window, so it cannot change halfway through one.
        // Held while the window closes, or the last frames of a wall picture
        // would snap back to twenty separate ones.
        if target > 0.0 {
            self.params.wall_mode = step(0.45, hash11(window * 3.1 + frame.globals.seed * 17.0));
        }

        // Half-time and heavily smoothed: a swell, not a hit.
        let energy =
            (frame.clock.pulse_div(2.0, 2.2) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        self.params.energy = smooth(self.params.energy, energy, dt, 0.3);
    }
}

/// Frame-rate independent exponential smoothing. `tau` is roughly the time to
/// cover most of the remaining distance.
fn smooth(current: f32, target: f32, dt: f32, tau: f32) -> f32 {
    let alpha = 1.0 - (-dt / tau.max(1e-4)).exp();
    current + (target - current) * alpha
}

fn step(edge: f32, x: f32) -> f32 {
    if x < edge { 0.0 } else { 1.0 }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The `hash11` from `fete::noise`, so the sync windows the CPU schedules are
/// drawn from the same sequence the shader would have produced.
fn hash11(p: f32) -> f32 {
    let mut x = (p * 0.1031).fract();
    x *= x + 33.33;
    x *= x + x;
    x.fract()
}

/// Registers Terebi with the show.
pub struct TerebiPlugin;

impl Plugin for TerebiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terebi.wgsl");
        app.add_visual::<Terebi>();
    }
}
