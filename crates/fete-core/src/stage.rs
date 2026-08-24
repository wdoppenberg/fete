//! The camera rig every visual renders through.
//!
//! Centralising this is most of why visuals stay short. Shaders write
//! unbounded HDR values and let the rig do the work: bloom turns bright pixels
//! into the glow that makes projected visuals read across a dark room, and
//! tonemapping keeps the highlights from clipping to flat white. Without it,
//! every visual would have to fake its own glow and they would never match.

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode};
use bevy::prelude::*;

use crate::bleed::Bleed;
use crate::grade::Grade;

/// Marker for the show camera.
#[derive(Component, Debug)]
pub struct StageCamera;

/// How the rig is configured. Change at runtime and the camera follows.
#[derive(Resource, Debug, Clone)]
pub struct StageSettings {
    /// Bloom strength. The dominant control over how "lit" the room feels.
    pub bloom: f32,
    /// How far bloom spreads. Larger values read better from far away.
    pub bloom_scatter: f32,
    /// Highlight rolloff.
    pub tonemapping: Tonemapping,
    /// Colour behind everything. Rarely anything but black on a projector —
    /// a projector cannot display black darker than the room's ambient light,
    /// so anything else only reduces contrast.
    pub clear_color: Color,
    /// The show-wide grade. Its artistic fields are pushed onto the camera;
    /// `resolution`, `time` and `level` are owned by the grade itself.
    pub grade: Grade,
}

impl Default for StageSettings {
    fn default() -> Self {
        Self {
            // Restrained. Bloom is what makes a visual read across a dark
            // room, but it is also what makes one glare — and a screen behind
            // a DJ is lit scenery, not the act.
            bloom: 0.22,
            // Tight rather than wide. A high scatter smears the brightest
            // colour on screen across the whole frame as a coloured haze,
            // which fills in the black these visuals depend on.
            bloom_scatter: 0.32,
            // AgX is the obvious pick for photographic content but it
            // desaturates hard, and these visuals are nothing but saturated
            // neon on black — exactly what it flattens. TonyMcMapface rolls
            // the highlights off while holding hue.
            tonemapping: Tonemapping::TonyMcMapface,
            clear_color: Color::BLACK,
            grade: Grade::default(),
        }
    }
}

/// Spawns the show camera.
pub fn spawn_stage_camera(mut commands: Commands, settings: Res<StageSettings>) {
    commands.spawn((
        Name::new("stage camera"),
        StageCamera,
        Camera2d,
        // HDR is what makes bloom mean anything: without it, everything above
        // 1.0 is clamped before the bloom pass ever sees it.
        Hdr,
        settings.tonemapping,
        bloom_from(&settings),
        // Both post-process passes the rig owns. The bleed runs first, in HDR;
        // the grade runs last, after tonemapping.
        Bleed::default(),
        settings.grade,
    ));
}

/// Pushes [`StageSettings`] changes onto the live camera.
pub fn sync_stage_settings(
    settings: Res<StageSettings>,
    mut cameras: Query<(&mut Bloom, &mut Tonemapping, &mut Grade), With<StageCamera>>,
    mut clear_color: ResMut<ClearColor>,
) {
    if !settings.is_changed() {
        return;
    }

    clear_color.0 = settings.clear_color;
    for (mut bloom, mut tonemapping, mut grade) in &mut cameras {
        *bloom = bloom_from(&settings);
        *tonemapping = settings.tonemapping;

        // Only the artistic fields. `resolution`, `time` and `level` are
        // written every frame by the grade's own system, and copying the
        // template over them would stutter the clock and blow away the fade.
        let template = settings.grade;
        grade.exposure = template.exposure;
        grade.scanline = template.scanline;
        grade.chroma = template.chroma;
        grade.grain = template.grain;
        grade.vignette = template.vignette;
        grade.wobble = template.wobble;
        grade.lift = template.lift;
        grade.aspect = template.aspect;
    }
}

fn bloom_from(settings: &StageSettings) -> Bloom {
    Bloom {
        intensity: settings.bloom,
        low_frequency_boost: settings.bloom_scatter,
        composite_mode: BloomCompositeMode::Additive,
        ..Bloom::NATURAL
    }
}
