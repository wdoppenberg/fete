// The bleed pass. Mirror of `Bleed` in `crates/fete-core/src/bleed.rs`.
//
// Reads this frame and what this pass left in the history last time, and
// writes to both the view and the history. Everything that makes a transition
// look like anything happens in `displace` and `keep_mask` below; the rest is
// a decaying ghost of the frame that was on screen at the cut.
//
// Runs in HDR, before bloom, so trails carry real intensity and glow.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Bleed {
    resolution: vec2<f32>,
    time: f32,
    delta: f32,
    progress: f32,
    trail: f32,
    warp: f32,
    pulse: f32,
    seed: f32,
    style: u32,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var history_texture: texture_2d<f32>;
@group(0) @binding(2) var bleed_sampler: sampler;
@group(0) @binding(3) var<uniform> bleed: Bleed;

// Must match `BleedStyle`.
const SMEAR: u32 = 0u;
const DISSOLVE: u32 = 1u;
const MELT: u32 = 2u;
const SWIRL: u32 = 3u;
const BURN: u32 = 4u;
const RUSH: u32 = 5u;

const TAU: f32 = 6.283185307179586;
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

struct Composite {
    // The picture.
    @location(0) view: vec4<f32>,
    // The same picture, kept for next frame to read back.
    @location(1) history: vec4<f32>,
}

