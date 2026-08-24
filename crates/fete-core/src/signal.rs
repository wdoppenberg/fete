//! Macro knobs and the modulation matrix that drives them.
//!
//! Visuals never read the keyboard, MIDI or audio directly. They read eight
//! normalised `0.0..1.0` [macros](Macros), and something else decides where
//! those values come from — a key binding, an LFO locked to the show clock, an
//! audio band, or a hand on a MIDI fader. That indirection is what makes a
//! visual reusable across very different inputs.

use bevy::prelude::*;

use crate::clock::ShowClock;

/// Number of macro knobs exposed to every visual.
///
/// Eight fits exactly into the two `vec4`s of [`FeteGlobals`](crate::globals::FeteGlobals)
/// and matches the fader bank on most small MIDI controllers.
pub const MACRO_COUNT: usize = 8;

/// The eight normalised knobs a visual can read.
///
/// Values are smoothed towards their targets so that a stepped input (a key
/// press, a coarse MIDI value) never produces a visible jump on a 4m screen.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Macros {
    /// Current, smoothed values. This is what visuals read.
    pub value: [f32; MACRO_COUNT],
    /// Where each knob is heading. This is what inputs write.
    pub target: [f32; MACRO_COUNT],
    /// Approximate time in seconds to travel most of the way to the target.
    pub smoothing: f32,
    /// Show-clock time each knob was last moved by hand, in seconds.
    ///
    /// Automation reads this and leaves recently-touched knobs alone. Without
    /// it the autopilot rewrites every knob every frame and manual input has no
    /// visible effect at all — the control simply appears dead.
    pub touched: [f64; MACRO_COUNT],
}

impl Default for Macros {
    fn default() -> Self {
        Self {
            value: [0.5; MACRO_COUNT],
            target: [0.5; MACRO_COUNT],
            smoothing: 0.15,
            // Far enough in the past that nothing counts as touched at startup.
            touched: [f64::NEG_INFINITY; MACRO_COUNT],
        }
    }
}

impl Macros {
    /// Read a knob, clamped to a valid index.
    pub fn get(&self, index: usize) -> f32 {
        self.value.get(index).copied().unwrap_or(0.0)
    }

    /// Aim a knob at a new value. Out-of-range indices are ignored.
    pub fn set(&mut self, index: usize, value: f32) {
        if let Some(slot) = self.target.get_mut(index) {
            *slot = value.clamp(0.0, 1.0);
        }
    }

    /// Nudge a knob by a delta.
    pub fn nudge(&mut self, index: usize, delta: f32) {
        let next = self.get(index) + delta;
        self.set(index, next);
    }

    /// Jump straight to a value with no smoothing. Use for scene loads.
    pub fn snap(&mut self, index: usize, value: f32) {
        let value = value.clamp(0.0, 1.0);
        if let Some(slot) = self.target.get_mut(index) {
            *slot = value;
        }
        if let Some(slot) = self.value.get_mut(index) {
            *slot = value;
        }
    }

    /// Mark a knob as being under manual control as of `now`.
    ///
    /// Call this from any input that a person is driving. Automation that
    /// respects [`held`](Self::held) will then keep its hands off.
    pub fn touch(&mut self, index: usize, now: f64) {
        if let Some(slot) = self.touched.get_mut(index) {
            *slot = now;
        }
    }

    /// Has this knob been touched by hand within the last `window` seconds?
    pub fn held(&self, index: usize, now: f64, window: f32) -> bool {
        self.touched
            .get(index)
            .is_some_and(|&t| now - t < window as f64)
    }

    /// Macros 0..4, ready for a shader uniform.
    pub fn as_vec4_a(&self) -> Vec4 {
        Vec4::new(self.value[0], self.value[1], self.value[2], self.value[3])
    }

    /// Macros 4..8, ready for a shader uniform.
    pub fn as_vec4_b(&self) -> Vec4 {
        Vec4::new(self.value[4], self.value[5], self.value[6], self.value[7])
    }
}

/// Waveform used by [`ModSource::Lfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
    /// Rising ramp. Good for anything that should "reset" rather than reverse.
    Saw,
    Square,
    /// Smoothed noise — a random walk that never jumps.
    Noise,
}

impl Wave {
    /// Evaluate at phase `t` (in turns), returning `0.0..1.0`.
    pub fn eval(self, t: f32) -> f32 {
        let t = t.rem_euclid(1.0);
        match self {
            Wave::Sine => 0.5 - 0.5 * (t * std::f32::consts::TAU).cos(),
            Wave::Triangle => 1.0 - (2.0 * t - 1.0).abs(),
            Wave::Saw => t,
            Wave::Square => {
                if t < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            // Value noise: hash two integer lattice points and smoothstep
            // between them, so successive frames stay correlated.
            Wave::Noise => {
                let i = t * 16.0;
                let lo = hash01(i.floor());
                let hi = hash01(i.floor() + 1.0);
                let f = i.fract();
                lo + (hi - lo) * (f * f * (3.0 - 2.0 * f))
            }
        }
    }
}

fn hash01(x: f32) -> f32 {
    let x = (x * 12.9898).sin() * 43758.547;
    x.fract().abs()
}

/// Frequency bands extracted from the incoming audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Level,
    Bass,
    Mid,
    High,
}

