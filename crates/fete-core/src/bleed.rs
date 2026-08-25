//! Bleed transitions — the frame that is on screen dissolving into the one
//! replacing it.
//!
//! A cut between two full-screen shaders is the one moment a generative show
//! looks like software. The old fix was a fade through black, which is honest
//! but costs two seconds of nothing on a screen whose whole job is to be lit.
//! This is the other answer: keep the frame that was already there and let the
//! incoming visual come up *through* it.
//!
//! The mechanism is a feedback buffer. One extra full-screen pass sits in the
//! camera's post-process chain, and every frame it writes its result to two
//! places at once — the view, and a history texture it reads back the next
//! frame. Normally it keeps nothing, so the pass is a copy and the picture is
//! unchanged. During a transition it keeps most of the previous frame,
//! displaced slightly, so the outgoing image smears, sags or burns away over a
//! few beats while the new one rises underneath it.
//!
//! Because the history is *the composited output* rather than the outgoing
//! visual, nothing here needs to know which visuals are involved, or that a
//! visual changed at all. That is what makes it work for every pair of visuals
//! in the registry, including the ones nobody has written yet.
//!
//! Runs in HDR, before bloom: the trails are real image data, so they should
//! glow and roll off through the same tone curve as everything else. Doing it
//! after tonemapping would give grey smears instead of neon ones.

use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::{Core2d, Core2dSystems};
use bevy::ecs::error::BevyError;
use bevy::image::ToExtents;
use bevy::post_process::bloom::bloom;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    Canonical, ColorTargetState, ColorWrites, FilterMode, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, Specializer,
    SpecializerKey, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, Variants,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::clock::ShowClock;
use crate::present::StageResolution;

/// How the outgoing frame leaves.
///
/// All six are the same feedback loop with a different displacement and a
/// different rule for which pixels let go first; the variety matters more than
/// any one of them, because a transition the audience can predict stops being
/// a transition and becomes a page turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum BleedStyle {
    /// Wind. The frame streaks off in one slowly turning direction.
    #[default]
    Smear = 0,
    /// Erosion. Each pixel has its own moment to let go, so the frame tears
    /// away in patches with a glowing edge.
    Dissolve = 1,
    /// Gravity. Columns of the old frame sag out of the bottom of the screen.
    Melt = 2,
    /// A drain. The frame rotates into the centre, faster the closer it gets.
    Swirl = 3,
    /// Fire. Brightness decides: the dark parts go first and the neon holds on
    /// until last, which is the shape these images actually have.
    Burn = 4,
    /// Speed. The frame magnifies past the viewer, dragging colour behind it.
    Rush = 5,
}

impl BleedStyle {
    pub const ALL: [Self; 6] = [
        Self::Smear,
        Self::Dissolve,
        Self::Melt,
        Self::Swirl,
        Self::Burn,
        Self::Rush,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Smear => "smear",
            Self::Dissolve => "dissolve",
            Self::Melt => "melt",
            Self::Swirl => "swirl",
            Self::Burn => "burn",
            Self::Rush => "rush",
        }
    }
}

/// The show-wide transition state: what a visual change looks like, and how
/// far through the current one we are.
///
/// Everything is phrased in beats, like the rest of the framework, so a
/// transition lands with the music at any tempo.
#[derive(Resource, Debug, Clone)]
pub struct Transition {
    /// Whether visual changes bleed at all. Off means a hard cut.
    pub enabled: bool,
    /// How long a transition lasts.
    ///
    /// This is the *envelope*, not how long the smear is visible — a trail
    /// shorter than the envelope has already faded before the envelope closes.
    /// Two bars is enough to read as deliberate without holding the old image
    /// long enough for anyone to wonder whether something is stuck.
    pub beats: f32,
    /// How long a pixel of the outgoing frame survives, in beats, to `1/e`.
    ///
    /// The single control over how heavy the effect is. Below about half a
    /// beat it reads as motion blur; above about four it reads as two visuals
    /// playing at once.
    pub trail_beats: f32,
    /// How far the outgoing frame travels per second, in screens.
    pub warp: f32,
    /// Force one style, or `None` to keep choosing new ones.
    pub style: Option<BleedStyle>,

