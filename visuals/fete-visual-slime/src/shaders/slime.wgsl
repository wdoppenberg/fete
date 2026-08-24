// Slime — a physarum polycephalum agent simulation.
//
// Millions of agents each do something trivial: sense the trail ahead of them
// in three directions, steer towards the strongest, step forward, and deposit.
// A separate pass blurs and decays the trail. That feedback loop — deposit,
// diffuse, follow — is the whole model, and it self-organises into transport
// networks that no part of the code describes.
//
// After Jeff Jones, "Characteristics of pattern formation and evolution in
// approximations of Physarum transport networks" (2010).

#import fete::noise::{hash11, hash12}

struct Agent {
    pos: vec2<f32>,
    angle: f32,
    // 0..1. Agents of different kinds deposit into and sense the same trail
    // but steer with slightly different constants, which is what produces the
    // interleaved, competing networks rather than one uniform mesh.
    kind: f32,
}

struct SlimeParams {
    resolution: vec2<f32>,
    agent_count: u32,
    time: f32,
    delta: f32,
    sensor_angle: f32,
    sensor_distance: f32,
    turn_speed: f32,
    move_speed: f32,
    deposit: f32,
    decay: f32,
    diffuse: f32,
    // Pushes agents outward from the centre on each beat.
    impulse: f32,
}

// Rgba16Float rather than the R32Float this simulation actually needs: only
// the red channel carries trail, but r32float is not filterable, and the
// display material wants to sample this texture bilinearly. The spare gba
// channels are free for per-species trails later.
@group(0) @binding(0) var trail_read: texture_storage_2d<rgba16float, read>;
@group(0) @binding(1) var trail_write: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<storage, read_write> agents: array<Agent>;
// Deposits accumulate here as fixed point rather than straight into the trail
// texture. Many agents land on the same texel in the same dispatch, and only
// an atomic add gets that right — a read-modify-write on a storage texture
// would silently drop most of them.
@group(0) @binding(3) var<storage, read_write> deposits: array<atomic<u32>>;
@group(0) @binding(4) var<uniform> params: SlimeParams;

// Deposits are scaled to fixed point before the atomic add. 1024 leaves plenty
// of headroom below u32 overflow while resolving contributions well under one
// unit of trail.
const DEPOSIT_SCALE: f32 = 1024.0;

fn texel_index(coord: vec2<i32>) -> u32 {
    return u32(coord.y) * u32(params.resolution.x) + u32(coord.x);
}

fn in_bounds(coord: vec2<i32>) -> bool {
    return coord.x >= 0 && coord.y >= 0
        && coord.x < i32(params.resolution.x)
        && coord.y < i32(params.resolution.y);
}

// Trail strength ahead of an agent, at a given angular offset.
fn sense(pos: vec2<f32>, angle: f32, offset: f32) -> f32 {
    let dir = vec2<f32>(cos(angle + offset), sin(angle + offset));
    let sample_pos = pos + dir * params.sensor_distance;

    // Sensing must wrap exactly as movement and diffusion do. Treating
    // off-screen as unattractive instead makes every agent near a border turn
    // away from it, and they accumulate into a bright rim around the whole
    // frame — a border artefact with no counterpart in the model.
    let bounds = vec2<i32>(params.resolution);
    let coord = (vec2<i32>(floor(sample_pos)) % bounds + bounds) % bounds;
    return textureLoad(trail_read, coord).r;
}

