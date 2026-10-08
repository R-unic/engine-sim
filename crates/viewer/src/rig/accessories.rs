//! Everything bolted around the engine that is not the engine itself: the serpentine belt drive
//! (alternator, A/C compressor, water pump, tensioner - all turning with the crank), the radiator
//! with its shrouded fan and coolant hoses, and the chassis rails the powertrain hangs from.

use super::anim::*;
use super::driveline::Dims;
use super::parts::*;
use super::{spline, Ctx};
use crate::fx::PolyPath;
use crate::layout::Layout;
use crate::meshgen::*;
use bevy::prelude::*;
use engine_core::EngineConfig;
use std::f32::consts::TAU;
use std::sync::Arc;

/// A paint mark riding the serpentine belt, so its travel reads at a glance.
#[derive(Component)]
pub struct BeltMark {
    pub path: Arc<PolyPath>,
    pub s0: f32,
    /// Belt speed per radian of crank rotation (the crank pulley radius).
    pub radius: f32,
}

pub fn belt_mark_system(sim: Res<crate::sim::Sim>, mut q: Query<(&BeltMark, &mut Transform)>) {
    let alpha = sim.alpha_v.rem_euclid(720.0 * 12.0) as f32 * std::f32::consts::PI / 180.0;
    for (mk, mut t) in &mut q {
        let len = mk.path.len().max(1e-3);
        let s = (mk.s0 + mk.radius * alpha).rem_euclid(len);
        let p = mk.path.at(s);
        let ahead = mk.path.at((s + 0.01).rem_euclid(len));
        let dir = (ahead - p).normalize_or_zero();
        t.translation = p;
        if dir != Vec3::ZERO {
            // box is long in local Z
            t.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
        }
    }
}

/// Beam between two points (square section `w` x `h`).
fn beam(cx: &mut Ctx, parent: Entity, a: Vec3, b: Vec3, w: f32, h: f32, mat: &Handle<StandardMaterial>) {
    let len = a.distance(b);
    if len < 1e-4 {
        return;
    }
    let dir = (b - a) / len;
    let q = Quat::from_rotation_arc(Vec3::X, dir);
    cx.spawn(parent, MB::cuboid(len, h, w), mat, Transform::from_translation((a + b) * 0.5).with_rotation(q));
}

struct Pulley {
    y: f32,
    z: f32,
    r: f32,
    kind: PulleyKind,
}

#[derive(Clone, Copy, PartialEq)]
enum PulleyKind {
    Alternator,
    Compressor,
    WaterPump,
    Tensioner,
}

