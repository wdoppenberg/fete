//! The render-world half of the slime simulation.
//!
//! Everything here lives in `RenderApp`. The main world owns the parameters and
//! the texture handles; they cross into the render world by extraction, and
//! this module turns them into dispatches.
//!
//! In Bevy 0.19 the render graph is a *schedule* and graph nodes are ordinary
//! systems taking [`RenderContext`] as a parameter — so the compute pass below
//! is just a system ordered before the camera driver.

use bevy::asset::load_embedded_asset;
use bevy::core_pipeline::schedule::camera_driver;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    storage_buffer_sized, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderGraph, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use fete_core::prelude::*;
use rand::Rng;

use crate::{SLIME_FORMAT, SlimeConfig, SlimeMarker, SlimeParams, SlimeRun};

/// Threads per workgroup in the agent pass. 64 is one Apple/AMD wavefront and
/// two NVIDIA warps — a safe default across the hardware a laptop VJ rig runs.
const AGENT_WORKGROUP: u32 = 64;
/// Tile size for the texture passes.
const GRID_WORKGROUP: u32 = 8;

/// One simulated agent. Must match `Agent` in `slime.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Agent {
    pos: [f32; 2],
    angle: f32,
    kind: f32,
}

/// GPU-side storage that outlives individual frames.
#[derive(Resource)]
pub struct SlimeBuffers {
    agents: Buffer,
    deposits: Buffer,
    params: UniformBuffer<SlimeParams>,
}

#[derive(Resource)]
pub struct SlimePipelines {
    layout: BindGroupLayoutDescriptor,
    update_agents: CachedComputePipelineId,
    diffuse: CachedComputePipelineId,
    clear: CachedComputePipelineId,
}

/// One bind group per ping-pong phase, indexed by [`SimTextures::read_index`].
#[derive(Resource)]
pub struct SlimeBindGroups([BindGroup; 2]);

/// Where the simulation is in its lifecycle.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlimeState {
    /// Shaders are still compiling.
    Loading,
    /// Seeding the trail. Runs for both ping-pong phases so neither texture is
    /// left holding whatever was in it from a previous activation.
    Seeding(u8),
    Running,
}

/// Which activation generation the render world has seeded for.
///
/// The main world bumps a counter every time the visual is switched on; when
/// the render world notices the counter moved, it re-seeds. Without this, the
/// second time you cycle back to Slime you would be looking at the network you
/// left behind rather than a fresh one.
#[derive(Resource, Default)]
pub struct SeededGeneration(u32);

pub struct SlimeComputePlugin;

impl Plugin for SlimeComputePlugin {
    fn build(&self, app: &mut App) {
        let config = *app.world().resource::<SlimeConfig>();

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .insert_resource(config)
            .insert_resource(SlimeState::Loading)
            .init_resource::<SeededGeneration>()
            .add_systems(RenderStartup, init_slime_pipelines)
            .add_systems(
                Render,
                (
                    advance_state.in_set(RenderSystems::Prepare),
                    prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                ),
            )
            // The simulation must finish before the camera renders the quad
            // that samples its output, or the display lags a frame behind.
            .add_systems(RenderGraph, slime_pass.before(camera_driver));
    }
}

fn init_slime_pipelines(
    mut commands: Commands,
    config: Res<SlimeConfig>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "slime",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_storage_2d(SLIME_FORMAT, StorageTextureAccess::ReadOnly),
                texture_storage_2d(SLIME_FORMAT, StorageTextureAccess::WriteOnly),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                uniform_buffer::<SlimeParams>(false),
            ),
        ),
    );

    let shader: Handle<Shader> = load_embedded_asset!(asset_server.as_ref(), "shaders/slime.wgsl");

    let queue = |entry_point: &'static str| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("slime::{entry_point}").into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(entry_point.into()),
            ..default()
        })
    };

    let pipelines = SlimePipelines {
        update_agents: queue("update_agents"),
        diffuse: queue("diffuse"),
        clear: queue("clear"),
        layout,
    };

    // Agents are seeded on the CPU and uploaded once. A compute seeding pass
    // would avoid the upload, but this runs a single time at startup and being
    // able to change the starting distribution in ordinary Rust is worth more
    // than the milliseconds saved.
    let agents = seed_agents(&config);
    let agent_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("slime agents"),
        contents: bytemuck::cast_slice(&agents),
        usage: BufferUsages::STORAGE,
    });

    let texel_count = (config.size.x * config.size.y) as u64;
    let deposits = render_device.create_buffer(&BufferDescriptor {
        label: Some("slime deposits"),
        size: texel_count * size_of::<u32>() as u64,
        usage: BufferUsages::STORAGE,
        mapped_at_creation: false,
    });

    commands.insert_resource(pipelines);
    commands.insert_resource(SlimeBuffers {
        agents: agent_buffer,
        deposits,
        params: UniformBuffer::default(),
    });
}

