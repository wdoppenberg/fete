//! **Terebi** — テレビ. A wall of 90s CRT sets in a dark room, each one playing
//! its own fragment of late-night Japanese television: 風雲!たけし城, a variety
//! studio, a quiz panel, ゲゲゲの鬼太郎, ドラゴンボール, and the ones showing
//! nothing but snow.
//!
//! The wall is carved rather than tiled. A cell is cut in half along its longer
//! side up to three times, each cut decided by a hash, and every rectangle that
//! falls out is one set — unequal sizes, no neighbour lookups, nothing stored.
//! That matters more here than anywhere else in the show: a grid of identical
//! lit rectangles is the one thing a wall of televisions must never look like.
//!
//! # What is on
//!
//! Footage, decoded by [`fete_video`] from the clips in `./video` — see
//! `tools/fetch-clips.sh`. Every set that is switched on is showing a real
//! broadcast, and which one is decided by *position* rather than by a hash, so
//! two sets near each other can never be showing the same thing.
//!
//! This visual used to carry nine synthesised programmes instead — a studio, an
//! anime impact frame, a vertical shooter, a platformer, a pseudo-3d racer, a
//! title card, a test card — each a pure function of a picture coordinate.
//! They are gone. What is left of that idea is snow, which is not a programme:
//! it is what a television does with no signal, and it covers the instant after
//! a set retunes and the case where there are no feeds at all.
//!
//! **The consequence is that clips are no longer optional.** Without them —
//! no `ffmpeg`, no `./video`, `--no-video` — every set on the wall shows snow.
//! That is an honest picture of a room full of untuned televisions and it is
//! not a crash, but it is not the visual either.
//!
//! # The sync
//!
//! Every so often the wall gangs up. Half the time that means every set is
//! handed the same feed and the same clock and plays it at its own scale — a
//! shop window with every screen tuned to one broadcast. The other half, every
//! set is handed the position of its own glass on the wall instead of its own
//! picture coordinate, and one enormous picture is split across every screen,
//! each piece still bulging through its own tube. That second one costs a `mix`
//! on two floats.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | how many sets are switched on |
//! | E/D | 2 | how often the sets retune |
//! | R/F | 3 | sync — how often the whole wall shows one broadcast |
//! | T/G | 4 | interference — snow, tracking, lost vertical hold |
//! | Y/H | 5 | set size — a few large sets or a bank of portables |
//! | U/J | 6 | colour spread — also how far footage is graded to the palette |
//! | I/K | 7 | beat depth (half-time) |

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use fete_core::prelude::*;
use fete_video::VideoWall;

/// Must match `TerebiParams` in `terebi.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct TerebiParams {
    /// Lateral drift of the wall, in screen units. Bounded — see [`Terebi::animate`].
    pub sway: Vec2,
    /// The night's schedule, in programme units. Wraps at [`PROGRAMME_WRAP`].
    pub programme: f32,
    /// Smoothed half-time beat energy.
    pub energy: f32,
    /// Smoothed interference amount, so tape damage comes and goes rather than
    /// switching on.
    pub interference: f32,
    /// How far into a sync window the wall is, `0.0..1.0`.
    pub sync: f32,
    /// What this sync window does: `0.0` puts every set on the same broadcast
    /// at its own scale, `1.0` spreads one picture across the whole wall.
    pub wall_mode: f32,
    /// How many layers of the video texture hold a picture.
    ///
    /// Zero — the default, and the case on any machine without clips or
    /// without `ffmpeg` — means the shader never reaches for the texture and
    /// the wall plays the nine synthesised channels alone.
    pub video_slots: f32,
}

/// Where the programme clock wraps.
///
/// The shader takes `floor(programme / dwell)` as a set's channel index, so an
/// f32 carrying hours of programme time has too little left for the fraction
/// that schedules the next cut. Wrapping costs one mass channel change every
/// twenty minutes or so, which on a wall of televisions is indistinguishable
/// from any other cut.
pub const PROGRAMME_WRAP: f32 = 512.0;

/// How often a sync window may open, in beats.
///
/// Twelve bars. The wall ganging up is the best thing this visual does and it
/// is the one moment where every set changes at once, which means it is also
/// the thing that decides whether the wall reads as a room of televisions or as
/// one display cut into rectangles. Too often and it is the latter.
const SYNC_PERIOD: f32 = 48.0;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub struct Terebi {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: TerebiParams,
    /// One layer per decoder, re-pointed by [`follow_video_wall`]. `None` binds
    /// Bevy's fallback texture, which is why the shader can sample it
    /// unconditionally and gate on `video_slots` instead.
    #[texture(2, dimension = "2d_array")]
    #[sampler(3)]
    video: Option<Handle<Image>>,
}

impl Material2d for Terebi {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_terebi/shaders/terebi.wgsl".into()
    }
}

