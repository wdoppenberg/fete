//! **Momiji** — 紅葉, "autumn leaves". A lantern-lit temple courtyard at
//! night, cherry blossoms framing the top corners, leaves whirling slowly
//! down through all of it. The only visual here that is not a shader-built
//! scene at all.
//!
//! The background is a single illustrated frame of the temple alone — not
//! marched, not modelled — sampled once per pixel with a cover fit so it
//! fills any window or projector shape without letterbox bars. The trees are
//! not in that art; they are drawn in the frame's own top corners out of the
//! same leaf shape the falling ones use, a static population rather than a
//! second material. Canopy and falling leaf alike sample the show's foliage
//! gradient at the exact same instant with no per-leaf offset, which is what
//! keeps the whole population one colour at a time rather than a scatter of
//! them.
//!
//! The leaf field is a screen-space overlay, one grid cell owning one leaf
//! exactly the way Kanban owns one glyph per cell, its motion computed
//! straight from `globals.time` with nothing kept between frames.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | leaf density |
//! | E/D | 2 | unused — the frame is fixed |
//! | R/F | 3 | wind — sway and fall speed |
//! | T/G | 4 | canopy reach — how far the corner trees extend |
//! | Y/H | 5 | foliage colour drift speed |
//! | U/J | 6 | unused |
//! | I/K | 7 | beat depth (gusts on the pulse) |

use bevy::asset::embedded_asset;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::prelude::*;
use fete_core::prelude::*;

/// Must match `MomijiParams` in `momiji.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct MomijiParams {
    /// Integrated wind phase — drives leaf sway and the orbit amplitude.
    pub wind: f32,
    /// Smoothed half-time beat energy, read as a gust.
    pub gust: f32,
    /// Integrated foliage colour phase. Unbounded; wrapped in the shader.
    pub hue_phase: f32,
    pub density: f32,
    /// Radius, in `centered()` screen units, that each corner's static
    /// canopy extends from its corner before fading out.
    pub canopy_reach: f32,
}

/// The background illustration, loaded once and handed to every material.
#[derive(Resource, Debug)]
struct Background(Handle<Image>);

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
#[bind_group_data(Tier)]
pub struct Momiji {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: MomijiParams,
    /// Attached by [`attach_background`] the frame the surface spawns, same
    /// as Kanban's atlas: the material is built from `Default` and cannot
    /// reach a resource, so nothing is bound until this runs.
    #[texture(2)]
    #[sampler(3)]
    background: Option<Handle<Image>>,
    /// Not a binding — the pipeline specialisation key.
    tier: Tier,
}

impl From<&Momiji> for Tier {
    fn from(visual: &Momiji) -> Self {
        visual.tier
    }
}

impl Material2d for Momiji {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_momiji/shaders/momiji.wgsl".into()
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
        // Leaf layers give the fall its parallax. Losing the two thin, distant
        // ones on the cheap tiers costs depth, not the leaves themselves — the
        // near layer, doing most of the reading, is kept at every tier.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "LEAF_LAYERS".into(),
            tier.pick(3, 2, 1),
        ));
        Ok(())
    }
}

impl Visual for Momiji {
    const ID: VisualId = "momiji";
    const NAME: &'static str = "Momiji";
    const TAGS: &'static [&'static str] = &["architecture", "garden", "night", "slow"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn set_quality(&mut self, quality: Quality) {
        self.tier = quality.tier;
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;

        // --- wind ------------------------------------------------------------
        // Integrated, not `time * rate`, for the reason Yama's cloud drift is:
        // a knob move should not step every leaf already in flight.
        let wind_rate = frame.knob_range(3, 0.4, 1.6);
        self.params.wind += wind_rate * dt;

        // Half-time and smoothed — a gust should read as the wind picking up,
        // not as every leaf flinching on the kick.
        let target =
            (frame.clock.pulse_div(2.0, 2.0) * 0.6 + frame.audio.bass * 0.4).clamp(0.0, 1.0);
        let alpha = 1.0 - (-dt / 0.6).exp();
        self.params.gust += (target - self.params.gust) * alpha;

        // --- foliage colour --------------------------------------------------
        // Driven by beats elapsed, not seconds — the same `dt` converted
        // through the live tempo — so the drift is still perfectly smooth
        // (nothing about this steps on the beat) but a faster track turns
        // the season on its own, automatically, without anyone reaching for
        // the knob. `hue_phase` is unbounded; the shader wraps it.
        let dbeats = dt * frame.clock.bpm / 60.0;
        let hue_rate = frame.knob_range(5, 1.0 / 64.0, 1.0 / 8.0);
        self.params.hue_phase += hue_rate * dbeats;

        self.params.canopy_reach = frame.knob_range(4, 0.45, 0.85);

        // Cut hard from where this started: the whirl's orbit radius is wide
        // enough now that neighbouring leaves' visual footprints overlap
        // well before their cell-count density would suggest a crowd, so
        // "dense" arrives at a much lower number than it used to.
        self.params.density = frame.knob_range(1, 0.06, 0.28);
    }
}

/// Loads the background illustration, embedded straight into the binary the
/// way Kanban's glyph atlas is — this is art, not a runtime asset a venue
/// would ever swap out.
fn load_background(mut commands: Commands, assets: Res<AssetServer>) {
    let handle = assets
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::ClampToEdge,
                address_mode_v: ImageAddressMode::ClampToEdge,
                mag_filter: ImageFilterMode::Linear,
                min_filter: ImageFilterMode::Linear,
                ..default()
            });
        })
        .load("embedded://fete_visual_momiji/background.png");
    commands.insert_resource(Background(handle));
}

/// Points the active material at the loaded background.
fn attach_background(
    background: Res<Background>,
    mut materials: ResMut<Assets<Momiji>>,
    surfaces: Query<&MeshMaterial2d<Momiji>, With<VisualSurface>>,
) {
    for handle in &surfaces {
        let Some(material) = materials.get(&handle.0) else {
            continue;
        };
        if material.background.as_ref() == Some(&background.0) {
            continue;
        }
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.background = Some(background.0.clone());
        }
    }
}

/// Registers Momiji with the show.
pub struct MomijiPlugin;

impl Plugin for MomijiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/momiji.wgsl");
        embedded_asset!(app, "background.png");
        app.add_systems(Startup, load_background)
            .add_visual::<Momiji>()
            .add_visual_systems::<Momiji, _>(
                Update,
                attach_background.after(VisualSystems::Animate),
            );
    }
}
