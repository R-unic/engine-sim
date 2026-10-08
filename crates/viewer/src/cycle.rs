//! The four-stroke teaching views: a 720-degree timeline (cylinder pressure, strokes, valve events,
//! spark, burn), the P-V loop with its work and pumping loss, the firing-order strip, the energy
//! balance and a few engineering numbers. All of it is read from the same tables, valve timing and
//! telemetry that drive the 3D rig and the sound, so the diagrams cannot disagree with the engine.

use crate::rig::anim::{local_angle, valve_open_amount, RigGeom};
use crate::sim::{CycleTable, Sim};
use bevy_egui::egui::{self, epaint::Mesh, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2};
use engine_core::acoustics::CylGeom;
use engine_core::{IgnitionType, StrokePhase};

const BG: Color32 = Color32::from_rgb(8, 9, 13);
const GRID: Color32 = Color32::from_rgb(30, 34, 46);
const DIM: Color32 = Color32::from_rgb(150, 155, 165);
const INTAKE_BLUE: Color32 = Color32::from_rgb(90, 165, 255);
const EXHAUST_ORANGE: Color32 = Color32::from_rgb(255, 150, 70);
const SPARK_YELLOW: Color32 = Color32::from_rgb(255, 232, 110);

pub fn stroke_of(local_deg: f32) -> StrokePhase {
    let d = local_deg.rem_euclid(720.0);
    if d < 180.0 {
        StrokePhase::Intake
    } else if d < 360.0 {
        StrokePhase::Compression
    } else if d < 540.0 {
        StrokePhase::Power
    } else {
        StrokePhase::Exhaust
    }
}

pub fn stroke_color(p: StrokePhase) -> Color32 {
    match p {
        StrokePhase::Intake => INTAKE_BLUE,
        StrokePhase::Compression => Color32::from_rgb(240, 190, 70),
        StrokePhase::Power => Color32::from_rgb(255, 92, 62),
        StrokePhase::Exhaust => Color32::from_rgb(150, 160, 185),
    }
}

pub fn stroke_name(p: StrokePhase) -> &'static str {
    match p {
        StrokePhase::Intake => "INTAKE",
        StrokePhase::Compression => "COMPRESSION",
        StrokePhase::Power => "POWER",
        StrokePhase::Exhaust => "EXHAUST",
    }
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// The valve events of the cam the rig and the sound model share, as engineers quote them.
#[derive(Clone, Copy, Debug)]
pub struct ValveEvents {
    /// Local crank angles (0..720) at which each valve starts to open / is fully closed.
    pub ivo: f32,
    pub ivc: f32,
    pub evo: f32,
    pub evc: f32,
    /// Degrees where both are open around top dead centre.
    pub overlap: f32,
}

pub fn valve_events(g: &RigGeom) -> ValveEvents {
    let ivo = (g.int_peak_deg - g.int_dur_deg * 0.5).rem_euclid(720.0);
    let ivc = (g.int_peak_deg + g.int_dur_deg * 0.5).rem_euclid(720.0);
    let evo = (g.exh_peak_deg - g.exh_dur_deg * 0.5).rem_euclid(720.0);
    let evc = (g.exh_peak_deg + g.exh_dur_deg * 0.5).rem_euclid(720.0);
    let overlap = if ivo > 360.0 && evc < 360.0 { (720.0 - ivo) + evc } else { 0.0 };
    ValveEvents { ivo, ivc, evo, evc, overlap }
}

/// Work done by the gas on one piston over one cycle, from the recorded pressure table.
#[derive(Clone, Copy, Debug, Default)]
pub struct Work {
    /// Compression + expansion (180..540 degrees), J.
    pub gross_j: f32,
    /// Intake + exhaust (the gas exchange), J; negative when the engine has to pump.
    pub pumping_j: f32,
    pub net_j: f32,
    /// Net work over swept volume, bar.
    pub imep_bar: f32,
}

pub fn cycle_work(t: &CycleTable, g: &CylGeom) -> Work {
    let (mut gross, mut pump) = (0.0f64, 0.0f64);
    for i in 0..720usize {
        let dv = (g.volume((i + 1) as f32) - g.volume(i as f32)) as f64;
        let p = 0.5 * (t.pressure[i] + t.pressure[(i + 1) % 720]) as f64;
        if (180..540).contains(&i) {
            gross += p * dv;
        } else {
            pump += p * dv;
        }
    }
    let vd = (g.piston_area * g.stroke) as f64;
    Work { gross_j: gross as f32, pumping_j: pump as f32, net_j: (gross + pump) as f32, imep_bar: ((gross + pump) / vd / 1e5) as f32 }
}

// ------------------------------------------------------------------------------------------ caption

pub struct Caption {
    pub title: String,
    pub color: Color32,
    pub body: String,
    pub facts: String,
}

