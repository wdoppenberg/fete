// The firefly. Every disc, every trail point, every mote of dust is this quad
// with this falloff on it.
//
// A port of the original's point-sprite fragment stage. The curve is two
// pieces: a bright core that reaches full brightness at the centre, and a
// wider, much softer skirt at sixty percent. Together they give a light source
// rather than a dot — the skirt is what survives the bloom pass and makes two
// hundred discs read as a lit field from across a room.
//
// Everything else the original computed here — the depth fade, the brightness
// flicker, the mix towards white at the top of a pulse — is baked into the
// vertex colour on the CPU, because the geometry is rebuilt every frame anyway.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, knob, knob_range, pulse_every}

@group(2) @binding(0) var<uniform> globals: Globals;

const CORE_SIZE: f32 = 0.24;
const GLOW_SIZE: f32 = 0.48;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = length(in.uv - vec2<f32>(0.5));
    if dist > GLOW_SIZE {
        discard;
    }

    let core = pow(1.0 - smoothstep(0.0, CORE_SIZE, dist), 1.5);
    let glow = pow(1.0 - smoothstep(CORE_SIZE, GLOW_SIZE, dist), 2.5) * 0.6;
    let alpha = (core + glow) * in.color.a;
    if alpha < 0.01 {
        discard;
    }

    // Half-time swell. Shallow, and on the colour rather than the alpha, so it
    // reads as the field breathing with the track instead of flashing at it.
    let react = knob(globals, 7u);
    let beat = 1.0 + pulse_every(globals, 2.0, 2.0) * react * 0.35;

    let gain = knob_range(globals, 0u, 0.35, 2.2);
    return vec4<f32>(in.color.rgb * gain * beat * globals.intensity, alpha);
}
