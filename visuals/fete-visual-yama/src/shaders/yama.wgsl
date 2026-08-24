// Yama — 山. A great volcanic cone at dusk, circled slowly, clouds drifting past.
//
// Everything else in this set is a night city. This one is landscape, and it is
// built against a different reference: the flat, layered, enormous distances of
// Breath of the Wild, where atmosphere does all the work and a mountain is a
// silhouette with one lit edge. It is the only visual here with a horizon.
//
// The cone is not marched as an SDF. It is a surface of revolution — a height
// field h(r) about the y axis — so a ray is bounded analytically to the span
// where it is inside the cylinder r < MAX_R, and only that span is stepped.
// A cone that fills the frame still costs a march of five world units, which is
// what leaves the budget for the sky.
//
// The profile is Fuji's, and it is *not* a cone: height falls as
// `(1 - r/R)^1.3`, which is steep at the summit and flattens towards the base.
// A straight cone reads as a pyramid, or as a tent. The concave flare is the
// whole silhouette, and it is a single `pow`.
//
// The sun sits at a fixed world azimuth while the camera orbits, so the light
// travels all the way round over a circuit: front-lit and flat, then raking
// across the flank, then straight into the sun with the cone black against it.
// That, rather than any animated parameter, is what makes a slow orbit worth
// watching.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, centered, knob, knob_range, pulse_every}
#import fete::noise::{hash11, hash12, hash22, noise2, fbm2, TAU}
#import fete::palette::{palette, vignette, saturate_color}