/// What the selected cylinder is doing at this instant, in words.
pub fn caption(sim: &Sim, g: &RigGeom, ci: usize) -> Caption {
    let cfg = sim.cfg();
    let th = local_angle(sim, ci);
    let phase = stroke_of(th);
    let table = &sim.tables[ci];
    let p_bar = table.pressure_at(th) / 1e5;
    let diesel = cfg.ignition_type == IgnitionType::Compression;
    let ev = valve_events(g);
    let t = &sim.tel;
    let title = format!("{} STROKE   {:.0} deg", stroke_name(phase), th);
    let color = stroke_color(phase);
    let manifold_kpa = (t.manifold_pressure_pa / 1000.0) as f32;
    let density_pct = (t.manifold_pressure_pa / t.atmospheric_pressure_pa.max(1.0) * 100.0) as f32;
    let (body, facts) = match phase {
        StrokePhase::Intake => {
            let vac = if diesel {
                "A diesel has no throttle, so it breathes freely."
            } else if t.throttle < 0.25 && density_pct < 90.0 {
                "The throttle is nearly shut, so the piston is pulling against a vacuum."
            } else if density_pct > 105.0 {
                "The turbo is packing the cylinder with air above atmospheric pressure."
            } else {
                "The throttle is open, so the cylinder fills almost completely."
            };
            (
                format!(
                    "The piston travels down with the intake valve open, so the pressure in the cylinder falls below the manifold's and fresh air rushes in{}. The valve opens before the top and closes after the bottom because the moving air keeps flowing. {}",
                    if diesel { "" } else { " along with fuel from the port injector" },
                    vac
                ),
                format!(
                    "intake valve {:.0}% open  |  manifold {:.0} kPa = {:.0}% of atmospheric density",
                    valve_open_amount(g, th, true) * 100.0,
                    manifold_kpa,
                    density_pct
                ),
            )
        }
        StrokePhase::Compression => {
            let ignite = if diesel {
                "Fuel is injected near the top and ignites by itself in the hot air; there is no spark plug.".to_string()
            } else {
                format!("The spark fires {:.0} degrees before top dead centre, so the burn peaks just after the top.", cfg.ignition_advance_deg)
            };
            (
                format!(
                    "Both valves are shut and the piston rises, squeezing the charge to {:.0}% of its volume ({:.1} to 1). Squeezing heats it and the pressure climbs steeply. {}",
                    100.0 / cfg.compression_ratio,
                    cfg.compression_ratio,
                    ignite
                ),
                format!("{:.1} bar  |  {:.0} deg to top dead centre", p_bar, 360.0 - th),
            )
        }
        StrokePhase::Power => {
            let (pk, pk_deg) = (360..540).map(|d| (table.pressure[d], d)).fold((0.0f32, 360), |a, b| if b.0 > a.0 { b } else { a });
            let missed = (330..420).any(|d| table.flags[d] & 2 != 0) || (th > 420.0 && table.burn[th as usize % 720] < 0.05);
            if !t.is_running && pk < 4.0e5 {
                (
                    "The engine is off, so nothing burns: the piston just compresses the air and lets it push back. Start the engine (hold E) to see the real cycle with combustion.".to_string(),
                    format!("{:.1} bar now  |  no combustion", p_bar),
                )
            } else if missed {
                (
                    "MISFIRE: there was no spark, so the charge does not burn. The piston gets almost no push and the pressure falls back along the way it came up. Cut the engine's torque like this on one cylinder and the whole engine runs rough.".to_string(),
                    format!("{:.1} bar  |  peak this cycle only {:.0} bar", p_bar, pk / 1e5),
                )
            } else {
                (
                    format!(
                        "The charge burns and the pressure peaks at {:.0} bar about {:.0} degrees after top dead centre. The expanding gas drives the piston down: this is the only stroke that makes work, and the other three each cost a little. Both valves are still shut.",
                        pk / 1e5,
                        pk_deg as f32 - 360.0
                    ),
                    format!("{:.1} bar now  |  charge burnt {:.0}%", p_bar, table.burn_at(th) * 100.0),
                )
            }
        }
        StrokePhase::Exhaust => (
            format!(
                "The exhaust valve opens {:.0} degrees before the bottom, so the still-hot, pressurised gas blows out on its own; then the rising piston pushes the rest out. Near the top the intake valve opens before the exhaust valve has closed: these {:.0} degrees of overlap let the leaving exhaust help draw in the next charge.",
                540.0 - ev.evo,
                ev.overlap
            ),
            format!("exhaust valve {:.0}% open  |  {:.2} bar (back-pressure)", valve_open_amount(g, th, false) * 100.0, p_bar),
        ),
    };
    Caption { title, color, body, facts }
}

// ------------------------------------------------------------------------------------------ helpers

