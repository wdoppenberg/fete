//! **Kura** — three flocks, the geometry that forms between them, and a bank
//! of weakly coupled oscillators making the whole field breathe.
//!
//! This is a reconstruction of VJ-FÊTE, the C++/OpenGL piece this whole
//! framework grew out of: 1340 boids in three interacting populations, drawn
//! as soft-cored fireflies with tapering trails, a lattice of links and filled
//! triangles between the ones that agree on a heading, a flow field smeared
//! into the background, and ripples where a field lands. Every constant is the
//! original's, down to the ones that look like accidents — see [`flock`] for
//! the two that matter.
//!
//! # How it is put together
//!
//! Everything else in the set is a pure function of position and time, or a
//! compute simulation feeding a texture. Kura is neither: it is a CPU
//! simulation feeding *geometry*. The framework asks a visual for a fullscreen
//! material and Kura provides one — [`Kura`], which paints black, the same
//! thing the original cleared to — and then draws four meshes in front of it,
//! rebuilt every frame, in the original's exact pass order:
//!
//! ```text
//! z = -0.94  flow field, then the light flock's triangles
//! z = -0.93  heavy discs
//! z = -0.92  links, filled triangles, field ripples
//! z = -0.91  small discs, then the trails over everything
//! ```
//!
//! Two materials cover all four: [`KuraSprite`] for anything that was a point
//! sprite, [`KuraFlat`] for anything that was a triangle or a thick line.
//!
//! Geometry is built in the original's 1920×1080 reference pixels and the mesh
//! transform carries the stretch to the window, which reproduces the fixed
//! render target the original presented through — including the fact that it
//! stretched a square world to a non-square screen.
//!
//! # What is phrased against the clock
//!
//! The flocking runs in real seconds, because it is physics and it should look
//! the same at any tempo. Everything periodic — the size pulse, the brightness
//! flicker, the oscillators, the flow field's wiggle — runs on beats scaled to
//! 128bpm, so it is identical to the original at the reference tempo and
//! belongs to the track at any other. Field ripples land on a beat interval,
//! and the whole picture swells at half-time.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | cohesion — loose scatter through to tight groups |
//! | E/D | 2 | speed |
//! | R/F | 3 | separation — how close they will get |
//! | T/G | 4 | trail length |
//! | Y/H | 5 | lattice — how readily links and triangles form |
//! | U/J | 6 | colour spread within each flock |
//! | I/K | 7 | beat depth, and how often a ripple lands |

pub mod config;
pub mod emergent;
pub mod flock;
pub mod flow;
pub mod kuramoto;
pub mod math;
pub mod palette;
pub mod render;
pub mod trails;

use bevy::asset::embedded_asset;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use fete_core::prelude::*;

use crate::config::*;
use crate::emergent::Emergent;
use crate::flock::Flocks;
use crate::flow::FlowField;
use crate::kuramoto::Kuramoto;
use crate::math::Rng;
use crate::palette::Colors;
use crate::render::MeshBuf;
use crate::trails::Trails;

/// Seconds in a beat at the tempo the original was tuned at. Everything
/// periodic is expressed in these, so 128bpm reproduces it exactly.
const REFERENCE_BEAT: f32 = 60.0 / 128.0;

/// Longest step the simulation will take. A frame spent compiling a shader
/// would otherwise advance the flock far enough to tear it apart.
const MAX_STEP: f32 = 0.05;

/// The ground the flocks are drawn on. See `kura.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct Kura {
    #[uniform(0)]
    globals: FeteGlobals,
}

impl Material2d for Kura {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_kura/shaders/kura.wgsl".into()
    }
}

impl Visual for Kura {
    const ID: VisualId = "kura";
    const NAME: &'static str = "Kura";
    const TAGS: &'static [&'static str] = &["flocking", "agents", "geometry", "origin"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }
}

