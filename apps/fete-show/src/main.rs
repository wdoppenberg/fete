//! The combined show — every visual in one app, running itself.
//!
//! This is what gets pointed at the projector. It expects to run unattended:
//! the autopilot cycles visuals and palettes and slowly wanders the parameter
//! space, so the screen never settles into one image over a long night. Keys
//! still work if someone walks up: `C` hands control back to them by switching
//! the autopilot off, and `V` pins the visual without stopping anything else.
//!
//! ```text
//! cargo run -p fete-show --release
//! cargo run -p fete-show --release -- --fullscreen --no-hud
//! cargo run -p fete-show --release -- --aspect 16:9        # a different screen
//! cargo run -p fete-show --release -- --start neon --manual
//! cargo run -p fete-show --release -- --start yama --no-rotate  # one visual, still alive
//! cargo run -p fete-show --release -- --quality low       # what the Pi renders
//! ```

use bevy::prelude::*;
use fete_app::prelude::*;
use fete_core::prelude::*;
use fete_input_panel::{PanelMap, PanelPlugin};
use fete_video::VideoPlugin;
use fete_visual_kanban::KanbanPlugin;
use fete_visual_kura::KuraPlugin;
use fete_visual_neon::NeonPlugin;
use fete_visual_slime::SlimePlugin;
use fete_visual_sprawl::SprawlPlugin;
use fete_visual_terebi::TerebiPlugin;
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
    if let Some(quality) = args.quality {
        config = config.with_quality(quality);
    }

    // `--panel-test` implies manual: with the autopilot cycling visuals and
    // drifting knobs underneath, half of what a tester sees is the show moving
    // on its own, and attributing a change to a button becomes guesswork.
    let manual = args.manual || args.panel_test;
    let rotate = args.rotate;

    let mut app = show(config);
    // Terebi's video channel, if there is anything to feed it. Added here
    // rather than inside `TerebiPlugin` because the clip directory is the
    // operator's choice on the night, and because a visual should not reach out
    // and decide what the app reads off the disk.
    if let Some(dir) = args.video {
        app.add_plugins(VideoPlugin::from_dir(dir));
    }
    // The button panel on the screen, if its receiver is plugged in. Absent,
    // unplugged or out of radio range are all the same thing to the show: it
    // runs as it always has.
    if let Some(port) = args.panel {
        let plugin = PanelPlugin::on_port(port);
        // Bring-up mapping: eight effects nobody could mistake for each other.
        app.add_plugins(if args.panel_test {
            plugin.with_map(PanelMap::distinct())
        } else {
            plugin
        });
    }

    app
        // The set. This order is what the autopilot cycles through and what the
        // digit keys select. Kanban sits away from the two city visuals so the
        // night never runs three Tokyo pieces back to back, and Yama — the one
        // landscape — sits between them as the break from the city. Terebi
        // follows it: the only visual shot indoors, and the only one where the
        // light in the frame is coming from objects in a room rather than from
        // a city, so it lands as a change of place rather than of subject.
        // Kura goes last, between Kanban and the wrap back to Sprawl: it is the
        // only visual with no horizon and no architecture in it, so it reads as
        // the room going abstract for a while before the city comes back.
        .add_plugins((
            SprawlPlugin,
            NeonPlugin,
            YamaPlugin,
            TerebiPlugin,
            SlimePlugin,
            KanbanPlugin,
            KuraPlugin,
        ))
        .add_systems(
            Startup,
            (opening_look, move |mut autopilot: ResMut<Autopilot>| {
                autopilot.enabled = !manual;
                autopilot.cycle_visuals = rotate;
            }),
        )
        .add_systems(Update, drift_focus)
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
    // The band does not hold still — see [`drift_focus`].
    stage.grade.tilt = 6.0;
    stage.grade.tilt_focus = TILT_FOCUS;
    stage.grade.tilt_width = TILT_WIDTH;

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

/// Where the sharp band sits, and how tall it is, before [`drift_focus`] moves
/// them. Slightly above the middle: the eye reads a focus band below centre as
/// ground and one above centre as sky, and just above is where a horizon sits.
const TILT_FOCUS: f32 = 0.44;
const TILT_WIDTH: f32 = 0.11;

