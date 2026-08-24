//! Run Kura on its own.
//!
//! `cargo run -p fete-visual-kura --release`
//!
//! Release matters here: fourteen hundred boids stepped six times a frame and
//! a hundred thousand vertices rebuilt on the CPU is real work, and the debug
//! profile spends all of it in the flocking loop.

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_kura::KuraPlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · kura"))
        .add_plugins(KuraPlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(
    mut stage: ResMut<StageSettings>,
    mut macros: ResMut<Macros>,
    mut modulation: ResMut<Modulation>,
) {
    // Kura is thousands of small bright objects on black, which is exactly the
    // content bloom flatters and exactly the content a wide bloom destroys: a
    // high scatter would fill the gaps between the discs and turn the field
    // into fog. Tight and moderate.
    stage.bloom = 0.20;
    stage.bloom_scatter = 0.26;
    // Lower exposure than the rest of the set. The frame is busy and mostly
    // lit, so the same exposure that suits a dark city reads as glare here.
    stage.grade.exposure = 0.52;
    // No tilt-shift: the depth in this image is the three flocks' brightness,
    // not distance up the frame, and a focus band would cut across it.
    stage.grade.tilt = 0.0;
    stage.grade.vignette = 0.45;

    macros.snap(0, 0.45); // brightness
    macros.snap(1, 0.50); // cohesion
    macros.snap(2, 0.45); // speed
    macros.snap(3, 0.45); // separation
    macros.snap(4, 0.55); // trail length
    macros.snap(5, 0.50); // lattice
    macros.snap(6, 0.50); // colour spread
    macros.snap(7, 0.45); // beat depth and ripple rate

    modulation.amount = 1.0;
    // Cohesion on a long cycle. It is the parameter that decides whether this
    // looks like one shoal or a scatter of pairs, and sweeping it slowly means
    // the flock is always somewhere between the two rather than settled.
    modulation.patch(
        Modulator::new(
            1,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 149.0,
            },
        )
        .with_depth(0.34)
        .with_bias(0.50),
    );
    // Separation on a period sharing no factor with it, so tight-and-close and
    // loose-and-far only coincide occasionally.
    modulation.patch(
        Modulator::new(
            3,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 97.0,
            },
        )
        .with_depth(0.26)
        .with_bias(0.45),
    );
}
