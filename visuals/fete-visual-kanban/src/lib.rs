//! **Kanban** — 看板, "signboard". Japanese neon signage floating past in the
//! dark.
//!
//! A field of signs — vertical columns of characters, framed boards, single
//! large glyphs, a few hanging off rails — drifting outward as the view flies
//! slowly forward through them. Every one of them says something: the words
//! are in [`lexicon`], the characters come from a real Japanese face baked into
//! the distance field in `glyphs.png`, and the whole vocabulary is the sort of
//! thing that is lit up on a street in Tokyo tonight.
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

pub mod lexicon;

use bevy::asset::embedded_asset;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
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

/// The vocabulary, as the shader reads it. Must match `KanbanLexicon` in
/// `kanban.wgsl`.
///
/// Constant for the life of the show — it is here rather than baked into the
/// shader because the words and the atlas they index have to come from one
/// place, and that place is [`lexicon`], which the atlas tool reads too.
#[derive(ShaderType, Debug, Clone)]
pub struct KanbanLexicon {
    /// Atlas columns, atlas rows, draw slots in use, and the atlas index of
    /// the long vowel mark.
    pub grid: Vec4,
    /// One row per draw slot: up to four glyph indices, `-1.0` for the tail.
    pub slots: [Vec4; lexicon::MAX_SLOTS],
}

impl Default for KanbanLexicon {
    fn default() -> Self {
        let slots = lexicon::draw_slots();
        let (cols, rows) = (lexicon::ATLAS_COLS as f32, lexicon::atlas_rows() as f32);
        let mut packed = [Vec4::splat(-1.0); lexicon::MAX_SLOTS];
        for (row, word) in packed.iter_mut().zip(&slots) {
            *row = Vec4::from_array(*word);
        }
        Self {
            grid: Vec4::new(
                cols,
                rows,
                slots.len() as f32,
                lexicon::glyph_index(lexicon::CHOONPU).map_or(-1.0, |i| i as f32),
            ),
            slots: packed,
        }
    }
}

/// The baked glyph atlas, loaded once and handed to every Kanban material.
#[derive(Resource, Debug)]
struct GlyphAtlas(Handle<Image>);

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
#[bind_group_data(Tier)]
pub struct Kanban {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: KanbanParams,
    #[uniform(2)]
    words: KanbanLexicon,
    /// The distance field every character is drawn from, pointed at the loaded
    /// atlas by [`attach_atlas`] on the same frame the surface is spawned.
    /// Until the image itself has finished loading the material cannot be
    /// prepared and nothing is drawn, which is the right way round: the
    /// fallback texture Bevy would bind for `None` is flat white, and flat
    /// white reads as deep inside a stroke everywhere.
    #[texture(3)]
    #[sampler(4)]
    atlas: Option<Handle<Image>>,
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

/// Loads the atlas once, at startup rather than on the first sign, so that the
/// first frame Kanban is on screen is a frame with characters in it.
///
/// The settings are not decoration. The image is distance, not colour: read it
/// as sRGB and every value in it is bent through a transfer curve, which moves
/// every edge in every character. And it is read with linear filtering, which
/// is what a distance field is for — the value halfway between two samples
/// really is the distance halfway between them, so one 128-pixel cell holds up
/// magnified across a third of the screen.
fn load_atlas(mut commands: Commands, assets: Res<AssetServer>) {
    let handle = assets
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                // Clamped, not repeated: a sample that runs off the atlas has
                // to stop at the edge rather than wrap to the far side of it.
                address_mode_u: ImageAddressMode::ClampToEdge,
                address_mode_v: ImageAddressMode::ClampToEdge,
                mag_filter: ImageFilterMode::Linear,
                min_filter: ImageFilterMode::Linear,
                ..default()
            });
        })
        .load("embedded://fete_visual_kanban/glyphs.png");
    commands.insert_resource(GlyphAtlas(handle));
}

/// Points the active material at the atlas.
///
/// The material is built by the framework from [`Default`], which cannot reach
/// a resource, so the handle is written in the frame the surface is spawned —
/// before anything is rendered, since this runs in `Update` and the spawn
/// happens in the state transition ahead of it.
fn attach_atlas(
    atlas: Res<GlyphAtlas>,
    mut materials: ResMut<Assets<Kanban>>,
    surfaces: Query<&MeshMaterial2d<Kanban>, With<VisualSurface>>,
) {
    for handle in &surfaces {
        let Some(material) = materials.get(&handle.0) else {
            continue;
        };
        if material.atlas.as_ref() == Some(&atlas.0) {
            continue;
        }
        // Taken mutably only when there is something to write: `get_mut` flags
        // the material as changed, and a change every frame is a uniform
        // re-upload every frame for nothing.
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.atlas = Some(atlas.0.clone());
        }
    }
}

/// Registers Kanban with the show.
pub struct KanbanPlugin;

impl Plugin for KanbanPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/kanban.wgsl");
        embedded_asset!(app, "glyphs.png");
        app.add_systems(Startup, load_atlas)
            .add_visual::<Kanban>()
            .add_visual_systems::<Kanban, _>(
                Update,
                // After `Animate`, which is where the material is written:
                // both touch it, and taking it mutably twice in one frame
                // costs an extra change-detection flag for nothing.
                attach_atlas.after(VisualSystems::Animate),
            );
    }
}