/// Soft-cored discs: the heavy flock, the small flock, and every trail point.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct KuraSprite {
    #[uniform(0)]
    globals: FeteGlobals,
}

impl Material2d for KuraSprite {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_kura/shaders/kura_sprite.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Flat colour: flow lines, light-flock triangles, links, ripples.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct KuraFlat {
    #[uniform(0)]
    globals: FeteGlobals,
}

impl Material2d for KuraFlat {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_kura/shaders/kura_flat.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Which of the four meshes an entity carries. Drawing order is the z it is
/// spawned at; this is only for finding the right mesh again.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Flow field, then the light flock.
    Ground,
    /// Heavy discs.
    Heavy,
    /// Links, filled triangles, ripples.
    Lattice,
    /// Small discs, then trails.
    Dust,
}

impl Layer {
    const ALL: [Self; 4] = [Self::Ground, Self::Heavy, Self::Lattice, Self::Dust];

    fn index(self) -> usize {
        match self {
            Self::Ground => 0,
            Self::Heavy => 1,
            Self::Lattice => 2,
            Self::Dust => 3,
        }
    }

    /// Just in front of the fullscreen quad at `-1.0`, in pass order.
    fn depth(self) -> f32 {
        -0.94 + self.index() as f32 * 0.01
    }

    fn sprites(self) -> bool {
        matches!(self, Self::Heavy | Self::Dust)
    }
}

/// The meshes and materials the visual draws through. Created once at startup
/// and reused for every activation.
#[derive(Resource, Debug)]
pub struct KuraAssets {
    meshes: [Handle<Mesh>; 4],
    sprite: Handle<KuraSprite>,
    flat: Handle<KuraFlat>,
}

/// Everything that is simulated.
#[derive(Resource, Debug)]
pub struct KuraSim {
    rng: Rng,
    params: FlockParams,
    flocks: Flocks,
    kuramoto: Kuramoto,
    flow: FlowField,
    emergent: Emergent,
    trails: Trails,
    colors: Colors,
    /// Vertex buffers, one per layer, kept so their capacity survives frames.
    buffers: [MeshBuf; 4],
    /// Beat position the next ripple is due at. Set on the first frame, once
    /// the show clock is known.
    next_field: Option<f64>,
    /// Ripples alternate between pulling and pushing.
    attract_next: bool,
}

impl KuraSim {
    fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let params = FlockParams::default();
        let flocks = Flocks::new(&mut rng, &params);
        let kuramoto = Kuramoto::new(N_HEAVY, &mut rng);
        let colors = Colors::new(&mut rng);
        let mut trails = Trails::new();
        trails.reset(&flocks.heavy);

        Self {
            rng,
            params,
            flocks,
            kuramoto,
            flow: FlowField::new(),
            emergent: Emergent::new(),
            trails,
            colors,
            buffers: Default::default(),
            next_field: None,
            attract_next: true,
        }
    }
}

impl Default for KuraSim {
    fn default() -> Self {
        Self::new(0x5EED)
    }
}

/// Registers Kura with the show.
pub struct KuraPlugin;

impl Plugin for KuraPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/kura.wgsl");
        embedded_asset!(app, "shaders/kura_sprite.wgsl");
        embedded_asset!(app, "shaders/kura_flat.wgsl");

        app.add_plugins((
            Material2dPlugin::<KuraSprite>::default(),
            Material2dPlugin::<KuraFlat>::default(),
        ))
        .init_resource::<KuraSim>()
        .add_systems(Startup, create_assets)
        .add_visual::<Kura>();

        app.add_systems(OnEnter(ActiveVisual::of::<Kura>()), begin_run);

        // In `Animate`, so it runs after the frame's globals and macros are
        // settled and before the visual-change machinery at the end of Update.
        app.add_visual_systems::<Kura, _>(Update, drive.in_set(VisualSystems::Animate));
    }
}

