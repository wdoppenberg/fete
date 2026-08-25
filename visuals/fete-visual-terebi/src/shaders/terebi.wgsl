// Terebi — テレビ. A wall of CRT sets in a dark room, each one playing its own
// fragment of late-night television: news, anime, a shooter, a racer, a test
// card, and the ones that are nothing but snow.
//
// The wall is *carved*, not tiled. A cell is cut in half along its longer side
// a couple of times, each cut decided by a hash, and every rectangle that falls
// out of that is one set. Nothing is looked up from a neighbour and nothing
// overlaps, and the sizes come out unequal for free — which matters more here
// than anywhere else in the set, because a grid of identical lit rectangles is
// the one thing this visual must never look like.
//
// A picture is a pure function of a coordinate in -1..1, which is what makes
// the whole piece work: when the wall syncs, every set is handed the position
// of its own tube on the wall instead of its own local coordinate, and the same
// nine functions draw one enormous picture split across every screen. The cost
// is a `mix` on two floats.
//
// Everything a set does — its size, its channel, when it cuts, whether it rolls,
// whether it is even switched on — hangs off one hash of the path the cuts took
// to reach it.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, centered, aspect_ratio, knob, knob_range}
#import fete::noise::{hash11, hash12, hash22, noise2, rotate2, TAU}
#import fete::palette::{palette, vignette, dither, saturate_color}

