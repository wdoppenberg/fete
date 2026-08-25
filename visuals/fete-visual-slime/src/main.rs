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
    macros.snap(4, 0.50); // sensor distance (see the modulator below)
    macros.snap(5, 0.45); // filament edges
    macros.snap(6, 0.40); // colour spread
    macros.snap(7, 0.55); // beat reactivity

    modulation.amount = 1.0;
    // Sensor distance on a very slow cycle. It sets the scale of the mesh, so
    // sweeping it moves the network between fine lace and broad arteries.
    //
    // Centred much higher than it used to be — `bias` is the value the knob
    // sits at, and 0.18 with a depth of 0.55 spent most of the cycle clamped
    // against zero, i.e. at the finest mesh the simulation can make. That was
    // right when one network had the frame to itself. Two of them tile it at
    // the same pitch, and at the old bias the result was 36% black where the
    // single-species version had been 75%: too much of the frame lit to hold
    // contrast on a projector. Opening the pitch up puts coverage back where
    // it was without touching the dynamics.
    modulation.patch(
        Modulator::new(
            4,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 64.0,
            },
        )
        .with_depth(0.35)
        .with_bias(0.45),
    );
}
