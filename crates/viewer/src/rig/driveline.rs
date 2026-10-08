//! Flywheel, clutch, gearbox (with the real gear ratios), driveshaft, differential, axle, wheels
//! and brakes. Rotations are driven by the sim's input/output shaft and wheel angles, and the
//! gear teeth are phased so meshing gears genuinely mesh.

use super::anim::*;
use super::Ctx;
use crate::cam::Anchor;
use crate::layout::Layout;
use crate::meshgen::*;
use bevy::prelude::*;
use engine_core::{EngineConfig, TransmissionConfig};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Key X stations along the car, computed once so intake/exhaust/driveline agree.
#[derive(Clone, Copy)]
pub struct Dims {
    pub x_rear: f32,
    pub x_gear0: f32,
    pub x_gear_end: f32,
    pub x_t_out: f32,
    pub x_pinion: f32,
    pub x_diff: f32,
    pub x_cat: f32,
    pub x_muffler: f32,
    pub x_end: f32,
    pub y_axle: f32,
    pub half_track: f32,
}

pub fn dims(lay: &Layout, trans: &TransmissionConfig) -> Dims {
    let ng = trans.gears.iter().filter(|g| g.ratio != 0.0).count() as f32;
    let x_rear = lay.x_rear;
    let x_gear0 = x_rear + 0.19;
    let x_gear_end = x_gear0 + 0.16 + 0.034 * ng;
    let x_t_out = x_gear_end + 0.16;
    let x_pinion = x_t_out + 1.05;
    let x_diff = x_pinion + 0.22;
    Dims {
        x_rear,
        x_gear0,
        x_gear_end,
        x_t_out,
        x_pinion,
        x_diff,
        x_cat: x_t_out + 0.62,
        x_muffler: x_diff + 0.60,
        x_end: x_diff + 1.38,
        y_axle: -0.23,
        half_track: 0.80,
    }
}

pub struct DrivelineOut {
    pub anchors: Vec<Anchor>,
}

fn mesh_phase(ratio_a: f32, phase_a: f32, za: f32, zb: f32, delta: f32) -> (f32, f32) {
    (-ratio_a * za / zb, delta + PI + (delta - phase_a) * (za / zb) + PI / zb)
}

fn spin_gear(cx: &mut Ctx, e: Entity, ratio: f32, phase: f32) {
    cx.cmd.entity(e).insert(Spin::x(SpinSrc::Input, ratio, phase));
}

