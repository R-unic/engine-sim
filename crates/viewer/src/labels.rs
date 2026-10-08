//! Named parts in the 3D view. The rig drops invisible [`PartTag`] markers on the parts worth naming;
//! each frame the ones near what the camera is looking at are projected to the screen, laid out so
//! they do not overlap, and drawn as labels. Hover one for a description with live values from the
//! simulation, click it to pin a card, double-click to fly the camera to it.

use crate::cam::CamRig;
use crate::cycle::{stroke_color, stroke_name, stroke_of, valve_events};
use crate::help;
use crate::rig::anim::{local_angle, valve_open_amount, RigGeom};
use crate::sim::Sim;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, FontId, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};
use engine_core::acoustics::CylGeom;
use engine_core::StrokePhase;

const ACCENT: Color32 = Color32::from_rgb(255, 150, 40);
const DIM: Color32 = Color32::from_rgb(150, 155, 165);

/// An invisible marker on the rig: where a part is, and what it is (a key into `help::part`).
#[derive(Component)]
pub struct PartTag {
    pub key: &'static str,
    /// For per-cylinder parts, which cylinder (so the selected one can be preferred).
    pub ci: Option<usize>,
}

/// One tag after projection to the screen.
#[derive(Clone)]
pub struct Projected {
    pub key: &'static str,
    pub ci: Option<usize>,
    pub world: Vec3,
    /// egui points.
    pub screen: Pos2,
    /// Distance from the camera, m.
    pub depth: f32,
}

/// Projects every tag the camera can see into egui points (`zoom` is egui's zoom factor).
pub fn project(camera: (&Camera, &GlobalTransform), tags: &Query<(&PartTag, &GlobalTransform)>, zoom: f32) -> Vec<Projected> {
    let (cam, cam_tf) = camera;
    let mut out = Vec::new();
    for (tag, tf) in tags.iter() {
        let world = tf.translation();
        if let Ok(p) = cam.world_to_viewport(cam_tf, world) {
            out.push(Projected { key: tag.key, ci: tag.ci, world, screen: Pos2::new(p.x / zoom, p.y / zoom), depth: world.distance(cam_tf.translation()) });
        }
    }
    out
}

/// What the player has pinned.
#[derive(Clone, Copy)]
pub struct Pinned {
    pub key: &'static str,
    pub ci: Option<usize>,
    pub world: Vec3,
}

/// Rough priority: larger = more worth naming when space is short.
fn priority(key: &str) -> i32 {
    match key {
        "piston" | "intake_valve" | "exhaust_valve" | "crank" | "camshaft" | "plug" | "injector" | "turbo" | "throttle" | "clutch" | "gearbox" | "diff" | "cat" | "muffler" => 3,
        "cylinder" | "conrod" | "header" | "flywheel" | "driveshaft" | "wheel" | "intercooler" | "plenum" | "radiator" | "o2" | "tailpipe" => 2,
        _ => 1,
    }
}

/// The camera distance beyond which a part is too small on screen to be worth a label.
fn visible_until(key: &str) -> f32 {
    match key {
        "piston" | "conrod" | "cylinder" | "intake_valve" | "exhaust_valve" | "camshaft" | "plug" | "coil" | "injector" => 1.6,
        "crank" | "damper" | "oil_pan" | "fuel_rail" | "throttle" | "plenum" | "filter" | "header" | "starter" | "alternator" | "ac" | "waterpump" | "belt" | "flywheel" | "clutch" => 3.0,
        _ => f32::MAX,
    }
}

fn is_internal(key: &str) -> bool {
    matches!(key, "piston" | "conrod" | "cylinder" | "intake_valve" | "exhaust_valve" | "camshaft" | "plug" | "injector" | "coil" | "crank")
}

fn label_size(ctx: &egui::Context, text: &str) -> Vec2 {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), FontId::proportional(11.5), Color32::WHITE));
    galley.size() + Vec2::new(14.0, 7.0)
}

