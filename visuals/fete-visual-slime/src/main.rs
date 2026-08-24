//! Run Slime on its own.
//!
//! `cargo run -p fete-visual-slime --release`
//!
//! Release is worth it here: two million agents is a real workload, and the
//! debug profile spends its time in the CPU-side seeding rather than the GPU.

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_slime::SlimePlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · slime"))
        .add_plugins(SlimePlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(mut macros: ResMut<Macros>, mut modulation: ResMut<Modulation>) {
    macros.snap(0, 0.45); // display gain
    macros.snap(1, 0.28); // sensor angle — narrow, so filaments run long
    macros.snap(2, 0.35); // move speed
    macros.snap(3, 0.35); // decay
    macros.snap(4, 0.30); // sensor distance
    macros.snap(5, 0.45); // filament edges
    macros.snap(6, 0.40); // colour spread
    macros.snap(7, 0.55); // beat reactivity

    modulation.amount = 1.0;
    // Sensor distance on a very slow cycle. It sets the scale of the mesh, so
    // sweeping it makes the network continuously reorganise between fine lace
    // and broad arteries — the visual never settles into one image.
    modulation.patch(
        Modulator::new(
            4,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 64.0,
            },
        )
        .with_depth(0.55)
        .with_bias(0.18),
    );
}
