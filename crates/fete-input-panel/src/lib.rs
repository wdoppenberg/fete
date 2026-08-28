//! The hardware button panel: buttons on the screen, a receiver on the laptop.
//!
//! Two ESP32-S3 boards. One is mounted on the screen with the buttons wired to
//! it and talks ESP-NOW; the other sits behind the booth on the laptop's USB
//! and repeats what it hears down a serial port. This crate is the last hop —
//! it turns those lines into the same knob movements and visual changes the
//! keyboard makes.
//!
//! ```no_run
//! use bevy::prelude::*;
//! # use fete_input_panel::PanelPlugin;
//! # let mut app = App::new();
//! app.add_plugins(PanelPlugin::on_port("/dev/tty.usbmodem101"));
//! ```
//!
//! Like `fete-app`'s keyboard module, nothing here reaches into a visual. It
//! writes [`Macros`] and sends [`VisualRequest`]s, which is why a button and a
//! key can do the same job without either knowing the other exists.
//!
//! # Three rules this follows
//!
//! **The panel is optional.** A missing port, an unplugged cable, a flat
//! battery on the screen: each of those is a show that carries on exactly as
//! it does today. Nothing here can fail in a way that stops the night.
//!
//! **Buttons let go.** A held button pushes a knob and *claims* it, which is
//! what stops the autopilot immediately overwriting the value. Released, the
//! claim lapses and the autopilot drifts the knob back on its own. Nobody has
//! to undo anything, which matters when the people pressing these are dancing
//! rather than operating.
//!
//! **Nothing unbounded.** Every action here moves a knob within its range or
//! changes which visual is up. There is deliberately no way to wind the output
//! past what the grade expects — a button held down for an hour by someone
//! leaning on the screen should look like a choice, not a fault.

mod link;
mod protocol;

pub use link::{LinkStatus, Mailbox, available, spawn as spawn_reader};
pub use protocol::{Frame, ParseError, parse_line};

use bevy::prelude::*;
use fete_core::prelude::*;

/// How fast a held button travels a knob's full range, in units per second.
///
/// Slower than the keyboard's, because a button is a blunter instrument than a
/// key someone is watching the screen while holding.
const TRAVEL_RATE: f32 = 0.8;

/// Silence from the receiver longer than this counts as a dead link.
const LINK_TIMEOUT_SECS: f64 = 1.5;

/// The receiver not hearing the panel for longer than this counts the same way.
const PANEL_TIMEOUT_MS: u32 = 1_500;

/// What one button does.
///
/// Deliberately a small set. Every variant is bounded: the worst a jammed
/// button can do is hold one knob at one end of its range, or sit on a visual.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelAction {
    /// While held, drive a macro knob toward `target`; on release, hand it back
    /// to the autopilot.
    Hold { macro_index: usize, target: f32 },
    /// One step of `delta` on each press, then let go again.
    Nudge { macro_index: usize, delta: f32 },
    /// Jump to the visual at this position in the registry.
    Select(usize),
    /// Step forwards or backwards through the visuals.
    Cycle(isize),
    /// Tap tempo. Tapping on the downbeat fixes phase as well as rate.
    Tap,
    /// Move the palette on to the next one.
    NextPalette,
    /// Re-show the current visual, which regenerates its seed and reshuffles
    /// everything derived from it.
    Reseed,
    /// Stop and start the modulation, so the picture holds still.
    ///
    /// Only bites when something has patched modulators: `amount` scales the
    /// modulation matrix, and an app that has patched none of them has nothing
    /// for this to scale. Nothing in this workspace patches any yet, so on
    /// `fete-show` as it stands this changes the HUD and nothing else.
    Freeze,
    /// Cut to black. Any button that selects a visual brings the show back.
    Blackout,
    /// Wired but unassigned.
    Unmapped,
}

/// Which action each button on the panel performs.
///
/// Index is the bit position in the frame's button mask, so button 0 is the
/// first bit. Buttons beyond the end of this list do nothing.
#[derive(Resource, Debug, Clone)]
pub struct PanelMap(pub Vec<PanelAction>);

