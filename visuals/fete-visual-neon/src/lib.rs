//! **Neon** — a city seen from above, drifting past.
//!
//! The city is a function rather than a model: a hash over an infinite integer
//! grid decides which cells are road and how tall each block is, and rays walk
//! that grid a cell at a time. It never repeats, it has no extent to run out
//! of, and it costs nothing to store.
//!
//! The camera hovers. At street level the same city reads as a handful of large
//! flat-faced boxes; from altitude a building is a few pixels and what you see
//! is a lit street grid with traffic on it, fading into haze.
//!
//! The traffic on that grid is the one thing here that is not a pure function
//! of position. Cars themselves are — a hash per slot decides what is in it —
//! but the *signals* are a ring of coupled phase oscillators integrated on the
//! CPU, the same Kuramoto system that makes the discs in `kura` breathe. A
//! shader has no memory, so signals written there could only ever run to a
//! fixed timetable; coupling them lets streets gather into a green wave, hold
//! it for a few phrases, and come apart again. Beat energy opens the coupling
//! rather than driving anything directly, so a build gathers the city and a
//! quiet passage lets it spread — a response measured in phrases, with nothing
//! on screen moving on the beat itself.
//!
//! Built for a room where the screen is atmosphere rather than the act, so the
//! camera moves slowly, the frame is mostly dark, and it reacts at half-time.
//!
//! # Knobs
//!
//! | key | knob | does |
//! |-----|------|------|
//! | Q/A | 0 | brightness |
//! | W/S | 1 | how many windows are lit |
//! | E/D | 2 | drift speed |
//! | R/F | 3 | altitude |
//! | T/G | 4 | how far the camera looks down |
//! | Y/H | 5 | haze — how far the city is visible |
//! | U/J | 6 | colour spread |
//! | I/K | 7 | beat depth (half-time) |

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use fete_core::prelude::*;

/// Traffic signals modelled per direction of travel.
///
/// Streets more than this many apart share an oscillator. Sixteen rows is over
/// a hundred blocks — four times the distance at which the haze has closed in
/// completely — so the repeat is never on screen twice.
const SIGNALS_PER_AXIS: usize = 16;
const SIGNAL_COUNT: usize = SIGNALS_PER_AXIS * 2;

/// Must match `NeonParams` in `neon.wgsl`.
#[derive(ShaderType, Debug, Clone, Copy)]
pub struct NeonParams {
    /// Distance travelled down the avenue.
    pub drift: f32,
    /// Smoothed half-time beat energy.
    pub energy: f32,
    /// Lateral position across the avenue.
    pub sway: f32,
    /// Camera height above the road.
    pub height: f32,
    /// Look direction, as small offsets from straight ahead.
    pub yaw: f32,
    pub pitch: f32,
    pub _pad0: f32,
    pub _pad1: f32,
    /// One traffic signal per entry: `(travel, brake, queue, unused)`.
    ///
    /// `travel` is how far that street's traffic has got, in blocks at cruising
    /// speed; `brake` is how hard it is braking; `queue` is how far it has
    /// bunched up. All three are integrated or decided here rather than in the
    /// shader: the shader has no memory, so a signal cycle written there could
    /// only ever be a periodic function of time. Keeping the state means the
    /// signals can be *coupled*, which is the whole point of what follows.
    pub signals: [Vec4; SIGNAL_COUNT],
}

impl Default for NeonParams {
    fn default() -> Self {
        Self {
            drift: 0.0,
            energy: 0.0,
            sway: 0.0,
            height: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            _pad0: 0.0,
            _pad1: 0.0,
            signals: [Vec4::ZERO; SIGNAL_COUNT],
        }
    }
}

