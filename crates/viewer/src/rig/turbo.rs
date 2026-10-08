//! One turbocharger assembly: hot-side scroll and wheel, centre cartridge, compressor volute and
//! wheel, wastegate actuator and oil feed. Built in its own frame (shaft along local Z, turbine on
//! +Z, compressor on -Z) so any number of them can be dropped in at any scale.

use super::anim::*;
use super::parts::*;
use super::Ctx;
use crate::meshgen::*;
use bevy::prelude::*;
use engine_core::AirFilter;
use std::f32::consts::{FRAC_PI_2, PI};

/// World-space connection points of a placed turbo.
pub struct TurboPorts {
    pub center: Vec3,
    /// Turbine inlet face (exhaust from the manifold arrives here, from +X).
    pub inlet: Vec3,
    /// Turbine outlet (downpipe starts here, heading +Z).
    pub outlet: Vec3,
    /// Compressor discharge (charge pipe starts here, heading -X).
    pub comp_out: Vec3,
}

/// Materials shared by every turbo in the rig.
pub struct TurboMats {
    pub hot: Handle<StandardMaterial>,
    pub boost: Handle<StandardMaterial>,
}

#[allow(clippy::too_many_arguments)]
pub fn build_turbo(
    cx: &mut Ctx,
    root: Entity,
    c: Vec3,
    s: f32,
    flip: bool,
    filter: AirFilter,
    mats: &TurboMats,
    oil_target: Vec3,
) -> TurboPorts {
    let m = cx.mats.clone();
    let yaw = Quat::from_rotation_y(-FRAC_PI_2); // local X -> +Z (the shaft axis)
    // `flip` mirrors the whole assembly across Z (compressor on +Z), keeping the turbine inlet on +X
    let zs = if flip { -1.0 } else { 1.0 };
    let node = cx.node(root, Transform::from_translation(c).with_scale(Vec3::new(s, s, s * zs)));
    let turb_z = 0.085f32;
    let comp_z = -0.07f32;
    let w = |l: Vec3| c + Vec3::new(l.x, l.y, l.z * zs) * s;

    // ---- hot side
    let scroll = MB::lathe(
        &(0..=24)
            .map(|i| {
                let a = PI * 2.0 * i as f32 / 24.0;
                (0.026 * a.cos(), 0.058 + 0.026 * a.sin(), true)
            })
            .collect::<Vec<_>>(),
        40,
    );
    cx.spawn(node, scroll, &mats.hot, Transform::from_xyz(0.0, 0.0, turb_z).with_rotation(yaw));
    // turbine outlet cone + flange
    cx.spawn(node, MB::tube(0.034, 0.028, 0.0, 0.06, 28), &mats.hot, Transform::from_xyz(0.0, 0.0, turb_z + 0.035).with_rotation(yaw));
    cx.spawn(node, flange_mesh(0.045, 6), &m.hot_metal, Transform::from_xyz(0.0, 0.0, turb_z + 0.095).with_rotation(yaw));
    // inlet flange with studs
    let inlet_l = Vec3::new(0.07, 0.0, turb_z);
    cx.spawn(node, flange_mesh(0.036, 4), &m.hot_metal, Transform::from_translation(inlet_l));
    // wheel
    let turb_wheel = cx.mesh(bladed_wheel(12, 0.011, 0.031, 0.026, -0.9, 0.0016, true, false));
    let tw = cx.put(node, &turb_wheel, &m.dark_steel, Transform::from_xyz(0.0, 0.0, turb_z - 0.012).with_rotation(yaw));
    cx.cmd.entity(tw).insert(Spin { src: SpinSrc::Turbo, axis: Vec3::X, ratio: 1.0, phase: 0.0, base: yaw });

    // ---- centre cartridge + shaft + v-band clamps
    cx.spawn(node, MB::tube(0.024, 0.0, -0.03, 0.03, 28), &m.cast_iron, Transform::default().with_rotation(yaw));
    for dz in [-0.034f32, 0.034] {
        cx.spawn(node, MB::tube(0.064, 0.056, -0.006, 0.006, 40), &m.steel, Transform::from_xyz(0.0, 0.0, dz).with_rotation(yaw));
        cx.spawn(node, MB::cuboid(0.016, 0.018, 0.014), &m.dark_steel, Transform::from_xyz(0.0, 0.064, dz));
    }
    let shaft = cx.mesh(MB::cylinder(0.0045, -0.09, 0.09, 14));
    let sh = cx.put(node, &shaft, &m.polished, Transform::default().with_rotation(yaw));
    cx.cmd.entity(sh).insert(Spin { src: SpinSrc::Turbo, axis: Vec3::X, ratio: 1.0, phase: 0.0, base: yaw });
    // oil feed (braided line, AN fittings) back to the engine
    let oil_start = w(Vec3::new(0.0, 0.03, 0.0));
    let oil_pts = [oil_start, oil_start + Vec3::new(0.06, 0.09, 0.02), oil_target];
    let sm = super::spline(&oil_pts, 6);
    cx.spawn(root, MB::sweep_tube(&sm, &[0.0042], 10, true), &m.braided, Transform::default());
    for p in [oil_start, oil_target] {
        cx.spawn(root, MB::cylinder(0.0085, -0.006, 0.006, 6).rot_to_y(), &m.anodized_blue, Transform::from_translation(p));
    }

    // ---- compressor: translucent volute (glows with boost) + wheel
    let comp_cover = MB::lathe(
        &[
            (0.058, 0.026, false),
            (0.058, 0.034, false),
            (0.035, 0.034, true),
            (0.012, 0.052, true),
            (-0.012, 0.066, true),
            (-0.03, 0.058, true),
            (-0.034, 0.03, false),
        ],
        44,
    );
    let cc = cx.spawn(node, comp_cover, &mats.boost, Transform::from_xyz(0.0, 0.0, comp_z).with_rotation(Quat::from_rotation_y(FRAC_PI_2)));
    cx.cmd.entity(cc).insert(BoostGlow { mat: mats.boost.clone() });
    let comp_wheel = cx.mesh(bladed_wheel(11, 0.010, 0.033, 0.032, 0.9, 0.0014, true, true));
    let cw = cx.put(node, &comp_wheel, &m.polished, Transform::from_xyz(0.0, 0.0, comp_z + 0.022).with_rotation(yaw));
    cx.cmd.entity(cw).insert(Spin { src: SpinSrc::Turbo, axis: Vec3::X, ratio: 1.0, phase: 0.0, base: yaw });
    // compressor discharge neck
    let out_l = Vec3::new(-0.03, 0.07, comp_z);
    cx.spawn(node, MB::tube(0.027, 0.022, 0.0, 0.045, 24), &m.anodized_blue, Transform::from_translation(out_l).with_rotation(orient_x(-Vec3::X)));
    cx.spawn(node, flange_mesh(0.032, 4), &m.anodized_blue, Transform::from_translation(out_l + Vec3::new(0.0, 0.0, 0.0)).with_rotation(orient_x(-Vec3::X)));
    // wastegate actuator
    let wg = cx.spawn(
        node,
        MB::cylinder(0.026, 0.0, 0.07, 22),
        &m.anodized_blue,
        Transform::from_xyz(0.11, 0.07, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
    );
    cx.cmd.entity(wg).insert(WastegateC);
    cx.spawn(node, MB::cylinder(0.003, 0.0, 0.05, 8), &m.polished, Transform::from_xyz(0.11, 0.0, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2)));
    cx.spawn(node, MB::cylinder(0.0075, 0.07, 0.078, 14), &m.steel, Transform::from_xyz(0.11, 0.07, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2)));

    // ---- compressor inlet: this turbo's own air filter
    let inlet_tip = Vec3::new(0.0, 0.0, comp_z - 0.058);
    filter_assembly(cx, root, filter, w(inlet_tip), Vec3::new(0.0, 0.0, -zs), 0.03 * s);

    TurboPorts {
        center: c,
        inlet: w(inlet_l),
        outlet: w(Vec3::new(0.0, 0.0, turb_z + 0.095)),
        comp_out: w(out_l + Vec3::new(-0.045, 0.0, 0.0)),
    }
}
