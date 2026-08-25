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
    /// How many layers of `video_tex` hold a picture. Zero means no video.
    video_slots: f32,
}

@group(2) @binding(0) var<uniform> globals: Globals;
@group(2) @binding(1) var<uniform> params: TerebiParams;

// The video wall: one layer per decoder, filled by `fete-video`. Bound
// unconditionally — with no clips, no `ffmpeg`, or no `--video` at all this is
// Bevy's fallback texture and `params.video_slots` is zero, which `tuned_to`
// reads as "nothing is on" and every set falls to snow.
@group(2) @binding(2) var video_tex: texture_2d_array<f32>;
@group(2) @binding(3) var video_samp: sampler;

// How many times a wall cell may be cut. Three gives one to eight sets per
// cell, though the falling cut probability means eight is rare — what it buys
// over two is the long tail: the occasional portable wedged between two big
// sets, which is what stops the size distribution reading as "large or medium".
const SPLITS: i32 = 3;
// Barrel distortion of the tube face. The strongest single cue that these are
// glass bottles and not flat panels — a rectangle with square corners reads as
// an LCD however it is coloured.
const CURVE: f32 = 0.10;
// Scan lines per tube. NTSC's, near enough; whether any of them survive to the
// projector is decided per set, by how many pixels tall that set is.
const LINES: f32 = 232.0;
// Width of one decoded video frame, in pixels. Must match `TILE` in
// `fete-video` — it sets the scale of the composite chroma smear, which is
// measured in source pixels rather than screen ones.
const VIDEO_WIDTH: f32 = 320.0;

// --- shapes ------------------------------------------------------------------

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

// --- what is on -------------------------------------------------------------
//
// Two things, now. The wall used to carry nine synthesised programmes — a
// studio, an anime impact frame, a vertical shooter, a platformer, a pseudo-3d
// racer, a title card, a test card — each a pure function of a picture
// coordinate. They are gone: every set that is on is showing footage.
//
// Snow is what remains, and it is not a programme. It is what a television does
// when it has nothing to show, which happens here for two reasons: the instant
// after a set retunes, and the case where there are no feeds at all.

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

// --- the tenth channel: something that was actually on -----------------------

