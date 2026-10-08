//! Procedural mesh toolkit. Everything in the rig is generated from real engine dimensions:
//! lathes (revolved profiles), extrusions (gears, crank webs, cam lobes), swept tubes (exhaust,
//! springs), turbine blades, conrods.
//!
//! Convention: revolved/extruded parts have their axis along local +X; the cross-section lives in
//! the YZ plane. Use [`MB::rot_to_y`] to stand a part upright.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

#[derive(Default, Clone)]
pub struct MB {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl MB {
    pub fn new() -> Self {
        Self::default()
    }

    fn vert(&mut self, p: Vec3, n: Vec3, uv: [f32; 2]) -> u32 {
        self.pos.push(p.to_array());
        self.nrm.push(n.to_array());
        self.uv.push(uv);
        (self.pos.len() - 1) as u32
    }

    /// Adds a triangle, flipping the winding if its geometric normal opposes `hint`.
    fn tri(&mut self, a: u32, b: u32, c: u32, hint: Vec3) {
        let pa = Vec3::from(self.pos[a as usize]);
        let pb = Vec3::from(self.pos[b as usize]);
        let pc = Vec3::from(self.pos[c as usize]);
        let g = (pb - pa).cross(pc - pa);
        if g.dot(hint) >= 0.0 {
            self.idx.extend_from_slice(&[a, b, c]);
        } else {
            self.idx.extend_from_slice(&[a, c, b]);
        }
    }

    pub fn append(&mut self, o: &MB, m: Mat4) {
        let nm = Mat3::from_mat4(m).inverse().transpose();
        let base = self.pos.len() as u32;
        for i in 0..o.pos.len() {
            self.pos.push(m.transform_point3(Vec3::from(o.pos[i])).to_array());
            self.nrm.push((nm * Vec3::from(o.nrm[i])).normalize_or_zero().to_array());
            self.uv.push(o.uv[i]);
        }
        self.idx.extend(o.idx.iter().map(|i| i + base));
    }

    pub fn transformed(mut self, m: Mat4) -> MB {
        let nm = Mat3::from_mat4(m).inverse().transpose();
        for p in self.pos.iter_mut() {
            *p = m.transform_point3(Vec3::from(*p)).to_array();
        }
        for n in self.nrm.iter_mut() {
            *n = (nm * Vec3::from(*n)).normalize_or_zero().to_array();
        }
        self
    }

    /// Stands an X-axis part upright (axis becomes +Y).
    pub fn rot_to_y(self) -> MB {
        self.transformed(Mat4::from_quat(Quat::from_rotation_z(FRAC_PI_2)))
    }

    /// Axis along +Z (X -> Z).
    pub fn rot_to_z(self) -> MB {
        self.transformed(Mat4::from_quat(Quat::from_rotation_y(-FRAC_PI_2)))
    }

    pub fn translated(self, v: Vec3) -> MB {
        self.transformed(Mat4::from_translation(v))
    }

    pub fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        mesh.insert_indices(Indices::U32(self.idx));
        mesh
    }

    // ------------------------------------------------------------------ revolved shapes

    /// Revolves a profile of `(x, radius, smooth)` points about the X axis. Each profile segment
    /// is its own strip (hard edges between segments) unless the shared point is marked smooth.
    pub fn lathe(profile: &[(f32, f32, bool)], segs: usize) -> MB {
        let mut mb = MB::new();
        let n = profile.len();
        if n < 2 {
            return mb;
        }
        // Outward normal of each segment in (x, r) space.
        let seg_n: Vec<Vec2> = (0..n - 1)
            .map(|i| {
                let d = Vec2::new(profile[i + 1].0 - profile[i].0, profile[i + 1].1 - profile[i].1);
                Vec2::new(-d.y, d.x).normalize_or_zero()
            })
            .collect();
        for i in 0..n - 1 {
            let (x0, r0, s0) = profile[i];
            let (x1, r1, s1) = profile[i + 1];
            let n0 = if s0 && i > 0 { (seg_n[i] + seg_n[i - 1]).normalize_or_zero() } else { seg_n[i] };
            let n1 = if s1 && i + 2 < n { (seg_n[i] + seg_n[i + 1]).normalize_or_zero() } else { seg_n[i] };
            let mut ring0 = Vec::with_capacity(segs + 1);
            let mut ring1 = Vec::with_capacity(segs + 1);
            for k in 0..=segs {
                let a = TAU * k as f32 / segs as f32;
                let (s, c) = a.sin_cos();
                let u = k as f32 / segs as f32;
                ring0.push(mb.vert(
                    Vec3::new(x0, r0 * c, r0 * s),
                    Vec3::new(n0.x, n0.y * c, n0.y * s).normalize_or_zero(),
                    [u, i as f32 / (n - 1) as f32],
                ));
                ring1.push(mb.vert(
                    Vec3::new(x1, r1 * c, r1 * s),
                    Vec3::new(n1.x, n1.y * c, n1.y * s).normalize_or_zero(),
                    [u, (i + 1) as f32 / (n - 1) as f32],
                ));
            }
            for k in 0..segs {
                let hint = Vec3::from(mb.nrm[ring0[k] as usize]) + Vec3::from(mb.nrm[ring1[k + 1] as usize]);
                // Degenerate (zero-radius) edges produce zero-area triangles; harmless.
                mb.tri(ring0[k], ring0[k + 1], ring1[k + 1], hint);
                mb.tri(ring0[k], ring1[k + 1], ring1[k], hint);
            }
        }
        mb
    }

