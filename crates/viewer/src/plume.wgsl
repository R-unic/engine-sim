// Ray-marched participating medium for everything that leaves the tailpipe: exhaust haze and steam (scattering),
// soot (absorbing), and flame (emitting, with a black-body colour). The CPU simulates particles and splats them into
// a 3D texture (see volume.rs); this shader integrates it along each camera ray, lights it from the key and fill
// lights with a self-shadow march, lets the flames light the smoke around them, and adds the fine turbulent detail
// the coarse grid cannot hold by eroding the density with moving noise.
//
// Channels of the field: r = haze/steam density (1/m of extinction), g = soot density (1/m), b = flame emission
// (radiance per metre before the temperature curve), a = flame temperature / 2500 K (emission-weighted).

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
    prepass_utils,
    view_transformations::{frag_coord_to_ndc, position_ndc_to_world, position_world_to_ndc},
}

struct Plume {
    // the box the ray is clipped to (tight around what is alive) and the box the texture covers, both in world space
    box_min: vec4<f32>,
    box_max: vec4<f32>,
    tex_min: vec4<f32>,
    tex_size: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    fill_dir: vec4<f32>,
    fill_col: vec4<f32>,
    amb: vec4<f32>,
    // x: time (s), y: steps, z: flame brightness, w: detail amount
    misc: vec4<f32>,
    // xyz: wind the detail noise is carried by (m/s), w: haze tint strength
    wind: vec4<f32>,
    // xyz: where the flames are (emission-weighted), w: their power
    flame_pos: vec4<f32>,
    haze_tint: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> plume: Plume;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var field: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var field_s: sampler;

const PI: f32 = 3.14159265;

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
        q = q * 2.03 + vec3(17.1, 3.7, 9.2);
        a *= 0.5;
    }
    return s / 0.875;
}

fn hg(cos_t: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(1.0 + g2 - 2.0 * g * cos_t, 1.5));
}

// colour of a black body, normalised to its brightest channel (Krystek's fit of the Planckian locus, linear sRGB)
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

fn sample_field(p: vec3<f32>) -> vec4<f32> {
    let uvw = (p - plume.tex_min.xyz) / plume.tex_size.xyz;
    if any(uvw < vec3(0.0)) || any(uvw > vec3(1.0)) {
        return vec4(0.0);
    }
    return textureSampleLevel(field, field_s, uvw, 0.0);
}

