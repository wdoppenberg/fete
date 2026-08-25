//! The lattice that forms between heavy boids that agree with each other.
//!
//! Two conditions have to hold at once: close enough, and travelling in nearly
//! the same direction. Proximity alone gives a mesh that follows the crowd and
//! reads as noise; the alignment term means a link only appears where the
//! flock has locally agreed on a heading, so the geometry marks structure
//! rather than density.
//!
//! Links persist. Once promoted they fade by [`LINK_DECAY`] per frame unless
//! the pair re-qualifies, so the lattice trails a little behind the motion
//! instead of flickering on and off with it.

use std::collections::HashMap;

use crate::config::*;
use crate::flock::{Boid, Grid};
use crate::math::{smoothstep, wrap_delta};
use crate::render::MeshBuf;

#[derive(Debug, Clone, Copy)]
struct Link {
    i: u32,
    j: u32,
    alpha: f32,
}

#[derive(Debug, Clone, Copy)]
struct Tri {
    a: u32,
    b: u32,
    c: u32,
    alpha: f32,
}

#[derive(Debug, Default)]
pub struct Emergent {
    links: Vec<Link>,
    tris: Vec<Tri>,
    link_index: HashMap<(u32, u32), usize>,
    tri_index: HashMap<(u32, u32, u32), usize>,
    /// Scratch for the two most recent neighbours of the boid being scanned.
    fresh: Vec<(u32, f32)>,
}

impl Emergent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.links.clear();
        self.tris.clear();
        self.link_index.clear();
        self.tri_index.clear();
    }

    pub fn update(&mut self, heavy: &[Boid], grid: &Grid, align_dot_min: f32) {
        for link in &mut self.links {
            link.alpha *= LINK_DECAY;
        }
        self.links.retain(|link| link.alpha >= MIN_LINK_ALPHA);
        self.link_index.clear();
        for (slot, link) in self.links.iter().enumerate() {
            self.link_index.insert((link.i, link.j), slot);
        }

        for tri in &mut self.tris {
            tri.alpha *= TRI_DECAY;
        }
        self.tris.retain(|tri| tri.alpha >= MIN_LINK_ALPHA);
        self.tri_index.clear();
        for (slot, tri) in self.tris.iter().enumerate() {
            self.tri_index.insert((tri.a, tri.b, tri.c), slot);
        }

        let near = LINK_DIST2.sqrt();
        let far = near * LINK_FADE_RANGE;

        for i in 0..heavy.len() {
            let a = heavy[i];
            let (cgx, cgy) = Grid::cell_of(a.x, a.y);
            self.fresh.clear();

            for gx in (cgx - 1)..=(cgx + 1) {
                for gy in (cgy - 1)..=(cgy + 1) {
                    for &j in grid.at(gx, gy) {
                        // Only the upper triangle, so a pair is considered once.
                        if (j as usize) <= i {
                            continue;
                        }
                        let b = heavy[j as usize];
                        let dx = wrap_delta(b.x - a.x);
                        let dy = wrap_delta(b.y - a.y);
                        let dist = dx.hypot(dy);

                        let by_distance = 1.0 - smoothstep(near, far, dist);
                        if by_distance <= 0.0 {
                            continue;
                        }

                        let dot = (a.vx * b.vx + a.vy * b.vy)
                            / (a.vx.hypot(a.vy) * b.vx.hypot(b.vy) + 1e-6);
                        let by_alignment = smoothstep(align_dot_min, 1.0, dot);
                        let alpha = by_distance * by_alignment;

                        self.promote_link(i as u32, j, alpha);
                        self.fresh.push((j, alpha));
                    }
                }
            }

            // One triangle per boid at most, from the first two neighbours the
            // scan happened to find. Filling every triple would turn the
            // lattice into a solid sheet.
            if self.fresh.len() >= 2 {
                let alpha = self.fresh[0].1.min(self.fresh[1].1);
                let (b, c) = (self.fresh[0].0, self.fresh[1].0);
                self.promote_tri(i as u32, b, c, alpha);
            }
        }
    }

    fn promote_link(&mut self, i: u32, j: u32, alpha: f32) {
        if alpha < MIN_LINK_ALPHA {
            return;
        }
        let key = if i > j { (j, i) } else { (i, j) };
        match self.link_index.get(&key) {
            Some(&slot) => {
                let existing = &mut self.links[slot];
                existing.alpha = existing.alpha.max(alpha);
            }
            None => {
                self.link_index.insert(key, self.links.len());
                self.links.push(Link {
                    i: key.0,
                    j: key.1,
                    alpha,
                });
            }
        }
    }

    fn promote_tri(&mut self, a: u32, b: u32, c: u32, alpha: f32) {
        if alpha < MIN_LINK_ALPHA {
            return;
        }
        let mut key = [a, b, c];
        key.sort_unstable();
        let key = (key[0], key[1], key[2]);
        match self.tri_index.get(&key) {
            Some(&slot) => {
                let existing = &mut self.tris[slot];
                existing.alpha = existing.alpha.max(alpha);
            }
            None => {
                self.tri_index.insert(key, self.tris.len());
                self.tris.push(Tri {
                    a: key.0,
                    b: key.1,
                    c: key.2,
                    alpha,
                });
            }
        }
    }

    /// White lines first, then the filled triangles over them.
    pub fn emit(&self, out: &mut MeshBuf, heavy: &[Boid], brightness: f32, budget: KuraBudget) {
        let colour = [brightness, brightness, brightness];

        for link in self.links.iter().take(budget.links) {
            let a = heavy[link.i as usize];
            let b = heavy[link.j as usize];
            // Localised around A, so a link across the seam is drawn as the
            // short way round rather than straight across the frame.
            out.thick_line(
                a.x,
                a.y,
                a.x + wrap_delta(b.x - a.x),
                a.y + wrap_delta(b.y - a.y),
                LINK_THICK_PX,
                colour,
                link.alpha,
            );
        }

        for tri in self.tris.iter().take(budget.tris) {
            let a = heavy[tri.a as usize];
            let b = heavy[tri.b as usize];
            let c = heavy[tri.c as usize];
            out.triangle(
                [a.x, a.y],
                [a.x + wrap_delta(b.x - a.x), a.y + wrap_delta(b.y - a.y)],
                [a.x + wrap_delta(c.x - a.x), a.y + wrap_delta(c.y - a.y)],
                colour,
                tri.alpha,
            );
        }
    }
}