/// Drifts the tilt-shift's focus band over the course of the night.
///
/// A lens focused at a fixed distance for six hours stops reading as a lens and
/// starts reading as a gradient painted on the output — the eye learns where
/// the sharp band is and then ignores it. Moving it slowly puts the cue back:
/// the frame reads as something being *looked at* rather than something being
/// displayed, and a visual whose structure drifts through the band is
/// alternately picked out and let go without either being an event.
///
/// Two sines on coprime beat periods, so the pair does not repeat inside a set,
/// and both are slow enough that nothing is ever caught moving. The height of
/// the band breathes on the longer of the two: widening it as it travels keeps
/// the *amount* of the frame in focus roughly constant, which is what stops the
/// drift reading as the picture going in and out of focus.
fn drift_focus(clock: Res<ShowClock>, mut stage: ResMut<StageSettings>) {
    let wander = |period_beats: f32, phase: f32| {
        ((clock.beats as f32 / period_beats + phase) * std::f32::consts::TAU).sin()
    };

    // At 128bpm these are roughly 45 and 70 seconds.
    stage.grade.tilt_focus = TILT_FOCUS + 0.09 * wander(97.0, 0.0);
    stage.grade.tilt_width = TILT_WIDTH + 0.025 * wander(149.0, 0.37);
}

struct Args {
    fullscreen: bool,
    no_hud: bool,
    manual: bool,
    /// Whether the autopilot moves off the visual it opens on.
    rotate: bool,
    bpm: f32,
    start: Option<String>,
    aspect: Option<f32>,
    /// `None` leaves the choice to `FETE_QUALITY` and then the adapter probe.
    quality: Option<Quality>,
    /// Directory of clips for Terebi's video channel. `None` disables it.
    video: Option<String>,
    /// Serial port the button panel's receiver is on. `None` disables it.
    panel: Option<String>,
    /// Use the bring-up button mapping rather than the show one.
    panel_test: bool,
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
                 --no-rotate      hold one visual; palette and knobs still move\n\
                 --aspect <r>     mask output to a shape, as `4:3` or `1.777`\n\
                 --bpm <n>        starting tempo (default 128)\n\
                 --start <id>     open on a named visual (sprawl, neon, yama, terebi, slime, kanban, kura)\n\
                 --quality <t>    high, medium or low (default: probe the GPU)\n\
                 --render-scale <f>  fraction of the window to render at, 0.25-1.0\n\
                 --video <dir>    clips for Terebi's televisions (default ./video)\n\
                 --no-video       leave the televisions on the synthesised channels\n\
                 --panel <port>   serial port of the button panel's receiver\n\
                 --panel-list     list serial ports and exit\n\
                 --panel-test     obvious effect per button, autopilot off; for bring-up\n"
            );
            std::process::exit(0);
        }

        // Finding the receiver in a dark venue is otherwise a guessing game.
        if has("--panel-list") {
            let ports = fete_input_panel::available();
            if ports.is_empty() {
                println!("no serial ports found");
            } else {
                for port in ports {
                    println!("{port}");
                }
            }
            std::process::exit(0);
        }

        Self {
            fullscreen: has("--fullscreen"),
            no_hud: has("--no-hud"),
            manual: has("--manual"),
            rotate: !has("--no-rotate"),
            bpm: value("--bpm").and_then(|v| v.parse().ok()).unwrap_or(128.0),
            start: value("--start"),
            aspect: value("--aspect").and_then(|v| parse_aspect(&v)),
            quality: parse_quality(value("--quality"), value("--render-scale")),
            // On by default, because `tools/fetch-clips.sh` writes here and
            // there is nothing to configure once it has. An absent directory
            // costs one log line — see `VideoPlugin`.
            video: if has("--no-video") {
                None
            } else {
                Some(value("--video").unwrap_or_else(|| "video".to_string()))
            },
            // Off unless asked for: an unrelated device on a guessed port is a
            // worse failure than no panel at all.
            panel: value("--panel"),
            panel_test: has("--panel-test"),
        }
    }
}

/// Builds the quality override, if either flag was given.
///
/// A `--render-scale` on its own is honoured against the default tier rather
/// than rejected: wanting a softer image without also cheapening every shader
/// is a reasonable thing to ask for on the night.
fn parse_quality(tier: Option<String>, scale: Option<String>) -> Option<Quality> {
    let tier = tier.map(|raw| {
        Tier::parse(&raw).unwrap_or_else(|| {
            eprintln!("--quality `{raw}` is not one of high, medium, low");
            std::process::exit(2);
        })
    });
    let scale = scale.map(|raw| {
        raw.trim().parse::<f32>().unwrap_or_else(|_| {
            eprintln!("--render-scale `{raw}` is not a number");
            std::process::exit(2);
        })
    });

    match (tier, scale) {
        (None, None) => None,
        (tier, scale) => {
            let mut quality = Quality::new(tier.unwrap_or_default());
            if let Some(scale) = scale {
                quality = quality.with_render_scale(scale);
            }
            Some(quality)
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
