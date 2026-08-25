# fete

A creative-coding framework for live GPU visuals, built on Bevy 0.19 and wgpu.

Made for one job: a 4×3m projection behind a DJ, running itself all night.
Everything in it is shaped by that. The screen is scenery, not the act, so the
visuals are dark, slow and react at half-time. Nobody is operating it, so an
autopilot cycles visuals, morphs palettes and slowly wanders the parameter
space. And it is all phrased against a beat clock rather than wall-clock
seconds, so it belongs to the music without anyone driving it.

## Layout

```
crates/fete-core          the framework — clock, modulation, palette, camera rig, visual switching
crates/fete-app           the shell — window, projector setup, keyboard control, HUD, captures
crates/fete-video         clips decoded into an array texture, for Terebi's tenth channel
visuals/fete-visual-sprawl Sprawl, an analytic megacity seen from a tower
visuals/fete-visual-neon  Neon City, a raymarched city, low-poly and owned
visuals/fete-visual-slime Slime, three physarum species in a cycle (compute)
visuals/fete-visual-kanban Kanban, Japanese neon signage floating past
visuals/fete-visual-yama  Yama, a volcanic cone at dusk, circled slowly
visuals/fete-visual-terebi Terebi, a wall of CRT sets playing late-night television
visuals/fete-visual-kura  Kura, three flocks and the geometry between them
apps/fete-show            the combined show: every visual, running itself
```

Each visual crate is both a library and a binary. Run one on its own while you
work on it; the combined show adds the same plugin alongside the others.

```sh
cargo run -p fete-visual-sprawl --release      # one visual, fast iteration
cargo run -p fete-visual-kanban --release
cargo run -p fete-visual-slime --release       # compute needs release
cargo run -p fete-visual-yama --release
cargo run -p fete-visual-terebi --release
cargo run -p fete-visual-kura --release        # CPU sim, release is not optional
cargo run -p fete-show --release               # the whole set, on autopilot
cargo run -p fete-show --release -- --fullscreen --no-hud
cargo run -p fete-show --release -- --start neon --manual
cargo run -p fete-show --release -- --start yama --no-rotate   # hold one visual
cargo run -p fete-show --release -- --quality low             # what a Pi renders
cargo run -p fete-show --release -- --no-video               # no clips on the televisions
```

## Running unattended

The autopilot is on by default. It changes visual every 192 beats through a
bleed transition, morphs the palette on a deliberately coprime 260-beat period
so visual/colour pairings rarely repeat, and continuously drifts every macro
knob nothing else is driving. A single visual held for ten minutes is never
quite the same twice, and a four-hour night does not loop.

Press `C` to switch it off and take over.

Rotation is separable from the rest of it. `V` — or `--no-rotate` at startup —
pins the visual while the palette keeps morphing and the knobs keep drifting,
which is what is wanted when one piece happens to suit the track that is
playing: the screen stays on it without going static for as long as it stays
there. The HUD reads `autopilot/held` while rotation is pinned, against
`autopilot/192b` when it is running and `manual` when the whole thing is off.

## Bleed transitions

Visuals do not cut, and they no longer fade through black. The frame that is
on screen stays there and bleeds away over the next few beats while the
incoming visual plays underneath it at full brightness — a double exposure that
resolves rather than a gap.

It is one extra full-screen pass on the camera, sitting in HDR before bloom. It
writes its result to two places at once: the picture, and a history texture it
reads back next frame. At rest it keeps nothing and the pass is a copy; at a
cut it holds on to what the history is carrying, displaces it a little every
frame, and adds it over the top of whatever is being drawn now.

Six of them, chosen at random per change and never the same one twice running:

```text
smear     wind — the frame streaks off in one slowly turning direction
dissolve  erosion — it tears away in patches with a glowing edge
melt      gravity — columns of it sag out of the bottom of the screen
swirl     a drain — it rotates into the centre, faster the closer it gets
burn      fire — the dark parts go first, the neon holds on until last
rush      speed — it magnifies past the viewer, dragging colour behind it
```

Because the history is the composited output rather than the outgoing visual,
none of this knows which visuals are involved, or that a visual changed at all.
Every pair in the set gets it, including visuals nobody has written yet.

`Transition` owns the settings — `beats` for how long a change takes,
`trail_beats` for how long a pixel of the old frame survives, `warp` for how
far it travels, `style` to pin one instead of rotating. Setting
`Autopilot::fade_beats` above zero brings the old fade to black back.

## Screen shape

Nothing assumes an aspect ratio. Visuals compose from aspect-corrected
coordinates, and the signage grid in Neon scales its cell counts with the
window, so the same show works on 4:3, 16:9 or anything reasonable between.

`--aspect 4:3` masks the output to a shape with black bars — worth it only when
the projector's shape genuinely differs from the output's and the extra image
would otherwise land on the wall.

## Quality tiers

Every visual here was sized by eye against a desktop GPU, which is the right way
to build them and the wrong way to ship them. A Raspberry Pi 4's VideoCore VI
has on the order of a five-hundredth of the arithmetic throughput they were
tuned on, and a 110-step raymarch does not become a 110-step raymarch that runs
slower — it becomes a slideshow.

