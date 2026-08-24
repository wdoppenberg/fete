//! The numbers the original was tuned to, kept together and kept named.
//!
//! Everything here is transcribed from `Config.h`, `PresetManager.h` and the
//! pass configs of the C++ VJ-FÊTE that this visual reproduces. They are
//! constants rather than knobs on purpose: the look *is* this parameter set,
//! and the handful of things worth moving live on the macro knobs instead.

/// Half-extent of the simulated world. Positions live in `-50.0..50.0` on both
/// axes and wrap at the seam.
pub const WORLD_SIZE: f32 = 50.0;

/// The resolution the original rendered into before stretching to the screen.
///
/// Every pixel-denominated size below — point sizes, line thicknesses — is in
/// *these* pixels. Building the geometry in this space and scaling the mesh to
/// the window reproduces the original's fixed render target exactly, at any
/// window size and any aspect.
pub const REFERENCE: [f32; 2] = [1920.0, 1080.0];

// ---------------------------------------------------------------- populations

/// Big pulsing discs. The ones that carry the colour and grow the geometry.
pub const N_HEAVY: usize = 240;
/// Oriented triangles. The shoal that moves through everything else.
pub const N_LIGHT: usize = 460;
/// Small dim discs. Dust, mostly — but they steer the other two.
pub const N_SMALL: usize = 640;
/// Positions remembered behind each heavy boid.
pub const TRAIL_LEN: usize = 56;

// ------------------------------------------------------------------- geometry

pub const HEAVY_PT: f32 = 48.0;
pub const SMALL_PT: f32 = 14.4;
pub const TRAIL_PT: f32 = 16.0;
pub const TRI_BASE: f32 = 0.6;
/// How much smaller and dimmer a boid at the far plane is.
pub const DEPTH_FAR_SCALE: f32 = 0.1;
/// Per-boid hue offset, in turns. This is what stops a flock reading as one
/// flat colour: every boid is a slightly different shade of its role's hue.
pub const ANALOG_SPREAD: f32 = 0.24;

// ---------------------------------------------------------------- flocking

pub const CELL_SIZE: f32 = 1.0;
pub const NEIGHBOR_DIST: f32 = 6.0;

/// Where the inward steering starts. Beyond this the flock is pushed back
/// towards the middle, which is why the world never actually uses its seam.
pub const EDGE_R: f32 = WORLD_SIZE * 0.82;
pub const EDGE_PUSH: f32 = 6.0;
pub const EDGE_SWIRL: f32 = 0.0;

pub const JITTER_RAD_PER_S: f32 = 0.05;
/// Extra angular noise applied *in proportion to how ordered a boid already
/// is*. A flock that has locked into perfect alignment is boring, so the
/// simulation deliberately kicks the ordered ones hardest.
pub const ORDER_NOISE_MAX: f32 = 0.4;

pub const CENTER_ATTRACT_STRENGTH: f32 = 0.1;
pub const CENTER_SWIRL_STRENGTH: f32 = 0.6;

/// Force scale of an attractor/repeller field, falling off as `1/r²`.
pub const FIELD_STRENGTH: f32 = 60.0;

// ------------------------------------------------------- emergent geometry

pub const LINK_FADE_RANGE: f32 = 1.35;
pub const MIN_LINK_ALPHA: f32 = 0.05;
pub const LINK_DIST2: f32 = NEIGHBOR_DIST * NEIGHBOR_DIST * 0.14;
/// Links and triangles fade by this factor per frame once they stop being
/// re-promoted. It is a per-frame constant rather than a rate, exactly as in
/// the original — the lattice is a little livelier at high frame rates.
pub const LINK_DECAY: f32 = 0.85;
pub const TRI_DECAY: f32 = 0.85;
pub const LINK_THICK_PX: f32 = 1.4;
pub const MAX_LINKS: usize = 4000;
pub const MAX_TRIS: usize = 600;

// --------------------------------------------------------------- field rings

/// How long a field keeps pulling before it dies.
pub const FIELD_VISUAL_DUR: f32 = 8.0;
pub const FIELD_RING_DURATION: f32 = 0.60;
pub const FIELD_RING_FADE: f32 = 0.30;
pub const RING_ALPHA_CUTOFF: f32 = 0.02;
pub const RING_COUNT: usize = 6;
pub const RING_SPACING: f32 = 0.12;
pub const RING_BASE_R: f32 = 6.0;
pub const RING_THICK_PX: f32 = 1.0;
pub const RING_WAVE_PERIOD: f32 = 0.40;
pub const RING_WAVE_DELAY: f32 = 0.10;
pub const RING_BASE_ALPHA: f32 = 0.06;
pub const RING_WAVE_ALPHA: f32 = 0.60;
pub const RING_THICK_PULSE: f32 = 0.2;

