// Presentation pass for the slime simulation.
//
// The compute passes produce a single scalar per texel — trail density, in an
// open-ended range. Everything that makes it look like anything happens here:
// a tone curve, a palette lookup, and an edge term that finds the network's
// filaments rather than its bulk.

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

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> display: SlimeDisplay;
@group(2) @binding(2) var trail: texture_2d<f32>;
@group(2) @binding(3) var trail_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let density = textureSample(trail, trail_sampler, uv).r;

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
    let dx = textureSample(trail, trail_sampler, uv + vec2<f32>(e.x, 0.0)).r
        - textureSample(trail, trail_sampler, uv - vec2<f32>(e.x, 0.0)).r;
    let dy = textureSample(trail, trail_sampler, uv + vec2<f32>(0.0, e.y)).r
        - textureSample(trail, trail_sampler, uv - vec2<f32>(0.0, e.y)).r;
    let edge = length(vec2<f32>(dx, dy));

    let edge_amount = knob(globals, 5u);
    let filament = pow(clamp(edge * 3.0, 0.0, 1.0), 1.6) * edge_amount;

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
    let hue = brightness * spread * 0.45 + coarse + fine + globals.seed;

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