/// Lays out and draws the labels. Returns what was clicked (to pin) and double-clicked (to zoom to).
pub fn draw(
    ctx: &egui::Context,
    projected: &[Projected],
    sim: &Sim,
    geom: Option<&RigGeom>,
    cam: &CamRig,
    sel_cyl: usize,
    blocked: &[Rect],
    pinned: &mut Option<Pinned>,
    fly_to: &mut Option<Vec3>,
) {
    let screen = ctx.content_rect();
    let focus = cam.target;
    let reach = (cam.dist * 0.8).max(0.4);
    // zoomed right in, only the parts of the combustion chamber and valvetrain are worth naming
    let close = cam.dist < 1.0;

    // the best tag per key: the selected cylinder's if there is one, otherwise the one nearest the focus
    let mut best: Vec<&Projected> = Vec::new();
    for p in projected {
        if !screen.shrink(8.0).contains(p.screen) || blocked.iter().any(|r| r.contains(p.screen)) {
            continue;
        }
        if p.world.distance(focus) > reach || (close && !is_internal(p.key)) || cam.dist > visible_until(p.key) {
            continue;
        }
        let rank = |q: &Projected| -> (bool, f32) { (q.ci == Some(sel_cyl), -q.world.distance(focus)) };
        match best.iter().position(|b| b.key == p.key) {
            Some(i) => {
                let (a, b) = (rank(p), rank(best[i]));
                if a.0 > b.0 || (a.0 == b.0 && a.1 > b.1) {
                    best[i] = p;
                }
            }
            None => best.push(p),
        }
    }
    best.sort_by(|a, b| {
        priority(b.key).cmp(&priority(a.key)).then(a.world.distance(focus).partial_cmp(&b.world.distance(focus)).unwrap())
    });
    best.truncate(18);

    let center = screen.center();
    let mut placed: Vec<Rect> = Vec::new();
    let layer = egui::LayerId::new(Order::Background, Id::new("part_labels"));
    let painter = ctx.layer_painter(layer);
    let mut hits: Vec<(Rect, &Projected, Id)> = Vec::new();

    for p in &best {
        let Some(info) = help::part(p.key) else { continue };
        let size = label_size(ctx, info.name);
        // push the label away from the middle of the screen so leader lines do not cross what is being looked at
        let out = {
            let d = p.screen - center;
            if d.length() < 1.0 { Vec2::new(1.0, -1.0).normalized() } else { d.normalized() }
        };
        let mut spot = None;
        'search: for dist in [34.0f32, 56.0, 84.0, 118.0] {
            for turn in [0.0f32, 0.5, -0.5, 1.0, -1.0, 1.5, -1.5, 2.0, -2.0, 2.6, -2.6, 3.14] {
                let (s, c) = turn.sin_cos();
                let dir = Vec2::new(out.x * c - out.y * s, out.x * s + out.y * c);
                let target = p.screen + dir * dist;
                let rect = Rect::from_center_size(target, size);
                let rect = if dir.x >= 0.0 { rect.translate(Vec2::new(size.x * 0.5 - 6.0, 0.0)) } else { rect.translate(Vec2::new(-size.x * 0.5 + 6.0, 0.0)) };
                if !screen.shrink(4.0).contains_rect(rect) {
                    continue;
                }
                if placed.iter().any(|r| r.expand(3.0).intersects(rect)) || blocked.iter().any(|r| r.intersects(rect)) {
                    continue;
                }
                spot = Some(rect);
                break 'search;
            }
        }
        let Some(rect) = spot else { continue };
        placed.push(rect);
        let near = rect.clamp(p.screen);
        painter.line_segment([p.screen, near], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 190, 110, 160)));
        painter.circle_filled(p.screen, 3.0, ACCENT);
        painter.circle_stroke(p.screen, 4.5, Stroke::new(1.0, Color32::from_black_alpha(160)));
        hits.push((rect, p, Id::new(("part_label", p.key))));
    }

    // drawn in one Area so clicks and hovers work on every pill
    egui::Area::new(Id::new("part_label_hits")).order(Order::Background).fixed_pos(Pos2::ZERO).interactable(true).show(ctx, |ui| {
        ui.set_clip_rect(screen);
        for (rect, p, id) in &hits {
            let resp = ui.interact(*rect, *id, Sense::click());
            let is_pinned = pinned.map_or(false, |q| q.key == p.key);
            let hot = resp.hovered() || is_pinned;
            let name = help::part(p.key).map_or(p.key, |i| i.name);
            ui.painter().rect_filled(*rect, 6.0, if hot { Color32::from_rgba_unmultiplied(60, 38, 14, 235) } else { Color32::from_rgba_unmultiplied(10, 12, 18, 215) });
            ui.painter().rect_stroke(*rect, 6.0, Stroke::new(1.0, if hot { ACCENT } else { Color32::from_rgb(90, 80, 70) }), egui::StrokeKind::Inside);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, name, FontId::proportional(11.5), if hot { Color32::WHITE } else { Color32::from_gray(225) });
            if resp.clicked() {
                *pinned = Some(Pinned { key: p.key, ci: p.ci, world: p.world });
            }
            if resp.double_clicked() {
                *fly_to = Some(p.world);
            }
            let (key, ci) = (p.key, p.ci);
            resp.on_hover_ui(|ui| part_card(ui, key, ci, sim, geom, sel_cyl, false));
        }
    });
}