fn fill_between(painter: &egui::Painter, top: &[Pos2], bottom: &[Pos2], color: Color32) {
    if top.len() < 2 || top.len() != bottom.len() {
        return;
    }
    let mut mesh = Mesh::default();
    for (t, b) in top.iter().zip(bottom) {
        mesh.colored_vertex(*t, color);
        mesh.colored_vertex(*b, color);
    }
    for i in 0..top.len() as u32 - 1 {
        let k = i * 2;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(Shape::mesh(mesh));
}

fn nice_step(max: f32) -> f32 {
    for s in [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0] {
        if max / s <= 6.0 {
            return s;
        }
    }
    100.0
}

// ------------------------------------------------------------------------------------------ timeline

/// The 720-degree timeline of one cylinder. Dragging across it turns the crank by hand and pauses.
pub fn timeline(ui: &mut egui::Ui, sim: &mut Sim, g: &RigGeom, ci: usize, width: f32) {
    let height = 164.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, BG);
    let plot = Rect::from_min_max(rect.min + Vec2::new(32.0, 8.0), rect.max - Vec2::new(8.0, 18.0));
    let x_of = |deg: f32| plot.left() + deg / 720.0 * plot.width();
    let pz = Rect::from_min_size(plot.min, Vec2::new(plot.width(), plot.height() * 0.5));
    let bz = Rect::from_min_size(Pos2::new(plot.left(), pz.bottom() + 2.0), Vec2::new(plot.width(), 14.0));
    let lane_h = (plot.bottom() - bz.bottom() - 6.0) * 0.5;
    let iz = Rect::from_min_size(Pos2::new(plot.left(), bz.bottom() + 3.0), Vec2::new(plot.width(), lane_h));
    let ez = Rect::from_min_size(Pos2::new(plot.left(), iz.bottom() + 3.0), Vec2::new(plot.width(), lane_h));

    let cfg = sim.cfg().clone();
    let table = sim.tables[ci].clone();
    let th_now = local_angle(sim, ci);
    let ev = valve_events(g);
    let diesel = cfg.ignition_type == IgnitionType::Compression;

    // stroke tint behind everything
    for s in 0..4 {
        let ph = stroke_of(s as f32 * 180.0 + 1.0);
        let r = Rect::from_min_max(Pos2::new(x_of(s as f32 * 180.0), pz.top()), Pos2::new(x_of((s + 1) as f32 * 180.0), ez.bottom()));
        painter.rect_filled(r, 0.0, with_alpha(stroke_color(ph), 14));
        let band = Rect::from_min_max(Pos2::new(r.left() + 1.0, bz.top()), Pos2::new(r.right() - 1.0, bz.bottom()));
        painter.rect_filled(band, 3.0, with_alpha(stroke_color(ph), 70));
        painter.text(band.center(), Align2::CENTER_CENTER, stroke_name(ph), FontId::proportional(10.0), Color32::WHITE);
    }

    // pressure axis
    let pmax_bar = {
        let m = table.pressure.iter().cloned().fold(0.0f32, f32::max) / 1e5;
        let step = nice_step(m.max(8.0));
        (m.max(8.0) / step).ceil() * step
    };
    let step = nice_step(pmax_bar);
    let y_of = |pa: f32| pz.bottom() - (pa / 1e5 / pmax_bar).clamp(0.0, 1.0) * pz.height();
    let mut b = step;
    while b <= pmax_bar + 0.01 {
        let y = y_of(b * 1e5);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, GRID));
        painter.text(Pos2::new(plot.left() - 3.0, y), Align2::RIGHT_CENTER, format!("{b:.0}"), FontId::proportional(9.0), DIM);
        b += step;
    }
    painter.text(Pos2::new(rect.left() + 3.0, pz.top() - 1.0), Align2::LEFT_TOP, "bar", FontId::proportional(9.0), DIM);
    let y_atm = y_of(101_325.0);
    painter.extend(Shape::dashed_line(&[Pos2::new(plot.left(), y_atm), Pos2::new(plot.right(), y_atm)], Stroke::new(1.0, Color32::from_gray(90)), 3.0, 3.0));

    // overlap shading at the wrap-around (both valves open)
    if ev.overlap > 0.5 {
        let a = Rect::from_min_max(Pos2::new(x_of(ev.ivo), pz.top()), Pos2::new(plot.right(), ez.bottom()));
        let c = Rect::from_min_max(Pos2::new(plot.left(), pz.top()), Pos2::new(x_of(ev.evc), ez.bottom()));
        for r in [a, c] {
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 255, 170, 46));
        }
    }

    // burn fraction (Wiebe S-curve), faint
    let burn_top: Vec<Pos2> = (0..=360).map(|i| {
        let d = i as f32 * 2.0;
        Pos2::new(x_of(d), pz.bottom() - table.burn_at(d).clamp(0.0, 1.0) * pz.height() * 0.9)
    }).collect();
    painter.add(Shape::line(burn_top, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 190, 90, 70))));

    // pressure trace, filled and coloured per stroke
    for s in 0..4 {
        let a = s * 90;
        let top: Vec<Pos2> = (a..=a + 90).map(|i| Pos2::new(x_of(i as f32 * 2.0), y_of(table.pressure_at(i as f32 * 2.0)))).collect();
        let base: Vec<Pos2> = top.iter().map(|p| Pos2::new(p.x, pz.bottom())).collect();
        let col = stroke_color(stroke_of(s as f32 * 180.0 + 1.0));
        fill_between(&painter, &top, &base, with_alpha(col, 38));
        painter.add(Shape::line(top, Stroke::new(1.8, col)));
    }

    // spark / injection marker
    let spark_deg = 360.0 - cfg.ignition_advance_deg as f32;
    let sx = x_of(spark_deg);
    painter.line_segment([Pos2::new(sx, pz.top()), Pos2::new(sx, pz.bottom())], Stroke::new(1.0, with_alpha(SPARK_YELLOW, 120)));
    painter.add(Shape::convex_polygon(
        vec![Pos2::new(sx - 4.0, pz.top()), Pos2::new(sx + 4.0, pz.top()), Pos2::new(sx, pz.top() + 7.0)],
        SPARK_YELLOW,
        Stroke::NONE,
    ));
    painter.text(
        Pos2::new(sx - 6.0, pz.top() + 1.0),
        Align2::RIGHT_TOP,
        if diesel { "injection".to_string() } else { format!("spark {:.0} BTDC", cfg.ignition_advance_deg) },
        FontId::proportional(9.0),
        SPARK_YELLOW,
    );

    // valve lanes: lift curves
    for (zone, intake, col, name) in [(iz, true, INTAKE_BLUE, "intake valve"), (ez, false, EXHAUST_ORANGE, "exhaust valve")] {
        painter.rect_filled(zone, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 6));
        let top: Vec<Pos2> = (0..=360).map(|i| {
            let d = i as f32 * 2.0;
            Pos2::new(x_of(d), zone.bottom() - valve_open_amount(g, d, intake).clamp(0.0, 1.0) * (zone.height() - 1.0))
        }).collect();
        let base: Vec<Pos2> = top.iter().map(|p| Pos2::new(p.x, zone.bottom())).collect();
        fill_between(&painter, &top, &base, with_alpha(col, 90));
        painter.add(Shape::line(top, Stroke::new(1.3, col)));
        painter.text(Pos2::new(plot.left() + 3.0, zone.center().y), Align2::LEFT_CENTER, name, FontId::proportional(9.0), with_alpha(col, 230));
    }

    // axis: dead centres
    for k in 0..=4 {
        let d = k as f32 * 180.0;
        let x = x_of(d);
        painter.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom() + 3.0)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)));
        let (txt, al) = match k {
            0 => ("TDC", Align2::LEFT_TOP),
            4 => ("TDC", Align2::RIGHT_TOP),
            2 => ("TDC (fire)", Align2::CENTER_TOP),
            _ => ("BDC", Align2::CENTER_TOP),
        };
        painter.text(Pos2::new(x, plot.bottom() + 4.0), al, txt, FontId::proportional(9.0), DIM);
    }

    // cursor
    let cx = x_of(th_now);
    painter.line_segment([Pos2::new(cx, plot.top()), Pos2::new(cx, plot.bottom())], Stroke::new(1.5, Color32::WHITE));
    painter.circle_filled(Pos2::new(cx, y_of(table.pressure_at(th_now))), 4.0, Color32::WHITE);
    painter.circle_stroke(Pos2::new(cx, y_of(table.pressure_at(th_now))), 5.5, Stroke::new(1.0, Color32::BLACK));

    // interaction: drag to turn the crank by hand
    let at = |pos: Pos2| ((pos.x - plot.left()) / plot.width() * 720.0).clamp(0.0, 719.9);
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            sim.scrub_to(ci, at(pos));
        }
    }
    if let Some(pos) = resp.hover_pos() {
        let d = at(pos);
        let hx = x_of(d);
        painter.line_segment([Pos2::new(hx, plot.top()), Pos2::new(hx, plot.bottom())], Stroke::new(1.0, Color32::from_white_alpha(70)));
        let phase = stroke_of(d);
        resp.clone().on_hover_ui_at_pointer(|ui| {
            ui.colored_label(stroke_color(phase), format!("{} at {:.0} deg", stroke_name(phase), d));
            ui.label(format!("cylinder pressure {:.1} bar", table.pressure_at(d) / 1e5));
            ui.label(format!("intake valve {:.0}% open, exhaust {:.0}% open", valve_open_amount(g, d, true) * 100.0, valve_open_amount(g, d, false) * 100.0));
            ui.label(format!("burnt {:.0}%", table.burn_at(d) * 100.0));
            ui.colored_label(DIM, "click or drag to turn the crank by hand");
        });
    }
}

