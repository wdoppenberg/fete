//! **Yama** — 山, "mountain". A volcanic cone at dusk, circled slowly, with
//! cloud drifting past it.
//!
//! The odd one out in this set. Everything else here is a night city seen from
//! above; this is a landscape with a horizon in it, written against the flat,
//! layered, enormous distances of *Breath of the Wild* — where the far ranges
//! are pale silhouettes stacked into the haze, the near ones are almost black,
//! and the whole picture is carried by one lit edge and a band of burning sky.
//!
//! The cone is a surface of revolution rather than an SDF, so a ray is bounded
//! analytically to the span where it is inside the base radius and only that
//! span is stepped — a mountain filling the frame costs a march of five world
//! units. Its profile is Fuji's: height falling as `(1 - r/R)^1.3`, steep at
//! the summit and flattening into the plain. A straight cone reads as a tent.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | cloud cover |
//! | E/D | 2 | how fast the camera circles |
//! | R/F | 3 | altitude — the shore, or the ridge above it |
//! | T/G | 4 | the hour — how far the sun has set |
//! | Y/H | 5 | haze |
//! | U/J | 6 | snow line |
//! | I/K | 7 | beat depth (half-time) |

use std::f32::consts::TAU;

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use fete_core::prelude::*;

/// Must match `YamaParams` in `yama.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct YamaParams {
    /// Orbit angle in radians. Integrated, and wrapped at a turn.
    pub orbit: f32,
    /// Smoothed half-time beat energy.
    pub energy: f32,
    /// Camera altitude, in mountain heights. The summit is at 1.0.
    pub height: f32,
    /// Orbit radius.
    pub radius: f32,
    /// Pan away from the axis, so the cone is not centred.
    pub look_yaw: f32,
    /// Height of the point the camera is aimed at.
    pub look_lift: f32,
    /// Sun azimuth. World-fixed, drifting only very slowly.
    pub sun_az: f32,
    /// Sun elevation in radians. Negative once it has set.
    pub sun_elev: f32,
    /// Integrated cloud drift, in noise-space units.
    pub wind: f32,
    /// Altitude of the banner cloud wrapping the cone.
    pub banner: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
#[bind_group_data(Tier)]
pub struct Yama {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: YamaParams,
    /// Not a binding — the pipeline specialisation key.
    tier: Tier,
}

impl From<&Yama> for Tier {
    fn from(visual: &Yama) -> Self {
        visual.tier
    }
}

impl Material2d for Yama {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_yama/shaders/yama.wgsl".into()
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let tier = key.bind_group_data;
        let Some(fragment) = descriptor.fragment.as_mut() else {
            return Ok(());
        };
        fragment.shader_defs.extend(tier.shader_defs());
        // Every step of both marches calls `mountain_height`, which is an
        // atan2, six sines and a pow — transcendentals, which a mobile GPU's
        // special-function unit runs at a fraction of its ALU rate. This is
        // the most expensive shader in the set and the steps are why.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "MARCH_STEPS".into(),
            tier.pick(48, 32, 20),
        ));
        // Bisection is what stops a coarse march showing terraces on the
        // ridge, so it is cut more gently than the march itself: each step
        // halves the remaining error, and three still resolve the surface to
        // an eighth of a march step.
        fragment
            .shader_defs
            .push(ShaderDefVal::Int("BISECT_STEPS".into(), tier.pick(6, 5, 3)));
        // The reflection march is unconditional at high quality, so a water
        // pixel reflecting the cone pays for the mountain twice. Below high it
        // keeps the sky and the airlight — which is most of what the water
        // shows anyway — and drops the reflected peak.
        if tier == Tier::Low {
            fragment.shader_defs.push("NO_MOUNTAIN_REFLECTION".into());
        }
        Ok(())
    }
}

