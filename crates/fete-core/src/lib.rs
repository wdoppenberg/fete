//! `fete-core` — the framework layer under every visual.
//!
//! It owns the parts of a live visual show that are not specific to any one
//! look: a musical clock, a modulation matrix feeding eight macro knobs, a
//! palette that morphs rather than cuts, the HDR camera rig, and the machinery
//! that swaps exactly one [`Visual`](visual::Visual) onto the screen at a time.
//!
//! A visual is a `Material2d` on a viewport-filling quad. That constraint is
//! what keeps each one to a shader plus a small struct, and it is why they all
//! share the same glow, tonemapping and colour scheme without coordinating.
//!
//! # Adding a visual
//!
//! ```ignore
//! #[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
//! struct Ripples {
//!     #[uniform(0)]
//!     globals: FeteGlobals,
//! }
//!
//! impl Material2d for Ripples {
//!     fn fragment_shader() -> ShaderRef {
//!         load_embedded_asset!(self, "ripples.wgsl").into()
//!     }
//! }
//!
//! impl Visual for Ripples {
//!     const ID: VisualId = "ripples";
//!     const NAME: &'static str = "Ripples";
//!     fn globals_mut(&mut self) -> &mut FeteGlobals {
//!         &mut self.globals
//!     }
//! }
//!
//! app.add_visual::<Ripples>();
//! ```

pub mod autopilot;
pub mod bleed;
pub mod clock;
pub mod globals;
pub mod grade;
pub mod palette;
pub mod present;
pub mod quality;
pub mod signal;
pub mod sim;
pub mod stage;
pub mod visual;

pub mod prelude;

use bevy::prelude::*;
use bevy::shader::load_shader_library;

use crate::autopilot::{Autopilot, run_autopilot};
use crate::bleed::{BleedPlugin, advance_transition};
use crate::clock::{ShowClock, advance_clock};
use crate::globals::{FeteGlobals, ShowOutput, update_globals};
use crate::grade::GradePlugin;
use crate::palette::{Palette, PaletteMorph, advance_palette_morph};
use crate::present::{
    StageResolution, follow_stage_target, retarget_stage_camera, setup_stage_target,
    track_stage_resolution,
};
use crate::quality::{Quality, detect_quality};
use crate::signal::{Audio, Macros, Modulation, apply_modulation, simulate_audio};
use crate::stage::{StageSettings, spawn_stage_camera, sync_stage_settings};
use crate::visual::{
    ActiveVisual, VisualQuad, VisualRegistry, VisualRequest, VisualSystems, apply_visual_requests,
    fit_surface_to_viewport,
};

/// Installs the framework: clock, signals, palette, camera rig and visual
/// switching. Add this before any [`add_visual`](visual::VisualAppExt::add_visual) call.
///
/// This does *not* add `DefaultPlugins` — see `fete-app` for the batteries-included
/// shell, or add them yourself for an embedded use.
pub struct FeteCorePlugin;

impl Plugin for FeteCorePlugin {
    fn build(&self, app: &mut App) {
        // Shared WGSL, embedded in the binary so downstream visual crates need
        // no assets folder of their own. With the `hot-shaders` feature these
        // still reload from disk while the app runs.
        load_shader_library!(app, "shaders/globals.wgsl");
        load_shader_library!(app, "shaders/noise.wgsl");
        load_shader_library!(app, "shaders/palette.wgsl");
        // Not libraries — the fragment shaders of the two passes the camera
        // rig owns.
        bevy::asset::embedded_asset!(app, "shaders/grade.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/bleed.wgsl");

        app.init_resource::<ShowClock>()
            .init_resource::<Macros>()
            .init_resource::<Modulation>()
            .init_resource::<Audio>()
            .init_resource::<Palette>()
            .init_resource::<PaletteMorph>()
            .init_resource::<FeteGlobals>()
            .init_resource::<ShowOutput>()
            .init_resource::<StageSettings>()
            .init_resource::<Quality>()
            .init_resource::<StageResolution>()
            .init_resource::<Autopilot>()
            .init_resource::<VisualRegistry>()
            .init_resource::<VisualQuad>()
            .init_state::<ActiveVisual>()
            .add_message::<VisualRequest>();

        app.add_plugins((BleedPlugin, GradePlugin));

        // Strictly ordered: the probe may lower the tier, the tier decides
        // whether there is a render target at all, and the camera has to be
        // pointed at it before the first frame — a camera retargeted on frame
        // two shows one frame of the wrong thing at every startup.
        app.add_systems(
            Startup,
            (
                detect_quality,
                setup_stage_target,
                spawn_stage_camera,
                retarget_stage_camera,
            )
                .chain(),
        );

        // Everything that defines "now" is resolved in `First`, so any system
        // in `Update` — framework or visual — reads a consistent frame.
        app.add_systems(
            First,
            (
                track_stage_resolution,
                follow_stage_target,
                advance_clock,
                advance_transition,
                simulate_audio,
                apply_modulation,
                advance_palette_morph,
            )
                .chain(),
        );

        app.configure_sets(
            Update,
            (VisualSystems::Prepare, VisualSystems::Animate).chain(),
        );

        app.add_systems(
            Update,
            (update_globals, fit_surface_to_viewport)
                .chain()
                .in_set(VisualSystems::Prepare),
        );

        // The autopilot runs before the frame is assembled so its fade and
        // drift are visible this frame, not next.
        app.add_systems(First, run_autopilot.after(apply_modulation));

        // Visual switching runs last so a request raised this frame takes
        // effect at the state transition before the next one.
        app.add_systems(
            Update,
            (apply_visual_requests, sync_stage_settings).after(VisualSystems::Animate),
        );
    }
}
