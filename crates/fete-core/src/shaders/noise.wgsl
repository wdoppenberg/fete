#define_import_path fete::noise

const TAU: f32 = 6.283185307179586;
const PI: f32 = 3.141592653589793;

// --- hashing -----------------------------------------------------------------
// Sine-based hashes. Not cryptographically anything, but cheap, stable across
// the GPUs that matter here, and good enough that the structure they seed reads
// as random.

fn hash11(p: f32) -> f32 {
    var x = fract(p * 0.1031);
    x *= x + 33.33;
    x *= x + x;
    return fract(x);
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn hash22(p: vec2<f32>) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.xx + p3.yz) * p3.zy);
}

fn hash33(p: vec3<f32>) -> vec3<f32> {
    var p3 = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yxz + 33.33);
    return fract((p3.xxy + p3.yxx) * p3.zyx);
}

// --- gradient noise ----------------------------------------------------------

// Perlin-style 2d gradient noise, roughly -1..1.
fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // Quintic interpolant: zero first and second derivatives at the cell
    // edges, so fbm built on it has no visible lattice creases.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);

    let a = dot(hash22(i + vec2<f32>(0.0, 0.0)) * 2.0 - 1.0, f - vec2<f32>(0.0, 0.0));
    let b = dot(hash22(i + vec2<f32>(1.0, 0.0)) * 2.0 - 1.0, f - vec2<f32>(1.0, 0.0));
    let c = dot(hash22(i + vec2<f32>(0.0, 1.0)) * 2.0 - 1.0, f - vec2<f32>(0.0, 1.0));
    let d = dot(hash22(i + vec2<f32>(1.0, 1.0)) * 2.0 - 1.0, f - vec2<f32>(1.0, 1.0));

    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0;
}

// Simplex-ish 3d value noise. Cheaper than true simplex and visually
// indistinguishable once stacked into fbm.
fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);

    let n000 = hash33(i + vec3<f32>(0.0, 0.0, 0.0)).x;
    let n100 = hash33(i + vec3<f32>(1.0, 0.0, 0.0)).x;
    let n010 = hash33(i + vec3<f32>(0.0, 1.0, 0.0)).x;
    let n110 = hash33(i + vec3<f32>(1.0, 1.0, 0.0)).x;
    let n001 = hash33(i + vec3<f32>(0.0, 0.0, 1.0)).x;
    let n101 = hash33(i + vec3<f32>(1.0, 0.0, 1.0)).x;
    let n011 = hash33(i + vec3<f32>(0.0, 1.0, 1.0)).x;
    let n111 = hash33(i + vec3<f32>(1.0, 1.0, 1.0)).x;

    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);

    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z) * 2.0 - 1.0;
}

// --- fractal sums ------------------------------------------------------------

// Stacked octaves of `noise2`, roughly -1..1.
//
// Each octave is rotated as well as scaled. Without the rotation the lattices
// of successive octaves line up and produce visible axis-aligned streaking.
fn fbm2(p: vec2<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var pos = p;
    let rot = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);

    for (var i = 0; i < octaves; i++) {
        sum += amplitude * noise2(pos);
        pos = rot * pos * 2.02;
        amplitude *= 0.5;
    }
    return sum;
}

fn fbm3(p: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var pos = p;

    for (var i = 0; i < octaves; i++) {
        sum += amplitude * noise3(pos);
        pos = pos * 2.03 + vec3<f32>(19.7, 7.3, 11.1);
        amplitude *= 0.5;
    }
    return sum;
}

// Ridged fbm: sharp creases instead of smooth blobs. Reads as filaments,
// lightning, or veins rather than clouds.
fn ridged2(p: vec2<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var pos = p;
    let rot = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);

    for (var i = 0; i < octaves; i++) {
        let n = 1.0 - abs(noise2(pos));
        sum += amplitude * n * n;
        pos = rot * pos * 2.02;
        amplitude *= 0.5;
    }
    return sum;
}

// --- flow --------------------------------------------------------------------

// Divergence-free 2d flow field, from the gradient of a noise potential.
//
// Because it cannot have sources or sinks, advecting along it swirls material
// around indefinitely instead of piling it up — the reason curl noise looks
// like smoke and plain noise gradients do not.
fn curl2(p: vec2<f32>, octaves: i32) -> vec2<f32> {
    let eps = 0.01;
    let dx = fbm2(p + vec2<f32>(eps, 0.0), octaves) - fbm2(p - vec2<f32>(eps, 0.0), octaves);
    let dy = fbm2(p + vec2<f32>(0.0, eps), octaves) - fbm2(p - vec2<f32>(0.0, eps), octaves);
    return vec2<f32>(dy, -dx) / (2.0 * eps);
}

// --- helpers -----------------------------------------------------------------

fn rotate2(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let s = sin(angle);
    let c = cos(angle);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Kaleidoscopic fold: collapse the plane onto a single mirrored wedge.
fn polar_fold(p: vec2<f32>, sides: f32) -> vec2<f32> {
    let segment = TAU / max(sides, 1.0);
    let radius = length(p);
    var angle = atan2(p.y, p.x);

    // Euclidean remainder, via floor. WGSL's `%` on floats takes the sign of
    // the dividend, so `angle % segment` is negative for the half of the plane
    // where `atan2` returns a negative angle — those points fold into the wrong
    // wedge and leave a hard seam straight along the positive x axis.
    angle = angle - segment * floor(angle / segment);

    // Mirror the wedge's second half onto its first, so adjacent wedges meet as
    // reflections rather than as a discontinuity.
    angle = abs(angle - segment * 0.5);

    return vec2<f32>(cos(angle), sin(angle)) * radius;
}
