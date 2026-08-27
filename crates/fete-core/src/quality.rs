//! How hard the hardware may be pushed.
//!
//! Every visual in this repo was written against a desktop GPU and sized by
//! eye until it looked right. That is the correct way to build them and the
//! wrong way to ship them: a Raspberry Pi 4's VideoCore VI has roughly a
//! five-hundredth of the arithmetic throughput the visuals were tuned on, and
//! a 110-step raymarch does not become a 110-step raymarch that runs slower —
//! it becomes a slideshow.
//!
//! So cost is a dial, and [`Tier`] is where it points. The rule is that a
//! visual owns its own table: the framework never guesses what to cut, it only
//! says how much. [`Tier::pick`] is the whole idiom —
//!
//! ```ignore
//! ShaderDefVal::Int("MARCH_STEPS".into(), tier.pick(110, 64, 32))
//! ```
//!
//! Loop bounds reach WGSL as *shader defs*, not as uniforms, for two reasons.
//! The shallow one is that [`FeteGlobals`](crate::globals::FeteGlobals) cannot
//! grow a field without breaking every material on this toolchain. The real
//! one is that a def becomes a `const` in the generated WGSL, so the loop stays
//! unrollable — and on a tile-based mobile GPU the difference between a
//! constant bound and a uniform one is not a rounding error.

use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::shader::ShaderDefVal;

/// Rendering effort. Chosen once at startup and held for the show.
///
/// Deliberately not `Ord`. The variants would order High < Medium < Low, so
/// `tier >= Tier::Medium` would mean "medium or *cheaper*" — true, and exactly
/// backwards from how anyone reads it out loud. Match instead, or use
/// [`pick`](Self::pick).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// What the visuals were authored at. A discrete or Apple-class GPU.
    #[default]
    High,
    /// Roughly half the cost of [`High`](Self::High), and meant to be hard to
    /// spot in motion. An integrated laptop GPU, or a projector fed at 1080p
    /// from something modest.
    Medium,
    /// As cheap as a visual can be while still being recognisably itself.
    /// Sized for a Raspberry Pi 4 driving 720p.
    Low,
}

impl Tier {
    /// Every tier, most expensive first. Handy for cycling in the HUD.
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];

    /// Parse a command-line or environment value. Case-insensitive.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "high" => Some(Self::High),
            "medium" | "med" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    /// Pick the value for this tier.
    ///
    /// Deliberately positional rather than a lookup table: a quality table
    /// written as `tier.pick(110, 64, 32)` is one line, sits next to the
    /// constant it replaces, and is obvious at a glance in a diff.
    pub fn pick<T>(self, high: T, medium: T, low: T) -> T {
        match self {
            Self::High => high,
            Self::Medium => medium,
            Self::Low => low,
        }
    }

    /// Fraction of the window the stage renders at before upscaling.
    ///
    /// The single biggest lever there is, because shader cost falls with the
    /// square. It is also the one these visuals tolerate best: they are soft,
    /// mostly-black, bloomed fields with no hard edges, seen from metres away.
    /// Halving the resolution of a text UI would be vandalism; halving the
    /// resolution of Yama's haze is invisible past the first row.
    pub fn default_render_scale(self) -> f32 {
        self.pick(1.0, 0.75, 0.5)
    }

    /// Defs handed to every specialised shader, on top of whatever table the
    /// visual adds itself.
    ///
    /// `QUALITY_TIER` is `0` at [`High`](Self::High) and `2` at
    /// [`Low`](Self::Low), so a shader can compare; the two booleans exist so
    /// the common case is a plain `#ifdef QUALITY_LOW` with no arithmetic.
    pub fn shader_defs(self) -> Vec<ShaderDefVal> {
        let mut defs = vec![ShaderDefVal::UInt(
            "QUALITY_TIER".into(),
            self.pick(0, 1, 2),
        )];
        // `QUALITY_MEDIUM` means "medium or cheaper", so the low tier gets
        // both. Otherwise every cheap branch in a shader would need two
        // ifdefs to cover the two tiers that want it.
        if self != Self::High {
            defs.push(ShaderDefVal::Bool("QUALITY_MEDIUM".into(), true));
        }
        if self == Self::Low {
            defs.push(ShaderDefVal::Bool("QUALITY_LOW".into(), true));
        }
        defs
    }
}

