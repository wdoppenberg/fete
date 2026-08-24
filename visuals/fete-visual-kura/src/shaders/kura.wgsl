// The ground the flocks are drawn on.
//
// Deliberately nothing. Every visual in the set is a fullscreen material, and
// Kura is no exception — but Kura's picture is geometry, drawn by the meshes
// this material sits behind, so all this pass owes the frame is the black the
// original cleared to. Anything painted here would be a haze behind two
// thousand small bright objects, and the contrast between them and the black is
// the entire effect.
//
// It is not dead weight: it is where `globals` reaches the GPU for this visual,
// and it guarantees a defined backdrop regardless of what the camera's clear
// colour happens to be set to.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::Globals

@group(2) @binding(0) var<uniform> globals: Globals;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Multiplying black by the master fade changes nothing today. It is
    // written that way so that if this pass ever stops being black, it is
    // already under the same fade as everything else.
    return vec4<f32>(vec3<f32>(0.0) * globals.intensity, 1.0);
}
