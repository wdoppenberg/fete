//! Turning the simulation into triangles.
//!
//! The original drew six passes of `GL_POINTS`, `GL_TRIANGLES` and thick line
//! quads into a fixed 1920×1080 target and stretched that to the screen. This
//! builds the same geometry into meshes, in the same order, in the same
//! reference-pixel space — the mesh entity carries the stretch, so a window of
//! any size or aspect gets exactly the picture the original would have shown
//! on that display.
//!
//! Point sprites become quads. Everything the original's vertex shader
//! computed per point — the depth scale, the size pulse, the brightness
//! flicker, the fade towards white at the top of a pulse — is computed here
//! instead, because the geometry is rebuilt every frame anyway and doing it on
//! the CPU keeps the shader to a falloff curve.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};

use crate::config::*;
use crate::flock::{Boid, Field};
use crate::math::lerp;

/// World units to reference pixels, per axis.
///
/// Deliberately not equal: the original projected `x / WORLD_SIZE` straight to
/// clip space on both axes, so the square world is stretched to fill whatever
/// shape the output is. Correcting it here would be a different picture.
pub const SCALE_X: f32 = REFERENCE[0] * 0.5 / WORLD_SIZE;
pub const SCALE_Y: f32 = REFERENCE[1] * 0.5 / WORLD_SIZE;

const PULSE_HZ: f32 = 0.25;
const PULSE_AMP: f32 = 0.45;

/// Vertex soup, built fresh each frame and handed to a [`Mesh`].
#[derive(Debug, Default)]
pub struct MeshBuf {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshBuf {
    pub fn clear(&mut self) {
        self.positions.clear();
        self.colors.clear();
        self.uvs.clear();
        self.indices.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// An axis-aligned quad, `size` reference pixels across, centred on a world
    /// position. `uv` runs `0..1` across it — the stand-in for `gl_PointCoord`.
    pub fn sprite(&mut self, x: f32, y: f32, size: f32, color: [f32; 3], alpha: f32) {
        let half = size * 0.5;
        let cx = x * SCALE_X;
        let cy = y * SCALE_Y;
        let base = self.positions.len() as u32;

        for (dx, dy, u, v) in [
            (-half, -half, 0.0, 0.0),
            (half, -half, 1.0, 0.0),
            (-half, half, 0.0, 1.0),
            (half, half, 1.0, 1.0),
        ] {
            self.positions.push([cx + dx, cy + dy, 0.0]);
            self.colors.push([color[0], color[1], color[2], alpha]);
            self.uvs.push([u, v]);
        }

        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }

    /// A line of fixed pixel width between two world positions.
    pub fn thick_line(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        color: [f32; 3],
        alpha: f32,
    ) {
        let ax = x0 * SCALE_X;
        let ay = y0 * SCALE_Y;
        let bx = x1 * SCALE_X;
        let by = y1 * SCALE_Y;

        let dx = bx - ax;
        let dy = by - ay;
        let len = dx.hypot(dy);
        if len < 1e-6 {
            return;
        }

        let half = thickness * 0.5;
        let nx = -dy / len * half;
        let ny = dx / len * half;

        let base = self.positions.len() as u32;
        for (px, py) in [
            (ax - nx, ay - ny),
            (ax + nx, ay + ny),
            (bx - nx, by - ny),
            (bx + nx, by + ny),
        ] {
            self.positions.push([px, py, 0.0]);
            self.colors.push([color[0], color[1], color[2], alpha]);
            self.uvs.push([0.5, 0.5]);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }

    /// A filled triangle between three world positions.
    pub fn triangle(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], color: [f32; 3], alpha: f32) {
        let base = self.positions.len() as u32;
        for point in [a, b, c] {
            self.positions
                .push([point[0] * SCALE_X, point[1] * SCALE_Y, 0.0]);
            self.colors.push([color[0], color[1], color[2], alpha]);
            self.uvs.push([0.5, 0.5]);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// A mesh of the right shape with nothing visible in it, so an entity can
    /// be spawned before there is anything to draw.
    pub fn empty_mesh() -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            // Kept in the main world too: the whole mesh is rewritten from the
            // CPU every frame, so it has to stay reachable through `Assets`.
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
        mesh.insert_indices(Indices::U32(Vec::new()));
        MeshBuf::default().write_into(&mut mesh);
        mesh
    }

    /// A layer can legitimately have nothing in it for a frame — no live
    /// fields, no links yet — and a zero-length vertex buffer makes the render
    /// world's slab allocator complain about a freed allocation every frame.
    /// One invisible degenerate triangle costs nothing and keeps it quiet.
    fn ensure_nonempty(&mut self) {
        if !self.indices.is_empty() {
            return;
        }
        for _ in 0..3 {
            self.positions.push([0.0; 3]);
            self.colors.push([0.0; 4]);
            self.uvs.push([0.5, 0.5]);
        }
        self.indices.extend_from_slice(&[0, 1, 2]);
    }

    /// Swap the buffers into a mesh, taking its previous ones back.
    ///
    /// Swapping rather than assigning is the point: the mesh is rebuilt from
    /// scratch sixty times a second, and handing the old allocations back means
    /// steady-state frames do no allocation at all.
    pub fn write_into(&mut self, mesh: &mut Mesh) {
        self.ensure_nonempty();

        match mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(values)) => {
                core::mem::swap(values, &mut self.positions)
            }
            _ => mesh.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                core::mem::take(&mut self.positions),
            ),
        }
        match mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(values)) => {
                core::mem::swap(values, &mut self.colors)
            }
            _ => mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, core::mem::take(&mut self.colors)),
        }
        match mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(values)) => {
                core::mem::swap(values, &mut self.uvs)
            }
            _ => mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, core::mem::take(&mut self.uvs)),
        }
        match mesh.indices_mut() {
            Some(Indices::U32(values)) => core::mem::swap(values, &mut self.indices),
            _ => mesh.insert_indices(Indices::U32(core::mem::take(&mut self.indices))),
        }
    }
}

