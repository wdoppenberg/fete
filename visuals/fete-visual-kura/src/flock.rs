//! Three interacting flocks on a wrapped square.
//!
//! A direct port of the original's `FlockSystem`, including the parts that are
//! arguably accidents. The two that matter, because the motion looks wrong
//! without them:
//!
//! * **Every flock is integrated twice per frame.** The original steps six
//!   ordered pairs — heavy against light, light against heavy, small against
//!   heavy, heavy against small, small against light, light against small — so
//!   each flock advances twice with the full timestep, seeing a different
//!   partner each time. Stepping once against a merged neighbourhood is not
//!   the same simulation and does not look the same.
//! * **Neighbour cells are not wrapped.** Distances are measured across the
//!   seam but the spatial grid is not consulted across it, so boids at the very
//!   edge have fewer neighbours than they should. In practice the edge field
//!   turns them back long before it shows.

use crate::config::*;
use crate::math::Rng;
use crate::math::{clamp_speed, smoothstep, wrap_delta, wrap_pos};

/// A boid. `z` is a fixed depth in `0.0..1.0` used only for how large and how
/// bright it draws — there is no third spatial dimension.
#[derive(Debug, Clone, Copy, Default)]
pub struct Boid {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
}

/// Cells per axis. The world is `100` units wide with unit cells, and a
/// position of exactly `+WORLD_SIZE` lands in one cell past the end.
pub const GRID_DIM: usize = 101;

/// A bucketed uniform grid over the world.
///
/// A flat `Vec` rather than the original's hash map: the world is bounded and
/// small, so there is nothing to hash.
#[derive(Debug, Clone)]
pub struct Grid {
    cells: Vec<Vec<u32>>,
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Grid {
    pub fn new() -> Self {
        Self {
            cells: vec![Vec::new(); GRID_DIM * GRID_DIM],
        }
    }

    /// Which cell a position falls in. May be out of range at the seam.
    pub fn cell_of(x: f32, y: f32) -> (i32, i32) {
        (
            ((x + WORLD_SIZE) / CELL_SIZE).floor() as i32,
            ((y + WORLD_SIZE) / CELL_SIZE).floor() as i32,
        )
    }

    pub fn rebuild(&mut self, boids: &[Boid]) {
        for cell in &mut self.cells {
            cell.clear();
        }
        for (index, boid) in boids.iter().enumerate() {
            let (gx, gy) = Self::cell_of(boid.x, boid.y);
            if let Some(cell) = self.cell_mut(gx, gy) {
                cell.push(index as u32);
            }
        }
    }

    fn cell_mut(&mut self, gx: i32, gy: i32) -> Option<&mut Vec<u32>> {
        let index = Self::index(gx, gy)?;
        self.cells.get_mut(index)
    }

    /// Occupants of a cell. Out-of-range cells are empty rather than wrapped —
    /// see the note at the top of this module.
    pub fn at(&self, gx: i32, gy: i32) -> &[u32] {
        match Self::index(gx, gy) {
            Some(index) => &self.cells[index],
            None => &[],
        }
    }

    fn index(gx: i32, gy: i32) -> Option<usize> {
        let dim = GRID_DIM as i32;
        if gx < 0 || gy < 0 || gx >= dim || gy >= dim {
            return None;
        }
        Some(gy as usize * GRID_DIM + gx as usize)
    }
}

/// A transient attractor or repeller dropped into the world.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// `1.0` pulls, `-1.0` pushes.
    pub sign: f32,
    pub x: f32,
    pub y: f32,
    /// Seconds since it was dropped.
    pub age: f32,
}

impl Field {
    /// Fields outlive their pull by the ring fade, so the last ripple can
    /// finish leaving.
    pub fn expired(&self) -> bool {
        self.age > FIELD_VISUAL_DUR + FIELD_RING_FADE
    }
}

/// The whole simulated population, its spatial index and its disturbances.
#[derive(Debug)]
pub struct Flocks {
    pub heavy: Vec<Boid>,
    pub light: Vec<Boid>,
    pub small: Vec<Boid>,

    pub grid_heavy: Grid,
    pub grid_light: Grid,
    pub grid_small: Grid,

    pub fields: Vec<Field>,
    /// A slow pull towards the middle, with a swirl on it. Off by default.
    pub center_attractor: bool,

    /// Double buffer for the integration step, kept to avoid reallocating.
    scratch: Vec<Boid>,
}