/// The city's traffic signals, as weakly coupled phase oscillators.
///
/// A grid of independently-timed signals looks wrong from the air, and a grid
/// of identically-timed ones looks worse — the first is noise, the second is a
/// metronome. Real signals are coordinated but not perfectly, and what that
/// produces is the green wave: a band of streets releasing in sequence, holding
/// together for a while, and coming apart again.
///
/// That is a Kuramoto system, so it is built as one — the same as the discs in
/// `kura`, and with the same restraint. Each street runs at its own natural
/// period and couples only to the streets either side of it, plus weakly to the
/// perpendicular direction, in antiphase: when one direction has green the
/// other has red. Coupling sits near the locking threshold and is opened up by
/// the beat energy, so the city drifts between coherent waves and independent
/// streets over a few phrases without ever fully locking. Full locking would be
/// every signal in the city turning at once, which is a strobe.
///
/// Time is beats throughout, so a tempo change re-times the traffic with it.
#[derive(Debug, Clone)]
struct Signals {
    /// Signal phase. Zero is the stall, halfway round is full flow.
    theta: [f32; SIGNAL_COUNT],
    /// Natural frequency, radians per beat.
    omega: [f32; SIGNAL_COUNT],
    /// How hard this street's signal bites: 1.0 stops it dead, 0.25 only slows
    /// it. Streets that never fully stop keep the frame from being mostly
    /// stationary, and keep red a minority of it.
    stall: [f32; SIGNAL_COUNT],
    /// Slow wander applied to the natural frequency, in cycles per beat. Fixed
    /// frequencies settle into a fixed point and stay there; these are
    /// mutually incommensurate, so the system is never finished moving.
    wobble: [f32; SIGNAL_COUNT],
    travel: [f32; SIGNAL_COUNT],
    speed: [f32; SIGNAL_COUNT],
    /// How hard this street is braking, 0..1. Sent to the shader in place of
    /// the speed, so the shader is left rendering rather than deciding.
    brake: [f32; SIGNAL_COUNT],
    /// How far the traffic has closed up, 0..1. Standing traffic occupies a
    /// fraction of the road that the same traffic at speed does — a queue of
    /// ten cars is fifty metres, and those ten cars doing fifty are spread over
    /// two hundred and fifty.
    queue: [f32; SIGNAL_COUNT],
    /// Beat position at the previous update, so the step is taken from the
    /// clock rather than from wall time — and is zero while it is paused.
    last_beats: f64,
}

/// Nearest-neighbour coupling, radians per beat, before beat energy opens it up.
///
/// Set against the spread of the natural frequencies, which is about `0.09`
/// either side. At that scale streets gather into waves without ever locking
/// solid; at twice it the whole direction turns as one, which is a metronome
/// driving the city and reads on screen as a strobe.
const SIGNAL_COUPLING: f32 = 0.085;
/// Pull towards the perpendicular direction's mean phase, in antiphase, as a
/// fraction of the neighbour coupling. Weaker than local coupling — a junction
/// is a weaker constraint on a street than the street's own length is.
const SIGNAL_CROSS: f32 = 0.55;

impl Default for Signals {
    fn default() -> Self {
        let mut out = Self {
            theta: [0.0; SIGNAL_COUNT],
            omega: [0.0; SIGNAL_COUNT],
            stall: [0.0; SIGNAL_COUNT],
            wobble: [0.0; SIGNAL_COUNT],
            travel: [0.0; SIGNAL_COUNT],
            speed: [1.0; SIGNAL_COUNT],
            brake: [0.0; SIGNAL_COUNT],
            queue: [0.0; SIGNAL_COUNT],
            last_beats: 0.0,
        };
        for i in 0..SIGNAL_COUNT {
            let n = i as u32;
            out.theta[i] = hash01(n) * std::f32::consts::TAU;
            // Twelve to eighteen beats a cycle — five to eight seconds at a
            // house tempo. The spread has to be narrow enough that a weak
            // coupling can gather streets into a wave, and wide enough that
            // they come apart again afterwards.
            let cycle = 12.0 + hash01(n + 101) * 6.0;
            out.omega[i] = std::f32::consts::TAU / cycle;
            // Biased hard towards a full stop: most streets have a red light on
            // them and most red lights stop the traffic. The tail that only
            // slows is the minor road with priority and the street the wave is
            // currently favouring.
            let ease = hash01(n + 211);
            out.stall[i] = 1.0 - ease * ease * ease * 0.5;
            out.wobble[i] = 0.004 + hash01(n + 307) * 0.009;
        }
        out
    }
}

impl Signals {
    /// Index of the signal driving street `n` of `axis`, where axis 0 runs
    /// along x.
    const fn index(axis: usize, n: usize) -> usize {
        axis * SIGNALS_PER_AXIS + n % SIGNALS_PER_AXIS
    }