So cost is a dial. `--quality high|medium|low` (or `FETE_QUALITY`) sets it, and
`--render-scale` (or `FETE_RENDER_SCALE`) overrides the resolution part on its
own. With neither, the adapter is probed at startup and anything that plainly
will not hold framerate — a Broadcom VideoCore, a software renderer — drops to
`low` with a line in the log saying so. The probe only ever moves *down*:
guessing that a GPU is fast is how you get a black screen in front of an
audience.

| | high | medium | low |
|---|---|---|---|
| render scale | 1.0 | 0.75 | 0.5 |
| Neon march / draw distance | 110 / 78 | 80 / 62 | 56 / 48 |
| Sprawl block march / density octaves | 64 / 3 | 40 / 2 | 24 / 2 |
| Yama march / bisect / lake reflection | 48 / 6 / yes | 32 / 5 / yes | 20 / 3 / no |
| Kanban signage layers | 4 | 3 | 2 |
| Slime grid / agents | 1920×1080 / 3.0M | 1280×720 / 1.3M | 640×360 / 300k |
| Kura trail / flow history | 56 / 10 | 32 / 6 | 16 / 3 |
| grade tilt-shift taps | 10 | 6 | 3 |

**`high` is the reference and nothing may quietly change it.** Every value in
that column is exactly what the visual was authored at, and there are tests
pinning the ones most likely to drift. The single deliberate exception is that
MSAA is now explicitly off: `Msaa` is a required component of `Camera` and
defaults to four samples, so the show had been paying for a 4× HDR attachment
and its resolve on content that is one fullscreen quad with no geometric edges
in it.

Two things are worth knowing about the mechanism. Shader loop bounds arrive as
*shader defs* through `Material2d::specialize`, not as uniform fields — partly
because `FeteGlobals` cannot grow a field on this toolchain, but mostly because
a def becomes a `const` in the generated WGSL and the loop stays unrollable.
And render scale is a sampling-rate change and nothing else: the stage renders
into a smaller image and a second camera stretches it over the window, but
`resolution` still reports logical pixels, so scanlines, grain and vignette keep
the size they were tuned at. At scale `1.0` none of that machinery exists — no
image, no second camera, no extra blit — and the pipeline is exactly what it was.

The one cost the render scale cannot touch is Slime, whose simulation runs at
its own resolution and is stretched by the display material. It is told
separately, and because its buffers are allocated while the plugin builds — before
a render device exists for the probe to look at — the auto-probe is too late to
shrink it. Pass `--quality low` explicitly on hardware that needs it; the probe
says so in its warning when it lowers the tier on its own.

## What a visual is

A `Material2d` painted on a quad that fills the viewport. That constraint is the
whole design: it means a new visual is a WGSL file plus a small struct, and it
means every visual inherits the same HDR pipeline, bloom, tonemapping, beat
clock and colour scheme without having to coordinate with any other.

```rust
#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
struct Ripples {
    #[uniform(0)]
    globals: FeteGlobals,
}

impl Material2d for Ripples {
    fn fragment_shader() -> ShaderRef {
        "embedded://my_crate/shaders/ripples.wgsl".into()
    }
}

impl Visual for Ripples {
    const ID: VisualId = "ripples";
    const NAME: &'static str = "Ripples";
    fn globals_mut(&mut self) -> &mut FeteGlobals { &mut self.globals }
}

app.add_visual::<Ripples>();
```

Visuals that need more than a fragment shader — a compute simulation, extra
geometry — add a plugin alongside and still present through a fullscreen
material. `fete-visual-slime` is the worked example for compute;
`fete-visual-kura` is the worked example for geometry.

### The contract

`FeteGlobals` is the one struct shared between Rust and WGSL. Its shader mirror
lives in `fete-core/src/shaders/globals.wgsl`; changing one means changing both.

| field | |
|---|---|
| `resolution` | render target size in pixels |
| `time`, `delta` | seconds |
| `beat`, `beat_phase`, `bar_phase` | musical position |
| `pulse` | decaying per-beat envelope |
| `seed` | fresh each activation, so a visual varies between appearances |
| `intensity` | master fade; multiply your final colour by it |
| `audio` | `(level, bass, mid, high)` |
| `macros_a`, `macros_b` | the eight knobs |
| `palette_*` | cosine gradient coefficients |

Beat subdivisions are *derived* rather than stored: `pulse_every(g, 2.0, 2.5)`
gives a half-time envelope and `phrase_of(g, 16.0)` a phrase ramp, both from
`g.beat`. Prefer half-time for anything atmospheric — reacting on every beat
makes a visual twitch along with the kick and compete with the music.

> **Do not add fields to `FeteGlobals`.** On this toolchain (Bevy 0.19,
> naga_oil 0.22), growing the struct by even one `f32` makes every material
> fail validation with `Entry point fragment at Fragment is invalid — invalid
> function call`, pointing at the calls that take a `Globals`. Removing the
> field fixes it. Until that is understood, derive what you need from the
> fields already there. This is the one sharp edge in the framework.
>
> The bug is specific to *this* struct — `Grade` grew a field without complaint
> — but it is why the quality tier reaches shaders as a shader def rather than
> as a uniform.

