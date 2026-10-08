//! Small reusable hardware: bolted flanges, hose clamps, air filters, throttle bodies, sensors.
//! Everything is built along local +X and then turned onto the requested direction.

use super::anim::ThrottlePlate;
use super::Ctx;
use crate::meshgen::*;
use bevy::prelude::*;
use engine_core::AirFilter;
use std::f32::consts::TAU;

/// Rotation taking local +X onto `dir`.
pub fn orient_x(dir: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::X, dir.normalize_or_zero())
}

/// A thin flange disc with a ring of hex bolt heads, axis along X.
pub fn flange_mesh(r: f32, bolts: usize) -> MB {
    let mut mb = MB::cylinder(r, -0.004, 0.004, 28);
    let rb = r * 0.8;
    let head = (r * 0.11).clamp(0.0025, 0.006);
    for k in 0..bolts {
        let a = TAU * (k as f32 + 0.5) / bolts as f32;
        mb.append(
            &MB::cylinder(head, 0.004, 0.0095, 6),
            Mat4::from_translation(Vec3::new(0.0, rb * a.cos(), rb * a.sin())),
        );
    }
    mb
}

/// Places a pre-built flange mesh at `at`, facing `axis`.
pub fn put_flange(cx: &mut Ctx, parent: Entity, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, at: Vec3, axis: Vec3) {
    cx.put(parent, mesh, mat, Transform::from_translation(at).with_rotation(orient_x(axis)));
}

/// Worm-drive hose clamp: a band with a screw housing.
pub fn clamp_ring(cx: &mut Ctx, parent: Entity, mat: &Handle<StandardMaterial>, at: Vec3, axis: Vec3, r: f32) {
    let mut mb = MB::tube(r * 1.12, r * 1.0, -0.005, 0.005, 28);
    mb.append(&MB::cuboid(0.012, 0.012, 0.01), Mat4::from_translation(Vec3::new(0.0, r * 1.14, 0.0)));
    mb.append(
        &MB::cylinder(0.0028, -0.007, 0.007, 8).rot_to_z(),
        Mat4::from_translation(Vec3::new(0.0, r * 1.2, 0.0)),
    );
    cx.spawn(parent, mb, mat, Transform::from_translation(at).with_rotation(orient_x(axis)));
}

/// Air filter on the end of an intake tube. `at` is the pipe end the filter attaches to, `dir`
/// points away from the engine (where the air comes from) and `r` is the pipe radius.
pub fn filter_assembly(cx: &mut Ctx, parent: Entity, kind: AirFilter, at: Vec3, dir: Vec3, r: f32) {
    let m = cx.mats.clone();
    let node = cx.node(parent, Transform::from_translation(at).with_rotation(orient_x(dir)));
    match kind {
        AirFilter::Stock => {
            // moulded hose into a sealed black airbox with a clipped lid and a resonator bulge
            cx.spawn(node, MB::tube(r * 1.08, r * 0.92, 0.0, 0.075, 24), &m.silicone_black, Transform::default());
            for k in 0..3 {
                cx.spawn(node, MB::tube(r * 1.14, r * 1.07, 0.012 + k as f32 * 0.02, 0.019 + k as f32 * 0.02, 24), &m.silicone_black, Transform::default());
            }
            let bx = 0.075 + 0.11;
            cx.spawn(node, MB::cuboid(0.22, 0.12, 0.19), &m.paint_black, Transform::from_xyz(bx, 0.0, 0.0));
            cx.spawn(node, MB::cuboid(0.225, 0.016, 0.195), &m.carbon, Transform::from_xyz(bx, 0.066, 0.0));
            cx.spawn(node, MB::cuboid(0.12, 0.03, 0.1), &m.paint_black, Transform::from_xyz(bx + 0.02, 0.092, 0.0));
            for sz in [-1.0f32, 1.0] {
                for k in 0..3 {
                    cx.spawn(
                        node,
                        MB::cuboid(0.016, 0.03, 0.006),
                        &m.steel,
                        Transform::from_xyz(bx - 0.07 + k as f32 * 0.07, 0.045, sz * 0.0985),
                    );
                }
            }
            // inlet snorkel
            cx.spawn(node, MB::tube(0.034, 0.028, 0.0, 0.07, 22), &m.paint_black, Transform::from_xyz(bx + 0.11, 0.0, 0.0));
        }
        AirFilter::ColdAir => {
            // polished intake tube, silicone coupler, big oiled-cotton cone
            cx.spawn(node, MB::tube(r * 1.06, r * 0.94, 0.0, 0.14, 26), &m.polished, Transform::default());
            cx.spawn(node, MB::tube(r * 1.14, r * 1.03, 0.14, 0.175, 26), &m.silicone_blue, Transform::default());
            clamp_ring(cx, node, &m.steel, Vec3::new(0.148, 0.0, 0.0), Vec3::X, r * 1.03);
            clamp_ring(cx, node, &m.steel, Vec3::new(0.168, 0.0, 0.0), Vec3::X, r * 1.03);
            let (x0, x1) = (0.175, 0.355);
            let (ra, rb) = (r * 1.28, r * 2.55);
            // cone body
            cx.spawn(
                node,
                MB::lathe(&[(x0, 0.0, false), (x0, ra, false), (x1, rb, false), (x1, 0.0, false)], 44),
                &m.filter_red,
                Transform::default(),
            );
            // pleat ribs
            for k in 1..9 {
                let t = k as f32 / 9.0;
                let (x, rr) = (x0 + (x1 - x0) * t, ra + (rb - ra) * t);
                cx.spawn(node, MB::tube(rr * 1.012, rr * 0.99, -0.0025, 0.0025, 44), &m.filter_black, Transform::from_xyz(x, 0.0, 0.0));
            }
            // base plate and end cap
            cx.spawn(node, MB::cylinder(ra * 1.12, x0 - 0.006, x0 + 0.004, 36), &m.anodized_blue, Transform::default());
            cx.spawn(node, MB::cylinder(rb * 1.02, x1 - 0.004, x1 + 0.012, 44), &m.filter_black, Transform::default());
            cx.spawn(node, MB::cylinder(rb * 0.42, x1 + 0.012, x1 + 0.02, 30), &m.chrome, Transform::default());
        }
        AirFilter::OpenStack => {
            // chrome bellmouth
            let bell = MB::lathe(
                &[
                    (0.0, r * 1.12, false),
                    (0.025, r * 1.4, true),
                    (0.06, r * 1.95, true),
                    (0.092, r * 2.35, true),
                    (0.104, r * 2.42, true),
                    (0.11, r * 2.3, true),
                    (0.1, r * 2.12, true),
                    (0.07, r * 1.7, true),
                    (0.035, r * 1.2, true),
                    (0.0, r * 0.98, false),
                ],
                44,
            );
            cx.spawn(node, bell, &m.chrome, Transform::default());
            cx.spawn(node, MB::tube(r * 1.18, r * 1.0, -0.002, 0.026, 26), &m.polished, Transform::default());
            // safety screen
            cx.spawn(node, MB::cylinder(r * 2.1, 0.098, 0.1, 40), &m.glass, Transform::default());
        }
    }
}

