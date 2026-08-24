//! The show clock: a musical time source that everything else is phrased against.
//!
//! Wall-clock seconds are a bad master for club visuals — motion that is not
//! locked to the music reads as noise on a big screen. Every visual therefore
//! animates against [`ShowClock`], which exposes beats, bar phase and a decaying
//! [`ShowClock::pulse`] envelope rather than raw seconds.

use bevy::prelude::*;

/// Musical transport for the show.
///
/// Beats accumulate as a `f64` so a set running for hours keeps sub-frame
/// precision; phases are derived from it and always land in `0.0..1.0`.
#[derive(Resource, Debug, Clone)]
pub struct ShowClock {
    /// Tempo in beats per minute. Free to change mid-show; [`beats`](Self::beats)
    /// keeps accumulating from wherever it was, so the phase never jumps.
    pub bpm: f32,
    /// Beats in a bar. 4 unless you are doing something interesting.
    pub beats_per_bar: u32,
    /// When paused the clock holds its position but visuals keep rendering.
    pub running: bool,
    /// Total beats since the show started.
    pub beats: f64,
    /// Seconds since the show started, ignoring pauses.
    pub elapsed: f64,
    /// Seconds elapsed in the last frame, scaled by nothing — raw delta.
    pub delta: f32,
    /// True on the first frame of each beat. Drives one-shot triggers.
    pub beat_edge: bool,
    /// True on the first frame of each bar.
    pub bar_edge: bool,
    /// Timestamps (in [`elapsed`](Self::elapsed) seconds) of recent tempo taps.
    taps: Vec<f64>,
}

impl Default for ShowClock {
    fn default() -> Self {
        Self {
            bpm: 128.0,
            beats_per_bar: 4,
            running: true,
            beats: 0.0,
            elapsed: 0.0,
            delta: 0.0,
            beat_edge: false,
            bar_edge: false,
            taps: Vec::new(),
        }
    }
}

impl ShowClock {
    /// Seconds per beat at the current tempo.
    pub fn beat_duration(&self) -> f32 {
        60.0 / self.bpm.max(1.0)
    }

    /// Position within the current beat, `0.0..1.0`.
    pub fn beat_phase(&self) -> f32 {
        self.beats.rem_euclid(1.0) as f32
    }

    /// Position within the current bar, `0.0..1.0`.
    pub fn bar_phase(&self) -> f32 {
        let per_bar = self.beats_per_bar.max(1) as f64;
        (self.beats.rem_euclid(per_bar) / per_bar) as f32
    }

    /// A sawtooth that resets on every `n`-beat boundary, `0.0..1.0`.
    ///
    /// `phrase(16.0)` is the usual way to drive slow structural change: a long
    /// sweep that lands exactly on the phrase boundary the DJ is mixing on.
    pub fn phrase(&self, beats: f32) -> f32 {
        let beats = beats.max(0.001) as f64;
        (self.beats.rem_euclid(beats) / beats) as f32
    }

    /// Envelope that snaps to 1.0 on each beat and decays over the beat.
    ///
    /// `shape` controls the curve: 1.0 is linear, higher is punchier. This is
    /// the single most useful signal in the whole framework — patch it into
    /// brightness, scale, or displacement and the visual instantly feels
    /// connected to the track.
    pub fn pulse(&self, shape: f32) -> f32 {
        (1.0 - self.beat_phase()).powf(shape.max(0.001))
    }

    /// Same as [`pulse`](Self::pulse) but firing once per bar.
    pub fn bar_pulse(&self, shape: f32) -> f32 {
        (1.0 - self.bar_phase()).powf(shape.max(0.001))
    }

    /// A pulse every `beats` beats rather than every beat.
    ///
    /// `pulse_div(2.0, ..)` is the useful one for atmosphere: reacting to every
    /// beat makes a visual twitch in time with the kick, which pulls the eye and
    /// competes with the music. Half-time reads as breathing with the track
    /// instead of being driven by it — the visual is clearly connected to what
    /// is playing, without asking to be watched.
    pub fn pulse_div(&self, beats: f32, shape: f32) -> f32 {
        (1.0 - self.phrase(beats)).powf(shape.max(0.001))
    }

    /// Register a tempo tap. Four or more taps give a usable estimate.
    ///
    /// Taps more than two seconds apart start a new measurement — that is
    /// slower than 30bpm, so it can only mean the operator stopped and
    /// restarted rather than genuinely tapping that slowly.
    pub fn tap(&mut self) {
        const RESET_AFTER: f64 = 2.0;

        if let Some(&last) = self.taps.last()
            && self.elapsed - last > RESET_AFTER
        {
            self.taps.clear();
        }
        self.taps.push(self.elapsed);
        // Keep a short window so the estimate tracks a drifting tempo.
        if self.taps.len() > 8 {
            self.taps.remove(0);
        }

        if self.taps.len() >= 2 {
            let span = self.taps[self.taps.len() - 1] - self.taps[0];
            let intervals = (self.taps.len() - 1) as f64;
            if span > 0.0 {
                self.bpm = (60.0 * intervals / span) as f32;
            }
        }

        // A tap is also a downbeat: realign the phase so the visual lands with
        // the operator's hand rather than wherever the free-running phase was.
        self.beats = self.beats.round();
    }

    /// Snap the phase to the nearest beat without touching the tempo.
    pub fn resync(&mut self) {
        self.beats = self.beats.round();
    }
}

/// Advances [`ShowClock`]. Runs in `First` so every later schedule sees the
/// same, already-updated musical position.
pub fn advance_clock(mut clock: ResMut<ShowClock>, time: Res<Time>) {
    let dt = time.delta_secs();
    clock.delta = dt;
    clock.elapsed += dt as f64;

    if !clock.running {
        clock.beat_edge = false;
        clock.bar_edge = false;
        return;
    }

    let before = clock.beats;
    clock.beats += (dt / clock.beat_duration()) as f64;

    clock.beat_edge = before.floor() != clock.beats.floor();
    let per_bar = clock.beats_per_bar.max(1) as f64;
    clock.bar_edge = (before / per_bar).floor() != (clock.beats / per_bar).floor();
}
