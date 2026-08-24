// The global grade. Mirror of `Grade` in `crates/fete-core/src/grade.rs`.
//
// Runs after tonemapping, on display-referred colour.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Grade {
    resolution: vec2<f32>,
    time: f32,
    exposure: f32,
    level: f32,
    scanline: f32,
    chroma: f32,
    grain: f32,
    vignette: f32,
    wobble: f32,
    lift: f32,
    aspect: f32,
    tilt: f32,
    tilt_focus: f32,
    tilt_width: f32,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> grade: Grade;

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let res = max(grade.resolution, vec2<f32>(1.0));
    let texel = 1.0 / res;

    // --- aspect mask ---------------------------------------------------------
    // Black bars for anything outside the projector's shape. This crops rather
    // than rescales: the visuals are unbounded procedural fields with no
    // composed subject, so showing less of one costs nothing, whereas
    // rescaling would need a second render target.
    if grade.aspect > 0.001 {
        let window_aspect = res.x / res.y;
        var half = vec2<f32>(0.5);
        if window_aspect > grade.aspect {
            // Window is wider than the target: pillarbox.
            half.x = 0.5 * grade.aspect / window_aspect;
        } else {
            // Window is taller: letterbox.
            half.y = 0.5 * window_aspect / grade.aspect;
        }
        let d = abs(uv - vec2<f32>(0.5));
        if d.x > half.x || d.y > half.y {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
    }

    // --- tape tracking -------------------------------------------------------
    // A slow horizontal displacement that varies down the frame. Two sine terms
    // at unrelated rates so it never settles into a visible repeat, and scaled
    // by a slower envelope so most of the time it is doing nothing at all —
    // constant wobble reads as a broken projector rather than as tape.
    var sample_uv = uv;
    if grade.wobble > 0.001 {
        let envelope = pow(max(sin(grade.time * 0.21) * 0.5 + 0.5, 0.0), 6.0);
        let shift = sin(uv.y * 90.0 + grade.time * 4.0) * 0.6
            + sin(uv.y * 13.0 - grade.time * 1.3) * 0.4;
        sample_uv.x += shift * envelope * grade.wobble * texel.x;
    }

    // --- chromatic aberration ------------------------------------------------
    // Radial, so the centre stays sharp and only the edges separate — that is
    // how a real lens behaves, and it keeps the middle of the frame readable.
    var color: vec3<f32>;
    if grade.chroma > 0.001 {
        let offset = (sample_uv - vec2<f32>(0.5)) * grade.chroma * texel * 2.0;
        color = vec3<f32>(
            textureSample(screen_texture, screen_sampler, sample_uv + offset).r,
            textureSample(screen_texture, screen_sampler, sample_uv).g,
            textureSample(screen_texture, screen_sampler, sample_uv - offset).b,
        );
    } else {
        color = textureSample(screen_texture, screen_sampler, sample_uv).rgb;
    }

    // --- tilt-shift ----------------------------------------------------------
    // A sharp horizontal band with everything above and below softening. The
    // mask is a function of screen position only — no depth buffer needed —
    // which is exactly why the effect is worth faking: it reads as a lens
    // focused at one distance, and on a scene viewed from above that is the
    // single strongest cue that you are looking at something vast.
    if grade.tilt > 0.01 {
        let defocus = smoothstep(
            grade.tilt_width,
            grade.tilt_width + 0.32,
            abs(uv.y - grade.tilt_focus),
        );
        if defocus > 0.02 {
            let radius = defocus * grade.tilt;
            var blurred = color;
            // A golden-angle spiral: evenly covers the disc without the
            // rosettes a fixed ring produces on hard edges.
            for (var i = 0; i < 10; i++) {
                let angle = f32(i) * 2.3999632;
                let r = sqrt((f32(i) + 0.5) / 10.0) * radius;
                let offset = vec2<f32>(cos(angle), sin(angle)) * r * texel;
                // `textureSampleLevel`, not `textureSample`: this branch depends
                // on uv, and implicit-derivative sampling is not allowed in
                // non-uniform control flow.
                blurred += textureSampleLevel(
                    screen_texture,
                    screen_sampler,
                    sample_uv + offset,
                    0.0,
                ).rgb;
            }
            color = mix(color, blurred / 11.0, defocus);
        }
    }

    // --- scanlines -----------------------------------------------------------
    // Locked to physical pixels rather than uv, so the line pitch does not
    // change with resolution. Only ever darkens: brightening alternate lines
    // would raise the average level, and the whole point is to lower it.
    if grade.scanline > 0.001 {
        let line = sin(uv.y * res.y * 3.14159265);
        color *= 1.0 - grade.scanline * (0.5 + 0.5 * line * line);
    }

    // --- grade ---------------------------------------------------------------
    color *= grade.exposure;

    // Lift the black point a touch. Pure black next to bloom looks digital;
    // a slightly milky shadow reads as film, and on a projector the blacks are
    // never truly black anyway.
    color = color + vec3<f32>(grade.lift) * (1.0 - color);

    if grade.vignette > 0.001 {
        let d = length(uv - vec2<f32>(0.5)) * 1.4142;
        color *= mix(1.0, smoothstep(1.05, 0.3, d), grade.vignette);
    }

    // --- grain ---------------------------------------------------------------
    // Scaled by `sqrt(luma)`: film grain lives in the midtones, and grain in
    // the shadows is just noise on a screen that is mostly shadow.
    if grade.grain > 0.001 {
        let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
        let n = hash12(uv * res + fract(grade.time) * 1000.0) - 0.5;
        color += n * grade.grain * sqrt(clamp(luma, 0.0, 1.0));
    }

    color *= grade.level;

    // Final dither against 8-bit banding in the large smooth gradients these
    // visuals are mostly made of.
    let d = hash12(uv * res + 17.0) - 0.5;
    color += d / 255.0;

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
