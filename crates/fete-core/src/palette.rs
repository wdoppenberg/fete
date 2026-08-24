//! Cosine gradient palettes.
//!
//! Every colour in the show comes from one four-vector formula:
//!
//! ```text
//! colour(t) = a + b * cos(TAU * (c * t + d))
//! ```
//!
//! Sixteen floats describe a whole colour scheme, they interpolate smoothly
//! into each other, and the same expression evaluates identically on the CPU
//! and in WGSL. That makes palette changes a first-class, animatable part of
//! the show rather than a set of hardcoded constants per visual.
//!
//! The formulation is Inigo Quilez's; see <https://iquilezles.org/articles/palettes/>.

use bevy::prelude::*;

/// A cosine gradient. `w` components are unused padding for uniform alignment.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// DC offset — the midpoint colour.
    pub a: Vec3,
    /// Amplitude — how far the gradient swings around the midpoint.
    pub b: Vec3,
    /// Frequency per channel. Non-integer values give gradients that do not
    /// loop, which reads as "expensive" rather than "cyclic" on a big screen.
    pub c: Vec3,
    /// Phase per channel. Offsetting the three channels is what produces hue
    /// travel rather than a simple brightness ramp.
    pub d: Vec3,
}

impl Default for Palette {
    fn default() -> Self {
        Self::SHINJUKU
    }
}

/// Tokyo-night palettes.
///
/// These are all built the same way: a **low DC offset** so the midpoint is
/// dark, and **large per-channel phase separation** in `d` so the swing between
/// channels produces saturated colour rather than a brightness ramp. That
/// combination is what gives neon — mostly-black frames with a few intensely
/// coloured regions — instead of the pastel wash a high offset produces.
impl Palette {
    /// Magenta and cyan over deep indigo. Signage after rain.
    pub const SHINJUKU: Self = Self {
        a: Vec3::new(0.26, 0.16, 0.34),
        b: Vec3::new(0.32, 0.20, 0.36),
        c: Vec3::new(1.0, 1.0, 1.0),
        d: Vec3::new(0.0, 0.33, 0.67),
    };

    /// Sodium street lamps and cold shadow. Warm, filmic, the least aggressive
    /// of the set — the safe default when the room is already busy.
    pub const SODIUM: Self = Self {
        a: Vec3::new(0.26, 0.19, 0.16),
        b: Vec3::new(0.30, 0.24, 0.18),
        c: Vec3::new(1.0, 0.95, 0.85),
        d: Vec3::new(0.02, 0.12, 0.28),
    };

    /// Hot pink through amber. Nightlife, high energy without going bright.
    pub const KABUKICHO: Self = Self {
        a: Vec3::new(0.28, 0.14, 0.20),
        b: Vec3::new(0.34, 0.18, 0.22),
        c: Vec3::new(1.0, 0.85, 0.9),
        d: Vec3::new(0.12, 0.05, 0.55),
    };

    /// Green-on-black phosphor. Terminals, elevator displays, 90s electronics.
    pub const PHOSPHOR: Self = Self {
        a: Vec3::new(0.14, 0.26, 0.18),
        b: Vec3::new(0.16, 0.32, 0.20),
        c: Vec3::new(0.9, 1.0, 0.95),
        d: Vec3::new(0.45, 0.55, 0.5),
    };

    /// Bleached cyan and steel. Fluorescent, clinical, late and cold.
    pub const FLUORESCENT: Self = Self {
        a: Vec3::new(0.20, 0.26, 0.30),
        b: Vec3::new(0.22, 0.28, 0.30),
        c: Vec3::new(1.0, 1.0, 0.95),
        d: Vec3::new(0.55, 0.62, 0.68),
    };

    /// Sodium amber against cold slate. Smog at altitude — almost a single
    /// hue plus its opposite, which is what keeps it from reading as neon.
    pub const SMOG: Self = Self {
        a: Vec3::new(0.24, 0.20, 0.20),
        b: Vec3::new(0.26, 0.20, 0.22),
        c: Vec3::new(1.0, 1.0, 1.0),
        d: Vec3::new(0.05, 0.20, 0.45),
    };

    /// Every built-in palette, in cycle order.
    pub const ALL: [Self; 6] = [
        Self::SHINJUKU,
        Self::SODIUM,
        Self::KABUKICHO,
        Self::PHOSPHOR,
        Self::FLUORESCENT,
        Self::SMOG,
    ];

    /// Human-readable names, index-matched with [`ALL`](Self::ALL).
    pub const NAMES: [&'static str; 6] = [
        "shinjuku",
        "sodium",
        "kabukicho",
        "phosphor",
        "fluorescent",
        "smog",
    ];

    /// Position of a preset in [`ALL`](Self::ALL), for seeding a morph.
    pub fn index_of(palette: Self) -> Option<usize> {
        Self::ALL.iter().position(|p| *p == palette)
    }

    /// Evaluate the gradient on the CPU. Matches `fete::palette::palette` in WGSL.
    pub fn sample(&self, t: f32) -> Vec3 {
        let phase = (self.c * t + self.d) * std::f32::consts::TAU;
        self.a + self.b * Vec3::new(phase.x.cos(), phase.y.cos(), phase.z.cos())
    }

    /// Linear blend between two palettes. Because the parameters are smooth,
    /// blending them produces a continuous morph rather than a cross-fade.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            a: self.a.lerp(other.a, t),
            b: self.b.lerp(other.b, t),
            c: self.c.lerp(other.c, t),
            d: self.d.lerp(other.d, t),
        }
    }
}

/// Drives [`Palette`] towards a selected preset.
///
/// Holding the morph in a resource rather than swapping the palette outright
/// means a palette change during a set is a two-second glide, not a cut.
#[derive(Resource, Debug, Clone)]
pub struct PaletteMorph {
    /// Where the palette is coming from.
    pub from: Palette,
    /// Index into [`Palette::ALL`] the palette is heading to.
    pub to: usize,
    /// Morph progress, `0.0..1.0`.
    pub t: f32,
    /// Seconds a full morph takes.
    pub duration: f32,
}

impl Default for PaletteMorph {
    fn default() -> Self {
        Self {
            from: Palette::default(),
            to: 0,
            t: 1.0,
            duration: 2.0,
        }
    }
}

impl PaletteMorph {
    /// Begin morphing to a preset from wherever the palette currently is.
    pub fn go_to(&mut self, current: Palette, index: usize) {
        self.from = current;
        self.to = index % Palette::ALL.len();
        self.t = 0.0;
    }

    /// Advance to the next preset in the cycle.
    pub fn next(&mut self, current: Palette) {
        let next = (self.to + 1) % Palette::ALL.len();
        self.go_to(current, next);
    }

    /// Name of the palette being morphed to.
    pub fn target_name(&self) -> &'static str {
        Palette::NAMES[self.to.min(Palette::NAMES.len() - 1)]
    }
}

/// Applies [`PaletteMorph`] to [`Palette`].
pub fn advance_palette_morph(
    time: Res<Time>,
    mut morph: ResMut<PaletteMorph>,
    mut palette: ResMut<Palette>,
) {
    if morph.t >= 1.0 {
        return;
    }

    morph.t = (morph.t + time.delta_secs() / morph.duration.max(0.001)).min(1.0);
    // Smoothstep so the morph eases in and out instead of starting abruptly.
    let eased = morph.t * morph.t * (3.0 - 2.0 * morph.t);
    let target = Palette::ALL[morph.to];
    *palette = morph.from.lerp(&target, eased);
}
