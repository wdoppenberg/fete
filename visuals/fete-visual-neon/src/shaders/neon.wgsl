// Neon — a city seen from above, drifting past.
//
// The city is not modelled. It is a function: for any integer cell of an
// infinite grid, a hash says whether that cell is road or block and how tall
// the block is. Rays walk that grid one cell at a time (Amanatides–Woo) and
// test the one box in each cell they enter, so cost is proportional to how far
// a ray travels rather than to the size of the city — which can therefore be
// unbounded, and never repeats.
//
// The camera hovers high and looks down. That is the whole design decision. At
// street level you are looking at a handful of large boxes, and every hard edge
// and flat face is visible, which reads as low-poly geometry. From up here a
// building is a few pixels across, and what you see instead is what a city
// actually looks like from the air at night — a lit grid of streets with
// traffic moving along it, dissolving into haze long before it runs out.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, centered, knob, knob_range, pulse_every}
#import fete::noise::{hash11, hash12, noise2}
#import fete::palette::{palette, vignette, saturate_color}

struct NeonParams {
    // Distance travelled over the city. Integrated on the CPU so the speed
    // control changes where the camera goes from here rather than rewriting
    // where it has been.
    drift: f32,
    // Smoothed half-time beat energy.
    energy: f32,
    // Lateral drift.
    sway: f32,
    // Altitude.
    height: f32,
    // Look direction. `pitch` is strongly negative — we are looking down.
    yaw: f32,
    pitch: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: NeonParams;

// Nothing is taller than this, so a climbing ray above it can stop.
const MAX_HEIGHT: f32 = 9.0;
// Gap between a block and its cell boundary — the pavement.
const INSET: f32 = 0.10;
// Far enough that the haze has taken over completely before the march gives
// up, so the city has no visible edge.
const MAX_DIST: f32 = 78.0;
const MARCH_STEPS: i32 = 110;

struct CityHit {
    t: f32,
    cell: vec2<f32>,
    height: f32,
    normal: vec3<f32>,
    hit: bool,
}

fn sign_nz(x: f32) -> f32 {
    return select(1.0, -1.0, x < 0.0);
}

// Is this cell road rather than block?
//
// Two coprime spacings rather than one: a single period gives a chessboard,
// while 7 against 9 produces long runs of frontage broken by the occasional
// junction, which is what a street plan looks like from above.
fn is_street(cell: vec2<f32>) -> bool {
    let z = cell.y;
    if z - 7.0 * floor(z / 7.0) < 1.0 {
        return true;
    }
    let x = cell.x;
    if x - 9.0 * floor(x / 9.0) < 1.0 {
        return true;
    }
    return false;
}

fn height_at(cell: vec2<f32>) -> f32 {
    if is_street(cell) {
        return 0.0;
    }
    let h = hash12(cell * 0.7311 + 5.3);
    // Districts: a low-frequency field so towers cluster into a downtown
    // instead of being sprinkled evenly. Squaring the hash biases towards low
    // blocks with the occasional tower, which is the shape of a real skyline.
    let district = 0.5 + 0.3 * noise2(cell * 0.06);
    return mix(0.5, 8.5, h * h) * clamp(district, 0.25, 1.45);
}

fn box_span(ro: vec3<f32>, inv_rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec2<f32> {
    let t0 = (bmin - ro) * inv_rd;
    let t1 = (bmax - ro) * inv_rd;
    let a = min(t0, t1);
    let b = max(t0, t1);
    return vec2<f32>(max(max(a.x, a.y), a.z), min(min(b.x, b.y), b.z));
}

// Walk the grid, testing one box per cell entered.
fn march_city(ro: vec3<f32>, rd: vec3<f32>, max_steps: i32) -> CityHit {
    var out: CityHit;
    out.hit = false;
    out.t = MAX_DIST;
    out.cell = vec2<f32>(0.0);
    out.height = 0.0;
    out.normal = vec3<f32>(0.0, 1.0, 0.0);

    let safe = vec3<f32>(
        sign_nz(rd.x) * max(abs(rd.x), 1e-5),
        sign_nz(rd.y) * max(abs(rd.y), 1e-5),
        sign_nz(rd.z) * max(abs(rd.z), 1e-5),
    );
    let inv_rd = 1.0 / safe;

    var cell = floor(ro.xz);
    let step_dir = vec2<f32>(sign_nz(rd.x), sign_nz(rd.z));
    let inv_xz = vec2<f32>(inv_rd.x, inv_rd.z);
    var t_next = (cell + max(step_dir, vec2<f32>(0.0)) - ro.xz) * inv_xz;
    let t_delta = abs(inv_xz);

    for (var i = 0; i < max_steps; i++) {
        let h = height_at(cell);
        if h > 0.0 {
            let bmin = vec3<f32>(cell.x + INSET, 0.0, cell.y + INSET);
            let bmax = vec3<f32>(cell.x + 1.0 - INSET, h, cell.y + 1.0 - INSET);
            let span = box_span(ro, inv_rd, bmin, bmax);
            if span.x <= span.y && span.y > 0.0 && span.x < MAX_DIST {
                let t = max(span.x, 0.0);
                let p = ro + rd * t;
                let centre = (bmin + bmax) * 0.5;
                let extent = max((bmax - bmin) * 0.5, vec3<f32>(1e-5));
                let d = (p - centre) / extent;
                let ad = abs(d);

                var n = vec3<f32>(0.0, sign_nz(d.y), 0.0);
                if ad.x > ad.y && ad.x > ad.z {
                    n = vec3<f32>(sign_nz(d.x), 0.0, 0.0);
                } else if ad.z > ad.y {
                    n = vec3<f32>(0.0, 0.0, sign_nz(d.z));
                }

                out.hit = true;
                out.t = t;
                out.cell = cell;
                out.height = h;
                out.normal = n;
                return out;
            }
        }

        let t_edge = min(t_next.x, t_next.y);
        if t_edge > MAX_DIST {
            break;
        }
        if rd.y > 0.0 && ro.y + rd.y * t_edge > MAX_HEIGHT {
            break;
        }

        if t_next.x < t_next.y {
            cell.x += step_dir.x;
            t_next.x += t_delta.x;
        } else {
            cell.y += step_dir.y;
            t_next.y += t_delta.y;
        }
    }

    return out;
}

fn sky(rd: vec3<f32>) -> vec3<f32> {
    // Almost nothing, with the city's glow bleeding up off the horizon. A night
    // sky over a city is not black, but it is very close, and every unit of
    // light here is contrast taken from the lights below.
    let horizon = pow(1.0 - clamp(abs(rd.y) * 2.2, 0.0, 1.0), 3.0);
    let up = clamp(rd.y, 0.0, 1.0);
    return max(palette(globals, 0.12 + globals.seed), vec3<f32>(0.0)) * horizon * 0.05
        + vec3<f32>(0.003, 0.004, 0.008) * (1.0 - up * 0.7);
}

// Roadway: the glowing grid, and the traffic on it.
fn road(p: vec3<f32>, cell: vec2<f32>) -> vec3<f32> {
    if !is_street(cell) {
        // Pavement between blocks — dark, with a little spill from above.
        return vec3<f32>(0.012, 0.011, 0.014);
    }

    // Which way this street runs. Cells can be both, at a junction.
    let runs_z = (cell.y - 7.0 * floor(cell.y / 7.0)) < 1.0;
    let along = select(p.z, p.x, runs_z);
    let across = select(fract(p.x), fract(p.z), runs_z);

    // Sodium street lighting, brightest at the kerb.
    let kerb = 1.0 - smoothstep(0.0, 0.45, abs(across - 0.5));
    var lit = vec3<f32>(0.9, 0.62, 0.30) * (0.04 + kerb * 0.09);

    // Traffic: dashes of light sliding along the street, two streams running
    // opposite ways — warm one direction, cold the other, the way headlights
    // and tail lights separate from the air. This is the single detail that
    // stops an aerial city looking like a printed circuit board.
    let flow = globals.time * 1.5 + hash11(cell.x * 13.1 + cell.y * 7.7) * 10.0;
    let lane_a = pow(fract(along * 0.85 - flow), 24.0);
    let lane_b = pow(fract(along * 0.7 + flow * 0.8 + 0.5), 24.0);
    let side_a = 1.0 - smoothstep(0.06, 0.3, abs(across - 0.34));
    let side_b = 1.0 - smoothstep(0.06, 0.3, abs(across - 0.66));
    lit += vec3<f32>(1.0, 0.85, 0.6) * lane_a * side_a * 2.4;
    lit += vec3<f32>(0.5, 0.7, 1.0) * lane_b * side_b * 2.0;

    return lit;
}

// What a building emits. There is no light source in this scene — everything
// visible is something a surface is putting out itself.
fn facade(hit: CityHit, p: vec3<f32>, lit_fraction: f32) -> vec3<f32> {
    let seed = hit.cell.x * 31.7 + hit.cell.y * 57.13;

    if abs(hit.normal.y) > 0.5 {
        // Roofs. From above these are most of what you see, so they set the
        // black level of the whole picture and are kept almost unlit. A few
        // carry an aircraft beacon.
        var roof = vec3<f32>(0.010, 0.010, 0.013);

        // A blinking red lamp — a *point* on the roof, not the roof. Applying
        // it to the whole face lit every tenth rooftop as a flat red slab,
        // which from above read as coloured paper laid over the city.
        let beacon = step(0.9, hash12(hit.cell * 1.7 + 3.0));
        let blink = pow(0.5 + 0.5 * sin(globals.time * 2.0 + seed), 8.0);
        let lamp = 1.0 - smoothstep(0.03, 0.08, length(fract(vec2<f32>(p.x, p.z)) - vec2<f32>(0.5)));
        roof += vec3<f32>(1.0, 0.12, 0.08) * beacon * blink * lamp * 4.0;
        return roof;
    }

    var across = p.z;
    if abs(hit.normal.z) > 0.5 {
        across = p.x;
    }
    let wall = vec2<f32>(fract(across), p.y);

    var emitted = vec3<f32>(0.0);

    // --- windows -------------------------------------------------------------
    // Pitch fixed in world units rather than per building, so a tower and a low
    // block share the same floor spacing.
    let grid = vec2<f32>(wall.x * 6.0, wall.y * 5.0);
    let pane_id = floor(grid);
    let pane_uv = fract(grid);

    let lit = step(1.0 - lit_fraction, hash12(pane_id + seed));
    let inside = step(0.18, pane_uv.x) * step(pane_uv.x, 0.82)
        * step(0.22, pane_uv.y) * step(pane_uv.y, 0.78);

    // Dim but *saturated*. Mixing in grey to subdue them is the wrong move: it
    // desaturates rather than darkens, and a facade of pale grey squares is
    // exactly the washed-out look this is trying to avoid.
    let warmth = hash12(pane_id * 2.7 + seed + 1.0);
    let window_hue = 0.5 + warmth * 0.45 + globals.seed;
    var window = saturate_color(palette(globals, window_hue), 1.3);
    if warmth > 0.94 {
        window *= 0.5 + 0.7 * hash11(floor(globals.time * 8.0) + seed);
    }
    emitted += max(window, vec3<f32>(0.0)) * lit * inside * 0.5;

    // --- signage -------------------------------------------------------------
    // Saturated panels facing the street on a minority of frontages. From this
    // height no detail survives, so they are panels rather than script — but
    // they are the brightest points in the frame and still read as signs.
    // Sparse and narrow. An earlier version put a wide panel on most
    // frontages and the result was a field of primary-coloured rectangles —
    // the signs stopped being lights on a city and became the city.
    if hash12(hit.cell * 2.13 + 9.0) > 0.80 {
        let top = mix(0.8, max(hit.height - 0.4, 1.0), hash12(hit.cell * 5.9 + 4.0));
        let bottom = top - mix(0.35, 1.3, hash12(hit.cell * 7.1));
        let sx = 0.25 + 0.5 * hash12(hit.cell * 3.7 + 2.0);
        let w = 0.035 + 0.045 * hash12(hit.cell * 6.3 + 8.0);
        if wall.y < top && wall.y > bottom {
            let band = 1.0 - smoothstep(w, w + 0.05, abs(wall.x - sx));
            let flicker = select(
                1.0,
                0.55 + 0.45 * sin(globals.time * (4.0 + hash12(hit.cell) * 5.0) + seed),
                hash12(hit.cell * 11.3) > 0.85,
            );
            let sign_hue = hash12(hit.cell * 8.9 + 6.0) * 1.1 + globals.seed;
            // Saturation pushed only a little. Driving it hard turns small
            // bright patches into flat primaries, which at this scale read as
            // stickers laid on the city rather than as lights in it.
            let tint = saturate_color(palette(globals, sign_hue), 1.15);
            emitted += max(tint, vec3<f32>(0.0)) * band * flicker * 1.3;
        }
    }

    // --- street level --------------------------------------------------------
    // A band of light where a building meets the pavement. Small on screen from
    // here, but it outlines every block and is what makes the street plan
    // legible as a plan.
    let street_level = exp(-max(p.y - 0.1, 0.0) * 5.0);
    let shop_hue = 0.25 + hash12(hit.cell * 4.1 + 7.0) * 0.5 + globals.seed;
    emitted += max(saturate_color(palette(globals, shop_hue), 1.4), vec3<f32>(0.0))
        * street_level
        * 0.8;

    return emitted;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let screen = centered(uv, globals.resolution);

    let lit_fraction = knob_range(globals, 1u, 0.05, 0.28);
    let fog_density = knob_range(globals, 5u, 0.016, 0.075);

    // --- camera --------------------------------------------------------------
    let ro = vec3<f32>(params.sway, params.height, params.drift);
    let fwd = normalize(vec3<f32>(params.yaw, params.pitch, 1.0));
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    // A long lens. A wide angle from altitude exaggerates the perspective and
    // makes the city look like a small model directly below; a narrow one keeps
    // the blocks near-parallel and the city reading as large.
    let rd = normalize(fwd + right * screen.x * 0.85 - up * screen.y * 0.85);

    var color: vec3<f32>;
    var dist = MAX_DIST;

    let hit = march_city(ro, rd, MARCH_STEPS);
    var t_ground = MAX_DIST;
    if rd.y < -1e-4 {
        t_ground = -ro.y / rd.y;
    }

    if t_ground > 0.0 && t_ground < min(hit.t, MAX_DIST) {
        dist = t_ground;
        let gp = ro + rd * t_ground;
        color = road(gp, floor(gp.xz));
    } else if hit.hit {
        dist = hit.t;
        color = facade(hit, ro + rd * hit.t, lit_fraction);
    } else {
        dist = MAX_DIST;
        color = sky(rd);
    }

    // --- haze ----------------------------------------------------------------
    // Exponential, and the most important element in the frame after the lights
    // themselves. It does the work of depth, it hides the far edge of the march
    // so the city has no end, and the glow it accumulates over distance is what
    // a real city looks like from the air.
    let fog = max(palette(globals, 0.12 + globals.seed), vec3<f32>(0.0)) * 0.016
        + vec3<f32>(0.003, 0.004, 0.007);
    color = mix(color, fog, 1.0 - exp(-dist * fog_density));

    // --- beat ----------------------------------------------------------------
    // Half-time and shallow: the city brightens very slightly every other beat.
    let react = knob(globals, 7u);
    color *= 1.0 + pulse_every(globals, 2.0, 2.0) * params.energy * react * 0.2;

    // --- output --------------------------------------------------------------
    color *= knob_range(globals, 0u, 0.6, 2.4);
    color = saturate_color(color, 1.3);
    color *= vignette(uv, 0.5);
    color *= globals.intensity;

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