impl Flocks {
    pub fn new(rng: &mut Rng, params: &FlockParams) -> Self {
        let mut flocks = Self {
            heavy: vec![Boid::default(); N_HEAVY],
            light: vec![Boid::default(); N_LIGHT],
            small: vec![Boid::default(); N_SMALL],
            grid_heavy: Grid::new(),
            grid_light: Grid::new(),
            grid_small: Grid::new(),
            fields: Vec::new(),
            center_attractor: false,
            scratch: Vec::new(),
        };
        flocks.scatter(rng, params);
        flocks
    }

    /// Random positions, random depths, random headings at the flock's speed.
    pub fn scatter(&mut self, rng: &mut Rng, params: &FlockParams) {
        for boid in self
            .heavy
            .iter_mut()
            .chain(&mut self.light)
            .chain(&mut self.small)
        {
            boid.x = rng.range(-WORLD_SIZE, WORLD_SIZE);
            boid.y = rng.range(-WORLD_SIZE, WORLD_SIZE);
            boid.z = rng.unit();
        }
        self.randomize_velocities(rng, params);
    }

    pub fn randomize_velocities(&mut self, rng: &mut Rng, params: &FlockParams) {
        let spin = |boids: &mut Vec<Boid>, min: f32, max: f32, rng: &mut Rng| {
            for boid in boids {
                let angle = rng.range(0.0, std::f32::consts::TAU);
                let speed = rng.range(min, max);
                boid.vx = angle.cos() * speed;
                boid.vy = angle.sin() * speed;
            }
        };
        spin(
            &mut self.heavy,
            params.min_speed_heavy,
            params.max_speed_heavy,
            rng,
        );
        spin(
            &mut self.light,
            params.min_speed_light,
            params.max_speed_light,
            rng,
        );
        spin(
            &mut self.small,
            params.min_speed_small,
            params.max_speed_small,
            rng,
        );
    }

    pub fn rebuild_grids(&mut self) {
        self.grid_heavy.rebuild(&self.heavy);
        self.grid_light.rebuild(&self.light);
        self.grid_small.rebuild(&self.small);
    }

    /// Drop a field somewhere at random. `sign` is `1.0` to attract.
    pub fn add_field(&mut self, rng: &mut Rng, sign: f32) {
        self.fields.push(Field {
            sign,
            x: rng.range(-WORLD_SIZE, WORLD_SIZE),
            y: rng.range(-WORLD_SIZE, WORLD_SIZE),
            age: 0.0,
        });
    }

    fn age_fields(&mut self, dt: f32) {
        for field in &mut self.fields {
            field.age += dt;
        }
        self.fields.retain(|field| !field.expired());
    }

    /// One frame of simulation.
    pub fn step(&mut self, dt: f32, params: &FlockParams, rng: &mut Rng) {
        self.age_fields(dt);
        self.rebuild_grids();

        // The six ordered pairs, in the original's order. Each call reads one
        // flock's grid and its partner's, and writes the first flock back —
        // which is why every flock advances twice per frame.
        let heavy = Weights {
            coh: params.coh_heavy,
            ali: params.ali_heavy,
            min_speed: params.min_speed_heavy,
            max_speed: params.max_speed_heavy,
            ..Weights::base(params)
        };
        let light = Weights {
            coh: params.coh_light,
            ali: params.ali_light,
            min_speed: params.min_speed_light,
            max_speed: params.max_speed_light,
            ..Weights::base(params)
        };
        let small = Weights {
            coh: params.coh_small,
            ali: params.ali_small,
            min_speed: params.min_speed_small,
            max_speed: params.max_speed_small,
            ..Weights::base(params)
        };

        let center_attractor = self.center_attractor;

        macro_rules! pair {
            ($me:ident, $me_grid:ident, $other:ident, $other_grid:ident, $w:expr, $cross:expr) => {
                step_flock(
                    &mut self.$me,
                    &mut self.scratch,
                    &self.$me_grid,
                    &self.$other,
                    &self.$other_grid,
                    &External {
                        fields: &self.fields,
                        center_attractor,
                    },
                    &Weights {
                        cross: $cross,
                        ..$w
                    },
                    dt,
                    rng,
                );
            };
        }

        pair!(
            heavy,
            grid_heavy,
            light,
            grid_light,
            heavy,
            params.w_light_on_heavy
        );
        pair!(
            light,
            grid_light,
            heavy,
            grid_heavy,
            light,
            params.w_heavy_on_light
        );
        pair!(
            small,
            grid_small,
            heavy,
            grid_heavy,
            small,
            params.w_heavy_on_small
        );
        pair!(
            heavy,
            grid_heavy,
            small,
            grid_small,
            heavy,
            params.w_small_on_heavy
        );
        pair!(
            small,
            grid_small,
            light,
            grid_light,
            small,
            params.w_light_on_small
        );
        pair!(
            light,
            grid_light,
            small,
            grid_small,
            light,
            params.w_small_on_light
        );
    }
}