/// The description of a part and what it is doing right now.
pub fn part_card(ui: &mut egui::Ui, key: &'static str, ci: Option<usize>, sim: &Sim, geom: Option<&RigGeom>, sel_cyl: usize, full: bool) {
    let Some(info) = help::part(key) else { return };
    ui.set_max_width(330.0);
    ui.label(egui::RichText::new(info.name).strong().size(14.0).color(ACCENT));
    ui.label(egui::RichText::new(info.role).italics());
    let lines = live_lines(key, ci.unwrap_or(sel_cyl), sim, geom);
    if !lines.is_empty() {
        ui.add_space(3.0);
        for l in lines {
            ui.label(egui::RichText::new(l).monospace().size(11.5).color(Color32::from_rgb(120, 220, 205)));
        }
    }
    ui.add_space(3.0);
    ui.label(info.detail);
    if !full {
        ui.add_space(2.0);
        ui.colored_label(DIM, "click to pin this card, double-click to zoom in");
    }
}

/// Live numbers for a part, straight from the sim.
pub fn live_lines(key: &str, ci: usize, sim: &Sim, geom: Option<&RigGeom>) -> Vec<String> {
    let t = &sim.tel;
    let cfg = sim.cfg();
    let ci = ci.min(sim.tables.len().saturating_sub(1));
    let cyl_stats = |geom: Option<&RigGeom>| -> Option<(f32, StrokePhase, f32)> {
        geom?;
        let th = local_angle(sim, ci);
        Some((th, stroke_of(th), sim.tables[ci].pressure_at(th) / 1e5))
    };
    let kph = t.vehicle_speed_mps * 3.6;
    let wheel_rpm = t.vehicle_speed_mps / engine_core::constants::WHEEL_RADIUS_M * 60.0 / std::f64::consts::TAU;
    let final_drive = sim.transmissions[sim.trans_idx].final_drive_ratio;
    let diag = sim.audio.lock().map(|a| a.diag.clone()).unwrap_or_default();
    // the exhaust figures come from the sound solver: nothing to say until it has built its model
    if diag.cells == 0 && matches!(key, "header" | "muffler" | "tailpipe") {
        return vec![];
    }
    match key {
        "piston" => {
            let cg = CylGeom::new(cfg);
            match cyl_stats(geom) {
                Some((th, ph, p)) => {
                    let frac = (cg.volume(th) - cg.clearance) / (cg.piston_area * cg.stroke);
                    let force_kn = (p * 1e5 - t.atmospheric_pressure_pa as f32) * cg.piston_area / 1000.0;
                    vec![
                        format!("cylinder {}: {} stroke", ci + 1, stroke_name(ph).to_lowercase()),
                        format!("{:.0}% of the way down its {:.0} mm stroke", frac * 100.0, cfg.stroke_meters * 1000.0),
                        format!("gas force on the crown {:+.1} kN", force_kn),
                    ]
                }
                None => vec![],
            }
        }
        "conrod" | "cylinder" => match cyl_stats(geom) {
            Some((th, ph, p)) => {
                let mut v = vec![format!("cylinder {}: {} stroke, {:.0} deg", ci + 1, stroke_name(ph).to_lowercase(), th), format!("pressure {:.1} bar", p)];
                if key == "cylinder" {
                    v.push(format!("compression ratio {:.1} : 1, {:.0} cc", cfg.compression_ratio, cfg.displacement_l() * 1000.0 / cfg.cylinder_count as f64));
                }
                v
            }
            None => vec![],
        },
        "intake_valve" | "exhaust_valve" => {
            let Some(g) = geom else { return vec![] };
            let intake = key == "intake_valve";
            let ev = valve_events(g);
            let th = local_angle(sim, ci);
            let (open, close) = if intake {
                (format!("opens {:.0} deg before TDC", 720.0 - ev.ivo), format!("closes {:.0} deg after BDC", ev.ivc - 180.0))
            } else {
                (format!("opens {:.0} deg before BDC", 540.0 - ev.evo), format!("closes {:.0} deg after TDC", ev.evc))
            };
            vec![format!("cylinder {}: {:.0}% open", ci + 1, valve_open_amount(g, th, intake) * 100.0), open, close]
        }
        "camshaft" => vec![format!("turning at {:.0} rpm (half the crank's {:.0})", t.rpm / 2.0, t.rpm)],
        "crank" | "damper" | "flywheel" => vec![format!("{:.0} rpm   {:.0} Nm   {:.0} hp", t.rpm, t.display_torque_nm, t.display_horsepower)],
        "plug" => vec![
            format!("fires {:.0} deg before TDC", cfg.ignition_advance_deg),
            format!("spark rate {:.0} sparks/s", t.rpm / 120.0 * cfg.cylinder_count as f64),
        ],
        "injector" => vec![format!("injects near TDC, {:.0} sprays/s across the engine", t.rpm / 120.0 * cfg.cylinder_count as f64)],
        "coil" => vec![],
        "throttle" => vec![
            format!("plate {:.0}% open", t.throttle * 100.0),
            format!("manifold {:.0} kPa ({:+.2} bar)", t.manifold_pressure_pa / 1000.0, (t.manifold_pressure_pa - t.atmospheric_pressure_pa) / 1e5),
        ],
        "plenum" => vec![format!("{:.0} kPa", t.manifold_pressure_pa / 1000.0)],
        "filter" => vec![format!("air at {:.0} C", t.charge_temp_k - 273.15)],
        "fuel_rail" => vec![format!("{:.1} L/h being burnt, AFR {:.1}", t.fuel_consumption_l_per_hour, t.air_fuel_ratio)],
        "header" => vec![
            format!("gas {:.0} C at the headers", diag.header_gas_k - 273.15),
            format!("primaries {:.2} m, tuned to about {:.0} rpm", diag.primary_m, engine_core::Tuning::tuned_rpm(cfg, diag.primary_m as f64)),
        ],
        "turbo" => vec![
            format!("spool {:.0}%   boost {:.2} / {:.2} bar", t.turbo_spool_fraction * 100.0, t.boost_pressure_pa / 1e5, t.boost_target_pa / 1e5),
            format!("charge temperature {:.0} C", t.charge_temp_k - 273.15),
        ],
        "intercooler" => vec![format!("charge air {:.0} C after cooling", t.charge_temp_k - 273.15)],
        "o2" => vec![
            format!("reads AFR {:.1}, fuel trim {:+.1}", t.exhaust.o2_reported_afr, t.exhaust.fuel_trim_afr),
            (if t.exhaust.closed_loop_active { "closed loop: ECU is trimming fuel" } else { "open loop: fixed fuel map" }).to_string(),
        ],
        "cat" => vec![
            format!("{:.0} C, {}", t.exhaust.catalyst_temp_k - 273.15, if t.exhaust.catalyst_lit_off { "lit off (working)" } else { "not lit off yet" }),
            format!("clogged {:.0}%", t.exhaust.catalyst_clog_fraction * 100.0),
        ],
        "muffler" | "tailpipe" => vec![format!("gas {:.0} C at the tip", diag.tail_gas_k - 273.15), format!("pipe drone about {:.0} Hz", diag.drone_hz)],
        "clutch" => vec![format!("clamped {:.0}%", t.clutch_engagement * 100.0)],
        "starter" => vec![(if t.is_cranking { "engaged: cranking" } else { "idle" }).to_string()],
        "gearbox" => vec![format!("gear {}   ratio {:.2} : 1", t.gear_label, t.gear_ratio), format!("overall {:.2} : 1 with the final drive", t.total_gear_ratio)],
        "driveshaft" => vec![format!("{:.0} rpm", wheel_rpm * final_drive)],
        "diff" => vec![format!("final drive {:.2} : 1", final_drive), format!("axles {:.0} rpm", wheel_rpm)],
        "wheel" => vec![format!("{:.0} km/h ({:.0} mph), {:.0} rpm", kph, kph / 1.609, wheel_rpm)],
        "brake" => vec![format!("pedal {:.0}%", t.brake * 100.0)],
        "radiator" => vec![format!("coolant {:.0} C", t.coolant_temp_k - 273.15)],
        _ => vec![],
    }
}