    fn step(&mut self, beats: f64, energy: f32) {
        // Straight from the clock: paused is a zero step, and a tempo change is
        // just a different number of beats this frame. Clamped so that a stall
        // on the machine cannot fling the traffic down the street.
        let dt = ((beats - self.last_beats).max(0.0) as f32).min(0.25);
        self.last_beats = beats;
        if dt <= 0.0 {
            return;
        }

        // Mean phase of each direction, as a vector sum. Its length is the
        // order parameter — how much that direction agrees with itself — and
        // weighting the cross term by it means a direction that is in disarray
        // does not get to impose its phase on the other one.
        let mut mean = [(0.0f32, 0.0f32); 2];
        for (axis, into) in mean.iter_mut().enumerate() {
            let (mut x, mut y) = (0.0, 0.0);
            for n in 0..SIGNALS_PER_AXIS {
                let th = self.theta[Self::index(axis, n)];
                x += th.cos();
                y += th.sin();
            }
            *into = (x / SIGNALS_PER_AXIS as f32, y / SIGNALS_PER_AXIS as f32);
        }

        // Beat energy opens the coupling rather than driving anything directly.
        // A build gathers the city into a wave and the drop lets it spread out
        // again, which is a response measured in phrases — nothing on screen
        // moves on the beat itself.
        let k = SIGNAL_COUPLING * (0.25 + 0.65 * energy.clamp(0.0, 1.0));

        // Advanced together rather than in place: updating in order would let
        // the first street of each ring lead every other one.
        let mut next = self.theta;
        for axis in 0..2 {
            let (mx, my) = mean[1 - axis];
            let coherence = (mx * mx + my * my).sqrt();
            let psi = my.atan2(mx);
            for n in 0..SIGNALS_PER_AXIS {
                let i = Self::index(axis, n);
                let th = self.theta[i];
                let left = self.theta[Self::index(axis, n + SIGNALS_PER_AXIS - 1)];
                let right = self.theta[Self::index(axis, n + 1)];

                let drift = std::f32::consts::TAU * self.wobble[i] * beats as f32;
                let omega = self.omega[i] * (1.0 + 0.06 * drift.sin());

                let mut d = omega;
                d += k * 0.5 * ((left - th).sin() + (right - th).sin());
                d += k * SIGNAL_CROSS * coherence * (psi + std::f32::consts::PI - th).sin();

                next[i] = (th + d * dt).rem_euclid(std::f32::consts::TAU);
            }
        }
        self.theta = next;

        for i in 0..SIGNAL_COUNT {
            let th = self.theta[i];

            // Speed over one signal cycle, as a fraction of this street's
            // cruising speed. A trapezoid, not a sinusoid: traffic holds a
            // steady speed for most of a cycle, brakes over a few seconds,
            // waits, and pulls away again.
            //
            // A sinusoid was the first attempt, because it integrates in closed
            // form — but a sinusoid normalised to a fixed average spends the
            // whole cycle either crawling or overshooting and never holds a
            // speed at all, and the cars read as alternately parked and
            // airborne. Nothing here needs a closed form any more.
            let open = smoothstep(0.0, 0.42, 0.5 - 0.5 * th.cos());
            let v = 1.0 - self.stall[i] * (1.0 - open);

            // Brake lights come off deceleration, not off being slow: a car
            // pulling away from a green is doing the same speed as one coming
            // up to a red, and only one of them is showing red.
            let accel = (v - self.speed[i]) / dt;
            let stopped = 1.0 - smoothstep(0.02, 0.30, v);
            self.brake[i] = smoothstep(0.0, -0.7, accel).max(stopped * 0.9);
            self.queue[i] = 1.0 - smoothstep(0.06, 0.7, v);

            self.travel[i] += v * dt;
            self.speed[i] = v;
        }
    }

    fn pack(&self, out: &mut [Vec4; SIGNAL_COUNT]) {
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = Vec4::new(self.travel[i], self.brake[i], self.queue[i], 0.0);
        }
    }
}