/// IVO / IVC / EVO / EVC in the notation engineers use, plus the overlap.
pub fn valve_event_text(g: &RigGeom) -> String {
    let e = valve_events(g);
    format!(
        "IVO {:.0} BTDC  IVC {:.0} ABDC  EVO {:.0} BBDC  EVC {:.0} ATDC  overlap {:.0} deg",
        720.0 - e.ivo,
        e.ivc - 180.0,
        540.0 - e.evo,
        e.evc,
        e.overlap
    )
}

// ------------------------------------------------------------------------------------------ firing strip

/// Every cylinder in firing order, coloured by its stroke right now. Click to follow one.
pub fn firing_strip(ui: &mut egui::Ui, sim: &Sim, g: &RigGeom, sel: &mut usize) {
    let cfg = sim.cfg();
    let n = cfg.cylinder_count;
    let sz = if n > 12 { 17.0 } else if n > 8 { 21.0 } else { 26.0 };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
        for k in 0..n {
            let ci = cfg.firing_order[k] as usize - 1;
            if ci >= sim.tables.len() {
                continue;
            }
            let th = local_angle(sim, ci);
            let ph = stroke_of(th);
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(sz), Sense::click());
            let hot = ph == StrokePhase::Power;
            ui.painter().rect_filled(r, 4.0, with_alpha(stroke_color(ph), if hot { 230 } else { 120 }));
            if *sel == ci {
                ui.painter().rect_stroke(r.expand(1.0), 4.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("{}", ci + 1), FontId::proportional(sz * 0.46), if hot { Color32::BLACK } else { Color32::WHITE });
            if resp.clicked() {
                *sel = ci;
            }
            resp.on_hover_ui(|ui| {
                ui.colored_label(stroke_color(ph), format!("Cylinder {}: {}", ci + 1, stroke_name(ph).to_lowercase()));
                ui.label(format!("{:.0} deg into its cycle, {:.1} bar", th, sim.tables[ci].pressure_at(th) / 1e5));
                ui.label(format!("fires number {} in the order, {:.0} deg after the previous one", k + 1, cfg.firing_intervals_deg[(k + n - 1) % n]));
            });
        }
    });
    let ints = &cfg.firing_intervals_deg;
    let even = ints.iter().all(|i| (i - ints[0]).abs() < 1.0);
    let hz = sim.tel.rpm / 120.0 * n as f64;
    ui.colored_label(
        DIM,
        RichText::new(format!(
            "{} power strokes per 720 deg, {}  |  {:.0} Hz at {:.0} rpm",
            n,
            if even { format!("one every {:.0} deg", ints[0]) } else { format!("uneven ({})", ints.iter().map(|i| format!("{i:.0}")).collect::<Vec<_>>().join("/")) },
            hz,
            sim.tel.rpm
        ))
        .size(10.5),
    );
    let _ = g;
}