@compute @workgroup_size(64)
fn update_agents(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    // The workload is folded across x and y because a few million agents
    // exceeds the 65535 workgroups-per-dimension limit.
    let index = gid.x + gid.y * num_workgroups.x * 64u;
    if index >= params.agent_count {
        return;
    }

    var agent = agents[index];
    let rng = hash11(f32(index) * 0.017 + params.time * 13.7);

    // Steering. Three sensors, and the agent turns towards whichever reads
    // strongest. Turning by a fixed amount rather than proportionally to the
    // difference is what keeps the networks crisp: proportional steering
    // averages the sensors out and the structure dissolves into fog.
    let kind_bias = mix(0.75, 1.35, agent.kind);
    let ahead = sense(agent.pos, agent.angle, 0.0);
    let left = sense(agent.pos, agent.angle, params.sensor_angle * kind_bias);
    let right = sense(agent.pos, agent.angle, -params.sensor_angle * kind_bias);

    let turn = params.turn_speed * params.delta * kind_bias;
    if ahead > left && ahead > right {
        // Already heading the right way; hold.
    } else if left > right {
        agent.angle += turn;
    } else if right > left {
        agent.angle -= turn;
    } else {
        // Sensors tied — usually virgin territory. A random turn here is what
        // seeds new branches instead of letting agents march in straight lines.
        agent.angle += (rng - 0.5) * 2.0 * turn;
    }

    // Beat impulse: a radial shove outward from the centre. The networks
    // rebuild over the following second, so a kick reads as the whole
    // structure breathing.
    if params.impulse > 0.001 {
        let centre = params.resolution * 0.5;
        let outward = normalize(agent.pos - centre + vec2<f32>(0.001));
        // Not `target`: that is a reserved word in WGSL.
        let outward_angle = atan2(outward.y, outward.x);
        agent.angle = mix(agent.angle, outward_angle, params.impulse * 0.35);
    }

    let speed = params.move_speed * mix(0.8, 1.2, agent.kind);
    var next = agent.pos + vec2<f32>(cos(agent.angle), sin(agent.angle)) * speed * params.delta;

    // Wrap at the edges. Reflecting instead would build up a visible rim of
    // agents along each border; wrapping keeps the density uniform and makes
    // the field read as a window onto something larger.
    next = (next + params.resolution) % params.resolution;
    agent.pos = next;
    agents[index] = agent;

    let coord = vec2<i32>(floor(next));
    if in_bounds(coord) {
        atomicAdd(&deposits[texel_index(coord)], u32(params.deposit * DEPOSIT_SCALE));
    }
}

@compute @workgroup_size(8, 8)
fn diffuse(@builtin(global_invocation_id) gid: vec3<u32>) {
    let coord = vec2<i32>(gid.xy);
    if !in_bounds(coord) {
        return;
    }

    // 3x3 box blur of the previous trail. Cheap, and one blur per frame
    // compounded over many frames approximates a gaussian anyway.
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            var sample_coord = coord + vec2<i32>(dx, dy);
            // Wrap to match the agents' wrapping, so the blur does not darken
            // the borders.
            sample_coord = (sample_coord + vec2<i32>(params.resolution)) % vec2<i32>(params.resolution);
            sum += textureLoad(trail_read, sample_coord).r;
        }
    }
    let blurred = sum / 9.0;

    let original = textureLoad(trail_read, coord).r;
    var value = mix(original, blurred, params.diffuse);

    // Fold in this frame's deposits and clear the accumulator for the next.
    let index = texel_index(coord);
    let deposited = f32(atomicExchange(&deposits[index], 0u)) / DEPOSIT_SCALE;
    value += deposited;

    // Decay. Without it the trail saturates everywhere within seconds and the
    // agents lose the gradient they are following.
    value *= params.decay;

    // Cap well above the display range: the material tone-maps this itself, and
    // letting hot spots run to 8 gives the bloom something to catch.
    textureStore(trail_write, coord, vec4<f32>(min(value, 8.0), 0.0, 0.0, 1.0));
}

// Seeds the trail. Run once when the visual starts.
@compute @workgroup_size(8, 8)
fn clear(@builtin(global_invocation_id) gid: vec3<u32>) {
    let coord = vec2<i32>(gid.xy);
    if !in_bounds(coord) {
        return;
    }
    // A faint noise floor rather than zero. On a perfectly flat trail every
    // sensor reads the same value and the agents' tie-break is the only thing
    // driving them, which takes far longer to organise.
    let seed = hash12(vec2<f32>(coord) * 0.01 + params.time) * 0.05;
    textureStore(trail_write, coord, vec4<f32>(seed, 0.0, 0.0, 1.0));
    atomicStore(&deposits[texel_index(coord)], 0u);
}