    /// Solid cylinder along X from `x0` to `x1`.
    pub fn cylinder(r: f32, x0: f32, x1: f32, segs: usize) -> MB {
        MB::lathe(&[(x0, 0.0, false), (x0, r, false), (x1, r, false), (x1, 0.0, false)], segs)
    }

    /// Hollow tube along X.
    pub fn tube(r_out: f32, r_in: f32, x0: f32, x1: f32, segs: usize) -> MB {
        MB::lathe(
            &[(x0, r_in, false), (x0, r_out, false), (x1, r_out, false), (x1, r_in, false), (x0, r_in, false)],
            segs,
        )
    }

    /// Cone/frustum along X.
    pub fn cone(r0: f32, r1: f32, x0: f32, x1: f32, segs: usize) -> MB {
        MB::lathe(&[(x0, 0.0, false), (x0, r0, false), (x1, r1, false), (x1, 0.0, false)], segs)
    }

    pub fn sphere(r: f32, segs: usize) -> MB {
        let rings = segs.max(6);
        let prof: Vec<(f32, f32, bool)> = (0..=rings)
            .map(|i| {
                let a = PI * i as f32 / rings as f32;
                (-r * a.cos(), r * a.sin(), true)
            })
            .collect();
        MB::lathe(&prof, segs)
    }

    // ------------------------------------------------------------------ boxes

    pub fn cuboid(sx: f32, sy: f32, sz: f32) -> MB {
        MB::frustum_box(-sy * 0.5, sy * 0.5, Vec2::new(sx, sz), Vec2::new(sx, sz))
    }

    /// Box-like prism along Y from `y0` to `y1`, with an (x, z) cross-section size at each end.
    pub fn frustum_box(y0: f32, y1: f32, s0: Vec2, s1: Vec2) -> MB {
        let h0 = s0 * 0.5;
        let h1 = s1 * 0.5;
        let c = [
            Vec3::new(-h0.x, y0, -h0.y),
            Vec3::new(h0.x, y0, -h0.y),
            Vec3::new(h0.x, y0, h0.y),
            Vec3::new(-h0.x, y0, h0.y),
            Vec3::new(-h1.x, y1, -h1.y),
            Vec3::new(h1.x, y1, -h1.y),
            Vec3::new(h1.x, y1, h1.y),
            Vec3::new(-h1.x, y1, h1.y),
        ];
        let centre = c.iter().copied().sum::<Vec3>() / 8.0;
        let faces: [[usize; 4]; 6] =
            [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [3, 2, 6, 7], [0, 3, 7, 4], [1, 2, 6, 5]];
        let mut mb = MB::new();
        for f in faces {
            let p: Vec<Vec3> = f.iter().map(|&i| c[i]).collect();
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let fc = (p[0] + p[1] + p[2] + p[3]) / 4.0;
            let n = if n.dot(fc - centre) >= 0.0 { n } else { -n };
            let v: Vec<u32> = p
                .iter()
                .enumerate()
                .map(|(i, &q)| mb.vert(q, n, [(i & 1) as f32, (i >> 1) as f32]))
                .collect();
            mb.tri(v[0], v[1], v[2], n);
            mb.tri(v[0], v[2], v[3], n);
        }
        mb
    }

    // ------------------------------------------------------------------ extrusions