impl Default for PanelMap {
    /// Ten buttons: step the visual backwards or forwards, then jump directly
    /// to each of the eight visuals.
    ///
    /// The navigation buttons come first because they are the ones whose effect
    /// is obvious to somebody who has never seen the panel — press it, the
    /// picture changes. The direct-select buttons follow the visual registry's
    /// order.
    fn default() -> Self {
        let mut actions = vec![PanelAction::Cycle(-1), PanelAction::Cycle(1)];
        actions.extend((0..8).map(PanelAction::Select));
        Self(actions)
    }
}

impl PanelMap {
    /// A mapping where every button does something unmistakable.
    ///
    /// The production default leans on the macro knobs, which is right for a
    /// night but wrong for a bring-up: knobs are subtle by design, and several
    /// of them look similar on the wrong visual. This one trades that for ten
    /// effects nobody could confuse, so a press either visibly happened or the
    /// link is broken. `--panel-test` selects it.
    pub fn distinct() -> Self {
        // Each button selects one named visual, then the three that are left
        // do the other things that show up plainly on a still picture.
        //
        // Nothing subtler survives a bring-up: the first version of this mapped
        // buttons to a palette morph, a re-seed, a freeze and three knob holds,
        // and on a dark, slow visual with the autopilot also moving things,
        // half of them were indistinguishable from the show doing its own
        // thing. "Press 3, get yama, every time" is a test; "press 4 and the
        // colours drift over the next two seconds" is not.
        //
        // Blackout is deliberately absent. It is the one effect nobody wants to
        // trigger by accident in front of an audience.
        let mut actions: Vec<PanelAction> = (0..7).map(PanelAction::Select).collect();
        actions.push(PanelAction::NextPalette);
        actions.push(PanelAction::Reseed);
        actions.push(PanelAction::Hold {
            macro_index: 0,
            target: 1.0,
        });
        Self(actions)
    }
}

/// Live state of the link and the buttons.
#[derive(Resource, Debug, Default)]
pub struct PanelState {
    /// Buttons currently held, one bit each.
    pub buttons: u32,
    /// Buttons held when the show last looked.
    previous: u32,
    /// Show-clock time the last usable frame arrived.
    last_frame: Option<f64>,
    /// Whether the panel is currently considered reachable.
    pub connected: bool,
    /// ESP-NOW packets the transmitter sequence says went missing, since startup.
    pub dropped: u64,
    /// Last sequence number seen.
    last_seq: Option<u16>,
}

impl PanelState {
    /// Buttons that went down since the previous frame.
    fn pressed(&self) -> u32 {
        self.buttons & !self.previous
    }
}

/// Reads a button panel on a serial port.
pub struct PanelPlugin {
    port: String,
    map: PanelMap,
}

impl PanelPlugin {
    /// Read the panel on `port`, with the default button mapping.
    pub fn on_port(port: impl Into<String>) -> Self {
        Self {
            port: port.into(),
            map: PanelMap::default(),
        }
    }

    /// Use a different mapping.
    pub fn with_map(mut self, map: PanelMap) -> Self {
        self.map = map;
        self
    }
}

impl Plugin for PanelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(link::spawn(self.port.clone()))
            .insert_resource(self.map.clone())
            .init_resource::<PanelState>()
            .add_systems(Update, (receive_frames, apply_buttons).chain());
    }
}

/// Drain the mailbox into [`PanelState`], and notice when the link goes quiet.
fn receive_frames(mailbox: Res<Mailbox>, clock: Res<ShowClock>, mut state: ResMut<PanelState>) {
    state.previous = state.buttons;

    for frame in mailbox.drain() {
        let panel_fresh = frame.panel_age_ms <= PANEL_TIMEOUT_MS;
        if panel_fresh {
            if let Some(last) = state.last_seq {
                // Wrapping arithmetic: the counter rolls over every 65536
                // packets, which at 50 Hz is about twenty minutes into a set.
                let gap = frame.seq.wrapping_sub(last).saturating_sub(1);
                state.dropped += u64::from(gap);
            }
            state.last_seq = Some(frame.seq);
        } else {
            // Sequence zero is emitted before the receiver has ever heard the
            // panel. Forget the baseline across any outage so the first packet
            // back is not mistaken for a giant gap.
            state.last_seq = None;
        }
        state.last_frame = Some(clock.elapsed);

        // The receiver is talking, but it may be repeating a panel that has
        // gone quiet. Treat a stale panel as no buttons held rather than as
        // whatever was held when it disappeared.
        state.buttons = if panel_fresh { frame.buttons } else { 0 };
    }

    let alive = state
        .last_frame
        .is_some_and(|seen| clock.elapsed - seen < LINK_TIMEOUT_SECS);

    if alive != state.connected {
        if alive {
            info!("panel: connected");
        } else {
            // Everything lets go. The autopilot has the knobs back within its
            // own hold window without anything further from here.
            warn!("panel: link lost — returning to the autopilot");
            state.buttons = 0;
        }
        state.connected = alive;
    }
}

