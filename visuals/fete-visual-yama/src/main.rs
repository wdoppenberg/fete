//! Run Yama on its own.
//!
//! `cargo run -p fete-visual-yama --release`

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_yama::YamaPlugin;

fn main() -> AppExit {
    show(ShowConfig::new("fete · yama"))
        .add_plugins(YamaPlugin)
        .add_systems(Startup, default_patch)
        .run()
}

fn default_patch(
    mut stage: ResMut<StageSettings>,
    mut macros: ResMut<Macros>,
    mut modulation: ResMut<Modulation>,
) {
    // A landscape has a lot more lit area than a city at night — sky rather
    // than black between the lights — so it is exposed lower than the rest of
    // the set and leans harder on the vignette to hold the frame together.
    stage.grade.exposure = 0.55;
    stage.grade.vignette = 0.5;
    // Tilt-shift off. On the aerial visuals the screen height is distance and
    // the blur reads as depth of field; on a landscape the sharp band would cut
    // straight across the mountain.
    stage.grade.tilt = 0.0;
    // Bloom low. The bright region here is a continuous band of sky, and a wide
    // bloom over a broad source is what turns dusk into fog.
    stage.bloom = 0.14;
    stage.bloom_scatter = 0.28;

    macros.snap(0, 0.40); // brightness
    macros.snap(1, 0.46); // cloud cover
    macros.snap(2, 0.28); // orbit speed — slow
    macros.snap(3, 0.22); // altitude — near the shore
    macros.snap(4, 0.42); // the hour — sun just above the horizon
    macros.snap(5, 0.45); // haze
    macros.snap(6, 0.55); // snow line
    macros.snap(7, 0.50); // beat depth

    modulation.amount = 1.0;
    // The hour walks slowly back and forth across sunset over sixty-four bars,
    // so the mountain is lit, then only its summit is, then it is a silhouette,
    // and back. It is the slowest thing in the show and the only one anybody
    // will consciously notice.
    modulation.patch(
        Modulator::new(
            4,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 256.0,
            },
        )
        .with_depth(0.40)
        .with_bias(0.45),
    );
    // Cloud cover on a period that shares no factor with the hour, so thick
    // cloud and deep dusk arrive together only occasionally.
    modulation.patch(
        Modulator::new(
            1,
            ModSource::Synced {
                wave: Wave::Sine,
                beats: 181.0,
            },
        )
        .with_depth(0.28)
        .with_bias(0.46),
    );
}