    /// Extrudes a YZ-plane polygon along X from `x0` to `x1`, optionally twisting about X
    /// (helical gears). Flat-shaded; the polygon should be star-shaped around its centroid.
    pub fn extrude(poly: &[Vec2], x0: f32, x1: f32, twist: f32, layers: usize) -> MB {
        let mut mb = MB::new();
        let n = poly.len();
        let centroid = poly.iter().copied().sum::<Vec2>() / n as f32;
        let layers = layers.max(1);
        let place = |p: Vec2, l: usize| -> Vec3 {
            let t = l as f32 / layers as f32;
            let a = twist * t;
            let (s, c) = a.sin_cos();
            Vec3::new(x0 + (x1 - x0) * t, p.x * c - p.y * s, p.x * s + p.y * c)
        };
        for l in 0..layers {
            for i in 0..n {
                let j = (i + 1) % n;
                let a = place(poly[i], l);
                let b = place(poly[j], l);
                let c = place(poly[j], l + 1);
                let d = place(poly[i], l + 1);
                let mid = (a + b + c + d) / 4.0;
                let out = Vec3::new(0.0, mid.y, mid.z);
                let nrm = (b - a).cross(d - a).normalize_or_zero();
                let nrm = if nrm.dot(out) >= 0.0 { nrm } else { -nrm };
                let v0 = mb.vert(a, nrm, [0.0, 0.0]);
                let v1 = mb.vert(b, nrm, [1.0, 0.0]);
                let v2 = mb.vert(c, nrm, [1.0, 1.0]);
                let v3 = mb.vert(d, nrm, [0.0, 1.0]);
                mb.tri(v0, v1, v2, nrm);
                mb.tri(v0, v2, v3, nrm);
            }
        }
        for (x_idx, nx) in [(0usize, -1.0f32), (layers, 1.0f32)] {
            let n3 = Vec3::new(nx, 0.0, 0.0);
            let cidx = mb.vert(place(centroid, x_idx), n3, [0.5, 0.5]);
            for i in 0..n {
                let j = (i + 1) % n;
                let a = mb.vert(place(poly[i], x_idx), n3, [0.0, 0.0]);
                let b = mb.vert(place(poly[j], x_idx), n3, [1.0, 0.0]);
                mb.tri(cidx, a, b, n3);
            }
        }
        mb
    }

    // ------------------------------------------------------------------ sweeps

    /// Sweeps a circle of per-point `radii` along `path` (parallel-transport frames), with caps.
    pub fn sweep_tube(path: &[Vec3], radii: &[f32], segs: usize, caps: bool) -> MB {
        let mut mb = MB::new();
        let n = path.len();
        if n < 2 {
            return mb;
        }
        let tangent = |i: usize| -> Vec3 {
            let a = path[i.saturating_sub(1)];
            let b = path[(i + 1).min(n - 1)];
            (b - a).normalize_or_zero()
        };
        let mut t = tangent(0);
        let mut nrm = if t.dot(Vec3::Y).abs() < 0.9 { t.cross(Vec3::Y).normalize() } else { t.cross(Vec3::X).normalize() };
        let mut rings: Vec<Vec<u32>> = Vec::with_capacity(n);
        for i in 0..n {
            let nt = tangent(i);
            if i > 0 {
                let axis = t.cross(nt);
                if axis.length() > 1e-6 {
                    let ang = t.dot(nt).clamp(-1.0, 1.0).acos();
                    nrm = Quat::from_axis_angle(axis.normalize(), ang) * nrm;
                }
                t = nt;
            }
            let bin = t.cross(nrm).normalize_or_zero();
            let r = radii[i.min(radii.len() - 1)];
            let mut ring = Vec::with_capacity(segs + 1);
            for k in 0..=segs {
                let a = TAU * k as f32 / segs as f32;
                let dir = nrm * a.cos() + bin * a.sin();
                ring.push(mb.vert(path[i] + dir * r, dir, [k as f32 / segs as f32, i as f32 / (n - 1) as f32]));
            }
            rings.push(ring);
        }
        for i in 0..n - 1 {
            for k in 0..segs {
                let hint = Vec3::from(mb.nrm[rings[i][k] as usize]);
                mb.tri(rings[i][k], rings[i][k + 1], rings[i + 1][k + 1], hint);
                mb.tri(rings[i][k], rings[i + 1][k + 1], rings[i + 1][k], hint);
            }
        }
        if caps {
            for (i, sign) in [(0usize, -1.0f32), (n - 1, 1.0)] {
                let tn = tangent(i) * sign;
                let c = mb.vert(path[i], tn, [0.5, 0.5]);
                let r = radii[i.min(radii.len() - 1)];
                // Rebuild the ring with the cap normal.
                let ring_pos: Vec<Vec3> = rings[i].iter().map(|&v| Vec3::from(mb.pos[v as usize])).collect();
                let _ = r;
                let cv: Vec<u32> = ring_pos.iter().map(|&p| mb.vert(p, tn, [0.0, 0.0])).collect();
                for k in 0..segs {
                    mb.tri(c, cv[k], cv[k + 1], tn);
                }
            }
        }
        mb
    }