fn create_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprites: ResMut<Assets<KuraSprite>>,
    mut flats: ResMut<Assets<KuraFlat>>,
) {
    commands.insert_resource(KuraAssets {
        meshes: std::array::from_fn(|_| meshes.add(MeshBuf::empty_mesh())),
        sprite: sprites.add(KuraSprite::default()),
        flat: flats.add(KuraFlat::default()),
    });
}

/// Reseed the whole simulation and put the four meshes on stage.
///
/// A fresh scatter every time rather than a paused world resumed: the flock
/// takes a few seconds to find its structure, and watching it do that is a
/// better opening than cutting into the middle of a settled one.
fn begin_run(
    mut commands: Commands,
    assets: Res<KuraAssets>,
    output: Res<ShowOutput>,
    mut sim: ResMut<KuraSim>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let seed = (output.seed as f64 * 4_294_967_296.0) as u64 ^ 0x9E37_79B9_7F4A_7C15;
    *sim = KuraSim::new(seed);

    // Old geometry would otherwise flash on the first frame, before `drive`
    // has had a chance to rebuild it.
    for handle in &assets.meshes {
        if let Some(mut mesh) = meshes.get_mut(handle) {
            MeshBuf::default().write_into(&mut mesh);
        }
    }

    for layer in Layer::ALL {
        let mut entity = commands.spawn((
            Name::new(format!("kura {layer:?}")),
            layer,
            Mesh2d(assets.meshes[layer.index()].clone()),
            Transform::from_xyz(0.0, 0.0, layer.depth()),
            // The mesh is rewritten every frame, so any bounds computed from it
            // are a frame stale. Nothing here is ever off screen anyway.
            NoFrustumCulling,
            DespawnOnExit(ActiveVisual::of::<Kura>()),
        ));
        if layer.sprites() {
            entity.insert(MeshMaterial2d(assets.sprite.clone()));
        } else {
            entity.insert(MeshMaterial2d(assets.flat.clone()));
        }
    }
}

