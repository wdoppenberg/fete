//! Run Neon on its own. `cargo run -p fete-visual-neon --release`

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_neon::NeonPlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · neon"))
        .add_plugins(NeonPlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(mut macros: ResMut<Macros>, mut stage: ResMut<StageSettings>) {
    macros.snap(0, 0.45); // brightness
    macros.snap(1, 0.35); // lit windows — sparse
    macros.snap(2, 0.30); // drift speed
    macros.snap(3, 0.35); // altitude
    macros.snap(4, 0.45); // look-down angle
    macros.snap(5, 0.40); // haze
    macros.snap(6, 0.45); // colour spread
    macros.snap(7, 0.50); // beat depth

    stage.grade.exposure = 0.9;
    stage.grade.tilt = 5.0;
    stage.grade.tilt_focus = 0.56;
    stage.grade.tilt_width = 0.14;
}