    // ------------------------------------------------------------------ 2D helpers

    pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
        let mut p = points.to_vec();
        p.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap().then(a.y.partial_cmp(&b.y).unwrap()));
        p.dedup_by(|a, b| (*a - *b).length() < 1e-6);
        if p.len() < 3 {
            return p;
        }
        let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
        let mut lower: Vec<Vec2> = Vec::new();
        for &q in &p {
            while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 {
                lower.pop();
            }
            lower.push(q);
        }
        let mut upper: Vec<Vec2> = Vec::new();
        for &q in p.iter().rev() {
            while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 {
                upper.pop();
            }
            upper.push(q);
        }
        lower.pop();
        upper.pop();
        lower.extend(upper);
        lower
    }

    pub fn circle_pts(centre: Vec2, r: f32, n: usize) -> Vec<Vec2> {
        (0..n).map(|i| centre + Vec2::from_angle(TAU * i as f32 / n as f32) * r).collect()
    }

    pub fn arc_pts(centre: Vec2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Vec2> {
        (0..=n).map(|i| centre + Vec2::from_angle(a0 + (a1 - a0) * i as f32 / n as f32) * r).collect()
    }
}

// ============================================================================ composite parts

/// Spur/helical gear outline polygon with `teeth` teeth.
pub fn gear_outline(teeth: usize, r_root: f32, r_tip: f32) -> Vec<Vec2> {
    let p = TAU / teeth as f32;
    let mut pts = Vec::with_capacity(teeth * 4);
    for i in 0..teeth {
        let c = i as f32 * p;
        pts.push(Vec2::from_angle(c - 0.25 * p) * r_root);
        pts.push(Vec2::from_angle(c - 0.12 * p) * r_tip);
        pts.push(Vec2::from_angle(c + 0.12 * p) * r_tip);
        pts.push(Vec2::from_angle(c + 0.25 * p) * r_root);
    }
    pts
}

/// A gear: toothed rim + hub, along X from `x0` to `x1`.
pub fn gear_mesh(teeth: usize, module: f32, width: f32, helix: f32, hub_r: f32) -> MB {
    let r_pitch = module * teeth as f32 * 0.5;
    let r_root = r_pitch - 1.1 * module;
    let r_tip = r_pitch + 0.9 * module;
    let mut mb = MB::extrude(&gear_outline(teeth, r_root, r_tip), 0.0, width, helix, 5);
    mb.append(&MB::cylinder(hub_r.min(r_root * 0.8), -0.003, width + 0.003, 24), Mat4::IDENTITY);
    mb
}

/// Crank web: convex hull of the pin circle, main journal circle and a counterweight arc opposite
/// the pin. Pin sits at local angle 0 (+Y); extruded along X from `x0` to `x1`.
pub fn crank_web(throw: f32, r_main: f32, r_pin: f32, r_cw: f32, cw_half_span: f32, x0: f32, x1: f32) -> MB {
    let mut pts = MB::circle_pts(Vec2::new(throw, 0.0), r_pin * 1.55, 20);
    pts.extend(MB::circle_pts(Vec2::ZERO, r_main * 1.5, 20));
    pts.extend(MB::arc_pts(Vec2::ZERO, r_cw, PI - cw_half_span, PI + cw_half_span, 14));
    let hull = MB::convex_hull(&pts);
    MB::extrude(&hull, x0, x1, 0.0, 1)
}