// Local rather than `#import fete::noise`: this shader is part of the camera
// rig, not a visual, and it should keep working if the visual libraries change.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn noise2(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash12(cell);
    let b = hash12(cell + vec2<f32>(1.0, 0.0));
    let c = hash12(cell + vec2<f32>(0.0, 1.0));
    let d = hash12(cell + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm2(p: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 0.5;
    var point = p;
    for (var i = 0; i < 4; i++) {
        value += amplitude * noise2(point);
        point *= 2.0;
        amplitude *= 0.5;
    }
    // Normalised by the sum of the amplitudes, so the result still spans 0..1
    // and the thresholds below can be read as fractions.
    return value / 0.9375;
}

fn rotate2(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let s = sin(angle);
    let c = cos(angle);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

// Where this pixel reads the previous frame from.
//
// `push` is one frame's worth of travel, in uv. It is small — a few thousandths
// — and the visible motion is the accumulation of it through the feedback
// loop, which is why every style here displaces by a little and never by a lot.
fn displace(uv: vec2<f32>, aspect: f32, push: f32) -> vec2<f32> {
    var p = (uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);

    switch bleed.style {
        case DISSOLVE: {
            // Barely moves. The erosion mask is doing the work here, and a
            // travelling image would blur the torn edge that makes it read.
            let drift = fbm2(uv * 3.0 + bleed.seed * 17.0) - 0.5;
            return uv - vec2<f32>(drift, drift * 0.5) * push * 0.5;
        }
        case MELT: {
            // Per-column speed, quantised into columns so neighbours sag
            // together into ribbons instead of each pixel going its own way.
            let column = floor(uv.x * 110.0);
            let speed = 0.35 + 1.7 * hash12(vec2<f32>(column, bleed.seed * 91.0));
            // Reading from above pulls the image downwards.
            return uv - vec2<f32>(0.0, speed * push * 1.2);
        }
        case SWIRL: {
            let radius = length(p);
            // Faster at the centre: a rigid rotation reads as the whole
            // picture turning, this reads as it going down a drain.
            let angle = push * 6.0 * (1.0 - smoothstep(0.0, 0.85, radius));
            p = rotate2(p, angle) * (1.0 - push * 0.35);
            return p / vec2<f32>(aspect, 1.0) + vec2<f32>(0.5);
        }
        case BURN: {
            // Rising, with a lateral shimmer — heat coming off the image.
            let shimmer = fbm2(uv * vec2<f32>(6.0, 3.0) - vec2<f32>(0.0, bleed.time * 0.6)) - 0.5;
            // Reading from below pushes the image upwards.
            return uv + vec2<f32>(shimmer * push * 1.4, push * 0.8);
        }
        case RUSH: {
            // Reading nearer the centre magnifies: the old frame rushes out
            // past the viewer.
            return p * (1.0 - push * 1.6) / vec2<f32>(aspect, 1.0) + vec2<f32>(0.5);
        }
        default: {
            // Smear. The direction turns slowly so consecutive transitions do
            // not all streak the same way, with low-frequency turbulence on
            // top so the streaks are not parallel.
            let angle = bleed.seed * TAU + bleed.time * 0.15;
            let direction = vec2<f32>(cos(angle), sin(angle));
            let turbulence = fbm2(uv * 2.5 + bleed.seed * 31.0) - 0.5;
            return uv - direction * (0.7 + turbulence * 1.2) * push;
        }
    }
}

// How much of the previous frame this pixel is still entitled to, `0.0..1.0`.
//
// The styles that return 1.0 dissolve purely by decay, evenly across the
// frame. The two that do not are the ones where *which* pixels go, and in what
// order, is the whole effect.
fn keep_mask(uv: vec2<f32>, aspect: f32, t: f32, level: f32) -> f32 {
    switch bleed.style {
        case DISSOLVE: {
            // Every pixel gets its own moment to let go, drawn from a noise
            // field, so the frame tears into patches instead of dimming.
            // Starts below zero and ends above one so the first and last
            // frames of the transition are clean.
            let threshold = fbm2(uv * vec2<f32>(aspect, 1.0) * 4.0 + bleed.seed * 53.0);
            return 1.0 - smoothstep(threshold - 0.14, threshold + 0.14, t * 1.3 - 0.15);
        }
        case BURN: {
            // Brightness decides instead of position. These frames are mostly
            // black with a little neon in them, so burning from the dark end
            // leaves the signal until last and the transition keeps its shape
            // all the way through.
            let flicker = fbm2(uv * 5.0 - vec2<f32>(0.0, bleed.time * 0.4) + bleed.seed * 11.0);
            let front = t * 1.35 - 0.15 + (flicker - 0.5) * 0.3;
            return smoothstep(front - 0.12, front + 0.12, level);
        }
        default: {
            return 1.0;
        }
    }
}

// Seconds this style holds a pixel for, given the show-wide setting.
fn style_trail(base: f32) -> f32 {
    switch bleed.style {
        // The mask decides when these let go, so the pixels they are still
        // holding should barely fade before their moment comes — otherwise the
        // pattern arrives on an image that has already gone grey.
        case DISSOLVE, BURN: {
            return base * 6.0;
        }
        case MELT: {
            return base * 2.5;
        }
        default: {
            return base;
        }
    }
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> Composite {
    let uv = in.uv;
    let current = textureSample(screen_texture, bleed_sampler, uv).rgb;

    let t = clamp(bleed.progress, 0.0, 1.0);
    // Transition strength: full at the cut, eased away by the end.
    let strength = 1.0 - smoothstep(0.0, 1.0, t);

    let aspect = max(bleed.resolution.x, 1.0) / max(bleed.resolution.y, 1.0);
    // One frame's travel. Beat-linked, so the smear surges with the track
    // rather than sliding at a constant machine speed.
    let push = bleed.warp * bleed.delta * strength * (1.0 + bleed.pulse * 0.6);

    let source = displace(uv, aspect, push);

    // Chromatic separation along the direction of travel, so the trail leaves
    // a colour fringe behind it. At rest `source` is `uv`, the drag is zero and
    // all three taps land on the same texel — no branch needed to switch it off.
    //
    // A *fraction* of the travel, and a small one. This is inside a feedback
    // loop, so the split compounds: at 0.15 the red channel simply travels 15%
    // further than the green each frame and the fringe grows smoothly, while
    // anything approaching 1.0 has the channels moving at wildly different
    // speeds and tears the picture into three separate images within a second.
    let drag = (source - uv) * 0.15;
    let previous = vec3<f32>(
        textureSample(history_texture, bleed_sampler, source + drag).r,
        textureSample(history_texture, bleed_sampler, source).g,
        textureSample(history_texture, bleed_sampler, source - drag).b,
    );

    // Brightness of the old pixel, compressed into 0..1 so a threshold against
    // it means something in HDR, where luma has no upper bound.
    let luma = max(dot(previous, LUMA), 0.0);
    let level = luma / (1.0 + luma);

    let mask = keep_mask(uv, aspect, t, level);
    // Force the last quarter of the transition closed. Without it the trail
    // ends whenever the decay says so, which is a different moment for every
    // style and never the beat the transition was supposed to end on.
    let envelope = 1.0 - smoothstep(0.75, 1.0, t);
    // Per-second decay resolved against this frame's delta, so the look is the
    // same at 60fps and at 144. Idle, the envelope is zero and this whole pass
    // falls through to a copy.
    let decay = exp(-bleed.delta / max(style_trail(bleed.trail), 1e-6));
    // Never quite 1.0, or a pixel could hold on forever.
    let keep = clamp(decay * mask * envelope, 0.0, 0.995);

    // What is left of the frame that was on screen at the cut. It decays on
    // its own and is never re-fed by the incoming visual, which is the whole
    // trick: the new visual plays at full brightness from its first frame and
    // the old one bleeds over the top of it. Feeding the ghost back through a
    // cross-fade instead smears *both* visuals, and the transition goes
    // through a dark middle where neither one is really on screen.
    let ghost = previous * keep;

    // Added, not mixed. These frames are mostly black with a little neon in
    // them, so a sum reads as a double exposure — two images genuinely in the
    // room at once — where a mix reads as fog over both.
    var color = current + ghost;

    // The edge of a mask flares as it passes. `mask * (1 - mask)` peaks in the
    // band where the ghost is halfway gone, which is exactly the line the eye
    // follows, and feeding the ghost's own colour into it keeps the flare in
    // the palette instead of blowing out to white.
    let rim = mask * (1.0 - mask) * 4.0;
    color += ghost * rim * 1.2;

    // What next frame reads back: the bare ghost while a transition is
    // running, and the finished picture once it is over — so the next cut
    // always has the frame that was on screen to work from. The handover sits
    // in the last of the envelope, by which point the ghost is nothing anyway.
    let gate = 1.0 - smoothstep(0.85, 1.0, t);
    let stored = mix(color, ghost, gate);

    return Composite(
        vec4<f32>(max(color, vec3<f32>(0.0)), 1.0),
        vec4<f32>(max(stored, vec3<f32>(0.0)), 1.0),
    );
}