/// Initial agent distribution: uniform over the frame, random headings.
///
/// Seeding a small disc instead gives a lovely growing-outward opening, but it
/// takes the better part of a minute to reach the corners — far too slow when
/// the visual has to look finished within a few seconds of being switched to
/// mid-set. Uniform seeding has the network covering the frame in about three
/// seconds and refining from there.
fn seed_agents(config: &SlimeConfig) -> Vec<Agent> {
    let mut rng = rand::rng();
    let size = config.size.as_vec2();

    (0..config.agent_count)
        .map(|_| Agent {
            pos: [rng.random::<f32>() * size.x, rng.random::<f32>() * size.y],
            angle: rng.random_range(0.0..std::f32::consts::TAU),
            kind: rng.random::<f32>(),
        })
        .collect()
}

fn advance_state(
    mut state: ResMut<SlimeState>,
    mut seeded: ResMut<SeededGeneration>,
    pipelines: Res<SlimePipelines>,
    pipeline_cache: Res<PipelineCache>,
    run: Res<SlimeRun>,
) {
    if !run.active {
        return;
    }

    // A new activation always re-seeds, whatever state we were in.
    if seeded.0 != run.generation && !matches!(*state, SlimeState::Loading) {
        seeded.0 = run.generation;
        *state = SlimeState::Seeding(0);
        return;
    }

    match *state {
        SlimeState::Loading => {
            let ready = pipelines_ready(
                &pipeline_cache,
                &[pipelines.update_agents, pipelines.diffuse, pipelines.clear],
            );
            if ready {
                seeded.0 = run.generation;
                *state = SlimeState::Seeding(0);
            }
        }
        SlimeState::Seeding(phase) if phase < 1 => *state = SlimeState::Seeding(phase + 1),
        SlimeState::Seeding(_) => *state = SlimeState::Running,
        SlimeState::Running => {}
    }
}

fn prepare_bind_groups(
    mut commands: Commands,
    pipelines: Res<SlimePipelines>,
    mut buffers: ResMut<SlimeBuffers>,
    textures: Res<SimTextures<SlimeMarker>>,
    params: Res<SlimeParams>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
) {
    // The images may not have been uploaded yet on the first frame or two.
    let (Some(a), Some(b)) = (gpu_images.get(&textures.a), gpu_images.get(&textures.b)) else {
        return;
    };

    buffers.params.set(*params);
    buffers.params.write_buffer(&render_device, &render_queue);

    let layout = pipeline_cache.get_bind_group_layout(&pipelines.layout);
    let group = |read: &GpuImage, write: &GpuImage| {
        render_device.create_bind_group(
            "slime",
            &layout,
            &BindGroupEntries::sequential((
                &read.texture_view,
                &write.texture_view,
                buffers.agents.as_entire_binding(),
                buffers.deposits.as_entire_binding(),
                &buffers.params,
            )),
        )
    };

    commands.insert_resource(SlimeBindGroups([group(a, b), group(b, a)]));
}

fn slime_pass(
    mut render_context: RenderContext,
    state: Res<SlimeState>,
    run: Res<SlimeRun>,
    config: Res<SlimeConfig>,
    pipelines: Res<SlimePipelines>,
    pipeline_cache: Res<PipelineCache>,
    bind_groups: Option<Res<SlimeBindGroups>>,
    textures: Res<SimTextures<SlimeMarker>>,
) {
    if !run.active {
        return;
    }
    let Some(bind_groups) = bind_groups else {
        return;
    };
    let bind_group = &bind_groups.0[textures.read_index()];

    let grid = workgroup_count_2d(config.size, GRID_WORKGROUP);
    let agents = workgroup_count_1d(config.agent_count, AGENT_WORKGROUP);

    let mut pass = render_context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("slime"),
            timestamp_writes: None,
        });
    pass.set_bind_group(0, bind_group, &[]);

    match *state {
        SlimeState::Loading => {}
        SlimeState::Seeding(_) => {
            if let Some(pipeline) = pipeline_cache.get_compute_pipeline(pipelines.clear) {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(grid.0, grid.1, grid.2);
            }
        }
        SlimeState::Running => {
            // Order matters: agents sense the trail as it stands and add their
            // deposits to the accumulator, then the diffuse pass folds those
            // deposits in while producing the next frame's trail. Running them
            // the other way round costs a frame of latency between an agent
            // moving and its deposit appearing.
            if let Some(pipeline) = pipeline_cache.get_compute_pipeline(pipelines.update_agents) {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(agents.0, agents.1, agents.2);
            }
            if let Some(pipeline) = pipeline_cache.get_compute_pipeline(pipelines.diffuse) {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(grid.0, grid.1, grid.2);
            }
        }
    }
}