pub fn build_belt_drive(cx: &mut Ctx, root: Entity, _cfg: &EngineConfig, lay: &Layout) {
    let m = cx.mats.clone();
    let b = lay.bore;
    let u = (b / 0.09).clamp(0.8, 1.25);
    let x_b = lay.x_front - 0.039; // belt plane (the damper pulley's groove)
    let r_crank = 0.5 * b;
    let ps = [
        Pulley { y: 0.045 * u, z: 0.19 * u, r: 0.028 * u, kind: PulleyKind::Tensioner },
        Pulley { y: 0.115 * u, z: 0.13 * u, r: 0.032 * u, kind: PulleyKind::Alternator },
        Pulley { y: 0.14 * u, z: -0.07 * u, r: 0.04 * u, kind: PulleyKind::WaterPump },
        Pulley { y: 0.02 * u, z: -0.17 * u, r: 0.052 * u, kind: PulleyKind::Compressor },
    ];
    let belt_t = 0.0035;
    let plate_x = lay.x_front - 0.006;

    // ---- pulleys and the accessories behind them
    for p in &ps {
        let at = Vec3::new(x_b, p.y, p.z);
        let spin = cx.node(root, Transform::from_translation(at));
        cx.cmd.entity(spin).insert(Spin::x(SpinSrc::Crank, r_crank / p.r, 0.0));
        // ribbed rim between two flanges
        let rim = MB::lathe(
            &[
                (-0.013, 0.0, false),
                (-0.013, p.r * 1.0, false),
                (-0.011, p.r + 0.003, true),
                (-0.008, p.r + 0.003, true),
                (-0.007, p.r, false),
                (0.007, p.r, false),
                (0.008, p.r + 0.003, true),
                (0.011, p.r + 0.003, true),
                (0.013, p.r, false),
                (0.013, 0.0, false),
            ],
            36,
        );
        let mat = match p.kind {
            PulleyKind::Alternator => &m.dark_steel,
            PulleyKind::Compressor => &m.steel,
            PulleyKind::WaterPump => &m.dark_steel,
            PulleyKind::Tensioner => &m.polished,
        };
        cx.spawn(spin, rim, mat, Transform::default());
        // bolt heads so the rotation is visible
        let bolt = cx.mesh(MB::cylinder(0.0032, -0.016, -0.011, 6));
        for k in 0..5 {
            let a = TAU * k as f32 / 5.0;
            cx.put(spin, &bolt, &m.polished, Transform::from_xyz(0.0, p.r * 0.5 * a.cos(), p.r * 0.5 * a.sin()));
        }
        cx.spawn(spin, MB::cylinder(p.r * 0.22, -0.018, -0.011, 12), &m.chrome, Transform::default());
        // axle back to the mounting plate
        cx.spawn(root, MB::cylinder(0.007, 0.0, plate_x - x_b, 10), &m.steel, Transform::from_translation(at));
        // accessory body in front of the pulley, hanging off the plate
        match p.kind {
            PulleyKind::Alternator => {
                let r = 0.048 * u;
                cx.spawn(root, MB::cylinder(r, -0.1, -0.016, 32), &m.brushed_alu, Transform::from_translation(at));
                cx.spawn(root, MB::cylinder(r * 1.02, -0.108, -0.1, 32), &m.paint_black, Transform::from_translation(at));
                for k in 0..10 {
                    let a = TAU * k as f32 / 10.0;
                    cx.spawn(root, MB::cuboid(0.07, 0.006, 0.012), &m.dark_steel, Transform::from_xyz(x_b - 0.058, p.y + r * 0.99 * a.cos(), p.z + r * 0.99 * a.sin()).with_rotation(Quat::from_rotation_x(-a)));
                }
                cx.spawn(root, MB::cuboid(0.03, 0.03, 0.026), &m.paint_black, Transform::from_xyz(x_b - 0.06, p.y + r * 1.1, p.z));
                cx.spawn(root, MB::cylinder(0.008, 0.0, 0.03, 10).rot_to_y(), &m.copper, Transform::from_xyz(x_b - 0.06, p.y + r * 1.1 + 0.015, p.z));
            }
            PulleyKind::Compressor => {
                let r = 0.056 * u;
                cx.spawn(root, MB::cylinder(r, -0.12, -0.02, 30), &m.anodized_blue, Transform::from_translation(at));
                cx.spawn(root, MB::tube(r * 1.04, r * 0.98, -0.07, -0.062, 30), &m.polished, Transform::from_translation(at));
                cx.spawn(root, MB::cuboid(0.05, 0.034, 0.05), &m.dark_steel, Transform::from_xyz(x_b - 0.1, p.y + 0.03, p.z - r * 0.7));
            }
            PulleyKind::WaterPump => {
                cx.spawn(root, MB::cylinder(0.05 * u, -0.05, -0.016, 28), &m.cast_iron, Transform::from_translation(at));
                cx.spawn(root, MB::tube(0.056 * u, 0.044 * u, -0.036, -0.026, 28), &m.dark_steel, Transform::from_translation(at));
                cx.spawn(root, MB::cylinder(0.02, -0.07, -0.05, 14).rot_to_z(), &m.cast_iron, Transform::from_translation(at + Vec3::new(0.0, 0.0, 0.0)));
            }
            PulleyKind::Tensioner => {
                // spring-loaded arm
                cx.spawn(root, MB::cylinder(0.02, -0.03, -0.016, 18), &m.anodized_red, Transform::from_translation(at));
                cx.spawn(root, MB::cuboid(0.022, 0.02, 0.07), &m.dark_steel, Transform::from_xyz(x_b - 0.024, p.y - 0.03, p.z + 0.03).with_rotation(Quat::from_rotation_x(0.5)));
            }
        }
    }

    // ---- mounting plate: hull of the accessory centres, kept out of the damper's way
    let mut plate_pts: Vec<Vec2> = Vec::new();
    for p in &ps {
        plate_pts.extend(MB::circle_pts(Vec2::new(p.y, p.z), p.r + 0.012, 20));
    }
    let hull = MB::convex_hull(&plate_pts);
    cx.spawn(root, MB::extrude(&hull, plate_x - 0.004, plate_x + 0.004, 0.0, 1), &m.frame_steel, Transform::default());

    // ---- the belt: a taut hull around crank + every pulley
    // hull points live in (z, y) space
    let mut pts: Vec<Vec2> = MB::circle_pts(Vec2::ZERO, r_crank + belt_t * 0.5, 32);
    for p in &ps {
        pts.extend(MB::circle_pts(Vec2::new(p.z, p.y), p.r + 0.003 + belt_t * 0.5, 24));
    }
    let hull = MB::convex_hull(&pts);
    let n = hull.len();
    let mut belt = MB::new();
    let mut loop_pts: Vec<Vec3> = Vec::with_capacity(n + 1);
    for i in 0..n {
        let a = hull[i];
        let c = hull[(i + 1) % n];
        let (za, ya, zc, yc) = (a.x, a.y, c.x, c.y);
        let d = Vec2::new(zc - za, yc - ya);
        let len = d.length();
        loop_pts.push(Vec3::new(x_b, ya, za));
        if len < 1e-5 {
            continue;
        }
        let ang = (-d.y).atan2(d.x);
        let mid = Vec3::new(0.0, (ya + yc) * 0.5, (za + zc) * 0.5);
        belt.append(
            &MB::cuboid(0.024, belt_t, len * 1.08),
            Mat4::from_translation(mid) * Mat4::from_quat(Quat::from_rotation_x(ang)),
        );
    }
    loop_pts.push(loop_pts[0]);
    cx.spawn(root, belt, &m.rubber, Transform::from_xyz(x_b, 0.0, 0.0));
    // rib stripes (slightly proud) so it reads as a poly-V belt
    let mut ribs = MB::new();
    for i in 0..n {
        let a = hull[i];
        let c = hull[(i + 1) % n];
        let d = Vec2::new(c.x - a.x, c.y - a.y);
        let len = d.length();
        if len < 1e-5 {
            continue;
        }
        let ang = (-d.y).atan2(d.x);
        let mid = Vec3::new(0.0, (a.y + c.y) * 0.5, (a.x + c.x) * 0.5);
        for dx in [-0.007f32, 0.0, 0.007] {
            ribs.append(
                &MB::cuboid(0.002, belt_t * 1.5, len * 1.04),
                Mat4::from_translation(mid + Vec3::new(dx, 0.0, 0.0)) * Mat4::from_quat(Quat::from_rotation_x(ang)),
            );
        }
    }
    cx.spawn(root, ribs, &m.filter_black, Transform::from_xyz(x_b, 0.0, 0.0));
    let path = Arc::new(PolyPath::new(loop_pts));
    let mark = cx.mesh(MB::cuboid(0.012, 0.0062, 0.012));
    let paint = cx.material(StandardMaterial {
        base_color: Color::srgb(0.95, 0.95, 0.9),
        emissive: LinearRgba::rgb(0.6, 0.6, 0.5),
        perceptual_roughness: 0.6,
        ..default()
    });
    let plen = path.len();
    for k in 0..6 {
        let e = cx.put(root, &mark, &paint, Transform::from_xyz(x_b, 0.0, 0.0));
        cx.cmd.entity(e).insert(BeltMark { path: path.clone(), s0: plen * k as f32 / 6.0, radius: r_crank + belt_t });
    }
}

