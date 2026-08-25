//! Running the show with nobody at the keyboard.
//!
//! An unattended show has one failure mode that matters: becoming boring. Not
//! crashing, not looking wrong — just settling into a state and staying there,
//! so that by the third hour it is wallpaper nobody has looked at since
//! midnight. Everything here exists to prevent that while staying inside the
//! brief of *not* demanding attention.
//!
//! Three mechanisms, on deliberately different timescales:
//!
//! - **Visual changes** every few dozen bars, through a bleed transition.
//!   Rotation can be held ([`Autopilot::cycle_visuals`]) without giving up the
//!   other two, for when one visual is right for the track that is playing.
//! - **Palette changes** on a different, non-multiple period, so the
//!   combination of visual and colour rarely repeats.
//! - **Continuous parameter drift** — a slow random walk of every macro knob
//!   nothing else is driving, so even a single visual held for ten minutes is
//!   never quite the same twice.
//!
//! All of it is phrased in beats and lands on bar boundaries, so changes read
//! as intentional rather than as a timer going off.

use bevy::prelude::*;

use crate::clock::ShowClock;
use crate::globals::ShowOutput;
use crate::palette::{Palette, PaletteMorph};
use crate::signal::{MACRO_COUNT, Macros, Modulation};
use crate::visual::VisualRequest;

/// Unattended show automation.
#[derive(Resource, Debug, Clone)]
pub struct Autopilot {
    pub enabled: bool,
    /// Beats a visual is held for. 128 is 32 bars, a minute at 128bpm.
    pub visual_beats: f32,
    /// Beats between palette changes.
    ///
    /// Deliberately not a multiple of [`visual_beats`](Self::visual_beats):
    /// if the two lined up, the audience would see the same visual/colour
    /// pairings recur all night. Coprime periods give many more combinations
    /// before anything repeats.
    pub palette_beats: f32,
    /// Length of the fade to black either side of a visual change.
    ///
    /// Zero — the default — hands over through a
    /// [`Transition`](crate::bleed::Transition) instead: the
    /// outgoing frame stays on screen and bleeds away while the new visual
    /// comes up underneath it. A fade is two seconds of nothing on a screen
    /// whose job is to be lit, and the bleed covers the same problem (an
    /// incoming simulation has nothing to show for its first second) without
    /// going dark. Set this above zero to get the old behaviour back.
    pub fade_beats: f32,
    /// Whether the autopilot changes visual at all.
    ///
    /// Off holds whatever is on screen while the palette still morphs and the
    /// knobs still drift — the show keeps moving, but it stays on one piece.
    /// That is what an operator wants when a visual happens to suit the track
    /// that is playing, and it is the difference between taking the wheel and
    /// switching the autopilot off entirely.
    pub cycle_visuals: bool,
    /// Whether unpatched macros wander on their own.
    pub drift: bool,
    /// Beats between new drift destinations.
    pub drift_beats: f32,
    /// How far from the middle drift is allowed to push a knob. Kept well
    /// inside `0..1` because the extremes of most parameters are where visuals
    /// stop looking good.
    pub drift_range: f32,
    /// Seconds a hand-moved knob stays under manual control before drift
    /// reclaims it.
    ///
    /// Long enough to actually work with a knob, short enough that an
    /// unattended show recovers on its own if somebody wanders off mid-tweak.
    pub release_seconds: f32,

    phase: Phase,
    next_visual_beat: f64,
    next_palette_beat: f64,
    next_drift_beat: f64,
    drift_from: [f32; MACRO_COUNT],
    drift_to: [f32; MACRO_COUNT],
    rng: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Hold,
    FadingOut,
    FadingIn,
}

impl Default for Autopilot {
    fn default() -> Self {
        Self {
            enabled: true,
            visual_beats: 192.0,
            palette_beats: 260.0,
            fade_beats: 0.0,
            cycle_visuals: true,
            drift: true,
            drift_beats: 64.0,
            drift_range: 0.3,
            release_seconds: 90.0,
            phase: Phase::Hold,
            next_visual_beat: 192.0,
            next_palette_beat: 260.0,
            next_drift_beat: 0.0,
            drift_from: [0.5; MACRO_COUNT],
            drift_to: [0.5; MACRO_COUNT],
            rng: 0x9E3779B9,
        }
    }
}

impl Autopilot {
    /// xorshift32. A real RNG would mean a dependency in the core crate for
    /// something whose only requirement is "not obviously periodic".
    fn next_f32(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1 << 24) as f32
    }

    /// Re-seed the timers to start from the current position. Call after
    /// enabling at runtime so a change does not fire immediately.
    pub fn restart(&mut self, clock: &ShowClock) {
        let now = clock.beats;
        self.next_palette_beat = now + self.palette_beats as f64;
        self.next_drift_beat = now;
        self.restart_visuals(clock);
    }

    /// Re-seed just the visual timer, for the same reason [`restart`] re-seeds
    /// all of them: turning rotation back on part-way through a hold would
    /// otherwise fire a change on the next frame, because the beat it was due
    /// on went by while rotation was off.
    ///
    /// [`restart`]: Self::restart
    pub fn restart_visuals(&mut self, clock: &ShowClock) {
        self.next_visual_beat = clock.beats + self.visual_beats as f64;
        self.phase = Phase::Hold;
    }
}