    chosen: BleedStyle,
    started: f64,
    progress: f32,
    seed: f32,
    rng: u32,
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            enabled: true,
            beats: 6.0,
            trail_beats: 2.0,
            warp: 0.3,
            style: None,
            chosen: BleedStyle::Smear,
            started: 0.0,
            // Idle: nothing to bleed, and the pass falls through to a copy.
            progress: 1.0,
            seed: 0.0,
            rng: 0x2545_F491,
        }
    }
}

impl Transition {
    /// Begin a transition from whatever is currently on screen.
    ///
    /// Call this *as* the change is requested, not after it lands: the frame
    /// being kept is the one the outgoing visual drew.
    pub fn start(&mut self, clock: &ShowClock) {
        if !self.enabled {
            return;
        }
        self.chosen = match self.style {
            Some(style) => style,
            None => self.pick(),
        };
        self.seed = self.next_f32();
        self.started = clock.beats;
        self.progress = 0.0;
    }

    /// The style the current transition is using.
    pub fn style(&self) -> BleedStyle {
        self.chosen
    }

    /// Progress through the current transition, `0.0..1.0`. `1.0` means idle.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn active(&self) -> bool {
        self.progress < 1.0
    }

    /// A style other than the one just used.
    ///
    /// Deliberately never a repeat: two identical transitions in a row read as
    /// a bug rather than as chance, and with six styles the audience cannot
    /// tell the difference between this and a free choice anyway.
    fn pick(&mut self) -> BleedStyle {
        let all = BleedStyle::ALL;
        let current = all
            .iter()
            .position(|style| *style == self.chosen)
            .unwrap_or(0);
        let offset = 1 + (self.next_u32() as usize % (all.len() - 1));
        all[(current + offset) % all.len()]
    }

    /// xorshift32, matching the autopilot's — the requirement is "not
    /// obviously periodic", not statistical quality.
    fn next_u32(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1 << 24) as f32
    }
}

/// Advances the current transition. Runs in `First`, right behind the clock.
pub fn advance_transition(clock: Res<ShowClock>, mut transition: ResMut<Transition>) {
    // Early out rather than writing 1.0 every frame: this is a change-detected
    // resource and the HUD reads it.
    if transition.progress >= 1.0 {
        return;
    }
    let span = transition.beats.max(0.01) as f64;
    let elapsed = clock.beats - transition.started;
    transition.progress = (elapsed / span).clamp(0.0, 1.0) as f32;
}

/// The bleed pass's uniform, attached to the camera it runs on.
///
/// Field order and types must stay in lockstep with `Bleed` in
/// `shaders/bleed.wgsl`.
#[derive(Component, ExtractComponent, ShaderType, Debug, Clone, Copy)]
pub struct Bleed {
    /// Render target size in pixels.
    pub resolution: Vec2,
    pub time: f32,
    /// Seconds since the previous frame. The decay is expressed per second and
    /// resolved against this, so the look does not change with the frame rate.
    pub delta: f32,
    /// Transition progress, `0.0..1.0`. `1.0` is idle.
    pub progress: f32,
    /// Seconds for the outgoing frame to decay to `1/e`.
    pub trail: f32,
    /// Screens travelled per second.
    pub warp: f32,
    /// Half-time beat envelope, so the smear surges with the track.
    pub pulse: f32,
    /// Random per transition, so the same style never tears the same way twice.
    pub seed: f32,
    /// [`BleedStyle`] as its discriminant.
    pub style: u32,
}

impl Default for Bleed {
    fn default() -> Self {
        Self {
            resolution: Vec2::new(1920.0, 1080.0),
            time: 0.0,
            delta: 1.0 / 60.0,
            progress: 1.0,
            trail: 0.0,
            warp: 0.0,
            pulse: 0.0,
            seed: 0.0,
            style: BleedStyle::Smear as u32,
        }
    }
}