A visual that wants to be cheaper on weak hardware keeps a `Tier` field, names
it in `#[bind_group_data(Tier)]`, assigns it in `set_quality`, and pushes its
own table in `Material2d::specialize`:

```rust
fragment.shader_defs.extend(tier.shader_defs());
fragment.shader_defs.push(ShaderDefVal::Int(
    "MARCH_STEPS".into(),
    tier.pick(110, 80, 56),
));
```

with `const MARCH_STEPS: i32 = #{MARCH_STEPS};` in the WGSL. The framework never
guesses what to cut — it only says how much, and the visual decides what that
buys. Anything already cheap enough to run everywhere (Terebi) implements none
of it and inherits the default no-op.

Shader helpers ship alongside it: `fete::globals` (coordinates, knobs,
beat subdivisions), `fete::noise` (hashes, fbm, ridged, curl, kaleidoscope
folds), `fete::palette` (cosine gradients, Oklab, vignette, dither).

## The grade

Every visual is seen through one post-process pass on the camera
(`fete_core::grade`), and it is most of why the show looks like one thing.
Scanlines, chromatic aberration, grain and a slow tape wobble, all low enough
that none is individually noticeable — together they stop the image looking
like it came out of a computer.

It also carries **tilt-shift**: a sharp horizontal band with everything above
and below softening. The mask is a function of screen position alone, so it
needs no depth buffer — and on a shot looking down at a plane, screen height
*is* distance, which makes it a free depth-of-field. It doubles as the cheapest
possible anti-aliasing for a near field full of sub-pixel detail.

And `exposure` — the one number to turn down when the visuals are competing
with the room.

## Sprawl

The densest of the set, and the cheapest per pixel. Written against the aerial
Tokyo photographs in `inspiration/`.

Nothing about the fine city is marched. A ray meets the ground plane
analytically — one ray-plane intersection — and the city is then a *texture
function* of the world position it landed on. Cost is constant per pixel and
completely independent of how much city is visible, so the light count is
effectively unbounded: every pixel out to the horizon can carry its own lights.
Only buildings with real vertical relief are marched, on a coarse ten-unit grid
where a ray crosses few cells.

The piece that makes that work is **filtering**. A light is far smaller than a
pixel at any real distance, so point-sampling a narrow gaussian mostly misses
it — an early version rendered a complete city that was forty times too dim to
see. Each light is instead widened to at least the pixel footprint and its peak
scaled by the area ratio, leaving total emitted energy unchanged. A pixel then
returns the correct *average* over the ground it covers: crisp points near, a
smooth glow far, no aliasing anywhere, and density that can go as high as you
like.

It also carries **atmospheric dispersion**. The air is a weak prism: blue
refracts more than red, so a light low on the horizon has its colours lifted by
different amounts and becomes a tiny vertical spectrum. On the ground plane
"up the screen" is "further away", so the split runs along the view direction,
and because dispersion is a fixed *angular* quantity it scales exactly as the
pixel footprint does — a constant separation in pixels, correct at every
distance. Keep it well under one pixel: once the split approaches the size of
the light, the channels separate into distinct coloured dots.

What the references taught, all of it the opposite of instinct: the light is
**cool** with warm accents, not amber; the air is nearly **clear**, not hazy;
buildings are **lit masses**, not silhouettes; and no two windows should look
alike — rooms differ in brightness, colour temperature and whether they are on
at all, and a few switch every ten seconds or so.

## Neon City

An infinite city, hashed rather than modelled. For any integer cell of a grid,
a hash decides road or block and how tall; rays walk that grid a cell at a time
(Amanatides–Woo) testing one box per cell, so cost scales with how far a ray
travels rather than with the size of the city. It never repeats and there is
nothing to store.

The camera hovers and looks down, which is the load-bearing decision. At street
level the same city is a handful of large flat-faced boxes and reads as
low-poly geometry; from altitude a building is a few pixels and what you see is
a lit street grid with traffic moving along it, fading into haze.

## Kanban

看板, "signboard". A field of Japanese neon signs floating past in the dark:
vertical columns of characters, framed boards, single large glyphs, a few
hanging off rails, all drifting outward as the view flies slowly through them.

There is no font and no texture. A character is *composed* the way a kanji is
composed — a square field carrying one, two or three radicals, side by side,
stacked, or one inside an enclosure — and each radical is a small arrangement
of strokes drawn as capsules. That is the whole trick: the eye reads a script
from across a room by its composition and its stroke density long before it can
read a character, so hashing the structure produces something unmistakably
East-Asian that is never a real word. Straight strokes alone read as Chinese;
the curved, sparser kana branch is what makes a column look Japanese.

