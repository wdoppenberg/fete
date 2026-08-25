//! **Sprawl** — a megacity under smog, seen from altitude.
//!
//! Where [Neon City](../fete_visual_neon/index.html) walks a grid of boxes and
//! is therefore limited in how much city can be in frame, this one never
//! marches at all: a ray meets the ground plane analytically and the city is a
//! texture function of where it landed. Cost per pixel is constant and
//! independent of how much is visible, so every pixel out to the horizon can
//! carry its own lights.
//!
//! That trades away real geometry — no facades, nothing occludes anything —
//! which is affordable because a city seen from altitude through heavy
//! atmosphere is not read as geometry. It is read as a luminous carpet with a
//! few monolithic silhouettes standing out of it. So the silhouettes are a
//! handful of parallaxed billboard walls, the carpet is analytic, and the haze
//! does the rest.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | how lit the sprawl is |
//! | E/D | 2 | drift speed |
//! | R/F | 3 | altitude |
//! | T/G | 4 | how far the camera looks down |
//! | Y/H | 5 | haze — how far the city stays visible |
//! | U/J | 6 | atmospheric dispersion |
//! | I/K | 7 | beat depth (half-time) |

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use fete_core::prelude::*;

/// Must match `SprawlParams` in `sprawl.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct SprawlParams {
    pub drift: f32,
    pub energy: f32,
    pub sway: f32,
    pub height: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
#[bind_group_data(Tier)]
pub struct Sprawl {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: SprawlParams,
    /// Not a binding — the pipeline specialisation key.
    tier: Tier,
}

impl From<&Sprawl> for Tier {
    fn from(sprawl: &Sprawl) -> Self {
        sprawl.tier
    }
}

impl Material2d for Sprawl {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_sprawl/shaders/sprawl.wgsl".into()
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
        // The march is the expensive half: every one of these steps runs a
        // full FBM before it can reject a cell, so the step count multiplies
        // the octave count below rather than adding to it.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "BLOCK_STEPS".into(),
            tier.pick(64, 40, 24),
        ));
        // Blocks sit on a 10-unit grid, so 24 steps still crosses 240 units —
        // far enough that the fog has closed long before the march gives up.
        // That is why this visual, unlike Neon, needs no matching fog change.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "BUILT_UP_OCTAVES".into(),
            tier.pick(3, 2, 2),
        ));
        Ok(())
    }
}

impl Visual for Sprawl {
    const ID: VisualId = "sprawl";
    const NAME: &'static str = "Sprawl";
    const TAGS: &'static [&'static str] = &["city", "analytic", "slow", "ambient"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn set_quality(&mut self, quality: Quality) {
        self.tier = quality.tier;
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // World units per second, where a unit is about ten metres. Even the
        // top of this range is only eighty metres a second, and from a
        // kilometre up that reads as a slow drift.
        let speed = frame.knob_range(2, 0.5, 8.0);
        self.params.drift += speed * dt;

        let wander =
            |period: f32, phase: f32| ((beats / period + phase) * std::f32::consts::TAU).sin();

        // Wide. Lateral motion is the only thing that parallaxes the horizon
        // silhouettes — forward motion correctly does nothing to something two
        // kilometres away — so it has to be big enough to see.
        self.params.sway = wander(53.0, 0.0) * 40.0;

        // Five hundred to eighteen hundred metres, and worth understanding why
        // the whole range sits this high. Looking at a plane, the image is
        // scale-invariant in altitude: doubling the height doubles every ground
        // distance, so the same picture comes back sampled at twice the world
        // scale. Altitude is therefore not "how far away the city is" — it is
        // how much city fits in the frame and how fine the grain of it is, and
        // the references are all grain. Low is a few blocks and a lot of roof.
        //
        // The floor still sits above the tallest block, so the camera never
        // ends up inside one and fills the frame with a single wall.
        self.params.height = frame.knob_range(3, 52.0, 180.0) + wander(71.0, 0.4) * 5.0;

        // Shallow. Most of the frame should be horizon and haze; tipping much
        // further down turns it into a map.
        let look_down = frame.knob_range(4, 0.06, 0.34);
        self.params.pitch = -look_down + wander(101.0, 0.7) * 0.012;

        // A slow pan, which is what sweeps the skyline past.
        self.params.yaw = wander(89.0, 0.2) * 0.30;

        let target =
            (frame.clock.pulse_div(2.0, 2.0) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        let alpha = 1.0 - (-dt / 0.4).exp();
        self.params.energy += (target - self.params.energy) * alpha;
    }
}

pub struct SprawlPlugin;

impl Plugin for SprawlPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/sprawl.wgsl");
        app.add_visual::<Sprawl>();
    }
}
