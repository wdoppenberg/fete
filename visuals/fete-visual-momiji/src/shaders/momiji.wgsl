// Momiji — 紅葉. A lantern-lit temple courtyard at night, cherry blossoms
// framing the top corners, leaves whirling slowly down through all of it.
//
// The background is one illustrated frame, sampled with a cover fit so it
// fills any window or projector shape without letterbox bars — not marched,
// not modelled. The temple in it has no trees; the two canopies are drawn
// here instead, in the frame's own top corners, out of the same leaf shape
// the falling ones use — a static population, one grid cell owning one leaf
// exactly the way Kanban owns one glyph per cell, just never in motion.
//
// Every leaf in the scene — canopy or falling — reads the same instant of
// one clock and comes out the same colour, because a garden does not have
// half its blossoms on a different season than the rest of it. What differs
// leaf to leaf is only ever motion: where it sits, how wide it swings, how
// fast it turns — never what colour it is right now.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, centered, aspect_ratio, knob_range}
#import fete::noise::{hash12, hash22, TAU}
#import fete::palette::{vignette, saturate_color}

struct MomijiParams {
    wind: f32,
    gust: f32,
    hue_phase: f32,
    density: f32,
    canopy_reach: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: MomijiParams;
@group(2) @binding(2) var background_tex: texture_2d<f32>;
@group(2) @binding(3) var background_sampler: sampler;

// --- foliage colour ------------------------------------------------------

// Five keyframes, cyclic. Linear mix rather than oklab: these are small,
// saturated swatches where the muddy midpoint oklab exists to avoid is not
// the thing anyone would notice, and the extra pow()s are not worth paying
// for hundreds of leaves a frame.
//
// One call, `autumn(params.hue_phase)`, is the show's whole foliage palette
// at this instant — every leaf, canopy or falling, samples this exact call
// with no per-leaf offset, which is what keeps the population one colour.
fn autumn(t: f32) -> vec3<f32> {
    let purple = vec3<f32>(0.58, 0.12, 0.85);
    let red = vec3<f32>(1.00, 0.07, 0.12);
    let pink = vec3<f32>(1.00, 0.32, 0.66);
    let yellow = vec3<f32>(1.00, 0.88, 0.15);
    let orange = vec3<f32>(1.00, 0.48, 0.04);

    let x = fract(t) * 5.0;
    let f = smoothstep(0.0, 1.0, fract(x));
    var c = mix(purple, red, f);
    c = select(c, mix(red, pink, f), x >= 1.0);
    c = select(c, mix(pink, yellow, f), x >= 2.0);
    c = select(c, mix(yellow, orange, f), x >= 3.0);
    c = select(c, mix(orange, purple, f), x >= 4.0);
    return c;
}

// --- the leaf shape ------------------------------------------------------

// One rotated, tapered ellipse. Both the static canopy and the falling
// overlay draw this and only this — a tree is a great many of the same
// falling leaf, just not falling.
fn leaf_shape(local: vec2<f32>, angle: f32, radius: f32) -> f32 {
    let s = sin(angle);
    let c = cos(angle);
    let r = vec2<f32>(c * local.x - s * local.y, s * local.x + c * local.y);
    let dist = length(r * vec2<f32>(1.0, 2.2)) - radius;
    return smoothstep(radius * 0.22, -radius * 0.22, dist);
}

fn leaves_over(base: vec4<f32>, layer: vec4<f32>) -> vec4<f32> {
    let a = layer.a + base.a * (1.0 - layer.a);
    if a < 1e-4 {
        return vec4<f32>(0.0);
    }
    let rgb = (layer.rgb * layer.a + base.rgb * base.a * (1.0 - layer.a)) / a;
    return vec4<f32>(rgb, a);
}

// --- canopy: a static population in each top corner ------------------------

// One leaf, fixed in place — jittered off its cell centre once, by hash,
// and never touched again. No neighbour search is needed the way the
// falling leaves need one: nothing here ever moves far enough to leave the
// cell that owns it.
//
// Reference size for a full-scale cluster (`scale` 1.0). Every cluster in
// `canopy` passes its own `scale`, which shrinks the leaf, the packing grid
// and the reach together — the same photograph of the same tree, smaller —
// rather than shrinking just one of them, which would read as fewer or as
// finer rather than as further away.
const CANOPY_LEAF_R: f32 = 0.020;
// Cell area sets leaf count; halving it doubles leaves per unit area, so the
// grid shrinks by sqrt(2), not by 2.
const CANOPY_CELL: f32 = 0.01004;
// Every cluster reads its vertical extent through this: not a squashed
// grid, just a boundary that answers "how far" differently on the vertical
// axis, so a circle of presence becomes a horizontally lying ellipse.
const CANOPY_VSQUASH: f32 = 1.5;

fn canopy_leaf(
    screen: vec2<f32>,
    id: vec2<f32>,
    cell_size: vec2<f32>,
    seed: f32,
    center_pos: vec2<f32>,
    reach: f32,
    leaf_r: f32,
) -> vec4<f32> {
    let h = hash22(id + seed);
    let center = (id + 0.5) * cell_size + (h - 0.5) * cell_size * 0.6;

    // Denser and more certain right at the middle, thinning out — by both
    // shrinking and by dropping leaves outright — toward `reach`, so the
    // edge of the canopy is a fade rather than a drawn line. Weighting the
    // vertical offset by `CANOPY_VSQUASH` before measuring it is what makes
    // that edge an ellipse: the same physical distance counts for more
    // vertically, so presence reaches zero sooner going up or down than it
    // does going sideways.
    let offset = center - center_pos;
    let d = length(vec2<f32>(offset.x, offset.y * CANOPY_VSQUASH)) / max(reach, 1e-3);
    let presence = smoothstep(1.05, 0.0, d);
    if hash12(id * 3.7 + seed + 50.0) > presence {
        return vec4<f32>(0.0);
    }

    let angle = hash12(id * 4.1 + seed + 9.0) * TAU;
    let r = leaf_r * mix(0.75, 1.0, h.x) * mix(0.6, 1.0, presence);
    let alpha = leaf_shape(screen - center, angle, r);

    let color = autumn(params.hue_phase) * 1.4;
    return vec4<f32>(color, alpha);
}

// A leaf this densely packed reaches well past its own cell, so the search
// is 7x7 rather than the 3x3 a sparser, static field could get away with —
// margin for `leaf_r` staying fixed while the cell it's packed into keeps
// shrinking.
fn canopy_cluster(
    screen: vec2<f32>,
    center_pos: vec2<f32>,
    reach: f32,
    seed: f32,
    scale: f32,
) -> vec4<f32> {
    let cell_size = vec2<f32>(CANOPY_CELL * scale);
    let leaf_r = CANOPY_LEAF_R * scale;
    let base_id = floor(screen / cell_size);
    var out = vec4<f32>(0.0);
    for (var dy = -3; dy <= 3; dy++) {
        for (var dx = -3; dx <= 3; dx++) {
            let id = base_id + vec2<f32>(f32(dx), f32(dy));
            out = leaves_over(out, canopy_leaf(screen, id, cell_size, seed, center_pos, reach, leaf_r));
        }
    }
    return out;
}

fn canopy(screen: vec2<f32>) -> vec4<f32> {
    let aspect = aspect_ratio(globals.resolution);
    let reach = params.canopy_reach;

    let top_left = vec2<f32>(-0.5 * aspect, -0.5);
    var out = canopy_cluster(screen, top_left, reach, 5001.0, 1.0);

    // Right corner, shrunk by 1.5x — reach and leaf size scale down together
    // so it reads as the same tree further off, not a thinner one.
    let right_scale = 1.0 / 1.5;
    let top_right = vec2<f32>(0.5 * aspect, -0.5);
    out = leaves_over(out, canopy_cluster(screen, top_right, reach * right_scale, 6007.0, right_scale));

    // A third cluster, standing on its own in the middle distance: started a
    // third of the way in from the left edge at vertical centre, then
    // nudged an eighth of the frame further left and down, shrunk by 4.5x
    // (the 3x it started at, pulled in another 1.5x).
    let mid_scale = 1.0 / 4.5;
    let mid_pos = vec2<f32>(-0.5 * aspect + aspect / 3.0 - aspect / 8.0, 0.0 + 0.125);
    out = leaves_over(out, canopy_cluster(screen, mid_pos, reach * mid_scale, 7001.0, mid_scale));

    // A fourth, tucked in just past the middle cluster's edge — a sixteenth
    // of the frame to the right and down from it, half its size.
    let small_scale = mid_scale / 2.0;
    let small_pos = mid_pos + vec2<f32>(aspect / 16.0, 1.0 / 16.0);
    out = leaves_over(out, canopy_cluster(screen, small_pos, reach * small_scale, 8009.0, small_scale));

    return out;
}

// --- falling leaves (screen-space overlay) ----------------------------------

// One leaf, belonging to cell `id`. Its loop swings wide enough that it can
// visibly leave its own cell, so this only computes where it is; `leaf_layer`
// below is what decides which cells are worth asking.
fn leaf_at(
    screen: vec2<f32>,
    id: vec2<f32>,
    cell_size: vec2<f32>,
    layer_seed: f32,
    speed: f32,
    size: f32,
) -> vec4<f32> {
    if hash12(id + layer_seed) >= params.density {
        return vec4<f32>(0.0);
    }

    let h = hash22(id + layer_seed);
    let h2 = hash12(id * 1.7 + layer_seed + 3.0);
    let h3 = hash12(id * 2.9 + layer_seed + 17.0);

    // Fall progress: a sawtooth in time, seeded per cell, wrapped so a leaf
    // that reaches the bottom of its cell re-enters at the top of the next
    // one down — an infinite fall built from no state at all. Slow: this is
    // a leaf drifting, not dropping.
    let fall_speed = speed * mix(0.7, 1.3, h.x);
    let phase = fract(globals.time * fall_speed / cell_size.y + h.y);

    // Whirl: a wide loop around the fall line, driven by its own clock
    // rather than by `phase`, so a slow fall does not also mean a slow
    // spin — the two read as different things and are, on purpose, no
    // longer coupled. Radius, rate and handedness are all drawn per leaf.
    let orbit_radius = mix(0.55, 1.5, h3) * cell_size.x;
    let orbit_rate = mix(0.9, 2.4, h2);
    let orbit_dir = select(-1.0, 1.0, h.x > 0.5);
    let theta = globals.time * orbit_rate * orbit_dir + h.y * TAU;
    let orbit = vec2<f32>(cos(theta), sin(theta) * 0.6) * orbit_radius
        * (1.0 + params.wind * 0.25 + params.gust * 0.3);

    // Flutter: the small, fast secondary wobble, unchanged in character.
    let flutter_amp = (0.05 + params.gust * 0.06) * cell_size.x;
    let flutter = sin(phase * TAU * 5.1 + h.y * TAU) * flutter_amp;

    let local_x = orbit.x + flutter;
    // `screen.y` grows downward (`centered` puts uv 0 at the top), so a rising
    // phase has to grow `local_y` too: top of the cell at phase 0, bottom at 1.
    let local_y = (phase - 0.5) * cell_size.y + orbit.y;

    let local = screen - (id * cell_size + cell_size * 0.5) - vec2<f32>(local_x, local_y);

    // Tumbling rotation, coupled to the whirl so the leaf visibly turns
    // edge-on as it swings the way a real one autorotates.
    let angle = phase * TAU * (2.0 + h.y * 2.0) + theta * 0.5;
    let leaf_r = size * mix(0.7, 1.0, h.x);
    let alpha = leaf_shape(local, angle, leaf_r);

    // Fades in falling out of the canopy line and out again near the ground —
    // a leaf materialising mid-air or vanishing mid-fall is the one thing
    // that would give the trick away.
    let fade = smoothstep(0.0, 0.08, phase) * smoothstep(1.0, 0.9, phase);

    // Pushed well past 1.0 on purpose: the stage's HDR bloom is what turns
    // that into a glow instead of a clipped, flat swatch.
    let color = autumn(params.hue_phase) * 2.4;
    return vec4<f32>(color, alpha * fade);
}

// A wide whirl means a leaf can wander well into a neighbouring cell's
// screen area, so every pixel asks the 5x5 block of cells around it rather
// than just the one it sits in. `orbit_radius` is capped under two cell
// widths for exactly this reason: a 5x5 search covers about 2.5 cells from
// centre, and nothing here swings further than that.
fn leaf_layer(
    screen: vec2<f32>,
    cell_size: vec2<f32>,
    layer_seed: f32,
    speed: f32,
    size: f32,
) -> vec4<f32> {
    let base_id = floor(screen / cell_size);
    var out = vec4<f32>(0.0);
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let id = base_id + vec2<f32>(f32(dx), f32(dy));
            out = leaves_over(out, leaf_at(screen, id, cell_size, layer_seed, speed, size));
        }
    }
    return out;
}