Depth is an **infinite zoom**. Four layers an octave apart grow and stream
outward, and when the zoom passes a whole octave each layer hands its contents
to the next one out. What makes the hand-off invisible is that every per-layer
quantity — scale, weight, dimming, and the content seed itself — is a function
of the continuous depth rather than of the layer index, so when the octave
rolls over, layer *i+1* inherits exactly the depth and the contents layer *i*
had an instant before and nothing on screen moves.

Two things that were not obvious:

- **The vanishing point is a hole.** Signs appear at it and grow away from it,
  so the smallest and dimmest content is always there. Left in the middle of
  the frame it puts a permanent gap in the middle of the composition; moved off
  to one side and wandered slowly, the same field reads as flying *past* the
  signs rather than into them.
- **Every sign fits inside its own cell, halo included.** The sign's extent is
  derived from the character size rather than the other way round, and the
  in-shader glow is deliberately tight, with the wide glow left to the bloom
  pass. That is what lets a pixel look at exactly one grid cell and one
  character — a column of four costs the same as a single glyph — with no
  neighbour sampling anywhere.

Lateral drift is bounded rather than integrated, which is the one place this
differs from the rest of the set: an unbounded drift walks the cell coordinates
into the thousands over a night, and the hashes that place the signs run out of
fractional precision and visibly quantise.

## Yama

山, "mountain". The odd one out: the only visual here with a horizon in it.
Everything else is a night city seen from above; this is a landscape at dusk,
written against the flat layered distances of *Breath of the Wild* — pale far
ranges stacked into the haze, near ones almost black, and the whole picture
carried by one lit edge and a band of burning sky. A great volcanic cone stands
in still water and the camera circles it, slowly and not quite exactly, while
cloud drifts past.

The cone is not an SDF. It is a **surface of revolution** — a height field
`h(r)` about the y axis — so a ray is bounded analytically to the span where it
is inside the base radius, and only that span is stepped. A mountain filling the
frame costs a march of five world units however far away it is, which is what
leaves the budget for the sky. Its profile is Fuji's and it is deliberately not
a cone: height falls as `(1 - r/R)^1.3`, steep at the summit and flattening into
the plain. A straight cone reads as a tent.

The sun is **fixed in the world** while the camera moves around it. That single
decision is what makes a slow orbit worth watching: over one circuit the light
goes from flat and frontal, through raking down the flanks with the gullies
throwing shadows, to straight into the sunset with the cone black against it —
three completely different pictures out of one move nobody can see happening.
The hour knob then walks the terminator up the mountain, so late in the cycle
only the summit is still in the light.

Everything else is a **horizontal band with a soft vertical profile**, which
turns out to be the one volume whose path integral is exact in closed form:
height is linear along a ray, so the optical depth through any vertical profile
is just the difference of its antiderivative at the two ends. The banner cloud
around the waist of the cone and the mist lying on the water are the same
function twice. Clipping each band at the terrain hit is what makes them
volume rather than decal — cloud in front of the mountain veils it, cloud behind
it does not.

Four things that were not obvious:

- **A short path through the air is not the sky.** Fading the mountain into the
  horizon glow paints the entire cone the colour of the sunset and throws the
  silhouette away. The glow is the radiance of a hundred kilometres of
  atmosphere seen end-on; the six units in front of a mountain are worth a
  fraction of it, and terrain fades into that instead.
- **`1 - |dot(n, rd)|` is not a rim light here.** This cone is shallow — under
  thirty degrees even at the summit — and it is seen from near the waterline, so
  the usual grazing term is near one over the entire lower flank rather than at
  its edge, and the whole mountain lights up as a pale sheet. The silhouette of
  a solid of revolution is where the outward normal points *across* the view,
  which is a statement about azimuth alone and survives any slope.
- **Anything projected onto a plane has to be filtered by its footprint.** A
  cloud deck is hit at `t = drop / rd.y`, so a one-pixel change in `rd.y` moves
  the sample by `t²·pixel/drop` — unbounded at the horizon. Point-sampled, it
  lays a bar of hard horizontal stripes along the skyline. The filter is derived
  rather than tuned, because where it bites depends on altitude, frequency and
  resolution all three.
- **The palette needs its warm end found, and normalising.** The presets here
  are Tokyo neon, and a fixed sample point lands on magenta in half of them —
  which turns the dusk mauve. Both ends of the gradient are sampled and the
  warmer kept, then normalised to a fixed luminance, so a palette decides the
  hue of the sunset and never how bright it is.

## Terebi

テレビ, "television". A wall of 90s CRT sets stacked in a dark room, each one
playing its own fragment of late-night Japanese broadcast: 風雲!たけし城, a
variety studio with a caption running under it, a quiz panel, ゲゲゲの鬼太郎,
ドラゴンボール, and the ones showing nothing but snow. The only visual in the set
shot indoors, and the only one where the light in the frame comes from objects
in a room.

The wall is **carved, not tiled**. A cell is cut in half along its longer side
up to three times, each cut decided by a hash, and every rectangle that falls
out is one set. Nothing is looked up from a neighbour, nothing overlaps, nothing
is stored, and the sizes come out unequal for free — which matters more here
than anywhere else in the show, because a grid of identical lit rectangles is
exactly what this must not look like. The path the cuts took is the set's
identity, and its size, tube colour, cut schedule, whether it has lost vertical
hold and whether it is switched on all hang off that one number.