// ------------------------------------------------------------------------------------------ P-V

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PvScale {
    #[default]
    Linear,
    Log,
    /// Zoomed to the gas-exchange loop near atmospheric pressure.
    Pumping,
}

pub fn pv_diagram(ui: &mut egui::Ui, sim: &Sim, g: &RigGeom, ci: usize, scale: PvScale, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, BG);
    let cfg = sim.cfg();
    let cg = CylGeom::new(cfg);
    let table = &sim.tables[ci];
    let (vc, vd) = (cg.clearance, cg.piston_area * cg.stroke);
    let inner = Rect::from_min_max(rect.min + Vec2::new(38.0, 18.0), rect.max - Vec2::new(8.0, 32.0));

    let pmax = (0..720).map(|d| table.pressure[d]).fold(0.0f32, f32::max).max(8.0e5) * 1.06;
    let (plo, phi) = match scale {
        PvScale::Pumping => {
            let (lo, hi) = (0..720)
                .filter(|d| *d < 176 || *d >= 548)
                .map(|d| table.pressure[d])
                .fold((f32::MAX, 0.0f32), |a, p| (a.0.min(p), a.1.max(p)));
            let span = (hi - lo).max(0.35e5);
            ((lo - span * 0.35).max(0.0), hi + span * 0.35)
        }
        PvScale::Log => (0.2e5, pmax),
        PvScale::Linear => (0.0, pmax),
    };
    let xv = |v: f32| match scale {
        PvScale::Log => ((v / vc).ln() / ((vc + vd) / vc).ln()).clamp(0.0, 1.0),
        _ => ((v - vc) / vd).clamp(0.0, 1.0),
    };
    let to = |v: f32, p: f32| {
        let fy = match scale {
            PvScale::Log => ((p.max(1.0) / plo).ln() / (phi / plo).ln()).clamp(0.0, 1.0),
            _ => ((p - plo) / (phi - plo)).clamp(0.0, 1.0),
        };
        Pos2::new(inner.left() + xv(v) * inner.width(), inner.bottom() - fy * inner.height())
    };

    // grid + labels
    painter.rect_stroke(inner, 0.0, Stroke::new(1.0, GRID), egui::StrokeKind::Inside);
    let ticks: Vec<f32> = match scale {
        PvScale::Linear => {
            let s = nice_step(pmax / 1e5);
            (1..).map(|k| k as f32 * s * 1e5).take_while(|p| *p <= phi).collect()
        }
        PvScale::Log => [0.3, 1.0, 3.0, 10.0, 30.0, 100.0].iter().map(|b| b * 1e5).filter(|p| *p >= plo && *p <= phi).collect(),
        PvScale::Pumping => {
            let span = (phi - plo) / 1e5;
            let s = if span > 1.5 { 0.5 } else if span > 0.6 { 0.2 } else { 0.1 };
            let mut v = (plo / 1e5 / s).ceil() * s;
            let mut out = Vec::new();
            while v * 1e5 <= phi {
                out.push(v * 1e5);
                v += s;
            }
            out
        }
    };
    for p in ticks {
        let y = to(vc, p).y;
        painter.line_segment([Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)], Stroke::new(1.0, GRID));
        let label = if scale == PvScale::Pumping { format!("{:.1}", p / 1e5 + 0.0) } else { format!("{:.0}", p / 1e5 + 0.0) };
        painter.text(Pos2::new(inner.left() - 3.0, y), Align2::RIGHT_CENTER, label, FontId::proportional(9.0), DIM);
    }
    let y_atm = to(vc, 101_325.0).y;
    if y_atm > inner.top() && y_atm < inner.bottom() {
        painter.extend(Shape::dashed_line(&[Pos2::new(inner.left(), y_atm), Pos2::new(inner.right(), y_atm)], Stroke::new(1.0, Color32::from_gray(100)), 3.0, 3.0));
        painter.text(Pos2::new(inner.right() - 2.0, y_atm - 1.0), Align2::RIGHT_BOTTOM, "1 atm", FontId::proportional(8.5), Color32::from_gray(120));
    }
    painter.text(rect.left_top() + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, format!("P-V  cylinder {}", ci + 1), FontId::proportional(11.0), DIM);
    painter.text(Pos2::new(inner.left(), inner.bottom() + 2.0), Align2::LEFT_TOP, "TDC volume", FontId::proportional(9.0), DIM);
    painter.text(Pos2::new(inner.right(), inner.bottom() + 2.0), Align2::RIGHT_TOP, "BDC volume", FontId::proportional(9.0), DIM);

    // filled work regions: strips between the two branches of each loop at equal volumes
    let branch = |a: usize, b: usize| -> Vec<(f32, f32)> {
        // (volume, pressure) for degrees a..=b, sorted by increasing volume
        let mut v: Vec<(f32, f32)> = (a..=b).map(|d| (cg.volume(d as f32), table.pressure[d % 720])).collect();
        v.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        v
    };
    let at_v = |br: &[(f32, f32)], v: f32| -> f32 {
        let i = br.partition_point(|p| p.0 < v).clamp(1, br.len() - 1);
        let (a, b) = (br[i - 1], br[i]);
        let t = if b.0 > a.0 { ((v - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
        a.1 + (b.1 - a.1) * t
    };
    let fill_loop = |lower: &[(f32, f32)], upper: &[(f32, f32)], up_good: bool| {  // green where `upper` is above `lower`
        let n = 56;
        let mut mesh = Mesh::default();
        for k in 0..=n {
            let v = vc + vd * k as f32 / n as f32;
            let (pl, pu) = (at_v(lower, v), at_v(upper, v));
            let good = (pu > pl) == up_good;
            let col = if good { Color32::from_rgba_unmultiplied(70, 220, 130, 70) } else { Color32::from_rgba_unmultiplied(255, 90, 70, 90) };
            mesh.colored_vertex(to(v, pu), col);
            mesh.colored_vertex(to(v, pl), col);
        }
        for i in 0..n as u32 {
            let k = i * 2;
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k + 1, k + 3, k + 2);
        }
        painter.add(Shape::mesh(mesh));
    };
    let comp = branch(180, 360);
    let expn = branch(360, 540);
    let intk = branch(0, 180);
    let exh = branch(540, 719);
    if scale != PvScale::Pumping {
        fill_loop(&comp, &expn, true);
    }
    fill_loop(&exh, &intk, true);

    // the cycle itself, a colour per stroke
    for s in 0..4 {
        if scale == PvScale::Pumping && (s == 1 || s == 2) {
            continue;
        }
        let a = s * 90;
        let pts: Vec<Pos2> = (a..=a + 90).map(|i| {
            let d = i as f32 * 2.0;
            to(cg.volume(d), table.pressure_at(d))
        }).collect();
        painter.add(Shape::line(pts, Stroke::new(1.7, stroke_color(stroke_of(s as f32 * 180.0 + 1.0)))));
    }
    // stroke names on the curve
    let names: &[(f32, StrokePhase)] = if scale == PvScale::Pumping {
        &[(90.0, StrokePhase::Intake), (630.0, StrokePhase::Exhaust)]
    } else {
        &[(270.0, StrokePhase::Compression), (440.0, StrokePhase::Power)]
    };
    for (d, ph) in names {
        let pt = to(cg.volume(*d), table.pressure_at(*d));
        let al = if *d > 400.0 && *d < 500.0 { Align2::LEFT_BOTTOM } else { Align2::CENTER_BOTTOM };
        painter.text(pt + Vec2::new(0.0, -5.0), al, stroke_name(*ph).to_lowercase(), FontId::proportional(9.0), stroke_color(*ph));
    }
    // where we are now
    let th = local_angle(sim, ci);
    let cur = to(cg.volume(th), table.pressure_at(th));
    painter.circle_filled(cur, 4.0, Color32::WHITE);
    painter.circle_stroke(cur, 5.5, Stroke::new(1.0, Color32::BLACK));

    // numbers
    let w = cycle_work(table, &cg);
    painter.text(
        rect.left_bottom() + Vec2::new(6.0, -3.0),
        Align2::LEFT_BOTTOM,
        format!("gross {:+.0} J   pumping {:+.0} J   net {:.0} J   IMEP {:.1} bar", w.gross_j, w.pumping_j, w.net_j, w.imep_bar),
        FontId::proportional(9.5),
        DIM,
    );
    let _ = g;
}

// ------------------------------------------------------------------------------------------ energy

pub struct EnergyFlow {
    pub fuel_kw: f32,
    pub brake_kw: f32,
    pub friction_kw: f32,
    /// Whatever is left: heat leaving by the exhaust and the coolant.
    pub heat_kw: f32,
}

impl EnergyFlow {
    pub fn efficiency(&self) -> f32 {
        if self.fuel_kw > 0.1 {
            self.brake_kw / self.fuel_kw
        } else {
            0.0
        }
    }
}

/// Fuel energy per second from the same trapped-air equation the cylinder model uses (manifold pressure x
/// volume at intake closing / R T, over the air-fuel ratio, once per cylinder per cycle). The telemetry's
/// L/h is smoothed over seconds, which would smear a diagram that is meant to show right now.
pub fn fuel_power_kw(sim: &Sim) -> f32 {
    let t = &sim.tel;
    let cfg = sim.cfg();
    let g = CylGeom::new(cfg);
    let v_ivc = g.volume(180.0) as f64;
    let air_kg = t.manifold_pressure_pa * v_ivc / (engine_core::constants::GAS_CONSTANT_AIR * t.charge_temp_k.max(200.0));
    let fuel_kg = air_kg / t.air_fuel_ratio.max(5.0);
    let cycles_per_s = t.rpm / 120.0 * cfg.cylinder_count as f64;
    (fuel_kg * cycles_per_s * sim.fuels[sim.fuel_idx].lower_heating_value / 1000.0) as f32
}

pub fn energy_flow(sim: &Sim) -> Option<EnergyFlow> {
    let t = &sim.tel;
    if !t.is_running || t.rpm < 300.0 {
        return None;
    }
    let fuel_kw = fuel_power_kw(sim);
    let brake_kw = (t.display_horsepower * 0.7457).max(0.0) as f32;
    let omega = t.rpm * std::f64::consts::TAU / 60.0;
    let friction_kw = (t.friction_torque_nm * omega / 1000.0).max(0.0) as f32;
    let heat_kw = (fuel_kw - brake_kw - friction_kw).max(0.0);
    Some(EnergyFlow { fuel_kw, brake_kw, friction_kw, heat_kw })
}

pub fn energy_bar(ui: &mut egui::Ui, sim: &Sim, width: f32) {
    let Some(e) = energy_flow(sim) else {
        ui.colored_label(DIM, "Start the engine to see where the fuel's energy goes.");
        return;
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 5.0, BG);
    let total = (e.brake_kw + e.friction_kw + e.heat_kw).max(0.1);
    let segs = [
        (e.brake_kw, Color32::from_rgb(70, 210, 190), "shaft"),
        (e.friction_kw, Color32::from_rgb(240, 190, 70), "friction"),
        (e.heat_kw, Color32::from_rgb(255, 110, 70), "heat"),
    ];
    let mut x = rect.left() + 2.0;
    for (v, c, name) in segs {
        let w = (rect.width() - 4.0) * v / total;
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(w, rect.height() - 4.0));
        p.rect_filled(r, 3.0, c);
        if w > 46.0 {
            p.text(r.center(), Align2::CENTER_CENTER, format!("{name} {:.0}%", v / total * 100.0), FontId::proportional(10.0), Color32::BLACK);
        }
        x += w;
    }
    resp.on_hover_text(crate::help::tip("energy"));
    ui.colored_label(
        DIM,
        format!(
            "fuel {:.0} kW  =  shaft {:.0}  +  friction {:.0}  +  heat {:.0} kW      efficiency {:.0}%",
            e.fuel_kw,
            e.brake_kw,
            e.friction_kw,
            e.heat_kw,
            e.efficiency() * 100.0
        ),
    );
}