impl Visual for Terebi {
    const ID: VisualId = "terebi";
    const NAME: &'static str = "Terebi";
    const TAGS: &'static [&'static str] = &["tokyo", "crt", "collage", "busy"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // Programme time. Integrated rather than `time * rate`, which are only
        // equal while the rate is constant: computed from the clock, turning the
        // cut rate up rewrites what every set has *already* shown and the whole
        // wall jumps to a different point in the schedule.
        self.params.programme += frame.knob_range(2, 0.012, 0.20) * dt;
        if self.params.programme >= PROGRAMME_WRAP {
            self.params.programme -= PROGRAMME_WRAP;
        }

        // Bounded, not integrated — same reason as Kanban. An unbounded drift
        // walks the cell coordinates into the thousands over a night and the
        // hashes that lay the wall out run out of fraction and quantise.
        let wander =
            |period: f32, phase: f32| ((beats / period + phase) * std::f32::consts::TAU).sin();
        self.params.sway = Vec2::new(wander(61.0, 0.0) * 0.05, wander(83.0, 0.37) * 0.03);

        // Glides over half a second. Snapped, every set in the frame tears at
        // once, which reads as a bug rather than as tape.
        self.params.interference = smooth(self.params.interference, frame.knob(4), dt, 0.5);

        // Sync windows. One may open every phrase; whether it does is a hash of
        // which phrase it is, so the knob sets how often the wall gangs up
        // rather than scheduling it. Held for four to twenty-four beats, drawn
        // from its own hash rather than from the one that opened the window —
        // sharing them ties how long a sync lasts to how rare it is, and a low
        // knob ends up only ever producing the shortest ones. Always closed well
        // before the next window, so the envelope is never interrupted.
        let window = (beats / SYNC_PERIOD).floor();
        let roll = hash11(window * 7.3 + frame.globals.seed * 31.0);
        let target = if roll < frame.knob(3) * 0.42 {
            let held = beats - window * SYNC_PERIOD;
            let hold = 4.0 + hash11(window * 13.9 + 3.3) * 20.0;
            (smoothstep(0.0, 1.0, held) * (1.0 - smoothstep(hold, hold + 2.0, held)))
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.params.sync = smooth(self.params.sync, target, dt, 0.10);
        // Decided with the window, so it cannot change halfway through one.
        // Held while the window closes, or the last frames of a wall picture
        // would snap back to twenty separate ones.
        if target > 0.0 {
            self.params.wall_mode = step(0.45, hash11(window * 3.1 + frame.globals.seed * 17.0));
        }

        // Half-time and heavily smoothed: a swell, not a hit.
        let energy =
            (frame.clock.pulse_div(2.0, 2.2) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        self.params.energy = smooth(self.params.energy, energy, dt, 0.3);
    }
}

/// Frame-rate independent exponential smoothing. `tau` is roughly the time to
/// cover most of the remaining distance.
fn smooth(current: f32, target: f32, dt: f32, tau: f32) -> f32 {
    let alpha = 1.0 - (-dt / tau.max(1e-4)).exp();
    current + (target - current) * alpha
}

fn step(edge: f32, x: f32) -> f32 {
    if x < edge { 0.0 } else { 1.0 }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The `hash11` from `fete::noise`, so the sync windows the CPU schedules are
/// drawn from the same sequence the shader would have produced.
fn hash11(p: f32) -> f32 {
    let mut x = (p * 0.1031).fract();
    x *= x + 33.33;
    x *= x + x;
    x.fract()
}

/// Points the material at the video wall, if the show has one.
///
/// `Option<Res<_>>`, because [`VideoPlugin`] installs nothing when there are no
/// clips to play — which is the usual case. Terebi has to be a complete visual
/// without it.
///
/// [`VideoPlugin`]: fete_video::VideoPlugin
fn follow_video_wall(
    wall: Option<Res<VideoWall>>,
    mut materials: ResMut<Assets<Terebi>>,
    surfaces: Query<&MeshMaterial2d<Terebi>, With<VisualSurface>>,
) {
    let Some(wall) = wall else {
        return;
    };

    for handle in &surfaces {
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        if material.video.as_ref() != Some(&wall.texture) {
            material.video = Some(wall.texture.clone());
        }
        // Read every frame rather than once: slots come live over the first
        // fraction of a second as their decoders produce a first frame, and a
        // set tuned to a layer that is still black would be a set that is
        // switched on and showing nothing.
        material.params.video_slots = wall.live_slots() as f32;
    }
}

/// Registers Terebi with the show.
pub struct TerebiPlugin;

impl Plugin for TerebiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terebi.wgsl");
        app.add_visual::<Terebi>().add_visual_systems::<Terebi, _>(
            Update,
            // After `Animate`, which is where the material is written: both
            // touch it, and taking it mutably twice in one frame costs an extra
            // change-detection flag for nothing.
            follow_video_wall.after(VisualSystems::Animate),
        );
    }
}
