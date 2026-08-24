// Kanban — 看板, "signboard". A field of Japanese neon signs floating past in
// the dark.
//
// There is no font here and no texture. A character is *composed* the way a
// kanji is composed: a square field carrying one, two or three radicals — side
// by side, stacked, or one sitting inside an enclosure — each of which is a
// small arrangement of strokes drawn as capsules. The eye reads a script from
// across a room by its composition and its stroke density long before it can
// read a character, so hashing that structure gives something unmistakably
// East-Asian signage that is never a real word.
//
// Depth is an infinite zoom. Four layers an octave apart drift outward and
// grow, and when the zoom passes a whole octave every layer hands its contents
// to the next one out. The trick that makes the hand-off invisible: everything
// about a layer is a function of the continuous depth `z` and of a seed that
// travels *with* the depth, never of the loop index.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import fete::globals::{Globals, centered, knob, knob_range, pulse_every}
#import fete::noise::{hash11, hash22, fbm2, rotate2, TAU}
#import fete::palette::{palette, vignette, dither, saturate_color}

struct KanbanParams {
    sway: vec2<f32>,
    zoom: f32,
    energy: f32,
    melt: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: KanbanParams;

const LAYERS: i32 = 4;
// The zoom counter wraps here rather than growing all night. Its integer part
// seeds the layers and its fractional part positions them, and an f32 carrying
// hours of octaves has too little left over for the fraction — the flight
// visibly steps. Wrapping is free because the seed lookup below is taken
// modulo the same number, so the wrap is just another octave hand-off.
const ZOOM_WRAP: f32 = 64.0;

// --- strokes -----------------------------------------------------------------

// Distance to a stroke, as a capsule. Every character in the frame is built
// out of these.
fn stroke(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

// Signed distance to a rectangle; negative inside.
fn box_fill(p: vec2<f32>, half: vec2<f32>) -> f32 {
    let d = abs(p) - half;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// Distance to a rectangle's *border* — four strokes for the price of one.
fn box_edge(p: vec2<f32>, half: vec2<f32>) -> f32 {
    return abs(box_fill(p, half));
}

// A curved stroke, as three capsules along a quadratic Bézier.
//
// Only the kana use it, and they are the reason it exists: a glyph set built
// entirely from straight segments reads as Chinese. The curves and the extra
// whitespace are what make a column look like written Japanese.
fn curve(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, bend: f32) -> f32 {
    let dir = b - a;
    let n = normalize(vec2<f32>(-dir.y, dir.x));
    let m = (a + b) * 0.5 + n * bend;

    var d = 1e9;
    var prev = a;
    for (var i = 1; i <= 3; i++) {
        let t = f32(i) / 3.0;
        let u = 1.0 - t;
        let q = a * (u * u) + m * (2.0 * u * t) + b * (t * t);
        d = min(d, stroke(p, prev, q));
        prev = q;
    }
    return d;
}

// Centre and half-extent of the span `lo..hi`, packed as one vec2.
fn span_of(lo: f32, hi: f32) -> vec2<f32> {
    return vec2<f32>((lo + hi) * 0.5, (hi - lo) * 0.5);
}

// --- characters --------------------------------------------------------------

// A radical: the unit kanji are actually built from.
//
// `c` and `r` are the centre and half-extent of the box it has to fill, passed
// in rather than applied to the coordinate by the caller. Scaling `p` would be
// shorter, but it scales the returned distance with it, and every stroke in a
// compound character would come out thinner than the strokes of a simple one.
// Placing the strokes instead keeps one stroke width across the whole frame.
fn radical(p: vec2<f32>, id: f32, c: vec2<f32>, r: vec2<f32>) -> f32 {
    let q = p - c;
    let kind = hash11(id * 1.31 + 0.7);
    let a = hash11(id * 2.17 + 3.1);
    let b = hash11(id * 3.71 + 8.3);

    var d = 1e9;

    if kind < 0.26 {
        // 口 日 目 田 — an enclosure, with none, one or two crossbars.
        d = box_edge(q, r);
        let bars = floor(a * 2.99);
        for (var i = 0.0; i < bars; i += 1.0) {
            let y = r.y - 2.0 * r.y * (i + 1.0) / (bars + 1.0);
            d = min(d, stroke(q, vec2<f32>(-r.x, y), vec2<f32>(r.x, y)));
        }
        if b > 0.78 {
            d = min(d, stroke(q, vec2<f32>(0.0, -r.y), vec2<f32>(0.0, r.y)));
        }
    } else if kind < 0.52 {
        // 三 王 十 土 — horizontals, on a stem about half the time. The widths
        // have to be unequal: a stack of identical bars reads as a barcode,
        // and no character in any script is that regular.
        let n = 2.0 + floor(a * 2.99);
        for (var i = 0.0; i < n; i += 1.0) {
            let y = mix(r.y, -r.y, i / (n - 1.0));
            let w = r.x * (0.55 + 0.45 * hash11(id + i * 5.3));
            d = min(d, stroke(q, vec2<f32>(-w, y), vec2<f32>(w, y)));
        }
        if b > 0.35 {
            d = min(d, stroke(q, vec2<f32>(0.0, r.y), vec2<f32>(0.0, -r.y)));
        }
    } else if kind < 0.70 {
        // 川 リ 竹 — verticals of unequal length.
        let n = 2.0 + floor(a * 1.99);
        for (var i = 0.0; i < n; i += 1.0) {
            let x = mix(-r.x, r.x, i / (n - 1.0));
            let top = r.y * (0.7 + 0.3 * hash11(id + i * 7.7));
            let bot = -r.y * (0.7 + 0.3 * hash11(id + i * 2.9));
            d = min(d, stroke(q, vec2<f32>(x, top), vec2<f32>(x, bot)));
        }
    } else if kind < 0.86 {
        // 人 大 木 天 — a stem with diagonals splaying off it.
        d = stroke(q, vec2<f32>(0.0, r.y), vec2<f32>(0.0, -r.y));
        let fork = r.y * mix(0.5, -0.1, a);
        d = min(d, stroke(q, vec2<f32>(0.0, fork), vec2<f32>(-r.x, -r.y)));
        d = min(d, stroke(q, vec2<f32>(0.0, fork), vec2<f32>(r.x, -r.y)));
        if b > 0.5 {
            let y = mix(r.y * 0.6, 0.0, b);
            d = min(d, stroke(q, vec2<f32>(-r.x * 0.9, y), vec2<f32>(r.x * 0.9, y)));
        }
    } else {
        // 小 心 火 — a stem and a pair of dots. The sparsest radical, and the
        // one that stops a column of characters being uniformly dense.
        d = stroke(q, vec2<f32>(0.0, r.y), vec2<f32>(0.0, -r.y * (0.6 + 0.4 * a)));
        d = min(d, stroke(q, vec2<f32>(-r.x * 0.7, r.y * 0.4), vec2<f32>(-r.x * 0.9, -r.y * 0.3)));
        d = min(d, stroke(q, vec2<f32>(r.x * 0.7, r.y * 0.4), vec2<f32>(r.x * 0.9, -r.y * 0.3)));
    }

    return d;
}

// One character, in a square field spanning -0.5..0.5.
//
// The layout — how the field is divided between radicals — carries far more of
// the impression than the strokes do. Left-against-right is the commonest
// shape in the language and the one that most makes a mark read as a
// character rather than a symbol.
fn glyph(p: vec2<f32>, id: f32) -> f32 {
    let shape = hash11(id * 0.93 + 5.7);
    let a = hash11(id * 4.11 + 1.3);
    let b = hash11(id * 6.29 + 2.9);

    if shape < 0.20 {
        // One radical filling the square: 口 十 人 小
        return radical(p, id + 11.0, vec2<f32>(0.0), vec2<f32>(0.40, 0.42));
    } else if shape < 0.48 {
        // Left | right: 明 好 話 — a narrow radical against a wider body.
        let split = mix(-0.12, 0.02, a);
        let l = span_of(-0.44, split - 0.03);
        let r = span_of(split + 0.03, 0.44);
        return min(
            radical(p, id + 3.0, vec2<f32>(l.x, 0.0), vec2<f32>(l.y, 0.42)),
            radical(p, id + 7.0, vec2<f32>(r.x, 0.0), vec2<f32>(r.y, 0.42)),
        );
    } else if shape < 0.66 {
        // Top | bottom: 音 星 分
        let split = mix(-0.02, 0.14, a);
        let t = span_of(split + 0.04, 0.44);
        let u = span_of(-0.44, split - 0.04);
        return min(
            radical(p, id + 13.0, vec2<f32>(0.0, t.x), vec2<f32>(0.40, t.y)),
            radical(p, id + 17.0, vec2<f32>(0.0, u.x), vec2<f32>(0.42, u.y)),
        );
    } else if shape < 0.80 {
        // A crown over a body: 京 高 市 安. The lone dot above the top bar is
        // a tiny mark that does an enormous amount of the work.
        var d = stroke(p, vec2<f32>(-0.38, 0.30), vec2<f32>(0.38, 0.30));
        d = min(d, stroke(p, vec2<f32>(0.0, 0.46), vec2<f32>(0.0, 0.36)));
        if a > 0.5 {
            // 宀 — the shoulders of a roof radical.
            d = min(d, stroke(p, vec2<f32>(-0.38, 0.30), vec2<f32>(-0.38, 0.14)));
            d = min(d, stroke(p, vec2<f32>(0.38, 0.30), vec2<f32>(0.38, 0.14)));
        }
        return min(d, radical(p, id + 23.0, vec2<f32>(0.0, -0.14), vec2<f32>(0.34, 0.30)));
    } else if shape < 0.88 {
        // An enclosure with something inside: 国 回 図
        return min(
            box_edge(p, vec2<f32>(0.42, 0.44)),
            radical(p, id + 29.0, vec2<f32>(0.0, -0.02), vec2<f32>(0.22, 0.24)),
        );
    }

    // Kana: two or three strokes, mostly curved, deliberately off centre. The
    // gap in density between these and the kanji above is most of what makes a
    // column of them look written rather than generated.
    var d = curve(
        p,
        vec2<f32>(0.30 - 0.5 * a, 0.42),
        vec2<f32>(-0.34, -0.40 + 0.3 * b),
        mix(-0.16, 0.16, a),
    );
    if a > 0.30 {
        d = min(d, stroke(p, vec2<f32>(-0.36, 0.30), vec2<f32>(0.30, 0.24 + 0.10 * b)));
    }
    if b > 0.45 {
        d = min(d, curve(p, vec2<f32>(0.26, 0.12), vec2<f32>(0.10, -0.36), 0.10));
    } else if b < 0.25 {
        d = min(d, stroke(p, vec2<f32>(0.24, -0.08), vec2<f32>(0.34, -0.30)));
    }
    return d;
}

// --- signs -------------------------------------------------------------------

// What one cell of the grid emits.
//
// `p` is cell-local, -0.5..0.5. `px` is one screen pixel measured in those same
// units — the filter width, which is what lets the far layers be tiny without
// disintegrating.
//
// Nothing here looks at a neighbouring cell. Everything a sign draws, halo
// included, is kept inside its own cell by deriving the sign's extent from the
// character size rather than the other way round.
fn sign_light(
    p: vec2<f32>,
    cell: vec2<f32>,
    ks: f32,
    px: f32,
    density: f32,
    melt: f32,
    spread: f32,
) -> vec3<f32> {
    let ha = hash22(cell + ks);
    if ha.x > density {
        return vec3<f32>(0.0);
    }

    let hb = hash22(cell * 1.63 + ks + 3.7);
    let hc = hash22(cell * 2.41 + ks + 9.1);
    let hd = hash22(cell * 3.17 + ks + 5.3);
    let he = hash22(cell * 4.73 + ks + 1.9);
    let hf = hash22(cell * 5.91 + ks + 7.1);

    let t = globals.time;

    // Vertical columns dominate. A shopfront in Tokyo hangs its name down the
    // side of the building because that is the face the street can see.
    let vertical = ha.y < 0.68;
    let count = 1.0 + floor(hb.x * 3.99);

    // Character half-size, shrinking as the count goes up so that a column of
    // four and a single large character both fit their cell with the same
    // margin around them for the halo.
    let gs = mix(0.055, 0.115, hb.y) * mix(1.0, 0.62, (count - 1.0) / 3.0);
    let span = gs * count;

    // Float. Each sign drifts on its own pair of slow periods — this is what
    // stops the grid underneath from reading as a grid.
    let bob = vec2<f32>(
        sin(t * mix(0.10, 0.25, hd.x) + hd.y * TAU),
        cos(t * mix(0.08, 0.22, he.x) + he.y * TAU),
    ) * 0.03;

    var q = p - bob - (hf - 0.5) * 0.05;
    q = rotate2(q, (hf.y - 0.5) * (0.08 + melt * 0.8));

    // The sign's own bounding box, and a cheap reject against it. Most pixels
    // of an occupied cell are nowhere near the sign, and the characters below
    // are by far the most expensive thing in this shader.
    let bounds = select(vec2<f32>(span, gs), vec2<f32>(gs, span), vertical);
    if box_fill(q, bounds + gs * 2.0) > 0.0 {
        return vec3<f32>(0.0);
    }

    // Which character of the sign this pixel falls in. Only that one is ever
    // evaluated, so a column of four costs the same as a single character.
    let axis = select(q.x, q.y, vertical);
    let slot = clamp(floor((axis + span) / (2.0 * gs)), 0.0, count - 1.0);
    let offset = -span + (slot + 0.5) * 2.0 * gs;
    var gq = q;
    if vertical {
        gq.y -= offset;
    } else {
        gq.x -= offset;
    }

    // A minority of signs change what they say, one character at a time and on
    // a phrase boundary, with the dip a real sign makes as it switches.
    let mutates = step(0.72, hc.y);
    let cycle = globals.beat / 16.0 + ha.x * 5.0 + slot * 0.13;
    let era = floor(cycle) * mutates;
    let relight = mix(1.0, smoothstep(0.0, 0.12, fract(cycle)), mutates);

    let id = hash11(dot(cell, vec2<f32>(12.9, 78.2)) + ks + slot * 31.7 + era * 101.3);

    // Melt: the characters squirm. Applied to the whole glyph rather than to
    // its strokes, which keeps them legible as characters while never quite
    // holding still.
    var gp = gq / gs;
    gp += vec2<f32>(sin(t * 1.9 + id * 31.0), cos(t * 1.5 + id * 17.0)) * melt * 0.10;
    gp = rotate2(gp, sin(t * 0.7 + id * 7.0) * melt * 0.20);

    let d = glyph(gp, id) * gs;

    // Stroke width follows character size, floored at the pixel footprint: a
    // sign whose strokes fall between samples has to get *dimmer* rather than
    // break up, so the tube is widened to the filter and its peak scaled down
    // by exactly the amount it was widened. Total emitted light is unchanged,
    // which is what makes the far layers read as a soft glow instead of a
    // boiling mess.
    let w = gs * 0.085;
    let wf = max(w, px * 0.8);
    let fill = w / wf;

    let core = 1.0 - smoothstep(wf * 0.35, wf, d);
    // Deliberately tight: the wide glow comes from the bloom pass downstream,
    // and a halo any broader than this would reach the cell boundary and get
    // cut off square.
    let halo = exp(-d / (gs * 0.30));

    // Faults and chases. A tube on its way out is the single detail that reads
    // as a real street rather than a render.
    var flick = 1.0;
    if hc.x > 0.86 {
        flick = mix(0.25, 1.0, step(0.25, hash11(floor(t * 18.0) + hc.y * 40.0)));
    } else if hc.x < 0.12 {
        flick = 0.55 + 0.45 * sin(t * 2.0 + hc.y * TAU);
    }

    let live = fill * relight * flick;

    // Hue comes from the sign's identity and a slow gradient across the field,
    // never from how bright it is. Colour that follows brightness makes every
    // bright thing the same colour, and the bright things are all the eye sees.
    let hue = globals.seed + dot(cell, vec2<f32>(0.021, 0.013)) + hc.x * spread + t * 0.008;
    let tint = max(saturate_color(palette(globals, hue), 1.25), vec3<f32>(0.0));

    var light = tint * (core * 1.50 + halo * 0.34) * live;

    // A lit tube is hotter at its centre than the gas around it, which is why
    // neon photographs with a white core inside a coloured glow. Adding white
    // here rather than desaturating the sign leaves the colour in the halo,
    // where the eye actually reads it. Kept low: on a near sign the core is
    // hundreds of pixels of it, and any more turns the biggest, most
    // conspicuous signs in the frame white.
    light += vec3<f32>(1.0) * core * live * 0.16;

    // --- the board -----------------------------------------------------------
    // Silhouette variation, and the reason this is not a lattice of lit
    // windows: identical shapes on a grid read as architecture however they
    // are coloured. Some signs are bare tube, some are framed boards, some
    // hang off a rail.
    if he.x > 0.74 {
        let frame = box_edge(q, bounds + gs * 0.45);
        light += tint * (1.0 - smoothstep(wf * 0.4, wf * 1.1, frame)) * live * 1.1;
        light += tint * exp(-frame / (gs * 0.30)) * live * 0.18;
    } else if he.x < 0.16 && count > 1.0 {
        let rail = min(span + gs * 0.5, 0.44);
        var rd = stroke(q, vec2<f32>(-gs * 1.3, rail), vec2<f32>(gs * 1.3, rail));
        rd = min(rd, stroke(q, vec2<f32>(-gs * 0.85, rail), vec2<f32>(-gs * 0.85, span)));
        rd = min(rd, stroke(q, vec2<f32>(gs * 0.85, rail), vec2<f32>(gs * 0.85, span)));
        light += tint * (1.0 - smoothstep(wf * 0.4, wf * 1.1, rd)) * live * 0.9;
    }

    // A panel behind the tube on a few boards, so the sign has a body. Lit by
    // its own tubes rather than filled flat: a flat panel magnified by the near
    // layer is a large evenly-lit rectangle, which is the one thing a projector
    // renders worst — it reads as a grey slab hanging in the black.
    if he.y > 0.82 {
        let panel = 1.0 - smoothstep(0.0, px * 2.0 + 0.004, box_fill(q, bounds + gs * 0.45));
        light += tint * panel * (0.012 + 0.05 * halo) * relight;
    }

    return light;
}

// --- frame -------------------------------------------------------------------

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let t = globals.time;

    var p = centered(uv, globals.resolution);
    // `centered` works in UV space, where +y points *down* the screen. The
    // characters are laid out the way they are written — crown above body,
    // 大's legs splaying towards -y — so the frame is flipped once here and
    // everything downstream reads y-up. Without this every glyph is drawn
    // reflected in the horizontal axis, which is upside down *and* mirrored.
    p.y = -p.y;

    let react = knob(globals, 7u);
    // How many cells carry a sign. The top of the range is deliberately
    // reachable: perspective means the far layer holds most of the signs in the
    // frame however this is set, so the only way to get a *mid*-distance one
    // near enough to read is to pack the grid tighter.
    let density = knob_range(globals, 1u, 0.18, 0.68);
    let base_cell = knob_range(globals, 5u, 0.055, 0.130);
    let spread = knob_range(globals, 6u, 0.20, 1.60);

    // --- the glass -----------------------------------------------------------
    // One domain warp over the whole frame rather than one per layer: near and
    // far bend together, so it reads as looking *through* something moving
    // rather than as each layer wobbling on its own.
    let warp = knob_range(globals, 3u, 0.0, 0.09);
    if warp > 0.001 {
        p += vec2<f32>(
            fbm2(p * 2.3 + vec2<f32>(0.0, t * 0.05), 3),
            fbm2(p * 2.3 + vec2<f32>(5.2, 1.3 - t * 0.04), 3),
        ) * warp;
    }

    // Two periods, both slow enough that the rotation is never seen moving,
    // only noticed to have moved.
    p = rotate2(p, sin(t * 0.021) * 0.05 + sin(t * 0.013) * 0.03);

    // The whole field breathes in on the beat. Tiny — 3% — and it does more
    // for the sense of the visual belonging to the music than anything else
    // here, because it moves every sign at once.
    p *= 1.0 - params.energy * react * 0.03;

    // Where the flight is heading. Signs stream outward from this point and
    // there is nothing but the smallest, dimmest layer at it, so leaving it in
    // the middle of the frame puts a permanent hole in the middle of the
    // composition. Off to one side it reads as flying *past* the signs rather
    // than into them, and wandering it slowly keeps the hole from settling
    // anywhere. Shared by every layer, so the octave hand-off is unaffected.
    p -= vec2<f32>(0.34 * sin(t * 0.017), 0.16 - 0.14 * sin(t * 0.011));

    // --- the flight ----------------------------------------------------------
    let phase = fract(params.zoom);
    let base = floor(params.zoom);
    let px_screen = 1.0 / max(globals.resolution.y, 1.0);

    var color = vec3<f32>(0.0);

    for (var i = 0; i < LAYERS; i++) {
        // Depth, continuous across the octave hand-off. Every per-layer
        // quantity below is a function of this and never of `i` — that is the
        // whole trick. When `phase` wraps, layer i+1 inherits exactly the depth
        // and the seed layer i had an instant earlier, and nothing on screen
        // moves.
        let z = f32(i) + phase;
        let cell_size = base_cell * exp2(z);

        // Content seed, travelling outward with the depth. Taken modulo the
        // zoom wrap so the counter resetting is just another hand-off.
        let index = base - f32(i);
        let ks = (index - ZOOM_WRAP * floor(index / ZOOM_WRAP)) * 7.13 + globals.seed * 31.0;

        // The nearest layer fades out as it grows past the frame and the
        // farthest fades in from nothing, so the two ends of the stack are
        // always at zero where content appears and disappears. The two middle
        // layers are always at full weight, so the frame never goes dark.
        //
        // The fade in is quicker than the fade out on purpose. Signs appear at
        // the vanishing point, and a slow fade there leaves a hole in the
        // middle of the composition; at that size they are a few pixels each
        // and nobody sees them arrive. Leaving is the opposite — a near sign
        // filling a quarter of the frame has to go gently.
        let weight = smoothstep(0.0, 0.65, z) * (1.0 - smoothstep(f32(LAYERS - 1), f32(LAYERS), z));
        // Far is dimmer, but not by much: the far layer is what fills the space
        // around the vanishing point, and dropping it too low leaves that part
        // of the frame empty whatever the density knob says.
        let depth = mix(0.50, 1.0, z / f32(LAYERS));

        // Lateral drift is one offset in cell units shared by every layer,
        // which puts the parallax in for free: the same shift in cell space is
        // `cell_size` times larger on screen for a near layer than a far one.
        let c = p / cell_size + params.sway / base_cell;

        color += sign_light(
            fract(c) - 0.5,
            floor(c),
            ks,
            px_screen / cell_size,
            density,
            params.melt,
            spread,
        ) * weight * depth;
    }

    // --- output --------------------------------------------------------------

    // Half-time and shallow: the room should feel this before it notices it.
    color *= 1.0 + pulse_every(globals, 2.0, 2.2) * params.energy * react * 0.22;

    // The dark is not empty. Pure black beside a saturated bloom reads as a
    // hole punched in the image; a trace of the palette's far end reads as air.
    // Textured and an order of magnitude below the dimmest sign, so it can
    // never be mistaken for a lifted black.
    let air = fbm2(p * 1.6 + vec2<f32>(t * 0.02, -t * 0.015), 3) * 0.5 + 0.5;
    color += max(palette(globals, globals.seed + 0.55), vec3<f32>(0.0)) * air * 0.012;

    // Deliberately a lower range than the other visuals top out at. This one
    // fills the frame with small bright points, and a field of points reads
    // far brighter in a small dark room than its peak value suggests.
    color *= knob_range(globals, 0u, 0.45, 1.70);
    color = saturate_color(color, 1.20);
    color *= vignette(uv, 0.50);
    color *= globals.intensity;
    color = dither(color, in.position.xy);

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}