/// Connecting rod, small end at origin, big end at (0, -len, 0); crank/pin axis along X.
pub fn conrod(len: f32, r_small: f32, r_big: f32, thickness: f32) -> MB {
    let mut mb = MB::new();
    let t = thickness;
    // pin eyes
    mb.append(&MB::tube(r_small * 1.6, r_small, -t / 2.0, t / 2.0, 24), Mat4::IDENTITY);
    mb.append(
        &MB::tube(r_big * 1.35, r_big, -t * 0.6, t * 0.6, 32),
        Mat4::from_translation(Vec3::new(0.0, -len, 0.0)),
    );
    // I-beam: web + two flanges, tapering toward the small end
    let y_top = -r_small * 1.4;
    let y_bot = -len + r_big * 1.25;
    let w_small = r_small * 2.4;
    let w_big = r_big * 1.9;
    mb.append(&MB::frustum_box(y_bot, y_top, Vec2::new(t * 0.35, w_big), Vec2::new(t * 0.35, w_small)), Mat4::IDENTITY);
    for sx in [-1.0f32, 1.0] {
        mb.append(
            &MB::frustum_box(y_bot, y_top, Vec2::new(t * 0.3, w_big * 0.95), Vec2::new(t * 0.3, w_small * 0.95)),
            Mat4::from_translation(Vec3::new(sx * t * 0.42, 0.0, 0.0)),
        );
    }
    // cap bolts
    for sz in [-1.0f32, 1.0] {
        mb.append(
            &MB::cylinder(r_big * 0.16, -t * 0.7, t * 0.7, 10),
            Mat4::from_translation(Vec3::new(0.0, -len - r_big * 1.1, sz * r_big * 0.7)),
        );
    }
    mb
}

/// Piston, axis +Y, origin at the wrist-pin centre, crown at +Y. Pin along X.
pub fn piston(bore: f32, comp_height: f32, skirt: f32) -> MB {
    let r = bore * 0.5 * 0.985;
    let top = comp_height;
    let bot = -skirt;
    let groove = 0.0025f32;
    let rr = r - 0.0014;
    let mut prof: Vec<(f32, f32, bool)> = vec![(bot, r * 0.78, false), (bot, r * 0.96, true), (bot + 0.002, r, false)];
    // skirt up to ring pack
    let mut y = top - 0.022;
    prof.push((y, r, false));
    for _ in 0..3 {
        prof.push((y, rr - 0.002, false));
        prof.push((y + groove, rr - 0.002, false));
        prof.push((y + groove, r, false));
        y += groove + 0.0035;
        prof.push((y, r, false));
    }
    prof.push((top - 0.0015, r, false));
    prof.push((top, r - 0.002, false));
    prof.push((top, 0.0, false));
    // Lathe revolves about X with x as axial coordinate - already ordered bottom to top.
    let mut mb = MB::lathe(&prof, 40).rot_to_y();
    // hollow skirt interior
    let inner = MB::lathe(
        &[(bot - 0.0005, r * 0.74, false), (top - 0.032, r * 0.74, false), (top - 0.032, 0.0, false)],
        32,
    )
    .rot_to_y();
    mb.append(&inner, Mat4::IDENTITY);
    // wrist pin + bosses
    let pin_r = bore * 0.12;
    mb.append(&MB::cylinder(pin_r, -r * 0.92, r * 0.92, 20), Mat4::IDENTITY);
    for s in [-1.0f32, 1.0] {
        mb.append(
            &MB::cylinder(pin_r * 1.7, s * r * 0.28, s * r * 0.62, 20),
            Mat4::IDENTITY,
        );
    }
    mb
}

/// Poppet valve, axis +Y with the head face at y = 0 and the stem going up.
pub fn valve(head_r: f32, stem_r: f32, stem_len: f32) -> MB {
    let prof = [
        (0.0, 0.0, false),
        (0.0, head_r, false),
        (0.0012, head_r, false),
        (0.006, head_r * 0.55, true),
        (0.014, stem_r * 1.2, true),
        (0.02, stem_r, false),
        (stem_len, stem_r, false),
        (stem_len, stem_r * 0.8, false),
        (stem_len + 0.003, stem_r * 0.8, false),
        (stem_len + 0.003, 0.0, false),
    ];
    MB::lathe(&prof, 20).rot_to_y()
}

/// Coil spring around +Y from y=0 to y=len.
pub fn spring(r: f32, wire: f32, coils: f32, len: f32) -> MB {
    let steps = (coils * 14.0) as usize;
    let path: Vec<Vec3> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let a = TAU * coils * t;
            Vec3::new(r * a.cos(), len * t, r * a.sin())
        })
        .collect();
    MB::sweep_tube(&path, &[wire], 6, true)
}