/// The shader builtin, which `f32` does not have. Edges may run either way.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// PCG-style integer hash, for seeding the oscillators reproducibly.
fn hash01(i: u32) -> f32 {
    let state = i.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    ((word >> 22) ^ word) as f32 / u32::MAX as f32
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
#[bind_group_data(Tier)]
pub struct Neon {
    #[uniform(0)]
    globals: FeteGlobals,
    #[uniform(1)]
    params: NeonParams,
    /// Not a binding — the pipeline specialisation key. Changing it rebuilds
    /// the shader with different loop bounds.
    tier: Tier,
    /// Signal state. Lives here rather than in a resource because it is only
    /// ever touched by `animate`, and it is packed into `params` on the way
    /// out.
    signals: Signals,
}

impl From<&Neon> for Tier {
    fn from(neon: &Neon) -> Self {
        neon.tier
    }
}

impl Material2d for Neon {
    fn fragment_shader() -> ShaderRef {
        "embedded://fete_visual_neon/shaders/neon.wgsl".into()
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let tier = key.bind_group_data;
        let Some(fragment) = descriptor.fragment.as_mut() else {
            return Ok(());
        };
        fragment.shader_defs.extend(tier.shader_defs());
        // Steps and draw distance move together, and the shader pulls the
        // fog in by the same ratio. Cutting the steps alone would stop the
        // buildings early while leaving the roads and the haze where they
        // were, which reads as a bald patch rather than as a smaller city.
        fragment.shader_defs.push(ShaderDefVal::Int(
            "MARCH_STEPS".into(),
            tier.pick(110, 80, 56),
        ));
        fragment.shader_defs.push(ShaderDefVal::Int(
            "MAX_DIST".into(),
            tier.pick(78, 62, 48),
        ));
        if tier != Tier::High {
            fragment.shader_defs.push("CHEAP_ROAD".into());
        }
        Ok(())
    }
}

impl Visual for Neon {
    const ID: VisualId = "neon";
    const NAME: &'static str = "Neon City";
    const TAGS: &'static [&'static str] = &["tokyo", "raymarched", "slow", "ambient"];

    fn globals_mut(&mut self) -> &mut FeteGlobals {
        &mut self.globals
    }

    fn set_quality(&mut self, quality: Quality) {
        self.tier = quality.tier;
    }

    fn animate(&mut self, frame: &Frame) {
        let dt = frame.clock.delta;
        let beats = frame.clock.beats as f32;

        // One cell is one city block, so this is blocks per second. Slow — from
        // altitude the ground moves visually slower than it does at street
        // level, so this can be a little faster than a flythrough without ever
        // reading as speed.
        let speed = frame.knob_range(2, 0.3, 3.0);
        self.params.drift += speed * dt;

        // The camera is never quite still, on periods long enough that no
        // single one is perceptible. Without this the flight is a rail and
        // reads as a screensaver; with it, it reads as handheld.
        let wander =
            |period: f32, phase: f32| ((beats / period + phase) * std::f32::consts::TAU).sin();

        // Nothing is above the buildings to collide with, so the camera is free
        // to wander much further than it could down a street.
        self.params.sway = wander(43.0, 0.0) * 6.0;

        // Altitude. The floor is set well above the tallest tower (8.5) so the
        // camera never clips through one, which from above would fill the frame
        // with a single roof.
        self.params.height = frame.knob_range(3, 12.0, 30.0) + wander(67.0, 0.4) * 1.6;

        // How far down we look. Shallow shows more horizon and haze and reads
        // as a wide establishing shot; steep shows the street grid as a plan.
        let look_down = frame.knob_range(4, 0.22, 0.85);
        self.params.pitch = -look_down + wander(97.0, 0.7) * 0.03;

        // Yaw trails the sway, the way you look towards where you are drifting.
        self.params.yaw = wander(43.0, 0.15) * 0.09;

        // Half-time and heavily smoothed: a swell, not a hit.
        let target =
            (frame.clock.pulse_div(2.0, 2.0) * 0.5 + frame.audio.bass * 0.5).clamp(0.0, 1.0);
        let alpha = 1.0 - (-dt / 0.35).exp();
        self.params.energy += (target - self.params.energy) * alpha;

        // Traffic. Stepped after the energy, so the coupling reads this frame's
        // swell rather than the last one's.
        self.signals.step(frame.clock.beats, self.params.energy);
        self.signals.pack(&mut self.params.signals);
    }
}

pub struct NeonPlugin;

