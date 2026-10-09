// The flame in one combustion chamber. The mesh is a cube whose local [-1, 1] range is the chamber (x, z across the
// bore, y along the cylinder, the plug at the top); the ray is marched through it in the chamber's own frame.
//
// A spark kernel grows around the plug into a turbulent flame front. Its surface is wrinkled by noise that is carried
// along with time; the front itself is a thin sheet of bright, nearly white-yellow reaction zone with a bluish skin,
// and the burned gas behind it glows with the colour of a black body.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
    mesh_functions::get_world_from_local,
    prepass_utils,
    view_transformations::{frag_coord_to_ndc, position_ndc_to_world},
}

struct Flame {
    // x: radius of the flame front (m), y: heat, z: time (s), w: seed
    state: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> flame: Flame;

fn hash13(p3: vec3<f32>) -> f32 {
    var p = fract(p3 * 0.1031);
    p += dot(p, p.zyx + 31.32);
    return fract((p.x + p.y) * p.z);
}

fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash13(i), hash13(i + vec3(1.0, 0.0, 0.0)), u.x), mix(hash13(i + vec3(0.0, 1.0, 0.0)), hash13(i + vec3(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash13(i + vec3(0.0, 0.0, 1.0)), hash13(i + vec3(1.0, 0.0, 1.0)), u.x), mix(hash13(i + vec3(0.0, 1.0, 1.0)), hash13(i + vec3(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

fn fbm3(p: vec3<f32>) -> f32 {
    var q = p;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0; i < 3; i++) {
        s += a * vnoise(q);
        q = q * 2.07 + vec3(11.7, 5.3, 2.9);
        a *= 0.5;
    }
    return s / 0.875;
}

fn blackbody(t_in: f32) -> vec3<f32> {
    let t = clamp(t_in, 1000.0, 9000.0);
    let t2 = t * t;
    let u = (0.860117757 + 1.54118254e-4 * t + 1.28641212e-7 * t2) / (1.0 + 8.42420235e-4 * t + 7.08145163e-7 * t2);
    let v = (0.317398726 + 4.22806245e-5 * t + 4.20481691e-8 * t2) / (1.0 - 2.89741816e-5 * t + 1.61456053e-7 * t2);
    let d = 2.0 * u - 8.0 * v + 4.0;
    let x = 3.0 * u / d;
    let y = 2.0 * v / d;
    let xyz = vec3(x / y, 1.0, (1.0 - x - y) / y);
    let rgb = vec3(
        3.2406 * xyz.x - 1.5372 * xyz.y - 0.4986 * xyz.z,
        -0.9689 * xyz.x + 1.8758 * xyz.y + 0.0415 * xyz.z,
        0.0557 * xyz.x - 0.2040 * xyz.y + 1.0570 * xyz.z,
    );
    let c = max(rgb, vec3(0.0));
    return c / max(max(c.x, c.y), max(c.z, 1e-4));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let heat = flame.state.y;
    if heat < 0.01 {
        discard;
    }
    let m = get_world_from_local(in.instance_index);
    let c0 = m[0].xyz;
    let c1 = m[1].xyz;
    let c2 = m[2].xyz;
    let origin = m[3].xyz;
    let sc = vec3(length(c0), length(c1), length(c2));
    let k0 = c0 / dot(c0, c0);
    let k1 = c1 / dot(c1, c1);
    let k2 = c2 / dot(c2, c2);

    let ro = view.world_position;
    let to_frag = in.world_position.xyz - ro;
    let rd = normalize(to_frag);
    let d0 = ro - origin;
    let ro_l = vec3(dot(k0, d0), dot(k1, d0), dot(k2, d0));
    let rd_l = vec3(dot(k0, rd), dot(k1, rd), dot(k2, rd));

    // the ray through the cube [-1, 1]^3 (t in metres, because rd is a unit vector in the world)
    let inv = 1.0 / select(rd_l, vec3(1e-6), abs(rd_l) < vec3(1e-6));
    let t0 = (-1.0 - ro_l) * inv;
    let t1 = (1.0 - ro_l) * inv;
    let tn = min(t0, t1);
    let tf = max(t0, t1);
    var ta = max(max(max(tn.x, tn.y), tn.z), 0.0);
    var tb = min(min(tf.x, tf.y), tf.z);
#ifdef DEPTH_PREPASS
    let depth = prepass_utils::prepass_depth(in.position, 0u);
    if depth > 0.0 {
        let ndc = frag_coord_to_ndc(in.position);
        let scene = position_ndc_to_world(vec3(ndc.xy, depth));
        tb = min(tb, length(scene - ro));
    }
#endif
    if tb <= ta {
        discard;
    }

    let time = flame.state.z;
    let seed = flame.state.w;
    let front_r = max(flame.state.x, 0.002);
    let bore_r = sc.x;
    let steps = 22;
    let dt = (tb - ta) / f32(steps);
    let jit = fract(52.9829189 * fract(dot(in.position.xy, vec2(0.06711056, 0.00583715))) + seed);
    var t = ta + dt * jit;

    // temperature of the burned gas: hottest when the burn is strongest, glowing orange as it dies away
    let t_burned = 1750.0 + 750.0 * clamp(heat, 0.0, 1.0);
    let bb_burned = blackbody(t_burned);
    let bb_front = blackbody(2900.0);
    var col = vec3(0.0);
    for (var i = 0; i < steps; i++) {
        let p_l = ro_l + rd_l * t;
        t += dt;
        let p = p_l * sc;
        let rel = p.xz;
        let rho = length(rel);
        if rho > bore_r {
            continue;
        }
        // the front is wrinkled at three scales and the wrinkles are blown around by the swirl and the piston
        let q = vec3(rel * (5.5 / bore_r), p_l.y * 1.4) + vec3(seed, seed * 0.37, time * 2.6);
        let n1 = fbm3(q);
        let n2 = vnoise(q * 3.1 + vec3(3.0, 1.0, time * 5.0));
        let wrinkle = (n1 - 0.5) * 0.55 + (n2 - 0.5) * 0.22;
        let rho_f = front_r * (1.0 + wrinkle) + 0.5 * bore_r * 0.05 * (n1 - 0.5);
        let dist = rho - rho_f;
        // the reaction zone: thin and sharp
        let w = 0.05 * bore_r + 0.0008;
        let sheet = exp(-(dist / w) * (dist / w));
        // the burned gas: everything inside, a little dimmer toward the middle where it burned first and has cooled
        let inside = 1.0 - smoothstep(-0.012, 0.01, dist);
        let depth_in = clamp(-dist / max(front_r, 1e-3), 0.0, 1.0);
        let burned = inside * (0.55 + 0.45 * (1.0 - depth_in)) * (0.75 + 0.5 * n2);
        // pockets of unburnt gas the front has passed by: dark holes in the glow
        let hole = smoothstep(0.78, 0.9, n1) * inside * 0.7;
        let e_sheet = sheet * 6.0;
        let e_burn = burned * (1.0 - hole) * 1.5;
        col += (bb_front * vec3<f32>(0.83, 0.90, 1.0) * e_sheet + bb_burned * e_burn) * dt * 55.0;
    }
    col *= pow(clamp(heat, 0.0, 1.5), 1.3) * 1.15;
    // AlphaMode::Add is premultiplied blending: what is added is rgb, and the alpha must be 0 or the background is darkened
    return vec4(col, 0.0);
}
