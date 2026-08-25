// Presentation pass for the slime simulation.
//
// The compute passes produce three trail densities per texel, one per species,
// each in an open-ended range. Everything that makes it look like anything
// happens here: a tone curve, a palette lookup, and an edge term that finds the
// network's filaments rather than its bulk.
//
// The three channels are what the simulation spends all its effort keeping
// apart, so this pass is careful not to throw that away in the first line.
// Brightness comes from their combination, but hue comes from *which species
// owns the texel* — and because the three sit at equal spacing around a circle,
// the natural way to mix them is a circular mean rather than an average.
//
// That mean hands back two things for the price of one. Its angle is the hue.
// Its *length* is how decided the texel is: near one where a single species
// holds it, a half along a two-way front, and zero where all three meet in
// equal measure. The last case is the core of a spiral — the defect the cycle
// cannot resolve — so the same number that picks the colour also finds the one
// structure in the frame worth pointing at.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, knob, knob_range, pulse_every}
#import fete::noise::fbm2
#import fete::palette::{palette, vignette, dither, saturate_color}

struct SlimeDisplay {
    // Texels per screen pixel, so the shader can step exactly one texel when
    // sampling neighbours regardless of how the sim is scaled.
    texel: vec2<f32>,
    // Smoothed beat energy.
    energy: f32,
    _padding: f32,
}

/// Named to avoid colliding with the `TAU` the imported palette module defines.
const INV_TAU: f32 = 0.15915494309189535;

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> display: SlimeDisplay;
@group(2) @binding(2) var trail: texture_2d<f32>;
@group(2) @binding(3) var trail_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let trails = textureSample(trail, trail_sampler, uv).rgb;
    let total = trails.r + trails.g + trails.b;
    let strongest = max(trails.r, max(trails.g, trails.b));

    // Deliberately not the sum. Each channel is calibrated to settle near 2.0
    // inside a healthy tube, which is where the tone curve wants its peak — so
    // adding them would put a three-way overlap at 6.0, and 6.0 tonemaps to
    // white, taking the hue with it. Since the overlaps are exactly the fronts
    // and the spiral cores, summing would bleach out the one structure this
    // visual exists to show. A soft maximum instead: a tube on its own reads
    // exactly as it always did, and an overlap reads brighter without leaving
    // the curve.
    let density = strongest + (total - strongest) * 0.35;

    // Circular mean over the three species, each placed a third of a turn
    // apart. A plain weighted average of hue positions would be wrong — halfway
    // between species 0 and species 2 is *not* species 1 — and it is exactly at
    // the fronts, where the average is taken, that being wrong would show.
    let share = trails / max(total, 1e-4);
    let wheel = share.r * vec2<f32>(1.0, 0.0)
        + share.g * vec2<f32>(-0.5, 0.8660254)
        + share.b * vec2<f32>(-0.5, -0.8660254);
    let owner = atan2(wheel.y, wheel.x) * INV_TAU;

    // How decided this texel is, and its complement. One species alone gives
    // `contested` near zero; a two-way front gives about a half; a point where
    // all three are equal gives one. That last case is a spiral core.
    let contested = clamp(1.0 - length(wheel) * 1.5, 0.0, 1.0);

    // Tone curve. Trail density is roughly exponentially distributed — a few
    // very hot ridges over a large, faint field — so a linear mapping shows
    // either the ridges or the field but never both. The log-ish curve
    // compresses the top end and lifts the middle so the whole network is
    // visible at once.
    let gain = knob_range(globals, 0u, 0.25, 3.0);
    let brightness = 1.0 - exp(-density * gain);

    // Edges. The gradient of the density field is strongest along the walls of
    // each transport tube, so this traces the network's outline. It is what
    // turns a glowing blob into something that reads as structure.
    let e = display.texel;
    let sx_pos = textureSample(trail, trail_sampler, uv + vec2<f32>(e.x, 0.0)).rgb;
    let sx_neg = textureSample(trail, trail_sampler, uv - vec2<f32>(e.x, 0.0)).rgb;
    let sy_pos = textureSample(trail, trail_sampler, uv + vec2<f32>(0.0, e.y)).rgb;
    let sy_neg = textureSample(trail, trail_sampler, uv - vec2<f32>(0.0, e.y)).rgb;
    // Gradients may use the plain sum: only their magnitude matters here, and
    // the sum is the most sensitive combination to any one channel changing.
    let dx = dot(sx_pos - sx_neg, vec3<f32>(1.0));
    let dy = dot(sy_pos - sy_neg, vec3<f32>(1.0));
    let edge = length(vec2<f32>(dx, dy));

    let edge_amount = knob(globals, 5u);
    // The fronts get a share of the filament budget on top of the geometric
    // edges, and the spiral cores — where `contested` goes all the way to one —
    // get the most of anyone. They are the structures that exist only because
    // the cycle cannot resolve itself, so they earn the contrast.
    let front = contested * contested * clamp(density * 0.4, 0.0, 1.0);
    let filament = clamp(pow(clamp(edge * 3.0, 0.0, 1.0), 1.6) + front * 0.55, 0.0, 1.0)
        * edge_amount;

    // Colour comes from two places, and keeping them separate is what stops
    // the image going white. Density alone would map every bright tube to the
    // same point in the palette, and since the tubes are also the brightest
    // pixels, that one colour would be the only one the eye registers — and
    // after bloom and tonemapping, a single very bright colour is white.
    // Adding a slow spatial gradient means tubes on the left and tubes on the
    // right are different hues at the same density.
    // A wider, slower spatial gradient than before, plus a second much finer
    // term. The coarse one gives the frame large regions of different colour;
    // the fine one keeps neighbouring strands of the same network from being
    // identical. Together they are what stop a bright network converging on
    // one hue and therefore on white.
    let spread = knob_range(globals, 6u, 0.3, 1.9);
    let coarse = uv.x * 0.45 + uv.y * 0.3 + globals.time * 0.012;
    let fine = fbm2(uv * 2.4 + globals.time * 0.03, 2) * 0.22;
    // Species separation is the largest single term, and deliberately so: it is
    // the one part of the hue that reports something the simulation decided
    // rather than something the palette was told. Compressed to about half a
    // turn overall, which puts the three species a sixth of a turn apart —
    // enough to read as three colours across a room without any of them
    // leaving the palette.
    let hue = owner * 0.5
        + brightness * spread * 0.45
        + coarse
        + fine
        + globals.seed;

    // Roughly half the previous level. The network covers a lot of the frame,
    // and at the old exposure that made it the brightest thing in the room
    // rather than something behind the act.
    var color = palette(globals, hue) * pow(brightness, 2.4) * 0.75;
    // Filaments carry a different hue again, and most of what little contrast
    // is left — they are thin, so they can be brighter than the bulk without
    // lifting the average.
    color += palette(globals, hue + 0.4) * filament * 1.1;

    // Beat.
    // Half-time and shallow, so the network swells rather than flashes.
    let react = knob(globals, 7u);
    color *= 1.0 + pulse_every(globals, 2.0, 2.0) * display.energy * react * 0.5;

    color = saturate_color(color, 1.30);
    color *= vignette(uv, 0.5);
    color *= globals.intensity;
    color = dither(color, in.position.xy);

    return vec4<f32>(color, 1.0);
}
