//! Where the nameable parts are. Each tag is an invisible marker that `labels.rs` projects to the
//! screen; per-cylinder parts hang off their bank so they follow its tilt.

use super::driveline::Dims;
use super::engine_build::{bank_to_engine, port_local, EngineBuild};
use super::plumbing::PlumbingOut;
use super::Ctx;
use crate::layout::Layout;
use bevy::prelude::*;
use engine_core::{Cat, EngineConfig, IgnitionType, Muffler, Runners, TransmissionConfig, Tuning};

#[allow(clippy::too_many_arguments)]
pub fn add_tags(
    cx: &mut Ctx,
    root: Entity,
    cfg: &EngineConfig,
    lay: &Layout,
    eb: &EngineBuild,
    d: &Dims,
    pl: &PlumbingOut,
    trans: &TransmissionConfig,
    tuning: &Tuning,
) {
    let g = &eb.geom;
    let b = lay.bore;
    let diesel = cfg.ignition_type == IgnitionType::Compression;

    // ---------------------------------------------------------------- per cylinder
    let seat_y = lay.deck + 0.001;
    let cam_y = g.bridge_rest_y + 0.004 + g.cam_base_r;
    for (ci, cg) in lay.cyls.iter().enumerate() {
        let node = eb.bank_nodes[cg.bank];
        let (x, c) = (cg.x, Some(ci));
        let z_in = -cg.sigma * 0.19 * b;
        let z_ex = cg.sigma * 0.19 * b;
        cx.tag(node, Vec3::new(x, lay.rod_len + lay.comp_h * 0.5, 0.0), "piston", c);
        cx.tag(node, Vec3::new(x, lay.rod_len * 0.5, 0.0), "conrod", c);
        cx.tag(node, Vec3::new(x, lay.deck - 0.05, 0.0), "cylinder", c);
        cx.tag(node, Vec3::new(x, seat_y + 0.035, z_in), "intake_valve", c);
        cx.tag(node, Vec3::new(x, seat_y + 0.035, z_ex), "exhaust_valve", c);
        cx.tag(node, Vec3::new(x, cam_y, z_in), "camshaft", c);
        if diesel {
            cx.tag(node, Vec3::new(x, lay.deck + 0.07, 0.0), "injector", c);
        } else {
            cx.tag(node, Vec3::new(x, lay.deck + 0.06, 0.0), "plug", c);
            cx.tag(node, Vec3::new(x, lay.deck + 0.14, 0.0), "coil", c);
        }
        // the numbered badge floats above the cylinder
        cx.tag(node, Vec3::new(x, lay.deck + 0.24, 0.0), "cyl_badge", c);
    }

    // ---------------------------------------------------------------- engine, front and rear
    cx.tag(root, Vec3::new(eb.x_mid, 0.0, 0.0), "crank", None);
    cx.tag(root, Vec3::new(lay.x_front - 0.045, b * 0.5, 0.0), "damper", None);
    cx.tag(root, Vec3::new(eb.x_mid, -0.07, b * 0.725), "oil_pan", None);
    cx.tag(root, Vec3::new(d.x_rear + 0.02, -0.14, 0.0), "flywheel", None);
    cx.tag(root, Vec3::new(d.x_rear + 0.07, 0.12, 0.0), "clutch", None);
    cx.tag(root, Vec3::new(d.x_rear + 0.16, -0.09, -0.185), "starter", None);

    // ---------------------------------------------------------------- belt drive and cooling
    let u = (b / 0.09).clamp(0.8, 1.25);
    cx.tag(root, Vec3::new(lay.x_front - 0.09, 0.115 * u, 0.13 * u), "alternator", None);
    cx.tag(root, Vec3::new(lay.x_front - 0.1, 0.02 * u, -0.17 * u), "ac", None);
    cx.tag(root, Vec3::new(lay.x_front - 0.06, 0.14 * u, -0.07 * u), "waterpump", None);
    cx.tag(root, Vec3::new(lay.x_front - 0.039, 0.045 * u, 0.19 * u + 0.035), "belt", None);
    cx.tag(root, Vec3::new(pl.rad_x, 0.12, -0.1), "radiator", None);

    // ---------------------------------------------------------------- intake
    cx.tag(root, pl.throttle_center, "throttle", None);
    if tuning.runners != Runners::Itb {
        cx.tag(root, Vec3::new(eb.x_mid, pl.plenum_c.y, pl.plenum_c.z), "plenum", None);
        if pl.n_turbos == 0 {
            cx.tag(root, pl.throttle_center + Vec3::new(-0.11, 0.0, 0.0), "filter", None);
        }
    }
    if let Some(p) = pl.fuel_rail {
        cx.tag(root, p, "fuel_rail", None);
    }

    // ---------------------------------------------------------------- exhaust
    if let Some(cg) = lay.cyls.first() {
        let port = bank_to_engine(cg.phi, port_local(lay, 0, false));
        let out = bank_to_engine(cg.phi, Vec3::new(0.0, 0.0, cg.sigma));
        cx.tag(root, port + out * 0.06 + Vec3::new(0.0, -0.02, 0.0), "header", None);
    }
    if let Some(t) = pl.turbo_center {
        cx.tag(root, t, "turbo", None);
    }
    if let Some(ic) = pl.intercooler {
        cx.tag(root, ic + Vec3::new(0.0, 0.12, 0.0), "intercooler", None);
    }
    let cc = pl.cat_center;
    cx.tag(root, Vec3::new(cc.x - 0.43, cc.y + 0.04, cc.z), "o2", None);
    if tuning.cat != Cat::Delete {
        cx.tag(root, Vec3::new(cc.x - 0.15, cc.y + 0.07, cc.z), "cat", None);
    }
    if tuning.muffler != Muffler::Delete {
        let m = pl.muffler_center;
        cx.tag(root, Vec3::new(m.x, m.y + 0.1, m.z), "muffler", None);
    }
    cx.tag(root, pl.tail_tip, "tailpipe", None);

    // ---------------------------------------------------------------- drivetrain
    let gb_mid = (d.x_gear0 + d.x_gear_end) * 0.5;
    cx.tag(root, Vec3::new(gb_mid, 0.115, 0.0), "gearbox", None);
    let z_ring = (trans.final_drive_ratio as f32 * 11.0).round().max(30.0);
    let ring_r = z_ring * 0.0056 * 0.5;
    let pin_y = d.y_axle + ring_r + 0.032;
    let (p0, p1) = (Vec3::new(d.x_t_out + 0.01, 0.0, 0.0), Vec3::new(d.x_pinion - 0.01, pin_y, 0.0));
    cx.tag(root, (p0 + p1) * 0.5 + Vec3::new(0.0, 0.05, 0.0), "driveshaft", None);
    cx.tag(root, Vec3::new(d.x_diff + 0.04, d.y_axle + 0.02 + ring_r + 0.06, 0.0), "diff", None);
    cx.tag(root, Vec3::new(d.x_diff, d.y_axle + 0.33, d.half_track), "wheel", None);
    cx.tag(root, Vec3::new(d.x_diff, d.y_axle + 0.2, d.half_track + 0.016), "brake", None);
}
