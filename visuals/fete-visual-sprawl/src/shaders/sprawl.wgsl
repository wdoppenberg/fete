// Sprawl — a megacity at night, seen from a tower.
//
// Written against the aerial Tokyo references in `inspiration/`. Four things in
// those photographs drive every decision here, and all four are the opposite of
// what a "moody future city" instinct produces:
//
//  - The light is **cool**. Blue-white windows everywhere, with sodium streets
//    and a few amber buildings as accents. Not amber overall. What colour there
//    is comes from *variety* — districts with a cast of their own, a minority of
//    saturated signs — rather than from tinting the whole city one way.
//  - The air is nearly **clear**. Distance thins the city into a fine
//    glittering band; it does not dissolve into fog until the far horizon.
//  - Buildings are **lit masses**, not silhouettes. They are brighter than the
//    gaps between them. The dark is the streets and the sky.
//  - Everything is **tiny and dense**. A building is a few pixels and there are
//    thousands of them.
//
// Scale: one world unit is about ten metres. A city block is eight units, a
// mid-rise three, a tower twenty-five, and the camera sits roughly a kilometre
// up.
//
// The ground is not marched. A ray meets it analytically, in one ray-plane
// intersection, and the fine city is a texture function of where it landed — so
// its light count is unbounded and costs the same at any distance. Only
// buildings with real vertical relief are marched, on a coarse grid of their
// own, which is cheap because a block is large and a ray crosses few.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, centered, knob, knob_range, pulse_every}
#import fete::noise::{hash11, hash12, hash22, fbm2}
#import fete::palette::{palette, vignette, saturate_color}

