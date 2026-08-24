//! Run Sprawl on its own. `cargo run -p fete-visual-sprawl --release`

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_sprawl::SprawlPlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · sprawl"))
        .add_plugins(SprawlPlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(
    mut macros: ResMut<Macros>,
    mut palette: ResMut<Palette>,
    mut morph: ResMut<PaletteMorph>,
    mut stage: ResMut<StageSettings>,
) {
    macros.snap(0, 0.45); // brightness
    macros.snap(1, 0.50); // how lit
    macros.snap(2, 0.30); // drift speed
    macros.snap(3, 0.42); // altitude
    macros.snap(4, 0.35); // look-down
    macros.snap(5, 0.45); // smog
    macros.snap(6, 0.55); // dispersion
    macros.snap(7, 0.45); // beat depth

    // Amber over cold slate. The whole reference is one hue plus its opposite.
    *palette = Palette::SMOG;
    morph.go_to(Palette::SMOG, Palette::index_of(Palette::SMOG).unwrap_or(0));

    stage.grade.exposure = 0.7;

    // Focus. On a shot looking down at a plane, screen height *is* distance —
    // the bottom of the frame is close and the horizon is far — so a
    // tilt-shift band is a depth-of-field for free, with no depth buffer
    // involved. The sharp band sits just below the horizon, where the city is
    // densest, and the soft near field both reads as a long lens and hides the
    // aliasing that a million sub-pixel windows would otherwise produce.
    stage.grade.tilt = 7.0;
    stage.grade.tilt_focus = 0.40;
    stage.grade.tilt_width = 0.09;

    stage.grade.scanline = 0.04;
    stage.grade.grain = 0.06;
    stage.grade.chroma = 1.6;
}
