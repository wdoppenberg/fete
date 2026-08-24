#define_import_path fete::globals

// Mirror of `FeteGlobals` in `crates/fete-core/src/globals.rs`.
//
// Field order and types must match the Rust struct exactly. Both sides use
// WGSL layout rules, so matching declarations is enough — do not add manual
// padding here.
struct Globals {
    // Render target size in pixels.
    resolution: vec2<f32>,
    // Seconds since the show started.
    time: f32,
    // Seconds since the previous frame.
    delta: f32,
    // Continuous beat position; the fractional part is `beat_phase`.
    beat: f32,
    // Position within the current beat, 0..1.
    beat_phase: f32,
    // Position within the current bar, 0..1.
    bar_phase: f32,
    // Decaying per-beat envelope, 0..1.
    pulse: f32,
    // Regenerated each time the visual is activated. Fold into hashes so the
    // same visual varies between appearances.
    seed: f32,
    // Master fade. Multiply final colour by this.
    intensity: f32,
    // (level, bass, mid, high)
    audio: vec4<f32>,
    // Macro knobs 0..4 and 4..8, each 0..1.
    macros_a: vec4<f32>,
    macros_b: vec4<f32>,
    // Cosine palette coefficients; w unused.
    palette_a: vec4<f32>,
    palette_b: vec4<f32>,
    palette_c: vec4<f32>,
    palette_d: vec4<f32>,
}

// Aspect-corrected coordinates centred on the screen, y in -0.5..0.5.
//
// Working in this space instead of raw uv keeps a visual's proportions fixed
// when it moves from a laptop window to a 16:9 projector.
fn centered(uv: vec2<f32>, resolution: vec2<f32>) -> vec2<f32> {
    let aspect = resolution.x / max(resolution.y, 1.0);
    return vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5);
}

fn aspect_ratio(resolution: vec2<f32>) -> f32 {
    return resolution.x / max(resolution.y, 1.0);
}

// Macro knob by index, spanning both packed vectors.
fn knob(g: Globals, index: u32) -> f32 {
    if index < 4u {
        return g.macros_a[index];
    }
    return g.macros_b[index - 4u];
}

// Macro knob remapped into lo..hi.
fn knob_range(g: Globals, index: u32, lo: f32, hi: f32) -> f32 {
    return mix(lo, hi, knob(g, index));
}

// Sawtooth over `beats` beats, 0..1. Derived from the continuous beat counter
// rather than stored, so it costs nothing in the uniform and any subdivision
// works — `phrase_of(g, 16.0)` for structure, `phrase_of(g, 0.5)` for eighths.
fn phrase_of(g: Globals, beats: f32) -> f32 {
    return fract(g.beat / max(beats, 0.001));
}

// Decaying envelope firing every `beats` beats.
//
// `pulse_every(g, 2.0, 2.5)` is the one to reach for in anything atmospheric.
// Reacting on every beat makes a visual twitch along with the kick, which pulls
// the eye and competes with the music; half-time reads as breathing with the
// track instead of being driven by it. `shape` sets the decay curve — 1.0 is
// linear, higher is punchier.
fn pulse_every(g: Globals, beats: f32, shape: f32) -> f32 {
    return pow(1.0 - phrase_of(g, beats), max(shape, 0.001));
}