/// BMEP, piston speed and friends.
pub fn engineering_numbers(ui: &mut egui::Ui, sim: &Sim) {
    let t = &sim.tel;
    let cfg = sim.cfg();
    let vd_m3 = cfg.displacement_l() / 1000.0;
    let bmep_bar = (t.display_torque_nm * 4.0 * std::f64::consts::PI / vd_m3 / 1e5).max(0.0);
    let piston_speed = 2.0 * cfg.stroke_meters * t.rpm / 60.0;
    let hp_per_l = t.display_horsepower / cfg.displacement_l();
    egui::Grid::new("eng_numbers").num_columns(2).spacing([14.0, 2.0]).show(ui, |ui| {
        let row = |ui: &mut egui::Ui, k: &str, tipkey: &str, v: String| {
            ui.colored_label(DIM, k).on_hover_text(crate::help::tip(tipkey));
            ui.label(egui::RichText::new(v).monospace());
            ui.end_row();
        };
        row(ui, "BMEP", "bmep", format!("{bmep_bar:5.1} bar"));
        row(ui, "Mean piston speed", "piston_speed", format!("{piston_speed:5.1} m/s"));
        row(ui, "Specific power", "specific_power", format!("{hp_per_l:5.0} hp/L"));
        row(ui, "Compression ratio", "", format!("{:5.1} : 1", cfg.compression_ratio));
        row(ui, "Bore x stroke", "", format!("{:.0} x {:.0} mm", cfg.bore_meters * 1000.0, cfg.stroke_meters * 1000.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::AudioState;
    use std::sync::{Arc, Mutex};

    fn warm_sim(id: &str) -> Sim {
        let mut s = Sim::new(Arc::new(Mutex::new(AudioState::default())), id);
        s.pre_roll(1.5);
        s
    }

    #[test]
    fn recorded_tables_have_no_unwritten_bins() {
        // a warm-up must not leave bins at their defaults between the ones the micro-steps landed in
        for id in ["v8", "i4"] {
            let s = warm_sim(id);
            for ci in 0..s.tables.len() {
                let t = &s.tables[ci];
                for i in 195..350usize {
                    let mid = 0.5 * (t.pressure[i - 1] + t.pressure[i + 1]);
                    assert!((t.pressure[i] - mid).abs() < 0.08 * mid + 2000.0, "{id} cyl {} bin {i}: {} vs neighbours {}", ci + 1, t.pressure[i], mid);
                }
                for i in 30..170usize {
                    assert!(t.burn[i] == t.burn[i - 1] || (t.burn[i] - t.burn[i - 1]).abs() < 0.2, "{id} cyl {} burn jumps at bin {i}", ci + 1);
                }
            }
        }
    }

    #[test]
    fn stroke_boundaries() {
        assert_eq!(stroke_of(0.0), StrokePhase::Intake);
        assert_eq!(stroke_of(179.9), StrokePhase::Intake);
        assert_eq!(stroke_of(180.0), StrokePhase::Compression);
        assert_eq!(stroke_of(400.0), StrokePhase::Power);
        assert_eq!(stroke_of(719.0), StrokePhase::Exhaust);
        assert_eq!(stroke_of(-1.0), StrokePhase::Exhaust);
    }

    #[test]
    fn pre_roll_leaves_a_running_idling_engine_with_full_tables() {
        for id in ["v8", "i4", "dieseli6"] {
            let s = warm_sim(id);
            assert!(s.tel.is_running, "{id} should be running");
            assert!(s.tel.rpm > 400.0 && s.tel.rpm < 1500.0, "{id} idle {}", s.tel.rpm);
            let hi = s.tables[0].pressure.iter().cloned().fold(0.0f32, f32::max);
            assert!(hi > 8.0e5, "{id}: compression pressure never recorded ({hi})");
        }
    }

    #[test]
    fn indicated_work_agrees_with_the_engines_own_indicated_torque() {
        // net work per cycle, times cylinders, over 4 pi, against the indicated torque the engine reports
        // (averaged: the instantaneous torque ripples at the firing rate). Steady idle, away from the limiter.
        for id in ["v8", "i4"] {
            let mut s = warm_sim(id);
            let mut t_acc = 0.0f64;
            let frames = 600;
            for _ in 0..frames {
                s.step(1.0 / 60.0);
                t_acc += s.tel.indicated_torque_nm;
            }
            let t_engine = (t_acc / frames as f64) as f32;
            let g = CylGeom::new(s.cfg());
            let n = s.cfg().cylinder_count;
            let w: f32 = (0..n).map(|c| cycle_work(&s.tables[c], &g).net_j).sum::<f32>() / n as f32;
            let t_from_work = w * n as f32 / (4.0 * std::f32::consts::PI);
            eprintln!("{id}: rpm {:.0}, work/cyl {w:.0} J, torque from work {t_from_work:.1} Nm, engine indicated (avg) {t_engine:.1} Nm", s.tel.rpm);
            assert!(t_from_work > 0.0);
            assert!((t_from_work / t_engine - 1.0).abs() < 0.2, "{id}: {t_from_work} vs {t_engine}");
        }
    }

    #[test]
    fn what_efficiency_does_the_sim_give_at_full_throttle() {
        for id in ["v8", "i4", "dieseli6"] {
            let mut s = warm_sim(id);
            let cfg = s.cfg().clone();
            for frac in [0.35, 0.6, 0.85] {
                let rpm = cfg.redline_rpm * frac;
                let tq = s.engine.dyno_point(rpm, 1.0);
                s.tel = s.engine.telemetry();
                let brake_kw = tq * rpm * std::f64::consts::TAU / 60.0 / 1000.0;
                let fuel_kw = fuel_power_kw(&s) as f64;
                eprintln!("{id} rpm {rpm:.0}: brake {brake_kw:.0} kW, fuel {fuel_kw:.0} kW, eff {:.1}% (AFR {:.1}, MAP {:.0} kPa)", brake_kw / fuel_kw * 100.0, s.tel.air_fuel_ratio, s.tel.manifold_pressure_pa / 1000.0);
            }
        }
    }

    #[test]
    fn energy_balance_is_sane_at_part_throttle() {
        let mut s = warm_sim("v8");
        s.throttle_input = 0.4;
        s.throttle_slider_active = true;
        for _ in 0..300 {
            s.step(1.0 / 60.0);
        }
        let e = energy_flow(&s).expect("running");
        eprintln!("fuel {:.0} kW brake {:.0} friction {:.0} heat {:.0} eff {:.1}%", e.fuel_kw, e.brake_kw, e.friction_kw, e.heat_kw, e.efficiency() * 100.0);
        assert!(e.fuel_kw > 0.0 && e.efficiency() < 0.6);
    }
}