struct TerebiParams {
    sway: vec2<f32>,
    programme: f32,
    energy: f32,
    interference: f32,
    sync: f32,
    wall_mode: f32,
    _pad0: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: TerebiParams;

// How many times a wall cell may be cut. Two gives one to four sets per cell,
// which is as small as a picture can get and still be a picture.
const SPLITS: i32 = 2;
// Barrel distortion of the tube face. The strongest single cue that these are
// glass bottles and not flat panels — a rectangle with square corners reads as
// an LCD however it is coloured.
const CURVE: f32 = 0.10;
// Scan lines per tube. NTSC's, near enough; whether any of them survive to the
// projector is decided per set, by how many pixels tall that set is.
const LINES: f32 = 232.0;

// --- shapes ------------------------------------------------------------------

fn stroke(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

fn box_fill(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let d = abs(p) - half;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

fn round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    return box_fill(p, max(half - vec2<f32>(r), vec2<f32>(0.0))) - r;
}

// The show's palette, never below zero.
fn tint(t: f32) -> vec3<f32> {
    return max(palette(globals, globals.seed + t), vec3<f32>(0.0));
}

// --- writing -----------------------------------------------------------------

// A row of text, illegible on purpose.
//
// A caption on one of these sets is three or four pixels tall on the projector,
// and what the eye reads at that size is rhythm and density — never shape. Runs
// of blocks of unequal width, broken by word gaps and banded horizontally, are
// indistinguishable from a line of Japanese at that scale and cost a fraction
// of what a glyph would. The banding is the part that matters: solid blocks
// read as a barcode, and it drops out below a couple of pixels rather than
// being sampled.
fn text_row(q: vec2<f32>, half: vec2<f32>, id: f32, px: f32) -> f32 {
    if abs(q.x) > half.x || abs(q.y) > half.y {
        return 0.0;
    }
    let w = max(half.y * 0.85, px);
    // Wrapped, so a ticker running all night never walks its character index
    // out of the precision the hash has to work with.
    var i = floor(q.x / (2.0 * w));
    i -= 4096.0 * floor(i / 4096.0);

    let h = hash22(vec2<f32>(i, id));
    if h.x < 0.18 {
        return 0.0;
    }

    let cx = (floor(q.x / (2.0 * w)) + 0.5) * 2.0 * w;
    let ch = half.y * (0.55 + 0.45 * h.y);
    let d = box_fill(vec2<f32>(q.x - cx, q.y), vec2<f32>(w * 0.66, ch));
    var cover = 1.0 - smoothstep(-px, px, d);

    let rows = 2.0 + floor(h.y * 2.0);
    let band = fract((q.y + ch) / max(2.0 * ch, 1e-4) * rows);
    let legible = smoothstep(px * 1.5, px * 3.5, ch / rows);
    cover *= mix(1.0, mix(0.30, 1.0, step(band, 0.55)), legible);
    return cover;
}

// One kanji-ish mark in a -0.5..0.5 field: bars on a stem, sometimes enclosed.
// The same composition trick Kanban is built on, cut down to what survives
// being drawn thirty centimetres tall on a television across a room.
fn mark(p: vec2<f32>, id: f32) -> f32 {
    let a = hash11(id * 1.7 + 0.3);
    let b = hash11(id * 3.1 + 5.1);
    let c = hash11(id * 5.9 + 2.7);

    var d = 1e9;
    let n = 2.0 + floor(a * 2.99);
    for (var i = 0.0; i < n; i += 1.0) {
        let y = mix(0.36, -0.36, i / (n - 1.0));
        let w = 0.36 * (0.55 + 0.45 * hash11(id + i * 4.7));
        d = min(d, stroke(p, vec2<f32>(-w, y), vec2<f32>(w, y)));
    }
    if b > 0.35 {
        d = min(d, stroke(p, vec2<f32>(0.0, 0.42), vec2<f32>(0.0, -0.42)));
    }
    if c > 0.62 {
        d = min(d, abs(box_fill(p, vec2<f32>(0.40, 0.42))));
    } else if c < 0.22 {
        d = min(d, stroke(p, vec2<f32>(0.0, 0.06), vec2<f32>(-0.38, -0.42)));
        d = min(d, stroke(p, vec2<f32>(0.0, 0.06), vec2<f32>(0.38, -0.42)));
    }
    return d;
}

// --- what is on -------------------------------------------------------------
//
// Nine channels. Each is a function of a picture coordinate in -1..1 with y up,
// and each is composed *dark*: a television at night is a small amount of very
// bright material on a black field, and a wall of evenly-lit rectangles is a
// grey haze from the back of the room however carefully it is coloured. The
// test card is the deliberate exception, and it is the rarest of the nine.

// カラーバー — the test card. What a channel shows when there is nothing to
// show, which on late-night Japanese television is most of the night.
fn ch_bars(q: vec2<f32>, id: f32, px: f32, spread: f32) -> vec3<f32> {
    let i = floor((q.x * 0.5 + 0.5) * 7.0);
    var col = tint(0.10 + i * 0.13 * spread) * (0.30 + 0.09 * hash11(i + id));

    if q.y < -0.38 {
        // The castellations under the bars, and the black bar under those.
        let j = floor((q.x * 0.5 + 0.5) * 4.0);
        col = tint(0.48 + j * 0.22 * spread) * 0.10;
        if q.y < -0.74 {
            col = vec3<f32>(0.02);
        }
    }
    // No tube ever showed a perfectly flat field.
    return col * (0.90 + 0.10 * noise2(q * 7.0));
}

// 砂嵐, "sandstorm" — an untuned set. Also what every other channel shows for
// an instant after it cuts.
fn ch_snow(q: vec2<f32>, id: f32, t: f32, px: f32) -> vec3<f32> {
    // Never finer than a pixel: static sampled below the footprint stops being
    // static and becomes a crawl.
    let grain = max(0.018, px * 1.5);
    let g = hash12(floor(q / grain) + floor(t * 20.0) * 37.0 + id);
    var v = g * g * 0.30;
    // The band a mistuned set rolls slowly up the screen.
    let band = smoothstep(0.14, 0.0, abs(fract(q.y * 0.35 - t * 0.11) - 0.5));
    v = v * (0.55 + 0.85 * band) + band * 0.04;
    return vec3<f32>(v) * mix(vec3<f32>(1.0), tint(0.62), 0.25);
}

// ニュース — a studio, a presenter, an over-the-shoulder box and a caption.
// The caption is what identifies it from across a room: nothing else on
// television puts a bright bar across the bottom sixth of the frame.
fn ch_news(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32) -> vec3<f32> {
    var col = tint(0.74) * 0.020;
    let key_light = q - vec2<f32>(-0.10, 0.10);
    col += tint(0.70) * 0.045 * exp(-dot(key_light, key_light) * 2.4);

    // The presenter, off centre. Two shapes and a rim: a filled silhouette
    // alone is a hole in the picture, and the lit edge is the whole difference
    // between a person and a blob.
    let sway = sin(t * 0.8 + id * 6.0) * 0.02;
    let hp = q - vec2<f32>(-0.34 + sway, -0.10);
    let head = length((hp - vec2<f32>(0.0, 0.40)) * vec2<f32>(1.0, 0.85)) - 0.21;
    let body = round_box(hp - vec2<f32>(0.0, -0.46), vec2<f32>(0.34, 0.34), 0.18);
    let figure = min(head, body);
    col = mix(col, tint(0.80) * 0.045, 1.0 - smoothstep(0.0, px * 2.0, figure));
    col += tint(0.12) * (1.0 - smoothstep(0.0, 0.035 + px, abs(figure + 0.015))) * 0.50;

    // The box over the shoulder.
    let bq = q - vec2<f32>(0.44, 0.34);
    let bd = box_fill(bq, vec2<f32>(0.34, 0.28));
    col = mix(col, tint(0.30) * 0.09, 1.0 - smoothstep(0.0, px * 2.0, bd));
    col += tint(0.30) * (1.0 - smoothstep(0.0, px * 1.5, abs(bd))) * 0.45;
    col += vec3<f32>(0.85) * text_row(bq - vec2<f32>(0.0, -0.19), vec2<f32>(0.26, 0.040), id + 5.0, px) * 0.7;

    // The lower third, and the ticker crawling under it.
    let lq = q - vec2<f32>(0.0, -0.70);
    let ld = box_fill(lq, vec2<f32>(0.88, 0.15));
    col = mix(col, tint(0.34) * 0.11, 1.0 - smoothstep(0.0, px * 2.0, ld));
    col += tint(0.34) * (1.0 - smoothstep(0.0, px * 1.5, abs(ld))) * 0.30;
    col += vec3<f32>(1.0) * text_row(lq - vec2<f32>(-0.12, 0.045), vec2<f32>(0.62, 0.055), id + 11.0, px) * 1.05;
    let crawl = text_row(vec2<f32>(q.x + t * 0.14, lq.y + 0.085), vec2<f32>(6.0, 0.030), id + 17.0, px);
    col += tint(0.20) * crawl * step(abs(q.x), 0.86) * 0.65;

    // The clock, top right. Every one of these channels had one.
    col += vec3<f32>(0.9) * text_row(q - vec2<f32>(0.72, 0.82), vec2<f32>(0.16, 0.038), floor(t * 0.5) + 3.0, px) * 0.8;
    return col;
}

// アニメ — an impact frame: the whole screen becomes radial speed lines behind
// a hard silhouette, which is the one composition any anime cuts to when
// something lands.
fn ch_anime(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32, hit: f32) -> vec3<f32> {
    let o = vec2<f32>(sin(id * 11.0) * 0.35, cos(id * 7.0) * 0.28);
    let d = q - o;
    let ang = atan2(d.y, d.x);
    let n = 22.0 + floor(hash11(id) * 22.0);
    let lines = abs(fract(ang / TAU * n + hash11(id * 3.0) + t * 0.03) - 0.5) * 2.0;
    let rad = length(d);

    // Widened by the footprint at the far end of the burst, where the lines are
    // narrower than a pixel and would otherwise break into dashes.
    let w = clamp(0.45 - px * 12.0, 0.05, 0.45);
    var col = tint(0.06 + hash11(id * 2.3) * 0.30 * spread)
        * smoothstep(1.0 - w, 1.0, lines)
        * smoothstep(0.04, 0.55, rad)
        * (0.30 + hit * 0.9);
    col += tint(0.56) * 0.035;

    // The figure: shapes hard enough to read at any size, cutting the burst.
    let fig = min(
        length((q - vec2<f32>(0.05, -0.15)) * vec2<f32>(1.0, 0.62)) - 0.34,
        box_fill(rotate2(q - vec2<f32>(0.24, 0.22), 0.7), vec2<f32>(0.52, 0.05)),
    );
    col *= smoothstep(-px, px * 2.0, fig);
    col += tint(0.24) * (1.0 - smoothstep(0.0, 0.025 + px, abs(fig))) * (0.8 + hit * 1.2);
    return col;
}

// シューティング — a vertical shooter. Starfield, a ship, bullets in lanes,
// a rank of enemies coming down. Everything on these machines sits on a
// lattice, which is what makes so little of it read as a game.
fn ch_shmup(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32) -> vec3<f32> {
    var col = vec3<f32>(0.0);

    for (var i = 0.0; i < 3.0; i += 1.0) {
        let sc = 7.0 + i * 6.0;
        let g = vec2<f32>(q.x * sc, (q.y + t * (0.5 + i * 0.55)) * sc);
        let h = hash22(floor(g) + i * 31.7 + id);
        if h.x < 0.18 {
            let dd = length(fract(g) - 0.5 - (h - 0.5) * 0.5);
            // Widen to the footprint and drop the peak by the same ratio, so a
            // small set gets a dimmer starfield rather than a boiling one.
            let r0 = 0.10;
            let rw = max(r0, px * sc * 0.9);
            col += tint(0.62 + i * 0.05 * spread) * (r0 / rw) * (0.80 - i * 0.18)
                * (1.0 - smoothstep(0.0, rw, dd));
        }
    }

    let sx = sin(t * 1.1 + id * 5.0) * 0.55;
    let sp = q - vec2<f32>(sx, -0.62);
    let ship = min(
        abs(sp.x) * 1.3 + abs(sp.y) - 0.11,
        box_fill(sp - vec2<f32>(0.0, -0.02), vec2<f32>(0.17, 0.022)),
    );
    col += tint(0.22) * (1.0 - smoothstep(0.0, px * 2.0 + 0.004, ship)) * 1.1;
    let flame = length((sp + vec2<f32>(0.0, 0.13)) * vec2<f32>(2.4, 1.0));
    col += tint(0.05) * exp(-flame * 9.0) * (0.5 + 0.5 * fract(t * 9.0));

    let lanes = 5.0;
    let li = floor((q.x * 0.5 + 0.5) * lanes);
    let lh = hash11(li * 3.7 + id + floor(t * 0.6) * 13.0);
    if lh > 0.45 {
        let bx = -1.0 + (li + 0.5) * 2.0 / lanes;
        let by = fract((q.y * 0.5 + 0.5) - t * (1.0 + lh) - lh * 7.0) * 2.0 - 1.0;
        let bd = box_fill(vec2<f32>(q.x - bx, by), vec2<f32>(0.014, 0.075));
        col += tint(0.30) * (1.0 - smoothstep(0.0, px * 2.0 + 0.004, bd)) * 1.5 * step(-0.55, q.y);
    }

    let wave = floor(t * 0.16 + id);
    let ey = 0.62 - fract(t * 0.16 + id) * 0.55;
    let ei = floor((q.x * 0.5 + 0.5) * 6.0);
    if hash11(ei * 2.3 + wave * 7.0) > 0.34 {
        let ex = -1.0 + (ei + 0.5) / 3.0 + sin(t * 1.6 + ei) * 0.05;
        let ed = length((q - vec2<f32>(ex, ey)) * vec2<f32>(1.0, 1.5)) - 0.055;
        col += tint(0.42) * (1.0 - smoothstep(0.0, px * 2.0 + 0.005, ed)) * 1.15;
    }

    // Score, top left. Nothing says arcade like a row of digits that never stop.
    col += vec3<f32>(0.8) * text_row(q - vec2<f32>(-0.55, 0.86), vec2<f32>(0.34, 0.045), floor(t * 3.0), px) * 0.8;
    return col;
}

// アクション — a side-scrolling platformer. Bricks, a row of blocks, hills
// behind at their own parallax, and something small hopping along.
fn ch_platform(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32) -> vec3<f32> {
    let scroll = t * 0.4 + id * 7.0;
    var col = tint(0.70) * 0.030 * (0.35 + (q.y * 0.5 + 0.5) * 0.6);

    for (var i = 0.0; i < 2.0; i += 1.0) {
        let s = scroll * (0.22 + i * 0.4);
        let hy = -0.16 - i * 0.12
            + sin((q.x + s) * (1.7 + i) + id) * (0.13 - i * 0.05)
            + sin((q.x + s) * 3.9) * 0.035;
        col = mix(col, tint(0.66 - i * 0.07) * (0.05 + i * 0.025), 1.0 - smoothstep(0.0, px * 2.0, q.y - hy));
    }

    let gy = -0.52;
    if q.y < gy {
        let b = vec2<f32>((q.x + scroll) * 4.0, (q.y - gy) * 4.0);
        let m = min(abs(fract(b.x + floor(b.y) * 0.5) - 0.5), abs(fract(b.y) - 0.5));
        // Mortar rather than brick: the dark line is what carries the pattern,
        // and it holds together when the footprint eats it.
        col = tint(0.44) * (0.05 + 0.20 * smoothstep(0.04, 0.14 + px * 2.0, m));
    }

    let k = floor((q.x + scroll) * 2.6);
    if hash11(k * 1.7 + id) > 0.62 {
        let bq = q - vec2<f32>((k + 0.5) / 2.6 - scroll, 0.04);
        let bd = box_fill(bq, vec2<f32>(0.11, 0.11));
        let lit = 0.5 + 0.5 * sin(t * 5.0 + k);
        col = mix(col, tint(0.28) * (0.12 + 0.30 * lit), 1.0 - smoothstep(0.0, px * 2.0, bd));
        col += tint(0.28) * (1.0 - smoothstep(0.0, px * 2.0, abs(bd) - 0.006)) * (0.45 + lit * 0.9);
    }

    let hop = abs(sin(t * 2.3 + id * 3.0));
    let cq = q - vec2<f32>(-0.28 + sin(t * 0.6 + id) * 0.10, gy + 0.15 + hop * 0.30);
    var cd = box_fill(cq, vec2<f32>(0.055, 0.080));
    cd = min(cd, box_fill(cq - vec2<f32>(0.0, 0.10), vec2<f32>(0.075, 0.028)));
    col = mix(col, tint(0.14) * 0.85, 1.0 - smoothstep(0.0, px * 2.0, cd));
    col += tint(0.14) * (1.0 - smoothstep(0.0, px * 1.5, abs(cd))) * 0.5;
    return col;
}

// レース — the pseudo-3d road. One over distance, which is the whole of it:
// every stripe, rail and dash is a function of `1/(horizon - y)`.
//
// It is also the one channel here with a genuine aliasing problem. Distance
// runs to infinity at the horizon, so the stripe frequency does too; the fix is
// not to sample harder but to fade the contrast out as the footprint grows,
// which is exactly what a real road does in haze.
fn ch_race(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32) -> vec3<f32> {
    let horizon = 0.16;
    var col = vec3<f32>(0.0);

    if q.y > horizon {
        col = tint(0.08) * 0.05 * (1.0 - (q.y - horizon) * 1.4);
        let sun = length((q - vec2<f32>(0.12, horizon + 0.26)) * vec2<f32>(1.0, 1.25)) - 0.20;
        // Banded, the way every one of these drew its sun.
        let bands = step(0.35, fract(q.y * 26.0));
        col += tint(0.10) * (1.0 - smoothstep(0.0, 0.02 + px, sun)) * bands * 0.55;
        col += tint(0.10) * exp(-max(sun, 0.0) * 6.0) * 0.10;
    } else {
        let d = horizon - q.y;
        let z = 0.05 / max(d, 1e-3) + t * 1.6 + id * 10.0;
        let w = d * 2.4 + 0.04;
        let bend = sin(z * 0.05 + id) * d * 1.1;
        let x = q.x - bend;
        // Contrast dies where a stripe is narrower than the footprint.
        let near = smoothstep(0.0, 0.22 + px * 4.0, d);
        let stripe = step(0.5, fract(z * 0.5));

        let road = 1.0 - smoothstep(w, w + px * 2.0 + 0.004, abs(x));
        col = mix(col, tint(0.70) * 0.04 * mix(1.0, 1.5, stripe * near), road);
        let rail = 1.0 - smoothstep(0.0, px * 2.0 + 0.005, abs(abs(x) - w));
        col += mix(tint(0.02), vec3<f32>(1.0), stripe) * rail * near * 0.8;
        let dash = (1.0 - smoothstep(0.0, px * 2.0 + 0.005, abs(x))) * step(0.62, fract(z));
        col += vec3<f32>(0.75) * dash * near * 0.55;
        // The car, always in the same place, because the road moves instead.
        let cq = q - vec2<f32>(sin(t * 0.7 + id) * 0.16, -0.72);
        var cd = round_box(cq, vec2<f32>(0.20, 0.075), 0.04);
        col = mix(col, tint(0.26) * 0.35, 1.0 - smoothstep(0.0, px * 2.0, cd));
        col += tint(0.26) * (1.0 - smoothstep(0.0, px * 1.5, abs(cd))) * 0.7;
        col += tint(0.30) * (1.0 - smoothstep(0.0, px * 2.0, box_fill(cq - vec2<f32>(0.0, -0.06), vec2<f32>(0.17, 0.014)))) * 1.2;
    }
    return col;
}

// 対戦 — a fighting game. Two health bars along the top and two figures on a
// floor: the most recognisable arrangement of lit shapes in the medium, and it
// survives being twelve pixels tall.
fn ch_versus(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32, hit: f32) -> vec3<f32> {
    var col = tint(0.78) * 0.020;
    col += tint(0.60) * 0.035 * smoothstep(-0.8, 0.6, q.y);
    col += tint(0.50) * 0.05 * (1.0 - smoothstep(0.0, 0.05 + px, abs(q.y + 0.62)));

    for (var i = 0.0; i < 2.0; i += 1.0) {
        let dir = i * 2.0 - 1.0;
        let ph = t * 1.5 + id * 7.0 + i * 2.1;
        let f = q - vec2<f32>(dir * (0.30 + sin(ph * 0.5) * 0.12), -0.28 + abs(sin(ph)) * 0.05);
        var d = length((f - vec2<f32>(0.0, 0.24)) * vec2<f32>(1.0, 0.9)) - 0.10;
        d = min(d, round_box(f, vec2<f32>(0.09, 0.16), 0.05));
        d = min(d, stroke(f, vec2<f32>(0.0, 0.06), vec2<f32>(-dir * 0.26, 0.10 + sin(ph) * 0.10)) - 0.032);
        d = min(d, stroke(f, vec2<f32>(0.0, -0.14), vec2<f32>(-dir * 0.13, -0.34)) - 0.035);
        d = min(d, stroke(f, vec2<f32>(0.0, -0.14), vec2<f32>(dir * 0.10, -0.34)) - 0.035);
        let c = tint(select(0.20, 0.44, i > 0.5));
        col = mix(col, c * 0.30, 1.0 - smoothstep(0.0, px * 2.0, d));
        col += c * (1.0 - smoothstep(0.0, 0.018 + px, abs(d))) * 0.8;
    }

    for (var i = 0.0; i < 2.0; i += 1.0) {
        let dir = i * 2.0 - 1.0;
        let bq = q - vec2<f32>(dir * 0.48, 0.84);
        col += vec3<f32>(0.55) * (1.0 - smoothstep(0.0, px * 1.5, abs(box_fill(bq, vec2<f32>(0.42, 0.055))))) * 0.30;
        // Drains from the middle outwards, the way they always did.
        let amt = 0.25 + 0.75 * fract(t * 0.06 + i * 0.5 + id);
        let fq = bq - vec2<f32>(-dir * 0.40 * (1.0 - amt), 0.0);
        let fd = box_fill(fq, vec2<f32>(0.40 * amt, 0.038));
        col += mix(tint(0.12), tint(0.32), amt) * (1.0 - smoothstep(0.0, px * 1.5, fd)) * 0.55;
    }

    col += vec3<f32>(1.0) * hit * hit * 0.18;
    return col;
}

// タイトル — a title card. One large mark, three small ones set down the side
// the way a Japanese title is, and a wipe crossing it.
fn ch_title(q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32, hit: f32) -> vec3<f32> {
    let era = floor(t * 0.22 + id);
    var col = tint(0.68) * 0.035 * (1.0 - abs(q.y) * 0.5);

    let ang = atan2(q.y, q.x);
    col += tint(0.60) * 0.025 * smoothstep(0.55, 1.0, abs(fract(ang / TAU * 12.0 + t * 0.02) - 0.5) * 2.0);

    // Stroke width follows the mark, floored at the footprint: the mark gets
    // dimmer on a small set rather than breaking into dashes.
    let w = max(0.055, px * 1.3);
    let big = mark((q - vec2<f32>(-0.30, 0.02)) / 0.60, era * 3.1 + id) * 0.60;
    let fill = 0.055 / w;
    col += tint(0.18) * (1.0 - smoothstep(w * 0.4, w, big)) * 1.5 * fill;
    col += tint(0.18) * exp(-big / 0.09) * 0.30;

    let cq = q - vec2<f32>(0.44, 0.02);
    let slot = clamp(floor((0.56 - cq.y) / 0.37), 0.0, 2.0);
    let sq = (cq - vec2<f32>(0.0, 0.56 - (slot + 0.5) * 0.37)) / 0.30;
    if abs(sq.x) < 0.6 && abs(sq.y) < 0.6 {
        let d = mark(sq, era * 7.7 + slot * 3.3 + id) * 0.30;
        col += tint(0.34) * (1.0 - smoothstep(w * 0.4, w, d)) * 1.1 * fill;
    }

    let wipe = fract(t * 0.32 + id);
    col *= 1.0 + smoothstep(0.10, 0.0, abs(q.x - (wipe * 2.6 - 1.3))) * (0.4 + hit);
    return col;
}

// Which channel a set lands on. Weighted: the games and the studio carry the
// wall, the test card and the snow are punctuation. Two sets showing snow reads
// as a room full of televisions; six reads as a broken visual.
fn pick_channel(h: f32) -> i32 {
    if h < 0.17 { return 4; }
    if h < 0.32 { return 6; }
    if h < 0.45 { return 2; }
    if h < 0.58 { return 5; }
    if h < 0.70 { return 3; }
    if h < 0.80 { return 7; }
    if h < 0.90 { return 8; }
    if h < 0.96 { return 1; }
    return 0;
}

fn channel(ch: i32, q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32, hit: f32) -> vec3<f32> {
    // Per-set framing. Two sets that land on the same channel must not show the
    // same picture: mirrored, zoomed a little differently and off centre, they
    // read as two televisions rather than as one signal wired to both. It is
    // the cheapest variation in the shader and it does more than any of the
    // per-channel detail does.
    var v = q;
    v.x *= select(1.0, -1.0, hash11(id * 1.93 + 4.1) > 0.5);
    v *= mix(0.88, 1.14, hash11(id * 5.31 + 0.7));
    v += vec2<f32>(hash11(id * 2.71) - 0.5, hash11(id * 3.37) - 0.5) * 0.10;

    if ch == 1 { return ch_snow(v, id, t, px); }
    if ch == 2 { return ch_news(v, id, t, px, spread); }
    if ch == 3 { return ch_anime(v, id, t, px, spread, hit); }
    if ch == 4 { return ch_shmup(v, id, t, px, spread); }
    if ch == 5 { return ch_platform(v, id, t, px, spread); }
    if ch == 6 { return ch_race(v, id, t, px, spread); }
    if ch == 7 { return ch_versus(v, id, t, px, spread, hit); }
    if ch == 8 { return ch_title(v, id, t, px, spread, hit); }
    return ch_bars(v, id, px, spread);
}

// --- the wall ----------------------------------------------------------------

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = globals.time;
    let res = globals.resolution;
    let asp = aspect_ratio(res);

    // y-up. Every picture above has a horizon, a floor or a caption along the
    // bottom, and all of them are written the way they are watched.
    var p = centered(uv, res);
    p.y = -p.y;
    // Bounded, never integrated: the cell coordinates have to stay near the
    // origin or the hashes that lay out the wall run out of fraction overnight.
    p += params.sway;

    let react = knob(globals, 7u);
    let spread = knob_range(globals, 6u, 0.15, 1.30);
    let live_frac = knob_range(globals, 1u, 0.45, 0.98);
    let size = knob_range(globals, 5u, 0.30, 0.68);

    // The whole wall leans in on the beat. Two percent, half-time, and it moves
    // every set at once — which is most of why this belongs to the music.
    p *= 1.0 - params.energy * react * 0.025;

    let c = p / size;
    let cid = floor(c);
    let f = fract(c) - 0.5;

    // --- carve the cell into sets --------------------------------------------
    // A cell is cut in half along its longer side, up to twice, each cut decided
    // by a hash of the path taken to get here. The result is one to four sets of
    // unequal size that tile the cell exactly, with no neighbour ever consulted
    // and nothing to store. `key` follows the path, so every set on the wall has
    // an identity and everything below hangs off it.
    var lo = vec2<f32>(-0.5);
    var hi = vec2<f32>(0.5);
    var key = hash12(cid * 1.7 + globals.seed * 11.0);

    for (var i = 0; i < SPLITS; i++) {
        let h = hash22(vec2<f32>(key * 37.0, f32(i) * 5.3 + 1.7));
        if h.x > mix(0.58, 0.34, f32(i)) {
            break;
        }
        let extent = hi - lo;
        let r = mix(0.36, 0.64, h.y);
        if extent.x >= extent.y {
            let m = lo.x + extent.x * r;
            if f.x < m {
                hi.x = m;
                key = hash11(key * 7.3 + 0.19);
            } else {
                lo.x = m;
                key = hash11(key * 7.3 + 0.83);
            }
        } else {
            let m = lo.y + extent.y * r;
            if f.y < m {
                hi.y = m;
                key = hash11(key * 7.3 + 0.41);
            } else {
                lo.y = m;
                key = hash11(key * 7.3 + 0.67);
            }
        }
    }

    let set_center = (lo + hi) * 0.5;
    let set_half = (hi - lo) * 0.5;

    let ha = hash22(vec2<f32>(key * 13.1, key * 29.7 + 3.3));
    let hb = hash22(vec2<f32>(key * 5.7 + 1.9, key * 41.3));
    let hc = hash22(vec2<f32>(key * 23.9 + 7.1, key * 3.3));

    // One screen pixel, in cell units. Everything small is widened to it.
    let px_cell = (1.0 / max(res.y, 1.0)) / size;

    // Cabinets do not touch. A wall of sets is stacked, and the black between
    // them is what stops the whole thing reading as one lit panel.
    let gap = min(set_half.x, set_half.y) * 0.09 + 0.006;
    let cab_half = max(set_half - gap, vec2<f32>(0.015));

    var q = f - set_center;
    // Nobody stacked these straight.
    q = rotate2(q, (ha.x - 0.5) * 0.045);

    let corner = min(cab_half.x, cab_half.y) * 0.20;
    let cab = round_box(q, cab_half, corner);

    var col = vec3<f32>(0.0);

    if cab < 0.0 {
        // The tube inside the moulding. A wide bezel and a small picture is a
        // portable; a narrow one is somebody's big set.
        let bezel = min(cab_half.x, cab_half.y) * mix(0.11, 0.24, ha.y);
        var inner = max(cab_half - bezel, vec2<f32>(0.008));
        // 4:3 — or, on the odd set, the 3:4 of an arcade monitor stood on end.
        let ratio = select(4.0 / 3.0, 3.0 / 4.0, hb.x > 0.92);
        if inner.x / inner.y > ratio {
            inner.x = inner.y * ratio;
        } else {
            inner.y = inner.x / ratio;
        }

        let tube = round_box(q, inner, min(inner.x, inner.y) * 0.16);
        let s = q / inner;
        // A pixel, in picture units. This is the number the whole visual is
        // filtered against: a set can be a third of the frame or forty pixels
        // across, and every feature below is widened to whichever it is.
        let px = px_cell / max(inner.y, 1e-4);

        // Which sets are on, and what they are playing.
        let dead = hb.y > live_frac;
        let dwell = mix(1.7, 6.5, ha.y);
        let phase = params.programme / dwell + hash11(key * 3.7) * 11.0;
        let age = fract(phase);
        let synced = params.sync > 0.5;
        let ch_local = pick_channel(hash11(key * 17.3 + floor(phase) * 7.7 + globals.seed * 3.0));
        let ch_wall = pick_channel(hash11(floor(params.programme * 0.35) * 13.0 + globals.seed * 5.0));
        let ch = select(ch_local, ch_wall, synced);

        // The tube's curvature.
        let cs = s * (1.0 + CURVE * dot(s, s));

        // The same point, expressed as a position on the whole wall. Mixing
        // towards it is the entire sync mechanism: at 1.0 every set is handed
        // the coordinate of its own glass rather than its own picture, and the
        // nine channel functions draw one picture across every screen on the
        // wall — each piece still bulging through its own tube. It costs a mix.
        //
        // Only half the sync windows do that, though. The other half leave every
        // set its own coordinate and change only *what* it is playing, so the
        // whole wall is tuned to one broadcast at its own scale — a shop window
        // rather than a video wall. Both are worth having and one picture
        // magnified twenty times is a flat colour field on most of the sets, so
        // it is not what every window should do.
        let wall_pt = (set_center + cs * inner) * size;
        let wall_uv = vec2<f32>(wall_pt.x / (0.5 * asp), wall_pt.y * 2.0) * 1.35;
        var cu = mix(cs, wall_uv, params.sync * params.wall_mode);

        // Identity and clock go global with it, or the sets would draw the same
        // picture out of step with each other.
        let content_id = select(
            key * 31.0 + globals.seed * 3.0,
            globals.seed * 3.0 + floor(params.programme * 0.35) * 1.7,
            synced,
        );
        let content_t = t + key * 23.0 * (1.0 - params.sync);

        // Vertical hold. A few sets have lost it and roll, with the blanking
        // bar between frames crossing the picture.
        var seam = 0.0;
        if hc.x < 0.07 + params.interference * 0.22 {
            let speed = mix(0.04, 0.5, hc.y);
            let yy = fract(cu.y * 0.5 + 0.5 - content_t * speed);
            seam = smoothstep(0.035, 0.0, min(yy, 1.0 - yy));
            cu.y = yy * 2.0 - 1.0;
        }
        // Tracking. A worn tape tears one band of the picture sideways, and it
        // is the artefact that says *recorded* rather than *broadcast*.
        if params.interference > 0.01 {
            let band = smoothstep(0.55, 1.0, noise2(vec2<f32>(cu.y * 2.6, content_t * 0.7 + key * 10.0)));
            cu.x += band * params.interference * 0.22 * (hash11(floor(content_t * 14.0) + key * 3.0) - 0.5);
        }

        // Half-time, with a phase per set — the wall shimmers with the track
        // instead of pumping as one thing.
        let hit = pow(1.0 - fract(globals.beat * 0.5 + hash11(key * 9.1)), 3.0) * react;

        var pic = channel(ch, cu, content_id, content_t, px, spread, hit);

        // The instant after a channel change: a set shows a frame of noise and
        // takes a moment to lock. Cheap, and it is what makes a cut read as a
        // television changing channel rather than a shader switching branch.
        if age < 0.045 {
            pic = mix(pic, ch_snow(cu, content_id, content_t, px), 1.0 - age / 0.045);
        }
        pic *= select(1.0, 0.0, dead);

        // Every tube is a different age and a different make. Brightness and
        // colour vary per set and neither follows the other.
        let gain = mix(0.75, 1.30, hash11(key * 3.9));
        let tube_tint = mix(vec3<f32>(1.0), tint(0.45 + hash11(key * 6.1) * 0.30 * spread), 0.20);
        var flick = 1.0;
        if hc.y > 0.90 {
            // One in ten is on its way out.
            flick = mix(0.45, 1.0, step(0.28, hash11(floor(content_t * 16.0) + key * 20.0)));
        }
        pic *= gain * tube_tint * flick;
        pic *= 1.0 - seam * 0.85;
        pic *= 1.0 + hit * params.energy * 0.35;

        // Scan lines, at the tube's pitch rather than the screen's. Whether any
        // survive depends on how many pixels tall this set is: under a couple of
        // pixels per line they are faded out rather than sampled, because the
        // alternative is a moiré that crawls across the wall. The curve is
        // written to average 1.0 so a set does not dim as it shrinks.
        let pitch = 2.0 / LINES;
        let visible = smoothstep(0.9, 2.4, pitch / max(px, 1e-5));
        let line = sin((cu.y * 0.5 + 0.5) * LINES * TAU) * 0.5 + 0.5;
        pic *= mix(1.0, 0.55 + 0.9 * line, visible * 0.55);

        // Mains hum, drifting slowly up the picture.
        pic *= 1.0 + smoothstep(0.4, 0.0, abs(fract(cu.y * 0.22 - content_t * 0.05 + key) - 0.5)) * 0.10;

        // The picture stops short of the glass — overscan — and because the
        // coordinate is curved, that black border is not straight.
        let border = 1.0 - smoothstep(-px * 1.5, px * 1.5, round_box(cs, vec2<f32>(0.98), 0.14));
        col += pic * border;

        if tube < 0.0 {
            // Dark glass still holds a reflection of the room. It is what keeps
            // an off set a shape on the wall rather than a hole in it.
            let sh = cs.x * 0.7 + cs.y + ha.x * 1.2 - 0.6;
            col += tint(0.55) * exp(-sh * sh * 14.0) * 0.016;
        } else {
            // The moulding is unlit plastic; everything on it is thrown there by
            // the tube. Sampling the picture once at its centre is enough — and
            // it means a cut, a wipe or a hit lights the whole cabinet with it.
            let src = channel(ch, vec2<f32>(0.0, -0.12), content_id, content_t, px, spread, hit);
            let lum = (dot(max(src, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722)) * 0.7 + 0.02)
                * gain * flick * select(1.0, 0.0, dead);
            col += mix(vec3<f32>(1.0), tube_tint, 0.7) * exp(-tube / max(bezel * 0.45, 1e-4)) * lum * 0.50;
            // A little of it reaches the outer edge of the cabinet, which is the
            // only thing separating one dark set from the next.
            col += tint(0.52) * exp(-max(-cab, 0.0) / max(corner, 1e-4)) * lum * 0.06;
        }

        // The standby lamp: green while a set is playing, red while it is not.
        // Widened to the pixel footprint with its peak scaled by the same ratio,
        // like every other point of light in this framework. Small, and it is
        // what makes the dark half of the wall read as televisions.
        let lamp_p = q - vec2<f32>(cab_half.x * 0.62, -cab_half.y * 0.80);
        let r0 = min(cab_half.x, cab_half.y) * 0.055;
        let rw = max(r0, px_cell * 0.9);
        let lamp = exp(-dot(lamp_p, lamp_p) / (rw * rw)) * (r0 * r0) / (rw * rw);
        col += select(tint(0.28) * 1.5, tint(0.0) * 0.7, dead) * lamp;
    }

    // --- the room ---------------------------------------------------------

    // The dark between the sets is not empty. A trace of the palette's far end,
    // textured, an order of magnitude under the dimmest picture — enough that
    // the black reads as a wall behind the televisions rather than as a hole
    // punched in the image.
    let room = noise2(p * 2.2 + vec2<f32>(t * 0.010, -t * 0.008)) * 0.5 + 0.5;
    col += max(palette(globals, globals.seed + 0.58), vec3<f32>(0.0)) * room * 0.005;

    col *= knob_range(globals, 0u, 0.45, 1.70);
    col = saturate_color(col, 1.15);
    col *= vignette(uv, 0.45);
    col *= globals.intensity;
    col = dither(col, in.position.xy);

    return vec4<f32>(max(col, vec3<f32>(0.0)), 1.0);
}
