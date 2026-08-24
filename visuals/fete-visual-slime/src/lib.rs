//! **Slime** — a physarum agent simulation on the GPU.
//!
//! The visual that exercises the framework's compute scaffolding. Sprawl is a
//! pure function of position and time; Slime has state — a few million agents
//! and a trail texture that feed back into each other every frame. It is here
//! to show that the [`Visual`] abstraction still holds when a visual is a
//! simulation: the presentation is a fullscreen material exactly like Sprawl,
//! and everything underneath is a plugin.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | display gain |
//! | W/S | 1 | sensor angle — the single biggest control over what grows |
//! | E/D | 2 | move speed |
//! | R/F | 3 | trail decay — how long the network remembers |
//! | T/G | 4 | sensor distance — sets the scale of the mesh |
//! | Y/H | 5 | filament edges |
//! | U/J | 6 | colour spread |
//! | I/K | 7 | beat reactivity |

mod compute;

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::TextureFormat;
use fete_core::prelude::*;

use crate::compute::SlimeComputePlugin;

/// Storage format for the trail texture. See the note in `slime.wgsl`.
pub const SLIME_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Marker distinguishing this simulation's [`SimTextures`] from any other's.
#[derive(Debug, Clone, Copy)]
pub struct SlimeMarker;

/// Fixed simulation setup. Changing these needs a restart.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SlimeConfig {
    /// Trail resolution. Independent of the window — the display material
    /// stretches it — so the simulation cost does not change when the operator
    /// goes fullscreen on a 4K projector.
    pub size: UVec2,
    pub agent_count: u32,
}

impl Default for SlimeConfig {
    fn default() -> Self {
        Self {
            size: UVec2::new(1920, 1080),
            // Roughly one agent per pixel. Denser looks like fog because every
            // texel saturates; much sparser and the network never closes into
            // continuous filaments.
            agent_count: 2_000_000,
        }
    }
}

/// Per-frame simulation parameters. Must match `SlimeParams` in `slime.wgsl`.
#[derive(Resource, ShaderType, ExtractResource, Debug, Clone, Copy)]
pub struct SlimeParams {
    pub resolution: Vec2,
    pub agent_count: u32,
    pub time: f32,
    pub delta: f32,
    pub sensor_angle: f32,
    pub sensor_distance: f32,
    pub turn_speed: f32,
    pub move_speed: f32,
    pub deposit: f32,
    pub decay: f32,
    pub diffuse: f32,
    pub impulse: f32,
}

impl Default for SlimeParams {
    fn default() -> Self {
        Self {
            resolution: Vec2::new(1920.0, 1080.0),
            agent_count: 0,
            time: 0.0,
            delta: 0.0,
            sensor_angle: 0.4,
            sensor_distance: 9.0,
            turn_speed: 6.0,
            move_speed: 60.0,
            deposit: 0.16,
            decay: 0.94,
            diffuse: 0.35,
            impulse: 0.0,
        }
    }
}

/// Whether the simulation should step, and which activation this is.
#[derive(Resource, ExtractResource, Debug, Clone, Copy, Default)]
pub struct SlimeRun {
    pub active: bool,
    /// Incremented on every activation so the render world knows to re-seed.
    pub generation: u32,
}

/// Presentation uniforms. Must match `SlimeDisplay` in `slime_display.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct SlimeDisplay {
    pub texel: Vec2,
    pub energy: f32,
    pub _padding: f32,
}

/// The fullscreen material that presents the simulation.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct Slime {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    display: SlimeDisplay,
    // Re-pointed at the freshest trail texture every frame by
    // [`follow_trail_texture`], because the simulation ping-pongs between two.
    #[texture(2)]
    #[sampler(3)]
    trail: Option<Handle<Image>>,
}

impl Material2d for Slime {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_slime/shaders/slime_display.wgsl".into()
    }
}

impl Visual for Slime {
    const ID: VisualId = "slime";
    const NAME: &'static str = "Slime";
    const TAGS: &'static [&'static str] = &["compute", "agents", "organic"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn animate(&mut self, frame: &Frame) {
        // Half-time, smoothed over a third of a second: a swell, not a hit.
        let target =
            (frame.clock.pulse_div(2.0, 2.0) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        let alpha = 1.0 - (-frame.clock.delta / 0.3).exp();
        self.display.energy += (target - self.display.energy) * alpha;
    }
}

/// Registers Slime with the show.
pub struct SlimePlugin;

impl Plugin for SlimePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/slime.wgsl");
        embedded_asset!(app, "shaders/slime_display.wgsl");

        app.init_resource::<SlimeConfig>()
            .init_resource::<SlimeParams>()
            .init_resource::<SlimeRun>()
            .add_plugins((
                SimTexturePlugin::<SlimeMarker>::default(),
                ExtractResourcePlugin::<SlimeParams>::default(),
                ExtractResourcePlugin::<SlimeRun>::default(),
                SlimeComputePlugin,
            ))
            .add_systems(Startup, create_trail_textures)
            .add_visual::<Slime>();

        // The ping-pong swap runs unconditionally in `First`, ahead of both the
        // parameter update and extraction, so the main and render worlds never
        // disagree about which texture is being read this frame.
        app.add_systems(First, swap_sim_textures::<SlimeMarker>);

        app.add_systems(OnEnter(ActiveVisual::of::<Slime>()), begin_run)
            .add_systems(OnExit(ActiveVisual::of::<Slime>()), end_run);

        app.add_visual_systems::<Slime, _>(
            Update,
            (update_params, follow_trail_texture).in_set(VisualSystems::Animate),
        );
    }
}