// ----------------------------------------------------------------- flow field

pub const FLOW_COLS: usize = 32;
pub const FLOW_ROWS: usize = 18;
pub const FLOW_SAMPLE_RADIUS: f32 = CELL_SIZE;
pub const FLOW_LEN_FRAC: f32 = 0.26;
pub const FLOW_SHIFT_FRAC: f32 = 1.35;
pub const FLOW_PULL_SCALE: f32 = 0.35;
pub const FLOW_BASE_ALPHA: f32 = 2.0;
pub const FLOW_ALPHA_SCALE: f32 = 0.40;
pub const FLOW_LEN_SCALE: f32 = 2.4;
pub const FLOW_DENSITY_GAMMA: f32 = 1.1;
pub const FLOW_THICK_PX: f32 = 2.0;
pub const FLOW_SMOOTH_DIR: f32 = 0.09;
pub const FLOW_SMOOTH_ALPHA: f32 = 0.085;
pub const FLOW_SMOOTH_LEN: f32 = 0.090;
pub const FLOW_WIGGLE_FREQ: f32 = 1.8;
pub const FLOW_MAX_WIGGLE: f32 = 0.20;
pub const FLOW_WIGGLE_DECAY: f32 = 0.94;
/// Per-frame decay of the accumulation buffer the flow lines were drawn into,
/// and the gain it was composited back with.
pub const FLOW_TRAIL_DECAY: f32 = 0.95;
pub const FLOW_TRAIL_GAIN: f32 = 1.2;
/// How many past frames of each flow line are drawn to stand in for that
/// buffer. See [`crate::flow`] for why this is equivalent.
pub const FLOW_HISTORY: usize = 10;

// ------------------------------------------------------------------ kuramoto

pub const KURA_FREQ_MIN: f32 = 0.08;
pub const KURA_FREQ_MAX: f32 = 0.35;
pub const KURA_K_BASE: f32 = 0.08;
pub const KURA_AMP: f32 = 0.28;
pub const AMP_JITTER: f32 = 0.25;
pub const KURA_NOISE_STD: f32 = 0.8;
pub const KURA_COUPLING_RADIUS: f32 = 2.5;

// -------------------------------------------------------------------- trails

pub const TRAIL_LEN_MIN_FRAC: f32 = 0.16;
pub const TRAIL_LEN_MAX_FRAC: f32 = 0.9;
pub const TRAIL_FADE_GAMMA: f32 = 2.0;

/// The flocking weights and speed limits. These are the "BigGroups" baseline
/// the original boots into, before any preset key is pressed.
#[derive(Debug, Clone, Copy)]
pub struct FlockParams {
    pub coh_heavy: f32,
    pub ali_heavy: f32,
    pub coh_light: f32,
    pub ali_light: f32,
    pub coh_small: f32,
    pub ali_small: f32,

    pub min_speed_heavy: f32,
    pub max_speed_heavy: f32,
    pub min_speed_light: f32,
    pub max_speed_light: f32,
    pub min_speed_small: f32,
    pub max_speed_small: f32,

    /// Cross-flock influence. Heavy pulls light hard, light barely pulls back:
    /// that asymmetry is what makes the shoal look like it is *reacting* to
    /// the discs rather than negotiating with them.
    pub w_heavy_on_light: f32,
    pub w_light_on_heavy: f32,
    pub w_small_on_heavy: f32,
    pub w_small_on_light: f32,
    pub w_heavy_on_small: f32,
    pub w_light_on_small: f32,

    pub desired_sep: f32,
    pub sep_w: f32,

    /// How aligned two heavy boids must be before a link forms between them.
    pub align_dot_min: f32,

    pub flow_w_heavy: f32,
    pub flow_w_light: f32,
    pub flow_w_small: f32,
}

impl Default for FlockParams {
    fn default() -> Self {
        Self {
            coh_heavy: 0.4,
            ali_heavy: 0.6,
            coh_light: 0.6,
            ali_light: 0.4,
            coh_small: 0.8,
            ali_small: 0.3,

            min_speed_heavy: 2.0,
            max_speed_heavy: 6.0,
            min_speed_light: 2.0,
            max_speed_light: 8.0,
            min_speed_small: 1.0,
            max_speed_small: 7.0,

            w_heavy_on_light: 1.8,
            w_light_on_heavy: 0.8,
            w_small_on_heavy: 0.3,
            w_small_on_light: 0.4,
            w_heavy_on_small: 2.0,
            w_light_on_small: 1.4,

            desired_sep: 3.0,
            sep_w: 1.2,

            align_dot_min: 0.62,

            flow_w_heavy: 0.10,
            flow_w_light: 0.01,
            flow_w_small: 0.01,
        }
    }
}