impl Plugin for NeonPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/neon.wgsl");
        app.add_visual::<Neon>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the signals for `beats` at 60fps and 128bpm, calling `sample` after
    /// each step.
    fn run(beats: f32, energy: f32, mut sample: impl FnMut(&Signals)) -> Signals {
        let mut signals = Signals::default();
        let step = (128.0 / 60.0) / 60.0;
        let mut at = 0.0f64;
        while at < beats as f64 {
            at += step;
            signals.step(at, energy);
            sample(&signals);
        }
        signals
    }

    /// Order parameter of one direction: 1.0 is every signal in step.
    fn coherence(signals: &Signals, axis: usize) -> f32 {
        let (mut x, mut y) = (0.0, 0.0);
        for n in 0..SIGNALS_PER_AXIS {
            let th = signals.theta[Signals::index(axis, n)];
            x += th.cos();
            y += th.sin();
        }
        (x * x + y * y).sqrt() / SIGNALS_PER_AXIS as f32
    }

    /// How regular the phase step between neighbouring streets is. Near 1.0 the
    /// signals are laid out as a smooth gradient — a green wave rolling across
    /// the city, or, if the step is zero, every street turning at once. Near
    /// 0.0 there is no relationship between one street and the next.
    fn twist(signals: &Signals, axis: usize) -> f32 {
        let (mut x, mut y) = (0.0, 0.0);
        for n in 0..SIGNALS_PER_AXIS {
            let d =
                signals.theta[Signals::index(axis, n + 1)] - signals.theta[Signals::index(axis, n)];
            x += d.cos();
            y += d.sin();
        }
        ((x * x + y * y).sqrt()) / SIGNALS_PER_AXIS as f32
    }

    #[test]
    fn traffic_never_reverses() {
        let mut last = [0.0f32; SIGNAL_COUNT];
        run(600.0, 0.5, |signals| {
            for (i, (&travel, was)) in signals.travel.iter().zip(last.iter_mut()).enumerate() {
                assert!(
                    travel >= *was,
                    "signal {i} moved backwards: {was} -> {travel}"
                );
                *was = travel;
            }
        });
    }

    /// The complaint that produced the current shape: a street should spend
    /// most of a cycle at a steady cruise, not sweep continuously between a
    /// crawl and a sprint.
    #[test]
    fn streets_hold_a_cruising_speed() {
        // A signal that stops its street dead, so this measures the worst case.
        let i = (0..SIGNAL_COUNT)
            .max_by(|a, b| Signals::default().stall[*a].total_cmp(&Signals::default().stall[*b]))
            .unwrap();

        let (mut cruising, mut stopped, mut total) = (0, 0, 0);
        run(600.0, 0.5, |signals| {
            let v = signals.speed[i];
            total += 1;
            if v > 0.95 {
                cruising += 1;
            }
            if v < 0.05 {
                stopped += 1;
            }
        });

        let cruising = cruising as f32 / total as f32;
        let stopped = stopped as f32 / total as f32;
        assert!(
            cruising > 0.4,
            "only {cruising:.2} of the time at a steady speed",
        );
        assert!(
            stopped > 0.05,
            "only {stopped:.2} of the time held at a red"
        );
    }

    /// Weak coupling, so the streets agree some of the time and not all of it.
    /// Both failures matter, and the second is the dangerous one: never
    /// agreeing is a grid of unrelated signals, always agreeing is one
    /// metronome driving the whole city, which on a big screen is a strobe.
    #[test]
    fn signals_drift_in_and_out_of_step() {
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        // Long enough to be past the transient and to have seen the system
        // gather and disperse several times over.
        run(4000.0, 0.5, |signals| {
            let r = twist(signals, 0);
            lowest = lowest.min(r);
            highest = highest.max(r);
        });

        assert!(highest > 0.6, "never gathers: peak twist {highest:.2}");
        assert!(lowest < 0.3, "never disperses: lowest twist {lowest:.2}");
    }

    /// Beat energy opens the coupling rather than driving anything directly.
    ///
    /// What that should buy is a *green wave* — neighbouring streets releasing
    /// in sequence — and not the whole direction turning at once. The two look
    /// nothing alike and the order parameter cannot tell them apart, so this
    /// checks the phase step between neighbours is what tightens while the
    /// city-wide agreement stays loose.
    #[test]
    fn energy_rolls_a_wave_rather_than_a_flash() {
        let mean = |energy: f32, of: fn(&Signals, usize) -> f32| {
            let (mut sum, mut n) = (0.0, 0);
            run(4000.0, energy, |signals| {
                sum += of(signals, 0);
                n += 1;
            });
            sum / n as f32
        };

        let quiet = mean(0.0, twist);
        let loud = mean(1.0, twist);
        assert!(
            loud > quiet * 1.5,
            "loud {loud:.2} did not gather the streets much past quiet {quiet:.2}",
        );
        assert!(
            mean(1.0, coherence) < 0.6,
            "the whole direction turns as one at full energy",
        );
    }

    /// A street that is stopped is a queue, and a queue is shorter than the
    /// same traffic strung out at speed.
    #[test]
    fn stopped_traffic_queues() {
        run(600.0, 0.5, |signals| {
            for i in 0..SIGNAL_COUNT {
                if signals.speed[i] < 0.05 {
                    assert!(signals.queue[i] > 0.9, "stopped but not queued");
                }
                if signals.speed[i] > 0.8 {
                    assert!(signals.queue[i] < 0.1, "cruising but still queued");
                }
            }
        });
    }
}
