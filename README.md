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
visuals/fete-visual-sprawl Sprawl, an analytic megacity seen from a tower
visuals/fete-visual-neon  Neon City, a raymarched city, low-poly and owned
visuals/fete-visual-slime Slime, a physarum agent simulation (compute)
visuals/fete-visual-kanban Kanban, Japanese neon signage floating past
visuals/fete-visual-yama  Yama, a volcanic cone at dusk, circled slowly
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
cargo run -p fete-visual-kura --release        # CPU sim, release is not optional
cargo run -p fete-show --release               # the whole set, on autopilot
cargo run -p fete-show --release -- --fullscreen --no-hud
cargo run -p fete-show --release -- --start neon --manual
```

## Running unattended

The autopilot is on by default. It changes visual every 192 beats through a
bleed transition, morphs the palette on a deliberately coprime 260-beat period
so visual/colour pairings rarely repeat, and continuously drifts every macro
knob nothing else is driving. A single visual held for ten minutes is never
quite the same twice, and a four-hour night does not loop.

Press `C` to switch it off and take over.

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

`C` switches the autopilot off entirely and hands everything over.

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
- **Keep a simulation out of equilibrium.** Slime converges within a minute to
  a configuration that satisfies its own rules and then stops being
  interesting. Three slow oscillators on coprime periods keep moving the target
  it is converging towards, so it is permanently mid-reorganisation.
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