Every so often the wall syncs. Half the time that means every set is handed the
same feed and the same clock and plays it at its own scale — a shop window with
every screen tuned to one broadcast. The other half, every set is handed the
position of its own glass on the wall instead of its own picture coordinate, and
one enormous picture is split across every screen, each piece still bulging
through its own tube. That second one costs a `mix` on two floats and nothing
else.

### What is on it

Footage, and only footage. This visual originally synthesised nine programmes
in WGSL — a studio, an anime impact frame, a vertical shooter, a platformer, a
pseudo-3d racer, a title card, a test card — each a pure function of a picture
coordinate in `-1..1`, which is what made the wall-sync trick free. They have
been removed. Every set that is switched on is playing a real broadcast.

What is left of that idea is snow, which is not a programme: it is what a
television does with no signal, and it covers the instant after a set retunes
and the case where there are no feeds at all. **Clips are therefore no longer
optional** — with no `ffmpeg`, no `./video` or `--no-video`, every set shows
snow. That is an honest picture of a room full of untuned televisions, and it is
not a crash, but it is not the visual either. `git log` has the nine channels if
they are ever wanted back.

Three things that were not obvious:

- **The bezel has no light of its own.** It is unlit plastic; everything on it
  is thrown there by the tube, sampled once at the centre of the picture. That
  is what makes a cut or a hit light the whole cabinet for an instant — and an
  early version with a constant floor under that term put a dull halo around
  every live set, which read as two dozen grey slabs hanging in the black. The
  room's own light has to be a *gradient* so the box has a top on it.
- **The tube sits high in the cabinet.** Every television ever built puts its
  speaker and its tuning controls below the screen, so the moulding there is
  deeper than it is anywhere else. That asymmetry is most of what separates a
  television from a monitor: a box with the picture centred in it reads as a
  flat panel however it is lit.
- **Scan lines belong to the tube, not to the screen.** They are drawn at the
  set's own pitch, and whether any survive is decided per set by how many pixels
  tall it is: under a couple of pixels per line they are faded out rather than
  sampled, because the alternative is a moiré that crawls across the wall. The
  curve is written to average one, so a set does not dim as it shrinks.

### The clips

The footage is the only thing in the show that depends on the world outside it,
and it earns that here and nowhere else.

```sh
tools/fetch-clips.sh                 # ~60 MB into ./video, a few minutes
cargo run -p fete-show --release
cargo run -p fete-show --release -- --video ~/my-clips
cargo run -p fete-show --release -- --no-video
```

The fetch pulls twenty-five segments of off-air VHS captures of Japanese
television from the Internet Archive: 風雲!たけし城, オレたちひょうきん族, 笑っていいとも!,
進ぬ!電波少年, quiz shows, off-air blocks, and anime — ゲゲゲの鬼太郎, おそ松くん,
めぞん一刻, ドラゴンボール, クレヨンしんちゃん, デジモン, 超くせになりそう and a Sanrio
kids' tape. 1985 to 1999, all of it domestic.

Nothing is downloaded whole: the Archive serves range requests, so `ffmpeg`
seeks to the timecode over the network and takes only the seventy-five seconds
asked for, a couple of megabytes instead of the several hundred each tape
weighs. Every clip is ffprobed after writing — a dropped connection leaves
`ffmpeg` exiting zero with a truncated file that has no index in it, and a plain
"file is non-empty" test calls that a cache hit forever. Re-running re-fetches
whatever failed or came back short, and the Archive fails a couple of these on
any given run.

Two things were left out after looking at the frames. The general commercial
compilations, because Japanese ad breaks suit this wall — short, saturated,
graphic — but the compilations on the Archive are a lottery and the segments
pulled came back with a Kodak ident in English, a New York skyline and a
European location shoot in them. And 平成教育テレビ (1992), because the segments
contain a performer in blackface, which early-90s Japanese variety used
routinely and which is not something to discover three metres tall behind a DJ.

`fete-video` runs sixteen `ffmpeg` subprocesses, one per slot, each looping its
own share of the clips and piping raw RGBA into one layer of a `2d_array`
texture. Decoding by subprocess rather than by linking libavcodec buys nothing
back except simplicity, which is the whole point: there is no seeking, no audio,
no container introspection and no synchronisation between slots, and `-re` makes
ffmpeg pace its own output so a blocking read *is* the playback clock. All
sixteen together measured at 3.6% of one core, because each spends nearly all of
its time asleep.

**Failure is the normal case.** No clips, no `ffmpeg`, no directory: one log
line, no `VideoWall`, and the wall plays the nine synthesised channels — which
is the visual as authored, not a degraded version of it. The shader gates on a
slot count rather than on whether the texture is bound, so the binding can stay
unconditional and fall back to Bevy's placeholder.

Six things that took a capture each to find:

- **`textureSampleLevel`, never `textureSample`.** The channel dispatch runs
  inside `if cab < 0.0`, which is non-uniform control flow, where
  implicit-derivative sampling is illegal and naga rejects the shader outright.
- **Broadcast video is mid-grey everywhere** — a studio floor, a lit wall, a
  face at forty per cent — and dropped in raw among channels that are mostly
  black it reads as a hole cut in the wall. It needs a black point and a curve
  that carries the top past 1.0 into the same bloom everything else lives in.
- **Not a smoothstep for that curve.** It is the obvious choice and it plateaus:
  everything above its upper edge arrives at one value, so a lit shirt or a
  studio lamp comes out as a flat white shape with no modelling in it. A power
  curve stays monotone all the way up and highlights still separate as they clip.
- **Video is the exception to the per-set mirror.** Flipping a synthesised
  channel is free variation; flipping a broadcast reverses the writing on it,
  and a wall of backwards Japanese is invisible until it is the only thing
  anyone can see. Video sets are pulled apart on zoom and framing instead.
- **The tape's head-switching noise has to scale with the picture.** At a fixed
  level it is the brightest thing on a set showing a night scene, so the wall
  ends up with a row of bright stripes drawing the eye to exactly the sets that
  should be reading as dark.
- **Feeds are dealt by position, not by hash.** Two televisions three cabinets
  apart showing the same clip is the clearest tell that a wall is generated, and
  it is a birthday problem — draw thirty sets from a dozen feeds at random and a
  collision is near certain. `rank` differs between every set in a cell and
  between neighbouring cells, which makes the case the eye actually checks
  provably different.

The footage is broadcast television, uploaded to the Archive as preservation. It
is not public domain and the Archive is not a licence. For a ticketed room,
point `--video` at your own folder — anything `ffmpeg` reads works.

### Switching off

Whether a set is on is not a property of the set. An early version rolled it
once per set and the wall became a still life: the dark ones were always the
same dark ones, and after a minute the eye had the layout memorised and stopped
looking. Each set now draws its own standby period, in beats and deliberately
unrelated to the programme clock — how often somebody switches a television off
has nothing to do with how often the ones that are on change channel, and
hanging both on the same knob was what made twenty sets read as one machine.

Sets go dark one at a time over minutes, and they do it properly: a CRT losing
its supply collapses vertically first, so the picture folds into a bright line
across the middle of the tube and that line shrinks to a dot which takes a
second or two to die. Coming back is slower and the other way round — a cold
tube takes a couple of seconds to reach brightness. It is the most recognisable
thing a television does and it is the whole reason a set going dark reads as
somebody switching it off rather than as a rectangle being masked out.

The bug worth recording: the collapse was gated on the state having *changed*
but not on how long ago. That condition stays true for the entire standby
period, so every switched-off set on the wall held the collapsed line at five
times brightness for a minute at a time — the brightest thing in the room, on
exactly the sets that were supposed to be off.

Most of the wall is dark at any moment, and that is the point. A projector
cannot render black darker than the room already is, so contrast on a wall of
lit rectangles is bought only by leaving most of them unlit — and a screen
bright enough to light faces on the floor is too bright whatever it looks like
on a laptop in the dark.

## Kura

The one this all came from. VJ-FÊTE was a C++/OpenGL piece — three interacting
flocks, soft-cored fireflies with tapering trails, a lattice of links and filled
triangles forming between the ones that agree on a heading, and a flow field
smeared across the background. This is that piece, reconstructed constant for
constant, running inside the framework it turned into.

It is the only visual here that is not a shader. It is a **CPU simulation
feeding geometry**: 1340 boids stepped on the CPU each frame and roughly a
hundred thousand vertices rebuilt from them, drawn as four meshes in front of a
fullscreen material that paints the black the original cleared to. Two
materials cover all four meshes — one falloff curve for anything that was a
point sprite, one pass-through for anything that was a triangle or a thick
line — which is the whole shader budget for the visual.

Three decisions carried the reconstruction:

- **Reference pixels, not screen pixels.** The original rendered into a fixed
  1920×1080 target and stretched it to the display, which is also how its
  square world ended up stretched to a non-square screen. The geometry is built
  in that space and the mesh transform carries the stretch, so any window at any
  aspect gets exactly the picture the original would have shown on it — the
  distortion included, because it is part of the look.
- **The flow field's accumulation buffer is not needed.** The original faded a
  full-screen buffer by five percent a frame so each flow line left a smear.
  But the lines are anchored to fixed cells, so all that buffer ever held at a
  pixel was that cell's own recent history — keeping the last ten states of each
  line and drawing them at the same decay weights is the same image with no
  feedback texture at all. The one thing that had to be reproduced by hand is
  8-bit saturation: without clamping each cell's weights to sum to one, a
  stationary flow line integrates to eight times white and blooms the frame
  away.
- **Its accidents are load-bearing.** Every flock is integrated *twice* per
  frame, because the original steps six ordered pairs; and neighbour cells are
  never looked up across the world seam even though distances are measured
  across it. Neither is what anyone would write on purpose and both change how
  the motion reads, so both are kept, with a note at the top of `flock.rs`.