/// Radiator core with tanks, a shrouded electric fan and the two coolant hoses.
pub fn build_cooling(cx: &mut Ctx, root: Entity, _cfg: &EngineConfig, lay: &Layout, rad_x: f32, neck: Vec3) {
    let m = cx.mats.clone();
    let (rh, rw, rt) = (0.40f32, 0.52f32, 0.05f32);
    let yc = -0.07f32;
    let c = Vec3::new(rad_x, yc, 0.0);
    // core: dark block with a merged mesh of fins
    cx.spawn(root, MB::cuboid(rt, rh, rw), &m.radiator_core, Transform::from_translation(c));
    let mut fins = MB::new();
    let nf = (rh / 0.0075) as i32;
    for k in 0..nf {
        fins.append(&MB::cuboid(rt + 0.004, 0.0024, rw - 0.01), Mat4::from_translation(Vec3::new(0.0, -rh * 0.5 + 0.01 + k as f32 * 0.0075, 0.0)));
    }
    cx.spawn(root, fins, &m.aluminum, Transform::from_translation(c));
    // plastic side tanks
    for sz in [-1.0f32, 1.0] {
        cx.spawn(root, MB::cuboid(rt + 0.02, rh + 0.02, 0.035), &m.paint_black, Transform::from_translation(c + Vec3::new(0.0, 0.0, sz * (rw * 0.5 + 0.017))));
    }
    // top/bottom rails + filler cap
    for sy in [-1.0f32, 1.0] {
        cx.spawn(root, MB::cuboid(rt + 0.016, 0.018, rw + 0.06), &m.dark_steel, Transform::from_translation(c + Vec3::new(0.0, sy * (rh * 0.5 + 0.009), 0.0)));
    }
    cx.spawn(root, MB::cylinder(0.022, 0.0, 0.03, 20).rot_to_y(), &m.brass, Transform::from_translation(c + Vec3::new(0.0, rh * 0.5 + 0.018, 0.17)));
    cx.spawn(root, MB::cylinder(0.026, 0.026, 0.034, 20).rot_to_y(), &m.polished, Transform::from_translation(c + Vec3::new(0.0, rh * 0.5 + 0.018, 0.17)));
    // shroud: a rectangular duct on the engine side of the core
    let sd = 0.065f32;
    for (dz, dy, sz_, sy_) in [(0.0, rh * 0.5, rw, 0.006), (0.0, -rh * 0.5, rw, 0.006)] {
        cx.spawn(root, MB::cuboid(sd, sy_, sz_), &m.paint_black, Transform::from_translation(c + Vec3::new(sd * 0.5 + rt * 0.5, dy, dz)));
    }
    for sz in [-1.0f32, 1.0] {
        cx.spawn(root, MB::cuboid(sd, rh, 0.006), &m.paint_black, Transform::from_translation(c + Vec3::new(sd * 0.5 + rt * 0.5, 0.0, sz * rw * 0.5)));
    }
    // fan: 7 pitched blades on a hub, spinning
    let fan_c = c + Vec3::new(rt * 0.5 + sd * 0.55, 0.0, 0.0);
    let fan = cx.node(root, Transform::from_translation(fan_c));
    cx.cmd.entity(fan).insert(Spin::x(SpinSrc::Crank, 1.6, 0.0));
    let blade_poly: Vec<Vec2> = vec![Vec2::new(0.035, -0.012), Vec2::new(0.19, -0.032), Vec2::new(0.205, 0.022), Vec2::new(0.04, 0.03)];
    let blade = cx.mesh(MB::extrude(&blade_poly, -0.006, 0.006, 0.55, 3));
    for k in 0..7 {
        cx.put(fan, &blade, &m.paint_black, Transform::from_rotation(Quat::from_rotation_x(TAU * k as f32 / 7.0)));
    }
    cx.spawn(fan, MB::cylinder(0.04, -0.012, 0.016, 26), &m.paint_black, Transform::default());
    cx.spawn(fan, MB::cylinder(0.014, 0.016, 0.022, 16), &m.chrome, Transform::default());
    cx.spawn(root, MB::cylinder(0.034, 0.0, 0.04, 22), &m.dark_steel, Transform::from_translation(fan_c + Vec3::new(0.02, 0.0, 0.0)));
    // fan motor spider
    for k in 0..2 {
        let a = TAU * k as f32 / 4.0 + 0.4;
        cx.spawn(root, MB::cuboid(0.006, 0.006, 0.4), &m.paint_black, Transform::from_translation(fan_c + Vec3::new(0.034, 0.0, 0.0)).with_rotation(Quat::from_rotation_x(a)));
    }
    // hoses (orange silicone) with clamps
    let top = c + Vec3::new(0.0, rh * 0.5 + 0.02, -0.14);
    let bot = c + Vec3::new(0.0, -rh * 0.5 - 0.02, 0.15);
    let up = [top, top + Vec3::new(0.08, 0.06, 0.0), Vec3::new(neck.x - 0.1, neck.y + 0.03, neck.z * 0.5 - 0.02), neck];
    let sm = spline(&up, 6);
    cx.spawn(root, MB::sweep_tube(&sm, &[0.019], 18, true), &m.hose_orange, Transform::default());
    clamp_ring(cx, root, &m.steel, top + Vec3::new(0.0, -0.005, 0.0), Vec3::Y, 0.019);
    clamp_ring(cx, root, &m.steel, neck + Vec3::new(-0.01, 0.0, 0.0), Vec3::X, 0.019);
    cx.spawn(root, MB::cylinder(0.019, -0.012, 0.012, 16).rot_to_y(), &m.dark_steel, Transform::from_translation(top + Vec3::new(0.0, -0.01, 0.0)));
    let u = (lay.bore / 0.09).clamp(0.8, 1.25);
    let wp = Vec3::new(lay.x_front - 0.039 - 0.05, 0.14 * u, -0.07 * u);
    let low = [bot, bot + Vec3::new(0.08, -0.03, 0.0), Vec3::new(wp.x - 0.06, wp.y - 0.12, wp.z + 0.1), wp + Vec3::new(0.0, 0.0, 0.02)];
    let sm = spline(&low, 6);
    cx.spawn(root, MB::sweep_tube(&sm, &[0.0175], 18, true), &m.hose_orange, Transform::default());
    clamp_ring(cx, root, &m.steel, bot + Vec3::new(0.0, 0.005, 0.0), Vec3::Y, 0.0175);
    // coolant overflow bottle
    cx.spawn(root, MB::cylinder(0.04, 0.0, 0.1, 22).rot_to_y(), &m.glass, Transform::from_translation(c + Vec3::new(0.05, rh * 0.5 + 0.03, 0.2)));
    cx.spawn(root, MB::cylinder(0.04, 0.1, 0.114, 22).rot_to_y(), &m.anodized_blue, Transform::from_translation(c + Vec3::new(0.05, rh * 0.5 + 0.03, 0.2)));
}