struct YamaParams {
    // Orbit angle in radians. Integrated on the CPU and wrapped, so the speed
    // knob changes where the camera goes next rather than rewriting where it
    // has been.
    orbit: f32,
    // Smoothed half-time beat energy.
    energy: f32,
    // Camera altitude, in mountain heights. The summit is at 1.0.
    height: f32,
    // Orbit radius.
    radius: f32,
    // Pan away from the axis, so the cone is not centred in the frame.
    look_yaw: f32,
    // Height of the point the camera is aimed at.
    look_lift: f32,
    // Sun position. Azimuth is world-fixed; elevation is the hour.
    sun_az: f32,
    sun_elev: f32,
    // Integrated cloud drift.
    wind: f32,
    // Altitude of the banner cloud wrapping the cone.
    banner: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: YamaParams;

// --- the mountain ------------------------------------------------------------

// Summit height, and the radius where the flanks reach the plain. The ratio is
// the one thing that decides whether this reads as a volcano or as an alp: a
// wide, low cone is Fuji, a narrow one is the Matterhorn.
const PEAK: f32 = 1.0;
const BASE_R: f32 = 2.45;
// The widest the base ever gets once it has been leaned. Everything outside
// this radius is provably plain, which is what bounds the march.
const MAX_R: f32 = 2.79;
// Profile exponent. Above 1.0 the flanks are concave — steep at the top,
// flattening into the plain — which is what a stratovolcano is.
const PROFILE: f32 = 1.3;

const MARCH_STEPS: i32 = 48;
const BISECT_STEPS: i32 = 6;
// Ray length beyond which nothing is resolved through the haze anyway.
const MAX_DIST: f32 = 120.0;

// The high cloud deck. Altitude and scale together decide how many cloud
// masses fit between the zenith and the horizon: the first version put a deck
// far too high and far too coarse, and a single blob covered the whole sky and
// read as a flat wash.
const DECK_Y: f32 = 2.6;
const DECK_SCALE: f32 = 0.20;

// Half-height of the frame in tangent units — a long lens. Wide angle from this
// close turns the cone into a pyramid looming over the frame; narrow keeps the
// flanks near-straight and the mountain reading as something twenty kilometres
// away, and leaves room either side of it to compose in.
const LENS: f32 = 0.74;

// How much of a projected field one pixel covers, and therefore how far it has
// to be blended towards its own mean.
//
// Every horizontal plane in this scene — both cloud slabs and the deck — is hit
// at `t = drop / rd.y`, so a one-pixel change in `rd.y` moves the sample by
// `t² · pixel / drop`. Near the horizon that grows without limit, and sampling
// a field there instead of averaging it lays a bar of hard horizontal stripes
// along the skyline. Derived rather than tuned, because the distance at which
// it bites depends on the altitude, the field's frequency and the resolution,
// and a hand-picked fade is wrong the moment any of the three changes.
fn footprint_blur(t: f32, drop: f32, scale: f32) -> f32 {
    let pixel = LENS / max(globals.resolution.y, 1.0);
    let fp = t * t * pixel * scale / max(abs(drop), 1e-3);
    return smoothstep(0.30, 1.10, fp);
}

// The lean. Two low harmonics stretch the base further out on one side than
// the other, so the cone is lopsided rather than a solid of revolution.
//
// Worth more than any amount of surface detail: a perfectly symmetrical
// silhouette is the one thing that reads as generated from across a room, and
// it stays symmetrical from every angle, so orbiting only confirms it.
fn lean(theta: f32) -> f32 {
    return 0.60 * sin(theta + 1.9) + 0.40 * sin(theta * 2.0 - 0.6);
}

// The radial ridges running down from the crater.
//
// Harmonics of the azimuth rather than noise, for one reason: they wrap exactly
// at ±π. Any noise sampled on the angle leaves a seam down one flank, and that
// seam is a vertical crease which sweeps through the frame as the camera
// orbits — the most visible artefact this visual could have.
fn ridges(theta: f32) -> f32 {
    return 0.52 * sin(theta * 5.0 + 0.7)
        + 0.30 * sin(theta * 9.0 - 2.3)
        + 0.13 * sin(theta * 17.0 + 1.9)
        + 0.05 * sin(theta * 29.0 - 0.4);
}

// Surface height above the plain at a world position.
fn mountain_height(p: vec2<f32>) -> f32 {
    let r = length(p);
    if r >= MAX_R {
        return 0.0;
    }
    let theta = atan2(p.y, p.x);
    let base_r = BASE_R * (1.0 + 0.09 * lean(theta));
    if r >= base_r {
        return 0.0;
    }
    let base = PEAK * pow(1.0 - r / base_r, PROFILE);

    // Relief carved into the flanks. Faded out at the summit — the gullies
    // converge and disappear there — and at the base, where it would otherwise
    // make the shoreline crinkle.
    let flank = smoothstep(0.0, 0.22, base) * smoothstep(1.0, 0.80, base);
    let relief = ridges(theta) * flank * 0.075;

    // The crater. Shallow, and only ever seen from a high orbit, but without it
    // the summit is a point and a volcano does not have a point.
    let crater = 0.05 * (1.0 - smoothstep(0.0, 0.10, r));

    return max(base + relief - crater, 0.0);
}

struct Hit {
    t: f32,
    hit: bool,
}

// Span of a ray inside the cylinder r < MAX_R. Everything outside it is
// provably below the plain, so this is the only stretch worth stepping.
fn cone_span(ro: vec3<f32>, rd: vec3<f32>) -> vec2<f32> {
    let a = dot(rd.xz, rd.xz);
    let b = 2.0 * dot(ro.xz, rd.xz);
    let c = dot(ro.xz, ro.xz) - MAX_R * MAX_R;
    let disc = b * b - 4.0 * a * c;
    if disc <= 0.0 || a < 1e-6 {
        return vec2<f32>(1.0, -1.0);
    }
    let s = sqrt(disc);
    return vec2<f32>((-b - s) / (2.0 * a), (-b + s) / (2.0 * a));
}

fn march_mountain(ro: vec3<f32>, rd: vec3<f32>, limit: f32) -> Hit {
    var out: Hit;
    out.hit = false;
    out.t = limit;

    var span = cone_span(ro, rd);
    span.x = max(span.x, 0.0);
    span.y = min(span.y, limit);

    // Clip against the summit plane: a ray climbing above PEAK, or descending
    // from above it, can only meet the surface on one side of that crossing.
    if abs(rd.y) > 1e-5 {
        let t_peak = (PEAK - ro.y) / rd.y;
        if rd.y > 0.0 {
            span.y = min(span.y, t_peak);
        } else {
            span.x = max(span.x, t_peak);
        }
    } else if ro.y > PEAK {
        return out;
    }

    // And against the plain: below y = 0 there is nothing but water.
    if rd.y < -1e-5 {
        span.y = min(span.y, -ro.y / rd.y);
    }

    if span.y <= span.x {
        return out;
    }

    // Height above the surface. Positive outside, negative in the rock.
    var t = span.x;
    // Starting inside means the ray origin is already in the rock — a
    // reflection cast from water lying within the base radius. Nothing
    // sensible to return.
    if ro.y + rd.y * t - mountain_height((ro + rd * t).xz) <= 0.0 {
        return out;
    }

    let step = (span.y - span.x) / f32(MARCH_STEPS);
    var t_prev = t;

    for (var i = 0; i < MARCH_STEPS; i++) {
        t += step;
        let p = ro + rd * t;
        let g = p.y - mountain_height(p.xz);
        if g < 0.0 {
            // Bisect the bracketed crossing. The height field is smooth, so a
            // handful of halvings lands well inside a pixel.
            var lo = t_prev;
            var hi = t;
            for (var j = 0; j < BISECT_STEPS; j++) {
                let mid = (lo + hi) * 0.5;
                let q = ro + rd * mid;
                if q.y - mountain_height(q.xz) < 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            out.hit = true;
            out.t = hi;
            return out;
        }
        t_prev = t;
    }

    return out;
}

// Surface normal by central difference.
//
// The offset grows with distance, which is deliberate: at a kilometre the
// gullies are finer than a pixel, and sampling them produces a boiling crawl
// along the flank. Widening the difference filters them into smooth shading
// instead — the same trick the city visuals use on their lights.
fn mountain_normal(p: vec3<f32>, dist: f32) -> vec3<f32> {
    let e = 0.004 + dist * 0.0022;
    let dx = mountain_height(p.xz + vec2<f32>(e, 0.0)) - mountain_height(p.xz - vec2<f32>(e, 0.0));
    let dz = mountain_height(p.xz + vec2<f32>(0.0, e)) - mountain_height(p.xz - vec2<f32>(0.0, e));
    return normalize(vec3<f32>(-dx, 2.0 * e, -dz));
}

// --- light -------------------------------------------------------------------

fn sun_direction() -> vec3<f32> {
    let ce = cos(params.sun_elev);
    return vec3<f32>(ce * cos(params.sun_az), sin(params.sun_elev), ce * sin(params.sun_az));
}

// How red the direct light has gone. Zero while the sun is still up, one once
// it is under the horizon and everything it reaches is the colour of the last
// light through the whole depth of the atmosphere.
fn redness() -> f32 {
    return smoothstep(0.11, -0.05, params.sun_elev);
}

// The warm end of the sky, tinted by the show palette.
//
// Two things had to be true of this and neither was obvious. The palette is
// sampled at *both* ends of its gradient and the warmer end is kept — the
// presets here are Tokyo neon and a fixed sample point lands on magenta in
// half of them, which turns the whole dusk mauve. And the result is normalised
// to a fixed luminance, so the palette decides the hue of the sunset and never
// how bright it is: unnormalised, a pale preset washes the sky out and a dark
// one puts the light out altogether.
fn ember() -> vec3<f32> {
    let c0 = max(palette(globals, globals.seed * 0.2), vec3<f32>(0.0));
    let c1 = max(palette(globals, 0.5 + globals.seed * 0.2), vec3<f32>(0.0));
    let raw = select(c1, c0, c0.r - c0.b > c1.r - c1.b);

    let anchor = mix(vec3<f32>(1.00, 0.46, 0.20), vec3<f32>(1.00, 0.24, 0.11), redness());
    let lum = dot(raw, vec3<f32>(0.30, 0.59, 0.11));
    // A palette with no warm end at all falls back to the anchor rather than
    // to a normalised near-black, which would be a hue picked out of noise.
    let pal = select(anchor, raw / max(lum, 1e-3) * 0.55, lum > 0.05);
    return mix(anchor, pal, 0.30);
}

// The sky in a direction, with no clouds or terrain in it.
//
// Symmetric about the horizon on purpose. Below the horizon this is haze over
// water rather than sky, but it is the same air lit the same way, and having
// one continuous function means terrain can be faded into its own background at
// any elevation without a seam appearing along the waterline.
fn atmosphere(rd: vec3<f32>, warm: vec3<f32>, glow: f32) -> vec3<f32> {
    let e = abs(rd.y);

    // Zenith to horizon. Both ends are very dark — a dusk sky is not blue, it
    // is a narrow bright band with almost nothing above it, and every unit of
    // light spent up here is contrast taken off the peak.
    let zenith = vec3<f32>(0.0026, 0.0038, 0.0120);
    let upper = vec3<f32>(0.0075, 0.0100, 0.0250);
    var col = mix(upper, zenith, smoothstep(0.0, 0.50, e));

    // Where the sun is. The horizon burns on that side and stays cold on the
    // other, which is what turns a slow orbit into a slow change of light.
    let flat_dir = normalize(vec3<f32>(rd.x, 0.0, rd.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let sun_flat = normalize(vec3<f32>(cos(params.sun_az), 0.0, sin(params.sun_az)));
    let toward = dot(flat_dir, sun_flat);

    let sunward = pow(max(toward * 0.5 + 0.5, 0.0), 5.0);
    // The anti-twilight arch — the dim rose band opposite the sun, sitting on
    // the earth's own shadow. Small, but its absence is what makes a fake dusk
    // look like a gradient.
    let anti = pow(max(-toward * 0.5 + 0.5, 0.0), 7.0);

    // Both of these are much tighter than they look like they should be. A
    // glow wide enough to be obviously a sunset is also wide enough to lift
    // the whole frame off black, and the result reads as grey haze from the
    // back of a room rather than as light. The band is narrow and *hot*; what
    // makes it look like a sunset is its intensity, not its extent.
    let band = pow(1.0 - min(e * 3.0, 1.0), 8.0);
    let broad = pow(1.0 - min(e * 1.25, 1.0), 3.0);

    col += warm * band * (2.90 * sunward + 0.14 * anti) * glow;
    col += warm * broad * 0.024 * sunward * glow;
    col += vec3<f32>(0.030, 0.045, 0.100) * broad * anti * 0.25 * glow;

    return col;
}

// What a short path through the air actually adds, for fading terrain into its
// background.
//
// Emphatically *not* the sky colour. The horizon glow is the radiance of a
// hundred kilometres of atmosphere seen end-on; using it as the airlight over
// the six units in front of a mountain paints the entire cone the colour of
// the sunset and throws the silhouette away — which is precisely what happened,
// and the frame became one flat salmon wash with no black left in it.
fn airlight(rd: vec3<f32>, warm: vec3<f32>, glow: f32) -> vec3<f32> {
    let e = abs(rd.y);
    let broad = pow(1.0 - min(e * 1.4, 1.0), 3.0);

    let flat_dir = normalize(vec3<f32>(rd.x, 0.0, rd.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let sun_flat = normalize(vec3<f32>(cos(params.sun_az), 0.0, sin(params.sun_az)));
    let sunward = pow(max(dot(flat_dir, sun_flat) * 0.5 + 0.5, 0.0), 4.0);

    return vec3<f32>(0.0055, 0.0075, 0.0165) + warm * broad * 0.048 * sunward * glow;
}

// Stars, above the horizon only.
//
// Cell counts around the sky are whole numbers so the grid closes on itself at
// ±π; anything else leaves a meridian where the field jumps as the camera
// passes it. Kept at roughly a pixel across and dim enough that only the
// brightest few reach the bloom threshold.
fn starfield(rd: vec3<f32>) -> vec3<f32> {
    if rd.y < 0.01 {
        return vec3<f32>(0.0);
    }
    // Proper spherical cells — azimuth against altitude — so a star is the
    // same size wherever it is. 700 of them around the sky is a whole number,
    // which is what closes the grid at ±π; anything else leaves a meridian the
    // field jumps across as the camera orbits.
    let az = atan2(rd.z, rd.x);
    let uv = vec2<f32>(az, asin(clamp(rd.y, -1.0, 1.0))) * (700.0 / TAU);
    let cell = floor(uv);
    let f = fract(uv);

    let h = hash22(cell + 3.1);
    if hash12(cell * 1.31 + 7.7) < 0.965 {
        return vec3<f32>(0.0);
    }

    let d = length(f - h);
    // Never smaller than a pixel or so. The show grade carries a tilt-shift
    // that softens the top of the frame, and a sub-pixel star does not survive
    // it — the same widen-to-the-footprint rule the city visuals use.
    let core = smoothstep(0.115, 0.0, d);
    let mag = 0.25 + 0.75 * pow(hash11(cell.x * 0.317 + cell.y * 1.71), 3.0);
    let twinkle = 0.72 + 0.28 * sin(globals.time * 1.7 + h.x * 40.0);

    // Cool white with a little scatter in temperature, and gone entirely near
    // the horizon where the glow has already washed them out.
    let tint = mix(vec3<f32>(0.72, 0.80, 1.0), vec3<f32>(1.0, 0.86, 0.70), h.y);
    return tint * core * mag * twinkle * 0.75 * smoothstep(0.02, 0.30, rd.y);
}

// --- clouds ------------------------------------------------------------------

// Cloud density on a plane at a fixed altitude, seen in perspective.
//
// Projecting onto a real plane rather than onto the sky sphere is what makes
// the deck converge at the horizon and stream past as the camera moves — and
// what makes the far half of it need filtering, since a plane seen edge-on runs
// away faster than a screen row can follow.
fn deck_density(ro: vec3<f32>, rd: vec3<f32>, altitude: f32, scale: f32, cover: f32) -> vec2<f32> {
    if rd.y < 0.012 {
        return vec2<f32>(0.0, 0.0);
    }
    let t = (altitude - ro.y) / rd.y;
    if t <= 0.0 {
        return vec2<f32>(0.0, 0.0);
    }

    let p = ro.xz + rd.xz * t;
    var q = p * scale + vec2<f32>(params.wind, params.wind * 0.27);
    // One turn of domain warp. Straight fbm gives an even speckle; warped, it
    // separates into masses with clear air between them, which is the whole
    // difference between cloud and static.
    q += 0.55 * vec2<f32>(noise2(q * 0.55), noise2(q * 0.55 + 31.7));

    let blur = footprint_blur(t, altitude - ro.y, scale);
    let raw = mix(fbm2(q, 4) * 0.5 + 0.5, 0.5, blur);
    let dens = smoothstep(1.0 - cover, 1.18 - cover, raw);

    // The deck runs all the way to the horizon and dissolves there rather than
    // stopping — the filter above has already turned its far half into smooth
    // haze, so there is nothing left to alias.
    let fade = 1.0 - smoothstep(70.0, 150.0, t);
    return vec2<f32>(dens * fade, raw);
}

// Antiderivative of the triangular profile `max(0, 1 - |u|)`, which integrates
// to one over its whole width.
fn tri_int(u: f32) -> f32 {
    let c = clamp(u, -1.0, 1.0);
    return select(0.5 + c - c * c * 0.5, (c + 1.0) * (c + 1.0) * 0.5, c <= 0.0);
}

// Optical depth of a soft horizontal band, clipped to whatever the ray hit.
// Returns `(depth, distance to the band's centre along the ray)`.
//
// This is how both the banner cloud around the cone and the mist on the water
// are done, and the reason it is worth doing exactly is that height is linear
// along a ray: the path integral of *any* vertical profile is just the
// difference of its antiderivative at the two ends, over `|rd.y|`. So a soft
// band costs the same as a hard one.
//
// It started as a hard slab and that was a mistake worth recording — a box
// integrates just as easily, but its edges are edges, and a cloud band with a
// sharp top and bottom cuts two dead straight horizontal lines across the
// mountain and reads as a pane of glass in front of it.
//
// Clipping at the terrain hit is what makes the band *volume* rather than
// decal: cloud in front of the cone veils it, cloud behind it does not.
fn band_depth(ro: vec3<f32>, rd: vec3<f32>, yc: f32, hw: f32, t_hit: f32) -> vec2<f32> {
    let t_max = min(t_hit, MAX_DIST);
    if t_max <= 0.0 {
        return vec2<f32>(0.0, 0.0);
    }

    // A ray running along the band accumulates unbounded path length, so both
    // branches are capped — uncapped, the horizon grows a hard opaque bar.
    if abs(rd.y) < 2e-3 {
        let f = max(1.0 - abs(ro.y - yc) / hw, 0.0);
        return vec2<f32>(min(f * t_max, 16.0), t_max * 0.5);
    }

    let u0 = (ro.y - yc) / hw;
    let u1 = (ro.y + rd.y * t_max - yc) / hw;
    let depth = abs(tri_int(u1) - tri_int(u0)) * hw / abs(rd.y);

    // Where the ray crosses the densest part of the band, which is the one
    // place worth sampling the noise.
    let t_c = clamp((yc - ro.y) / rd.y, 0.0, t_max);
    return vec2<f32>(min(depth, 16.0), t_c);
}

// --- the ranges on the horizon ----------------------------------------------

// Triangle wave, period one.
fn tri(x: f32) -> f32 {
    return abs(fract(x) * 2.0 - 1.0);
}

// Skyline of one distant range, as an elevation above the horizon.
//
// Triangles rather than sines: summed sines give smooth domes, and a horizon
// full of smooth domes reads as sand dunes. Peaks want to be points and the
// valleys between them want to be V-shaped, which is what a triangle wave is.
//
// Whole numbers of cycles around the sky, so the skyline closes on itself at
// ±π — the same reason the gullies on the cone are harmonics of the azimuth.
fn range_line(az: f32, k: f32) -> f32 {
    let t = az / TAU;
    let profile = 0.55 * tri(t * 9.0 + k * 0.37)
        + 0.26 * tri(t * 19.0 - k * 0.61)
        + 0.13 * tri(t * 37.0 + k * 0.23)
        + 0.06 * tri(t * 71.0 - k * 0.17);
    return clamp(profile, 0.0, 1.0);
}

// --- the sky, complete -------------------------------------------------------

// Everything behind the mountain: air, stars, ranges, high cloud. Called once
// for the view ray and once more for whatever the water is reflecting.
fn backdrop(
    ro: vec3<f32>,
    rd: vec3<f32>,
    warm: vec3<f32>,
    glow: f32,
    cover: f32,
    detail: bool,
) -> vec3<f32> {
    var col = atmosphere(rd, warm, glow);

    let sun_flat = normalize(vec3<f32>(cos(params.sun_az), 0.0, sin(params.sun_az)));
    let flat_dir = normalize(vec3<f32>(rd.x, 0.0, rd.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let sunward = pow(max(dot(flat_dir, sun_flat) * 0.5 + 0.5, 0.0), 3.0);

    if detail {
        col += starfield(rd);
    }

    // --- distant ranges ------------------------------------------------------
    // Stacked flat silhouettes, each closer one darker than the last. That
    // inversion — near is dark, far is nearly sky — is aerial perspective, and
    // on a landscape it does more for depth than any amount of geometry.
    let az = atan2(rd.z, rd.x);
    let el = rd.y / max(length(rd.xz), 1e-4);
    // The air as it was before any range was drawn. Each layer is composited
    // against *this* rather than against whatever the last layer left behind:
    // stacked multiplicatively, three overlapping silhouettes darken to one
    // solid bar and the ragged skylines that make them read as three separate
    // ranges disappear into it.
    let air = col;

    for (var i = 0; i < 3; i++) {
        let k = f32(i);
        // Distance in the same units as the world, used only for parallax:
        // as the camera swings round its orbit the near ranges slide against
        // the far ones, which is the only reason they read as separate.
        let dist = 90.0 - k * 26.0;
        // Clamped, because this is also called from a point out on the water
        // to fill in a reflection, and from far enough out the raw term swings
        // the skyline round by a radian and the reflection stops matching what
        // it is reflecting.
        let shift = clamp((ro.x * cos(az) - ro.z * sin(az)) / dist, -0.25, 0.25);
        let top = range_line(az + shift, k) * (0.070 + k * 0.038) + 0.006;

        if el < top {
            // A range is the sky it stands in front of, darkened by however
            // little of its own surface survives the air between. Written that
            // way rather than as a colour of its own it can never disagree with
            // the sky at the skyline — and far ranges melting into the glow
            // while near ones go black is aerial perspective, which on a
            // landscape does more for depth than geometry does.
            let aerial = 0.80 - k * 0.29;
            col = mix(vec3<f32>(0.0030, 0.0040, 0.0095), air, aerial);
            // A rim on the skyline where the sun is behind the ridge. One pixel
            // of warm light along an otherwise flat shape is what stops the
            // ranges reading as cut paper.
            let rim = smoothstep(0.0060, 0.0, top - el);
            col += warm * rim * sunward * 0.22 * glow;
        }
    }

    // --- high cloud ----------------------------------------------------------
    let deck = deck_density(ro, rd, DECK_Y, DECK_SCALE, cover);
    if deck.x > 0.001 {
        // Self-shading by comparing the density here with the density a short
        // way towards the sun. Cheaper than marching the deck, and at this
        // scale indistinguishable: what the eye reads is a lit edge and a dark
        // body, and this gives exactly that.
        let t = (DECK_Y - ro.y) / max(rd.y, 0.012);
        let p = ro.xz + rd.xz * t;
        let toward = normalize(vec2<f32>(cos(params.sun_az), sin(params.sun_az)));
        let ahead = fbm2((p + toward * 5.0) * DECK_SCALE + vec2<f32>(params.wind, params.wind * 0.27), 2) * 0.5 + 0.5;
        let lit = clamp((deck.y - ahead) * 2.4 + 0.42, 0.0, 1.0);

        // Shadowed cloud is the colour of the sky above it, not grey. Grey
        // cloud on a dark sky reads as smoke.
        let shade = vec3<f32>(0.008, 0.010, 0.023);
        var cloud = mix(shade, warm * 0.062, lit * (0.30 + 0.70 * sunward));
        // The fringe: brightest where the cloud is thinnest and the sun is
        // behind it. Most of the deck's light lives here rather than in its
        // body, which is what keeps a sky full of cloud from being a sky full
        // of grey — but only if it is sharp. `4·d·(1-d)` peaks correctly at a
        // half-covered pixel and then falls off far too slowly, and on a
        // soft-edged deck the untightened version covers most of the sky and
        // stops being a fringe at all.
        let fringe = pow(deck.x * (1.0 - deck.x) * 4.0, 2.5);
        cloud += warm * fringe * sunward * 0.85 * glow;

        col = mix(col, cloud, deck.x * 0.95);
    }

    return col;
}

// --- surface -----------------------------------------------------------------

fn shade_mountain(
    p: vec3<f32>,
    rd: vec3<f32>,
    dist: f32,
    warm: vec3<f32>,
    snow_line: f32,
) -> vec3<f32> {
    let n = mountain_normal(p, dist);
    let sun = sun_direction();
    let theta = atan2(p.z, p.x);

    // --- snow ----------------------------------------------------------------
    // The line is pushed down inside the gullies and lifted on the ridges
    // between them, so the cap ends in tongues running down the flanks rather
    // than in a circle. That ragged edge is the single most recognisable thing
    // about a snow-capped volcano.
    let gully = ridges(theta);
    // Coarse, and gone by the time the flank is far away. At 20-odd cycles per
    // world unit the snow edge is finer than a pixel at any real distance and
    // crawls; this is the same widen-or-drop rule as everywhere else.
    let grain = noise2(p.xz * 9.0) * 0.014 * (1.0 - smoothstep(4.0, 11.0, dist));
    let line = snow_line + gully * 0.055 + grain;
    let snow = smoothstep(line, line + 0.05, p.y);

    // Basalt, mottled. The low frequency is on purpose: enough to break the lit
    // flank up into something with weather on it, coarse enough that it can
    // never alias however far away the mountain gets.
    let mottle = 0.72 + 0.52 * (noise2(p.xz * 2.2) * 0.5 + 0.5);
    let rock = vec3<f32>(0.075, 0.068, 0.084) * mottle;
    let albedo = mix(rock, vec3<f32>(0.86, 0.87, 0.95), snow);

    // --- direct light --------------------------------------------------------
    // The cone is convex, so N·L is the exact shadow term; nothing here needs a
    // shadow ray.
    var direct = max(dot(n, sun), 0.0);
    // Softened towards the terminator. A hard N·L on a smooth cone draws a
    // clean curve down the flank that reads as a shading artefact.
    direct = direct * direct * (3.0 - 2.0 * min(direct * 1.4, 1.0)) * 0.5 + direct * 0.5;

    // Flattened into bands, part way. Cel shading outright is wrong at this
    // distance — it quantises the haze as well — but a partial posterise keeps
    // the broad flat planes that make the reference look painted.
    direct = mix(direct, floor(direct * 4.0 + 0.35) * 0.25, 0.35);

    // The last light climbs the mountain as the sun goes down: once it is under
    // the horizon only the summit is still in it. This is why the hour knob
    // changes the picture rather than just its exposure.
    let light_line = clamp(-params.sun_elev * 8.5, -0.3, 1.10);
    direct *= smoothstep(light_line - 0.12, light_line + 0.07, p.y);

    let sun_tint = mix(vec3<f32>(1.00, 0.55, 0.28), vec3<f32>(1.00, 0.26, 0.10), redness());
    // The sun is nearly all of the contrast in the frame, so it carries more
    // than it looks like it should: lit snow lands around two, where it blooms
    // and still holds its hue, while everything unlit sits an order of magnitude
    // below the sky. Pushing it much further tonemaps the cap towards white and
    // takes the alpenglow with it, and a white cap is a snowfield at noon.
    var col = albedo * sun_tint * direct * 2.40;

    // --- fill ----------------------------------------------------------------
    // Skylight, written as a floor rather than as albedo × light. Scaled
    // physically off basalt at this exposure it is indistinguishable from black,
    // and a black cone loses its form completely — the reference has deep blue
    // shadow, not a hole. This is the one place the shading is a deliberate lie
    // and it is the difference between a silhouette and a gap.
    //
    // Strongly blue, and very low. Both matter: any brighter and the cone stops
    // being a silhouette against the sky it stands in, and neutral grey reads as
    // a grey mass rather than as rock in the blue hour.
    let fill = mix(vec3<f32>(0.0042, 0.0062, 0.0140), vec3<f32>(0.0072, 0.0102, 0.0225), n.y * 0.5 + 0.5);
    // Snow takes far more of the skylight than basalt does, and at this hour
    // that difference is the *only* thing separating the cap from the rock over
    // most of the mountain — the sun has already left everything below the
    // line, so a cap that is not paler in shadow is not a cap at all.
    col += fill * (1.0 + 2.6 * snow);

    // And a trace of the burning horizon on anything turned towards it.
    let facing = pow(1.0 - abs(n.y), 3.0) * max(dot(normalize(vec3<f32>(n.x, 0.0, n.z)), sun) * 0.5 + 0.5, 0.0);
    col += warm * facing * 0.007 * (0.4 + 0.6 * snow);

    // --- rim -----------------------------------------------------------------
    // Down the two vertical edges of the cone, and only with the sun behind it.
    // Backlit, the whole mountain goes black with one burning edge, which is
    // the shot worth orbiting for.
    //
    // Written against the *azimuth* of the normal rather than as the usual
    // `1 - |dot(n, rd)|`, and that correction is the single largest thing that
    // went wrong here. This cone is shallow — under thirty degrees even at the
    // summit — and it is seen from near the waterline, so the ordinary grazing
    // term is close to one over the entire lower flank, not just at the edge.
    // The whole mountain lit up as a pale sheet and stopped being a mountain.
    //
    // The silhouette of a solid of revolution is where the outward normal
    // points across the view rather than along it, which is a statement about
    // azimuth alone and stays true however shallow the surface is.
    let side = 1.0 - abs(dot(normalize(vec2<f32>(n.x, n.z) + vec2<f32>(1e-6, 0.0)), normalize(rd.xz)));
    let backlit = max(dot(rd, sun), 0.0);
    col += warm * pow(side, 5.0) * pow(backlit, 2.0) * 0.55;

    return col;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let screen = centered(uv, globals.resolution);

    // --- knobs ---------------------------------------------------------------
    // Capped well short of a blanket. Past about two thirds the deck closes
    // over and the sky becomes one lit surface — and a lit surface covering
    // most of the frame is the grey-haze failure however dim it is per pixel.
    let cover = knob_range(globals, 1u, 0.16, 0.62);
    let haze = knob_range(globals, 5u, 0.012, 0.045);
    // Higher knob, lower line, more snow.
    let snow_line = knob_range(globals, 6u, 0.68, 0.34);
    let react = knob(globals, 7u);

    // Half-time, and shallow. The mountain does not move on the beat; the air
    // around it brightens very slightly, which reads as breathing rather than
    // as a light being switched.
    let swell = 1.0 + pulse_every(globals, 2.0, 2.4) * params.energy * react * 0.16;

    // How much light is left in the sky at all.
    //
    // Without this the hour knob moved the *terminator* up the mountain but
    // left the sunset burning at full strength behind it, so the deepest hour
    // was a black cone against a blazing sky — the one combination that cannot
    // happen. Scaling the warm end at source means everything lit by it dims
    // together: sky, cloud, airlight, the rim on the ranges and the rim on the
    // cone. The knob becomes the time of night rather than a shading control.
    let dusk = 0.25 + 0.75 * smoothstep(-0.16, -0.02, params.sun_elev);
    let warm = ember() * dusk;
    let glow = swell;

    // --- camera --------------------------------------------------------------
    let cam = vec3<f32>(
        sin(params.orbit) * params.radius,
        params.height,
        cos(params.orbit) * params.radius,
    );
    let aim = normalize(vec3<f32>(0.0, params.look_lift, 0.0) - cam);
    let cy = cos(params.look_yaw);
    let sy = sin(params.look_yaw);
    let fwd = normalize(vec3<f32>(cy * aim.x + sy * aim.z, aim.y, -sy * aim.x + cy * aim.z));
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    let rd = normalize(fwd + right * screen.x * LENS - up * screen.y * LENS);

    var color: vec3<f32>;
    var dist = MAX_DIST;

    let hit = march_mountain(cam, rd, MAX_DIST);
    var t_water = MAX_DIST;
    if rd.y < -1e-4 {
        t_water = -cam.y / rd.y;
    }

    if hit.hit && hit.t < t_water {
        // --- the mountain ----------------------------------------------------
        dist = hit.t;
        let p = cam + rd * hit.t;
        color = shade_mountain(p, rd, dist, warm, snow_line);

        // Faded into its own background, weighted towards the base: the air
        // near the plain is the thickest and the flanks have to dissolve into
        // the horizon rather than end on it.
        let depth = 1.0 - exp(-dist * haze * exp(-p.y * 1.1));
        color = mix(color, airlight(rd, warm, glow), depth);
    } else if t_water < MAX_DIST {
        // --- water -----------------------------------------------------------
        dist = t_water;
        let p = cam + rd * t_water;

        // Ripple, damped with distance so the far sheet is mirror-flat. Left
        // undamped it is a field of sub-pixel normals and it boils.
        let damp = 1.0 / (1.0 + dist * 0.55);
        let w = p.xz * 2.4 + vec2<f32>(globals.time * 0.10, globals.time * 0.06);
        let ripple = vec2<f32>(
            noise2(w) + 0.5 * noise2(w * 2.3 + 5.0),
            noise2(w + 13.7) + 0.5 * noise2(w * 2.3 + 19.0),
        );
        let n = normalize(vec3<f32>(ripple.x * 0.028 * damp, 1.0, ripple.y * 0.028 * damp));
        let rr = reflect(rd, n);

        var mirror = backdrop(p, rr, warm, glow, cover, false);
        let rhit = march_mountain(p, rr, 40.0);
        if rhit.hit {
            let rp = p + rr * rhit.t;
            let m = shade_mountain(rp, rr, rhit.t + dist, warm, snow_line);
            let rdepth = 1.0 - exp(-(rhit.t + dist) * haze * exp(-rp.y * 1.1));
            mirror = mix(m, airlight(rr, warm, glow), rdepth);
        }

        // Grazing rays reflect nearly everything, steep ones almost nothing —
        // so the far water carries the sky and the near water stays black,
        // which is exactly the composition this needs.
        let fresnel = 0.02 + 0.98 * pow(1.0 - max(dot(-rd, n), 0.0), 5.0);
        // Never the full value. Still water at dusk is darker than the sky it
        // holds, and a reflection at full strength doubles the lit area of the
        // frame.
        color = mirror * fresnel * 0.22 + vec3<f32>(0.0025, 0.0035, 0.009);

        // Water fades into the sky rather than into airlight: the far sheet
        // genuinely is looking along the whole depth of the atmosphere towards
        // the horizon, and it has to arrive at the same value the sky does or
        // the waterline becomes a step.
        let depth = 1.0 - exp(-dist * haze * 0.85);
        color = mix(color, atmosphere(rd, warm, glow), depth);
    } else {
        // --- sky -------------------------------------------------------------
        color = backdrop(cam, rd, warm, glow, cover, true);
    }

    // --- cloud in front ------------------------------------------------------
    let sun_flat = normalize(vec3<f32>(cos(params.sun_az), 0.0, sin(params.sun_az)));
    let flat_dir = normalize(vec3<f32>(rd.x, 0.0, rd.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let sunward = pow(max(dot(flat_dir, sun_flat) * 0.5 + 0.5, 0.0), 3.0);

    // The banner cloud — the lenticular band that hangs across the waist of a
    // volcano. Because the band is clipped at the terrain hit, the part of it in
    // front of the cone veils the cone and the part behind does not, so the
    // mountain stands *in* the cloud rather than behind a decal.
    let band = band_depth(cam, rd, params.banner, 0.16, dist);
    if band.x > 0.0 {
        let mid = cam + rd * band.y;
        let blur = footprint_blur(band.y, params.banner - cam.y, 0.55);
        let wisp = mix(fbm2(mid.xz * 0.55 + vec2<f32>(params.wind * 0.5, params.wind * 0.2), 3) * 0.5 + 0.5, 0.5, blur);
        // Sparse. A banner cloud is a torn ribbon that happens to be crossing
        // the mountain, not a layer the mountain is standing behind.
        let d = band.x * smoothstep(0.50, 0.86, wisp) * (0.14 + cover * 0.42);
        // Weak extinction over a long path rather than strong over a short one.
        // The rays that cross the waist of the cone run nearly along the band,
        // so at any real density the opacity saturates over most of its width
        // and its soft edges collapse back into the two hard horizontal lines
        // this was rewritten to avoid.
        let alpha = 1.0 - exp(-d * 0.13);
        var cloud = mix(vec3<f32>(0.007, 0.009, 0.020), warm * 0.105, 0.15 + 0.50 * sunward);
        cloud += warm * sunward * 0.055 * glow;
        color = mix(color, cloud, clamp(alpha, 0.0, 0.62));
    }

    // Mist on the water. Thin, and never off: it is what puts the base of the
    // mountain at a distance and keeps the bottom of the frame from being an
    // empty black plane. Centred below the waterline so it only ever thins
    // upwards — mist lies on water, it does not float above it.
    let mist = band_depth(cam, rd, -0.04, 0.12, dist);
    if mist.x > 0.0 {
        let mid = cam + rd * mist.y;
        let blur = footprint_blur(mist.y, -0.04 - cam.y, 0.30);
        let drift = mix(fbm2(mid.xz * 0.30 + vec2<f32>(params.wind * 0.3, -params.wind * 0.15), 3) * 0.5 + 0.5, 0.5, blur);
        let d = mist.x * (0.35 + 0.65 * drift);
        let alpha = 1.0 - exp(-d * 0.065);
        let tint = mix(vec3<f32>(0.0065, 0.0080, 0.0155), warm * 0.060, 0.15 + 0.40 * sunward);
        color = mix(color, tint, clamp(alpha, 0.0, 0.80) * 0.9);
    }

    // --- output --------------------------------------------------------------
    color *= knob_range(globals, 0u, 0.55, 2.1);
    color = saturate_color(color, 1.12);
    color *= vignette(uv, 0.4);
    color *= globals.intensity;

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