/// Feeds the clock and the transition into the pass each frame.
fn update_bleed(
    clock: Res<ShowClock>,
    transition: Res<Transition>,
    stage: Res<StageResolution>,
    mut bleeds: Query<&mut Bleed>,
) {
    let beat = clock.beat_duration();

    for mut bleed in &mut bleeds {
        bleed.resolution = stage.0;
        bleed.time = clock.elapsed as f32;
        bleed.delta = clock.delta;
        bleed.progress = if transition.enabled {
            transition.progress
        } else {
            1.0
        };
        bleed.trail = transition.trail_beats.max(0.0) * beat;
        bleed.warp = transition.warp;
        // Half-time, like everything else that reacts to the track: a smear
        // that surges on every kick pulls the eye, one that surges every other
        // beat reads as breathing.
        bleed.pulse = clock.pulse_div(2.0, 2.0);
        bleed.seed = transition.seed;
        bleed.style = transition.chosen as u32;
    }
}

/// Installs the transition state and the feedback pass.
pub struct BleedPlugin;

impl Plugin for BleedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Transition>()
            .add_plugins((
                ExtractComponentPlugin::<Bleed>::default(),
                UniformComponentPlugin::<Bleed>::default(),
            ))
            .add_systems(Update, update_bleed);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .add_systems(RenderStartup, init_bleed_pipeline)
            .add_systems(
                Render,
                (
                    prepare_bleed_pipelines.in_set(RenderSystems::Prepare),
                    prepare_bleed_history.in_set(RenderSystems::PrepareResources),
                ),
            )
            // Before bloom, so a trail glows the same way the pixel that left
            // it did. After bloom the smear would be flat and the transition
            // would read as darker than either visual.
            .add_systems(
                Core2d,
                bleed_pass.in_set(Core2dSystems::PostProcess).before(bloom),
            );
    }
}

#[derive(Resource)]
struct BleedPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    variants: Variants<RenderPipeline, BleedSpecializer>,
}

struct BleedSpecializer;

#[derive(PartialEq, Eq, Hash, Clone, Copy, SpecializerKey)]
struct BleedPipelineKey {
    target_format: TextureFormat,
}

impl Specializer<RenderPipeline> for BleedSpecializer {
    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let fragment = descriptor.fragment_mut()?;
        let target = ColorTargetState {
            format: key.target_format,
            blend: None,
            write_mask: ColorWrites::ALL,
        };
        // Two targets: the view, and the history the next frame reads back.
        // Writing both in one pass is what keeps this to a single extra
        // full-screen draw per frame.
        fragment.set_target(0, target.clone());
        fragment.set_target(1, target);
        Ok(key)
    }

    type Key = BleedPipelineKey;
}

fn init_bleed_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "bleed_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // This frame, as the visuals drew it.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // What this pass wrote last frame.
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<Bleed>(true),
            ),
        ),
    );

    // Linear, and explicitly so: the history is read back at fractional
    // offsets every frame, and a nearest sample would quantise the drift into
    // visible steps rather than a smooth streak.
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("bleed_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });

    let variants = Variants::new(
        BleedSpecializer,
        RenderPipelineDescriptor {
            label: Some("bleed_pipeline".into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: asset_server.load("embedded://fete_core/shaders/bleed.wgsl"),
                ..default()
            }),
            ..default()
        },
    );

    commands.insert_resource(BleedPipeline {
        layout,
        sampler,
        variants,
    });
}

#[derive(Component)]
struct BleedPipelineId(CachedRenderPipelineId);

fn prepare_bleed_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<BleedPipeline>,
    views: Query<(Entity, &ExtractedView), With<Bleed>>,
) -> Result<(), BevyError> {
    for (entity, view) in &views {
        let id = pipeline.variants.specialize(
            &pipeline_cache,
            BleedPipelineKey {
                target_format: view.target_format,
            },
        )?;
        commands.entity(entity).insert(BleedPipelineId(id));
    }
    Ok(())
}

/// The two textures the feedback ping-pongs between.
#[derive(Component)]
struct BleedHistory {
    read: CachedTexture,
    write: CachedTexture,
}