struct SprawlParams {
    // Distance travelled. Integrated on the CPU so the speed control changes
    // where the camera goes from here rather than rewriting where it has been.
    drift: f32,
    // Smoothed half-time beat energy.
    energy: f32,
    // Lateral position.
    sway: f32,
    // Altitude.
    height: f32,
    yaw: f32,
    pitch: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: SprawlParams;

// Buildings with real vertical relief sit on their own coarse grid. Coarse
// keeps the march cheap: a ray crosses few cells even looking to the horizon.
const BLOCK_CELL: f32 = 10.0;
// Fed by the quality tier — see `Sprawl::specialize`.
const BLOCK_STEPS: i32 = #{BLOCK_STEPS};
const BLOCK_MAX_HEIGHT: f32 = 30.0;

// The colour of city light. Overwhelmingly cool — fluorescent and LED interiors
// seen through glass — which is what the references actually look like. The
// palette tints it rather than replacing it, so the show still recolours as a
// whole without collapsing the city into a single hue.
fn city_white() -> vec3<f32> {
    return vec3<f32>(0.72, 0.82, 1.0);
}

fn tinted(hue: f32, amount: f32) -> vec3<f32> {
    let p = max(palette(globals, hue + globals.seed), vec3<f32>(0.0));
    return mix(city_white(), saturate_color(p, 1.2), amount);
}

// --- atmosphere --------------------------------------------------------------

// Weak, and cool. In the references the far city is still resolvable as
// individual lights; it thins rather than dissolving. Heavy haze was the single
// biggest thing making an earlier version look nothing like them.
fn haze() -> vec3<f32> {
    return saturate_color(max(palette(globals, 0.5 + globals.seed), vec3<f32>(0.0)), 0.60) * 0.020
        + vec3<f32>(0.0012, 0.0016, 0.0028);
}

fn sky(rd: vec3<f32>) -> vec3<f32> {
    // Near black, with a thin warm band sitting on the horizon — the city's
    // glow scattering into the first few degrees of sky and nothing above it.
    let band = pow(1.0 - clamp(rd.y * 14.0, 0.0, 1.0), 3.0);
    let up = clamp(rd.y, 0.0, 1.0);
    let warm = saturate_color(max(palette(globals, 0.05 + globals.seed), vec3<f32>(0.0)), 0.75);
    return warm * band * 0.040 + vec3<f32>(0.0025, 0.0030, 0.0055) * (1.0 - up * 0.85);
}

// How built-up somewhere is. Shared by the carpet and the blocks, so the tall
// buildings stand where the lights are dense rather than being scattered
// independently of them.
fn built_up(p: vec2<f32>) -> f32 {
    // Called once per march step, so its octave count is multiplied by
    // `BLOCK_STEPS` — by far the largest single term in this shader's cost.
    // Dropping to two octaves loses the finest variation in where the city is
    // dense, which is a scale the fog has already swallowed at the distances
    // the march spends most of its steps on.
    return smoothstep(-0.55, 0.45, fbm2(p * 0.006, #{BUILT_UP_OCTAVES}));
}

// Which way a neighbourhood's light leans, as an offset along the palette.
//
// Sodium here, mercury-vapour a kilometre over, cold LED in whatever was built
// last decade: a real city is never one colour, and one hue across every cool
// light is what reads as procedural however well lit it is. This walks slowly
// enough that a district is several hundred metres across, so the frame carries
// a few casts at once rather than a gradient.
//
// Deliberately hue-only. It moves no brightness at all, which is what keeps the
// composition — dark streets, lit masses — exactly where it was.
fn district(p: vec2<f32>) -> f32 {
    return fbm2(p * 0.0022 + vec2<f32>(31.7, 12.9), 2) * 0.30;
}

// --- the fine city -----------------------------------------------------------

// Everything too small to be worth marching: low buildings, windows, lit signs,
// street lamps, evaluated analytically at a world position.
//
// `footprint` is how much ground one pixel covers here, and every feature is
// filtered against it. That filtering is what lets the density go arbitrarily
// high without the distance turning into a boiling mess of aliasing.
fn carpet(
    p: vec2<f32>,
    footprint: f32,
    scintillation: f32,
    away: vec2<f32>,
    dispersion: f32,
) -> vec3<f32> {
    let density = built_up(p);
    if density < 0.02 {
        // Water and parkland. The references have plenty, and those dark shapes
        // are as much of the composition as the lights are.
        return vec3<f32>(0.0);
    }

    let drift = district(p);
    let cool = 0.45 + drift;

    // What the point lights below become once they are finer than a pixel.
    // Without it the distance goes dark instead of merging into a glow.
    let glow = 0.4 + 0.6 * fbm2(p * 0.04, 3);
    var lit = tinted(cool, 0.55) * density * glow * 0.030;

    // Roads. Long-exposure traffic in the references reads as continuous warm
    // lines, and it is the only large-scale structure in an otherwise even
    // field of light.
    for (var i = 0; i < 2; i++) {
        let period = select(8.0, 39.0, i == 1);
        let width = select(0.10, 0.30, i == 1);
        let gain = select(0.05, 0.16, i == 1);
        let q = abs(fract(p / period) - vec2<f32>(0.5)) * period;
        let d = min(q.x, q.y);
        // Widen towards the pixel size, and dim by the same ratio, so a road
        // thinner than a pixel fades instead of flickering.
        let w = max(width, footprint * 0.6);
        lit += tinted(0.06 + drift * 0.5, 0.88) * exp(-d / w) * (width / w) * gain * density * 1.8;
    }

    // Point lights, four octaves, finest first.
    for (var i = 0; i < 4; i++) {
        let cell = 0.32 * pow(2.9, f32(i));
        if footprint > cell * 7.0 {
            continue;
        }

        let q = p / cell;
        let id = floor(q);
        let f = fract(q) - vec2<f32>(0.5);
        let h = hash22(id + f32(i) * 37.13);

        if h.x > 0.10 + density * 0.62 {
            continue;
        }

        let offset = (hash22(id * 1.71 + 5.0) - vec2<f32>(0.5)) * 0.6;
        let centre_to_pixel = (f - offset) * cell;

        // Filtering, and the reason an earlier version rendered a city that was
        // there but forty times too dim to see. A light is far smaller than a
        // pixel at any real distance, so point-sampling a narrow gaussian
        // mostly misses it — the pixel lands between lights and returns
        // nothing. Widening the light to at least the pixel and scaling its
        // peak by the area ratio leaves total emitted energy unchanged, so the
        // pixel returns the correct *average* over the ground it covers: crisp
        // points near, a smooth glow far, and no aliasing at any range.
        let r0 = cell * 0.10;
        let radius = max(r0, footprint * 0.55);
        let conserve = (r0 * r0) / (radius * radius);

        // Atmospheric dispersion. The air is a weak prism: blue is refracted
        // more than red, so a light low on the horizon is lifted slightly and
        // its colours are lifted by different amounts. Each point source
        // becomes a tiny vertical spectrum — blue edge above, red below — and
        // the effect grows as the line of sight flattens and passes through
        // more air.
        //
        // On the ground plane, "up the screen" is "further from the camera", so
        // the split runs along `away`. Blue appears higher, therefore further,
        // therefore the blue reaching this pixel left a light slightly nearer
        // than the one the green channel sees, and red slightly further.
        //
        // Dispersion is a fixed *angular* quantity, so in world units at the
        // ground it scales exactly as the pixel footprint does — which makes
        // this a constant separation in pixels and correct at every distance.
        let split = away * dispersion * footprint;
        let dr = length(centre_to_pixel + split);
        let dg = length(centre_to_pixel);
        let db = length(centre_to_pixel - split);
        let inv_r2 = 1.0 / (radius * radius);
        let core = vec3<f32>(
            exp(-(dr * dr) * inv_r2),
            exp(-(dg * dg) * inv_r2),
            exp(-(db * db) * inv_r2),
        ) * conserve;

        // Distant lights seen through more air twinkle; near ones do not.
        let rate = 1.5 + h.y * 5.0;
        let twinkle =
            1.0 - scintillation * 0.5 * (0.5 + 0.5 * sin(globals.time * rate + h.y * 41.0));

        // Mostly cool, a warm minority, and a scattering of fully saturated
        // signs. The signs are what actually carry colour at this distance:
        // they are the only thing narrow enough in hue to survive the haze and
        // the bloom, where a merely warmer white arrives as white.
        // Saturation rises with how bright the light is, which is both true of
        // real fittings and the only way colour survives here: a core this far
        // over 1.0 tonemaps to white whatever hue it was given, so the hue has
        // to live in the gaussian skirt around it, and a skirt only carries hue
        // if the tint underneath it is strong.
        var tint = tinted(cool, 0.30 + 0.40 * h.y);
        let kind = hash11(id.x * 3.1 + id.y * 7.7 + f32(i));
        if kind > 0.86 {
            tint = tinted(0.04 + drift, 0.95);
        } else if kind > 0.79 {
            tint = tinted(0.70 + drift, 1.0);
        } else if kind > 0.74 {
            tint = tinted(0.28 + drift, 0.90);
        }

        // A high peak is right for something this small, and the filtering
        // above is what keeps it from aliasing. It also puts near lights over
        // 1.0 so bloom catches them.
        lit += tint * core * twinkle * mix(1.4, 5.5, h.y);
    }

    return lit;
}

// --- blocks ------------------------------------------------------------------

struct Block {
    height: f32,
    half_width: f32,
}

fn block_at(cell: vec2<f32>) -> Block {
    var out: Block;
    out.height = 0.0;
    out.half_width = 0.0;

    let centre = (cell + vec2<f32>(0.5)) * BLOCK_CELL;
    let dt = built_up(centre);
    if hash12(cell * 1.371 + 9.0) > 0.05 + dt * 0.30 {
        return out;
    }

    // Mostly mid-rise with a rare tower, which is the shape of the references:
    // an even field of three-to-eight storey blocks, with clusters of towers in
    // the business districts.
    let tall = pow(hash12(cell * 5.113 + 3.0), 3.6);
    out.height = mix(0.6, BLOCK_MAX_HEIGHT, tall) * mix(0.5, 1.15, dt);
    out.half_width = mix(2.2, 4.2, hash12(cell * 2.711 + 7.0));
    return out;
}

struct BlockHit {
    t: f32,
    cell: vec2<f32>,
    point: vec3<f32>,
    normal: vec3<f32>,
    hit: bool,
}

fn box_span(ro: vec3<f32>, inv_rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec2<f32> {
    let t0 = (bmin - ro) * inv_rd;
    let t1 = (bmax - ro) * inv_rd;
    let a = min(t0, t1);
    let b = max(t0, t1);
    return vec2<f32>(max(max(a.x, a.y), a.z), min(min(b.x, b.y), b.z));
}

fn sign_nz(x: f32) -> f32 {
    return select(1.0, -1.0, x < 0.0);
}

fn march_blocks(ro: vec3<f32>, rd: vec3<f32>, limit: f32) -> BlockHit {
    var out: BlockHit;
    out.hit = false;
    out.t = limit;
    out.cell = vec2<f32>(0.0);
    out.point = vec3<f32>(0.0);
    out.normal = vec3<f32>(0.0, 1.0, 0.0);

    let safe = vec3<f32>(
        sign_nz(rd.x) * max(abs(rd.x), 1e-5),
        sign_nz(rd.y) * max(abs(rd.y), 1e-5),
        sign_nz(rd.z) * max(abs(rd.z), 1e-5),
    );
    let inv_rd = 1.0 / safe;

    var cell = floor(ro.xz / BLOCK_CELL);
    let step_dir = vec2<f32>(sign_nz(rd.x), sign_nz(rd.z));
    let inv_xz = vec2<f32>(inv_rd.x, inv_rd.z);
    var t_next = ((cell + max(step_dir, vec2<f32>(0.0))) * BLOCK_CELL - ro.xz) * inv_xz;
    let t_delta = abs(vec2<f32>(BLOCK_CELL) * inv_xz);

    for (var i = 0; i < BLOCK_STEPS; i++) {
        let block = block_at(cell);
        if block.height > 0.0 {
            let centre = (cell + vec2<f32>(0.5)) * BLOCK_CELL;
            let bmin = vec3<f32>(centre.x - block.half_width, 0.0, centre.y - block.half_width);
            let bmax =
                vec3<f32>(centre.x + block.half_width, block.height, centre.y + block.half_width);
            let span = box_span(ro, inv_rd, bmin, bmax);
            if span.x <= span.y && span.y > 0.0 && span.x < limit {
                let t = max(span.x, 0.0);
                let p = ro + rd * t;
                let c = (bmin + bmax) * 0.5;
                let e = max((bmax - bmin) * 0.5, vec3<f32>(1e-4));
                let d = (p - c) / e;
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
                out.point = p;
                out.normal = n;
                return out;
            }
        }

        let t_edge = min(t_next.x, t_next.y);
        if t_edge > limit {
            break;
        }
        if rd.y > 0.0 && ro.y + rd.y * t_edge > BLOCK_MAX_HEIGHT {
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

// A building's surface. Lit, not silhouetted: in the references the buildings
// are the bright part of the picture and the dark is the streets between them.
fn block_shade(hit: BlockHit, footprint: f32) -> vec3<f32> {
    let seed = hit.cell.x * 31.7 + hit.cell.y * 57.13;
    // The same cast the carpet around this building has, so a district holds
    // together instead of its towers reading as visitors from another city.
    let drift = district(hit.point.xz);
    var col = vec3<f32>(0.004, 0.005, 0.007);

    if abs(hit.normal.y) > 0.5 {
        // Roof. Dark — from above these are a large fraction of the frame, and
        // they are what the lit facades read against. A few carry a beacon,
        // which is a point on the roof and not the whole face.
        let to_centre = length(fract(hit.point.xz / BLOCK_CELL) - vec2<f32>(0.5)) * BLOCK_CELL;
        let lamp = 1.0 - smoothstep(0.5, 1.4, to_centre);
        let blink = pow(0.5 + 0.5 * sin(globals.time * 1.7 + seed), 10.0);
        col += vec3<f32>(1.0, 0.11, 0.07) * lamp * blink * step(0.72, hash12(hit.cell * 1.9)) * 3.5;
        // Rooftop plant catching a little spill.
        col += tinted(0.5 + drift, 0.45) * 0.012 * hash12(hit.cell * 3.3);
        return col;
    }

    var across = hit.point.z;
    if abs(hit.normal.z) > 0.5 {
        across = hit.point.x;
    }

    // Windows. A storey is 0.35 units — three and a half metres — and panes are
    // the same across. An earlier version used 4.5, which at this scale is half
    // a city block per window, and the buildings came out as chequerboards.
    let pitch = 0.35;
    let grid = vec2<f32>(across, hit.point.y) / pitch;
    let id = floor(grid);
    let pane = fract(grid);

    // Below a pixel the grid is replaced by its own average rather than
    // sampled, which keeps the far city from shimmering.
    let resolved = 1.0 - smoothstep(pitch * 0.7, pitch * 2.2, footprint);
    let occupancy = 0.07 + 0.23 * hash12(hit.cell * 4.4);

    let inside = step(0.12, pane.x) * step(pane.x, 0.88) * step(0.15, pane.y) * step(pane.y, 0.85);

    // --- which windows are lit, and how brightly -----------------------------
    //
    // A facade where every lit window is the same full-brightness white is the
    // thing that most gives this away as procedural. Four sources of variation,
    // each modelling something real:

    // 1. Rooms switch on and off. An epoch counter reseeds occupancy every few
    //    seconds and the two epochs crossfade over the last of it, so a handful
    //    of windows change at any moment rather than the whole facade popping.
    let epoch_len = 11.0;
    let epoch = floor(globals.time / epoch_len);
    let epoch_t = fract(globals.time / epoch_len);
    let now_on = step(1.0 - occupancy, hash12(id * 2.31 + epoch + seed));
    let next_on = step(1.0 - occupancy, hash12(id * 2.31 + epoch + 1.0 + seed));
    let occupied = mix(now_on, next_on, smoothstep(0.90, 1.0, epoch_t));

    // 2. Rooms are not equally bright. Depth, blinds, and what the fitting
    //    actually is. Squaring biases towards dim, with a few standouts.
    let level = 0.15 + 0.85 * pow(hash12(id * 1.77 + seed + 4.0), 2.2);

    // 3. Whole floors. Offices light a storey at a time, so a lit row reads
    //    very differently from scattered rooms — and both appear in the
    //    references.
    let storey = step(0.88, hash12(vec2<f32>(7.0, id.y) + seed));

    // 4. A few fluorescents flicker, on their own fast cycle.
    var flicker = 1.0;
    if hash12(id * 5.53 + seed) > 0.972 {
        flicker = 0.45 + 0.55 * step(0.42, fract(globals.time * 9.0 + hash12(id * 1.3)));
    }

    // Per-*window* colour temperature, not just per building: a warm domestic
    // room next to a cold office is exactly what the references show, and a
    // building of uniformly tinted panes never looks inhabited.
    var lamp_tint = tinted(0.45 + drift, 0.38);
    let ct = hash12(id * 3.91 + seed + 8.0);
    if ct > 0.84 {
        lamp_tint = tinted(0.05 + drift, 0.85);
    } else if ct > 0.70 {
        lamp_tint = tinted(0.60 + drift, 0.62);
    }
    // Buildings still have an overall bias on top of that.
    if hash12(hit.cell * 6.1 + 2.0) > 0.80 {
        lamp_tint = mix(lamp_tint, tinted(0.05 + drift, 0.9), 0.6);
    }

    let emitted = occupied * inside * level * flicker * (1.0 + storey * 1.1);
    // Unresolved, the facade collapses to its own mean rather than a sample.
    let average = occupancy * 0.35;

    col += lamp_tint * mix(average, emitted, resolved) * 1.3;

    return col;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let screen = centered(uv, globals.resolution);

    // Scaled to the altitude the camera flies at. Fog is per unit distance, and
    // raising the camera lengthened every line of sight by about a third — the
    // old numbers would now dissolve the far city this shader exists to keep
    // resolvable.
    let fog_density = knob_range(globals, 5u, 0.00045, 0.0034);
    let light_gain = knob_range(globals, 1u, 0.20, 0.90);
    // Red-to-blue separation, in pixels. Well under one: dispersion should be
    // a *fringe* on a light, and once the split approaches the size of the
    // light itself the three channels separate into distinct coloured dots
    // instead — which reads as a broken renderer, not as atmosphere.
    let dispersion = knob_range(globals, 6u, 0.0, 0.7);

    // --- camera --------------------------------------------------------------
    let ro = vec3<f32>(params.sway, params.height, params.drift);
    let fwd = normalize(vec3<f32>(params.yaw, params.pitch, 1.0));
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    // A long lens. Wide angle from altitude turns a city into a model.
    let fov = 0.75;
    let rd = normalize(fwd + right * screen.x * fov - up * screen.y * fov);
    let angular = fov * 2.0 / max(globals.resolution.y, 1.0);

    var color = sky(rd);
    var dist = 4000.0;

    var t_ground = 1e9;
    if rd.y < -1e-4 {
        t_ground = -ro.y / rd.y;
    }

    if t_ground < 1e8 {
        let p = ro + rd * t_ground;
        let footprint = t_ground * angular / max(-rd.y, 0.02);
        let scintillation = smoothstep(200.0, 1600.0, t_ground);
        // Direction away from the camera along the ground, which is "up the
        // screen" for anything below the horizon.
        let away = normalize(rd.xz + vec2<f32>(1e-5));
        color = carpet(p.xz, footprint, scintillation, away, dispersion) * light_gain;
        dist = t_ground;
    }

    // Buildings, marched on their coarse grid so they stand on the same ground
    // the carpet is painted on: they occlude the lights behind them, they fade
    // by their true distance, and they parallax correctly.
    let block = march_blocks(ro, rd, min(t_ground, 900.0));
    if block.hit {
        color = block_shade(block, block.t * angular) * light_gain;
        dist = block.t;
    }

    // --- haze ----------------------------------------------------------------
    // Weak. The references are nearly clear air: distance thins the city into a
    // fine band rather than dissolving it.
    color = mix(color, haze(), 1.0 - exp(-dist * fog_density));

    // A brighter sliver right at the horizon, where the line of sight passes
    // through the most air.
    let horizon_glow = pow(1.0 - clamp(abs(rd.y) * 9.0, 0.0, 1.0), 3.0);
    color += haze() * horizon_glow * 0.22;

    // --- spinners ------------------------------------------------------------
    // A few aircraft crossing at altitude — the only thing in frame with
    // independent motion, which is what stops the rest reading as a still.
    for (var i = 0; i < 3; i++) {
        let seed = f32(i) * 17.3;
        let phase = globals.time * (0.010 + hash11(seed) * 0.014) + hash11(seed + 1.0);
        let lane = hash11(seed + 2.0);
        let sp = vec2<f32>(fract(phase) * 2.4 - 0.7, 0.06 + lane * 0.16);
        let d = (uv - sp) * vec2<f32>(globals.resolution.x / max(globals.resolution.y, 1.0), 1.0);
        let blink = step(0.55, fract(globals.time * 1.3 + seed));
        color += vec3<f32>(1.0, 0.3, 0.18) * exp(-dot(d, d) * 120000.0) * blink * 1.6;
    }

    // --- beat ----------------------------------------------------------------
    let react = knob(globals, 7u);
    color *= 1.0 + pulse_every(globals, 2.0, 2.0) * params.energy * react * 0.16;

    // --- output --------------------------------------------------------------
    color *= knob_range(globals, 0u, 0.18, 0.90);
    color = saturate_color(color, 1.20);
    color *= vignette(uv, 0.55);
    color *= globals.intensity;

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