What is phrased against the clock is only what should be. The flocking runs in
real seconds, because it is physics and should look the same at any tempo.
Everything periodic — the Kuramoto oscillators driving the discs' breathing, the
brightness flicker, the flow field's wiggle — runs on beats scaled to 128bpm, so
it is identical to the original at that tempo and belongs to the track at any
other.

Colour is the one thing deliberately not preserved. The original generated its
own palettes from hue relationships; here only the three *role* hues are
re-sourced from the show's cosine palette, sampled at three points around the
gradient and pushed apart if they land too close. The saturation and value of
each role are the original's exactly, because three populations at three
brightnesses is the depth in the image — the heavy discs nearly fully saturated,
the light shoal dimmer, the small dust pale.

## Signal flow

Nothing reads the keyboard directly. Inputs write to eight normalised macro
knobs, and visuals read only those. In between sits a modulation matrix that can
drive any knob from an LFO, a beat-synced oscillator, the beat envelope or an
audio band:

```rust
modulation.patch(
    Modulator::new(1, ModSource::Synced { wave: Wave::Sine, beats: 32.0 })
        .with_depth(0.5)
        .with_bias(0.35),
);
```

That indirection is why a visual works identically under a hand on a keyboard,
an LFO, or a MIDI fader — and why adding MIDI later needs no change to any
visual.

## Controls

