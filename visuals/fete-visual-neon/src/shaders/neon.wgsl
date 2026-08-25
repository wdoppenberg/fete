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
    // Traffic signals, one street to an entry: (travel, brake, queue, unused).
    // Integrated on the CPU as a ring of coupled oscillators — see `Signals` in
    // `lib.rs`. A shader has no memory, so a signal cycle derived here could
    // only ever be a periodic function of time; state coming in from outside is
    // what lets the signals influence each other.
    signals: array<vec4<f32>, 32>,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: NeonParams;

// Nothing is taller than this, so a climbing ray above it can stop.
const MAX_HEIGHT: f32 = 9.0;
// Gap between a block and its cell boundary — the pavement.
const INSET: f32 = 0.10;
// Far enough that the haze has taken over completely before the march gives
// up, so the city has no visible edge. Shortened by the quality tier, with the
// fog pulled in to match — see `REFERENCE_DIST`.
const MAX_DIST: f32 = f32(#{MAX_DIST});
// The draw distance this visual was tuned at. A cheaper tier draws less city,
// and the fog is thickened so the haze still closes before the city runs out —
// otherwise the buildings stop while the roads and the haze carry on past them,
// which reads as a bald patch rather than as a smaller city.
//
// The square root is the whole trick. Thickening the fog by the full ratio
// makes the haze reach the same value at the new limit, but it also doubles it
// everywhere nearer, and the city vanishes into grey. Half the exponent closes
// the far field most of the way while leaving the near field close to where it
// was tuned, which is the half of the frame anyone is actually looking at.
const REFERENCE_DIST: f32 = 78.0;
// Longest loop in the repo, and the reason this visual is the second most
// expensive: cells are one unit across, so a near-horizontal ray genuinely
// burns every step reaching MAX_DIST. Fed by the quality tier — see
// `Neon::specialize`. Shortening it pulls the far haze closer rather than
// putting an edge on the city, because the march gives up into fog either way.
const MARCH_STEPS: i32 = #{MARCH_STEPS};
// A long lens. A wide angle from altitude exaggerates the perspective and makes
// the city look like a small model directly below; a narrow one keeps the blocks
// near-parallel and the city reading as large.
const LENS: f32 = 0.85;
// How much further the lens throws red than blue, as a fraction of its focal
// length. About two pixels at the edge of the frame — a car lamp is only a few
// across, so this is already enough to fringe one, and much more separates it
// into three coloured dots and reads as a fault rather than as a lens.
const ABERRATION: f32 = 0.0020;
// Streets this far apart share a traffic signal. Over a hundred blocks, so the
// haze has closed in long before the repeat could be seen.
const SIGNALS_PER_AXIS: f32 = 16.0;

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

// What the air over the city looks like, and the single most load-bearing
// colour in the frame: at this view distance most of the picture is seen
// through some depth of it.
//
// Kept close to neutral. Haze at night is the city's own light scattered back,
// which is a dim warm grey — taking the palette's hue at full strength instead
// laid a saturated wash over the entire far field, and a wash that covers
// everything reads as a filter on the lens rather than as distance. A fifth of
// the saturation is enough that it is not grey.
fn haze_color() -> vec3<f32> {
    let tint = max(palette(globals, 0.12 + globals.seed), vec3<f32>(0.0));
    return saturate_color(tint, 0.20) * 0.010;
}

// Ray through a pixel, for a lens of the given focal length.
fn view_ray(fwd: vec3<f32>, right: vec3<f32>, up: vec3<f32>, screen: vec2<f32>, lens: f32) -> vec3<f32> {
    return normalize(fwd + right * screen.x * lens - up * screen.y * lens);
}

// Where a ray meets the ground, shaded.
fn road_under(ro: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    let p = ro + dir * (-ro.y / min(dir.y, -1e-4));
    return road(p, floor(p.xz));
}

fn sky(rd: vec3<f32>) -> vec3<f32> {
    // Almost nothing, with the city's glow bleeding up off the horizon. A night
    // sky over a city is not black, but it is very close, and every unit of
    // light here is contrast taken from the lights below.
    let horizon = pow(1.0 - clamp(abs(rd.y) * 2.2, 0.0, 1.0), 3.0);
    let up = clamp(rd.y, 0.0, 1.0);
    return haze_color() * horizon * 3.0
        + vec3<f32>(0.003, 0.004, 0.008) * (1.0 - up * 0.7);
}

// One lane of traffic: a lattice of slots sliding past the pixel, and what the
// cars in them put on the road.
//
// Nothing is simulated. `slots` is position along the street in a frame that
// moves with the traffic, so a car keeps its slot index for good and every hash
// of that index is stable for as long as the car is on screen — which is what
// lets a car have a size and a place in its slot at all.
struct Lane {
    // Distance along the street, in slots.
    slots: f32,
    // Distance from this lane's centre line, in blocks. Signed, but nothing
    // here is asymmetric across the lane, so only its magnitude is used.
    across: f32,
    // Slots to the block, so that beam lengths can be written in blocks.
    density: f32,
    // Separates the two directions, and one street from the next.
    salt: f32,
    occupancy: f32,
    platoon_k: f32,
    platoon_phase: f32,
    // How far the traffic has closed up, 0..1.
    queue: f32,
}

// A car's lamps, and separately the light they spill on the road. Kept apart
// because only the first is small and hot enough to be worth dispersing through
// the lens.
struct CarLight {
    lamp: f32,
    spill: f32,
}

fn traffic_lane(lane: Lane) -> CarLight {
    // Which platoon this pixel is looking at. Platoons are counted in the frame
    // that moves with the traffic, so a car belongs to one for good.
    let group = floor(lane.slots * lane.platoon_k + lane.platoon_phase);
    // Where its front is — the stop line it will be held at.
    let front = (group + 0.72 - lane.platoon_phase) / lane.platoon_k;

    // Queueing, as a warp of the road rather than a move of each car. Standing
    // traffic closes up towards the front of its platoon; the same cars at
    // speed are strung out over several times the distance. Moving each car
    // would carry it out of reach of the two slots a pixel looks at, and the
    // lattice is what gives a car its identity, so the *coordinate* contracts
    // about the front instead and the lattice is left alone.
    let close = 1.0 - 0.42 * lane.queue;
    let s = front + (lane.slots - front) / close;
    let base = floor(s);

    var out: CarLight;
    out.lamp = 0.0;
    out.spill = 0.0;

    // A pixel is lit by the car in its own slot and by the one ahead throwing
    // its beam back over the slot boundary, so two slots are enough.
    for (var k = 0; k < 2; k++) {
        let slot = base + f32(k);

        // Platoons. Traffic leaves a signal in a group and arrives at the next
        // one as a group, so a street is a run of cars and then empty road.
        //
        // Measured against this pixel's own platoon and deliberately not
        // wrapped: the contraction reaches back over the platoon behind, and
        // letting the window run off its ends is what stops that platoon being
        // drawn a second time in the wrong place.
        let win = slot * lane.platoon_k + lane.platoon_phase - group;
        let gate = smoothstep(0.0, 0.10, win) * (1.0 - smoothstep(0.58, 0.80, win));

        // Whether this slot carries a car at all. The gaps this leaves are the
        // point: an unbroken train of dashes reads as a moving texture, and it
        // is only once the stream has holes in it that the lights read as
        // separate vehicles.
        if hash11(slot * 1.13 + lane.salt) >= lane.occupancy * gate {
            continue;
        }

        // Where in its slot the car sits — enough jitter to break the lattice,
        // not enough to let it cross into a neighbour's slot and be missed.
        let jitter = (hash11(slot * 2.71 + lane.salt + 4.3) - 0.5) * 0.6;

        // Blocks ahead of the car as they land on screen: out of the warped
        // slot coordinate, back through the contraction, into blocks. Working
        // in the units the road is actually drawn in is what keeps a car the
        // same size stopped as it is moving.
        let ahead = (s - (slot + 0.5 + jitter)) * close / lane.density;
        let side = abs(lane.across);

        // One car in fifteen is a truck. Bigger lamps and a longer reach, so
        // that the eye has a single vehicle to follow down the street rather
        // than a run of identical marks.
        let big = step(0.93, hash11(slot * 5.17 + lane.salt + 9.1));

        // The lamps. Small, hot, and the only part of a car with a hard edge —
        // at this range this dot and the little it throws is the whole vehicle.
        let lamp_l = mix(0.026, 0.040, big);
        let lamp_w = mix(0.022, 0.034, big);
        let lamp = exp(-(ahead * ahead) / (lamp_l * lamp_l) - (side * side) / (lamp_w * lamp_w));

        // The beam, thrown forward and spreading as it goes. This is what a car
        // actually contributes to a night street from above — a short wedge of
        // lit tarmac in front of it, not a mark on the road. A flat band was
        // the previous version and it read as a printed dash, because a light
        // with no falloff and no spread is not a light, it is a rectangle.
        //
        // Kept shorter than the gap between cars. Reaching further is what a
        // headlight really does, but on a street this dense every beam then
        // laps the car in front and the lane fuses into one continuous lit
        // ribbon — which loses both the cars and the black the frame is built
        // on. Seen from directly above, most of a beam is not pointed at you
        // anyway.
        let reach = max(ahead, 0.0);
        let spread = 0.028 + reach * 0.20;
        let beam = exp(-reach * mix(8.0, 6.0, big))
            * exp(-(side * side) / (spread * spread))
            * (1.0 - smoothstep(0.0, 0.03, -ahead))
            * 0.45;

        // And the pool immediately under it: light off the tarmac, going every
        // way at once. Tight and weak, and it is what stops the beam looking
        // like it was cut out of paper.
        let pool = exp(-(ahead * ahead) / 0.020 - (side * side) / 0.007) * 0.22;

        out.lamp += lamp * mix(1.0, 1.5, big);
        out.spill += (beam + pool) * mix(1.0, 1.4, big);
    }

    return out;
}

// Roadway: the glowing grid, and the traffic on it.
fn road(p: vec3<f32>, cell: vec2<f32>) -> vec3<f32> {
    if !is_street(cell) {
        // Pavement between blocks — dark, with a little spill from above.
        return vec3<f32>(0.012, 0.011, 0.014);
    }

    // Which way this street runs. Cells can be both, at a junction.
    let runs_x = (cell.y - 7.0 * floor(cell.y / 7.0)) < 1.0;
    let along = select(p.z, p.x, runs_x);
    let across = select(fract(p.x), fract(p.z), runs_x);

    // A street's identity is the coordinate it does *not* vary along. Seeding
    // from the cell instead — which changes every block as you move down the
    // street — gives each block its own independent phase, and the traffic
    // comes out as an even scatter of dots that all move in lockstep rather
    // than as streams with anywhere to go.
    let street_id = select(cell.x * 0.73 + 41.7, cell.y * 1.31, runs_x) + globals.seed * 17.0;

    // Class. Most streets are quiet and a few are arterials — faster, fuller,
    // better lit. Without this every street carries identical traffic and the
    // grid reads as texture; with it there are bright rivers running through a
    // dim city, which is both what one looks like from the air and something
    // for the eye to follow across the frame.
    let cls = hash11(street_id * 3.71 + 1.9);
    let arterial = smoothstep(0.62, 0.92, cls);
    // Floored well above zero: the hierarchy should be that a side street is
    // quiet, not that it is closed. A street with no traffic at all reads as a
    // gap in the city rather than as a quiet part of it.
    let occupancy = mix(0.26, 0.85, smoothstep(0.12, 0.88, cls));
    let density = mix(0.85, 1.25, arterial);
    let platoon_k = mix(0.10, 0.055, arterial);

    // Sodium street lighting. The lamps stand along both kerbs, so this is two
    // soft rails with the carriageway darker between them.
    //
    // It was one falloff centred on the crown of the road, which peaks in the
    // middle and fills the whole street with an even warm wash — the street
    // then reads as a lit beige tube, the traffic has nothing to sit against,
    // and a frame that is supposed to be mostly black is not. Two rails outline
    // the block *better*, which was the point of having it at all.
    let to_kerb = abs(across - 0.5);
    let rail = 1.0 - smoothstep(0.0, 0.22, abs(to_kerb - 0.38));
    var lit = vec3<f32>(0.9, 0.62, 0.30) * (0.022 + rail * 0.085) * mix(0.75, 1.35, cls);

    // --- signals -------------------------------------------------------------
    // How far this street's traffic has got, and how fast it is going, from the
    // coupled oscillator driving it. Streets take turns rather than all
    // flowing, and that — more than the lights themselves — is what makes a
    // grid seen from the air read as a city rather than as a circuit board.
    //
    // The whole street shares one signal rather than one per junction. Per
    // junction needs a queue, a queue needs a simulation, and the phase break
    // at the stop line shows as a seam. From this height the difference is not
    // visible; which streets are moving is.
    let ord = floor(select(cell.x / 9.0, cell.y / 7.0, runs_x));
    let ring = ord - SIGNALS_PER_AXIS * floor(ord / SIGNALS_PER_AXIS);
    let sig_index = u32(ring) + select(u32(SIGNALS_PER_AXIS), 0u, runs_x);
    let signal = params.signals[sig_index];

    // Distance is carried at cruising speed and scaled here, so a street's
    // class costs the simulation nothing — sixteen oscillators drive an
    // unbounded number of streets. An arterial cruises about half again as
    // fast as a side street, which is the real ratio and as far as it should
    // be pushed: any wider and the arterials read as a different medium.
    let travel = signal.x * mix(0.9, 1.4, arterial);

    // Brake lights, decided with the rest of the traffic. Red at a standstill
    // is the one thing in this frame that needs no explaining, and it is what
    // tells you the lights are traffic rather than decoration.
    let brake = signal.y;

    // Two streams running opposite ways — warm one direction, cold the other,
    // the way headlights and tail lights separate from the air. Both share the
    // street's signal, because a green releases both.
    let group_phase = hash11(street_id * 6.13 + 3.4);
    let a = traffic_lane(Lane(
        (along - travel) * density, across - 0.34, density,
        street_id, occupancy, platoon_k, group_phase, signal.z));
    let b = traffic_lane(Lane(
        (-along - travel) * density, across - 0.66, density,
        street_id + 19.7, occupancy, platoon_k, group_phase + 0.37, signal.z));

    let warm = mix(vec3<f32>(1.0, 0.85, 0.6), vec3<f32>(1.0, 0.13, 0.05), brake * 0.85);
    let cold = mix(vec3<f32>(0.5, 0.7, 1.0), vec3<f32>(1.0, 0.18, 0.07), brake * 0.85);

    // The lamps run hot and the spill stays well under them. Tarmac lit by a
    // headlight is never brighter than the headlight, and letting the spill
    // come up to meet it is what turned these into slabs of light before.
    lit += warm * (a.lamp * 2.0 + a.spill * 0.50);
    lit += cold * (b.lamp * 1.7 + b.spill * 0.42);

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
    let fog_density = knob_range(globals, 5u, 0.016, 0.075) * sqrt(REFERENCE_DIST / MAX_DIST);

    // --- camera --------------------------------------------------------------
    let ro = vec3<f32>(params.sway, params.height, params.drift);
    let fwd = normalize(vec3<f32>(params.yaw, params.pitch, 1.0));
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    let rd = view_ray(fwd, right, up, screen, LENS);

    var color: vec3<f32>;
    var dist = MAX_DIST;

    let hit = march_city(ro, rd, MARCH_STEPS);
    var t_ground = MAX_DIST;
    if rd.y < -1e-4 {
        t_ground = -ro.y / rd.y;
    }

    if t_ground > 0.0 && t_ground < min(hit.t, MAX_DIST) {
        dist = t_ground;

        // Chromatic aberration, as the thing it actually is: a lens magnifies
        // red a shade more than blue, so each channel gets its own focal
        // length and the fringe falls out radially, widening towards the
        // corners. Only the road is dispersed. It carries the traffic, which
        // is the only thing in the frame small and bright enough for a fringe
        // to register on, and it is reached by a plane intersection rather
        // than by the march — running that three times would cost most of the
        // frame to fringe buildings nobody could see it on.
#ifdef CHEAP_ROAD
        // One shading pass instead of three. The fringe is the first thing to
        // go on weak hardware: it is two pixels wide at the frame edge, it is
        // gone entirely under the grade's own chroma split, and it costs twice
        // the whole road pass to keep.
        color = road_under(ro, rd);
#else
        let red = road_under(ro, view_ray(fwd, right, up, screen, LENS * (1.0 + ABERRATION)));
        let green = road_under(ro, rd);
        let blue = road_under(ro, view_ray(fwd, right, up, screen, LENS * (1.0 - ABERRATION)));
        color = vec3<f32>(red.r, green.g, blue.b);
#endif
    } else if hit.hit {
        dist = hit.t;
        color = facade(hit, ro + rd * hit.t, lit_fraction);
    } else {
        dist = MAX_DIST;
        color = sky(rd);
    }

    // Saturation belongs to the city, not to the air in front of it. Applied
    // at the end it caught the haze too and drove the far field further from
    // neutral the further away it was, which is the opposite of what distance
    // does.
    color = saturate_color(color, 1.3);

    // --- haze ----------------------------------------------------------------
    // Exponential, and the most important element in the frame after the lights
    // themselves. It does the work of depth, and it hides the far edge of the
    // march so the city has no end.
    let fog = haze_color() + vec3<f32>(0.0035, 0.0038, 0.0050);
    color = mix(color, fog, 1.0 - exp(-dist * fog_density));

    // --- beat ----------------------------------------------------------------
    // Half-time and shallow: the city brightens very slightly every other beat.
    let react = knob(globals, 7u);
    color *= 1.0 + pulse_every(globals, 2.0, 2.0) * params.energy * react * 0.2;

    // --- output --------------------------------------------------------------
    color *= knob_range(globals, 0u, 0.6, 2.4);
    color *= vignette(uv, 0.5);
    color *= globals.intensity;

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
