//! **Kanban** — 看板, "signboard". Japanese neon signage floating past in the
//! dark.
//!
//! A field of signs — vertical columns of characters, framed boards, single
//! large glyphs, a few hanging off rails — drifting outward as the view flies
//! slowly forward through them. No font and no texture: characters are
//! composed the way kanji are composed, out of radicals placed on a square
//! field, so the set never repeats and nothing ever spells anything.
//!
//! Built dark on purpose. It is a lot of small bright points, and a field of
//! points reads far brighter in a small room than its peak value suggests, so
//! its brightness range tops out lower than the rest of the set.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | how many cells carry a sign |
//! | E/D | 2 | flight speed |
//! | R/F | 3 | warp — the moving glass everything is seen through |
//! | T/G | 4 | melt — how much the characters squirm and tilt |
//! | Y/H | 5 | scale — a few large signs against a deep field of small ones |
//! | U/J | 6 | colour spread |
//! | I/K | 7 | beat depth (half-time) |

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use fete_core::prelude::*;

/// Must match `KanbanParams` in `kanban.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct KanbanParams {
    /// Lateral drift, in screen units. Bounded — see [`Kanban::animate`].
    pub sway: Vec2,
    /// Distance flown, in octaves. Wraps at [`ZOOM_WRAP`].
    pub zoom: f32,
    /// Smoothed half-time beat energy.
    pub energy: f32,
    /// Smoothed melt amount, so squirming in and out is a morph not a cut.
    pub melt: f32,
    pub _pad0: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

/// Where the zoom counter wraps. Must match `ZOOM_WRAP` in the shader.
///
/// The shader splits this counter into an integer part that seeds the layers
/// and a fractional part that positions them. Left to grow all night the
/// integer part eats the mantissa and the fraction visibly steps, so it wraps —
/// and because the shader takes the seed lookup modulo the same number, the
/// wrap is indistinguishable from an ordinary octave hand-off.
pub const ZOOM_WRAP: f32 = 64.0;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
#[bind_group_data(Tier)]
pub struct Kanban {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: KanbanParams,
    /// Not a binding — the pipeline specialisation key.
    tier: Tier,
}

impl From<&Kanban> for Tier {
    fn from(visual: &Kanban) -> Self {
        visual.tier
    }
}

impl Material2d for Kanban {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_kanban/shaders/kanban.wgsl".into()
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
        // Four parallax layers of signage, each an independent grid lookup
        // with its own character SDF. The nearest layers carry the silhouette;
        // the furthest is a haze of small marks that the bloom smears anyway.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "LAYERS".into(),
            tier.pick(4, 3, 2),
        ));
        // The air FBM runs on every pixel whether or not there is a sign on
        // it. It is a slow brightness drift across the frame, and two octaves
        // hold that shape.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "AIR_OCTAVES".into(),
            tier.pick(3, 2, 2),
        ));
        Ok(())
    }
}

impl Visual for Kanban {
    const ID: VisualId = "kanban";
    const NAME: &'static str = "Kanban";
    const TAGS: &'static [&'static str] = &["tokyo", "signage", "trippy", "slow"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn set_quality(&mut self, quality: Quality) {
        self.tier = quality.tier;
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // Octaves per second — the whole field doubles in size this often, so
        // even the top of the range is a slow forward drift rather than
        // flight. Integrated rather than computed as `time * speed`, which are
        // only equal while the speed is constant: the moment the knob moves,
        // `time * speed` rewrites where the flight has been and the field
        // jumps to a different depth.
        self.params.zoom += frame.knob_range(2, 0.0, 0.10) * dt;
        if self.params.zoom >= ZOOM_WRAP {
            self.params.zoom -= ZOOM_WRAP;
        }

        // Sway is bounded rather than integrated, which is the one place this
        // differs from every other visual in the set. An unbounded lateral
        // drift walks the cell coordinates away from the origin all night, and
        // once they are in the thousands the hashes that place the signs run
        // out of fraction and the field quantises. Two slow periods read as
        // drift and never leave the neighbourhood.
        let wander =
            |period: f32, phase: f32| ((beats / period + phase) * std::f32::consts::TAU).sin();
        self.params.sway = Vec2::new(wander(53.0, 0.0) * 0.09, wander(71.0, 0.3) * 0.05);

        // Melt glides over about half a second. Snapping it makes every
        // character in the frame flinch at once.
        self.params.melt = smooth(self.params.melt, frame.knob(4), dt, 0.4);

        // Half-time and heavily smoothed: a swell, not a hit.
        let target =
            (frame.clock.pulse_div(2.0, 2.2) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        self.params.energy = smooth(self.params.energy, target, dt, 0.3);
    }
}

/// Frame-rate independent exponential smoothing. `tau` is roughly the time to
/// cover most of the remaining distance.
fn smooth(current: f32, target: f32, dt: f32, tau: f32) -> f32 {
    let alpha = 1.0 - (-dt / tau.max(1e-4)).exp();
    current + (target - current) * alpha
}

/// Registers Kanban with the show.
pub struct KanbanPlugin;

impl Plugin for KanbanPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/kanban.wgsl");
        app.add_visual::<Kanban>();
    }
}
