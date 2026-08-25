//! The flow field — the backdrop the whole piece sits on.
//!
//! A coarse grid samples the local heading of all three flocks and draws one
//! short line per cell. Alone that is a diagram; what makes it the backdrop is
//! that the original drew those lines into an accumulation buffer that faded
//! by five percent a frame, so each line left a smear of where it had recently
//! been pointing.
//!
//! There is no accumulation buffer here, and there does not need to be one.
//! The lines are anchored to fixed cells, so the only thing that buffer ever
//! held at a given pixel was that cell's own recent history — which means
//! keeping the last few states of each line and drawing them at the same decay
//! weights reproduces it exactly, for the cost of a few thousand triangles and
//! no feedback texture at all.
//!
//! The one thing that has to be reproduced by hand is saturation: the original
//! accumulated into an 8-bit target, so a line that sat still stopped getting
//! brighter once it hit white. [`FlowField::emit`] normalises each cell's
//! weights to sum to at most one for the same reason — without it, a still
//! flow line integrates to eight times white and blooms the frame away.

use crate::config::*;
use crate::flock::{Boid, Flocks, Grid};
use crate::math::{lerp, normalize2, wrap_delta};
use crate::render::MeshBuf;

const CELLS: usize = FLOW_COLS * FLOW_ROWS;

/// One drawn line, in world units.
#[derive(Debug, Clone, Copy, Default)]
struct Segment {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    alpha: f32,
}

#[derive(Debug)]
pub struct FlowField {
    /// Instantaneous heading and occupancy, rebuilt every frame.
    dir_x: Vec<f32>,
    dir_y: Vec<f32>,
    count: Vec<f32>,
    /// How recently a cell had anything in it; decays when it empties, and
    /// gates the wiggle so empty cells sit still.
    wiggle: Vec<f32>,

    /// Smoothed state. Everything the eye sees is one of these, because the
    /// raw per-frame values jitter badly at this sample radius.
    smooth_dir_x: Vec<f32>,
    smooth_dir_y: Vec<f32>,
    smooth_alpha: Vec<f32>,
    smooth_len: Vec<f32>,
    max_count: f32,

    /// `FLOW_HISTORY` past segments per cell, newest at `head`.
    history: Vec<Segment>,
    head: usize,
    /// Frames recorded so far, so a fresh visual does not smear out of nothing.
    filled: usize,
}

impl Default for FlowField {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowField {
    pub fn new() -> Self {
        Self {
            dir_x: vec![0.0; CELLS],
            dir_y: vec![-1.0; CELLS],
            count: vec![0.0; CELLS],
            wiggle: vec![0.0; CELLS],
            smooth_dir_x: vec![0.0; CELLS],
            smooth_dir_y: vec![1.0; CELLS],
            smooth_alpha: vec![0.0; CELLS],
            smooth_len: vec![0.0; CELLS],
            max_count: 1.0,
            history: vec![Segment::default(); CELLS * FLOW_HISTORY],
            head: 0,
            filled: 0,
        }
    }