#[allow(clippy::too_many_arguments)]
pub fn build_driveline(
    cx: &mut Ctx,
    root: Entity,
    crank: Entity,
    _cfg: &EngineConfig,
    lay: &Layout,
    trans: &TransmissionConfig,
    d: &Dims,
) -> DrivelineOut {
    let m = cx.mats.clone();
    let b = lay.bore;
    let x_rear = d.x_rear;

    // ------------------------------------------------------------------ flywheel + ring gear
    let fw_r = 0.155f32;
    let flywheel = MB::lathe(
        &[
            (0.0, 0.0, false),
            (0.0, 0.06, false),
            (0.006, 0.066, true),
            (0.014, 0.1, true),
            (0.014, fw_r, false),
            (0.04, fw_r, false),
            (0.04, 0.0, false),
        ],
        64,
    );
    cx.spawn(crank, flywheel, &m.dark_steel, Transform::from_xyz(x_rear + 0.004, 0.0, 0.0));
    let teeth = 122;
    let ring = MB::extrude(&gear_outline(teeth, fw_r - 0.001, fw_r + 0.006), 0.0, 0.016, 0.0, 1);
    cx.spawn(crank, ring, &m.steel, Transform::from_xyz(x_rear + 0.026, 0.0, 0.0));
    // pressure plate, diaphragm spring, cover (crank side)
    let cover = MB::lathe(
        &[
            (0.0, 0.147, false),
            (0.0, 0.158, false),
            (0.07, 0.158, false),
            (0.07, 0.05, true),
            (0.062, 0.05, false),
            (0.062, 0.15, false),
        ],
        56,
    );
    cx.spawn(crank, cover, &m.anodized_red, Transform::from_xyz(x_rear + 0.044, 0.0, 0.0));
    cx.spawn(crank, MB::tube(0.145, 0.08, 0.0, 0.016, 56), &m.dark_steel, Transform::from_xyz(x_rear + 0.058, 0.0, 0.0));
    cx.spawn(crank, MB::cone(0.14, 0.06, 0.0, 0.03, 40), &m.brass, Transform::from_xyz(x_rear + 0.075, 0.0, 0.0));
    // clutch friction disc (input-shaft side), slides open/closed
    let disc_root = cx.node(root, Transform::from_xyz(x_rear + 0.046, 0.0, 0.0));
    cx.cmd.entity(disc_root).insert(ClutchDisc { rest_x: x_rear + 0.046, travel: 0.012 });
    let disc = cx.node(disc_root, Transform::default());
    cx.cmd.entity(disc).insert(Spin::x(SpinSrc::Input, 1.0, 0.0));
    cx.spawn(disc, MB::tube(0.143, 0.085, 0.0, 0.009, 56), &m.carbon, Transform::default());
    cx.spawn(disc, MB::tube(0.1435, 0.138, -0.001, 0.010, 56), &m.copper, Transform::default());
    cx.spawn(disc, MB::tube(0.085, 0.03, 0.0, 0.012, 40), &m.steel, Transform::default());
    for k in 0..6 {
        let a = TAU * k as f32 / 6.0;
        cx.spawn(
            disc,
            MB::cylinder(0.008, -0.004, 0.012, 12),
            &m.brass,
            Transform::from_xyz(0.0, 0.058 * a.cos(), 0.058 * a.sin()),
        );
    }
    // starter motor + pinion
    let st = Vec3::new(x_rear + 0.06, -0.09, -0.185);
    cx.spawn(root, MB::cylinder(0.05, 0.0, 0.2, 32), &m.anodized_blue, Transform::from_translation(st));
    cx.spawn(root, MB::cylinder(0.034, 0.2, 0.26, 28), &m.dark_steel, Transform::from_translation(st));
    cx.spawn(root, MB::cylinder(0.04, -0.045, 0.0, 28), &m.steel, Transform::from_translation(st));

    // ------------------------------------------------------------------ bellhousing + gearbox shell
    let bell = MB::lathe(&[(0.0, 0.225, false), (0.0, 0.232, false), (d.x_gear0 - x_rear, 0.185, true), (d.x_gear0 - x_rear, 0.178, false)], 64);
    cx.spawn(root, bell, &m.ghost_casing, Transform::from_xyz(x_rear + 0.05, 0.0, 0.0));
    let gb_len = d.x_gear_end - d.x_gear0;
    cx.spawn(
        root,
        MB::cuboid(gb_len, 0.31, 0.27),
        &m.ghost_casing,
        Transform::from_xyz(d.x_gear0 + gb_len * 0.5, -0.045, 0.0),
    );
    for k in 0..4 {
        let x = d.x_gear0 + gb_len * (k as f32 / 3.0);
        cx.spawn(root, MB::cuboid(0.01, 0.33, 0.29), &m.head_glass, Transform::from_xyz(x, -0.045, 0.0));
    }

    // ------------------------------------------------------------------ gearbox internals
    let module = 0.0025f32;
    let (z_in, z_ca) = (22.0f32, 36.0f32);
    let center_dist = module * (z_in + z_ca) * 0.5;
    let width = 0.022f32;
    let y_ctr = -center_dist;
    // input shaft (clutch -> constant-mesh pair)
    let in_shaft_x0 = x_rear + 0.04;
    let in_shaft_x1 = d.x_gear0 + 0.1;
    let input_shaft = cx.node(root, Transform::default());
    cx.cmd.entity(input_shaft).insert(Spin::x(SpinSrc::Input, 1.0, 0.0));
    cx.spawn(input_shaft, MB::cylinder(0.014, in_shaft_x0, in_shaft_x1, 20), &m.polished, Transform::default());
    // splines on the input shaft where the clutch disc rides
    cx.spawn(input_shaft, MB::tube(0.017, 0.012, in_shaft_x0 + 0.02, in_shaft_x0 + 0.09, 24), &m.steel, Transform::default());
    // constant-mesh input gear
    let x_in = d.x_gear0 + 0.04;
    let g_in = cx.spawn(root, gear_mesh(z_in as usize, module, width, 0.5, 0.016), &m.steel, Transform::from_xyz(x_in, 0.0, 0.0));
    spin_gear(cx, g_in, 1.0, 0.0);
    let (r_ca, p_ca) = mesh_phase(1.0, 0.0, z_in, z_ca, PI);
    let g_ca = cx.spawn(root, gear_mesh(z_ca as usize, module, width, -0.5, 0.016), &m.steel, Transform::from_xyz(x_in, y_ctr, 0.0));
    spin_gear(cx, g_ca, r_ca, p_ca);

    // countershaft (rotates with the counter gears)
    let cs_x0 = x_in - 0.012;
    let cs_x1 = d.x_gear_end - 0.03;
    let cshaft = cx.node(root, Transform::from_xyz(0.0, y_ctr, 0.0));
    cx.cmd.entity(cshaft).insert(Spin::x(SpinSrc::Input, r_ca, p_ca));
    cx.spawn(cshaft, MB::cylinder(0.012, cs_x0, cs_x1, 18), &m.polished, Transform::default());

    // mainshaft (output speed)
    let out_x1 = d.x_t_out;
    let mshaft = cx.node(root, Transform::default());
    cx.cmd.entity(mshaft).insert(Spin::x(SpinSrc::Output, 1.0, 0.0));
    cx.spawn(mshaft, MB::cylinder(0.0155, d.x_gear0 + 0.07, out_x1, 20), &m.polished, Transform::default());
    // output flange / yoke
    cx.spawn(mshaft, MB::cylinder(0.034, out_x1 - 0.02, out_x1 + 0.006, 26), &m.dark_steel, Transform::default());

    // gear pairs, one per ratio (R, 1, 2, ...)
    let mut k = 0usize;
    for (gi, g) in trans.gears.iter().enumerate() {
        if g.ratio == 0.0 {
            continue;
        }
        let r = g.ratio.abs() as f32;
        let z_m = (58.0 * r / (z_ca / z_in + r)).round().clamp(14.0, 44.0);
        let z_c = 58.0 - z_m;
        let x = d.x_gear0 + 0.1 + k as f32 * 0.034;
        k += 1;
        // counter gear (fixed to the countershaft)
        let gc = cx.spawn(root, gear_mesh(z_c as usize, module, width, 0.5, 0.014), &m.dark_steel, Transform::from_xyz(x, y_ctr, 0.0));
        spin_gear(cx, gc, r_ca, p_ca);
        // main gear (free on the mainshaft unless this gear is selected)
        let (r_m, p_m) = mesh_phase(r_ca, p_ca, z_c, z_m, 0.0);
        let hl = cx.material(StandardMaterial {
            base_color: Color::srgb(0.7, 0.72, 0.76),
            metallic: 1.0,
            perceptual_roughness: 0.3,
            ..default()
        });
        let hl_mat = hl.clone();
        let gm = cx.spawn(root, gear_mesh(z_m as usize, module, width, -0.5, 0.016), &hl_mat, Transform::from_xyz(x, 0.0, 0.0));
        spin_gear(cx, gm, r_m, p_m);
        cx.cmd.entity(gm).insert(GearHighlight { gear_idx: gi, mat: hl });
        // gear label ring (so each gear reads as its own)
        let _ = (gi, &g.label);
    }
    // synchro collars on the mainshaft
    for kk in 0..((k + 1) / 2) {
        let x = d.x_gear0 + 0.1 + kk as f32 * 0.068 + 0.017 + 0.011;
        cx.spawn(mshaft, MB::cylinder(0.028, x - 0.006, x + 0.006, 24), &m.brass, Transform::default());
    }

    let final_drive = trans.final_drive_ratio as f32;
    let z_ring = (final_drive * 11.0).round().max(30.0);
    let ring_mod = 0.0056f32;
    let ring_r = z_ring * ring_mod * 0.5;
    let pinion_r = 0.032f32;
    let pin_c = Vec3::new(d.x_diff - 0.07, d.y_axle + ring_r + pinion_r, 0.0);
    // ------------------------------------------------------------------ extension housing + driveshaft
    cx.spawn(root, MB::tube(0.058, 0.05, d.x_gear_end, d.x_t_out + 0.02, 32), &m.brushed_alu, Transform::default());
    let p0 = Vec3::new(d.x_t_out + 0.01, 0.0, 0.0);
    let p1 = Vec3::new(d.x_pinion - 0.01, pin_c.y, 0.0);
    let dir = (p1 - p0).normalize();
    let len = (p1 - p0).length();
    let q = Quat::from_rotation_arc(Vec3::X, dir);
    let ds_pivot = cx.node(root, Transform::from_translation(p0).with_rotation(q));
    let ds = cx.node(ds_pivot, Transform::default());
    cx.cmd.entity(ds).insert(Spin::x(SpinSrc::Output, 1.0, 0.0));
    cx.spawn(ds, MB::tube(0.041, 0.036, 0.1, len - 0.1, 36), &m.dark_steel, Transform::default());
    // reflective bands so rotation reads
    cx.spawn(ds, MB::tube(0.0415, 0.036, len * 0.35, len * 0.35 + 0.05, 36), &m.floor_mark, Transform::default());
    cx.spawn(ds, MB::tube(0.0415, 0.036, len * 0.65, len * 0.65 + 0.03, 36), &m.anodized_red, Transform::default());
    for end in [0.0f32, len] {
        let sgn = if end == 0.0 { 1.0 } else { -1.0 };
        // U-joint: yoke + cross
        cx.spawn(ds, MB::cylinder(0.04, end, end + sgn * 0.1, 28), &m.steel, Transform::default());
        let cx0 = end + sgn * 0.1;
        cx.spawn(ds, MB::cylinder(0.008, -0.034, 0.034, 12).rot_to_y(), &m.brass, Transform::from_xyz(cx0, 0.0, 0.0));
        cx.spawn(ds, MB::cylinder(0.008, -0.034, 0.034, 12).rot_to_z(), &m.brass, Transform::from_xyz(cx0, 0.0, 0.0));
    }
    // centre bearing carrier
    cx.spawn(root, MB::cuboid(0.07, 0.02, 0.1), &m.rubber, Transform::from_translation(p0 + dir * (len * 0.5) + Vec3::new(0.0, 0.07, 0.0)));

    // ------------------------------------------------------------------ differential
    let diff_c = Vec3::new(d.x_diff, d.y_axle, 0.0);
    // ring gear about Z: mesh built along X and turned onto Z
    let ring_base = Quat::from_rotation_y(-FRAC_PI_2);
    let ring_mesh = cx.mesh(gear_mesh(z_ring as usize, ring_mod, 0.026, 0.0, 0.05));
    let ring = cx.put(root, &ring_mesh, &m.dark_steel, Transform::from_translation(diff_c + Vec3::new(0.0, 0.0, -0.013)).with_rotation(ring_base));
    cx.cmd.entity(ring).insert(Spin { src: SpinSrc::Wheel, axis: Vec3::X, ratio: 1.0, phase: 0.0, base: ring_base });
    // pinion (worm-style) on top, driven by the driveshaft
    let pin_mesh = cx.mesh(gear_mesh(9, 0.0066, 0.15, 1.9, 0.02));
    let pin = cx.put(root, &pin_mesh, &m.steel, Transform::from_translation(pin_c + Vec3::new(0.0, 0.0, 0.0)));
    let _ = pinion_r;
    cx.cmd.entity(pin).insert(Spin::x(SpinSrc::Output, 1.0, 0.0));
    // pinion shaft + flange
    let pin_shaft = cx.node(root, Transform::from_translation(pin_c));
    cx.cmd.entity(pin_shaft).insert(Spin::x(SpinSrc::Output, 1.0, 0.0));
    cx.spawn(pin_shaft, MB::cylinder(0.02, -0.12, 0.0, 22), &m.polished, Transform::default());
    cx.spawn(pin_shaft, MB::cylinder(0.042, -0.14, -0.12, 26), &m.dark_steel, Transform::default());
    // carrier + housing (translucent)
    cx.spawn(root, MB::cuboid(0.34, ring_r * 2.0 + 0.12, 0.17), &m.ghost_casing, Transform::from_translation(diff_c + Vec3::new(0.04, 0.02, 0.0)));
    cx.spawn(root, MB::cuboid(0.2, 0.1, 0.12), &m.ghost_casing, Transform::from_translation(Vec3::new(d.x_pinion - 0.04, pin_c.y, 0.0)));
    // diff cover bolts ring
    cx.spawn(root, MB::tube(ring_r * 0.97 + 0.07, ring_r * 0.97 + 0.02, 0.0, 0.01, 40), &m.dark_steel, Transform::from_translation(diff_c + Vec3::new(0.0, 0.0, 0.088)).with_rotation(ring_base));
    // axle shafts (spin with the wheels) inside housing tubes
    for sz in [-1.0f32, 1.0] {
        let hub_z = sz * d.half_track;
        let axle_base = if sz > 0.0 { Quat::from_rotation_y(-FRAC_PI_2) } else { Quat::from_rotation_y(FRAC_PI_2) };
        let ratio = sz;
        let shaft = cx.node(root, Transform::from_translation(Vec3::new(d.x_diff, d.y_axle, 0.0)).with_rotation(axle_base));
        cx.cmd.entity(shaft).insert(Spin { src: SpinSrc::Wheel, axis: Vec3::X, ratio, phase: 0.0, base: axle_base });
        cx.spawn(shaft, MB::cylinder(0.019, 0.05, d.half_track - 0.07, 20), &m.polished, Transform::default());
        // housing tube (translucent so the axle shows)
        let tube = MB::tube(0.034, 0.026, 0.07, d.half_track - 0.13, 28);
        cx.spawn(root, tube, &m.ghost_casing, Transform::from_translation(Vec3::new(d.x_diff, d.y_axle, 0.0)).with_rotation(axle_base));
        build_wheel(cx, root, Vec3::new(d.x_diff, d.y_axle, hub_z), sz);
    }

    // ------------------------------------------------------------------ anchors
    let anchors = vec![
        Anchor {
            name: "Clutch & gearbox",
            target: Vec3::new((x_rear + d.x_gear_end) * 0.5, -0.03, 0.0),
            dist: 0.95,
            yaw: 0.8,
            pitch: 0.38,
        },
        Anchor {
            name: "Driveshaft & diff",
            target: Vec3::new((d.x_t_out + d.x_diff) * 0.5, -0.15, 0.0),
            dist: 2.3,
            yaw: 0.75,
            pitch: 0.28,
        },
        Anchor {
            name: "Differential",
            target: Vec3::new(d.x_diff - 0.1, d.y_axle + 0.02, 0.0),
            dist: 0.8,
            yaw: 0.6,
            pitch: 0.42,
        },
        Anchor {
            name: "Wheel & brake",
            target: Vec3::new(d.x_diff, d.y_axle, d.half_track),
            dist: 1.05,
            yaw: 1.0,
            pitch: 0.18,
        },
    ];
    let _ = b;
    DrivelineOut { anchors }
}