/// The show-wide quality setting.
///
/// Read it from [`Frame`](crate::globals::Frame) inside a visual, or as a
/// resource anywhere else. Changing [`tier`](Self::tier) at runtime works —
/// materials respecialise — but costs a pipeline rebuild, so it is meant to be
/// set once at startup rather than swept live.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    pub tier: Tier,
    /// Fraction of the window the stage renders at, `0.25..=1.0`.
    pub render_scale: f32,
    /// True while nobody has asked for a particular tier, which is what lets
    /// [`detect_quality`] lower it after seeing the adapter. An explicit
    /// `--quality` or `FETE_QUALITY` clears it, because being overruled by a
    /// hardware probe you did not ask for is the kind of surprise that costs
    /// an hour at a venue.
    pub auto: bool,
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            tier: Tier::High,
            render_scale: Tier::High.default_render_scale(),
            auto: true,
        }
    }
}

impl Quality {
    /// A tier chosen on purpose, with its matching default render scale.
    pub fn new(tier: Tier) -> Self {
        Self {
            tier,
            render_scale: tier.default_render_scale(),
            auto: false,
        }
    }

    /// Override the render scale independently of the tier.
    ///
    /// Clamped rather than asserted: this arrives from a command line, and a
    /// show that starts at the wrong resolution beats a show that panics.
    pub fn with_render_scale(mut self, scale: f32) -> Self {
        self.render_scale = scale.clamp(0.25, 1.0);
        self
    }

    /// Read `FETE_QUALITY` and `FETE_RENDER_SCALE`.
    ///
    /// Environment rather than only a flag because the Pi runs this from a
    /// systemd unit, where an `Environment=` line is easier to change than the
    /// `ExecStart` command.
    pub fn from_env() -> Option<Self> {
        let tier = std::env::var("FETE_QUALITY").ok().and_then(|raw| {
            let parsed = Tier::parse(&raw);
            if parsed.is_none() {
                warn!("FETE_QUALITY=`{raw}` is not one of high, medium, low — ignoring");
            }
            parsed
        });
        let scale = std::env::var("FETE_RENDER_SCALE")
            .ok()
            .and_then(|raw| raw.trim().parse::<f32>().ok());

        match (tier, scale) {
            (None, None) => None,
            (tier, scale) => {
                let mut quality = Self::new(tier.unwrap_or_default());
                // A scale given without a tier is a deliberate nudge, not a
                // tier choice, so leave the probe free to lower the tier.
                quality.auto = tier.is_none();
                if let Some(scale) = scale {
                    quality = quality.with_render_scale(scale);
                }
                Some(quality)
            }
        }
    }

    /// The stage render target size for a given window size.
    pub fn stage_size(&self, window: UVec2) -> UVec2 {
        let scaled = window.as_vec2() * self.render_scale;
        // Never zero: a zero-sized render target is an instant wgpu error, and
        // a minimised window is a normal thing for a laptop to do mid-show.
        scaled.round().as_uvec2().max(UVec2::splat(1))
    }
}

