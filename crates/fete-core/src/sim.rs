//! Scaffolding for compute-driven visuals.
//!
//! Anything with feedback — reaction-diffusion, slime moulds, fluid, trails —
//! needs the same three things: a pair of storage textures that swap roles
//! every frame, those handles visible in the render world, and a way to hold
//! off dispatching until the pipelines have finished compiling. This module
//! provides all three so a simulation crate only writes its own passes.

use core::marker::PhantomData;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{
    CachedComputePipelineId, CachedPipelineState, PipelineCache, TextureFormat, TextureUsages,
};
use bevy::shader::ShaderCacheError;

/// Build an image usable as both a compute storage target and a sampled texture.
///
/// `Rgba32Float` is the usual choice: simulations accumulate values well
/// outside `0..1`, and clamping them to 8 bits shows up as banding in exactly
/// the smooth gradients these visuals are made of.
pub fn storage_texture(size: UVec2, format: TextureFormat) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, format, None);
    // The simulation is the only thing that ever writes these, so there is no
    // reason to keep a copy in main memory.
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING;
    image
}

/// A double-buffered pair of simulation textures.
///
/// `M` is a marker type so several simulations can coexist without colliding.
/// Which texture is being read and which is being written alternates every
/// frame; [`read`](Self::read) and [`write`](Self::write) resolve that for you.
#[derive(Resource, Debug)]
pub struct SimTextures<M: Send + Sync + 'static> {
    pub a: Handle<Image>,
    pub b: Handle<Image>,
    pub size: UVec2,
    /// Flips each frame. `false` means A is the read target.
    pub swapped: bool,
    marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Clone for SimTextures<M> {
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            b: self.b.clone(),
            size: self.size,
            swapped: self.swapped,
            marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> SimTextures<M> {
    /// Allocate a fresh pair of textures.
    pub fn new(images: &mut Assets<Image>, size: UVec2, format: TextureFormat) -> Self {
        let image = storage_texture(size, format);
        Self {
            a: images.add(image.clone()),
            b: images.add(image),
            size,
            swapped: false,
            marker: PhantomData,
        }
    }

    /// The texture holding last frame's state.
    pub fn read(&self) -> &Handle<Image> {
        if self.swapped { &self.b } else { &self.a }
    }

    /// The texture this frame's pass writes into.
    pub fn write(&self) -> &Handle<Image> {
        if self.swapped { &self.a } else { &self.b }
    }

    /// Index of the read texture, for picking between two prepared bind groups.
    pub fn read_index(&self) -> usize {
        usize::from(self.swapped)
    }
}

impl<M: Send + Sync + 'static> ExtractResource for SimTextures<M> {
    type Source = Self;

    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// Swaps the read and write textures. Run once per frame in the main world,
/// before extraction, so both worlds agree on which is which.
pub fn swap_sim_textures<M: Send + Sync + 'static>(mut textures: ResMut<SimTextures<M>>) {
    textures.swapped = !textures.swapped;
}

/// Mirrors [`SimTextures`] into the render world and keeps the swap running.
pub struct SimTexturePlugin<M: Send + Sync + 'static>(PhantomData<fn() -> M>);

impl<M: Send + Sync + 'static> Default for SimTexturePlugin<M> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<M: Send + Sync + 'static> Plugin for SimTexturePlugin<M> {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractResourcePlugin::<SimTextures<M>>::default());
    }
}

/// Whether a set of compute pipelines has finished compiling.
///
/// Dispatching against a pipeline that is still queued panics, and shader
/// compilation happens asynchronously over several frames, so every compute
/// visual needs this gate. Compile *errors*, unlike "not ready yet", are worth
/// surfacing loudly — a typo in WGSL otherwise shows up as a silently black
/// screen.
pub fn pipelines_ready(cache: &PipelineCache, ids: &[CachedComputePipelineId]) -> bool {
    ids.iter()
        .all(|id| match cache.get_compute_pipeline_state(*id) {
            CachedPipelineState::Ok(_) => true,
            CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_)) => false,
            CachedPipelineState::Err(err) => {
                error!("compute pipeline failed to compile: {err}");
                false
            }
            _ => false,
        })
}

/// Dispatch counts covering a 2d grid with the given workgroup size.
///
/// Rounds up, so the shader must bounds-check against the real size — the last
/// workgroup in each axis will usually run partly outside the texture.
pub fn workgroup_count_2d(size: UVec2, workgroup: u32) -> (u32, u32, u32) {
    let workgroup = workgroup.max(1);
    (size.x.div_ceil(workgroup), size.y.div_ceil(workgroup), 1)
}

/// Dispatch counts for a 1d workload, folded into 2d.
///
/// A particle count in the millions exceeds the 65535 limit on a single
/// dispatch dimension, so the workload is spread across x and y. Shaders index
/// with `id.x + id.y * num_workgroups.x * workgroup_size.x`.
pub fn workgroup_count_1d(count: u32, workgroup: u32) -> (u32, u32, u32) {
    const MAX_PER_DIM: u32 = 65535;
    let total = count.div_ceil(workgroup.max(1));
    let x = total.clamp(1, MAX_PER_DIM);
    (x, total.div_ceil(x), 1)
}