/// Where a modulator gets its value from.
#[derive(Debug, Clone, Copy)]
pub enum ModSource {
    /// Free-running oscillator, rate given in Hz.
    Lfo { wave: Wave, hz: f32 },
    /// Oscillator locked to the show clock, rate given in beats per cycle.
    Synced { wave: Wave, beats: f32 },
    /// The decaying per-beat envelope. `shape` matches [`ShowClock::pulse`].
    Pulse { shape: f32 },
    /// An audio band.
    Audio(Band),
}

/// One row of the modulation matrix: a source patched into a macro.
#[derive(Debug, Clone, Copy)]
pub struct Modulator {
    /// Macro index this writes to.
    pub target: usize,
    pub source: ModSource,
    /// How far the source swings the knob, in knob units.
    pub depth: f32,
    /// The value the knob sits at when the source reads zero.
    pub bias: f32,
}

impl Modulator {
    pub fn new(target: usize, source: ModSource) -> Self {
        Self {
            target,
            source,
            depth: 1.0,
            bias: 0.0,
        }
    }

    pub fn with_depth(mut self, depth: f32) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_bias(mut self, bias: f32) -> Self {
        self.bias = bias;
        self
    }
}

/// The active modulation matrix.
///
/// Macros driven by a modulator are written every frame, so manual input to
/// those knobs is overridden. Unpatched knobs stay under manual control.
#[derive(Resource, Debug, Clone)]
pub struct Modulation {
    pub rows: Vec<Modulator>,
    /// Scales every modulator at once. Pull to zero to freeze all motion.
    pub amount: f32,
}

impl Default for Modulation {
    fn default() -> Self {
        // Full depth. Deriving `Default` would leave `amount` at zero, which
        // silently does nothing to every patched modulator and reports itself
        // in the HUD as "frozen" on an app nobody has touched.
        Self {
            rows: Vec::new(),
            amount: 1.0,
        }
    }
}

impl Modulation {
    pub fn patch(&mut self, modulator: Modulator) -> &mut Self {
        self.rows.push(modulator);
        self
    }

    /// Remove every modulator writing to a macro, returning it to manual control.
    pub fn unpatch(&mut self, target: usize) {
        self.rows.retain(|row| row.target != target);
    }

    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Audio-reactive levels, all normalised to roughly `0.0..1.0`.
///
/// The default [`FeteCorePlugin`](crate::FeteCorePlugin) fills these in from
/// the show clock so visuals behave sensibly before any real input is wired up;
/// replace the producer to drive them from a capture device.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct Audio {
    /// Broadband loudness.
    pub level: f32,
    pub bass: f32,
    pub mid: f32,
    pub high: f32,
    /// Rises sharply on a transient, decays over a few frames.
    pub onset: f32,
}

impl Audio {
    pub fn band(&self, band: Band) -> f32 {
        match band {
            Band::Level => self.level,
            Band::Bass => self.bass,
            Band::Mid => self.mid,
            Band::High => self.high,
        }
    }

    /// Packed for a shader uniform as `(level, bass, mid, high)`.
    pub fn as_vec4(&self) -> Vec4 {
        Vec4::new(self.level, self.bass, self.mid, self.high)
    }
}

/// Stand-in audio derived from the show clock.
///
/// It is not a substitute for a capture device, but it means every visual can
/// be written against [`Audio`] from day one and will already look musical when
/// a real input is connected.
pub fn simulate_audio(clock: Res<ShowClock>, mut audio: ResMut<Audio>) {
    let kick = clock.pulse(3.0);
    // Offbeat hats: a pulse phase-shifted by half a beat.
    let offbeat = (1.0 - (clock.beat_phase() + 0.5).rem_euclid(1.0)).powf(6.0);
    let sweep = Wave::Sine.eval(clock.phrase(32.0));

    audio.bass = kick;
    audio.mid = (0.35 + 0.4 * sweep) * (0.5 + 0.5 * clock.pulse(1.0));
    audio.high = offbeat * 0.8;
    audio.level = (audio.bass * 0.5 + audio.mid * 0.3 + audio.high * 0.2).clamp(0.0, 1.0);
    audio.onset = if clock.beat_edge {
        1.0
    } else {
        audio.onset * 0.85
    };
}

/// Applies the modulation matrix, then smooths every macro toward its target.
pub fn apply_modulation(
    clock: Res<ShowClock>,
    audio: Res<Audio>,
    modulation: Res<Modulation>,
    mut macros: ResMut<Macros>,
) {
    for row in &modulation.rows {
        let raw = match row.source {
            ModSource::Lfo { wave, hz } => wave.eval(clock.elapsed as f32 * hz),
            ModSource::Synced { wave, beats } => wave.eval(clock.phrase(beats)),
            ModSource::Pulse { shape } => clock.pulse(shape),
            ModSource::Audio(band) => audio.band(band),
        };
        macros.set(row.target, row.bias + raw * row.depth * modulation.amount);
    }

    // Exponential smoothing, frame-rate independent: the same `smoothing`
    // produces the same glide whether we are running at 60 or 144 fps.
    let alpha = if macros.smoothing <= f32::EPSILON {
        1.0
    } else {
        1.0 - (-clock.delta / macros.smoothing).exp()
    };
    for i in 0..MACRO_COUNT {
        macros.value[i] += (macros.target[i] - macros.value[i]) * alpha;
    }
}
