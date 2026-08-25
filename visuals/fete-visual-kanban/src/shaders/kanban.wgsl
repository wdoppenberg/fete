// Kanban — 看板, "signboard". A field of Japanese neon signs floating past in
// the dark.
//
// The signs say real things. Every one of them carries a word from the
// vocabulary in `lexicon.rs` — drink, noodles, baths, pachinko, a place name,
// the phrase a shopfront uses to say it is open, and the words of 酉の市 — set
// in real characters taken from a real Japanese face. What ships is not a font
// and not a picture of the characters: `glyphs.png` holds one cell per
// character of the *distance* to its strokes, which is exactly what a neon tube
// is drawn from, and what lets a single 128-pixel cell serve a sign filling a
// third of the frame and a sign four pixels tall in the same frame.
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

// Room in the uniform for the weight-expanded vocabulary. Must match
// `MAX_SLOTS` in `lexicon.rs`.
const MAX_SLOTS: u32 = 128u;

struct KanbanLexicon {
    // Atlas columns, atlas rows, draw slots in use, and the atlas index of the
    // long vowel mark — the one character whose shape depends on whether the
    // sign is set down the column or across.
    grid: vec4<f32>,
    // One row per draw slot: up to four glyph indices, `-1.0` for the unused
    // tail. A word that should come up more often occupies more rows, which is
    // why picking one is a single lookup with no distribution to walk.
    slots: array<vec4<f32>, MAX_SLOTS>,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: KanbanParams;
@group(2) @binding(2) var<uniform> lexicon: KanbanLexicon;
@group(2) @binding(3) var atlas: texture_2d<f32>;
@group(2) @binding(4) var atlas_sampler: sampler;

// Fed by the quality tier — see `Kanban::specialize`.
const LAYERS: i32 = #{LAYERS};
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

// --- characters --------------------------------------------------------------

// How much of an atlas cell the character's em square covers, inverted: the
// cell is this many em across. Must match `ATLAS_EM` in `lexicon.rs`.
//
// The margin around the character is not padding. It is where the distance
// field lives — a character drawn edge to edge in its cell would have nowhere
// to record how far away it is, and its glow would stop dead on the cell
// boundary.
const CELL_EM: f32 = 1.0 / 0.62;
// The distance at full black and full white, in em. Must match `ATLAS_RANGE`.
const ATLAS_RANGE: f32 = 0.8;
// Half the light weight's stroke width, in em, and how much is added to it to
// make a tube. The face the atlas is baked from is the thinnest weight that
// still holds its shape, and the width the signs are actually lit at is set
// here — a heavy face would close up the inside of a dense character (繁, 舞,
// 薬) the moment it was asked to glow, and no amount of shader can reopen it.
const STROKE: f32 = 0.024;
const THICKEN: f32 = 0.018;
// Half a texel of the atlas, in cell units — the inset that keeps a sample
// from reaching into the character next door.
const ATLAS_INSET: f32 = 0.5 / 128.0;

// Signed distance to one character, in em, at `p` em from the centre of its
// square. Negative inside a stroke.
//
// `turn` sets the character a quarter turn, for the long vowel mark: ラーメン
// runs the dash across the line and down the column, and it is the only
// character in the vocabulary that is not the same shape both ways.
fn glyph(index: f32, p: vec2<f32>, turn: bool) -> f32 {
    var q = p;
    if turn {
        q = vec2<f32>(q.y, -q.x);
    }

    let cols = lexicon.grid.x;
    let col = index - cols * floor(index / cols);
    let row = floor(index / cols);

    // Into the cell, and clamped to it: the field outside is another
    // character's, and one texel of bleed at this magnification is a stray
    // stroke hanging in the air beside the sign.
    var t = vec2<f32>(0.5 + q.x / CELL_EM, 0.5 - q.y / CELL_EM);
    t = clamp(t, vec2<f32>(ATLAS_INSET), vec2<f32>(1.0 - ATLAS_INSET));

    let uv = (vec2<f32>(col, row) + t) / lexicon.grid.xy;
    // Sampled at an explicit level: this runs inside branching that neighbouring
    // pixels do not agree on, where an implicit derivative is undefined.
    let field = textureSampleLevel(atlas, atlas_sampler, uv, 0.0).r;
    let d = (0.5 - field) * (2.0 * ATLAS_RANGE);

    // Past the cell the sample is clamped and the field stops falling away,
    // which would leave the glow with a square edge on it. The cell's own box
    // never overstates the distance to something inside the cell, so taking
    // whichever is larger keeps the falloff going with nothing to sample.
    return max(d, box_fill(q, vec2<f32>(CELL_EM * 0.5)));
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

    // A minority of signs change what they say, on a phrase boundary and all
    // at once — a word only means anything whole, so this is one sign
    // alternating between two messages rather than its characters turning over
    // independently. The dip is what a real sign does as it relights.
    let mutates = step(0.72, hc.y);
    let cycle = globals.beat / 16.0 + ha.x * 5.0;
    let era = floor(cycle) * mutates;
    let relight = mix(1.0, smoothstep(0.0, 0.12, fract(cycle)), mutates);

    // What this sign says: one row of the vocabulary, held until it changes.
    // Everything about the sign follows from the word — how many characters,
    // how large they are, how much board there is to frame — rather than the
    // word being cut to fit a sign that was already sized.
    let pick = hash11(dot(cell, vec2<f32>(12.9, 78.2)) + ks + era * 101.3);
    let slots = lexicon.grid.z;
    let word = lexicon.slots[u32(clamp(floor(pick * slots), 0.0, slots - 1.0))];

    // How long the word is. The tail of the row is -1.
    var count = 1.0;
    if word.y >= 0.0 { count = 2.0; }
    if word.z >= 0.0 { count = 3.0; }
    if word.w >= 0.0 { count = 4.0; }

    // Vertical columns dominate. A shopfront in Tokyo hangs its name down the
    // side of the building because that is the face the street can see.
    let vertical = ha.y < 0.68;

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
    // of an occupied cell are nowhere near the sign, and everything below is
    // the expensive half of this shader — the only texture fetch in it
    // included.
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

    // Which character of the word that is. A column is read downwards and the
    // axis runs upwards, so a vertical sign takes them in reverse — get this
    // the wrong way round and every sign in the frame is spelled backwards.
    let index = word[u32(select(slot, count - 1.0 - slot, vertical))];
    let turn = vertical && abs(index - lexicon.grid.w) < 0.5;

    // The character's em square, a shade under the slot it sits in. The gap is
    // what keeps a column from reading as one tall smear once the halo is on.
    let em = gs * 2.0 * 0.78;

    // Melt: the characters squirm. Applied to the whole character rather than
    // to its strokes, which keeps it legible as a word while never quite
    // holding still.
    let phase = hash11(dot(cell, vec2<f32>(12.9, 78.2)) + ks + slot * 31.7);
    var gp = gq / em;
    gp += vec2<f32>(sin(t * 1.9 + phase * 31.0), cos(t * 1.5 + phase * 17.0)) * melt * 0.10;
    gp = rotate2(gp, sin(t * 0.7 + phase * 7.0) * melt * 0.20);

    // Signed, and thickened into a tube on the way out of em.
    let d = (glyph(index, gp, turn) - THICKEN) * em;

    // Strokes narrower than the pixel they land on have to get *dimmer* rather
    // than break up, so the tube is widened to the filter and its peak scaled
    // down by exactly the amount it was widened. Total emitted light is
    // unchanged, which is what makes the far layers read as a soft glow instead
    // of a boiling mess.
    let hw = em * (STROKE + THICKEN);
    let aa = max(px * 0.8, 1e-6);
    // What the tube is actually drawn at, and what that costs in brightness.
    let wf = max(hw, aa);
    let widen = wf - hw;
    let fill = hw / wf;

    let core = 1.0 - smoothstep(-aa * 0.5, aa * 0.5, d - widen);
    // Deliberately tight: the wide glow comes from the bloom pass downstream,
    // and a halo any broader than this would reach the cell boundary and get
    // cut off square.
    let halo = exp(-max(d, 0.0) / (em * 0.19));

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
    // Runs on every pixel, sign or no sign, so it is one of the few costs
    // here that no early-out avoids. Fed by the quality tier.
    let air = fbm2(p * 1.6 + vec2<f32>(t * 0.02, -t * 0.015), #{AIR_OCTAVES}) * 0.5 + 0.5;
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