// A set tuned to one of the decoded feeds.
//
// Nine of these channels are pure functions and this one is a texture fetch,
// which makes it the only thing in the show that depends on the world outside
// it. It earns that here and nowhere else: the joke of this visual is a wall of
// televisions, and a wall of televisions where one set is playing what was
// actually on in 1991 recontextualises the other nineteen.
//
// It arrives already inside the tube. The coordinate handed in has been curved
// by the glass, possibly rolled by a lost vertical hold and torn sideways by
// worn tape, and everything downstream — scan lines at the tube's pitch, the
// overscan border, the tube's own tint and flicker, the light thrown on the
// cabinet — is applied to whatever this returns. None of that had to be written
// twice.
fn ch_video(q: vec2<f32>, id: f32, px: f32, spread: f32, feed: f32) -> vec3<f32> {
    // Which feed this set is tuned to, decided by the caller — see `rank`.
    let slot = i32(clamp(feed, 0.0, max(params.video_slots - 1.0, 0.0)));

    // The wall is drawn y-up; a video frame is stored top row first.
    // Clamped rather than wrapped: a set whose picture runs past the edge of
    // the signal smears, the way an overscanning set does, instead of showing
    // the other side of the frame.
    let uv = clamp(vec2<f32>(q.x, -q.y) * 0.5 + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));

    // Composite colour. On a real set the chroma subcarrier carries a fraction
    // of the luminance bandwidth and smears sideways; taking hue from a pair of
    // horizontally offset taps while brightness comes from the sharp one is the
    // whole of that effect. It is the strongest single cue that this is a tape
    // through a tube rather than a video file in a rectangle, and it covers for
    // the source being only 320 pixels across on the way past.
    let bleed = vec2<f32>(1.7 / VIDEO_WIDTH, 0.0);
    let sharp = textureSampleLevel(video_tex, video_samp, uv, slot, 0.0).rgb;
    let west = textureSampleLevel(video_tex, video_samp, clamp(uv - bleed, vec2<f32>(0.0), vec2<f32>(1.0)), slot, 0.0).rgb;
    let east = textureSampleLevel(video_tex, video_samp, clamp(uv + bleed, vec2<f32>(0.0), vec2<f32>(1.0)), slot, 0.0).rgb;
    let smeared = (sharp + west + east) / 3.0;

    let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
    let lum = dot(sharp, luma);

    // Broadcast video is mid-grey nearly everywhere — a studio floor, a lit
    // wall, a face at forty per cent — and dropped in raw among nine channels
    // that are mostly black it reads as a hole cut in the wall rather than as a
    // television in it. A black point takes the grey out, and the exponent
    // carries the top past 1.0 into the bloom the other channels live in.
    //
    // Deliberately not a smoothstep, which is the obvious curve here and the
    // wrong one: it plateaus, and everything above its upper edge comes out at
    // exactly the same value. On footage that means a lit shirt or a studio
    // light arrives as a flat white shape with no modelling in it, which is
    // the one thing that makes video read as a bug rather than as a picture.
    // A power curve is monotone all the way up, so highlights still separate
    // as they clip.
    let base = clamp((lum - 0.05) / 0.95, 0.0, 1.0);
    let level = pow(base, 1.25) * 2.0;

    // Hue from the smeared tap, brightness from the sharp one: the same
    // separation the rest of the show is built on. Dividing the smear by its
    // own luminance keeps its colour and discards its brightness, so a
    // saturated backdrop stays saturated as the level under it is crushed.
    let natural = smeared / max(dot(smeared, luma), 1e-3);

    // And the palette's reading of the same picture. Knob 6 is colour spread
    // everywhere else in this shader, so it is colour spread here too: at the
    // low end the tape plays as it was shot, and at the high end the wall is
    // one colour scheme and the footage has been dragged into it.
    let toned = mix(vec3<f32>(1.0), tint(0.22 + lum * 0.55), 0.85);
    let hue = mix(natural, toned, clamp(spread - 0.2, 0.0, 1.0) * 0.8);

    // Head-switching noise. The bottom two per cent of a VHS field is torn, and
    // every one of these clips is a tape. Widened to the pixel footprint so it
    // survives on the small sets instead of aliasing into a flicker.
    //
    // Scaled by the picture it is tearing, and that matters more than it
    // sounds: at a fixed level the band is the brightest thing on a set showing
    // a night scene, so the wall ends up with a row of bright stripes drawing
    // the eye to exactly the sets that should be reading as dark.
    let torn = smoothstep(-0.94 - px * 2.0, -0.98, q.y);
    let band = hash11(floor(q.y * 90.0) + floor(globals.time * 25.0) * 7.3 + f32(slot));

    return mix(hue * level, vec3<f32>(band) * (0.10 + level * 0.5), torn * 0.8);
}

// What a set is showing.
//
// A wall of televisions has exactly two states worth modelling now: tuned, and
// not. `feeds` being zero is the whole no-video path — no clips, no `ffmpeg`,
// `--no-video` — and it lands on snow, which is what a room of untuned sets
// actually looks like rather than a failure mode bolted on.
fn tuned_to() -> i32 {
    return select(1, 9, params.video_slots >= 1.0);
}

