//! Operator HUD.
//!
//! Drawn on the show output rather than a second window, because a laptop
//! screen mirrored to a projector is the common case and a separate control
//! window would end up projected anyway. Press `F1` before the audience sees
//! it.
//!
//! Everything here is read-only: the HUD reports state, it never owns any.

use bevy::prelude::*;
use fete_core::prelude::*;

use crate::control::HudVisible;

/// Marker for the HUD text node.
#[derive(Component)]
pub struct HudText;

/// Width of the ASCII meters, in characters.
const METER_WIDTH: usize = 12;

pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Name::new("hud"),
        HudText,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(16.0),
            ..default()
        },
        Text::new(""),
        TextFont {
            // Sized in viewport height rather than pixels: the same HUD is read
            // on a 1600x900 preview window and on a 4K projector feed.
            font_size: FontSize::Vh(1.5),
            ..default()
        },
        // Slightly blue-tinted white: pure white picks up the bloom pass and
        // smears, which makes small text unreadable.
        TextColor(Color::srgb(0.72, 0.80, 0.88)),
    ));
}

pub fn update_hud(
    hud_visible: Res<HudVisible>,
    registry: Res<VisualRegistry>,
    current: Res<State<ActiveVisual>>,
    clock: Res<ShowClock>,
    macros: Res<Macros>,
    modulation: Res<Modulation>,
    morph: Res<PaletteMorph>,
    output: Res<ShowOutput>,
    audio: Res<Audio>,
    autopilot: Res<Autopilot>,
    transition: Res<Transition>,
    clock_for_hold: Res<ShowClock>,
    mut hud: Query<(&mut Text, &mut Node), With<HudText>>,
) {
    for (mut text, mut node) in &mut hud {
        node.display = if hud_visible.0 {
            Display::Flex
        } else {
            Display::None
        };
        if !hud_visible.0 {
            continue;
        }
        text.0 = render_hud(
            &registry,
            current.get(),
            &clock,
            &macros,
            &modulation,
            &morph,
            &output,
            &audio,
            &autopilot,
            &transition,
            clock_for_hold.elapsed,
        );
    }
}

#[expect(clippy::too_many_arguments, reason = "the HUD reports on everything")]
fn render_hud(
    registry: &VisualRegistry,
    current: &ActiveVisual,
    clock: &ShowClock,
    macros: &Macros,
    modulation: &Modulation,
    morph: &PaletteMorph,
    output: &ShowOutput,
    audio: &Audio,
    autopilot: &Autopilot,
    transition: &Transition,
    now: f64,
) -> String {
    let mut out = String::with_capacity(512);

    let (name, position) = match current.0 {
        Some(id) => {
            let index = registry.index_of(id).map(|i| i + 1).unwrap_or(0);
            let info = registry.info(id);
            (
                info.map(|i| i.name).unwrap_or(id),
                format!("{index}/{}", registry.len()),
            )
        }
        None => ("— blackout —", "-".to_string()),
    };

    out.push_str(&format!(
        "fete  ·  {name}  [{position}]{}\n",
        if autopilot.enabled {
            format!("  autopilot/{:.0}b", autopilot.visual_beats)
        } else {
            "  manual".to_string()
        }
    ));

    // The beat readout is the one thing that must be readable at a glance:
    // a filled marker on the beat is easier to check against the music than
    // a number.
    let beat_in_bar = (clock.beats as u32) % clock.beats_per_bar.max(1);
    let mut beats = String::new();
    for i in 0..clock.beats_per_bar {
        beats.push(if i == beat_in_bar { '#' } else { '·' });
        beats.push(' ');
    }
    out.push_str(&format!(
        "{:>5.1} bpm  {beats} {}\n",
        clock.bpm,
        if clock.running { "" } else { "(paused)" }
    ));

    out.push_str(&format!(
        "palette {}   master {}\n",
        morph.target_name(),
        meter(output.level())
    ));

    out.push_str(&format!(
        "audio   {} bass {} mid {} high\n",
        meter(audio.bass),
        meter(audio.mid),
        meter(audio.high)
    ));

    // Always a line, even at rest: one that appeared only during a transition
    // would make the block below it jump every time a visual changed.
    out.push_str(&format!(
        "bleed   {:<9} {}\n",
        if transition.enabled {
            transition.style().name()
        } else {
            "off"
        },
        if transition.active() {
            meter(transition.progress())
        } else {
            "—".to_string()
        }
    ));

    out.push_str(&format!(
        "\nmacros{}\n",
        if modulation.amount > 0.5 {
            ""
        } else {
            "  (modulation frozen)"
        }
    ));
    const LABELS: [&str; MACRO_COUNT] = ["Q/A", "W/S", "E/D", "R/F", "T/G", "Y/H", "U/J", "I/K"];
    for (index, label) in LABELS.iter().enumerate() {
        let patched = modulation.rows.iter().any(|row| row.target == index);
        let held = macros.held(index, now, autopilot.release_seconds);
        // `held` wins the label: it is the one that explains why a knob is or
        // is not moving on its own.
        let tag = if held {
            "  held"
        } else if patched {
            "  ~mod"
        } else if autopilot.enabled && autopilot.drift {
            "  auto"
        } else {
            ""
        };
        out.push_str(&format!(
            "  {index} {label}  {} {:.2}{tag}\n",
            meter(macros.get(index)),
            macros.get(index),
        ));
    }

    out.push_str(
        "\ntab visual · 1-9 select · space tap · [ ] bpm · p palette\n\
         b blackout · z/x master · m freeze · c autopilot\n\
         / hud · \\ fullscreen (esc exits) · . still\n",
    );

    out
}

/// A `0.0..1.0` value as a fixed-width ASCII bar.
fn meter(value: f32) -> String {
    let filled = (value.clamp(0.0, 1.0) * METER_WIDTH as f32).round() as usize;
    let mut bar = String::with_capacity(METER_WIDTH + 2);
    bar.push('[');
    for i in 0..METER_WIDTH {
        bar.push(if i < filled { '=' } else { ' ' });
    }
    bar.push(']');
    bar
}