/// The pinned card, as a small draggable window.
pub fn pinned_window(ctx: &egui::Context, pinned: &mut Option<Pinned>, sim: &Sim, geom: Option<&RigGeom>, sel_cyl: usize, fly_to: &mut Option<Vec3>) {
    let Some(p) = *pinned else { return };
    let mut open = true;
    let name = help::part(p.key).map_or("PART", |i| i.name);
    egui::Window::new(name.to_uppercase())
        .id(Id::new("pinned_part"))
        .open(&mut open)
        .default_pos(Pos2::new(ctx.content_rect().right() - 380.0, 330.0))
        .resizable(false)
        .show(ctx, |ui| {
            part_card(ui, p.key, p.ci, sim, geom, sel_cyl, true);
            ui.add_space(4.0);
            if ui.button("Zoom camera to it").clicked() {
                *fly_to = Some(p.world);
            }
        });
    if !open {
        *pinned = None;
    }
}

/// Small numbered badges over every cylinder, coloured by what each is doing: the 3D counterpart of the firing strip.
pub fn cylinder_badges(ctx: &egui::Context, projected: &[Projected], sim: &Sim, blocked: &[Rect], sel: &mut usize) {
    let screen = ctx.content_rect();
    let layer = egui::LayerId::new(Order::Background, Id::new("cyl_badges"));
    let painter = ctx.layer_painter(layer);
    egui::Area::new(Id::new("cyl_badge_hits")).order(Order::Background).fixed_pos(Pos2::ZERO).interactable(true).show(ctx, |ui| {
        ui.set_clip_rect(screen);
        for p in projected.iter().filter(|p| p.key == "cyl_badge") {
            let Some(ci) = p.ci else { continue };
            if ci >= sim.tables.len() || !screen.contains(p.screen) || blocked.iter().any(|r| r.contains(p.screen)) {
                continue;
            }
            let ph = stroke_of(local_angle(sim, ci));
            let col = stroke_color(ph);
            let r = Rect::from_center_size(p.screen, Vec2::splat(if *sel == ci { 24.0 } else { 19.0 }));
            let resp = ui.interact(r, Id::new(("cyl_badge", ci)), Sense::click());
            painter.circle_filled(r.center(), r.width() * 0.5, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 225));
            painter.circle_stroke(r.center(), r.width() * 0.5, Stroke::new(if *sel == ci { 2.5 } else { 1.0 }, if *sel == ci { Color32::WHITE } else { Color32::from_black_alpha(160) }));
            painter.text(r.center(), Align2::CENTER_CENTER, format!("{}", ci + 1), FontId::proportional(11.0), Color32::BLACK);
            if resp.clicked() {
                *sel = ci;
            }
            resp.on_hover_text(format!("Cylinder {}: {} stroke. Click to follow it in the cycle panel.", ci + 1, stroke_name(ph).to_lowercase()));
        }
    });
}

