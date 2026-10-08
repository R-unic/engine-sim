//! Builds the engine proper: crankshaft, pistons, rods, sleeves, valvetrain, cams, ignition.

use super::anim::*;
use super::Ctx;
use crate::layout::Layout;
use crate::meshgen::*;
use bevy::prelude::*;
use engine_core::{EngineConfig, IgnitionType};
use std::f32::consts::{FRAC_PI_2, PI};

#[allow(dead_code)]
pub struct EngineBuild {
    pub geom: RigGeom,
    pub bank_nodes: Vec<Entity>,
    pub crank: Entity,
    pub x_mid: f32,
    pub r_main: f32,
    pub r_pin: f32,
}

/// Engine-frame position of a bank-local point (bank frame = engine frame rotated by phi about X).
pub fn bank_to_engine(phi: f32, p: Vec3) -> Vec3 {
    Quat::from_rotation_x(phi) * p
}

/// Port opening (bank-local) for cylinder `cg`.
pub fn port_local(lay: &Layout, ci: usize, intake: bool) -> Vec3 {
    let cg = &lay.cyls[ci];
    let side = if intake { -cg.sigma } else { cg.sigma };
    Vec3::new(cg.x, lay.deck + 0.032, side * (lay.bore * 0.5 + 0.012))
}

