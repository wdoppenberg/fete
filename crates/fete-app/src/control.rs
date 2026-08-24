//! Live keyboard control.
//!
//! The layout assumes one hand on the visuals while the other is doing
//! something else, in a dark room, without looking down. That rules out
//! modifier chords for anything used mid-track, and it is why the macro knobs
//! sit on two adjacent QWERTY rows — they read as a fader bank by touch.
//!
//! Everything here writes to `fete-core` resources; nothing reaches into a
//! visual directly. Swapping this module for MIDI or OSC input needs no
//! changes anywhere else.

use bevy::prelude::*;
use bevy::window::{MonitorSelection, WindowMode};
use fete_core::prelude::*;

/// Which key raises and lowers each macro knob.
///
/// Top row raises, home row below it lowers: Q/A is knob 0, W/S is knob 1, and
/// so on across eight columns.
const MACRO_KEYS: [(KeyCode, KeyCode); MACRO_COUNT] = [
    (KeyCode::KeyQ, KeyCode::KeyA),
    (KeyCode::KeyW, KeyCode::KeyS),
    (KeyCode::KeyE, KeyCode::KeyD),
    (KeyCode::KeyR, KeyCode::KeyF),
    (KeyCode::KeyT, KeyCode::KeyG),
    (KeyCode::KeyY, KeyCode::KeyH),
    (KeyCode::KeyU, KeyCode::KeyJ),
    (KeyCode::KeyI, KeyCode::KeyK),
];

/// Digit keys for jumping straight to a visual.
const SELECT_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// How fast a held macro key travels the full range, in units per second.
const MACRO_RATE: f32 = 1.2;

/// Whether the operator HUD is drawn.
#[derive(Resource, Debug, Clone, Copy)]
pub struct HudVisible(pub bool);

impl Default for HudVisible {
    fn default() -> Self {
        Self(true)
    }
}

/// Blackout latch, so `B` toggles rather than needing a second key.
#[derive(Resource, Debug, Default)]
pub struct Blackout {
    pub active: bool,
    /// Visual to restore when blackout is released.
    previous: Option<VisualId>,
}

/// Cycles visuals, taps tempo, and drives the master fade.
pub fn handle_show_keys(
    keys: Res<ButtonInput<KeyCode>>,
    registry: Res<VisualRegistry>,
    current: Res<State<ActiveVisual>>,
    mut requests: MessageWriter<VisualRequest>,
    mut clock: ResMut<ShowClock>,
    mut blackout: ResMut<Blackout>,
    mut hud: ResMut<HudVisible>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        let backwards = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        requests.write(VisualRequest::Cycle(if backwards { -1 } else { 1 }));
    }

    for (index, key) in SELECT_KEYS.iter().enumerate() {
        if keys.just_pressed(*key)
            && let Some(info) = registry.get(index)
        {
            requests.write(VisualRequest::Show(info.id));
        }
    }

    if keys.just_pressed(KeyCode::KeyB) {
        blackout.active = !blackout.active;
        if blackout.active {
            blackout.previous = current.get().0;
            requests.write(VisualRequest::Blackout);
        } else if let Some(id) = blackout.previous.take() {
            requests.write(VisualRequest::Show(id));
        } else {
            requests.write(VisualRequest::Cycle(0));
        }
    }

    // Tempo. Tapping also realigns the phase, so a tap on the downbeat is the
    // one gesture that fixes both "wrong tempo" and "right tempo, wrong phase".
    if keys.just_pressed(KeyCode::Space) {
        clock.tap();
    }
    if keys.just_pressed(KeyCode::Enter) {
        clock.resync();
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        clock.bpm = (clock.bpm - 0.5).max(20.0);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        clock.bpm = (clock.bpm + 0.5).min(300.0);
    }

    // `Slash` as well as `F1`, and likewise for fullscreen and stills below.
    // On macOS the function keys are media keys unless the user has changed a
    // system setting, so an F-key-only binding is unreachable on the average
    // laptop.
    if keys.just_pressed(KeyCode::F1) || keys.just_pressed(KeyCode::Slash) {
        hud.0 = !hud.0;
    }
}

/// Macro knobs and the master fade.
pub fn handle_macro_keys(
    keys: Res<ButtonInput<KeyCode>>,
    clock: Res<ShowClock>,
    mut macros: ResMut<Macros>,
    mut modulation: ResMut<Modulation>,
    mut output: ResMut<ShowOutput>,
) {
    let step = MACRO_RATE * clock.delta;

    for (index, (up, down)) in MACRO_KEYS.iter().enumerate() {
        let up_held = keys.pressed(*up);
        let down_held = keys.pressed(*down);
        if !up_held && !down_held {
            continue;
        }
        if up_held {
            macros.nudge(index, step);
        }
        if down_held {
            macros.nudge(index, -step);
        }
        // Claim the knob. Automation checks this and backs off; without it the
        // autopilot rewrites the value on the very next frame and the key
        // appears to do nothing at all.
        macros.touch(index, clock.elapsed);
    }

    // Master fade. Deliberately not instant — a hard cut to black mid-set is
    // more jarring than a fast fade, and the ramp is still under a second.
    if keys.pressed(KeyCode::KeyX) {
        output.master = (output.master + step).min(1.0);
    }
    if keys.pressed(KeyCode::KeyZ) {
        output.master = (output.master - step).max(0.0);
    }

    // Freeze: drop all modulation depth so the picture holds still. Useful
    // when the track breaks down and the visual should stop moving with it.
    if keys.just_pressed(KeyCode::KeyM) {
        modulation.amount = if modulation.amount > 0.5 { 0.0 } else { 1.0 };
    }
}

/// Toggles the autopilot.
///
/// The show is expected to run unattended, so the autopilot is on by default
/// and this exists to *stop* it — someone who walks up and starts turning
/// knobs does not want an automatic visual change thirty seconds later.
pub fn handle_autopilot_key(
    keys: Res<ButtonInput<KeyCode>>,
    clock: Res<ShowClock>,
    mut autopilot: ResMut<Autopilot>,
) {
    if !keys.just_pressed(KeyCode::KeyC) {
        return;
    }
    autopilot.enabled = !autopilot.enabled;
    if autopilot.enabled {
        // Restart the timers from now, so re-enabling does not immediately
        // fire a change that was due while it was off.
        autopilot.restart(&clock);
    }
    info!("autopilot {}", if autopilot.enabled { "on" } else { "off" });
}

/// Palette selection.
pub fn handle_palette_keys(
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<Palette>,
    mut morph: ResMut<PaletteMorph>,
) {
    if keys.just_pressed(KeyCode::KeyP) {
        morph.next(*palette);
    }
}

/// Fullscreen toggle.
///
/// `MonitorSelection::Current` rather than `Primary`: at a venue the show
/// window has usually already been dragged onto the projector, and forcing it
/// back to the primary display is exactly the wrong move.
pub fn handle_window_keys(keys: Res<ButtonInput<KeyCode>>, mut windows: Query<&mut Window>) {
    let toggle = keys.just_pressed(KeyCode::F11) || keys.just_pressed(KeyCode::Backslash);
    // Escape only ever leaves fullscreen. Panic button: whatever else is going
    // on, that gets the window back.
    let leave = keys.just_pressed(KeyCode::Escape);

    if !toggle && !leave {
        return;
    }

    for mut window in &mut windows {
        window.mode = match window.mode {
            _ if leave => WindowMode::Windowed,
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
}