/// The subset of [`FlockParams`] one ordered pair needs.
#[derive(Debug, Clone, Copy)]
struct Weights {
    coh: f32,
    ali: f32,
    min_speed: f32,
    max_speed: f32,
    /// How much the partner flock counts for, relative to one's own.
    cross: f32,
    desired_sep: f32,
    sep_w: f32,
}

impl Weights {
    fn base(params: &FlockParams) -> Self {
        Self {
            coh: 0.0,
            ali: 0.0,
            min_speed: 0.0,
            max_speed: 0.0,
            cross: 0.0,
            desired_sep: params.desired_sep,
            sep_w: params.sep_w,
        }
    }
}

/// Forces that come from outside the flock.
struct External<'a> {
    fields: &'a [Field],
    center_attractor: bool,
}

/// Advance one flock by one timestep against itself and one partner.
fn step_flock(
    me: &mut Vec<Boid>,
    scratch: &mut Vec<Boid>,
    me_grid: &Grid,
    other: &[Boid],
    other_grid: &Grid,
    external: &External,
    w: &Weights,
    dt: f32,
    rng: &mut Rng,
) {
    let neigh2 = NEIGHBOR_DIST * NEIGHBOR_DIST;
    let sep2 = w.desired_sep * w.desired_sep;

    scratch.clear();
    scratch.reserve(me.len());

    for i in 0..me.len() {
        let boid = me[i];
        let (cgx, cgy) = Grid::cell_of(boid.x, boid.y);

        let mut sep_x = 0.0f32;
        let mut sep_y = 0.0f32;
        let mut align_x = 0.0f32;
        let mut align_y = 0.0f32;
        let mut coh_x = 0.0f32;
        let mut coh_y = 0.0f32;
        let mut weight = 0.0f32;

        for gx in (cgx - 1)..=(cgx + 1) {
            for gy in (cgy - 1)..=(cgy + 1) {
                for &j in me_grid.at(gx, gy) {
                    if j as usize == i {
                        continue;
                    }
                    let nb = me[j as usize];
                    let dx = wrap_delta(nb.x - boid.x);
                    let dy = wrap_delta(nb.y - boid.y);
                    let d2 = dx * dx + dy * dy;

                    if d2 < neigh2 {
                        // Inverse-square weighting: a boid two units away
                        // counts four times one at four units, which is what
                        // keeps clusters tight without a hard radius.
                        let weight_ij = 1.0 / (d2 + 1e-3);
                        align_x += weight_ij * nb.vx;
                        align_y += weight_ij * nb.vy;
                        coh_x += weight_ij * dx;
                        coh_y += weight_ij * dy;
                        weight += weight_ij;
                    }
                    if d2 < sep2 && d2 > 0.0 {
                        let inv = 1.0 / (d2 + 1e-6).sqrt();
                        sep_x -= dx * inv;
                        sep_y -= dy * inv;
                    }
                }

                for &j in other_grid.at(gx, gy) {
                    let nb = other[j as usize];
                    let dx = wrap_delta(nb.x - boid.x);
                    let dy = wrap_delta(nb.y - boid.y);
                    let d2 = dx * dx + dy * dy;

                    if d2 < neigh2 {
                        let weight_ij = w.cross / (d2 + 1e-3);
                        align_x += weight_ij * nb.vx;
                        align_y += weight_ij * nb.vy;
                        coh_x += weight_ij * dx;
                        coh_y += weight_ij * dy;
                        weight += weight_ij;
                    }
                    if d2 < sep2 && d2 > 0.0 {
                        let inv = 1.0 / (d2 + 1e-6).sqrt();
                        sep_x -= dx * inv;
                        sep_y -= dy * inv;
                    }
                }
            }
        }

        let mut vx = boid.vx;
        let mut vy = boid.vy;
        let prev_vx = vx;
        let prev_vy = vy;

        let mut mean_x = 0.0;
        let mut mean_y = 0.0;
        let mut acc_x = 0.0;
        let mut acc_y = 0.0;
        let mut coh_fx = 0.0;
        let mut coh_fy = 0.0;

        if weight > 0.0 {
            mean_x = align_x / weight;
            mean_y = align_y / weight;

            // Steer towards the neighbourhood *heading* at full speed rather
            // than towards its average velocity: matching speeds as well as
            // headings collapses a flock into a rigid block.
            let mean_len = mean_x.hypot(mean_y) + 1e-6;
            acc_x = w.ali * (mean_x / mean_len * w.max_speed - vx);
            acc_y = w.ali * (mean_y / mean_len * w.max_speed - vy);

            coh_fx = w.coh * (coh_x / weight);
            coh_fy = w.coh * (coh_y / weight);
        }

        vx += (acc_x + coh_fx + sep_x * w.sep_w) * dt;
        vy += (acc_y + coh_fy + sep_y * w.sep_w) * dt;

        // Noise proportional to local order. The more a boid already agrees
        // with its neighbours, the harder it is kicked — which is what stops
        // the flock freezing into a crystal.
        let mut order = 0.0;
        if weight > 0.0 {
            let mlen = mean_x.hypot(mean_y);
            let vlen = prev_vx.hypot(prev_vy);
            order = ((mean_x / (mlen + 1e-6)) * (prev_vx / (vlen + 1e-6))
                + (mean_y / (mlen + 1e-6)) * (prev_vy / (vlen + 1e-6)))
                .clamp(0.0, 1.0);
        }

        if vx.hypot(vy) > 0.1 {
            let extra = ORDER_NOISE_MAX * smoothstep(0.6, 1.0, order);
            let angle = (JITTER_RAD_PER_S + extra) * rng.normal() * dt;
            let (sin, cos) = angle.sin_cos();
            let rx = vx * cos - vy * sin;
            let ry = vx * sin + vy * cos;
            vx = rx;
            vy = ry;
        }

        apply_external_forces(external, &boid, &mut vx, &mut vy, dt);

        let (edge_x, edge_y) = edge_field(boid.x, boid.y);
        vx += edge_x * dt;
        vy += edge_y * dt;

        clamp_speed(&mut vx, &mut vy, w.min_speed, w.max_speed);

        scratch.push(Boid {
            x: wrap_pos(boid.x + vx * dt),
            y: wrap_pos(boid.y + vy * dt),
            z: boid.z,
            vx,
            vy,
        });
    }

    // Swap rather than copy, so neither buffer is ever reallocated.
    std::mem::swap(me, scratch);
}

