//! Everything a visual crate normally needs.
//!
//! ```ignore
//! use bevy::prelude::*;
//! use fete_core::prelude::*;
//! ```

pub use crate::FeteCorePlugin;
pub use crate::autopilot::Autopilot;
pub use crate::bleed::{Bleed, BleedStyle, Transition};
pub use crate::clock::ShowClock;
pub use crate::globals::{FeteGlobals, Frame, ShowOutput};
pub use crate::grade::Grade;
pub use crate::palette::{Palette, PaletteMorph};
pub use crate::present::{PRESENT_LAYER, PresentCamera, StageResolution, StageTarget};
pub use crate::quality::{Quality, Tier};
pub use crate::signal::{Audio, Band, MACRO_COUNT, Macros, ModSource, Modulation, Modulator, Wave};
pub use crate::sim::{
    SimTexturePlugin, SimTextures, pipelines_ready, storage_texture, swap_sim_textures,
    workgroup_count_1d, workgroup_count_2d,
};
pub use crate::stage::{StageCamera, StageSettings};
pub use crate::visual::{
    ActiveVisual, Visual, VisualAppExt, VisualId, VisualInfo, VisualRegistry, VisualRequest,
    VisualSurface, VisualSystems,
};

// Re-exported so visual crates can implement `Material2d` without adding a
// direct dependency on the renderer internals.
pub use bevy::asset::load_embedded_asset;
pub use bevy::render::render_resource::{AsBindGroup, ShaderType};
pub use bevy::shader::{ShaderDefVal, ShaderRef};
pub use bevy::sprite_render::{
    AlphaMode2d, Material2d, Material2dKey, Material2dPlugin, MeshMaterial2d,
};

// The rest of what `Material2d::specialize` needs in its signature. A visual
// that scales its loop bounds with the quality tier has to implement it, and
// having to name four crates to write one function is exactly the kind of
// friction that makes people skip the cheap tier instead.
pub use bevy::mesh::MeshVertexBufferLayoutRef;
pub use bevy::render::render_resource::{RenderPipelineDescriptor, SpecializedMeshPipelineError};