fn channel(ch: i32, q: vec2<f32>, id: f32, t: f32, px: f32, spread: f32, feed: f32) -> vec3<f32> {
    // Per-set framing. Two sets on the same feed must not show the same
    // picture: re-zoomed and knocked off centre they read as two televisions
    // showing one broadcast rather than as one signal wired to both. It is the
    // cheapest variation in the shader and it does more than anything else.
    //
    // Not mirrored, though — that was free variation when the channels were
    // drawn, and on footage it reverses the writing. A wall of backwards
    // Japanese is invisible until it is the only thing anyone can see.
    var v = q;
    let zoom = hash11(id * 5.31 + 0.7);
    v *= mix(0.78, 1.36, zoom);
    v += vec2<f32>(hash11(id * 2.71) - 0.5, hash11(id * 3.37) - 0.5) * 0.20;

    if ch == 9 { return ch_video(v, id, px, spread, feed); }
    return ch_snow(v, id, t, px);
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
    // How many sets are switched on. Deliberately never near one: this is
    // scenery behind a DJ and a projector cannot render black darker than the
    // room already is, so contrast on a wall of lit rectangles is bought only
    // by leaving most of them unlit. Even at the top of the knob a third of the
    // wall is dark, and the dark sets are what the lit ones read against.
    let live_frac = knob_range(globals, 1u, 0.22, 0.68);
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
    // Which of the cell's sub-rectangles this is: one bit per cut, set when the
    // far side was taken. Unique per set within the cell, and the only handle
    // this shader has on *where* a set is rather than on who it is. See `rank`.
    var sub = 0;

    for (var i = 0; i < SPLITS; i++) {
        let h = hash22(vec2<f32>(key * 37.0, f32(i) * 5.3 + 1.7));
        // Falls off with depth, so the wall is mostly one or two sets to a
        // cell with a few cut much finer.
        if h.x > 0.60 - f32(i) * 0.19 {
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
                sub |= 1 << u32(i);
            }
        } else {
            let m = lo.y + extent.y * r;
            if f.y < m {
                hi.y = m;
                key = hash11(key * 7.3 + 0.41);
            } else {
                lo.y = m;
                key = hash11(key * 7.3 + 0.67);
                sub |= 1 << u32(i);
            }
        }
    }

    let set_center = (lo + hi) * 0.5;
    let set_half = (hi - lo) * 0.5;

    // A number that differs between every set in a cell and between every
    // neighbouring cell — a hash would not, and that is the entire point.
    //
    // Two televisions three cabinets apart showing the same thing is the
    // clearest tell that a wall is generated, and it is a birthday problem: draw
    // thirty sets from twelve feeds at random and a collision is not unlikely,
    // it is near certain. Picking by position instead of by hash makes the
    // common case — sets next to each other — provably different, which is the
    // case the eye is actually checking. Coprime strides so the two axes do not
    // alias into each other.
    let rank = sub + i32(cid.x) * 3 + i32(cid.y) * 5;

    // Eight independent draws per set. Every property below takes its own —
    // sharing one ties two unrelated things together across the entire wall,
    // and a wall where the sets with wide bezels are also the ones that have
    // lost vertical hold reads as a pattern even when nobody can name it.
    let ha = hash22(vec2<f32>(key * 13.1, key * 29.7 + 3.3));
    let hb = hash22(vec2<f32>(key * 5.7 + 1.9, key * 41.3));
    let hc = hash22(vec2<f32>(key * 23.9 + 7.1, key * 3.3));
    let hd = hash22(vec2<f32>(key * 47.7 + 11.3, key * 17.9 + 5.1));

    // One screen pixel, in cell units. Everything small is widened to it.
    let px_cell = (1.0 / max(res.y, 1.0)) / size;

    // Cabinets do not touch. A wall of sets is stacked, and the black between
    // them is what stops the whole thing reading as one lit panel. The gap
    // varies per set: a uniform one is a grid however the sets are sized.
    let gap = min(set_half.x, set_half.y) * mix(0.07, 0.17, hash11(key * 61.3)) + 0.006;
    let cab_half = max(set_half - gap, vec2<f32>(0.015));

    var q = f - set_center;
    // Nobody stacked these straight.
    q = rotate2(q, (ha.x - 0.5) * 0.055);

    // Moulded plastic, so the corners are generously rounded and no two makes
    // agree by how much.
    let corner = min(cab_half.x, cab_half.y) * mix(0.14, 0.30, hd.x);
    let cab = round_box(q, cab_half, corner);

    var col = vec3<f32>(0.0);

    if cab < 0.0 {
        // The tube inside the moulding. A wide bezel and a small picture is a
        // portable; a narrow one is somebody's big set.
        let bezel = min(cab_half.x, cab_half.y) * mix(0.10, 0.26, ha.y);

        // The apron: the extra moulding under the glass. Every television ever
        // built puts its speaker and its tuning controls below the screen, so
        // the plastic there is deeper than it is anywhere else, and the tube
        // sits high in the cabinet rather than centred in it. That asymmetry is
        // most of what separates a television from a monitor — a box with the
        // picture in the middle of it reads as a flat panel however it is lit,
        // which is exactly what this wall must not look like.
        let apron = bezel * mix(0.45, 1.9, hb.x);
        var inner = max(
            vec2<f32>(cab_half.x - bezel, cab_half.y - bezel - apron * 0.5),
            vec2<f32>(0.008),
        );
        // 4:3 — or, on the odd set, the 3:4 of an arcade monitor stood on end.
        let ratio = select(4.0 / 3.0, 3.0 / 4.0, hd.y > 0.94);
        if inner.x / inner.y > ratio {
            inner.x = inner.y * ratio;
        } else {
            inner.y = inner.x / ratio;
        }

        // Everything from here is measured from the centre of the glass, which
        // is above the centre of the box.
        let screen_rise = vec2<f32>(0.0, apron * 0.5);
        let qt = q - screen_rise;

        let tube = round_box(qt, inner, min(inner.x, inner.y) * 0.16);
        let s = qt / inner;
        // A pixel, in picture units. This is the number the whole visual is
        // filtered against: a set can be a third of the frame or forty pixels
        // across, and every feature below is widened to whichever it is.
        let px = px_cell / max(inner.y, 1e-4);

        // --- standby -------------------------------------------------------
        //
        // Whether this set is switched on *at the moment*, not for the whole
        // night. A fixed roll per set is what this used to be, and it makes the
        // wall a still life: the dark ones are always the same dark ones, and
        // after a minute the eye has the layout memorised and stops looking.
        //
        // The schedule runs on the show clock rather than on the programme
        // clock, and that separation is the point. How often somebody switches
        // a television off has nothing to do with how often the ones that are
        // on change channel, and hanging both on the same knob was what made
        // twenty sets read as one machine. Each set draws its own period and
        // its own offset, so they go dark one at a time over minutes.
        let standby_period = mix(90.0, 340.0, hash11(key * 53.7 + 1.9));
        let standby_phase = globals.beat / standby_period + hash11(key * 67.1 + 6.3) * 23.0;
        let dead = hash11(floor(standby_phase) * 5.7 + key * 91.3) > live_frac;
        let was_dead = hash11(floor(standby_phase - 1.0) * 5.7 + key * 91.3) > live_frac;
        // Beats since this set last reconsidered. In beats and not in phase
        // units, so the collapse below takes the same time on a set that
        // reconsiders every forty seconds as on one that takes three minutes.
        let since_switch = fract(standby_phase) * standby_period;
        // How long this set sits on a channel, in programme units.
        //
        // Wide on purpose, and squared to push most sets towards the long end.
        // Two things go wrong with a narrow range: the wall cuts too often to
        // watch, and — worse — every set cuts at roughly the same rate, so even
        // with the phases scattered the whole thing pulses. With a set that
        // holds for a quarter of a minute next to one that holds for two, no
        // rhythm ever establishes itself.
        let dwell = mix(3.0, 26.0, hash11(key * 19.7 + 2.3) * hash11(key * 43.1 + 8.9));
        let phase = params.programme / dwell + hash11(key * 3.7) * 11.0;
        let age = fract(phase);
        let synced = params.sync > 0.5;
        let ch = tuned_to();

        // Which feed a video set is tuned to.
        //
        // Position, not identity: `rank` differs between every set in a cell
        // and between neighbouring cells, so two sets near each other cannot
        // draw the same clip. The era rotates the whole assignment as sets cut,
        // so a set does not sit on one feed all night, and the modulus keeps it
        // inside whatever `fete-video` actually managed to start.
        //
        // A sync window overrides it: the entire point of the wall ganging up is
        // that every set is showing *the same broadcast*, so there the feed comes
        // from the programme clock like the channel does.
        let feeds = max(params.video_slots, 1.0);
        let spread_feed = f32((rank + i32(floor(phase))) % i32(feeds) + i32(feeds)) % feeds;
        let wall_feed = floor(hash11(floor(params.programme * 0.35) * 29.0 + globals.seed * 41.0) * feeds);
        let feed = select(spread_feed, wall_feed, synced);

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
            let speed = mix(0.04, 0.5, hc.y);  // rate of the roll, not of the fault
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

        // --- switching off, and on -----------------------------------------
        //
        // A CRT losing its supply does not fade: the deflection collapses
        // vertically first, so the picture folds into a bright horizontal line
        // across the middle of the tube, and that line shrinks to a dot which
        // takes a second or two to die. It is the most recognisable thing a
        // television does, and it is the whole reason a set going dark reads as
        // somebody switching it off rather than as a rectangle being masked out
        // of the wall.
        //
        // The line is bright because the same beam energy is being painted into
        // a fraction of the height, so the level goes up as the squash goes
        // down. Capped, or a set switching off would be the brightest thing in
        // the room for a frame — the opposite of what this is for.
        // Three beats, and then the tube is simply off. The elapsed-time bound
        // is not decoration: the state change alone is true for the whole
        // standby period, and without it every switched-off set on the wall
        // holds the collapsed line at five times brightness for a minute at a
        // time, which is both wrong and the brightest thing in the room.
        var live = select(1.0, 0.0, dead);
        var dying_dot = 0.0;
        if dead && !was_dead && since_switch < 3.0 {
            let fold = clamp(since_switch / 0.9, 0.0, 1.0);
            let squash = max(1.0 - fold, 0.02);
            cu.y /= squash;
            live = step(abs(cu.y), 1.0) * min(1.0 / squash, 5.0);
            // Then the dot, in tube coordinates rather than collapsed ones.
            let fade = clamp((since_switch - 0.8) / 2.2, 0.0, 1.0);
            let r = max(px * 1.5, 0.02);
            dying_dot = (1.0 - fade) * (1.0 - fade)
                * exp(-dot(cs, cs) / (r * r)) * step(0.55, fold);
        } else if !dead && was_dead && since_switch < 2.4 {
            // Coming back is the other way round and much slower: a cold tube
            // takes a couple of seconds to reach brightness.
            live = smoothstep(0.0, 2.4, since_switch);
        }

        // Half-time, with a phase per set — the wall shimmers with the track
        // instead of pumping as one thing.
        let hit = pow(1.0 - fract(globals.beat * 0.5 + hash11(key * 9.1)), 3.0) * react;

        var pic = channel(ch, cu, content_id, content_t, px, spread, feed);

        // The instant after a channel change: a set shows a frame of noise and
        // takes a moment to lock. Cheap, and it is what makes a cut read as a
        // television changing channel rather than a shader switching branch.
        if age < 0.045 {
            pic = mix(pic, ch_snow(cu, content_id, content_t, px), 1.0 - age / 0.045);
        }
        pic *= live;
        // The dot is the tube's own phosphor, not the picture, so it is added
        // rather than multiplied and it survives the picture being gone.
        pic += tint(0.30) * dying_dot * 1.6;

        // Every tube is a different age and a different make. Brightness and
        // colour vary per set and neither follows the other.
        // Wide, and wider than looks reasonable written down. With more sets on
        // the wall than there are channels, some of them are always showing the
        // same programme as somebody else, and no amount of detail inside a
        // channel fixes that — what fixes it is that the two sets are visibly
        // different *televisions*. A tube twenty years old with a drifted
        // colour balance next to a newer one is the cheapest way to say so, and
        // it costs two hashes.
        let gain = mix(0.60, 1.45, hash11(key * 3.9));
        let tube_tint = mix(
            vec3<f32>(1.0),
            tint(0.25 + hash11(key * 6.1) * 0.55),
            0.18 + 0.28 * spread,
        );
        var flick = 1.0;
        let ailing = hash11(key * 71.9 + 4.7);
        if ailing > 0.94 {
            // One set in sixteen is on its way out. Shallow and slow: a tube
            // with a dying capacitor breathes, it does not strobe, and a wall
            // of hard-switching sets is both wrong and genuinely unpleasant to
            // stand in front of for an hour. The sine carries most of it and
            // the sparse drop-out is the occasional harder blink.
            let breath = sin(content_t * mix(4.0, 11.0, ailing) + key * 30.0) * 0.5 + 0.5;
            flick = 1.0 - breath * 0.10
                - step(0.93, hash11(floor(content_t * 9.0) + key * 20.0)) * 0.12;
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
            // --- the moulding ------------------------------------------------
            //
            // Almost all of the light on the plastic is thrown there by the
            // tube. Sampling the picture once at its centre is enough, and it
            // is what lights the whole cabinet with a cut, a wipe or a hit.
            let src = channel(ch, vec2<f32>(0.0, -0.12), content_id, content_t, px, spread, feed);
            // `live` and not `dead`, so the cabinet dims with the collapse and
            // warms back up with the tube instead of snapping at either end.
            let lum = (dot(max(src, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722)) * 0.7 + 0.02)
                * gain * flick * clamp(live, 0.0, 1.0);

            // Where this point sits on the front face, top to bottom and out to
            // the rim. Both are wanted repeatedly below and neither costs an
            // sdf evaluation: the box is axis-aligned, so its own coordinate is
            // the shading term.
            let up = clamp(q.y / max(cab_half.y, 1e-4), -1.0, 1.0);
            let rim_w = max(min(cab_half.x, cab_half.y) * 0.13, px_cell * 1.4);
            let rim = 1.0 - smoothstep(0.0, rim_w, -cab);

            // The plastic itself. Beige, grey or near-black depending on the
            // make, and a couple of orders under the picture.
            let plastic = mix(vec3<f32>(1.0), tint(0.60 + hd.x * 0.25), 0.55)
                * mix(0.55, 1.25, hash11(key * 8.3));

            var case_col = mix(vec3<f32>(1.0), tube_tint, 0.7)
                * exp(-tube / max(bezel * 0.45, 1e-4)) * lum * 0.50;

            // The room, from above. Deliberately a gradient and not a constant:
            // a flat floor under this term is what an early version had, and it
            // put a dull even halo around every live set that read as two dozen
            // grey slabs hanging in the black. A gradient costs the same and
            // gives the box an *up*, which is the whole difference between a
            // lit rectangle and an object with a top on it.
            case_col += plastic * (0.004 + 0.020 * max(up, 0.0) * max(up, 0.0));

            // The chamfer. Moulded cabinets have a bevelled front edge, and a
            // bright line along the top of it with the bottom edge falling into
            // its own shade is more of what says *box* than any amount of fill.
            case_col += plastic * rim * (0.030 * max(up, 0.0) + 0.004);
            case_col *= 1.0 - rim * max(-up, 0.0) * 0.55;

            // The glass is sunk behind the opening, so the top lip of the bezel
            // shades the plastic immediately under it and the bottom lip catches
            // the picture. Without this the bezel is a flat frame painted on.
            let lip = 1.0 - smoothstep(0.0, bezel * 0.7, tube);
            let lip_up = clamp(qt.y / max(inner.y, 1e-4), -1.0, 1.0);
            case_col *= 1.0 - lip * max(lip_up, 0.0) * 0.45;

            // The speaker grille and the tuning controls, in the apron. Three
            // or four pixels tall on the projector, so what registers is a band
            // of texture with a couple of points of light beside it — which is
            // exactly what it is. Faded out rather than sampled once the slots
            // stop resolving, like the scan lines.
            let deck = qt.y + inner.y;
            let slot_h = apron * 0.30;
            if deck < 0.0 && deck > -apron * 1.1 && apron > px_cell * 2.5 {
                let slot_w = max(apron * 0.22, px_cell * 1.6);
                let legible = smoothstep(px_cell * 0.8, px_cell * 2.2, slot_w);
                let grille = fract((qt.x + inner.x * 0.55) / slot_w);
                let band = smoothstep(px_cell / slot_w, 0.0, abs(deck + apron * 0.55) - slot_h * 0.5);
                case_col *= 1.0 - band * step(qt.x, inner.x * 0.25)
                    * smoothstep(0.35, 0.65, grille) * legible * 0.5;
            }

            col += case_col;

            // A little of the picture reaches the outer edge of the cabinet,
            // which is the only thing separating one dark set from the next.
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

    // Master brightness. The top of this range used to be 1.7, which on a
    // projector in a room with a dancefloor in it is enough to light faces —
    // and a screen that lights the room is too bright whatever it looks like on
    // a laptop in the dark.
    col *= knob_range(globals, 0u, 0.30, 1.15);
    col = saturate_color(col, 1.15);
    col *= vignette(uv, 0.45);
    col *= globals.intensity;
    col = dither(col, in.position.xy);

    return vec4<f32>(max(col, vec3<f32>(0.0)), 1.0);
}