fn create_trail_textures(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    config: Res<SlimeConfig>,
) {
    commands.insert_resource(SimTextures::<SlimeMarker>::new(
        &mut images,
        config.size,
        SLIME_FORMAT,
    ));
}

fn begin_run(mut run: ResMut<SlimeRun>) {
    run.active = true;
    run.generation = run.generation.wrapping_add(1);
}

fn end_run(mut run: ResMut<SlimeRun>) {
    run.active = false;
}

/// Maps macro knobs onto simulation parameters.
///
/// This is where a compute visual differs from a fullscreen one: the shader
/// cannot read `globals` because it runs in the render world with its own
/// bindings, so knob values are translated into simulation units here.
fn update_params(
    mut params: ResMut<SlimeParams>,
    config: Res<SlimeConfig>,
    clock: Res<ShowClock>,
    macros: Res<Macros>,
    audio: Res<Audio>,
) {
    let knob = |i: usize, lo: f32, hi: f32| lo + (hi - lo) * macros.get(i);

    params.resolution = config.size.as_vec2();
    params.agent_count = config.agent_count;
    params.time = clock.elapsed as f32;

    // Clamped: a long frame — a shader recompiling, the window being dragged —
    // would otherwise advance agents far enough to jump clean across the
    // network they were following and destroy it in one step.
    params.delta = clock.delta.min(1.0 / 30.0);

    // Sensor angle is the parameter worth putting on a knob. Narrow angles
    // give long straight filaments; wide angles give dense cellular meshes.
    // Everything between is a different organism.
    params.sensor_angle = knob(1, 0.15, 1.4);
    params.move_speed = knob(2, 15.0, 140.0);
    // Decay bottoms out well above zero: below about 0.85 the trail vanishes
    // faster than agents can reinforce it and no structure ever forms.
    params.decay = knob(3, 0.999, 0.88);
    params.sensor_distance = knob(4, 3.0, 26.0);
    params.diffuse = 0.32;

    // --- keep it out of equilibrium -----------------------------------------
    //
    // Left on fixed parameters this simulation *converges*. Within a minute or
    // so the network finds a configuration that satisfies its own rules and
    // then barely changes — which is precisely the state that is least
    // interesting to look at. The good-looking phase is the first fifteen
    // seconds, while the structure is still reorganising.
    //
    // So the parameters never hold still. Three slow oscillators on mutually
    // prime periods continuously move the target the agents are converging
    // towards, which keeps the network permanently mid-reorganisation without
    // ever visibly "changing setting". The periods are in beats and are
    // deliberately long — 37, 53 and 71 beats is roughly 17, 25 and 33 seconds
    // — so no single cycle is perceptible and their sum does not repeat for
    // over an hour.
    let wander = |period_beats: f32, phase: f32| {
        ((clock.beats as f32 / period_beats + phase) * std::f32::consts::TAU).sin()
    };

    // Sensor angle moves least: it decides what kind of organism this is, and
    // swinging it far enough to change that reads as a scene change.
    params.sensor_angle *= 1.0 + 0.30 * wander(37.0, 0.0);
    // Sensor distance moves most. It sets the scale of the mesh, so sweeping it
    // makes the network continuously rebuild between fine lace and broad
    // arteries — this is the one doing most of the work.
    params.sensor_distance *= 1.0 + 0.45 * wander(53.0, 0.31);
    params.move_speed *= 1.0 + 0.25 * wander(71.0, 0.62);

    // Derived after the wander, so turn rate stays matched to sensor angle.
    // Agents that turn much faster than they can sense produce noise; much
    // slower and they overshoot every filament they are trying to follow.
    params.turn_speed = params.sensor_angle * 14.0;

    // Deposit is chosen so the trail settles at a useful *scale*, not by
    // taste. A texel inside a tube receives roughly `deposit * concentration`
    // per frame and keeps `decay` of what it had, so it converges to
    // `deposit * concentration / (1 - decay)`. With one agent per texel on
    // average and tubes running perhaps eight times denser, this lands the
    // equilibrium near 2.0 — comfortably inside the display's tone curve
    // instead of pinned at its top, where every tube is the same white.
    const TUBE_CONCENTRATION: f32 = 8.0;
    const TARGET_DENSITY: f32 = 2.0;
    let agents_per_texel =
        config.agent_count as f32 / (config.size.x * config.size.y).max(1) as f32;
    params.deposit =
        TARGET_DENSITY * (1.0 - params.decay) / (agents_per_texel * TUBE_CONCENTRATION).max(0.01);

    // A radial kick on the beat, but only on the downbeat of each bar —
    // every beat would never let the network re-form.
    params.impulse = if clock.bar_phase() < 0.12 {
        (1.0 - clock.bar_phase() / 0.12) * audio.bass * 0.5
    } else {
        0.0
    };
}

/// Points the material at whichever trail texture was written this frame.
fn follow_trail_texture(
    textures: Res<SimTextures<SlimeMarker>>,
    config: Res<SlimeConfig>,
    mut materials: ResMut<Assets<Slime>>,
    surfaces: Query<&MeshMaterial2d<Slime>, With<VisualSurface>>,
) {
    let texel = Vec2::ONE / config.size.as_vec2();

    for handle in &surfaces {
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        material.display.texel = texel;
        let latest = textures.write();
        if material.trail.as_ref() != Some(latest) {
            material.trail = Some(latest.clone());
        }
    }
}