    /// Resample the flocks and push one new segment per cell.
    ///
    /// `time` is in reference-tempo seconds, so the wiggle stays with the
    /// track.
    pub fn update(&mut self, flocks: &Flocks, params: &FlockParams, time: f32) {
        let cell_w = (2.0 * WORLD_SIZE) / FLOW_COLS as f32;
        let cell_h = (2.0 * WORLD_SIZE) / FLOW_ROWS as f32;
        let base_half_len = 0.5 * FLOW_LEN_FRAC * cell_w.min(cell_h);

        let mut raw_max = 1.0f32;

        for row in 0..FLOW_ROWS {
            for col in 0..FLOW_COLS {
                let index = row * FLOW_COLS + col;
                let cx = -WORLD_SIZE + (col as f32 + 0.5) * cell_w;
                let cy = -WORLD_SIZE + (row as f32 + 0.5) * cell_h;

                let mut vx = 0.0;
                let mut vy = 0.0;
                let mut count = 0.0;

                sample(
                    cx,
                    cy,
                    &flocks.heavy,
                    &flocks.grid_heavy,
                    params.flow_w_heavy,
                    &mut vx,
                    &mut vy,
                    &mut count,
                );
                sample(
                    cx,
                    cy,
                    &flocks.light,
                    &flocks.grid_light,
                    params.flow_w_light,
                    &mut vx,
                    &mut vy,
                    &mut count,
                );
                sample(
                    cx,
                    cy,
                    &flocks.small,
                    &flocks.grid_small,
                    params.flow_w_small,
                    &mut vx,
                    &mut vy,
                    &mut count,
                );

                if count == 0.0 {
                    vx = 0.0;
                    vy = -1.0;
                }
                normalize2(&mut vx, &mut vy);

                self.dir_x[index] = vx;
                self.dir_y[index] = vy;
                self.count[index] = count;
                raw_max = raw_max.max(count);

                self.wiggle[index] = if count > 0.0 {
                    1.0
                } else {
                    self.wiggle[index] * FLOW_WIGGLE_DECAY
                };
            }
        }

        self.max_count = lerp(self.max_count, raw_max, FLOW_SMOOTH_ALPHA);

        // Advance the ring buffer, then write this frame into it.
        self.head = (self.head + 1) % FLOW_HISTORY;
        self.filled = (self.filled + 1).min(FLOW_HISTORY);

        for row in 0..FLOW_ROWS {
            for col in 0..FLOW_COLS {
                let index = row * FLOW_COLS + col;
                let cx = -WORLD_SIZE + (col as f32 + 0.5) * cell_w;
                let cy = -WORLD_SIZE + (row as f32 + 0.5) * cell_h;

                // Flip guard: a heading that reverses would otherwise sweep the
                // line through ninety degrees on its way round, which reads as
                // a twitch. Lines have no arrowhead, so pointing the other way
                // is the same picture.
                let mut nx = self.dir_x[index];
                let mut ny = self.dir_y[index];
                if self.smooth_dir_x[index] * nx + self.smooth_dir_y[index] * ny < 0.0 {
                    nx = -nx;
                    ny = -ny;
                }
                self.smooth_dir_x[index] = lerp(self.smooth_dir_x[index], nx, FLOW_SMOOTH_DIR);
                self.smooth_dir_y[index] = lerp(self.smooth_dir_y[index], ny, FLOW_SMOOTH_DIR);
                let mut sx = self.smooth_dir_x[index];
                let mut sy = self.smooth_dir_y[index];
                normalize2(&mut sx, &mut sy);
                self.smooth_dir_x[index] = sx;
                self.smooth_dir_y[index] = sy;

                if self.wiggle[index] > 0.001 {
                    let angle = (time * FLOW_WIGGLE_FREQ + index as f32).sin()
                        * (self.wiggle[index] * FLOW_MAX_WIGGLE);
                    let (sin, cos) = angle.sin_cos();
                    let rx = sx * cos - sy * sin;
                    let ry = sx * sin + sy * cos;
                    sx = rx;
                    sy = ry;
                }

                let density = if self.max_count > 0.5 {
                    (self.count[index] / self.max_count).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let density = density.powf(FLOW_DENSITY_GAMMA);

                let instant_alpha = (FLOW_BASE_ALPHA * density).min(1.0) * FLOW_ALPHA_SCALE;
                let instant_len =
                    base_half_len * (1.0 + FLOW_PULL_SCALE * density) * FLOW_LEN_SCALE;

                self.smooth_alpha[index] =
                    lerp(self.smooth_alpha[index], instant_alpha, FLOW_SMOOTH_ALPHA);
                self.smooth_len[index] = lerp(self.smooth_len[index], instant_len, FLOW_SMOOTH_LEN);

                // Busy cells push their line forward along the flow, so a dense
                // stream reads as moving rather than as a static hatch.
                let shift = base_half_len * FLOW_SHIFT_FRAC * density;
                let half = self.smooth_len[index];

                self.history[index * FLOW_HISTORY + self.head] = Segment {
                    x0: cx - sx * half + sx * shift,
                    y0: cy - sy * half + sy * shift,
                    x1: cx + sx * half + sx * shift,
                    y1: cy + sy * half + sy * shift,
                    alpha: self.smooth_alpha[index],
                };
            }
        }
    }

    /// Draw the smear, oldest first.
    pub fn emit(&self, out: &mut MeshBuf, brightness: f32, budget: usize) {
        // What the tail beyond the kept history would have contributed, had the
        // buffer kept fading. Folded into the oldest sample rather than
        // dropped, so a still line reaches the same brightness it used to.
        let tail = FLOW_TRAIL_DECAY.powi(FLOW_HISTORY as i32 - 1) / (1.0 - FLOW_TRAIL_DECAY);

        let mut weights = [0.0f32; FLOW_HISTORY];
        let colour = [
            FLOW_TRAIL_GAIN * brightness,
            FLOW_TRAIL_GAIN * brightness,
            FLOW_TRAIL_GAIN * brightness,
        ];

        for cell in 0..CELLS {
            let mut total = 0.0;
            let kept = self.filled.min(budget.max(1));
            for (age, weight) in weights.iter_mut().enumerate().take(kept) {
                let slot = (self.head + FLOW_HISTORY - age) % FLOW_HISTORY;
                let alpha = self.history[cell * FLOW_HISTORY + slot].alpha;
                let decay = if age == FLOW_HISTORY - 1 {
                    tail
                } else {
                    FLOW_TRAIL_DECAY.powi(age as i32)
                };
                *weight = alpha * decay;
                total += *weight;
            }
            if total <= 1e-4 {
                continue;
            }
            let scale = if total > 1.0 { 1.0 / total } else { 1.0 };

            for age in (0..self.filled).rev() {
                let alpha = weights[age] * scale;
                if alpha < 0.004 {
                    continue;
                }
                let slot = (self.head + FLOW_HISTORY - age) % FLOW_HISTORY;
                let segment = self.history[cell * FLOW_HISTORY + slot];
                out.thick_line(
                    segment.x0,
                    segment.y0,
                    segment.x1,
                    segment.y1,
                    FLOW_THICK_PX,
                    colour,
                    alpha,
                );
            }
        }
    }
}

/// Accumulate the velocity of every boid within [`FLOW_SAMPLE_RADIUS`] of a
/// point. The radius is far smaller than a cell, so this is a probe at the
/// cell centre rather than an average over it.
fn sample(
    cx: f32,
    cy: f32,
    boids: &[Boid],
    grid: &Grid,
    weight: f32,
    vx: &mut f32,
    vy: &mut f32,
    count: &mut f32,
) {
    if weight <= 0.0 {
        return;
    }
    let radius2 = FLOW_SAMPLE_RADIUS * FLOW_SAMPLE_RADIUS;
    let (cgx, cgy) = Grid::cell_of(cx, cy);
    let cells = (FLOW_SAMPLE_RADIUS / CELL_SIZE).ceil() as i32;

    for gx in (cgx - cells)..=(cgx + cells) {
        for gy in (cgy - cells)..=(cgy + cells) {
            for &j in grid.at(gx, gy) {
                let boid = boids[j as usize];
                let dx = wrap_delta(boid.x - cx);
                let dy = wrap_delta(boid.y - cy);
                if dx * dx + dy * dy < radius2 {
                    *vx += boid.vx * weight;
                    *vy += boid.vy * weight;
                    *count += weight;
                }
            }
        }
    }
}
