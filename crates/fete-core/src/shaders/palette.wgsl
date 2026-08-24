#define_import_path fete::palette

#import fete::globals::Globals

const TAU: f32 = 6.283185307179586;

// Inigo Quilez's cosine gradient: colour(t) = a + b * cos(TAU * (c * t + d)).
// See https://iquilezles.org/articles/palettes/
fn cosine_palette(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>, t: f32) -> vec3<f32> {
    return a + b * cos(TAU * (c * t + d));
}

// Sample the show's current palette. This is how a visual should produce
// colour: map whatever scalar the visual computes into 0..1 and look it up
// here, and the whole show recolours together when the palette changes.
fn palette(g: Globals, t: f32) -> vec3<f32> {
    return cosine_palette(g.palette_a.rgb, g.palette_b.rgb, g.palette_c.rgb, g.palette_d.rgb, t);
}

// --- colour space ------------------------------------------------------------

// Oklab is perceptually uniform, so interpolating in it avoids the muddy grey
// midpoint that linear RGB blends produce between complementary colours.
fn linear_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
    let m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
    let s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;

    let l_ = pow(max(l, 0.0), 1.0 / 3.0);
    let m_ = pow(max(m, 0.0), 1.0 / 3.0);
    let s_ = pow(max(s, 0.0), 1.0 / 3.0);

    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn oklab_to_linear(c: vec3<f32>) -> vec3<f32> {
    let l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

// Perceptually even blend between two colours.
fn mix_oklab(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    return oklab_to_linear(mix(linear_to_oklab(a), linear_to_oklab(b), t));
}

// --- grading -----------------------------------------------------------------

// Push saturation around the luminance-preserving grey point.
fn saturate_color(c: vec3<f32>, amount: f32) -> vec3<f32> {
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    return mix(vec3<f32>(luma), c, amount);
}

// Ordered dithering, applied before the frame is quantised to 8 bits.
//
// Large smooth gradients band badly on a projector; a fraction of a code value
// of noise breaks the contours up and costs nothing.
fn dither(color: vec3<f32>, frag_coord: vec2<f32>) -> vec3<f32> {
    let noise = fract(sin(dot(frag_coord, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    return color + (noise - 0.5) / 255.0;
}

// Radial falloff towards the frame edges.
//
// On a projector this does double duty: it focuses the eye, and it hides the
// soft, uneven edge of the projected rectangle against the wall.
fn vignette(uv: vec2<f32>, amount: f32) -> f32 {
    let d = length(uv - vec2<f32>(0.5)) * 1.4142;
    return mix(1.0, smoothstep(1.0, 0.35, d), amount);
}