/// Claims this frame's history textures and decides which way round they go.
///
/// Two textures rather than one because a pass cannot sample a texture it is
/// also drawing into. The cache hands back the same pair every frame as long
/// as the descriptors match, so the contents survive from one frame to the
/// next — which is the entire point.
fn prepare_bleed_history(
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
    mut flip: Local<bool>,
    views: Query<(Entity, &ExtractedView, &ExtractedCamera), With<Bleed>>,
) {
    *flip = !*flip;

    for (entity, view, camera) in &views {
        let Some(size) = camera.physical_target_size else {
            continue;
        };

        let mut descriptor = TextureDescriptor {
            label: Some("bleed_history_a"),
            size: size.to_extents(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            // Matching the view: the history holds real HDR image data, and
            // rounding it to display range would clip exactly the bright
            // pixels whose trails are the point.
            format: view.target_format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };
        let a = texture_cache.get(&render_device, descriptor.clone());
        descriptor.label = Some("bleed_history_b");
        let b = texture_cache.get(&render_device, descriptor);

        let history = if *flip {
            BleedHistory { read: a, write: b }
        } else {
            BleedHistory { read: b, write: a }
        };
        commands.entity(entity).insert(history);
    }
}

/// Composites this frame over the last one.
///
/// Runs every frame, transition or not. Skipping it when idle would be free,
/// but then the history would hold whatever was on screen the last time a
/// transition ended — and the one frame that has to be right is the one at the
/// cut, which is exactly the frame a skipped pass would not have recorded.
fn bleed_pass(
    view: ViewQuery<(
        &ViewTarget,
        &BleedHistory,
        &BleedPipelineId,
        &DynamicUniformIndex<Bleed>,
    )>,
    pipeline: Option<Res<BleedPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Bleed>>,
    mut ctx: RenderContext,
) {
    let (view_target, history, pipeline_id, uniform_index) = view.into_inner();

    let Some(pipeline) = pipeline else {
        return;
    };
    let (Some(render_pipeline), Some(uniform)) = (
        pipeline_cache.get_render_pipeline(pipeline_id.0),
        uniforms.uniforms().binding(),
    ) else {
        return;
    };

    let target = view_target.post_process_write();

    let bind_group = ctx.render_device().create_bind_group(
        "bleed_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            target.source,
            &history.read.default_view,
            &pipeline.sampler,
            uniform,
        )),
    );

    let attachment = |view| {
        Some(RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })
    };

    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("bleed"),
        color_attachments: &[
            attachment(target.destination),
            attachment(&history.write.default_view),
        ],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });

    pass.set_render_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[uniform_index.index()]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with just the resources `advance_transition` touches.
    fn harness() -> App {
        let mut app = App::new();
        app.init_resource::<ShowClock>()
            .init_resource::<Transition>()
            .add_systems(Update, advance_transition);
        app
    }

    fn beats(app: &mut App, to: f64) -> f32 {
        app.world_mut().resource_mut::<ShowClock>().beats = to;
        app.update();
        app.world().resource::<Transition>().progress()
    }

    #[test]
    fn a_transition_never_repeats_a_style() {
        let mut transition = Transition::default();
        let clock = ShowClock::default();

        let mut last = transition.style();
        for _ in 0..64 {
            transition.start(&clock);
            assert_ne!(
                transition.style(),
                last,
                "the same bleed twice running reads as a stuck transition"
            );
            last = transition.style();
        }
    }

    #[test]
    fn progress_runs_to_one_over_the_configured_beats() {
        let mut app = harness();
        let span = app.world().resource::<Transition>().beats as f64;

        {
            let clock = app.world().resource::<ShowClock>().clone();
            app.world_mut().resource_mut::<Transition>().start(&clock);
        }
        assert_eq!(app.world().resource::<Transition>().progress(), 0.0);
        assert!(app.world().resource::<Transition>().active());

        assert!((beats(&mut app, span * 0.5) - 0.5).abs() < 1e-5);
        assert_eq!(beats(&mut app, span), 1.0);
        assert!(!app.world().resource::<Transition>().active());

        // And it stays finished rather than wrapping round to another one.
        assert_eq!(beats(&mut app, span * 4.0), 1.0);
    }

    #[test]
    fn a_forced_style_is_kept() {
        let mut transition = Transition {
            style: Some(BleedStyle::Burn),
            ..default()
        };
        let clock = ShowClock::default();

        for _ in 0..4 {
            transition.start(&clock);
            assert_eq!(transition.style(), BleedStyle::Burn);
        }
    }

    #[test]
    fn a_disabled_transition_stays_idle() {
        // The pass still runs — it has to, to keep the history current — but
        // with nothing to show, so a visual change is a hard cut.
        let mut transition = Transition {
            enabled: false,
            ..default()
        };
        transition.start(&ShowClock::default());
        assert!(!transition.active());
    }
}