/// Hover a part in the 3D view (with the labels off) to see what it is: the nearest tagged part within a few
/// pixels of the pointer gets a ring and the same card the labels show. Click pins it, double-click zooms.
#[allow(clippy::too_many_arguments)]
pub fn hover_pick(
    ctx: &egui::Context,
    projected: &[Projected],
    sim: &Sim,
    geom: Option<&RigGeom>,
    cam: &CamRig,
    sel_cyl: usize,
    blocked: &[Rect],
    pinned: &mut Option<Pinned>,
    fly_to: &mut Option<Vec3>,
    fake_pointer: Option<(f32, f32)>,
) {
    if fake_pointer.is_none() && (ctx.is_pointer_over_egui() || ctx.input(|i| i.pointer.any_down())) {
        return;
    }
    let Some(pos) = fake_pointer.map(|(x, y)| Pos2::new(x, y)).or_else(|| ctx.input(|i| i.pointer.hover_pos())) else { return };
    if blocked.iter().any(|r| r.contains(pos)) {
        return;
    }
    let mut best: Option<(&Projected, f32)> = None;
    for p in projected {
        if help::part(p.key).is_none() || cam.dist > visible_until(p.key) {
            continue;
        }
        let d = p.screen.distance(pos);
        if d > 26.0 {
            continue;
        }
        // nearest on screen, and of two close ones the one nearer the camera
        let score = d + p.depth * 6.0 + if p.ci == Some(sel_cyl) { -6.0 } else { 0.0 };
        if best.map_or(true, |b| score < b.1) {
            best = Some((p, score));
        }
    }
    let Some((p, _)) = best else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("hover_pick_ring")));
    painter.circle_stroke(p.screen, 10.0, Stroke::new(2.0, ACCENT));
    painter.circle_filled(p.screen, 3.0, ACCENT);
    egui::Area::new(Id::new("hover_card")).order(Order::Tooltip).fixed_pos(pos + Vec2::new(18.0, 14.0)).interactable(false).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| part_card(ui, p.key, p.ci, sim, geom, sel_cyl, false));
    });
    if ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
        *fly_to = Some(p.world);
    } else if ctx.input(|i| i.pointer.primary_clicked()) {
        *pinned = Some(Pinned { key: p.key, ci: p.ci, world: p.world });
    }
}