fn falling_leaves(screen: vec2<f32>) -> vec4<f32> {
    var out = vec4<f32>(0.0);
    // Roughly a quarter of the old pace: the ask was a slow drift, not a
    // shower, and the whirl above is what carries the motion now.
    let base_speed = 0.025 + params.wind * 0.008;

    let far_cell = vec2<f32>(0.032, 0.055);
    out = leaves_over(out, leaf_layer(screen, far_cell, 401.0, base_speed * 0.6, 0.010));
#if LEAF_LAYERS >= 2
    let mid_cell = vec2<f32>(0.06, 0.10);
    out = leaves_over(out, leaf_layer(screen, mid_cell, 809.0, base_speed, 0.017));
#endif
#if LEAF_LAYERS >= 3
    let near_cell = vec2<f32>(0.11, 0.16);
    out = leaves_over(out, leaf_layer(screen, near_cell, 1201.0, base_speed * 1.6, 0.030));
#endif
    return out;
}

// --- main --------------------------------------------------------------

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let screen = centered(uv, globals.resolution);

    // Cover fit: scale the shorter screen axis up so the image fills the
    // frame, cropping the other — the same rule a phone wallpaper picker
    // uses, and for the same reason: no letterbox bars on a projector whose
    // aspect nobody guaranteed in advance.
    let img_size = vec2<f32>(textureDimensions(background_tex));
    let img_aspect = img_size.x / max(img_size.y, 1.0);
    let screen_aspect = aspect_ratio(globals.resolution);
    var scale: vec2<f32>;
    if screen_aspect > img_aspect {
        scale = vec2<f32>(1.0, img_aspect / screen_aspect);
    } else {
        scale = vec2<f32>(screen_aspect / img_aspect, 1.0);
    }
    let bg_uv = (uv - vec2<f32>(0.5)) * scale + vec2<f32>(0.5);
    var color = textureSample(background_tex, background_sampler, bg_uv).rgb;

    // Brightness knob, applied to the background only — the leaves already
    // read fine at their own values and this should not blow them to white.
    color *= knob_range(globals, 0u, 1.0, 1.8);

    // Canopy first, falling leaves last: the corners read as trees standing
    // behind the ones actually in the air.
    let corner_leaves = canopy(screen);
    color = mix(color, corner_leaves.rgb, corner_leaves.a);

    let leaf = falling_leaves(screen);
    color = mix(color, leaf.rgb, leaf.a);

    color = saturate_color(color, 1.1);
    color *= vignette(uv, 0.35);
    color *= globals.intensity;

    return vec4<f32>(color, 1.0);
}
