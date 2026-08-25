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
        // Off, explicitly. `Msaa` is a required component of `Camera` and
        // defaults to `Sample4`, so leaving it out does not mean "no MSAA" —
        // it means four samples, silently. A visual is one fullscreen quad
        // with no geometric edges anywhere in it, so every one of those
        // samples resolves to the same value.
        //
        // On a desktop GPU that is merely wasteful. On a tile-based mobile
        // GPU it is worse than wasteful: four samples of `Rgba16Float` is
        // 32 bytes per pixel of tile storage, which forces far smaller tiles
        // and many more flushes to memory. Kura draws real geometry and would
        // in principle like the antialiasing, but it draws soft-edged blended
        // sprites that are already antialiased by their own falloff.
        Msaa::Off,
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
        // The tilt-shift, which used to be missing here. The camera is spawned
        // from `StageSettings` in `Startup` and an app's own look is applied in
        // `Startup` too, so whether the tilt survived came down to which of the
        // two the scheduler happened to run first — and when it lost, the
        // effect was simply absent for the whole show with nothing to say so.
        // Copying it makes the resource the single source of truth, and lets
        // the focus band be moved at runtime.
        grade.tilt = template.tilt;
        grade.tilt_focus = template.tilt_focus;
        grade.tilt_width = template.tilt_width;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An app holding just what `sync_stage_settings` reads and writes.
    fn harness() -> App {
        let mut app = App::new();
        app.init_resource::<StageSettings>()
            .init_resource::<ClearColor>()
            .add_systems(Update, sync_stage_settings);
        app.world_mut().spawn((
            StageCamera,
            Bloom::default(),
            Tonemapping::TonyMcMapface,
            Grade::default(),
        ));
        app
    }

    #[test]
    fn tilt_reaches_the_camera() {
        // The regression: the camera is spawned from `StageSettings` during
        // `Startup` and an app's own look is applied during `Startup` too, so
        // an app that lost the ordering race got a camera with `tilt` still at
        // its default of zero — and since the sync did not copy the tilt
        // fields, nothing ever corrected it. The tilt-shift was simply absent.
        let mut app = harness();
        {
            let mut settings = app.world_mut().resource_mut::<StageSettings>();
            settings.grade.tilt = 6.0;
            settings.grade.tilt_focus = 0.44;
            settings.grade.tilt_width = 0.11;
        }
        app.update();

        let mut cameras = app.world_mut().query::<&Grade>();
        let grade = cameras.iter(app.world()).next().expect("no stage camera");
        assert_eq!(grade.tilt, 6.0, "tilt did not reach the camera");
        assert_eq!(grade.tilt_focus, 0.44);
        assert_eq!(grade.tilt_width, 0.11);
    }

    #[test]
    fn moving_the_focus_band_at_runtime_is_picked_up() {
        // What `drift_focus` in the show depends on: writing the resource after
        // startup has to reach the live camera, or the band cannot be animated.
        let mut app = harness();
        app.update();
        app.world_mut()
            .resource_mut::<StageSettings>()
            .grade
            .tilt_focus = 0.61;
        app.update();

        let mut cameras = app.world_mut().query::<&Grade>();
        let grade = cameras.iter(app.world()).next().expect("no stage camera");
        assert_eq!(grade.tilt_focus, 0.61);
    }
}