/// One road wheel: tyre, alloy rim, vented brake rotor (glows when hot), caliper.
fn build_wheel(cx: &mut Ctx, root: Entity, hub: Vec3, side: f32) {
    let m = cx.mats.clone();
    // everything is built about local +X (the axle), then turned onto the Z axle
    let base = if side > 0.0 { Quat::from_rotation_y(-FRAC_PI_2) } else { Quat::from_rotation_y(FRAC_PI_2) };
    let spin_node = cx.node(root, Transform::from_translation(hub).with_rotation(base));
    cx.cmd.entity(spin_node).insert(Spin { src: SpinSrc::Wheel, axis: Vec3::X, ratio: side, phase: 0.0, base });
    // tyre profile (x = across the tread, r = radius)
    let tyre = MB::lathe(
        &[
            (-0.095, 0.222, false),
            (-0.118, 0.25, true),
            (-0.116, 0.292, true),
            (-0.095, 0.324, true),
            (-0.05, 0.331, true),
            (0.05, 0.331, true),
            (0.095, 0.324, true),
            (0.116, 0.292, true),
            (0.118, 0.25, true),
            (0.095, 0.222, false),
        ],
        72,
    );
    cx.spawn(spin_node, tyre, &m.rubber, Transform::default());
    // rim barrel + lip
    cx.spawn(
        spin_node,
        MB::lathe(
            &[
                (-0.11, 0.212, false),
                (-0.11, 0.225, true),
                (-0.09, 0.222, false),
                (0.09, 0.222, false),
                (0.11, 0.225, true),
                (0.11, 0.212, false),
                (0.09, 0.2, false),
                (-0.09, 0.2, false),
                (-0.11, 0.212, false),
            ],
            72,
        ),
        &m.polished,
        Transform::default(),
    );
    // 5 twin-spoke face
    let spoke_pts: Vec<Vec2> = vec![
        Vec2::new(0.035, -0.022),
        Vec2::new(0.205, -0.042),
        Vec2::new(0.205, -0.004),
        Vec2::new(0.052, 0.014),
    ];
    let spoke = cx.mesh(MB::extrude(&spoke_pts, 0.055, 0.082, 0.0, 1));
    let spoke2: Vec<Vec2> = spoke_pts.iter().map(|p| Vec2::new(p.x, p.y + 0.05)).collect();
    let spoke2m = cx.mesh(MB::extrude(&spoke2, 0.055, 0.082, 0.0, 1));
    for i in 0..5 {
        let a = TAU * i as f32 / 5.0;
        let q = Quat::from_rotation_x(a);
        cx.put(spin_node, &spoke, &m.aluminum, Transform::from_rotation(q));
        cx.put(spin_node, &spoke2m, &m.aluminum, Transform::from_rotation(q));
    }
    cx.spawn(spin_node, MB::cylinder(0.065, 0.045, 0.09, 36), &m.aluminum, Transform::default());
    cx.spawn(spin_node, MB::cylinder(0.022, 0.09, 0.098, 20), &m.anodized_red, Transform::default());
    for i in 0..5 {
        let a = TAU * i as f32 / 5.0;
        cx.spawn(spin_node, MB::cylinder(0.006, 0.09, 0.1, 10), &m.polished, Transform::from_xyz(0.0, 0.045 * a.cos(), 0.045 * a.sin()));
    }
    // brake rotor (inboard of the rim)
    let rotor_mat = cx.material(StandardMaterial {
        base_color: Color::srgb(0.38, 0.38, 0.4),
        metallic: 1.0,
        perceptual_roughness: 0.45,
        ..default()
    });
    cx.cmd.entity(spin_node).insert(BrakeGlow { mat: rotor_mat.clone() });
    cx.spawn(spin_node, MB::tube(0.172, 0.095, 0.02, 0.032, 56), &rotor_mat, Transform::default());
    cx.spawn(spin_node, MB::tube(0.172, 0.095, -0.004, 0.008, 56), &rotor_mat, Transform::default());
    for i in 0..24 {
        let a = TAU * i as f32 / 24.0;
        cx.spawn(spin_node, MB::cylinder(0.004, 0.008, 0.02, 6), &rotor_mat, Transform::from_xyz(0.0, 0.135 * a.cos(), 0.135 * a.sin()));
    }
    cx.spawn(spin_node, MB::tube(0.098, 0.04, 0.0, 0.034, 40), &m.dark_steel, Transform::default());
    // caliper (static, bolted to the knuckle) - sits across the rotor's top
    let cal_base = Quat::from_rotation_y(if side > 0.0 { -FRAC_PI_2 } else { FRAC_PI_2 });
    let cal = cx.node(root, Transform::from_translation(hub + Vec3::new(0.0, 0.0, 0.0)).with_rotation(cal_base));
    cx.spawn(cal, MB::cuboid(0.07, 0.09, 0.15), &m.anodized_red, Transform::from_xyz(0.016, 0.157, -0.0));
    for dz in [-0.04f32, 0.04] {
        cx.spawn(cal, MB::cylinder(0.018, -0.001, 0.014, 20).rot_to_z(), &m.polished, Transform::from_xyz(0.016, 0.156, dz));
    }
    cx.spawn(cal, MB::cuboid(0.012, 0.03, 0.05), &m.dark_steel, Transform::from_xyz(0.016, 0.205, 0.0));
}
