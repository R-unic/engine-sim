// A parcel of gas flowing through the intake or the exhaust: a soft Gaussian blob seen through a lens. The line
// integral of a spherical Gaussian through its centre is a Gaussian in the distance from the centre of the picture,
// so a camera-facing quad with that falloff *is* the volume (no sprite sheet), and eroding it with moving noise makes
// it a wisp of turbulent gas rather than a ball.
//
// Vertex colour: rgb = what the gas radiates or scatters (premultiplied), a = how much it absorbs behind it.
// uv_b.x = a seed of this parcel, uv_b.y = how far through its life it is.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::globals,
}

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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let r2 = dot(uv, uv);
    if r2 >= 1.0 {
        discard;
    }
    let seed = in.uv_b.x * 41.0;
    let life = in.uv_b.y;
    // 3 sigma at the edge of the quad
    let g = exp(-4.5 * r2) * (1.0 - smoothstep(0.6, 1.0, r2));
    let t = globals.time;
    let n = vnoise(vec3(uv * 2.6 + vec2(seed, seed * 0.7), t * 1.7 + seed)) * 0.65 + vnoise(vec3(uv * 6.1 - vec2(seed * 0.3, 0.0), t * 3.1 - seed)) * 0.35;
    let dens = g * (0.35 + 1.15 * n) * (1.0 - life * life);
    let c = in.color;
    return vec4(c.rgb * dens, c.a * dens);
}