/// Turn held and pressed buttons into knob movement and visual changes.
fn apply_buttons(
    map: Res<PanelMap>,
    state: Res<PanelState>,
    registry: Res<VisualRegistry>,
    palette: Res<Palette>,
    mut morph: ResMut<PaletteMorph>,
    mut modulation: ResMut<Modulation>,
    mut autopilot: ResMut<Autopilot>,
    mut output: ResMut<ShowOutput>,
    mut macros: ResMut<Macros>,
    mut requests: MessageWriter<VisualRequest>,
    // Mutable because tap tempo writes to it; everything else only reads.
    mut clock: ResMut<ShowClock>,
) {
    let elapsed = clock.elapsed;
    let step = TRAVEL_RATE * clock.delta;
    let pressed = state.pressed();

    for (button, action) in map.0.iter().enumerate() {
        let bit = 1u32 << (button.min(31));
        let held = state.buttons & bit != 0;
        let just_pressed = pressed & bit != 0;

        // One line per press, naming what it did. During bring-up this is the
        // difference between "the link is dead" and "the link is fine and this
        // button is mapped to something you cannot see on this visual".
        if just_pressed {
            info!("panel: button {button} -> {action:?}");
        }

        match *action {
            PanelAction::Hold {
                macro_index,
                target,
            } if held => {
                let current = macros.get(macro_index);
                let next = if target > current {
                    (current + step).min(target)
                } else {
                    (current - step).max(target)
                };
                macros.set(macro_index, next);
                // Claim it. Without this the autopilot rewrites the knob on the
                // next frame and the button appears dead.
                macros.touch(macro_index, elapsed);
            }
            PanelAction::Nudge { macro_index, delta } if just_pressed => {
                macros.nudge(macro_index, delta);
                macros.touch(macro_index, elapsed);
            }
            PanelAction::Select(index) if just_pressed => {
                if let Some(info) = registry.get(index) {
                    requests.write(VisualRequest::Show(info.id));
                    // Give a person's choice a complete hold instead of
                    // allowing an older automation deadline to replace it.
                    autopilot.restart_visuals(&clock);
                }
            }
            PanelAction::Cycle(by) if just_pressed => {
                requests.write(VisualRequest::Cycle(by));
                autopilot.restart_visuals(&clock);
            }
            PanelAction::Tap if just_pressed => clock.tap(),
            PanelAction::NextPalette if just_pressed => morph.next(*palette),
            PanelAction::Reseed if just_pressed => {
                // Writing the seed directly, because asking for the visual that
                // is already up does nothing: `apply_visual_requests` skips a
                // request whose target it is already showing, so routing this
                // through a `Show` would silently do nothing at all.
                output.seed = (clock.elapsed * 1000.0).fract() as f32;
            }
            PanelAction::Blackout if just_pressed => {
                requests.write(VisualRequest::Blackout);
            }
            PanelAction::Freeze if just_pressed => {
                modulation.amount = if modulation.amount > 0.5 { 0.0 } else { 1.0 };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with just enough in it to run `receive_frames`.
    fn harness() -> App {
        let mut app = App::new();
        app.init_resource::<Mailbox>()
            .init_resource::<ShowClock>()
            .init_resource::<PanelState>()
            .add_systems(Update, receive_frames);
        app
    }

    /// Advance the show clock without running the real clock system.
    fn tick(app: &mut App, seconds: f64) {
        app.world_mut().resource_mut::<ShowClock>().elapsed += seconds;
        app.update();
    }

    fn send(app: &App, buttons: u32, seq: u16, panel_age_ms: u32) {
        app.world().resource::<Mailbox>().push(Frame {
            buttons,
            seq,
            panel_age_ms,
        });
    }

    #[test]
    fn default_map_has_navigation_and_all_eight_visuals() {
        assert_eq!(
            PanelMap::default().0,
            vec![
                PanelAction::Cycle(-1),
                PanelAction::Cycle(1),
                PanelAction::Select(0),
                PanelAction::Select(1),
                PanelAction::Select(2),
                PanelAction::Select(3),
                PanelAction::Select(4),
                PanelAction::Select(5),
                PanelAction::Select(6),
                PanelAction::Select(7),
            ]
        );
    }

    #[test]
    fn a_frame_arrives_and_the_link_comes_up() {
        let mut app = harness();
        send(&app, 0b101, 1, 5);
        tick(&mut app, 0.016);

        let state = app.world().resource::<PanelState>();
        assert_eq!(state.buttons, 0b101);
        assert!(state.connected);
    }

    #[test]
    fn silence_releases_every_button() {
        let mut app = harness();
        send(&app, 0b1111, 1, 5);
        tick(&mut app, 0.016);
        assert!(app.world().resource::<PanelState>().connected);

        // Nothing further arrives. A panel that vanished mid-press must not
        // leave the show holding those knobs for the rest of the night.
        tick(&mut app, LINK_TIMEOUT_SECS + 0.1);

        let state = app.world().resource::<PanelState>();
        assert!(!state.connected, "the link should have timed out");
        assert_eq!(state.buttons, 0, "a lost link releases everything");
    }

    #[test]
    fn a_receiver_repeating_a_dead_panel_is_not_trusted() {
        let mut app = harness();
        // The receiver is fine — frames keep coming — but it has not heard the
        // panel in longer than the panel timeout.
        send(&app, 0b1111, 1, PANEL_TIMEOUT_MS + 1);
        tick(&mut app, 0.016);

        let state = app.world().resource::<PanelState>();
        assert!(state.connected, "the receiver itself is still talking");
        assert_eq!(state.buttons, 0, "but its button state is stale");
    }

    #[test]
    fn gaps_in_the_sequence_are_counted() {
        let mut app = harness();
        send(&app, 0, 1, 0);
        send(&app, 0, 5, 0);
        tick(&mut app, 0.016);
        assert_eq!(app.world().resource::<PanelState>().dropped, 3);
    }

    #[test]
    fn repeated_radio_sequence_is_not_a_drop() {
        let mut app = harness();
        send(&app, 0, 7, 0);
        send(&app, 0, 7, 20);
        tick(&mut app, 0.016);
        assert_eq!(app.world().resource::<PanelState>().dropped, 0);
    }

    #[test]
    fn first_packet_after_radio_outage_starts_a_new_sequence_baseline() {
        let mut app = harness();
        send(&app, 0, 0, PANEL_TIMEOUT_MS + 1);
        send(&app, 0, 500, 0);
        tick(&mut app, 0.016);
        assert_eq!(app.world().resource::<PanelState>().dropped, 0);
    }

    #[test]
    fn the_sequence_counter_may_wrap_without_counting_65000_drops() {
        let mut app = harness();
        send(&app, 0, u16::MAX, 0);
        send(&app, 0, 1, 0);
        tick(&mut app, 0.016);
        assert_eq!(
            app.world().resource::<PanelState>().dropped,
            1,
            "wrapping past 65535 is one dropped frame, not a whole counter's worth"
        );
    }

    #[test]
    fn a_press_is_an_edge_not_a_level() {
        let mut app = harness();
        send(&app, 0b1, 1, 0);
        tick(&mut app, 0.016);
        assert_eq!(app.world().resource::<PanelState>().pressed(), 0b1);

        // Still held on the next frame, but no longer a new press.
        send(&app, 0b1, 2, 0);
        tick(&mut app, 0.016);
        assert_eq!(app.world().resource::<PanelState>().pressed(), 0);
    }
}