/// Throttle body: housing, butterfly plate (animated) and its shaft. Returns the plate entity.
pub fn throttle_body(cx: &mut Ctx, parent: Entity, at: Vec3, dir: Vec3, r: f32, len: f32) -> Entity {
    let m = cx.mats.clone();
    let node = cx.node(parent, Transform::from_translation(at).with_rotation(orient_x(dir)));
    cx.spawn(node, MB::cylinder(r, 0.0, len, 28), &m.glass, Transform::default());
    cx.spawn(node, MB::tube(r * 1.2, r * 0.97, -0.004, 0.0, 28), &m.dark_steel, Transform::default());
    cx.spawn(node, MB::tube(r * 1.2, r * 0.97, len, len + 0.004, 28), &m.dark_steel, Transform::default());
    // cast housing around the bore
    cx.spawn(node, MB::tube(r * 1.12, r * 0.97, 0.0, len, 28), &m.brushed_alu, Transform::default());
    let pivot = Vec3::new(len * 0.5, 0.0, 0.0);
    let plate = cx.mesh(MB::cylinder(r * 0.985, -0.0012, 0.0012, 28));
    let pl = cx.put(node, &plate, &m.polished, Transform::from_translation(pivot));
    cx.cmd.entity(pl).insert(ThrottlePlate);
    cx.spawn(node, MB::cylinder(r * 0.1, -r * 1.35, r * 1.35, 10).rot_to_z(), &m.steel, Transform::from_translation(pivot));
    // throttle position sensor on the end of the shaft
    cx.spawn(node, MB::cuboid(r * 0.5, r * 0.45, r * 0.4), &m.paint_black, Transform::from_translation(pivot + Vec3::new(0.0, 0.0, r * 1.5)));
    pl
}

/// Blow-off valve: a stubby anodised canister with a flared vent.
pub fn bov(cx: &mut Ctx, parent: Entity, at: Vec3, up: Vec3) {
    let m = cx.mats.clone();
    let node = cx.node(parent, Transform::from_translation(at).with_rotation(orient_x(up)));
    cx.spawn(node, MB::cylinder(0.024, 0.0, 0.05, 24), &m.anodized_purple, Transform::default());
    cx.spawn(node, MB::cylinder(0.019, 0.05, 0.085, 24), &m.polished, Transform::default());
    cx.spawn(node, MB::tube(0.03, 0.02, 0.085, 0.1, 24), &m.anodized_purple, Transform::default());
    cx.spawn(node, MB::cylinder(0.014, 0.0, -0.03, 16), &m.steel, Transform::default());
}

/// Wideband / narrowband O2 sensor screwed into a pipe, with a short braided pigtail.
pub fn o2_sensor(cx: &mut Ctx, parent: Entity, at: Vec3, up: Vec3) {
    let m = cx.mats.clone();
    let node = cx.node(parent, Transform::from_translation(at).with_rotation(orient_x(up)));
    cx.spawn(node, MB::cylinder(0.0085, 0.0, 0.012, 6), &m.steel, Transform::default());
    cx.spawn(node, MB::cylinder(0.0065, 0.012, 0.05, 16), &m.stainless, Transform::default());
    cx.spawn(node, MB::cylinder(0.0045, 0.05, 0.062, 12), &m.paint_black, Transform::default());
}
