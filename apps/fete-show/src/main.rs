//! The combined show — every visual in one app, running itself.
//!
//! This is what gets pointed at the projector. It expects to run unattended:
//! the autopilot cycles visuals and palettes and slowly wanders the parameter
//! space, so the screen never settles into one image over a long night. Keys
//! still work if someone walks up, and `C` hands control back to them by
//! switching the autopilot off.
//!
//! ```text
//! cargo run -p fete-show --release
//! cargo run -p fete-show --release -- --fullscreen --no-hud
//! cargo run -p fete-show --release -- --aspect 16:9        # a different screen
//! cargo run -p fete-show --release -- --start neon --manual
//! ```

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_visual_kanban::KanbanPlugin;
use fete_visual_kura::KuraPlugin;
use fete_visual_neon::NeonPlugin;
use fete_visual_slime::SlimePlugin;
use fete_visual_sprawl::SprawlPlugin;
use fete_visual_yama::YamaPlugin;

fn main() -> AppExit {
    let args = Args::parse();

    let mut config = ShowConfig::new("fete").with_bpm(args.bpm);
    if args.fullscreen {
        // `Current`, not `Primary`: at a venue the window has usually already
        // been dragged onto the projector before this is toggled.
        config = config.fullscreen(MonitorSelection::Current);
    }
    if args.no_hud {
        config = config.without_hud();
    }
    if let Some(id) = &args.start {
        config = config.starting_with(id);
    }
    if let Some(aspect) = args.aspect {
        config = config.with_aspect(aspect);
    }

    let manual = args.manual;

    show(config)
        // The set. This order is what the autopilot cycles through and what the
        // digit keys select. Kanban sits away from the two city visuals so the
        // night never runs three Tokyo pieces back to back, and Yama — the one
        // landscape — sits between them as the break from the city. Kura goes
        // last, between Kanban and the wrap back to Sprawl: it is the only
        // visual with no horizon and no architecture in it, so it reads as the
        // room going abstract for a while before the city comes back.
        .add_plugins((
            SprawlPlugin,
            NeonPlugin,
            YamaPlugin,
            SlimePlugin,
            KanbanPlugin,
            KuraPlugin,
        ))
        .add_systems(
            Startup,
            (opening_look, move |mut autopilot: ResMut<Autopilot>| {
                autopilot.enabled = !manual
            }),
        )
        .run()
}

/// The look of the night.
///
/// Everything here is a deliberate choice against the room: the screen is
/// scenery behind a DJ, so it has to be present without asking to be watched.
/// That means dark, slow, and reacting at half-time.
fn opening_look(mut stage: ResMut<StageSettings>, mut macros: ResMut<Macros>) {
    // The single control over how loud the visuals are. Below 1.0 by default —
    // it is far easier to turn a show up on the night than to discover
    // mid-set that it has been overpowering the room for an hour.
    // Low. The screen is scenery behind an act, not a light source for the
    // room — if it is lighting faces, it is too bright whatever it looks like
    // on a laptop in the dark.
    stage.grade.exposure = 0.6;

    // Tape and CRT artefacts, all low. Individually none of these should be
    // noticeable; together they stop the image looking like it came out of a
    // computer, which is most of the period reference.
    stage.grade.scanline = 0.10;
    stage.grade.chroma = 1.2;
    stage.grade.grain = 0.05;
    stage.grade.wobble = 0.7;
    // No lift. See `Grade::lift` — on a mostly-black frame it reads as a grey
    // veil over the entire image rather than as film.
    stage.grade.lift = 0.0;
    stage.grade.vignette = 0.45;

    // Tilt-shift. Reads as a lens focused at one distance; on the aerial city
    // it is what makes the ground look far away rather than like a texture.
    stage.grade.tilt = 6.0;
    stage.grade.tilt_focus = 0.44;
    stage.grade.tilt_width = 0.11;

    // Bloom: enough glow to read across the room, not enough to glare. Scatter
    // deliberately low — a wide, low-frequency bloom takes the colour of the
    // brightest thing on screen and washes it across the whole frame, which
    // shows up as a coloured haze that fills the black and costs the contrast
    // the signs depend on.
    stage.bloom = 0.16;
    stage.bloom_scatter = 0.3;

    // Macros start neutral and the autopilot wanders from there. Deliberately
    // not a tuned patch, because each visual reads the same eight knobs
    // differently and a patch that suits one is wrong for the others.
    for index in 0..MACRO_COUNT {
        macros.snap(index, 0.5);
    }
    macros.snap(0, 0.42); // every visual maps knob 0 to brightness
}

struct Args {
    fullscreen: bool,
    no_hud: bool,
    manual: bool,
    bpm: f32,
    start: Option<String>,
    aspect: Option<f32>,
}

impl Args {
    /// Hand-rolled rather than pulling in a parser: a handful of flags does not
    /// justify the dependency or the compile time.
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let has = |name: &str| args.iter().any(|a| a == name);
        let value = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };

        if has("--help") || has("-h") {
            println!(
                "fete-show\n\
                 \n\
                 --fullscreen     start fullscreen on the current monitor\n\
                 --no-hud         hide the operator overlay\n\
                 --manual         do not run the autopilot\n\
                 --aspect <r>     mask output to a shape, as `4:3` or `1.777`\n\
                 --bpm <n>        starting tempo (default 128)\n\
                 --start <id>     open on a named visual (sprawl, neon, yama, slime, kanban, kura)\n"
            );
            std::process::exit(0);
        }

        Self {
            fullscreen: has("--fullscreen"),
            no_hud: has("--no-hud"),
            manual: has("--manual"),
            bpm: value("--bpm").and_then(|v| v.parse().ok()).unwrap_or(128.0),
            start: value("--start"),
            aspect: value("--aspect").and_then(|v| parse_aspect(&v)),
        }
    }
}

/// Accepts either `4:3` or a bare ratio like `1.777`.
fn parse_aspect(raw: &str) -> Option<f32> {
    if let Some((w, h)) = raw.split_once(':') {
        let w: f32 = w.trim().parse().ok()?;
        let h: f32 = h.trim().parse().ok()?;
        if h.abs() < f32::EPSILON {
            return None;
        }
        return Some(w / h);
    }
    raw.parse().ok()
}
