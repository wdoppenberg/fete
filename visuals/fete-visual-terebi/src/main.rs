//! Run Terebi on its own.
//!
//! `cargo run -p fete-visual-terebi`

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_terebi::TerebiPlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · terebi"))
        .add_plugins(TerebiPlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(mut macros: ResMut<Macros>, mut modulation: ResMut<Modulation>) {
    macros.snap(0, 0.40); // brightness — a wall of lit rectangles adds up
    macros.snap(1, 0.70); // how many sets are on; the dark ones carry the black
    macros.snap(2, 0.30); // cut rate
    macros.snap(3, 0.35); // sync
    macros.snap(4, 0.18); // interference, barely on
    macros.snap(5, 0.45); // set size
    macros.snap(6, 0.55); // colour spread
    macros.snap(7, 0.55); // beat depth

    modulation.amount = 1.0;
    // The tape wears out and recovers over twelve bars. Nothing on the wall
    // ever settles into being clean.
    modulation.patch(
        Modulator::new(
            4,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 48.0,
            },
        )
        .with_depth(0.30)
        .with_bias(0.16),
    );
    // Channels churn faster and slower on a deliberately unrelated period, so
    // the wall goes from restless to nearly still and back without ever lining
    // up with the interference.
    modulation.patch(
        Modulator::new(
            2,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 70.0,
            },
        )
        .with_depth(0.35)
        .with_bias(0.32),
    );
}