/// Cam lobe: polar profile along X. `lift_fn(psi)` returns extra radius at cam angle psi
/// (0 = nose pointing down the -Y axis). Extruded from x0 to x1.
pub fn cam_lobe(base_r: f32, lift: f32, half_width_cam_rad: f32, x0: f32, x1: f32) -> MB {
    let n = 96;
    let pts: Vec<Vec2> = (0..n)
        .map(|i| {
            let psi = TAU * i as f32 / n as f32;
            let d = ((psi + PI).rem_euclid(TAU)) - PI; // -pi..pi around the nose
            let bump = if d.abs() < half_width_cam_rad {
                let x = d / half_width_cam_rad * FRAC_PI_2;
                x.cos().powi(2)
            } else {
                0.0
            };
            // nose at -Y: point at angle psi measured from -Y towards +Z
            let r = base_r + lift * bump;
            Vec2::new(-r * psi.cos(), r * psi.sin())
        })
        .collect();
    MB::extrude(&pts, x0, x1, 0.0, 1)
}

/// Bump function shared by the cam lobe mesh and valve lift so the two always agree.
pub fn cam_bump(d_cam_rad: f32, half_width_cam_rad: f32) -> f32 {
    let d = ((d_cam_rad + PI).rem_euclid(TAU)) - PI;
    if d.abs() < half_width_cam_rad {
        (d / half_width_cam_rad * FRAC_PI_2).cos().powi(2)
    } else {
        0.0
    }
}

/// Bladed wheel (compressor or turbine): hub + `blades` twisted blades along X.
#[allow(clippy::too_many_arguments)]
pub fn bladed_wheel(blades: usize, r_hub: f32, r_tip: f32, depth: f32, sweep: f32, thickness: f32, backplate: bool, flip: bool) -> MB {
    let mut mb = MB::new();
    let hub_prof = [
        (0.0, 0.0, false),
        (0.0, r_hub * 1.4, false),
        (depth * 0.35, r_hub * 1.15, true),
        (depth, r_hub * 0.7, true),
        (depth + 0.004, r_hub * 0.6, false),
        (depth + 0.004, 0.0, false),
    ];
    mb.append(&MB::lathe(&hub_prof, 28), Mat4::IDENTITY);
    if backplate {
        mb.append(&MB::cylinder(r_tip * 0.97, -0.002, 0.0015, 40), Mat4::IDENTITY);
    }
    let rs = 8;
    let cs = 6;
    for b in 0..blades {
        let phi0 = TAU * b as f32 / blades as f32;
        let mut grid: Vec<Vec<(Vec3, Vec3)>> = Vec::new();
        for side in [-1.0f32, 1.0] {
            let mut g = Vec::new();
            for ri in 0..=rs {
                let s = ri as f32 / rs as f32;
                let radius = r_hub * 1.2 + (r_tip - r_hub * 1.2) * s;
                let mut row = Vec::new();
                for ci in 0..=cs {
                    let c = ci as f32 / cs as f32;
                    // axial extent shrinks toward the tip (mixed-flow wheel profile)
                    let ax_len = depth * (1.0 - 0.75 * s * s);
                    let x = c * ax_len;
                    let flipx = if flip { depth - x } else { x };
                    // backswept camber: the blade sheet curves tangentially along its chord
                    let camber = sweep * (c * c) * (0.4 + 0.6 * s);
                    let phi = phi0 + camber + side * thickness / radius;
                    let p = Vec3::new(flipx, radius * phi.cos(), radius * phi.sin());
                    row.push((p, Vec3::new(0.0, -phi.sin(), phi.cos()) * side));
                }
                g.push(row);
            }
            grid.push(g.into_iter().flatten().collect());
        }
        for (si, g) in grid.iter().enumerate() {
            let base = mb.pos.len() as u32;
            for (p, n) in g.iter() {
                mb.vert(*p, *n, [0.0, 0.0]);
            }
            let w = cs + 1;
            for ri in 0..rs {
                for ci in 0..cs {
                    let a = base + (ri * w + ci) as u32;
                    let hint = grid[si][ri * w + ci].1;
                    mb.tri(a, a + 1, a + w as u32 + 1, hint);
                    mb.tri(a, a + w as u32 + 1, a + w as u32, hint);
                }
            }
        }
    }
    mb
}

/// Rotation (about X) helper: Y toward Z positive.
pub fn rot_x(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
