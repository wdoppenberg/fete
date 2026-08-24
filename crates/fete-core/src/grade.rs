//! The global grade — one post-process pass every visual is seen through.
//!
//! This is what gives a night a single look. Individual visuals differ in what
//! they draw; the grade decides how the whole show *feels*, and because it sits
//! on the camera rather than in any visual, changing it changes everything at
//! once and nothing can escape it.
//!
//! It also owns the two show-wide safety rails: [`Grade::exposure`], the single
//! control over how bright the night is, and [`Grade::aspect`], which masks the
//! output to the projector's shape.
//!
//! Runs *after* tonemapping, so it works on display-referred colour. Scanlines,
//! grain and chroma offset are artefacts of a display, and applying them in HDR
//! before the tone curve would let the tone curve smear them back out.

use bevy::core_pipeline::Core2dSystems;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::core_pipeline::schedule::Core2d;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::ecs::schedule::{ScheduleConfigs, ScheduleLabel};
use bevy::ecs::system::BoxedSystem;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;

use crate::clock::ShowClock;
use crate::globals::ShowOutput;

/// Show-wide grade, attached to the stage camera.
#[derive(Component, ExtractComponent, ShaderType, Debug, Clone, Copy)]
pub struct Grade {
    /// Render target size in pixels.
    pub resolution: Vec2,
    pub time: f32,
    /// Overall output level, applied last.
    ///
    /// The one number to turn down when the visuals are competing with the room
    /// rather than supporting it. Individual visuals should not need retuning
    /// for a darker night; this should do it.
    pub exposure: f32,
    /// Master fade, `0.0..1.0`, driven by the operator and the autopilot.
    pub level: f32,
    /// CRT line structure, `0.0..1.0`.
    pub scanline: f32,
    /// Red/blue separation, in pixels at the frame edge.
    pub chroma: f32,
    /// Film grain amount.
    pub grain: f32,
    /// Corner falloff, `0.0..1.0`.
    pub vignette: f32,
    /// Horizontal tape-tracking wobble, in pixels.
    pub wobble: f32,
    /// Raises the black point.
    ///
    /// **Defaults to zero, and should usually stay there.** A trace of lift
    /// reads as film on a bright image, but these visuals are mostly black, and
    /// a lift larger than the picture itself flattens the whole frame into a
    /// uniform grey haze. A projector's black is already raised by the room;
    /// adding more here double-counts it and costs the only contrast available.
    pub lift: f32,
    /// Target output shape as width/height, e.g. `4.0 / 3.0`. Zero fills the
    /// window. Anything outside the shape is masked to black.
    pub aspect: f32,
    /// Tilt-shift blur radius in pixels at full strength. Zero disables it.
    ///
    /// A horizontal band stays sharp and everything above and below softens.
    /// It is the standard lens treatment for a scene viewed from above, and it
    /// costs nothing to fake because the mask is purely a function of screen
    /// position — no depth buffer required.
    pub tilt: f32,
    /// Height of the sharp band, in uv.
    pub tilt_focus: f32,
    /// Half-height of the fully sharp region, in uv.
    pub tilt_width: f32,
}

impl Default for Grade {
    /// Deliberately restrained. Every one of these is an artefact, and an
    /// artefact you *notice* has been turned up too far — the point is for the
    /// image to feel like it came off a tape, not to look like a filter.
    fn default() -> Self {
        Self {
            resolution: Vec2::new(1920.0, 1080.0),
            time: 0.0,
            exposure: 0.75,
            level: 1.0,
            scanline: 0.10,
            chroma: 1.1,
            grain: 0.045,
            vignette: 0.42,
            wobble: 0.6,
            lift: 0.0,
            aspect: 0.0,
            tilt: 0.0,
            tilt_focus: 0.55,
            tilt_width: 0.12,
        }
    }
}

impl Grade {
    /// 4:3, the shape of the screen this was built for.
    pub const FOUR_THREE: f32 = 4.0 / 3.0;
    pub const SIXTEEN_NINE: f32 = 16.0 / 9.0;

    /// No tape artefacts — just exposure, vignette and the aspect mask.
    ///
    /// Use when a visual should read as clean and modern rather than filmic.
    pub fn clean() -> Self {
        Self {
            scanline: 0.0,
            chroma: 0.0,
            grain: 0.0,
            wobble: 0.0,
            lift: 0.0,
            tilt: 0.0,
            ..Self::default()
        }
    }
}

impl FullscreenMaterial for Grade {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_core/shaders/grade.wgsl".into()
    }

    fn schedule() -> impl ScheduleLabel + Clone {
        Core2d
    }

    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        // After tonemapping, not before: these are display artefacts.
        system.in_set(Core2dSystems::PostProcess).after(tonemapping)
    }
}

pub struct GradePlugin;

impl Plugin for GradePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FullscreenMaterialPlugin::<Grade>::default())
            .add_systems(Update, update_grade);
    }
}

/// Feeds the clock and the master level into the grade each frame.
fn update_grade(
    clock: Res<ShowClock>,
    output: Res<ShowOutput>,
    windows: Query<&Window>,
    mut grades: Query<&mut Grade>,
) {
    let resolution = windows
        .iter()
        .next()
        .map(|w| Vec2::new(w.width(), w.height()));

    for mut grade in &mut grades {
        if let Some(resolution) = resolution {
            grade.resolution = resolution;
        }
        grade.time = clock.elapsed as f32;
        grade.level = output.level();
    }
}