/// Lowers the tier on hardware the visuals were plainly not authored for.
///
/// Runs in `Startup`, which is late enough that `RenderAdapterInfo` exists in
/// the main world (the renderer inserts it during plugin `finish`) and early
/// enough that no material has been built yet.
///
/// This only ever moves *down*. Guessing that a GPU is fast is how you end up
/// with a black screen in front of an audience; guessing that it is slow costs
/// a slightly softer image and a line in the log saying so.
pub fn detect_quality(mut quality: ResMut<Quality>, adapter: Option<Res<RenderAdapterInfo>>) {
    let Some(adapter) = adapter else {
        return;
    };

    if !quality.auto {
        info!(
            "quality: {} at {:.0}% render scale (set explicitly)",
            quality.tier.as_str(),
            quality.render_scale * 100.0
        );
        return;
    }

    // Broadcom is the Pi's VideoCore. Mesa's synthetic id covers llvmpipe and
    // lavapipe, which mean software rendering — Bevy warns about that on its
    // own, but a software adapter wants the cheapest tier for the same reason.
    //
    // Matching on vendor ids rather than `DeviceType::Cpu` keeps wgpu out of
    // this crate's dependency list; `DeviceType` is one of the few wgpu types
    // Bevy does not re-export.
    const BROADCOM: u32 = 0x14E4;
    const MESA_SOFTWARE: u32 = 0x1_0005;

    if matches!(adapter.vendor, BROADCOM | MESA_SOFTWARE) {
        *quality = Quality::new(Tier::Low);
        // Keep `auto` set: the probe made this choice, nobody asked for it,
        // and the HUD says as much.
        quality.auto = true;
        warn!(
            "quality: `{}` will not hold framerate at high quality — dropping to low \
             ({:.0}% render scale).",
            adapter.name,
            quality.render_scale * 100.0
        );
        // The probe cannot run any earlier than this: `RenderAdapterInfo` does
        // not exist until the render device has been created, which is after
        // every plugin has built. Anything sized at plugin-build time is
        // therefore already fixed — which for the simulation visuals is most of
        // their cost and nearly all of their memory. Say so, rather than
        // leaving someone to wonder why the tier says low and the Pi still
        // runs out of memory.
        warn!(
            "quality: simulation visuals were already sized at the previous tier. \
             Pass --quality low or set FETE_QUALITY=low to shrink them too."
        );
    } else {
        info!(
            "quality: {} at {:.0}% render scale on `{}`",
            quality.tier.as_str(),
            quality.render_scale * 100.0,
            adapter.name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_names_a_flag_would_carry() {
        assert_eq!(Tier::parse("low"), Some(Tier::Low));
        assert_eq!(Tier::parse("  HIGH "), Some(Tier::High));
        assert_eq!(Tier::parse("med"), Some(Tier::Medium));
        assert_eq!(Tier::parse("potato"), None);
    }

    #[test]
    fn pick_selects_by_tier() {
        assert_eq!(Tier::High.pick(110, 64, 32), 110);
        assert_eq!(Tier::Medium.pick(110, 64, 32), 64);
        assert_eq!(Tier::Low.pick(110, 64, 32), 32);
    }

    #[test]
    fn low_defines_medium_too() {
        let defs = Tier::Low.shader_defs();
        let names: Vec<&str> = defs
            .iter()
            .map(|def| match def {
                ShaderDefVal::Bool(name, _) | ShaderDefVal::Int(name, _) => name.as_str(),
                ShaderDefVal::UInt(name, _) => name.as_str(),
            })
            .collect();
        assert!(names.contains(&"QUALITY_MEDIUM"));
        assert!(names.contains(&"QUALITY_LOW"));

        let high: Vec<_> = Tier::High.shader_defs();
        assert_eq!(high.len(), 1, "high tier defines only QUALITY_TIER");
    }

    #[test]
    fn stage_size_scales_and_never_collapses() {
        let quality = Quality::new(Tier::Low);
        assert_eq!(
            quality.stage_size(UVec2::new(1280, 720)),
            UVec2::new(640, 360)
        );
        assert_eq!(quality.stage_size(UVec2::ZERO), UVec2::splat(1));
    }

    #[test]
    fn render_scale_is_clamped_not_trusted() {
        assert_eq!(
            Quality::new(Tier::High).with_render_scale(4.0).render_scale,
            1.0
        );
        assert_eq!(
            Quality::new(Tier::High).with_render_scale(0.0).render_scale,
            0.25
        );
    }
}