fn apply_external_forces(external: &External, boid: &Boid, vx: &mut f32, vy: &mut f32, dt: f32) {
    for field in external.fields {
        let dx = wrap_delta(field.x - boid.x);
        let dy = wrap_delta(field.y - boid.y);
        let d2 = dx * dx + dy * dy + 0.01;
        *vx += field.sign * dx / d2 * FIELD_STRENGTH * dt;
        *vy += field.sign * dy / d2 * FIELD_STRENGTH * dt;
    }

    if external.center_attractor {
        let dx = wrap_delta(-boid.x);
        let dy = wrap_delta(-boid.y);
        *vx += dx * CENTER_ATTRACT_STRENGTH * dt;
        *vy += dy * CENTER_ATTRACT_STRENGTH * dt;

        let dist = (dx * dx + dy * dy).sqrt() + 1e-6;
        *vx += -dy / dist * CENTER_SWIRL_STRENGTH * dt;
        *vy += dx / dist * CENTER_SWIRL_STRENGTH * dt;
    }
}

/// Inward acceleration once a boid strays past [`EDGE_R`].
///
/// Measured on the infinity norm, so the containment is a square with the same
/// shape as the frame rather than a circle inscribed in it — the flock uses
/// the corners of the screen.
fn edge_field(x: f32, y: f32) -> (f32, f32) {
    let ax = x.abs();
    let ay = y.abs();
    let r_inf = ax.max(ay);

    if r_inf < EDGE_R {
        return (0.0, 0.0);
    }

    let denom = (WORLD_SIZE - EDGE_R).max(1e-6);
    let t = ((r_inf - EDGE_R) / denom).clamp(0.0, 1.0);

    let bx = x.clamp(-EDGE_R, EDGE_R);
    let by = y.clamp(-EDGE_R, EDGE_R);
    let mut nx = x - bx;
    let mut ny = y - by;
    let nlen = nx.hypot(ny);

    if nlen > 1e-6 {
        nx /= nlen;
        ny /= nlen;
    } else if ax > ay {
        nx = if x >= 0.0 { 1.0 } else { -1.0 };
        ny = 0.0;
    } else {
        ny = if y >= 0.0 { 1.0 } else { -1.0 };
        nx = 0.0;
    }

    (
        (-nx * EDGE_PUSH + ny * EDGE_SWIRL) * t,
        (-ny * EDGE_PUSH - nx * EDGE_SWIRL) * t,
    )
}