| | |
|---|---|
| `Tab` / `Shift+Tab` | next / previous visual |
| `1`–`9` | select visual directly |
| `Q/A W/S E/D R/F T/G Y/H U/J I/K` | macro knobs 0–7, raise / lower |
| `Space` | tap tempo (also realigns the downbeat) |
| `Enter` | resync phase without changing tempo |
| `[` `]` | tempo ∓0.5 bpm |
| `P` | next palette (morphs over two seconds) |
| `B` | blackout toggle |
| `Z` / `X` | master fade down / up |
| `M` | freeze modulation |
| `C` | autopilot on/off |
| `V` | visual rotation on/off (palette and drift keep running) |
| `/` or `F1` | HUD |
| `\` or `F11` | fullscreen on the current monitor (`Esc` always exits) |
| `.` or `F12` | save a still to `captures/` |

The non-function-key bindings exist because macOS treats `F1`/`F11`/`F12` as
media keys unless the user has changed a system setting, which makes an
F-key-only binding unreachable on the average laptop.

### Manual control versus the autopilot

They coexist. Moving a knob claims it: the autopilot stops drifting *that* knob
and leaves it alone for `Autopilot::release_seconds` (90s by default) before
reclaiming it, picking up smoothly from wherever it was left. The HUD tags each
knob `held`, `auto` or `~mod` so it is always clear who is driving what.

`C` switches the autopilot off entirely and hands everything over. `V` is the
narrower version: it stops the visual changing and nothing else.

## Working on shaders

```sh
cargo run -p fete-show --features hot-shaders
```

Shaders are embedded in the binary via `embedded_asset!`, so a visual crate
ships its WGSL with no assets folder. With `hot-shaders` they also reload from
their source files while the app is running — edit, save, watch it change.

## Scripted stills

```sh
FETE_CAPTURE=preview.png@20 cargo run -p fete-show --release
```

Renders for twenty seconds, writes one frame, exits. Captures the app's own
render target.

## Notes on looking good on a projector

These are the decisions that mattered most, recorded so they do not get
undone by accident:

- **Most of the frame must be black.** A projector cannot show black darker
  than the room's ambient light, so contrast is bought only by leaving pixels
  unlit. An evenly-lit frame reads as grey haze from the back of the room.
- **Write HDR, let bloom do the glow.** Shaders push crests well past 1.0 and
  leave troughs under it. Uniform brightness blooms the whole frame into mush.
- **Peak around 2, not 5.** Anything much brighter tonemaps to white and takes
  its hue with it.
- **Separate what drives brightness from what drives hue.** If colour is a
  function of intensity alone, every bright thing is the same colour — and the
  bright things are all the eye sees.
- **Nothing global may reach zero.** A mask that can darken the whole frame
  will eventually do so, mid-set.
- **Keep bloom scatter low.** A wide, low-frequency bloom takes the colour of
  the brightest thing on screen and washes it across the whole frame. It shows
  up as coloured haze filling the black, and it is what turned Neon's signage
  into lit windows on a grey wall.
- **Vary the silhouette, not just the colour.** Identical shapes on a grid read
  as architecture — a lattice of lit windows — however you colour them.
- **Scale hides geometry.** Boxes look like boxes up close. The fix for
  "low-poly" was not more detail per building, it was moving the camera until a
  building was a few pixels wide.
- **Make a simulation unable to settle, rather than stirring it.** Slime used
  to converge within a minute to a configuration that satisfied its own rules
  and then stop being interesting, and the fix was three slow oscillators
  sweeping its parameters — which worked, but the motion belonged to the
  oscillators. The better fix is structural, and Kura had it all along: an
  interaction that is *not symmetric* has no energy function to minimise, so
  there is no equilibrium to reach. Slime now runs two species on two trail
  channels where one is drawn to the other's trail and the other is pushed off
  its. Measured over half a second of simulation, the old version's rate of
  change fell from 0.17 to 0.14 between five and forty seconds while the frame
  emptied out; the new one holds 0.23 to 0.26 with the frame still full.
- **A chase dissipates; only a cycle winds up.** Two species where one hunts
  the other stops the simulation settling, but the front where they meet
  wanders off and nothing brings it back — the frame stays a uniform carpet at
  one scale. Three species where each hunts the next cannot resolve a point
  where all three meet, so that point is pinned and the fronts wind around it.
  Spirals tens of times the width of a filament, nucleating and annihilating in
  pairs. Two conditions, both of which cost a rewrite to find: **fleeing must
  outweigh chasing** (otherwise a texel holding all three is more attractive to
  each of them than its own, and they pile onto the same filaments), and **the
  species must read each other from much further away than they read
  themselves** (the pattern's scale is set by the range over which they can
  feel each other, so at one range you get colour fringing on shared tubes and
  nothing more).
- **Check the arithmetic of a random walk before blaming the model.** Slime's
  heritable trait converged to the bottom of its range within a couple of
  minutes. Four increasingly elaborate theories about selection — sharpen it,
  reward rarity, make fitness depend on the cycle, change which traits are
  heritable — all landed on the *same* mean of 0.02 and spread of 0.014, which
  is the tell that the thing being adjusted is not the cause. It was the
  mutation scaled by `dt` instead of `sqrt(dt)`: a random walk needs a Wiener
  increment, which is what the Kuramoto oscillators next door already do and
  say why. Eight times too little noise, and it lost to the averaging. The
  arithmetic that found it was noticing the spread it settled at was exactly
  the spread the mutation rate could support. All four "fixes" then measured
  *better* removed than kept.
- **A hash that is not centred will integrate.** `hash11` has a mean of 0.4956
  over the arguments these shaders feed it. That is invisible in a one-shot
  decision like breaking a tie between two sensors, but anything accumulating
  it drifts — a heading gains a systematic curl, a random-walking trait loses
  half its range over a few minutes. The difference of two independent samples
  has mean exactly zero whatever the marginal's bias is.
- **A beat kick aimed at a point is a source at that point.** Slime's downbeat
  impulse pushed every agent away from the centre of the frame. The field
  wraps, so what it evicted re-entered at the borders and never came back, and
  the network slowly reorganised into arcs around a hole. Alternating the sign
  bar to bar — out, then in — makes the net displacement zero over a pair and
  the profile flat. Two details go with it: interpolate the heading along the
  *shortest arc* (`agent.angle` accumulates without bound while `atan2` returns
  `-PI..PI`, so a naive `mix` between them is a scramble, not a shove), and if
  you exempt a disc at the centre from the kick, keep it tiny — an exempt zone
  drains itself, because agents just outside are pushed away on the outward bar
  and those inside are never pushed back in.
- **Two networks tile a frame faster than one.** Splitting Slime's population
  between two species left 36% of the frame black where the single-species
  version had left 75%, at the same mesh pitch — enough lit area to cost real
  contrast on a projector. Coverage is set by the scale of the mesh, not by how
  long the trail is remembered: opening the sensor distance up put it back,
  while shortening the decay made it worse (agents stop concentrating into
  filaments and the field becomes a wash).
- **Localise emissive detail.** A rooftop beacon applied to the whole roof face
  lit every tenth building as a flat red slab. Point lights need to be points.
- **Filter sub-pixel detail, do not sample it.** Widen a feature to the pixel
  footprint and scale its peak by the area ratio. Skipping this makes dense
  fields either invisible or a boiling mess, depending on which way you err.
- **Vary everything, per instance.** Identical lit windows at identical
  brightness is the single clearest tell that something is generated. Vary
  brightness, colour temperature, whether it is on, and change a few over time.
- **Check references early.** Three photographs corrected more in one pass than
  several rounds of guessing: cool not amber, clear not hazy, lit not
  silhouetted.
- **Sub-pixel bright lines alias into coloured dashes.** A rim light tight
  enough to sit inside a pixel of a sloped silhouette becomes a staircase once
  the grade's chromatic aberration finds it. Wide edges are both what the
  reference looks like and the only version that survives being drawn.
- **A silhouette wants a floor, not a physical shadow.** Skylight scaled
  properly off dark rock at this exposure is indistinguishable from black, and
  a black shape loses its form. A deep blue floor is a deliberate lie and it is
  the difference between a silhouette and a hole in the picture.
- **Never lift the black.** `Grade::lift` defaults to zero for a reason: on a
  mostly-black frame a lift larger than the picture reads as a grey veil over
  the whole image. The projector and the room already raise the black floor;
  adding more only spends contrast.
- **The show should not light the room.** If faces are lit, it is too bright,
  whatever it looks like on a laptop in the dark. `Grade::exposure` is the one
  control for this and it lives well below 1.0.