impl Visual for Yama {
    const ID: VisualId = "yama";
    const NAME: &'static str = "Yama";
    const TAGS: &'static [&'static str] = &["landscape", "dusk", "slow", "ambient"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn set_quality(&mut self, quality: Quality) {
        self.tier = quality.tier;
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // Sine on a beat period. Every wander below is built from these on
        // mutually prime periods, which is what keeps a five-minute circuit
        // from ever coming back to the same picture.
        let wander = |period: f32, phase: f32| ((beats / period + phase) * TAU).sin();

        // --- the circle ------------------------------------------------------
        // Radians per second. Even the fast end is under two minutes for a full
        // circuit and the slow end is a quarter of an hour, because the point
        // of the orbit is the light changing, not the movement.
        //
        // Integrated rather than `time * rate`: the two are only equal while the
        // rate is constant, and the moment the knob moves `time * rate` rewrites
        // where the camera has already been and the mountain jumps.
        let rate = frame.knob_range(2, 0.006, 0.055);
        // "Not too exact": the rate itself breathes, so the orbit eases without
        // ever stopping.
        self.params.orbit += rate * (1.0 + 0.35 * wander(137.0, 0.0)) * dt;
        if self.params.orbit >= TAU {
            self.params.orbit -= TAU;
        }

        // --- where the camera is --------------------------------------------
        // Altitude, in mountain heights. The whole range is low, and it has to
        // be: with the camera near the summit the peak sits on the horizon line
        // and the cone flattens into a smear. Fuji is photographed from
        // sea level because that is the only place it looks tall.
        self.params.height = frame.knob_range(3, 0.14, 0.62) + wander(71.0, 0.4) * 0.018;

        // Radius follows altitude rather than being its own knob: climbing and
        // pulling back together keeps the mountain a constant size in frame and
        // turns the knob into a single "how aerial is this" control. The wander
        // on top is a slow breath in and out that nothing else is driving.
        self.params.radius = 5.8 + self.params.height * 3.0 + wander(83.0, 0.2) * 0.55;

        // --- where it looks --------------------------------------------------
        // Off-axis, always. A cone centred in the frame is a diagram; pushed to
        // one side and drifting slowly across, the same shot reads as a camera
        // being carried past. Two periods so it never settles into a sweep.
        //
        // The amplitude is bounded by the shape rather than chosen: this cone
        // spans about forty-eight degrees against a sixty-six degree frame, so
        // there is only around ten degrees of room either side. Past that a
        // flank runs off the edge, and a silhouette this recognisable does not
        // read as a crop — it reads as a mistake.
        self.params.look_yaw = wander(311.0, 0.12) * 0.115 + wander(191.0, 0.4) * 0.045;

        // Aim below the summit so the peak sits high in the frame, and lift the
        // aim as the camera climbs so it stays there.
        self.params.look_lift = 0.30 + self.params.height * 0.35 + wander(101.0, 0.7) * 0.05;

        // --- the sun ---------------------------------------------------------
        // Fixed in the world while the camera moves around it. This is the one
        // decision that makes the orbit worth watching: over a circuit the
        // light goes from flat and frontal, through raking across the flank, to
        // straight into the sun with the cone black against it — three
        // completely different pictures out of one slow move.
        self.params.sun_az = 2.4 + wander(1201.0, 0.0) * 0.6;

        // The hour. Positive is the sun still just up and the snow burning;
        // negative is under the horizon, the flanks gone, the sky doing all the
        // work. The shader climbs the last light up the mountain as this falls.
        self.params.sun_elev = frame.knob_range(4, 0.055, -0.10) + wander(163.0, 0.15) * 0.005;

        // --- the air ---------------------------------------------------------
        // Cloud drift, integrated, and slow — a deck at five mountain-heights
        // moves visibly only near the horizon. Left unwrapped on purpose: the
        // noise field is not periodic, so wrapping the offset would step the
        // whole sky, and the magnitude after a four-hour night is still small
        // enough that the hashes have fraction to spare.
        self.params.wind += 0.045 * (1.0 + 0.4 * wander(233.0, 0.1)) * dt;

        // The banner cloud rides up and down the waist of the cone.
        self.params.banner = 0.38 + wander(97.0, 0.25) * 0.10;

        // --- beat ------------------------------------------------------------
        // Half-time and heavily smoothed. Nothing in a landscape should move on
        // a kick; this is the air brightening slightly and falling back.
        let target =
            (frame.clock.pulse_div(2.0, 2.2) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        let alpha = 1.0 - (-dt / 0.45).exp();
        self.params.energy += (target - self.params.energy) * alpha;
    }
}

/// Registers Yama with the show.
pub struct YamaPlugin;

impl Plugin for YamaPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/yama.wgsl");
        app.add_visual::<Yama>();
    }
}
