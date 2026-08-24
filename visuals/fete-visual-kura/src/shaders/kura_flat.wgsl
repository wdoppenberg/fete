// Flat geometry: the flow field, the light flock's triangles, the lattice
// between the heavy boids, and the ripples a field leaves.
//
// The original drew all four with a shader that did nothing but pass its
// vertex colour through, and there is no reason to do more — the shapes carry
// the image and the colour is decided on the CPU, where the palette lives.
// The only additions are the show's master fade and the same half-time swell
// the sprites get, so the two halves of the picture breathe together.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, knob, knob_range, pulse_every}

@group(2) @binding(0) var<uniform> globals: Globals;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if in.color.a < 0.002 {
        discard;
    }

    let react = knob(globals, 7u);
    let beat = 1.0 + pulse_every(globals, 2.0, 2.0) * react * 0.35;

    let gain = knob_range(globals, 0u, 0.35, 2.2);
    return vec4<f32>(in.color.rgb * gain * beat * globals.intensity, in.color.a);
}
