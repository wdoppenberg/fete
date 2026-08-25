//! The uniform block every visual receives, and the CPU-side frame context.
//!
//! [`FeteGlobals`] is the single contract between Rust and WGSL. A visual
//! embeds it as `#[uniform(0)]` and the framework keeps it up to date; the
//! shader side declares the identical struct via `#import fete::globals`.
//! Adding a field means editing both halves, which is why the block is
//! deliberately small and generic — anything visual-specific belongs in that
//! visual's own uniform.

use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;

use crate::clock::ShowClock;
use crate::palette::Palette;
use crate::present::StageResolution;
use crate::quality::Quality;
use crate::signal::{Audio, Macros};

/// Per-frame state shared by every visual, laid out for a uniform buffer.
///
/// Field order and types must stay in lockstep with the `Globals` struct in
/// `shaders/globals.wgsl`.
///
/// **Adding a field here currently breaks every visual.** On this toolchain
/// (Bevy 0.19 / naga_oil 0.22), growing this struct by even a single `f32`
/// makes every material fail validation with `Entry point fragment at Fragment
/// is invalid — invalid function call`, pointing at the calls that take a
/// `Globals`. Removing the field fixes it. Until that is understood, derive
/// what you need from the fields that are already here — see `phrase_of` and
/// `pulse_every` in `shaders/globals.wgsl`, which give beat subdivisions
/// without costing a field.
#[derive(Resource, ShaderType, Debug, Clone, Copy, Default)]
pub struct FeteGlobals {
    /// Render target size in pixels.
    pub resolution: Vec2,
    /// Seconds since the show started.
    pub time: f32,
    /// Seconds since the previous frame.
    pub delta: f32,
    /// Continuous beat position. Fractional part is the beat phase.
    pub beat: f32,
    /// Position within the current beat, `0.0..1.0`.
    pub beat_phase: f32,
    /// Position within the current bar, `0.0..1.0`.
    pub bar_phase: f32,
    /// Decaying per-beat envelope, `0.0..1.0`.
    pub pulse: f32,
    /// Random value regenerated each time a visual is activated. Seed hashes
    /// with this so the same visual looks different on every appearance.
    pub seed: f32,
    /// Master output level, `0.0..1.0`. Visuals should multiply final colour by
    /// this so the operator always has a working fade-to-black.
    pub intensity: f32,
    /// `(level, bass, mid, high)`.
    pub audio: Vec4,
    /// Macro knobs 0..4.
    pub macros_a: Vec4,
    /// Macro knobs 4..8.
    pub macros_b: Vec4,
    /// Cosine palette DC offset, `w` unused.
    pub palette_a: Vec4,
    /// Cosine palette amplitude, `w` unused.
    pub palette_b: Vec4,
    /// Cosine palette frequency, `w` unused.
    pub palette_c: Vec4,
    /// Cosine palette phase, `w` unused.
    pub palette_d: Vec4,
}

impl FeteGlobals {
    /// Macro knob by index, spanning both packed vectors.
    pub fn macro_value(&self, index: usize) -> f32 {
        match index {
            0..=3 => self.macros_a[index],
            4..=7 => self.macros_b[index - 4],
            _ => 0.0,
        }
    }
}

/// Master output level and other show-wide output settings.
///
/// The master and the autopilot's transition fade are kept separate so they
/// cannot clobber each other: the autopilot dips `autofade` through a visual
/// change while an operator or a control app owns `master`, and neither has to
/// know what the other is doing.
#[derive(Resource, Debug, Clone)]
pub struct ShowOutput {
    /// Master fade, `0.0..1.0`. Owned by whoever is driving the show.
    pub master: f32,
    /// Transition fade, `0.0..1.0`. Owned by the autopilot.
    pub autofade: f32,
    /// Random seed handed to the active visual.
    pub seed: f32,
}

impl Default for ShowOutput {
    fn default() -> Self {
        Self {
            master: 1.0,
            autofade: 1.0,
            seed: 0.0,
        }
    }
}

impl ShowOutput {
    /// The level actually applied to the output.
    pub fn level(&self) -> f32 {
        self.master * self.autofade
    }
}

/// Everything a visual needs to animate itself for one frame.
///
/// Assembled by the framework and passed to [`Visual::animate`](crate::visual::Visual::animate),
/// so a visual can stay a plain data type with no system parameters of its own.
pub struct Frame<'a> {
    pub globals: &'a FeteGlobals,
    pub clock: &'a ShowClock,
    pub macros: &'a Macros,
    pub audio: &'a Audio,
    pub palette: &'a Palette,
    /// How much this machine can be asked for. Visuals that scale a CPU-side
    /// cost — a particle count, a trail length — read it here; shader loop
    /// bounds go through `specialize` instead, so they stay constants.
    pub quality: Quality,
}

impl Frame<'_> {
    /// Shorthand for a macro knob.
    pub fn knob(&self, index: usize) -> f32 {
        self.macros.get(index)
    }

    /// Shorthand for a macro knob remapped into `min..max`.
    pub fn knob_range(&self, index: usize, min: f32, max: f32) -> f32 {
        min + (max - min) * self.macros.get(index)
    }
}

/// Rebuilds [`FeteGlobals`] from the individual resources each frame.
pub fn update_globals(
    mut globals: ResMut<FeteGlobals>,
    clock: Res<ShowClock>,
    macros: Res<Macros>,
    audio: Res<Audio>,
    palette: Res<Palette>,
    output: Res<ShowOutput>,
    stage: Res<StageResolution>,
) {
    // The stage size, not the window size: below full render scale the two
    // differ, and a shader that thinks it has more pixels than it does draws
    // scanlines and grain it cannot resolve. `StageResolution` already holds
    // the last good value when the window is minimised, so there is nothing
    // to guard against here — a zero would turn every `uv / resolution` into
    // a NaN.
    globals.resolution = stage.0;

    globals.time = clock.elapsed as f32;
    globals.delta = clock.delta;
    globals.beat = clock.beats as f32;
    globals.beat_phase = clock.beat_phase();
    globals.bar_phase = clock.bar_phase();
    globals.pulse = clock.pulse(2.0);
    globals.seed = output.seed;
    globals.intensity = output.level();
    globals.audio = audio.as_vec4();
    globals.macros_a = macros.as_vec4_a();
    globals.macros_b = macros.as_vec4_b();
    globals.palette_a = palette.a.extend(0.0);
    globals.palette_b = palette.b.extend(0.0);
    globals.palette_c = palette.c.extend(0.0);
    globals.palette_d = palette.d.extend(0.0);
}