/// Ladder frame stub the powertrain hangs from: two rails, cross members, engine/gearbox mounts.
pub fn build_chassis(cx: &mut Ctx, root: Entity, lay: &Layout, d: &Dims, front_x: f32) {
    let m = cx.mats.clone();
    let rail_z = 0.50f32;
    let rail_y = -0.215f32;
    let x0 = front_x - 0.1;
    let x1 = d.x_pinion;
    let (rw, rh) = (0.05f32, 0.07f32);
    for sz in [-1.0f32, 1.0] {
        // box rail with a lighter lower flange line and drain slots
        cx.spawn(root, MB::cuboid(x1 - x0, rh, rw), &m.frame_steel, Transform::from_xyz((x0 + x1) * 0.5, rail_y, sz * rail_z));
        cx.spawn(root, MB::cuboid(x1 - x0 + 0.004, 0.006, rw + 0.01), &m.dark_steel, Transform::from_xyz((x0 + x1) * 0.5, rail_y - rh * 0.5, sz * rail_z));
        let slot = cx.mesh(MB::cuboid(0.07, 0.022, rw + 0.004));
        let mut x = x0 + 0.25;
        while x < x1 - 0.1 {
            cx.put(root, &slot, &m.paint_black, Transform::from_xyz(x, rail_y, sz * rail_z));
            x += 0.3;
        }
        // rail end caps
        cx.spawn(root, MB::cuboid(0.02, rh + 0.01, rw + 0.01), &m.anodized_red, Transform::from_xyz(x0, rail_y, sz * rail_z));
    }
    // cross members: front, behind the gearbox, and just ahead of the pinion
    for (x, tall) in [(x0 + 0.02, 0.06f32), (d.x_gear_end + 0.06, 0.06), (d.x_pinion - 0.08, 0.06)] {
        cx.spawn(root, MB::cuboid(0.05, tall, rail_z * 2.0), &m.frame_steel, Transform::from_xyz(x, rail_y, 0.0));
    }
    // engine mounts: outriggers from the oil pan out to the rails, with a rubber puck each
    let pan_half = lay.bore * 1.45 * 0.5;
    for x in [lay.x_front + 0.12, lay.x_rear - 0.1] {
        for sz in [-1.0f32, 1.0] {
            let a = Vec3::new(x, -0.13, sz * (pan_half + 0.03));
            let bz = Vec3::new(x, -0.13, sz * (rail_z - 0.03));
            beam(cx, root, a, bz, 0.05, 0.02, &m.frame_steel);
            cx.spawn(root, MB::cuboid(0.05, 0.1, 0.03), &m.frame_steel, Transform::from_translation(Vec3::new(x, rail_y + 0.05, sz * (rail_z - 0.02))));
            cx.spawn(root, MB::cylinder(0.03, 0.0, 0.045, 22).rot_to_y(), &m.rubber, Transform::from_translation(a + Vec3::new(0.0, 0.01, 0.0)));
            cx.spawn(root, MB::cylinder(0.012, 0.045, 0.06, 12).rot_to_y(), &m.polished, Transform::from_translation(a + Vec3::new(0.0, 0.01, 0.0)));
            cx.spawn(root, MB::cuboid(0.045, 0.012, 0.05), &m.frame_steel, Transform::from_translation(Vec3::new(x, -0.075, sz * (pan_half + 0.03))));
        }
    }
    // gearbox mount: rubber isolator on the rear cross member
    let xg = d.x_gear_end + 0.06;
    cx.spawn(root, MB::cylinder(0.04, 0.0, 0.05, 24).rot_to_y(), &m.rubber, Transform::from_translation(Vec3::new(xg, rail_y + 0.03, 0.0)));
    cx.spawn(root, MB::cuboid(0.06, 0.02, 0.1), &m.frame_steel, Transform::from_translation(Vec3::new(xg, rail_y + 0.09, 0.0)));
}
