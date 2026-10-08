//! Intake (filter, throttle, plenum or individual throttle bodies, runners, intercooler), exhaust
//! (headers, collectors, turbos, downpipes, catalyst, muffler, tips) and the cooling hardware
//! that sits in front of them. Every part follows the active [`Tuning`], so swapping a header or
//! adding a turbo genuinely changes what is on screen.

use super::accessories;
use super::anim::*;
use super::driveline::Dims;
use super::engine_build::{bank_to_engine, port_local, EngineBuild};
use super::parts::*;
use super::turbo::{build_turbo, TurboMats};
use super::{bezier, spline, Ctx};
use crate::layout::Layout;
use crate::meshgen::*;
use bevy::prelude::*;
use engine_core::{AirFilter, Cat, EngineConfig, Header, IgnitionType, Intercooler, Muffler, Runners, Tuning};

#[allow(dead_code)]
pub struct PlumbingOut {
    /// Per-cylinder polylines the gas-flow particles follow (plenum->port, port->tailpipe).
    pub intake_paths: Vec<Vec<Vec3>>,
    pub exhaust_paths: Vec<Vec<Vec3>>,
    pub tail_tip: Vec3,
    pub turbo_center: Option<Vec3>,
    pub intake_front: Vec3,
    pub muffler_center: Vec3,
    pub throttle_center: Vec3,
    pub cat_center: Vec3,
    pub n_turbos: usize,
    /// Frontmost X of the radiator / intercooler stack (where the chassis rails begin).
    pub front_x: f32,
    /// Where the labelled parts sit (see `rig::tags`).
    pub plenum_c: Vec3,
    pub rad_x: f32,
    pub intercooler: Option<Vec3>,
    pub fuel_rail: Option<Vec3>,
}

fn pipe(cx: &mut Ctx, parent: Entity, pts: &[Vec3], radius: f32, mat: &Handle<StandardMaterial>) -> Entity {
    let sm = spline(pts, 6);
    let mb = MB::sweep_tube(&sm, &[radius], 18, true);
    cx.spawn(parent, mb, mat, Transform::default())
}

fn hot_material(cx: &mut Ctx, c: Color, rough: f32) -> Handle<StandardMaterial> {
    cx.material(StandardMaterial {
        base_color: c,
        metallic: 1.0,
        perceptual_roughness: rough,
        reflectance: 0.5,
        ..default()
    })
}

