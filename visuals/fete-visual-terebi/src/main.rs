//! Run Terebi on its own.
//!
//! ```text
//! cargo run -p fete-visual-terebi
//! cargo run -p fete-visual-terebi -- --video ~/clips   # a folder of your own
//! cargo run -p fete-visual-terebi -- --no-video
//! ```
//!
//! Clips default to `./video`, which is where `tools/fetch-clips.sh` puts them.
//! Without any, the wall plays the nine synthesised channels alone — which is
//! the visual as it was authored, not a degraded version of it.

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_video::VideoPlugin;
use fete_visual_terebi::TerebiPlugin;

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let video = if args.iter().any(|arg| arg == "--no-video") {
        None
    } else {
        Some(
            args.iter()
                .position(|arg| arg == "--video")
                .and_then(|index| args.get(index + 1))
                .cloned()
                .unwrap_or_else(|| "video".to_string()),
        )
    };

    let mut app = show(ShowConfig::new("fete · terebi"));
    app.add_plugins(TerebiPlugin)
        .add_systems(Startup, default_patch);
    if let Some(dir) = video {
        app.add_plugins(VideoPlugin::from_dir(dir));
    }
    app.run()
}

fn default_patch(mut macros: ResMut<Macros>, mut modulation: ResMut<Modulation>) {
    macros.snap(0, 0.45); // brightness — a wall of lit rectangles adds up
    macros.snap(1, 0.50); // how many sets are on; the dark ones carry the black
    macros.snap(2, 0.26); // cut rate
    // Sync — the one time the wall deliberately shows one picture on every set.
    // Low: it is the best thing this visual does and it is also the only thing
    // that makes the sets stop being twenty different televisions, so it wants
    // to be an event rather than a texture. Turn it up on R.
    macros.snap(3, 0.22); // sync
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
    // up with the interference. Shallow, and biased low: this knob is the one
    // that decides how often the whole wall is cutting, and past the middle of
    // its range the sets stop holding a programme long enough to be watched.
    modulation.patch(
        Modulator::new(
            2,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 70.0,
            },
        )
        .with_depth(0.20)
        .with_bias(0.26),
    );
}