/// One firefly: the original's point-sprite vertex and fragment stages, minus
/// the falloff curve the shader still owns.
///
/// `alpha_in` is the trail fade — exactly `1.0` marks a head, and heads are the
/// only things that pulse. `pulse` is the oscillator's size multiplier.
#[allow(clippy::too_many_arguments)]
pub fn emit_firefly(
    out: &mut MeshBuf,
    x: f32,
    y: f32,
    depth: f32,
    alpha_in: f32,
    pulse: f32,
    color: [f32; 3],
    point_size: f32,
    time: f32,
) {
    let head = alpha_in >= 0.999;

    // Size: smaller with depth, tapering along a trail, breathing slowly if it
    // is a head. The per-boid phase comes from its depth, which is fixed for
    // the whole run and therefore a free stable seed.
    let depth_scale = 1.0 - (1.0 - DEPTH_FAR_SCALE) * depth;
    let size_pulse = if head {
        1.0 + PULSE_AMP * (std::f32::consts::TAU * PULSE_HZ * time + depth * 23.0).sin()
    } else {
        1.0
    };
    let size = point_size * depth_scale * alpha_in * size_pulse * pulse;

    // Brightness: a much faster flicker than the size pulse, and out of phase
    // per boid, so the field twinkles rather than beating.
    let flicker = if head {
        0.6 + 0.4 * (time * 4.0 + depth * 20.0).sin()
    } else {
        1.0
    };
    let depth_fade = lerp(1.0, DEPTH_FAR_SCALE, depth);
    let alpha = alpha_in * flicker * depth_fade;

    // The shape's peak is a little over 1.0, so anything under this cannot
    // reach the shader's own discard threshold.
    if size < 0.35 || alpha < 0.006 {
        return;
    }

    let color = if head {
        // At the top of the flicker a head goes white. This is what stops two
        // hundred saturated discs reading as a flat colour field.
        let t = ((flicker - 0.6) / 0.4).clamp(0.0, 1.0);
        [
            lerp(color[0], 1.0, t),
            lerp(color[1], 1.0, t),
            lerp(color[2], 1.0, t),
        ]
    } else {
        color
    };

    out.sprite(x, y, size, color, alpha);
}

/// Discs, one per boid. `pulses` is the oscillator bank, or `None` for a flock
/// that does not have one.
pub fn emit_discs(
    out: &mut MeshBuf,
    boids: &[Boid],
    colors: &[[f32; 3]],
    pulses: Option<&[f32]>,
    point_size: f32,
    time: f32,
) {
    for (i, boid) in boids.iter().enumerate() {
        let pulse = pulses.and_then(|p| p.get(i)).copied().unwrap_or(1.0);
        emit_firefly(
            out, boid.x, boid.y, boid.z, 1.0, pulse, colors[i], point_size, time,
        );
    }
}

