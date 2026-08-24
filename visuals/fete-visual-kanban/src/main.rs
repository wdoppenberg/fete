//! Run Kanban on its own.
//!
//! `cargo run -p fete-visual-kanban`

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_kanban::KanbanPlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · kanban"))
        .add_plugins(KanbanPlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(mut macros: ResMut<Macros>, mut modulation: ResMut<Modulation>) {
    macros.snap(0, 0.35); // brightness — low, this plays in a dark room
    macros.snap(1, 0.38); // density
    macros.snap(2, 0.35); // flight speed
    macros.snap(3, 0.30); // warp
    macros.snap(4, 0.12); // melt, barely on
    macros.snap(5, 0.45); // scale
    macros.snap(6, 0.50); // colour spread
    macros.snap(7, 0.55); // beat depth

    modulation.amount = 1.0;
    // The glass thickens and thins over eight bars, so the field is never
    // wobbling at a rate anyone can lock onto.
    modulation.patch(
        Modulator::new(
            3,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 32.0,
            },
        )
        .with_depth(0.45)
        .with_bias(0.35),
    );
    // Melt on a deliberately unrelated period: the characters go from crisp to
    // liquid and back on a cycle that never lines up with the warp.
    modulation.patch(
        Modulator::new(
            4,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 52.0,
            },
        )
        .with_depth(0.35)
        .with_bias(0.20),
    );
}