pub fn build_engine(cx: &mut Ctx, root: Entity, cfg: &EngineConfig, lay: &Layout) -> EngineBuild {
    let m = cx.mats.clone();
    let b = lay.bore;
    let r = lay.r;
    let r_main = 0.31 * b;
    let r_pin = (0.25 * b).min(r * 0.62);
    let web_t = 0.17 * b;
    let n = lay.n();
    let is_diesel = cfg.ignition_type == IgnitionType::Compression;

    // ------------------------------------------------------------------ crankshaft
    let crank = cx.node(root, Transform::default());
    cx.cmd.entity(crank).insert(Spin::x(SpinSrc::Crank, 1.0, 0.0));

    let mut boundaries: Vec<(f32, f32)> = Vec::new(); // (x_left_outer, x_right_outer) per throw
    for t in &lay.throws {
        let m_cnt = t.cyls.len() as f32;
        let half = m_cnt * lay.rod_w * 0.5 + 0.004;
        let (xl, xr) = (t.x - half, t.x + half);
        boundaries.push((xl - web_t, xr + web_t));
        // webs (pin at local +Y, then rotated into place about X)
        let web = cx.mesh(crank_web(r, r_main, r_pin, r * 1.62 + 0.012, 0.95, 0.0, web_t));
        for x0 in [xl - web_t, xr] {
            cx.put(crank, &web, &m.dark_steel, Transform::from_translation(Vec3::new(x0, 0.0, 0.0)).with_rotation(rot_x(t.beta0)));
        }
        // rod journal
        let pin = cx.mesh(MB::cylinder(r_pin, xl - 0.001, xr + 0.001, 32).translated(Vec3::new(0.0, r, 0.0)));
        cx.put(crank, &pin, &m.polished, Transform::from_rotation(rot_x(t.beta0)));
    }
    // main journals between/around throws
    let mut edges: Vec<f32> = vec![lay.x_front - 0.02];
    for (l, rr) in &boundaries {
        edges.push(*l);
        edges.push(*rr);
    }
    edges.push(lay.x_rear + 0.02);
    let journal_mesh_cache = |cx: &mut Ctx, x0: f32, x1: f32| cx.mesh(MB::cylinder(r_main, x0, x1, 28));
    let mut mains: Vec<f32> = Vec::new();
    for i in (0..edges.len()).step_by(2) {
        let (x0, x1) = (edges[i], edges[i + 1]);
        let h = journal_mesh_cache(cx, x0 - 0.0005, x1 + 0.0005);
        cx.put(crank, &h, &m.polished, Transform::default());
        mains.push((x0 + x1) * 0.5);
    }
    // front snout + damper pulley
    let pulley = MB::lathe(
        &[
            (0.0, 0.0, false),
            (0.0, b * 0.52, false),
            (0.012, b * 0.52, false),
            (0.012, b * 0.5, false),
            (0.030, b * 0.5, false),
            (0.030, b * 0.46, false),
            (0.036, b * 0.46, false),
            (0.036, 0.0, false),
        ],
        48,
    )
    .translated(Vec3::new(lay.x_front - 0.060, 0.0, 0.0));
    cx.spawn(crank, pulley, &m.anodized_blue, Transform::default());
    cx.spawn(
        crank,
        MB::tube(b * 0.515, b * 0.45, lay.x_front - 0.048, lay.x_front - 0.03, 48),
        &m.rubber,
        Transform::default(),
    );
    cx.spawn(crank, MB::cylinder(r_main * 0.7, lay.x_front - 0.07, lay.x_front - 0.02, 20), &m.steel, Transform::default());
    // timing sprocket for the cams, on the crank
    cx.spawn(
        crank,
        gear_mesh(19, 0.0036, 0.012, 0.0, 0.01).translated(Vec3::new(lay.x_front - 0.016, 0.0, 0.0)),
        &m.dark_steel,
        Transform::default(),
    );

    // ------------------------------------------------------------------ static casing (ghost)
    let x_last = lay.throws.last().map(|t| t.x).unwrap_or(0.0);
    let x_mid = (lay.x_front + lay.x_rear) * 0.5;
    let len = lay.x_rear - lay.x_front;
    // oil pan + crankcase
    cx.spawn(
        root,
        MB::cuboid(len, 0.16, b * 1.45),
        &m.ghost_casing,
        Transform::from_xyz(x_mid, -0.05, 0.0),
    );
    // main bearing caps
    let cap = cx.mesh(MB::cuboid(0.02, 0.028, r_main * 3.0));
    for x in &mains {
        cx.put(root, &cap, &m.cast_iron, Transform::from_xyz(*x, -(r_main + 0.016), 0.0));
    }

    // oil filter on the pan, drain plug, and (inline engines) a dipstick tube with a yellow loop handle
    let pan_z = b * 1.45 * 0.5;
    let x_filter = lay.x_front + 0.13;
    cx.spawn(root, MB::cylinder(0.011, 0.0, 0.03, 14).rot_to_z(), &m.dark_steel, Transform::from_xyz(x_filter, -0.075, pan_z));
    cx.spawn(root, MB::cylinder(0.04, 0.03, 0.135, 28).rot_to_z(), &m.anodized_blue, Transform::from_xyz(x_filter, -0.075, pan_z));
    cx.spawn(root, MB::tube(0.0405, 0.036, 0.0, 0.006, 28).rot_to_z(), &m.polished, Transform::from_xyz(x_filter, -0.075, pan_z + 0.06));
    cx.spawn(root, MB::cylinder(0.034, 0.135, 0.145, 6).rot_to_z(), &m.steel, Transform::from_xyz(x_filter, -0.075, pan_z));
    cx.spawn(root, MB::cylinder(0.012, 0.0, 0.016, 6).rot_to_y(), &m.steel, Transform::from_xyz(x_mid, -0.05 - 0.08, 0.0));
    if lay.tilts.len() == 1 {
        let zs = -pan_z - 0.018;
        let x_ds = lay.x_rear - 0.12;
        let path = [
            Vec3::new(x_ds, -0.05, -pan_z + 0.002),
            Vec3::new(x_ds, 0.03, zs),
            Vec3::new(x_ds, 0.12, zs - 0.012),
            Vec3::new(x_ds, 0.2, zs - 0.016),
        ];
        cx.spawn(root, MB::sweep_tube(&super::spline(&path, 5), &[0.0055], 10, true), &m.stainless, Transform::default());
        cx.spawn(root, MB::tube(0.012, 0.0085, 0.0, 0.012, 16).rot_to_z(), &m.hose_orange, Transform::from_xyz(x_ds, 0.213, zs - 0.016));
    }

    // ------------------------------------------------------------------ per-bank nodes
    let mut bank_nodes = Vec::new();
    for (bi, phi) in lay.tilts.iter().enumerate() {
        let node = cx.node(root, Transform::from_rotation(rot_x(*phi)));
        bank_nodes.push(node);
        let cyls_in_bank: Vec<usize> = (0..n).filter(|&i| lay.cyls[i].bank == bi).collect();
        let x0 = cyls_in_bank.iter().map(|&i| lay.cyls[i].x).fold(f32::MAX, f32::min) - lay.pitch * 0.55;
        let x1 = cyls_in_bank.iter().map(|&i| lay.cyls[i].x).fold(f32::MIN, f32::max) + lay.pitch * 0.55;
        let xm = (x0 + x1) * 0.5;
        // block deck (ghost) and head (ghost)
        cx.spawn(
            node,
            MB::cuboid(x1 - x0, lay.deck - 0.07, b * 1.3),
            &m.ghost_casing,
            Transform::from_xyz(xm, 0.07 + (lay.deck - 0.07) * 0.5, 0.0),
        );
        cx.spawn(
            node,
            MB::cuboid(x1 - x0, 0.045, b * 1.34),
            &m.head_glass,
            Transform::from_xyz(xm, lay.deck - 0.0225, 0.0),
        );
    }

    // ------------------------------------------------------------------ valvetrain constants
    let valve_lift = 0.115 * b;
    let stem_len = 0.074f32;
    let spring_len = stem_len - 0.032;
    let cam_base_r = 0.011f32;
    let seat_y = lay.deck + 0.001;
    let bridge_rest_y = seat_y + stem_len + 0.0045;
    let cam_y = bridge_rest_y + 0.004 + cam_base_r;
    // the same per-engine cam the sound model flows gas through, so what you see open is what you hear
    let vt = engine_core::acoustics::valve_train(cfg);
    let (int_dur, exh_dur) = (vt.intake.dur, vt.exhaust.dur);
    let int_peak = (vt.intake.open + int_dur * 0.5).rem_euclid(720.0);
    let exh_peak = (vt.exhaust.open + exh_dur * 0.5).rem_euclid(720.0);
    let geom = RigGeom {
        lay: lay.clone(),
        valve_lift,
        int_peak_deg: int_peak,
        exh_peak_deg: exh_peak,
        int_dur_deg: int_dur,
        exh_dur_deg: exh_dur,
        spring_len,
        bridge_rest_y,
        cam_base_r,
        is_turbo: cfg.turbo.is_some(),
    };

    // shared meshes
    let h_int = 0.168 * b;
    let h_exh = 0.145 * b;
    let valve_int = cx.mesh(valve(h_int, 0.0028, stem_len));
    let valve_exh = cx.mesh(valve(h_exh, 0.0029, stem_len));
    let spring_mesh = cx.mesh(spring(0.0095, 0.0015, 7.0, spring_len));
    let bridge_mesh = cx.mesh(MB::cuboid(0.4 * b, 0.007, 0.022));
    let retainer = cx.mesh(MB::cylinder(0.0075, 0.0, 0.0035, 14).rot_to_y());
    let skirt = 0.55 * b;
    let piston_mesh = cx.mesh(piston(b, lay.comp_h, skirt));
    let rod_mesh = cx.mesh(conrod(lay.rod_len, 0.0125 * (b / 0.09).max(0.8), r_pin * 1.12, lay.rod_w * 0.9));
    let sleeve_len = lay.stroke + lay.chamber_h + 0.055;
    let sleeve_mesh = cx.mesh(MB::tube(b * 0.5 + 0.0035, b * 0.5, 0.0, sleeve_len, 48).rot_to_y());
    let lobe_mesh = cx.mesh(cam_lobe(cam_base_r, valve_lift, (int_dur * 0.25).to_radians(), -0.011, 0.011));
    let lobe_mesh_exh = cx.mesh(cam_lobe(cam_base_r, valve_lift, (exh_dur * 0.25).to_radians(), -0.011, 0.011));
    let plug_body = cx.mesh({
        let mut mb = MB::cylinder(0.0075, 0.0, 0.022, 16).rot_to_y(); // hex-ish shell
        mb.append(&MB::cylinder(0.0052, 0.022, 0.011 + 0.075, 14).rot_to_y(), Mat4::IDENTITY);
        mb
    });
    let plug_ceramic = cx.mesh(MB::cylinder(0.0047, 0.0, 0.07, 16).rot_to_y());
    let coil_mesh = cx.mesh(MB::cuboid(0.034, 0.075, 0.034));
    let coil_cap = cx.mesh(MB::cuboid(0.036, 0.012, 0.036));
    let flame_mesh = cx.mesh(MB::sphere(1.0, 20));
    let spark_mesh = cx.mesh(MB::sphere(0.0045, 8));
    let flamelet_mesh = cx.mesh(MB::sphere(1.0, 8));
    // three jagged arc shapes from the centre electrode to the ground strap
    let bolt_meshes: Vec<Handle<Mesh>> = (0..3)
        .map(|v| {
            let h = |i: usize, c: usize| ((i * 7 + c * 13 + v * 29) % 11) as f32 / 11.0 - 0.5;
            let pts: Vec<Vec3> = (0..=7)
                .map(|i| {
                    let u = i as f32 / 7.0;
                    let j = if i == 0 || i == 7 { 0.0 } else { 0.0028 };
                    Vec3::new(0.0062 * u + j * h(i, 0), 0.0055 - 0.0075 * u + j * h(i, 1), j * h(i, 2))
                })
                .collect();
            cx.mesh(MB::sweep_tube(&pts, &[0.00065], 5, false))
        })
        .collect();
    let cam_shaft_mesh_len = |x0: f32, x1: f32| MB::cylinder(0.0088, x0, x1, 18);

    // per-bank cam shafts
    for (bi, node) in bank_nodes.iter().enumerate() {
        let cyls_in_bank: Vec<usize> = (0..n).filter(|&i| lay.cyls[i].bank == bi).collect();
        let sigma = lay.cyls[cyls_in_bank[0]].sigma;
        let xa = cyls_in_bank.iter().map(|&i| lay.cyls[i].x).fold(f32::MAX, f32::min) - lay.pitch * 0.5;
        let xb = cyls_in_bank.iter().map(|&i| lay.cyls[i].x).fold(f32::MIN, f32::max) + lay.pitch * 0.5;
        for intake in [true, false] {
            let z = if intake { -sigma } else { sigma } * 0.19 * b;
            let shaft = cx.node(*node, Transform::from_xyz(0.0, cam_y, z));
            cx.cmd.entity(shaft).insert(Spin::x(SpinSrc::Cam, 1.0, 0.0));
            cx.spawn(shaft, cam_shaft_mesh_len(xa - 0.03, xb + 0.01), &m.steel, Transform::default());
            // timing sprocket (front)
            cx.spawn(
                shaft,
                gear_mesh(38, 0.0036, 0.012, 0.0, 0.008).translated(Vec3::new(xa - 0.028, 0.0, 0.0)),
                if intake { &m.anodized_red } else { &m.anodized_blue },
                Transform::default(),
            );
            for &ci in &cyls_in_bank {
                let cg = &lay.cyls[ci];
                let peak = if intake { int_peak } else { exh_peak };
                let c = ((cg.offset_deg + peak) * 0.5).to_radians();
                let lobe = if intake { &lobe_mesh } else { &lobe_mesh_exh };
                cx.put(shaft, lobe, &m.polished, Transform::from_xyz(cg.x, 0.0, 0.0).with_rotation(rot_x(c)));
                // journal bearing caps between cylinders
                cx.spawn(
                    shaft,
                    MB::cylinder(0.0105, -0.007, 0.007, 16).translated(Vec3::new(cg.x + lay.pitch * 0.5, 0.0, 0.0)),
                    &m.brass,
                    Transform::default(),
                );
            }
        }
    }

    // ------------------------------------------------------------------ per cylinder
    for ci in 0..n {
        let cg = &lay.cyls[ci];
        let node = bank_nodes[cg.bank];
        let x = cg.x;

        // piston + rod (positions driven by the animation system)
        let p = cx.put(node, &piston_mesh, &m.aluminum, Transform::from_xyz(x, lay.r + lay.rod_len, 0.0));
        cx.cmd.entity(p).insert(PistonC(ci));
        let rd = cx.put(node, &rod_mesh, &m.steel, Transform::from_xyz(x, lay.r + lay.rod_len, 0.0));
        cx.cmd.entity(rd).insert(RodC(ci));

        // glass sleeve, tinted by cylinder pressure
        let sleeve_mat = cx.material(StandardMaterial {
            base_color: Color::srgba(0.75, 0.88, 1.0, 0.09),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.05,
            reflectance: 0.6,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let s = cx.put(node, &sleeve_mesh, &sleeve_mat, Transform::from_xyz(x, lay.deck - sleeve_len, 0.0));
        cx.cmd.entity(s).insert(PressureTint { ci, mat: sleeve_mat });

        // combustion glow volume
        let flame_mat = cx.material(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::BLACK,
            alpha_mode: AlphaMode::Add,
            unlit: false,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let f = cx.put(node, &flame_mesh, &flame_mat, Transform::from_xyz(x, lay.deck - 0.01, 0.0).with_scale(Vec3::splat(0.001)));
        cx.cmd.entity(f).insert((
            FlameC { ci, mat: flame_mat, crown_to_deck: 0.0, hold: 0.0, r_hold: 0.0, burn_prev: 0.0 },
            Visibility::Hidden,
        ));
        // bright licks riding the flame front
        let front_mat = cx.material(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::BLACK,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        for k in 0..5 {
            let l = cx.put(node, &flamelet_mesh, &front_mat, Transform::from_xyz(x, lay.deck - 0.01, 0.0).with_scale(Vec3::splat(0.001)));
            cx.cmd.entity(l).insert((Flamelet { ci, k, mat: front_mat.clone() }, Visibility::Hidden));
        }

        // valves, springs, retainers, bridges
        for intake in [true, false] {
            let z = if intake { -cg.sigma } else { cg.sigma } * 0.19 * b;
            let vm = if intake { &valve_int } else { &valve_exh };
            let vmat = if intake { &m.polished } else { &m.hot_metal };
            for dx in [-0.2 * b, 0.2 * b] {
                let v = cx.put(node, vm, vmat, Transform::from_xyz(x + dx, seat_y, z));
                cx.cmd.entity(v).insert(ValveC { ci, intake, base_y: seat_y });
                let sp = cx.put(node, &spring_mesh, &m.steel, Transform::from_xyz(x + dx, seat_y + 0.014, z));
                cx.cmd.entity(sp).insert(SpringC { ci, intake });
                let rt = cx.put(node, &retainer, &m.brass, Transform::from_xyz(x + dx, seat_y + 0.014 + spring_len, z));
                cx.cmd.entity(rt).insert(ValveC { ci, intake, base_y: seat_y + 0.014 + spring_len });
            }
            let br = cx.put(node, &bridge_mesh, &m.dark_steel, Transform::from_xyz(x, bridge_rest_y, z));
            cx.cmd.entity(br).insert(BridgeC { ci, intake });
        }

        // spark plug (gasoline) or glow plug / injector (diesel) + coil
        let plug_y = lay.deck;
        if !is_diesel {
            cx.put(node, &plug_body, &m.steel, Transform::from_xyz(x, plug_y + 0.004, 0.0));
            cx.put(node, &plug_ceramic, &m.plug_ceramic, Transform::from_xyz(x, plug_y + 0.034, 0.0));
            cx.put(node, &coil_mesh, &m.carbon, Transform::from_xyz(x, plug_y + 0.115, 0.0));
            cx.put(node, &coil_cap, &m.paint_red, Transform::from_xyz(x, plug_y + 0.158, 0.0));
        } else {
            let inj = MB::lathe(
                &[(0.0, 0.0, false), (0.0, 0.0055, false), (0.07, 0.0055, false), (0.07, 0.009, false), (0.12, 0.009, false), (0.12, 0.0, false)],
                14,
            )
            .rot_to_y();
            cx.spawn(node, inj, &m.gold_foil, Transform::from_xyz(x, plug_y + 0.002, 0.0));
        }
        // spark / glow flash at the electrode
        let spark_mat = cx.material(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::BLACK,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        let sp = cx.put(node, &spark_mesh, &spark_mat, Transform::from_xyz(x, plug_y - 0.006, 0.0));
        for (variant, bm) in bolt_meshes.iter().enumerate() {
            let bolt = cx.put(node, bm, &spark_mat, Transform::from_xyz(x, plug_y - 0.006, 0.0));
            cx.cmd.entity(bolt).insert((SparkBolt { ci, variant }, Visibility::Hidden));
        }
        cx.cmd.entity(sp).insert((
            SparkC { ci, mat: spark_mat, timer: 0.0, color: Color::WHITE },
            Visibility::Hidden,
        ));
    }
    let _ = (x_last, FRAC_PI_2, PI);

    EngineBuild { geom, bank_nodes, crank, x_mid, r_main, r_pin }
}