/// A see-through variant so gas-flow particles inside the pipe stay visible.
fn glassy_material(cx: &mut Ctx, c: Color, alpha: f32) -> Handle<StandardMaterial> {
    let l = c.to_linear();
    cx.material(StandardMaterial {
        base_color: Color::linear_rgba(l.red, l.green, l.blue, alpha),
        alpha_mode: AlphaMode::Blend,
        metallic: 0.9,
        perceptual_roughness: 0.3,
        reflectance: 0.5,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

fn heat_zone(cx: &mut Ctx, root: Entity, mat: &Handle<StandardMaterial>, gain: f32) {
    let e = cx
        .cmd
        .spawn((HeatGlow { mat: mat.clone(), gain, base: Color::BLACK }, Transform::default(), Visibility::default()))
        .id();
    cx.cmd.entity(e).insert(ChildOf(root));
}

/// One exhaust collector: the cylinders of one bank that dump into the same pipe.
struct Sub {
    bank: usize,
    lane: usize,
    cyls: Vec<usize>,
    /// Which turbo (or, without turbos, which bank pipe) this collector feeds.
    slot: usize,
}

/// Splits the cylinders over `n_slots` turbos. Whole banks are shared when there are fewer
/// turbos than banks; otherwise each bank's cylinders are chunked between its own turbos.
fn make_subs(lay: &Layout, n_slots: usize) -> (Vec<Sub>, usize) {
    let nb = lay.tilts.len();
    let n = lay.n();
    let mut subs = Vec::new();
    if n_slots == 0 || n_slots < nb {
        for bi in 0..nb {
            let cyls: Vec<usize> = (0..n).filter(|&i| lay.cyls[i].bank == bi).collect();
            if cyls.is_empty() {
                continue;
            }
            let slot = if n_slots == 0 { bi } else { bi * n_slots / nb };
            subs.push(Sub { bank: bi, lane: 0, cyls, slot });
        }
        let used = subs.iter().map(|s| s.slot).max().map_or(0, |m| m + 1);
        return (subs, used);
    }
    let mut per_bank = vec![n_slots / nb; nb];
    for b in 0..n_slots % nb {
        per_bank[b] += 1;
    }
    let mut slot = 0;
    for bi in 0..nb {
        let mut cyls: Vec<usize> = (0..n).filter(|&i| lay.cyls[i].bank == bi).collect();
        cyls.sort_by(|&a, &b| lay.cyls[a].x.partial_cmp(&lay.cyls[b].x).unwrap());
        let chunks = per_bank[bi].min(cyls.len()).max(1);
        let base = cyls.len() / chunks;
        let extra = cyls.len() % chunks;
        let mut at = 0;
        for k in 0..chunks {
            let len = base + usize::from(k < extra);
            subs.push(Sub { bank: bi, lane: k, cyls: cyls[at..at + len].to_vec(), slot });
            at += len;
            slot += 1;
        }
    }
    (subs, slot)
}

struct HeaderStyle {
    runner_r: f32,
    handle: f32,
    drop: f32,
    coll_r: f32,
    coll_ext: f32,
}

fn header_style(h: Header) -> HeaderStyle {
    match h {
        Header::Stock => HeaderStyle { runner_r: 1.45, handle: 0.028, drop: 0.0, coll_r: 1.7, coll_ext: 0.0 },
        Header::Shorty => HeaderStyle { runner_r: 1.0, handle: 0.045, drop: 0.0, coll_r: 1.0, coll_ext: 0.0 },
        Header::LongTube => HeaderStyle { runner_r: 0.93, handle: 0.09, drop: 0.06, coll_r: 0.9, coll_ext: 0.22 },
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_plumbing(
    cx: &mut Ctx,
    root: Entity,
    cfg: &EngineConfig,
    lay: &Layout,
    _eb: &EngineBuild,
    d: &Dims,
    tuning: &Tuning,
) -> PlumbingOut {
    let m = cx.mats.clone();
    let b = lay.bore;
    let n = lay.n();
    let multi_bank = lay.tilts.len() > 1;
    let is_diesel = cfg.ignition_type == IgnitionType::Compression;
    let n_turbo_req = tuning.turbo_count_for(cfg) as usize;
    let turbo = n_turbo_req > 0;
    let hs = header_style(tuning.header);
    let pipe_scale = tuning.pipe_scale as f32;
    let turbo_size = tuning.turbo_size as f32;

    // One hot material per heat zone so each glows independently of the others.
    let (mani_col, mani_alpha) = match tuning.header {
        Header::Stock => (Color::srgb(0.27, 0.24, 0.22), 0.8),
        Header::Shorty => (Color::srgb(0.58, 0.59, 0.62), 0.5),
        Header::LongTube => (Color::srgb(0.42, 0.4, 0.58), 0.5),
    };
    let hot_mani = glassy_material(cx, mani_col, mani_alpha);
    let hot_pipe = hot_material(cx, Color::srgb(0.46, 0.46, 0.48), 0.4);
    let hot_cat = hot_material(cx, Color::srgb(0.5, 0.5, 0.52), 0.35);
    heat_zone(cx, root, &hot_mani, 1.0);
    heat_zone(cx, root, &hot_pipe, 0.55);
    heat_zone(cx, root, &hot_cat, 1.15);

    // ---------------------------------------------------------------- ports in engine frame
    let int_ports: Vec<Vec3> = (0..n).map(|i| bank_to_engine(lay.cyls[i].phi, port_local(lay, i, true))).collect();
    let exh_ports: Vec<Vec3> = (0..n).map(|i| bank_to_engine(lay.cyls[i].phi, port_local(lay, i, false))).collect();
    let int_dirs: Vec<Vec3> =
        (0..n).map(|i| bank_to_engine(lay.cyls[i].phi, Vec3::new(0.0, 0.0, -lay.cyls[i].sigma))).collect();
    let exh_dirs: Vec<Vec3> =
        (0..n).map(|i| bank_to_engine(lay.cyls[i].phi, Vec3::new(0.0, 0.0, lay.cyls[i].sigma))).collect();

    let x_first = lay.cyls.iter().map(|c| c.x).fold(f32::MAX, f32::min);
    let x_last = lay.cyls.iter().map(|c| c.x).fold(f32::MIN, f32::max);
    let mean_int = int_ports.iter().copied().sum::<Vec3>() / n as f32;
    let ghost_alu = glassy_material(cx, Color::srgb(0.7, 0.74, 0.8), 0.32);

    // ---------------------------------------------------------------- intake manifold
    let itb = tuning.runners == Runners::Itb;
    let (pl_scale, pl_lift, run_len, run_drop, run_fat) = match tuning.runners {
        Runners::LongRunner => (0.85, 0.07, 0.075, 0.07, 0.9),
        Runners::Stock => (1.0, 0.0, 0.035, 0.0, 1.0),
        Runners::ShortRunner => (1.3, -0.012, 0.018, -0.01, 1.28),
        Runners::Itb => (1.0, 0.0, 0.04, 0.0, 1.0),
    };
    let plenum_c = if multi_bank {
        Vec3::new(0.0, mean_int.y + 0.085 + pl_lift, 0.0)
    } else {
        Vec3::new(0.0, mean_int.y + 0.045 + pl_lift, mean_int.z - 0.075 - pl_lift * 0.6)
    };
    let pl_r = (if multi_bank { 0.052 } else { 0.042 }) * pl_scale;
    let pl_x0 = x_first - 0.045;
    let pl_x1 = x_last + 0.045;
    let run_r = (0.15 * b).max(0.014) * run_fat;
    let flange_int = cx.mesh(flange_mesh(run_r * 1.7, 4));
    let mut intake_paths: Vec<Vec<Vec3>> = Vec::with_capacity(n);
    let mut inj_by_bank: Vec<Vec<Vec3>> = vec![Vec::new(); lay.tilts.len()];
    let thr_c: Vec3;

    if !itb {
        let mut plenum = MB::cylinder(pl_r, pl_x0, pl_x1, 36);
        plenum.append(&MB::sphere(pl_r, 24), Mat4::from_translation(Vec3::new(pl_x1, 0.0, 0.0)));
        plenum.append(&MB::sphere(pl_r, 24), Mat4::from_translation(Vec3::new(pl_x0, 0.0, 0.0)));
        cx.spawn(root, plenum, &ghost_alu, Transform::from_translation(plenum_c));
        let rib = cx.mesh(MB::tube(pl_r * 1.03, pl_r * 0.96, 0.0, 0.006, 36));
        for i in 0..=((pl_x1 - pl_x0) / 0.06) as i32 {
            cx.put(root, &rib, &m.aluminum, Transform::from_translation(plenum_c + Vec3::new(pl_x0 + 0.02 + i as f32 * 0.06, 0.0, 0.0)));
        }
        // plenum end caps: bolted plates
        for xe in [pl_x0 - pl_r * 0.55, pl_x1 + pl_r * 0.55] {
            cx.spawn(root, MB::cylinder(pl_r * 0.5, -0.004, 0.004, 24), &m.brushed_alu, Transform::from_translation(plenum_c + Vec3::new(xe, 0.0, 0.0)));
        }
        for i in 0..n {
            let x = lay.cyls[i].x;
            let p0 = int_ports[i];
            let p1 = p0 + int_dirs[i] * run_len;
            let target = Vec3::new(x, plenum_c.y - pl_r * 0.6, plenum_c.z);
            let p2 = target + Vec3::new(0.0, -0.045 - run_drop, 0.0) + (p1 - target) * 0.15;
            let path = bezier(p0, p1, p2, target, 12);
            intake_paths.push(path.iter().rev().cloned().collect());
            let mb = MB::sweep_tube(&path, &[run_r * 1.12, run_r, run_r * 0.92], 16, true);
            cx.spawn(root, mb, &ghost_alu, Transform::default());
            inj_by_bank[lay.cyls[i].bank].push(p1 + int_dirs[i] * 0.02 + Vec3::new(0.0, 0.02, 0.0));
            put_flange(cx, root, &flange_int, &m.dark_steel, p0, int_dirs[i]);
        }
        // throttle body at the front of the plenum
        thr_c = Vec3::new(pl_x0 - 0.07, plenum_c.y, plenum_c.z);
        throttle_body(cx, root, thr_c, Vec3::X, 0.036, 0.06);
        pipe(cx, root, &[thr_c + Vec3::new(0.06, 0.0, 0.0), Vec3::new(pl_x0 + 0.01, plenum_c.y, plenum_c.z)], 0.034, &m.aluminum);
        clamp_ring(cx, root, &m.steel, thr_c + Vec3::new(0.056, 0.0, 0.0), Vec3::X, 0.036);
        // idle air valve + vacuum lines
        cx.spawn(root, MB::cylinder(0.014, 0.0, 0.05, 14).rot_to_y(), &m.paint_black, Transform::from_translation(thr_c + Vec3::new(0.03, 0.04, 0.0)));
    } else {
        // individual throttle bodies: a stack on every cylinder, linked by one rod
        let up = Vec3::Y;
        let mut rods: Vec<Vec<Vec3>> = vec![Vec::new(); lay.tilts.len()];
        for i in 0..n {
            let p0 = int_ports[i];
            let top = p0 + int_dirs[i] * 0.045 + up * 0.17;
            let p1 = p0 + int_dirs[i] * 0.05;
            let p2 = top - up * 0.07 - int_dirs[i] * 0.01;
            let path = bezier(p0, p1, p2, top, 12);
            let mb = MB::sweep_tube(&path, &[run_r * 1.12, run_r, run_r], 16, true);
            cx.spawn(root, mb, &ghost_alu, Transform::default());
            put_flange(cx, root, &flange_int, &m.dark_steel, p0, int_dirs[i]);
            inj_by_bank[lay.cyls[i].bank].push(p1 + int_dirs[i] * 0.02 + Vec3::new(0.0, 0.02, 0.0));
            throttle_body(cx, root, top, up, run_r * 1.28, 0.05);
            let mouth = top + up * 0.05;
            match tuning.air_filter {
                AirFilter::Stock => {}
                kind => filter_assembly(cx, root, kind, mouth, up, run_r * 1.28),
            }
            rods[lay.cyls[i].bank].push(top + up * 0.025);
            let mut ip: Vec<Vec3> = vec![mouth + up * 0.12];
            ip.extend(path.iter().rev().cloned());
            intake_paths.push(ip);
        }
        for pts in rods.iter().filter(|p| p.len() > 1) {
            let a = pts.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(*p));
            let bb = pts.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(*p));
            cx.spawn(root, MB::cylinder(0.0035, -0.5 * (bb.x - a.x) - 0.03, 0.5 * (bb.x - a.x) + 0.03, 8), &m.polished, Transform::from_translation((a + bb) * 0.5 + Vec3::new(0.0, 0.0, 0.03)));
        }
        if tuning.air_filter == AirFilter::Stock {
            // shared airbox lying over the stacks
            let y_top = int_ports.iter().map(|p| p.y).fold(f32::MIN, f32::max) + 0.17 + 0.05;
            let zc = int_ports.iter().map(|p| p.z).sum::<f32>() / n as f32;
            let zspan = int_ports.iter().map(|p| (p.z - zc).abs()).fold(0.0, f32::max) + 0.08;
            let xs = x_last - x_first + 0.1;
            cx.spawn(root, MB::cuboid(xs, 0.1, zspan * 2.0), &m.paint_black, Transform::from_xyz((x_first + x_last) * 0.5, y_top + 0.05, zc));
            cx.spawn(root, MB::cuboid(xs + 0.005, 0.014, zspan * 2.0 + 0.005), &m.carbon, Transform::from_xyz((x_first + x_last) * 0.5, y_top + 0.105, zc));
            cx.spawn(root, MB::tube(0.045, 0.038, 0.0, 0.09, 24), &m.paint_black, Transform::from_xyz(x_first - 0.05 - xs * 0.5 + 0.0, y_top + 0.05, zc).with_rotation(Quat::from_rotation_y(0.0)));
            thr_c = Vec3::new(x_first - 0.08, y_top + 0.05, zc);
        } else {
            thr_c = Vec3::new(x_first - 0.08, int_ports.iter().map(|p| p.y).fold(f32::MIN, f32::max) + 0.2, 0.0);
        }
    }

    // fuel rail(s): gold tube along the injectors, with a feed line
    let mut fuel_rail_pt = None;
    if !is_diesel {
        for pts in inj_by_bank.iter().filter(|p| !p.is_empty()) {
            let mut sorted = pts.clone();
            sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
            let lift: Vec<Vec3> = sorted.iter().map(|p| *p + Vec3::new(0.0, 0.032, 0.0)).collect();
            if fuel_rail_pt.is_none() {
                fuel_rail_pt = lift.get(lift.len() / 2).copied();
            }
            if lift.len() >= 2 {
                let mut ext = vec![lift[0] - Vec3::new(0.03, 0.0, 0.0)];
                ext.extend(lift.iter().cloned());
                ext.push(*lift.last().unwrap() + Vec3::new(0.03, 0.0, 0.0));
                cx.spawn(root, MB::sweep_tube(&ext, &[0.0072], 12, true), &m.gold_foil, Transform::default());
            }
            let inj = cx.mesh(MB::cylinder(0.0062, 0.0, 0.036, 14).rot_to_y());
            let cup = cx.mesh(MB::cylinder(0.0095, 0.0, 0.006, 14).rot_to_y());
            for p in &sorted {
                cx.put(root, &inj, &m.paint_black, Transform::from_translation(*p));
                cx.put(root, &cup, &m.gold_foil, Transform::from_translation(*p + Vec3::new(0.0, 0.03, 0.0)));
            }
            // regulator + feed line
            if let Some(last) = sorted.last() {
                cx.spawn(root, MB::cylinder(0.014, 0.0, 0.04, 16).rot_to_y(), &m.anodized_red, Transform::from_translation(*last + Vec3::new(0.03, 0.032, 0.0)));
            }
        }
    }

    // ---------------------------------------------------------------- exhaust headers
    let (subs, n_slots) = make_subs(lay, if turbo { n_turbo_req.min(n) } else { 0 });
    let n_turbos = if turbo { n_slots } else { 0 };
    let y_pipe = -0.31f32;
    let z_ex = 0.30f32 * pipe_scale.clamp(0.85, 1.2);
    let x_c_front = lay.x_front - 0.07;
    let x_c_rear = lay.x_rear + 0.06 + hs.coll_ext;
    let mut rank_pos = 0;
    let mut rank_neg = 0;
    let mut bank_rank = vec![0usize; lay.tilts.len()];
    for bi in 0..lay.tilts.len() {
        let Some(first) = (0..n).find(|&i| lay.cyls[i].bank == bi) else { continue };
        let sigma = lay.cyls[first].sigma;
        bank_rank[bi] = if sigma > 0.0 { rank_pos } else { rank_neg };
        if sigma > 0.0 {
            rank_pos += 1;
        } else {
            rank_neg += 1;
        }
    }
    let flange_exh_r = ((0.13 * b).max(0.013)) * hs.runner_r.min(1.2) * 1.9;
    let flange_exh = cx.mesh(flange_mesh(flange_exh_r, 4));
    let mut runner_paths: Vec<Vec<Vec3>> = vec![Vec::new(); n];
    struct Coll {
        end: Vec3,
    }
    let mut colls: Vec<Coll> = Vec::new();
    let x_end_coll = if turbo { x_c_front } else { x_c_rear };
    for sub in &subs {
        let sigma = lay.cyls[sub.cyls[0]].sigma;
        let rank = bank_rank[sub.bank] as f32 + sub.lane as f32 * 0.85;
        let avg_port = sub.cyls.iter().map(|&i| exh_ports[i]).sum::<Vec3>() / sub.cyls.len() as f32;
        let yc = avg_port.y - 0.095 - 0.04 * rank - hs.drop;
        let zc = avg_port.z + sigma * (0.085 + 0.045 * rank);
        let rr = ((0.13 * b).max(0.013)) * hs.runner_r;
        for &i in &sub.cyls {
            let x = lay.cyls[i].x;
            let p0 = exh_ports[i];
            let p1 = p0 + exh_dirs[i] * hs.handle;
            let tgt = Vec3::new(x, yc, zc);
            let p2 = tgt + Vec3::new(0.0, 0.05, 0.0) + (p1 - tgt) * 0.1;
            let path = bezier(p0, p1, p2, tgt, 12);
            runner_paths[i] = path.clone();
            let mb = MB::sweep_tube(&path, &[rr * 1.2, rr, rr * 0.95], 16, true);
            cx.spawn(root, mb, &hot_mani, Transform::default());
            put_flange(cx, root, &flange_exh, &m.hot_metal, p0, exh_dirs[i]);
        }
        let xa = sub.cyls.iter().map(|&i| lay.cyls[i].x).fold(f32::MAX, f32::min);
        let xb = sub.cyls.iter().map(|&i| lay.cyls[i].x).fold(f32::MIN, f32::max);
        let coll_r = ((0.19 * b).max(0.02)) * hs.coll_r;
        let (xs, xe) = if turbo { (x_end_coll, xb) } else { (xa, x_end_coll) };
        cx.spawn(root, MB::cylinder(coll_r, xs, xe, 24), &hot_mani, Transform::from_xyz(0.0, yc, zc));
        // cast manifolds are ribbed; long-tube collectors get a merge cone and a flange
        match tuning.header {
            Header::Stock => {
                // cast bosses where each runner joins the log
                let boss = cx.mesh(MB::tube(coll_r * 1.22, coll_r * 0.98, -0.011, 0.011, 24));
                for &i in &sub.cyls {
                    cx.put(root, &boss, &m.cast_iron, Transform::from_xyz(lay.cyls[i].x, yc, zc));
                }
                cx.spawn(root, MB::cylinder(coll_r * 1.25, if turbo { xs - 0.012 } else { xe }, if turbo { xs } else { xe + 0.012 }, 24), &m.cast_iron, Transform::from_xyz(0.0, yc, zc));
            }
            Header::Shorty => {}
            Header::LongTube => {
                let (cone_x0, cone_x1) = if turbo { (xb - 0.09, xb + 0.03) } else { (xa - 0.03, xa + 0.09) };
                cx.spawn(
                    root,
                    MB::cone(coll_r * 1.8, coll_r, cone_x0, cone_x1, 24),
                    &hot_mani,
                    Transform::from_xyz(0.0, yc, zc),
                );
                // coated collector wrap bands
                for k in 0..4 {
                    let x = if turbo { x_end_coll + 0.03 + k as f32 * 0.04 } else { x_end_coll - 0.03 - k as f32 * 0.04 };
                    cx.spawn(root, MB::tube(coll_r * 1.1, coll_r * 0.98, 0.0, 0.006, 24), &m.stainless, Transform::from_xyz(x, yc, zc));
                }
            }
        }
        colls.push(Coll { end: Vec3::new(x_end_coll, yc, zc) });
    }

    // ---------------------------------------------------------------- turbos + charge piping
    let main_pipe_r = ((0.25 * b).clamp(0.026, 0.04)) * pipe_scale.clamp(0.7, 1.5);
    let exhaust_start = Vec3::new(d.x_cat - 0.55, y_pipe, z_ex);
    let intake_front: Vec3;
    let mut turbo_center = None;
    let mut sub_common: Vec<Vec<Vec3>> = vec![Vec::new(); subs.len()];
    let x_t = lay.x_front - 0.30;
    let y_t = -0.02f32;
    let mut rad_x = lay.x_front - 0.40;
    // cars with an intercooler line the charge pipes up to this core
    let mut ic_c = Vec3::new(x_t - 0.34, -0.06, 0.0);

    if turbo {
        let ts = (turbo_size.clamp(0.6, 1.6).sqrt() / (n_turbos.max(1) as f32).powf(0.25)).clamp(0.55, 1.3);
        let z_col = 0.09 + 0.10 * ts;
        let tier_dy = 0.19 * ts + 0.03;
        // order slots by where their collectors sit so left pipes stay left
        let mut order: Vec<usize> = (0..n_turbos).collect();
        let slot_z = |slot: usize| -> f32 {
            let zs: Vec<f32> = subs.iter().zip(&colls).filter(|(s, _)| s.slot == slot).map(|(_, c)| c.end.z).collect();
            zs.iter().sum::<f32>() / zs.len().max(1) as f32
        };
        order.sort_by(|&a, &bb| slot_z(a).partial_cmp(&slot_z(bb)).unwrap());
        let half = n_turbos.div_ceil(2);
        let boost_mat = cx.material(StandardMaterial {
            base_color: Color::srgba(0.6, 0.72, 0.85, 0.26),
            alpha_mode: AlphaMode::Blend,
            metallic: 0.9,
            perceptual_roughness: 0.25,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let turb_mat = hot_material(cx, Color::srgb(0.46, 0.4, 0.36), 0.38);
        heat_zone(cx, root, &turb_mat, 1.0);
        let tmats = TurboMats { hot: turb_mat.clone(), boost: boost_mat.clone() };
        let mut ports = Vec::new();
        let mut tier_max = 0usize;
        for (rank, &slot) in order.iter().enumerate() {
            let (col, tier) = if n_turbos == 1 { (0.0f32, 0) } else if rank < half { (-1.0, rank) } else { (1.0, rank - half) };
            tier_max = tier_max.max(tier);
            let c = Vec3::new(x_t, y_t + tier as f32 * tier_dy, col * z_col);
            let flip = col > 0.0;
            let oil = Vec3::new(lay.x_front - 0.04, 0.12 + 0.02 * tier as f32, 0.04 + 0.02 * col);
            let p = build_turbo(cx, root, c, ts, flip, tuning.air_filter, &tmats, oil);
            ports.push((slot, p, col, tier));
        }
        turbo_center = Some(ports.iter().map(|p| p.1.center).sum::<Vec3>() / ports.len() as f32);
        // exhaust: each collector to its turbine, then every downpipe to the shared cat pipe
        let mut slot_down: Vec<Vec<Vec3>> = vec![Vec::new(); n_turbos];
        for (slot, p, col, tier) in &ports {
            let lift = *tier as f32 * 0.02;
            let z_mid = p.outlet.z * 0.55;
            let path = [
                p.outlet,
                p.outlet + Vec3::new(0.0, -0.06, -col * 0.035),
                Vec3::new(x_t + 0.05, y_pipe + 0.17 + lift, z_mid),
                Vec3::new(x_t + 0.24, y_pipe + 0.05, z_ex * 0.45 + z_mid * 0.3),
                Vec3::new(x_t + 0.42, y_pipe, z_ex),
                exhaust_start,
            ];
            pipe(cx, root, &path, main_pipe_r * 1.1, &hot_pipe);
            slot_down[*slot] = spline(&path, 6);
        }
        for (si, sub) in subs.iter().enumerate() {
            let inlet = ports.iter().find(|p| p.0 == sub.slot).unwrap().1.inlet;
            let e = colls[si].end;
            let path = bezier(
                e,
                e + Vec3::new(-0.08, 0.0, 0.0),
                inlet + Vec3::new(0.09, 0.0, (e.z - inlet.z).clamp(-0.05, 0.05)),
                inlet,
                14,
            );
            pipe(cx, root, &path, 0.027 * ts.max(0.8), &hot_mani);
            let mut full = path;
            full.extend(slot_down[sub.slot].iter().cloned());
            sub_common[si] = full;
        }

        // ---- intercooler, charge pipes, radiator
        let rows_shift = 0.0;
        ic_c = Vec3::new(x_t - 0.34 - rows_shift, -0.06 + 0.5 * tier_max as f32 * tier_dy, 0.0);
        rad_x = x_t - 0.22;
        let (ic_h, ic_w, ic_t) = match tuning.intercooler {
            Intercooler::None => (0.0, 0.0, 0.0),
            Intercooler::Stock => (0.30, 0.62, 0.09),
            Intercooler::Large => (0.40, 0.78, 0.13),
        };
        let thr_in = thr_c + Vec3::new(-0.01, 0.0, 0.0);
        let comp_outs: Vec<(Vec3, f32)> = ports.iter().map(|(_, p, col, _)| (p.comp_out, *col)).collect();
        if tuning.intercooler == Intercooler::None {
            // straight, long charge pipe from every compressor to the throttle body
            for (k, (co, _)) in comp_outs.iter().enumerate() {
                let mid = Vec3::new(lay.x_front - 0.14, plenum_c.y + 0.08 + 0.02 * k as f32, 0.0);
                let path = [*co, *co + Vec3::new(-0.06, 0.03, 0.0), Vec3::new(mid.x - 0.08, mid.y - 0.05, co.z * 0.4), mid, thr_in];
                pipe(cx, root, &path, 0.027, &m.silicone_blue);
            }
            intake_front = thr_in;
        } else {
            // dark core with proud aluminium fins, so the front face reads as stacked fins
            cx.spawn(root, MB::cuboid(ic_t, ic_h, ic_w), &m.radiator_core, Transform::from_translation(ic_c));
            let mut fins = MB::new();
            let nf = (ic_h / 0.0085) as i32 - 2;
            for k in 0..nf {
                fins.append(&MB::cuboid(ic_t + 0.006, 0.0032, ic_w - 0.02), Mat4::from_translation(Vec3::new(0.0, -ic_h * 0.5 + 0.012 + k as f32 * 0.0085, 0.0)));
            }
            cx.spawn(root, fins, &m.aluminum, Transform::from_translation(ic_c));
            // top and bottom rails of the core
            for sy in [-1.0f32, 1.0] {
                cx.spawn(root, MB::cuboid(ic_t + 0.012, 0.012, ic_w), &m.dark_steel, Transform::from_translation(ic_c + Vec3::new(0.0, sy * (ic_h * 0.5 + 0.004), 0.0)));
            }
            let tank_z = ic_w * 0.5 + 0.025;
            for sz in [-1.0f32, 1.0] {
                // cast end tanks with a taper toward the core
                cx.spawn(root, MB::cuboid(ic_t + 0.03, ic_h + 0.01, 0.05), &m.brushed_alu, Transform::from_translation(ic_c + Vec3::new(0.0, 0.0, sz * tank_z)));
                cx.spawn(root, MB::cuboid(ic_t + 0.04, 0.016, 0.056), &m.anodized_blue, Transform::from_translation(ic_c + Vec3::new(0.0, ic_h * 0.5 - 0.01, sz * tank_z)));
                cx.spawn(root, MB::cuboid(ic_t + 0.04, 0.016, 0.056), &m.anodized_blue, Transform::from_translation(ic_c + Vec3::new(0.0, -ic_h * 0.5 + 0.01, sz * tank_z)));
            }
            // end-tank bolts
            let bolt = cx.mesh(MB::cylinder(0.005, 0.0, 0.01, 6).rot_to_z());
            for sz in [-1.0f32, 1.0] {
                for kx in [-1.0f32, 1.0] {
                    for ky in [-1.0f32, 0.0, 1.0] {
                        cx.put(root, &bolt, &m.polished, Transform::from_translation(ic_c + Vec3::new(kx * ic_t * 0.5, ky * ic_h * 0.38, sz * (tank_z + 0.025))));
                    }
                }
            }
            if n_turbos == 1 {
                let co = comp_outs[0].0;
                let pts = [co, co + Vec3::new(-0.05, 0.05, 0.0), Vec3::new(ic_c.x + 0.02, ic_c.y + 0.06, -ic_w * 0.5 - 0.06), ic_c + Vec3::new(0.0, 0.06, -tank_z)];
                pipe(cx, root, &pts, 0.026, &m.silicone_blue);
                let out = [
                    ic_c + Vec3::new(0.0, 0.06, tank_z),
                    Vec3::new(ic_c.x + 0.05, ic_c.y + 0.2, 0.3),
                    Vec3::new(lay.x_front - 0.12, plenum_c.y + 0.1, 0.12),
                    thr_in,
                ];
                pipe(cx, root, &out, 0.030, &m.silicone_blue);
            } else {
                // both end tanks are fed, the charge leaves from a tank on top of the core
                for (co, col) in &comp_outs {
                    let sz = if *col < 0.0 { -1.0 } else { 1.0 };
                    let pts = [*co, *co + Vec3::new(-0.06, 0.0, 0.0), Vec3::new(ic_c.x + 0.04, co.y.min(ic_c.y + ic_h * 0.4), sz * (tank_z + 0.04)), ic_c + Vec3::new(0.0, ic_h * 0.3, sz * tank_z)];
                    pipe(cx, root, &pts, 0.024 * ts.max(0.8), &m.silicone_blue);
                }
                cx.spawn(root, MB::cuboid(ic_t * 0.8, 0.05, 0.16), &m.anodized_blue, Transform::from_translation(ic_c + Vec3::new(0.0, ic_h * 0.5 + 0.025, 0.0)));
                let out = [
                    ic_c + Vec3::new(0.0, ic_h * 0.5 + 0.05, 0.0),
                    Vec3::new(ic_c.x + 0.1, ic_c.y + ic_h * 0.5 + 0.18, 0.0),
                    Vec3::new(lay.x_front - 0.12, plenum_c.y + 0.1, 0.04),
                    thr_in,
                ];
                pipe(cx, root, &out, 0.03, &m.silicone_blue);
            }
            intake_front = ic_c;
        }
        // blow-off valve on the charge pipe, just ahead of the throttle
        bov(cx, root, thr_c + Vec3::new(-0.10, 0.03, -0.03), Vec3::Y);
    } else {
        // naturally aspirated: an air filter on the throttle inlet
        match tuning.runners {
            Runners::Itb => {}
            _ => filter_assembly(cx, root, tuning.air_filter, thr_c + Vec3::new(-0.004, 0.0, 0.0), -Vec3::X, 0.036),
        }
        intake_front = thr_c - Vec3::X * 0.3;
        // exhaust collectors to the cat
        if colls.len() == 1 {
            let e = colls[0].end;
            let path = bezier(e, e + Vec3::new(0.1, 0.0, 0.0), Vec3::new(e.x + 0.3, y_pipe, z_ex), exhaust_start, 16);
            pipe(cx, root, &path, main_pipe_r, &hot_pipe);
            sub_common[0] = path;
        } else {
            let nc = colls.len();
            for (k, c) in colls.iter().enumerate() {
                let e = c.end;
                let off = (k as f32 - (nc as f32 - 1.0) * 0.5) * 0.02;
                let path = bezier(
                    e,
                    e + Vec3::new(0.12, 0.0, 0.0),
                    Vec3::new(e.x + 0.3, y_pipe + 0.02, z_ex + off * 3.0),
                    exhaust_start + Vec3::new(0.0, 0.0, off),
                    16,
                );
                pipe(cx, root, &path, main_pipe_r * 0.9, &hot_pipe);
                sub_common[k] = path;
            }
        }
    }

    // ---------------------------------------------------------------- cooling: radiator, fan, hoses, belts
    let neck = if multi_bank {
        Vec3::new(lay.x_front + 0.02, plenum_c.y - 0.01, 0.0)
    } else {
        Vec3::new(lay.x_front + 0.02, lay.deck + 0.03, 0.0)
    };
    accessories::build_cooling(cx, root, cfg, lay, rad_x, neck);
    accessories::build_belt_drive(cx, root, cfg, lay);

    // ---------------------------------------------------------------- cat, muffler, tail
    let y = y_pipe;
    let z = z_ex;
    let cat_x0 = d.x_cat - 0.35;
    let cat_x1 = d.x_cat + 0.05;
    let pipe_r = main_pipe_r;
    let flange_pipe = cx.mesh(flange_mesh(pipe_r * 1.55, 6));
    cx.spawn(root, MB::cylinder(pipe_r, exhaust_start.x, cat_x0, 22), &hot_pipe, Transform::from_xyz(0.0, y, z));
    // O2 sensor ahead of the catalyst
    o2_sensor(cx, root, Vec3::new(exhaust_start.x + 0.12, y + pipe_r * 0.7, z), Vec3::new(0.0, 1.0, 0.25));
    let cat_r = match tuning.cat {
        Cat::Stock => 0.064,
        Cat::HighFlow => 0.05,
        Cat::Delete => pipe_r,
    };
    match tuning.cat {
        Cat::Stock => {
            let can = MB::lathe(
                &[
                    (cat_x0, 0.0, false),
                    (cat_x0, pipe_r, false),
                    (cat_x0 + 0.05, cat_r.max(pipe_r * 1.2), true),
                    (cat_x1 - 0.05, cat_r.max(pipe_r * 1.2), true),
                    (cat_x1, pipe_r, false),
                    (cat_x1, 0.0, false),
                ],
                32,
            );
            cx.spawn(root, can, &hot_cat, Transform::from_xyz(0.0, y, z));
            for k in 0..3 {
                cx.spawn(root, MB::tube(0.066, 0.062, 0.0, 0.006, 32), &m.steel, Transform::from_xyz(cat_x0 + 0.07 + k as f32 * 0.07, y, z));
            }
            // heat shield plate with dimples
            cx.spawn(root, MB::cuboid(0.3, 0.004, 0.11), &m.stainless, Transform::from_xyz((cat_x0 + cat_x1) * 0.5, y + 0.074, z));
            for k in 0..5 {
                cx.spawn(root, MB::cylinder(0.01, 0.0, 0.007, 12).rot_to_y(), &m.steel, Transform::from_xyz(cat_x0 + 0.06 + k as f32 * 0.06, y + 0.076, z));
            }
        }
        Cat::HighFlow => {
            // slim metallic-substrate cat: polished body, weld seam and two bungs
            let can = MB::lathe(
                &[
                    (cat_x0 + 0.03, 0.0, false),
                    (cat_x0 + 0.03, pipe_r, false),
                    (cat_x0 + 0.08, cat_r, true),
                    (cat_x1 - 0.08, cat_r, true),
                    (cat_x1 - 0.03, pipe_r, false),
                    (cat_x1 - 0.03, 0.0, false),
                ],
                32,
            );
            cx.spawn(root, can, &m.polished, Transform::from_xyz(0.0, y, z));
            cx.spawn(root, MB::tube(cat_r * 1.02, cat_r * 0.97, 0.0, 0.01, 32), &m.ti_burnt, Transform::from_xyz((cat_x0 + cat_x1) * 0.5, y, z));
            cx.spawn(root, MB::cylinder(pipe_r, cat_x0, cat_x0 + 0.04, 22), &hot_pipe, Transform::from_xyz(0.0, y, z));
            cx.spawn(root, MB::cylinder(pipe_r, cat_x1 - 0.04, cat_x1, 22), &hot_pipe, Transform::from_xyz(0.0, y, z));
            o2_sensor(cx, root, Vec3::new(cat_x1 - 0.1, y + cat_r * 0.8, z), Vec3::new(0.0, 1.0, -0.25));
        }
        Cat::Delete => {
            // test pipe: two flanges and a straight sleeve
            cx.spawn(root, MB::cylinder(pipe_r, cat_x0, cat_x1, 22), &hot_cat, Transform::from_xyz(0.0, y, z));
            for xf in [cat_x0 + 0.02, cat_x1 - 0.02] {
                put_flange(cx, root, &flange_pipe, &m.hot_metal, Vec3::new(xf, y, z), Vec3::X);
            }
            cx.spawn(root, MB::tube(pipe_r * 1.14, pipe_r * 0.98, cat_x0 + 0.1, cat_x1 - 0.1, 22), &m.stainless, Transform::from_xyz(0.0, y, z));
        }
    }

    let (mu_half, mu_r) = match tuning.muffler {
        Muffler::Stock => (0.32, 0.085),
        Muffler::Sport => (0.27, 0.07),
        Muffler::Straight => (0.46, 0.058),
        Muffler::Delete => (0.0, 0.0),
    };
    let mu_x0 = d.x_muffler - mu_half;
    let mu_x1 = d.x_muffler + mu_half;
    cx.spawn(root, MB::cylinder(pipe_r * 0.95, cat_x1, mu_x0, 22), &m.steel, Transform::from_xyz(0.0, y, z));
    put_flange(cx, root, &flange_pipe, &m.steel, Vec3::new(cat_x1 + 0.02, y, z), Vec3::X);
    match tuning.muffler {
        Muffler::Stock => {
            let mu = MB::lathe(
                &[
                    (mu_x0, 0.0, false),
                    (mu_x0, pipe_r * 0.95, false),
                    (mu_x0 + 0.05, mu_r, true),
                    (mu_x1 - 0.05, mu_r, true),
                    (mu_x1, pipe_r * 0.95, false),
                    (mu_x1, 0.0, false),
                ],
                36,
            );
            cx.spawn(root, mu, &m.steel, Transform::from_xyz(0.0, y, z).with_scale(Vec3::new(1.0, 0.8, 1.0)));
            for k in 0..4 {
                cx.spawn(
                    root,
                    MB::tube(mu_r + 0.002, mu_r - 0.002, 0.0, 0.004, 36),
                    &m.polished,
                    Transform::from_xyz(mu_x0 + 0.1 + k as f32 * 0.14, y, z).with_scale(Vec3::new(1.0, 0.8, 1.0)),
                );
            }
        }
        Muffler::Sport => {
            // round chambered can with a polished band and a resonator ahead of it
            let mu = MB::lathe(
                &[
                    (mu_x0, 0.0, false),
                    (mu_x0, pipe_r * 0.95, false),
                    (mu_x0 + 0.035, mu_r, true),
                    (mu_x1 - 0.035, mu_r, true),
                    (mu_x1, pipe_r * 0.95, false),
                    (mu_x1, 0.0, false),
                ],
                40,
            );
            cx.spawn(root, mu, &m.stainless, Transform::from_xyz(0.0, y, z));
            cx.spawn(root, MB::tube(mu_r * 1.01, mu_r * 0.96, 0.0, 0.05, 40), &m.ti_burnt, Transform::from_xyz(d.x_muffler - 0.025, y, z));
            cx.spawn(root, MB::cylinder(0.04, mu_x0 - 0.2, mu_x0 - 0.04, 28), &m.stainless, Transform::from_xyz(0.0, y, z));
            cx.spawn(root, MB::tube(0.041, 0.037, 0.0, 0.01, 28), &m.polished, Transform::from_xyz(mu_x0 - 0.12, y, z));
        }
        Muffler::Straight => {
            // long straight-through canister with polished end caps and a visible perforated core band
            let mu = MB::lathe(
                &[
                    (mu_x0, 0.0, false),
                    (mu_x0, pipe_r * 0.95, false),
                    (mu_x0 + 0.03, mu_r, true),
                    (mu_x1 - 0.03, mu_r, true),
                    (mu_x1, pipe_r * 0.95, false),
                    (mu_x1, 0.0, false),
                ],
                40,
            );
            cx.spawn(root, mu, &m.polished, Transform::from_xyz(0.0, y, z));
            cx.spawn(root, MB::tube(mu_r * 1.015, mu_r * 0.97, 0.0, 0.12, 40), &m.carbon, Transform::from_xyz(d.x_muffler - 0.06, y, z));
            for k in 0..6 {
                cx.spawn(root, MB::tube(mu_r * 1.01, mu_r * 0.97, 0.0, 0.004, 40), &m.steel, Transform::from_xyz(mu_x0 + 0.1 + k as f32 * 0.14, y, z));
            }
        }
        Muffler::Delete => {
            cx.spawn(root, MB::cylinder(pipe_r * 0.95, mu_x0, mu_x1.max(mu_x0 + 0.5), 22), &m.steel, Transform::from_xyz(0.0, y, z));
        }
    }
    // rubber hangers: a strap from the pipe up and out to the frame rail (only where there is a rail)
    for xh in [cat_x1 + 0.25, cat_x0 - 0.5] {
        if xh < d.x_pinion - 0.2 && xh > lay.x_rear {
            let on_pipe = Vec3::new(xh, y + pipe_r + 0.012, z);
            let on_rail = Vec3::new(xh, -0.215, 0.5 - 0.03);
            cx.spawn(root, MB::cuboid(0.032, 0.026, 0.032), &m.rubber, Transform::from_translation(on_pipe));
            let dir = on_rail - on_pipe;
            let len = dir.length();
            cx.spawn(
                root,
                MB::cuboid(len, 0.004, 0.022),
                &m.steel,
                Transform::from_translation((on_pipe + on_rail) * 0.5).with_rotation(Quat::from_rotation_arc(Vec3::X, dir / len)),
            );
        }
    }

    // tail: single flared tip, dual tips, titanium slash tip or a bare pipe
    let tail_start = Vec3::new(if tuning.muffler == Muffler::Delete { mu_x1.max(mu_x0 + 0.5) } else { mu_x1 }, y, z);
    let tip_c = Vec3::new(d.x_end, y + 0.05, z + 0.08);
    let tail_r = pipe_r * 0.8;
    let tail = [tail_start, tail_start + Vec3::new(0.12, 0.0, 0.02), Vec3::new(tail_start.x + 0.3, y + 0.03, z + 0.06), tip_c];
    match tuning.muffler {
        Muffler::Sport => {
            for sz in [-1.0f32, 1.0] {
                let off = sz * 0.05;
                let t = [tail_start, tail_start + Vec3::new(0.12, 0.0, 0.02 + off * 0.5), Vec3::new(tail_start.x + 0.3, y + 0.03, z + 0.06 + off * 0.9), tip_c + Vec3::new(0.0, 0.0, off)];
                pipe(cx, root, &t, tail_r * 0.78, &m.stainless);
                let at = tip_c + Vec3::new(0.0, 0.0, off);
                cx.spawn(root, MB::tube(tail_r * 1.12, tail_r * 0.78, 0.0, 0.1, 30), &m.chrome, Transform::from_translation(at));
                cx.spawn(root, MB::tube(tail_r * 1.12, tail_r * 0.9, 0.0, 0.12, 30).translated(Vec3::ZERO), &m.chrome, Transform::from_translation(at));
            }
        }
        Muffler::Straight => {
            pipe(cx, root, &tail, tail_r * 1.1, &m.ti_burnt);
            cx.spawn(root, MB::tube(tail_r * 1.55, tail_r * 1.1, 0.0, 0.16, 36), &m.ti_burnt, Transform::from_translation(tip_c));
            cx.spawn(root, MB::tube(tail_r * 1.6, tail_r * 1.5, 0.12, 0.16, 36), &m.carbon, Transform::from_translation(tip_c));
        }
        Muffler::Delete => {
            pipe(cx, root, &tail, tail_r, &m.steel);
            cx.spawn(root, MB::tube(tail_r * 1.04, tail_r * 0.86, 0.0, 0.03, 26), &m.polished, Transform::from_translation(tip_c));
        }
        Muffler::Stock => {
            pipe(cx, root, &tail, tail_r, &m.steel);
            cx.spawn(root, MB::cone(tail_r, tail_r * 1.6, 0.0, 0.1, 28), &m.polished, Transform::from_translation(tip_c));
        }
    }

    // main line from the merge point to the tail tip, shared by every cylinder
    let mut main_line: Vec<Vec3> = vec![
        exhaust_start,
        Vec3::new(cat_x0, y, z),
        Vec3::new(cat_x1, y, z),
        Vec3::new(mu_x0, y, z),
        Vec3::new(mu_x1, y, z),
    ];
    main_line.extend(spline(&tail, 5));
    let tail_tip = tip_c;
    let mut exhaust_paths: Vec<Vec<Vec3>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut p = runner_paths[i].clone();
        let si = subs.iter().position(|s| s.cyls.contains(&i)).unwrap_or(0);
        p.extend(sub_common[si].iter().cloned());
        p.extend(main_line.iter().cloned());
        exhaust_paths.push(p);
    }

    PlumbingOut {
        intake_paths,
        exhaust_paths,
        tail_tip,
        turbo_center,
        intake_front,
        muffler_center: Vec3::new(d.x_muffler, y, z),
        throttle_center: thr_c,
        cat_center: Vec3::new(d.x_cat, y, z),
        n_turbos,
        front_x: if turbo && tuning.intercooler != Intercooler::None { ic_c.x - 0.12 } else { rad_x - 0.14 },
        plenum_c,
        rad_x,
        intercooler: (turbo && tuning.intercooler != Intercooler::None).then_some(ic_c),
        fuel_rail: fuel_rail_pt,
    }
}