/// One frame: step the simulation, rebuild the geometry, resize the stage.
fn drive(
    mut sim: ResMut<KuraSim>,
    assets: Res<KuraAssets>,
    globals: Res<FeteGlobals>,
    clock: Res<ShowClock>,
    quality: Res<Quality>,
    macros: Res<Macros>,
    palette: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprites: ResMut<Assets<KuraSprite>>,
    mut flats: ResMut<Assets<KuraFlat>>,
    mut layers: Query<(&Layer, &mut Transform)>,
) {
    let sim = &mut *sim;

    // --- what the knobs mean --------------------------------------------------
    let knob = |index: usize, lo: f32, hi: f32| lo + (hi - lo) * macros.get(index);

    let base = FlockParams::default();
    let cohesion = knob(1, 0.35, 1.9);
    let speed = knob(2, 0.55, 1.6);

    sim.params = FlockParams {
        coh_heavy: base.coh_heavy * cohesion,
        coh_light: base.coh_light * cohesion,
        coh_small: base.coh_small * cohesion,
        min_speed_heavy: base.min_speed_heavy * speed,
        max_speed_heavy: base.max_speed_heavy * speed,
        min_speed_light: base.min_speed_light * speed,
        max_speed_light: base.max_speed_light * speed,
        min_speed_small: base.min_speed_small * speed,
        max_speed_small: base.max_speed_small * speed,
        desired_sep: knob(3, 1.6, 5.2),
        // Inverted: turning the knob up should give *more* lattice, which means
        // demanding less agreement before a link forms.
        align_dot_min: knob(5, 0.92, 0.30),
        ..base
    };

    // --- time -----------------------------------------------------------------
    let dt = clock.delta.min(MAX_STEP);
    // Reference-tempo seconds. Identical to wall time at 128bpm.
    let tempo = clock.bpm / 128.0;
    let musical_dt = dt * tempo;
    let musical = clock.beats as f32 * REFERENCE_BEAT;

    // --- simulate -------------------------------------------------------------
    sim.flocks.step(dt, &sim.params, &mut sim.rng);

    // The grids are rebuilt again because the six flocking passes have moved
    // everything since the last rebuild, and both the oscillators and the
    // lattice want to know where things are *now*.
    sim.flocks.rebuild_grids();
    let Flocks {
        heavy, grid_heavy, ..
    } = &sim.flocks;
    sim.kuramoto
        .update(heavy, grid_heavy, musical_dt, &mut sim.rng);
    sim.emergent
        .update(heavy, grid_heavy, sim.params.align_dot_min);
    sim.trails.push(heavy);

    sim.flow.update(&sim.flocks, &sim.params, musical);
    sim.colors.apply(&palette, globals.seed, knob(6, 0.0, 1.8));

    // --- ripples --------------------------------------------------------------
    // A field lands every so many beats, alternating between pulling the flock
    // in and shoving it apart. It is the only event in the piece, so it is kept
    // rare: at the middle of the knob roughly one every thirty-five seconds.
    let interval = knob(7, 128.0, 24.0) as f64;
    let due = *sim.next_field.get_or_insert(clock.beats + interval);
    if clock.beats >= due {
        let sign = if sim.attract_next { 1.0 } else { -1.0 };
        sim.attract_next = !sim.attract_next;
        let rng = &mut sim.rng;
        sim.flocks.add_field(rng, sign);
        sim.next_field = Some(clock.beats + interval);
    }

    // --- build the geometry ---------------------------------------------------
    let trail_length = knob(4, 0.35, 1.6);
    // What the frame may spend on vertices. This visual is CPU-bound, so this
    // is the knob that decides whether it holds framerate — see [`KuraBudget`].
    let budget = quality
        .tier
        .pick(KuraBudget::FULL, KuraBudget::MID, KuraBudget::LEAN);

    for buffer in &mut sim.buffers {
        buffer.clear();
    }

    let ground = &mut sim.buffers[Layer::Ground.index()];
    sim.flow.emit(ground, 1.0, budget.flow_history);
    render::emit_light_triangles(ground, &sim.flocks.light, &sim.colors.light, musical);

    render::emit_discs(
        &mut sim.buffers[Layer::Heavy.index()],
        &sim.flocks.heavy,
        &sim.colors.heavy,
        Some(sim.kuramoto.pulses()),
        HEAVY_PT,
        musical,
    );

    let lattice = &mut sim.buffers[Layer::Lattice.index()];
    sim.emergent.emit(lattice, &sim.flocks.heavy, 1.0, budget);
    render::emit_rings(lattice, &sim.flocks.fields, 1.0);

    let dust = &mut sim.buffers[Layer::Dust.index()];
    render::emit_discs(
        dust,
        &sim.flocks.small,
        &sim.colors.small,
        None,
        SMALL_PT,
        musical,
    );
    sim.trails.emit(
        dust,
        &sim.colors.heavy,
        sim.kuramoto.pulses(),
        musical,
        trail_length,
        budget.trail_len,
    );

    for (index, handle) in assets.meshes.iter().enumerate() {
        if let Some(mut mesh) = meshes.get_mut(handle) {
            sim.buffers[index].write_into(&mut mesh);
        }
    }

    // --- present --------------------------------------------------------------
    // The geometry is built in the original's reference pixels; the transform
    // is what stretches that fixed frame onto whatever window it is given.
    let scale = Vec3::new(
        globals.resolution.x / REFERENCE[0],
        globals.resolution.y / REFERENCE[1],
        1.0,
    );
    for (layer, mut transform) in &mut layers {
        transform.scale = scale;
        transform.translation.z = layer.depth();
    }

    for (_, material) in sprites.iter_mut() {
        material.globals = *globals;
    }
    for (_, material) in flats.iter_mut() {
        material.globals = *globals;
    }
}