/// Drives visual changes, palette changes and macro drift.
pub fn run_autopilot(
    clock: Res<ShowClock>,
    modulation: Res<Modulation>,
    palette: Res<Palette>,
    mut autopilot: ResMut<Autopilot>,
    mut output: ResMut<ShowOutput>,
    mut macros: ResMut<Macros>,
    mut morph: ResMut<PaletteMorph>,
    mut requests: MessageWriter<VisualRequest>,
) {
    if !autopilot.enabled {
        // Release the fade so a manual takeover is not left dimmed.
        output.autofade = (output.autofade + clock.delta * 2.0).min(1.0);
        return;
    }

    let now = clock.beats;

    // --- visual changes ------------------------------------------------------
    if !autopilot.cycle_visuals {
        // Hold the current visual. The timer is re-seeded every frame rather
        // than left to run, so switching rotation back on gives a full period
        // before the next change instead of one that is already overdue — and
        // the fade is released in case rotation was switched off mid-change,
        // which would otherwise leave the screen stuck part-way to black.
        autopilot.restart_visuals(&clock);
        output.autofade = (output.autofade + clock.delta * 2.0).min(1.0);
    } else {
        match autopilot.phase {
            Phase::Hold => {
                // With a fade configured the change starts early, so the fade is
                // finished by the beat the change was due on.
                let due = autopilot.next_visual_beat - autopilot.fade_beats.max(0.0) as f64;
                if now >= due {
                    if autopilot.fade_beats > 0.0 {
                        autopilot.phase = Phase::FadingOut;
                    } else {
                        // Straight into the change, with nothing dipping: the
                        // transition started by the switch itself is what covers
                        // it, and it needs the outgoing frame at full level to
                        // have anything worth keeping.
                        requests.write(VisualRequest::Cycle(1));
                        autopilot.next_visual_beat = now + autopilot.visual_beats as f64;
                    }
                }
            }
            Phase::FadingOut => {
                let remaining = (autopilot.next_visual_beat - now) as f32;
                output.autofade = (remaining / autopilot.fade_beats.max(0.01)).clamp(0.0, 1.0);

                if now >= autopilot.next_visual_beat {
                    output.autofade = 0.0;
                    requests.write(VisualRequest::Cycle(1));
                    autopilot.phase = Phase::FadingIn;
                    autopilot.next_visual_beat = now + autopilot.visual_beats as f64;
                }
            }
            Phase::FadingIn => {
                // The incoming visual needs a moment before it is worth showing —
                // a simulation in particular has nothing on screen for its first
                // second — so the fade back in is the same length as the fade out.
                let elapsed =
                    (now - (autopilot.next_visual_beat - autopilot.visual_beats as f64)) as f32;
                output.autofade = (elapsed / autopilot.fade_beats.max(0.01)).clamp(0.0, 1.0);

                if output.autofade >= 1.0 {
                    autopilot.phase = Phase::Hold;
                }
            }
        }
    }

    // --- palette changes -----------------------------------------------------
    // No fade needed: `PaletteMorph` already interpolates the coefficients, so
    // this is a slow recolour rather than a cut.
    if now >= autopilot.next_palette_beat {
        autopilot.next_palette_beat = now + autopilot.palette_beats as f64;
        morph.next(*palette);
    }

    // --- macro drift ---------------------------------------------------------
    if !autopilot.drift {
        return;
    }

    if now >= autopilot.next_drift_beat {
        autopilot.next_drift_beat = now + autopilot.drift_beats as f64;
        autopilot.drift_from = autopilot.drift_to;

        let range = autopilot.drift_range;
        for index in 0..MACRO_COUNT {
            let r = autopilot.next_f32();
            autopilot.drift_to[index] = (0.5 + (r - 0.5) * 2.0 * range).clamp(0.0, 1.0);
        }
    }

    let span = autopilot.drift_beats.max(0.01) as f64;
    let t = (1.0 - (autopilot.next_drift_beat - now) / span).clamp(0.0, 1.0) as f32;
    // Smoothstep, so a knob eases into and out of each destination instead of
    // travelling at constant speed and stopping dead.
    let eased = t * t * (3.0 - 2.0 * t);

    let release = autopilot.release_seconds;
    for index in 0..MACRO_COUNT {
        // Anything with a modulator patched is already being driven; drifting
        // it too would just fight the modulator.
        if modulation.rows.iter().any(|row| row.target == index) {
            continue;
        }

        // A knob somebody is holding belongs to them. Both endpoints are pinned
        // to wherever they left it, so when the takeover expires the drift
        // continues from that value instead of snapping back to a destination
        // chosen before the knob was touched.
        if macros.held(index, clock.elapsed, release) {
            let current = macros.get(index);
            autopilot.drift_from[index] = current;
            autopilot.drift_to[index] = current;
            continue;
        }

        let value = autopilot.drift_from[index]
            + (autopilot.drift_to[index] - autopilot.drift_from[index]) * eased;
        macros.set(index, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an app with just the resources `run_autopilot` touches.
    fn harness(elapsed: f64) -> App {
        let mut app = App::new();
        let mut clock = ShowClock::default();
        clock.elapsed = elapsed;
        clock.beats = elapsed * 2.0;
        clock.delta = 1.0 / 60.0;

        app.insert_resource(clock)
            .init_resource::<Modulation>()
            .init_resource::<Palette>()
            .init_resource::<PaletteMorph>()
            .init_resource::<Autopilot>()
            .init_resource::<ShowOutput>()
            .init_resource::<Macros>()
            .add_message::<VisualRequest>()
            .add_systems(Update, run_autopilot);
        app
    }

    /// Run one frame at a beat the visual timer is already past, and report
    /// whether the autopilot asked for a change.
    fn cycled(configure: impl FnOnce(&mut Autopilot)) -> bool {
        let mut app = harness(1.0);
        {
            let mut autopilot = app.world_mut().resource_mut::<Autopilot>();
            // Well past the first `visual_beats`, so a rotating autopilot is
            // certain to fire on this frame.
            autopilot.next_visual_beat = 0.0;
            configure(&mut autopilot);
        }
        app.update();

        let messages = app.world().resource::<Messages<VisualRequest>>();
        let mut cursor = messages.get_cursor();
        cursor.read(messages).count() > 0
    }

    #[test]
    fn rotation_off_holds_the_visual() {
        assert!(cycled(|_| {}), "the harness should cycle by default");
        assert!(
            !cycled(|autopilot| autopilot.cycle_visuals = false),
            "rotation was off, so nothing should have asked for a new visual"
        );
    }

    #[test]
    fn rotation_resumes_with_a_full_period() {
        let mut app = harness(1.0);
        {
            let mut autopilot = app.world_mut().resource_mut::<Autopilot>();
            autopilot.cycle_visuals = false;
            autopilot.next_visual_beat = 0.0;
        }
        // Time passes with rotation held...
        app.update();
        app.world_mut().resource_mut::<Autopilot>().cycle_visuals = true;
        app.update();

        // ...and switching it back on does not immediately fire the change that
        // came due while it was off.
        let messages = app.world().resource::<Messages<VisualRequest>>();
        let mut cursor = messages.get_cursor();
        assert_eq!(
            cursor.read(messages).count(),
            0,
            "resuming rotation fired a change that was due while it was held"
        );
    }

    #[test]
    fn palette_and_drift_still_run_with_rotation_off() {
        let mut app = harness(1.0);
        {
            let mut autopilot = app.world_mut().resource_mut::<Autopilot>();
            autopilot.cycle_visuals = false;
            autopilot.next_palette_beat = 0.0;
        }
        app.world_mut().resource_mut::<Macros>().snap(0, 0.9);
        let palette_before = app.world().resource::<PaletteMorph>().to;
        app.update();

        // Holding the visual is not the same gesture as switching the autopilot
        // off: the screen has to keep moving, or an hour on one piece is an hour
        // of the same frame.
        let macros = app.world().resource::<Macros>();
        assert!(
            (macros.target[0] - 0.9).abs() > 0.1,
            "drift stopped when only rotation was held: {}",
            macros.target[0]
        );
        assert_ne!(
            app.world().resource::<PaletteMorph>().to,
            palette_before,
            "the palette change stopped when only rotation was held"
        );
    }

    #[test]
    fn drift_moves_untouched_knobs() {
        let mut app = harness(1.0);
        app.world_mut().resource_mut::<Macros>().snap(0, 0.9);
        app.update();

        // Nothing is holding knob 0, so the autopilot owns it and pulls it back
        // towards its own destination.
        let macros = app.world().resource::<Macros>();
        assert!(
            (macros.target[0] - 0.9).abs() > 0.1,
            "expected drift to take an untouched knob, got {}",
            macros.target[0]
        );
    }

    #[test]
    fn a_touched_knob_is_left_alone() {
        let mut app = harness(1.0);
        {
            let mut macros = app.world_mut().resource_mut::<Macros>();
            macros.snap(3, 0.9);
            macros.touch(3, 1.0);
        }
        app.update();

        // This is the regression that made every macro key look broken: the
        // autopilot rewrote all eight knobs every frame, so manual input was
        // overwritten before it could ever be seen.
        let macros = app.world().resource::<Macros>();
        assert!(
            (macros.target[3] - 0.9).abs() < 1e-6,
            "autopilot overwrote a hand-held knob: {}",
            macros.target[3]
        );
    }

    #[test]
    fn a_knob_is_released_after_the_timeout() {
        let release = Autopilot::default().release_seconds as f64;
        let mut app = harness(release + 10.0);
        {
            let mut macros = app.world_mut().resource_mut::<Macros>();
            macros.snap(3, 0.9);
            // Touched once, long ago.
            macros.touch(3, 1.0);
        }
        app.update();

        let macros = app.world().resource::<Macros>();
        assert!(
            (macros.target[3] - 0.9).abs() > 0.1,
            "expected the autopilot to reclaim a knob after {release}s, got {}",
            macros.target[3]
        );
    }
}