// a ray against the box: (t_near, t_far)
fn box_hit(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec2<f32> {
    let inv = 1.0 / select(rd, vec3(1e-6), abs(rd) < vec3(1e-6));
    let t0 = (bmin - ro) * inv;
    let t1 = (bmax - ro) * inv;
    let tn = min(t0, t1);
    let tf = max(t0, t1);
    return vec2(max(max(tn.x, tn.y), tn.z), min(min(tf.x, tf.y), tf.z));
}

struct FragOut {
    @location(0) color: vec4<f32>,
    // the volume writes a depth of its own (where it is dense or bright enough to be a thing in the picture): the
    // depth-of-field pass blurs by depth, and without this the smoke is blurred as if it were the far wall behind it
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fragment(in: VertexOutput) -> FragOut {
    let ro = view.world_position;
    let to_frag = in.world_position.xyz - ro;
    let dist_frag = length(to_frag);
    let rd = to_frag / dist_frag;

    let hit = box_hit(ro, rd, plume.box_min.xyz, plume.box_max.xyz);
    var ta = max(hit.x, 0.0);
    var tb = hit.y;

    // whatever opaque thing is in the way ends the ray
    var scene_depth = 0.0;
#ifdef DEPTH_PREPASS
    scene_depth = prepass_utils::prepass_depth(in.position, 0u);
    if scene_depth > 0.0 {
        let ndc = frag_coord_to_ndc(in.position);
        let scene = position_ndc_to_world(vec3(ndc.xy, scene_depth));
        tb = min(tb, length(scene - ro));
    }
#endif
    if tb <= ta {
        discard;
    }

    let steps = i32(plume.misc.y);
    let len = tb - ta;
    let dt = len / f32(steps);
    // interleaved gradient noise: a different start for neighbouring pixels hides the banding of the steps
    let jit = fract(52.9829189 * fract(dot(in.position.xy, vec2(0.06711056, 0.00583715))) + plume.misc.x * 7.31);
    var t = ta + dt * jit;

    let time = plume.misc.x;
    let scroll = plume.wind.xyz * time;
    let sun = normalize(plume.sun_dir.xyz);
    let fill = normalize(plume.fill_dir.xyz);
    let cos_sun = dot(rd, sun);
    let cos_fill = dot(rd, fill);
    // forward-scattering lobe plus a little back-scatter: smoke lit from behind glows, from the front is flat grey
    let ph_sun = mix(hg(cos_sun, 0.55), hg(cos_sun, -0.25), 0.3);
    let ph_fill = mix(hg(cos_fill, 0.45), hg(cos_fill, -0.2), 0.3);
    let ph_iso = 1.0 / (4.0 * PI);

    var trans = 1.0;
    var col = vec3(0.0);
    var w_sum = 0.0;
    var t_sum = 0.0;
    for (var i = 0; i < steps; i++) {
        if trans < 0.02 {
            break;
        }
        let p = ro + rd * t;
        t += dt;
        let f = sample_field(p);
        if f.r + f.g + f.b < 0.004 {
            continue;
        }
        // fine structure: dense cores keep their shape, thin edges break into wisps; the noise is carried along
        let q = p * 7.0 - scroll * 7.0;
        let n = fbm3(q + vec3(0.0, -time * 0.9, 0.0));
        let n2 = vnoise(p * 21.0 - scroll * 21.0 + vec3(time * 1.7, 0.0, 0.0));
        let body = clamp((f.r + f.g) / (f.r + f.g + 1.5), 0.0, 1.0);
        let detail = plume.misc.w;
        let carve = mix(1.0, smoothstep(0.18, 0.82, n * (0.65 + 0.7 * n2)) * 1.9, detail * (1.0 - 0.55 * body));
        let haze = f.r * carve;
        let soot = f.g * carve;
        // flame: a young flame is solid, an old one breaks into sheets and fragments as its fuel runs out
        let tk = max(f.a * 2500.0, 1000.0);
        // domain-warped noise: tongues and filaments of flame, not a cloud
        let warp = vec3(
            vnoise(p * 11.0 + vec3(0.0, -time * 3.0, 0.0)),
            vnoise(p * 11.0 + vec3(5.2, 1.3, -time * 2.4)),
            vnoise(p * 11.0 + vec3(2.7, 7.1, time * 2.0)),
        ) - vec3(0.5);
        let fl_n = fbm3((p + warp * 0.07) * 26.0 - scroll * 26.0 + vec3(0.0, -time * 6.0, 0.0));
        let age_k = clamp((2300.0 - tk) / 1300.0, 0.0, 1.0);
        let thr = mix(0.16, 0.56, age_k);
        let flame = f.b * mix(1.0, smoothstep(thr, thr + 0.24, fl_n) * 2.8, detail);

        let sigma_t = max(haze + soot * 1.25, 1e-4);
        // light reaching this point: a short march toward the key light for self-shadowing
        var tau_l = 0.0;
        for (var j = 1; j <= 3; j++) {
            let ql = p + sun * (0.07 * f32(j));
            let fq = sample_field(ql);
            tau_l += (fq.r + fq.g * 1.25) * 0.07;
        }
        let shadow = exp(-tau_l * 1.1);
        // flames light the smoke around them
        let df = p - plume.flame_pos.xyz;
        let glow = plume.flame_pos.w / (0.03 + dot(df, df) * 9.0);
        let l_in = plume.sun_col.rgb * ph_sun * shadow + plume.fill_col.rgb * ph_fill + plume.amb.rgb * ph_iso + vec3(1.0, 0.45, 0.14) * glow * ph_iso * 4.0;
        let albedo = (haze * plume.haze_tint.rgb * 0.93 + soot * 1.25 * vec3(0.34, 0.31, 0.28)) / sigma_t;
        let scatter = albedo * l_in * sigma_t;

        // emission: black body at the flame's temperature, brightness following T^3.5
        let emit = blackbody(tk) * pow(tk / 2000.0, 3.5) * flame * plume.misc.z;

        let ext = sigma_t + flame * 0.06;
        let tr_step = exp(-ext * dt);
        let k = select((1.0 - tr_step) / ext, dt, ext < 1e-4);
        let contrib = trans * (scatter + emit) * k;
        col += contrib;
        // where the picture's weight is, along the ray
        let wgt = trans * (1.0 - tr_step) + 0.4 * dot(contrib, vec3(0.3, 0.5, 0.2));
        w_sum += wgt;
        t_sum += wgt * (t - 0.5 * dt);
        trans *= tr_step;
    }
    let a = 1.0 - trans;
    let lum = dot(col, vec3(0.3, 0.5, 0.2));
    if a < 0.001 && lum < 0.001 {
        discard;
    }
    var out: FragOut;
    out.color = vec4(col, a);
    var d = scene_depth;
    if (a > 0.08 || lum > 0.25) && w_sum > 1e-6 {
        let tm = t_sum / w_sum;
        d = max(d, position_world_to_ndc(ro + rd * tm).z);
    }
    out.depth = d;
    return out;
}