/// Triangles pointing along their velocity, one per light boid.
pub fn emit_light_triangles(out: &mut MeshBuf, boids: &[Boid], colors: &[[f32; 3]], time: f32) {
    for (i, boid) in boids.iter().enumerate() {
        // Same flicker as the discs, so the two flocks feel like one system.
        let pulse = 0.6 + 0.4 * (0.5 + 0.5 * (time * 4.0 + boid.z * 20.0).sin());
        let depth_scale = 1.0 - (1.0 - DEPTH_FAR_SCALE) * boid.z;
        let alpha = pulse * depth_scale;

        let speed = boid.vx.hypot(boid.vy) + 1e-6;
        let dx = boid.vx / speed;
        let dy = boid.vy / speed;
        let px = -dy;
        let py = dx;
        let s = TRI_BASE * depth_scale;

        out.triangle(
            [boid.x + dx * s, boid.y + dy * s],
            [
                boid.x - dx * s * 0.5 + px * s * 0.5,
                boid.y - dy * s * 0.5 + py * s * 0.5,
            ],
            [
                boid.x - dx * s * 0.5 - px * s * 0.5,
                boid.y - dy * s * 0.5 - py * s * 0.5,
            ],
            colors[i],
            alpha,
        );
    }
}

/// Concentric ripples around each live field.
///
/// A short burst, under a second long: six rings on a travelling wave that
/// fires when the field lands and is gone well before the field stops pulling.
/// The disturbance in the flock outlasts the announcement of it.
pub fn emit_rings(out: &mut MeshBuf, fields: &[Field], brightness: f32) {
    let colour = [brightness, brightness, brightness];

    for field in fields {
        for k in 0..RING_COUNT {
            let radius = RING_BASE_R * (1.0 + k as f32 * RING_SPACING);

            let phase =
                field.age / RING_WAVE_PERIOD - k as f32 * (RING_WAVE_DELAY / RING_WAVE_PERIOD);
            let crest = tri01(phase);

            let alpha = (RING_BASE_ALPHA + RING_WAVE_ALPHA * crest).clamp(0.0, 1.0)
                * ring_strength(field.age);
            if alpha < RING_ALPHA_CUTOFF {
                continue;
            }

            let thickness = RING_THICK_PX * (1.0 + RING_THICK_PULSE * crest);
            let segments = ring_segments(radius, fields.len());

            emit_ring(
                out, field.x, field.y, radius, alpha, thickness, colour, segments,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_ring(
    out: &mut MeshBuf,
    cx: f32,
    cy: f32,
    radius: f32,
    alpha: f32,
    thickness: f32,
    colour: [f32; 3],
    segments: usize,
) {
    let segments = segments.max(3);
    let step = std::f32::consts::TAU / segments as f32;
    let (sin, cos) = step.sin_cos();

    let mut vx = radius;
    let mut vy = 0.0f32;
    let mut px = cx + vx;
    let mut py = cy + vy;

    for _ in 0..segments {
        let nx = vx * cos - vy * sin;
        let ny = vx * sin + vy * cos;
        vx = nx;
        vy = ny;

        let qx = cx + vx;
        let qy = cy + vy;
        out.thick_line(px, py, qx, qy, thickness, colour, alpha);
        px = qx;
        py = qy;
    }
}

/// Segment count by radius, degraded when a lot of fields are live at once.
fn ring_segments(radius: f32, field_count: usize) -> usize {
    let mut segments = if radius < 1.0 {
        32
    } else if radius < 3.0 {
        64
    } else {
        96
    };
    if field_count > 16 {
        segments = (segments / 2).max(24);
    }
    if field_count > 28 {
        segments = (segments / 2).max(16);
    }
    segments
}

/// Full strength for the ring duration, then a short linear fade out.
fn ring_strength(age: f32) -> f32 {
    if age < 0.0 {
        return 0.0;
    }
    if age < FIELD_RING_DURATION {
        return 1.0;
    }
    (1.0 - (age - FIELD_RING_DURATION) / FIELD_RING_FADE).max(0.0)
}

/// Triangle wave on `0.0..1.0`.
fn tri01(x: f32) -> f32 {
    let f = x - x.floor();
    1.0 - (f * 2.0 - 1.0).abs()
}
