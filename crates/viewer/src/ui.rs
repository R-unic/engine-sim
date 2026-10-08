//! egui HUD: toolbar, selectors, gauges, drive controls, the tabbed analysis panel (cycle / P-V / dyno /
//! power), tuning, faults, lessons and the part labels. Every control and readout carries a tooltip.

use crate::cam::{Anchors, CamRig};
use crate::cycle::{self, PvScale};
use crate::dyno_bg::{peak, DynoJobs};
use crate::help;
use crate::hud::{Panel, Tab, UiState};
use crate::labels::{self, PartTag};
use crate::learn;
use crate::rig::anim::{local_angle, RigGeom};
use crate::scene::MainCamera;
use crate::sim::Sim;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2};
use engine_core::{
    stock_turbo_count, AirFilter, Cat, CelState, Crossover, EngineConfig, EvapMonitorStatus, Header, IgnitionType, Intercooler,
    Muffler, PipeMaterial, Runners, Tuning, BOOST_MAX_BAR, BOOST_MIN_BAR,
};
use std::f32::consts::PI;

const ACCENT: Color32 = Color32::from_rgb(255, 150, 40);
const TEAL: Color32 = Color32::from_rgb(70, 210, 190);
const DIM: Color32 = Color32::from_rgb(150, 155, 165);
const PANEL_BG: Color32 = Color32::from_rgb(8, 9, 13);

#[derive(Resource)]
pub struct DynoTrace {
    pub torque: Vec<f32>,
    pub power: Vec<f32>,
    pub seen: Vec<bool>,
}

impl Default for DynoTrace {
    fn default() -> Self {
        Self { torque: vec![0.0; 120], power: vec![0.0; 120], seen: vec![false; 120] }
    }
}

/// Everything the HUD reads from the ECS besides the sim and camera.
#[derive(SystemParam)]
pub struct UiWorld<'w, 's> {
    anchors: Res<'w, Anchors>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    jobs: Res<'w, DynoJobs>,
    geom: Option<Res<'w, RigGeom>>,
    cams: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<MainCamera>>,
    tags: Query<'w, 's, (&'static PartTag, &'static GlobalTransform)>,
    wants: Res<'w, bevy_egui::input::EguiWantsInput>,
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.window_fill = Color32::from_rgba_unmultiplied(12, 14, 20, 232);
    v.panel_fill = Color32::from_rgba_unmultiplied(12, 14, 20, 200);
    v.window_corner_radius = egui::CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(50, 56, 70));
    v.widgets.inactive.bg_fill = Color32::from_rgb(34, 38, 50);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.interaction.tooltip_delay = 0.25;
    });
}

pub fn ui_system(
    mut ctxs: EguiContexts,
    mut sim: ResMut<Sim>,
    mut cam: ResMut<CamRig>,
    mut dyno: ResMut<DynoTrace>,
    mut st: ResMut<UiState>,
    mut see: ResMut<crate::seethrough::SeeThrough>,
    w: UiWorld,
) {
    let Ok(ctx) = ctxs.ctx_mut() else { return };
    if !st.styled {
        st.styled = true;
        style(ctx);
    }
    if (ctx.zoom_factor() - st.ui_scale).abs() > 1e-3 {
        ctx.set_zoom_factor(st.ui_scale);
    }
    if st.last_engine != sim.engine_idx {
        st.last_engine = sim.engine_idx;
        *dyno = DynoTrace::default();
        st.sel_cyl = 0;
        st.pinned = None;
    }
    let typing = w.wants.wants_keyboard_input();
    if !typing {
        if w.keys.just_pressed(KeyCode::Tab) {
            st.show_ui = !st.show_ui;
        }
        if w.keys.just_pressed(KeyCode::KeyL) {
            st.labels = !st.labels;
        }
        if w.keys.just_pressed(KeyCode::F1) {
            st.toggle(Panel::Learn);
        }
    }

    // dyno trace sampling (wide-open throttle only, steady in gear or neutral)
    {
        let t = &sim.tel;
        if t.throttle > 0.9 && t.is_running && !t.is_shifting && t.rpm > 800.0 {
            let bin = ((t.rpm / 100.0) as usize).min(119);
            let hp = (t.display_horsepower as f32).max(0.0);
            let tq = (t.display_torque_nm as f32).max(0.0);
            if !dyno.seen[bin] {
                dyno.seen[bin] = true;
                dyno.torque[bin] = tq;
                dyno.power[bin] = hp;
            } else {
                dyno.torque[bin] += (tq - dyno.torque[bin]) * 0.15;
                dyno.power[bin] += (hp - dyno.power[bin]) * 0.15;
            }
        }
    }

    // a camera move parked until the rig has been rebuilt (an engine swap changes the anchors)
    if let Some(c) = st.pending_cam {
        if !sim.rebuild && learn::apply_cam(c, &mut cam, &w.anchors, w.geom.as_deref(), st.sel_cyl) {
            st.pending_cam = None;
        }
    }

    if !st.show_ui || sim.hide_ui {
        cam.ui_wants_pointer = ctx.is_pointer_over_egui();
        return;
    }

    let mut fly_to: Option<Vec3> = None;
    sim.focus_cyl = (st.is_open(Panel::Analysis) && matches!(st.tab, Tab::Cycle | Tab::Pv)).then_some(st.sel_cyl);

    toolbar(ctx, &mut st);
    if st.is_open(Panel::Engine) {
        engine_window(ctx, &mut st, &mut sim, &mut cam, &w.anchors, &mut see);
    }
    if st.is_open(Panel::Tuning) {
        tuning_window(ctx, &mut st, &mut sim, &w.jobs);
    }
    if st.is_open(Panel::Telemetry) {
        telemetry_window(ctx, &mut st, &mut sim);
    }
    drive_window(ctx, &st, &mut sim);
    if st.is_open(Panel::Analysis) {
        if let Some(g) = w.geom.as_deref() {
            analysis_window(ctx, &mut st, &mut sim, g, &dyno, &w.jobs);
        }
    }
    if st.is_open(Panel::Faults) {
        faults_window(ctx, &mut st, &mut sim);
    }
    learn::learn_window(ctx, &mut st, &mut sim);
    learn::glossary_window(ctx, &mut st);
    welcome_window(ctx, &mut st, &mut sim);

    // names in the 3D view
    let welcoming = st.welcome;
    if !welcoming {
        if let Ok(camera) = w.cams.single() {
            let projected = labels::project(camera, &w.tags, ctx.zoom_factor());
            let blocked = blocked_rects(ctx, &st);
            let sel = st.sel_cyl;
            let mut pinned = st.pinned;
            if st.labels {
                labels::draw(ctx, &projected, &sim, w.geom.as_deref(), &cam, sel, &blocked, &mut pinned, &mut fly_to);
            } else {
                labels::hover_pick(ctx, &projected, &sim, w.geom.as_deref(), &cam, sel, &blocked, &mut pinned, &mut fly_to, st.hover_override);
            }
            st.pinned = pinned;
            if st.is_open(Panel::Analysis) && st.tab == Tab::Cycle {
                let mut s = st.sel_cyl;
                labels::cylinder_badges(ctx, &projected, &sim, &blocked, &mut s);
                st.sel_cyl = s;
            }
        }
    }
    {
        let mut pinned = st.pinned;
        labels::pinned_window(ctx, &mut pinned, &sim, w.geom.as_deref(), st.sel_cyl, &mut fly_to);
        st.pinned = pinned;
    }
    if let Some(p) = fly_to {
        cam.t_target = p;
        cam.t_dist = (cam.t_dist * 0.5).clamp(0.3, 1.1);
        cam.preset = usize::MAX;
    }

    cam.ui_wants_pointer = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
}

/// Rectangles of the panels that are actually on screen, so labels avoid them.
fn blocked_rects(ctx: &egui::Context, st: &UiState) -> Vec<Rect> {
    let mut titles = vec!["TOOLBAR", "DRIVE"];
    for (p, t) in [
        (Panel::Engine, "ENGINE SIM"),
        (Panel::Tuning, "TUNING"),
        (Panel::Telemetry, "TELEMETRY"),
        (Panel::Analysis, "ANALYSIS"),
        (Panel::Faults, "FAULTS"),
        (Panel::Learn, "LEARN"),
        (Panel::Glossary, "GLOSSARY"),
    ] {
        if st.is_open(p) {
            titles.push(t);
        }
    }
    titles
        .iter()
        .filter_map(|t| ctx.memory(|m| m.area_rect(if *t == "TOOLBAR" { egui::Id::new("TOOLBAR") } else { crate::hud::win_id(t) })))
        .collect()
}

// ------------------------------------------------------------------------------------------ toolbar

fn toolbar(ctx: &egui::Context, st: &mut UiState) {
    egui::Area::new(egui::Id::new("TOOLBAR")).anchor(Align2::CENTER_TOP, [0.0, 8.0]).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(12, 14, 20, 225))
            .stroke(Stroke::new(1.0, Color32::from_rgb(50, 56, 70)))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let learn_on = st.is_open(Panel::Learn);
                    let b = egui::Button::new(RichText::new("LEARN").strong().color(if learn_on { Color32::BLACK } else { ACCENT }))
                        .fill(if learn_on { ACCENT } else { Color32::from_rgb(34, 38, 50) });
                    if ui.add(b).on_hover_text("Guided lessons: how an engine works, with the camera and engine set up for you (F1)").clicked() {
                        st.toggle(Panel::Learn);
                    }
                    let b = egui::Button::new(RichText::new("Labels").color(if st.labels { Color32::BLACK } else { Color32::WHITE }))
                        .fill(if st.labels { TEAL } else { Color32::from_rgb(34, 38, 50) });
                    if ui.add(b).on_hover_text(help::tip("labels")).clicked() {
                        st.labels = !st.labels;
                    }
                    ui.separator();
                    for (p, name, tip) in [
                        (Panel::Engine, "Engine", "Engine, gearbox and fuel selectors, time controls and camera views"),
                        (Panel::Tuning, "Tuning", "Bolt on intake, exhaust and turbo parts and watch the engine, the dyno and the sound change"),
                        (Panel::Analysis, "Analysis", "The four-stroke cycle, P-V diagram, dyno curves and energy balance"),
                        (Panel::Telemetry, "Telemetry", "Gauges and live readings"),
                        (Panel::Faults, "Faults", "Break things on purpose: ignition, catalyst, O2 sensor, EVAP, fuel"),
                    ] {
                        let on = st.is_open(p);
                        if ui.selectable_label(on, name).on_hover_text(tip).clicked() {
                            st.toggle(p);
                        }
                    }
                    ui.separator();
                    if ui.button("?").on_hover_text("Glossary of engine terms").clicked() {
                        st.toggle(Panel::Glossary);
                    }
                    if ui.small_button("A-").on_hover_text(help::tip("ui_scale")).clicked() {
                        st.ui_scale = (st.ui_scale - 0.1).max(0.7);
                        st.save_prefs();
                    }
                    if ui.small_button("A+").on_hover_text(help::tip("ui_scale")).clicked() {
                        st.ui_scale = (st.ui_scale + 0.1).min(1.8);
                        st.save_prefs();
                    }
                });
            });
    });
}

// ------------------------------------------------------------------------------------------ engine panel

fn engine_window(ctx: &egui::Context, st: &mut UiState, sim: &mut Sim, cam: &mut CamRig, anchors: &Anchors, see: &mut crate::seethrough::SeeThrough) {
    let mut open = true;
    crate::hud::window("ENGINE SIM").default_pos([12.0, 56.0]).resizable(false).default_width(262.0).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 5.0);
        let cfg_name = sim.cfg().display_name.clone();
        let disp = sim.cfg().displacement_l();
        egui::ComboBox::from_label("Engine")
            .selected_text(format!("{cfg_name}  ({disp:.1} L)"))
            .width(150.0)
            .show_ui(ui, |ui| {
                for i in 0..sim.engines.len() {
                    let n = sim.engines[i].display_name.clone();
                    if ui.selectable_label(sim.engine_idx == i, n).clicked() {
                        sim.select_engine(i);
                    }
                }
            })
            .response
            .on_hover_text(help::tip("engine"));
        let trans_name = sim.transmissions[sim.trans_idx].display_name;
        egui::ComboBox::from_label("Gearbox")
            .selected_text(trans_name)
            .width(150.0)
            .show_ui(ui, |ui| {
                for i in 0..sim.transmissions.len() {
                    let n = sim.transmissions[i].display_name;
                    if ui.selectable_label(sim.trans_idx == i, n).clicked() {
                        sim.select_transmission(i);
                    }
                }
            })
            .response
            .on_hover_text(help::tip("gearbox"));
        let fuel_name = sim.fuels[sim.fuel_idx].display_name;
        let ig = sim.cfg().ignition_type;
        egui::ComboBox::from_label("Fuel")
            .selected_text(fuel_name)
            .width(150.0)
            .show_ui(ui, |ui| {
                for i in 0..sim.fuels.len() {
                    if sim.fuels[i].ignition_type != ig {
                        continue;
                    }
                    let n = sim.fuels[i].display_name;
                    if ui.selectable_label(sim.fuel_idx == i, n).clicked() {
                        sim.select_fuel(i);
                    }
                }
            })
            .response
            .on_hover_text(help::tip("fuel"));
        let brake_name = sim.brakes[sim.brake_idx].display_name;
        egui::ComboBox::from_label("Brakes")
            .selected_text(brake_name)
            .width(150.0)
            .show_ui(ui, |ui| {
                for i in 0..sim.brakes.len() {
                    let n = sim.brakes[i].display_name;
                    if ui.selectable_label(sim.brake_idx == i, n).clicked() {
                        sim.select_brake(i);
                    }
                }
            })
            .response
            .on_hover_text(help::tip("brakes"));
        ui.separator();
        ui.horizontal(|ui| {
            ui.colored_label(DIM, "speed");
            for (v, name) in [(1.0f32, "1x"), (0.3, "0.3x"), (0.1, "0.1x"), (0.03, "0.03x")] {
                if ui.selectable_label((sim.time_scale - v).abs() < 0.005, name).on_hover_text(help::tip("time")).clicked() {
                    sim.time_scale = v;
                }
            }
        });
        if sim.dilation < 0.995 {
            ui.colored_label(DIM, format!("playback {:.0}% of real speed", sim.dilation * 100.0)).on_hover_text(help::tip("visual_cap"));
        }
        ui.horizontal(|ui| {
            if ui.checkbox(&mut sim.paused, "pause").changed() && !sim.paused {
                sim.scrub = false;
            }
            if ui.button("Refuel").clicked() {
                sim.engine.refuel();
            }
            ui.checkbox(&mut cam.auto_orbit, "orbit");
        });
        ui.collapsing("More settings", |ui| {
            let mut ts = sim.time_scale.log10();
            if ui
                .add(egui::Slider::new(&mut ts, -2.0..=0.0).text("time").custom_formatter(|v, _| format!("{:.2}x", 10f64.powf(v))))
                .on_hover_text(help::tip("time"))
                .changed()
            {
                sim.time_scale = 10f32.powf(ts);
            }
            let mut cap = sim.visual_cap_rpm;
            ui.add(egui::Slider::new(&mut cap, 0.0..=3000.0).text("visual cap").custom_formatter(|v, _| if v < 1.0 { "real".to_string() } else { format!("{v:.0} rpm") }))
                .on_hover_text(help::tip("visual_cap"));
            sim.visual_cap_rpm = cap;
            ui.checkbox(&mut see.enabled, "see-through").on_hover_text(help::tip("seethrough"));
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut sim.master_volume, 0.0..=1.5).text("volume")).on_hover_text(help::tip("volume"));
                ui.checkbox(&mut sim.muted, "mute");
            });
            ui.checkbox(&mut sim.auto_shift, "auto shift").on_hover_text(help::tip("autoshift"));
            let r = ui.add(egui::Slider::new(&mut st.ui_scale, 0.7..=1.8).text("UI size")).on_hover_text(help::tip("ui_scale"));
            if r.drag_stopped() {
                st.save_prefs();
            }
        });
        ui.separator();
        ui.label(RichText::new("CAMERA").small().color(DIM));
        ui.horizontal_wrapped(|ui| {
            for (i, a) in anchors.0.iter().enumerate() {
                if ui.selectable_label(cam.preset == i, a.name).clicked() {
                    cam.preset = i;
                    let a = a.clone();
                    cam.goto(&a);
                }
            }
        });
        ui.collapsing("Keyboard controls", |ui| {
            ui.colored_label(
                DIM,
                "E hold to crank   W throttle   S / Space brake\nX / Z shift up / down   N neutral   R reverse\nC cycle views   O orbit   L labels\nTab hide the HUD   P pause   [ ] slower / faster\nmouse: left orbit, right pan, wheel zoom",
            );
        });
    });
    st.set(Panel::Engine, open);
}

// ------------------------------------------------------------------------------------------ telemetry

fn telemetry_window(ctx: &egui::Context, st: &mut UiState, sim: &mut Sim) {
    let mut open = true;
    let right = ctx.content_rect().right();
    crate::hud::window("TELEMETRY").default_pos([right - 12.0, 56.0]).pivot(Align2::RIGHT_TOP).resizable(false).open(&mut open).show(ctx, |ui| {
        let t = sim.tel.clone();
        ui.horizontal(|ui| {
            let rpm_max = ((t.redline_rpm / 1000.0).ceil() as f32 + 1.0) * 1000.0;
            gauge(ui, 168.0, t.rpm as f32, 0.0, rpm_max, Some(t.redline_rpm as f32 - 400.0), 1000.0, "RPM", &format!("{:.0}", t.rpm), 1000.0).on_hover_text(help::tip("rpm"));
            if sim.speed_kph {
                let kph = t.vehicle_speed_mps as f32 * 3.6;
                gauge(ui, 132.0, kph, 0.0, 320.0, None, 40.0, "km/h", &format!("{:.0}", kph), 1.0).on_hover_text(help::tip("speed"));
            } else {
                let mph = t.vehicle_speed_mps as f32 * 2.236_936;
                gauge(ui, 132.0, mph, 0.0, 200.0, None, 25.0, "mph", &format!("{:.0}", mph), 1.0).on_hover_text(help::tip("speed"));
            }
        });
        ui.checkbox(&mut sim.speed_kph, "speedometer in km/h");
        ui.horizontal(|ui| {
            let (gl, gcol) = match t.gear_label.as_str() {
                "N" => ("N".to_string(), DIM),
                "R" => ("R".to_string(), Color32::from_rgb(255, 90, 90)),
                g => (g.to_string(), TEAL),
            };
            ui.label(RichText::new(gl).size(40.0).strong().color(gcol)).on_hover_text("The gear selected: N is neutral, R is reverse. Z and X change it.");
            ui.vertical(|ui| {
                let (txt, col, why) = if t.is_cranking {
                    ("CRANKING", ACCENT, "The starter motor is turning the engine over. Fuel and spark begin after about a second.")
                } else if t.is_out_of_fuel && !t.is_running {
                    ("OUT OF FUEL", Color32::RED, "The tank is empty. Use Refuel in the ENGINE panel.")
                } else if t.is_running {
                    if t.is_rev_limiting {
                        ("REV LIMIT", Color32::RED, "The ECU is cutting fuel or spark to stop the engine passing its redline.")
                    } else if t.is_misfiring {
                        ("MISFIRE", ACCENT, "At least one cylinder is not burning its charge. Follow it in the cycle panel.")
                    } else if t.is_knocking {
                        ("KNOCK", ACCENT, "The mixture is detonating before the flame reaches it. Lower the boost, cool the intake air or use a higher octane fuel.")
                    } else {
                        ("RUNNING", TEAL, "The engine is running normally.")
                    }
                } else {
                    ("OFF", DIM, "The engine is off. Hold E or the START button to crank it.")
                };
                ui.label(RichText::new(txt).strong().color(col)).on_hover_text(why);
                if t.is_shifting {
                    ui.colored_label(ACCENT, "shifting");
                }
                if t.is_overheating {
                    ui.colored_label(Color32::RED, "OVERHEAT").on_hover_text("Coolant is above about 110 C.");
                }
                let blink = ((ui.input(|i| i.time) * 2.0) as i64) % 2 == 0;
                let cel_tip = "The ECU has stored a fault code. Misfires, a failed O2 sensor and EVAP leaks all light it. A flashing lamp means a misfire severe enough to damage the catalyst.";
                match t.cel_state {
                    CelState::Steady => {
                        ui.colored_label(ACCENT, "(!) CHECK ENGINE").on_hover_text(cel_tip);
                    }
                    CelState::Flashing => {
                        ui.colored_label(if blink { Color32::from_rgb(255, 60, 40) } else { Color32::from_rgb(70, 30, 25) }, "(!) CHECK ENGINE").on_hover_text(cel_tip);
                    }
                    CelState::Off => {}
                }
                if t.is_low_fuel {
                    ui.colored_label(ACCENT, "LOW FUEL");
                }
            });
        });
        ui.separator();
        egui::Grid::new("tel").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, k: &str, tip: &str, v: String| {
                ui.colored_label(DIM, k).on_hover_text(help::tip(tip));
                ui.label(RichText::new(v).monospace()).on_hover_text(help::tip(tip));
                ui.end_row();
            };
            row(ui, "Torque", "torque", format!("{:5.0} Nm  {:4.0} lb-ft", t.display_torque_nm, t.display_torque_lb_ft));
            row(ui, "Power", "power", format!("{:5.0} hp  {:4.0} kW", t.display_horsepower, t.display_horsepower * 0.7457));
            row(ui, "Manifold", "manifold", format!("{:5.0} kPa  ({:+.2} bar)", t.manifold_pressure_pa / 1000.0, (t.manifold_pressure_pa - t.atmospheric_pressure_pa) / 100_000.0));
            if sim.cfg().turbo.is_some() {
                row(ui, "Boost", "boost", format!("{:5.2} bar  spool {:3.0}%", t.boost_pressure_pa / 100_000.0, t.turbo_spool_fraction * 100.0));
            }
            row(ui, "AFR", "afr", format!("{:5.1}  (O2 {:.1}, trim {:+.1})", t.air_fuel_ratio, t.exhaust.o2_reported_afr, t.exhaust.fuel_trim_afr));
            row(ui, "Coolant", "coolant", format!("{:5.0} C", t.coolant_temp_k - 273.15));
            row(ui, "EGT", "egt", format!("{:5.0} C   cat {:.0} C{}", t.exhaust_gas_temp_k - 273.15, t.exhaust.catalyst_temp_k - 273.15, if t.exhaust.catalyst_lit_off { " lit" } else { "" }));
            row(ui, "Fuel", "fuel_level", format!("{:4.1} / {:.0} L  {:.1} L/h", t.fuel_level_l, t.fuel_tank_capacity_l, t.fuel_consumption_l_per_hour));
            row(ui, "Loop", "loop", if t.exhaust.closed_loop_active { "closed".into() } else { "open".into() });
            if sim.cfg().ignition_type == IgnitionType::Spark {
                let evap_status = match t.evap_monitor_status {
                    EvapMonitorStatus::NotReady => "not ready",
                    EvapMonitorStatus::Testing => "testing...",
                    EvapMonitorStatus::Pass => "pass",
                    EvapMonitorStatus::SmallLeak => "SMALL LEAK",
                    EvapMonitorStatus::LargeLeak => "LARGE LEAK",
                };
                row(ui, "EVAP", "evap", format!("{evap_status}  tank {:+.1} kPa", t.evap.tank_pressure_pa / 1000.0));
            }
            row(ui, "Clutch", "clutch", format!("{:3.0}%", t.clutch_engagement * 100.0));
        });
        ui.add_space(2.0);
        ui.colored_label(DIM, "hover over any reading to see what it means").on_hover_text("Everything on the HUD has a tooltip.");
    });
    st.set(Panel::Telemetry, open);
}

// ------------------------------------------------------------------------------------------ drive

/// One line of advice for the player, from what the engine is doing right now.
pub fn coach_hint(sim: &Sim) -> Option<(Color32, String)> {
    let t = &sim.tel;
    let warn = Color32::from_rgb(255, 120, 90);
    if t.is_out_of_fuel && !t.is_running {
        return Some((warn, "Out of fuel. Press Refuel in the ENGINE panel.".into()));
    }
    if !t.is_running && !t.is_cranking {
        return Some((ACCENT, "Engine off: hold START (or E) to crank it. It catches after about a second.".into()));
    }
    if t.is_rev_limiting {
        return Some((warn, "Rev limiter: the ECU cuts fuel and spark at redline to protect the engine. Lift off or shift up (X).".into()));
    }
    if t.is_knocking {
        return Some((warn, "Knock: the fuel is detonating early. Lower the boost, cool the intake air or switch to a higher-octane fuel.".into()));
    }
    if t.is_overheating {
        return Some((warn, "Overheating: coolant is above 110 C. Ease off and let the radiator catch up.".into()));
    }
    if !matches!(t.cel_state, CelState::Off) {
        return Some((ACCENT, "Check-engine lamp: the ECU has found a fault (misfire, O2 sensor or EVAP). See the FAULTS panel.".into()));
    }
    if t.is_misfiring {
        return Some((ACCENT, "Misfire: a cylinder is not burning its charge. Follow it in the cycle panel.".into()));
    }
    if t.is_running && t.total_gear_ratio.abs() < 1e-3 && t.vehicle_speed_mps < 1.0 && !sim.paused {
        return Some((TEAL, "In neutral. Press X (or click 1) for first gear, then hold W to drive off.".into()));
    }
    if t.is_running && t.total_gear_ratio.abs() > 1e-3 && t.throttle < 0.02 && t.vehicle_speed_mps > 6.0 && t.rpm > 1800.0 {
        return Some((TEAL, "Throttle closed: the pistons pull against a vacuum and slow the car (engine braking).".into()));
    }
    None
}

fn drive_window(ctx: &egui::Context, st: &UiState, sim: &mut Sim) {
    let c = ctx.content_rect();
    crate::hud::window("DRIVE").default_pos([530.0, c.bottom() - 12.0]).pivot(Align2::LEFT_BOTTOM).title_bar(false).resizable(false).show(ctx, |ui| {
        if st.coach && st.lesson.is_none() {
            if let Some((col, msg)) = coach_hint(sim) {
                ui.set_max_width(600.0);
                ui.colored_label(col, msg);
            }
        }
        ui.horizontal(|ui| {
            let start = ui.add_sized([86.0, 34.0], egui::Button::new(RichText::new("START").strong()).fill(Color32::from_rgb(30, 110, 70))).on_hover_text(help::tip("start"));
            sim.starter_held_by_ui = start.is_pointer_button_down_on();
            if ui.add_sized([60.0, 34.0], egui::Button::new("STOP")).on_hover_text(help::tip("stop")).clicked() {
                sim.engine.shut_off();
                sim.auto_start = false;
            }
            ui.separator();
            let gears: Vec<String> = sim.transmissions[sim.trans_idx].gears.iter().map(|g| g.label.to_string()).collect();
            for (i, g) in gears.iter().enumerate() {
                let sel = sim.gear_idx == i;
                let b = egui::Button::new(RichText::new(g).strong().color(if sel { Color32::BLACK } else { Color32::WHITE })).fill(if sel { TEAL } else { Color32::from_rgb(34, 38, 50) });
                let tip = match g.as_str() {
                    "R" => "Reverse".to_string(),
                    "N" => "Neutral: the engine is disconnected from the wheels.".to_string(),
                    n => format!("Gear {n}: ratio {:.2} to 1", sim.transmissions[sim.trans_idx].gears[i].ratio),
                };
                if ui.add_sized([30.0, 34.0], b).on_hover_text(tip).clicked() {
                    sim.set_gear_index(i);
                }
            }
            ui.separator();
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Throttle");
                    let r = ui.add(egui::Slider::new(&mut sim.throttle_input, 0.0..=1.0).show_value(true).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).on_hover_text(help::tip("throttle"));
                    if r.dragged() {
                        sim.throttle_slider_active = true;
                    }
                    ui.checkbox(&mut sim.throttle_slider_active, "hold");
                });
                ui.horizontal(|ui| {
                    ui.label("Brake   ");
                    ui.add(egui::Slider::new(&mut sim.brake_input, 0.0..=1.0).show_value(true).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).on_hover_text(help::tip("brake"));
                });
            });
        });
    });
}

// ------------------------------------------------------------------------------------------ analysis

fn analysis_window(ctx: &egui::Context, st: &mut UiState, sim: &mut Sim, g: &RigGeom, dyno: &DynoTrace, jobs: &DynoJobs) {
    if sim.tables.len() != g.lay.n() {
        return;
    }
    let mut open = true;
    let c = ctx.content_rect();
    crate::hud::window("ANALYSIS").default_pos([12.0, c.bottom() - 12.0]).pivot(Align2::LEFT_BOTTOM).resizable(false).default_width(496.0).open(&mut open).show(ctx, |ui| {
        ui.set_max_width(496.0);
        ui.horizontal(|ui| {
            for (t, name, tip) in [
                (Tab::Cycle, "Cycle", "The four strokes of one cylinder, with valve events, spark and pressure on a 720 degree timeline"),
                (Tab::Pv, "P-V", help::tip("pv")),
                (Tab::Dyno, "Dyno", help::tip("dyno")),
                (Tab::Power, "Power", "Engineering numbers and where the fuel's energy goes"),
            ] {
                ui.selectable_value(&mut st.tab, t, name).on_hover_text(tip);
            }
        });
        ui.separator();
        match st.tab {
            Tab::Cycle => cycle_tab(ui, st, sim, g),
            Tab::Pv => pv_tab(ui, st, sim, g),
            Tab::Dyno => {
                dyno_chart(ui, dyno, sim, jobs, Vec2::new(480.0, 210.0), true);
                ui.colored_label(DIM, "orange = torque, teal = power.  Faint lines are the stock engine; the dots are what you actually drove through at full throttle.");
            }
            Tab::Power => power_tab(ui, sim),
        }
    });
    st.set(Panel::Analysis, open);
}

fn cycle_tab(ui: &mut egui::Ui, st: &mut UiState, sim: &mut Sim, g: &RigGeom) {
    let n = sim.cfg().cylinder_count;
    st.sel_cyl = st.sel_cyl.min(n - 1);
    ui.horizontal(|ui| {
        if ui.small_button("<").on_hover_text("previous cylinder").clicked() {
            st.sel_cyl = (st.sel_cyl + n - 1) % n;
        }
        ui.label(RichText::new(format!("Cylinder {} of {}", st.sel_cyl + 1, n)).strong());
        if ui.small_button(">").on_hover_text("next cylinder").clicked() {
            st.sel_cyl = (st.sel_cyl + 1) % n;
        }
        ui.separator();
        if ui.small_button(if sim.paused { "Play" } else { "Pause" }).clicked() {
            sim.paused = !sim.paused;
            if !sim.paused {
                sim.scrub = false;
            }
        }
        for (d, name) in [(-15.0f32, "-15"), (15.0, "+15")] {
            if ui.small_button(name).on_hover_text("turn the crank by hand, in degrees").clicked() {
                let th = local_angle(sim, st.sel_cyl);
                sim.scrub_to(st.sel_cyl, th + d);
            }
        }
        ui.separator();
        for (v, name) in [(1.0f32, "1x"), (0.1, "0.1x"), (0.03, "0.03x")] {
            if ui.selectable_label((sim.time_scale - v).abs() < 0.005 && !sim.paused, name).on_hover_text("simulation speed").clicked() {
                sim.time_scale = v;
                sim.paused = false;
                sim.scrub = false;
            }
        }
    });
    let ci = st.sel_cyl;
    cycle::timeline(ui, sim, g, ci, 480.0);
    ui.colored_label(DIM, RichText::new(cycle::valve_event_text(g)).size(10.5))
        .on_hover_text("Valve events: IVO/IVC = intake valve opens/closes, EVO/EVC = exhaust. BTDC = before top dead centre, ABDC = after bottom dead centre.");
    let cap = cycle::caption(sim, g, ci);
    ui.add_space(2.0);
    ui.label(RichText::new(cap.title).strong().size(14.0).color(cap.color));
    ui.allocate_ui(Vec2::new(480.0, 58.0), |ui| {
        ui.set_min_height(58.0);
        ui.label(RichText::new(cap.body).size(12.0));
    });
    ui.colored_label(DIM, RichText::new(cap.facts).size(11.0));
    ui.add_space(2.0);
    ui.label(RichText::new("ALL CYLINDERS, IN FIRING ORDER").small().color(DIM)).on_hover_text(help::tip("firing"));
    cycle::firing_strip(ui, sim, g, &mut st.sel_cyl);
}

fn pv_tab(ui: &mut egui::Ui, st: &mut UiState, sim: &mut Sim, g: &RigGeom) {
    let n = sim.cfg().cylinder_count;
    st.sel_cyl = st.sel_cyl.min(n - 1);
    ui.horizontal(|ui| {
        if ui.small_button("<").clicked() {
            st.sel_cyl = (st.sel_cyl + n - 1) % n;
        }
        ui.label(RichText::new(format!("Cylinder {}", st.sel_cyl + 1)).strong());
        if ui.small_button(">").clicked() {
            st.sel_cyl = (st.sel_cyl + 1) % n;
        }
        ui.separator();
        for (s, name, tip) in [
            (PvScale::Linear, "linear", "Pressure and volume on plain axes: the classic picture. The big loop is the power the cylinder makes."),
            (PvScale::Log, "log-log", "Both axes logarithmic: compression and expansion become near-straight lines, and the low-pressure loop is easier to see."),
            (PvScale::Pumping, "pumping zoom", "Zoomed to the gas-exchange loop near atmospheric pressure: the energy spent breathing."),
        ] {
            ui.selectable_value(&mut st.pv_scale, s, name).on_hover_text(tip);
        }
    });
    cycle::pv_diagram(ui, sim, g, st.sel_cyl, st.pv_scale, Vec2::new(480.0, 250.0));
    ui.label(
        RichText::new("The area inside the loop is the work the gas does on the piston: green when the cylinder pushes the piston, red when the piston has to push the gas (the pumping loss). A turbo can turn the pumping loop green.")
            .size(11.5)
            .color(DIM),
    );
}

fn power_tab(ui: &mut egui::Ui, sim: &mut Sim) {
    ui.label(RichText::new("WHERE THE FUEL'S ENERGY GOES").small().color(DIM)).on_hover_text(help::tip("energy"));
    cycle::energy_bar(ui, sim, 480.0);
    ui.add_space(6.0);
    ui.label(RichText::new("ENGINEERING NUMBERS").small().color(DIM));
    cycle::engineering_numbers(ui, sim);
    ui.add_space(4.0);
    let n = sim.cfg().cylinder_count as f64;
    ui.colored_label(DIM, format!("Firing rate {:.0} Hz ({} cylinders at {:.0} rpm)", sim.tel.rpm / 120.0 * n, n, sim.tel.rpm))
        .on_hover_text("The fundamental frequency of the exhaust note: rpm / 120 x cylinders.");
}

// ------------------------------------------------------------------------------------------ faults

fn faults_window(ctx: &egui::Context, st: &mut UiState, sim: &mut Sim) {
    let mut open = true;
    let c = ctx.content_rect();
    crate::hud::window("FAULTS").default_pos([c.right() - 12.0, c.bottom() - 100.0]).pivot(Align2::RIGHT_BOTTOM).resizable(false).open(&mut open).show(ctx, |ui| {
        ui.colored_label(DIM, "Break things and watch what the ECU, the cycle graph and the sound do.");
        let n = sim.cfg().cylinder_count;
        st.fault_cyl = st.fault_cyl.min(n.saturating_sub(1));
        ui.horizontal(|ui| {
            ui.label("Cylinder");
            ui.add(egui::Slider::new(&mut st.fault_cyl, 0..=n - 1).custom_formatter(|v, _| format!("#{}", v as usize + 1)));
        });
        let label = (st.fault_cyl + 1) as u32;
        if sim.cfg().ignition_type == IgnitionType::Spark {
            if let Some(ch) = sim.engine.cylinder_ignition_mut(label) {
                ui.checkbox(&mut ch.connector.connected, "coil connector plugged in").on_hover_text("Unplug it and that cylinder cannot fire: a dead cylinder, rough running and a check-engine lamp.");
                ui.add(egui::Slider::new(&mut ch.connector.corrosion, 0.0..=1.0).text("connector corrosion")).on_hover_text("A corroded connector sometimes drops the spark.");
                ui.add(egui::Slider::new(&mut ch.coil.health, 0.0..=1.0).text("coil health")).on_hover_text("A weak coil can fail to make a spark under high pressure.");
                ui.checkbox(&mut ch.coil.shorted, "coil shorted");
                ui.add(egui::Slider::new(&mut ch.spark_plug.fouling, 0.0..=1.0).text("plug fouling")).on_hover_text("Deposits on the plug can short the spark.");
                ui.add(egui::Slider::new(&mut ch.spark_plug.electrode_wear, 0.0..=1.0).text("electrode wear")).on_hover_text("A worn electrode needs more voltage to jump the gap.");
                ui.add(egui::Slider::new(&mut ch.spark_plug.gap_mm, 0.3..=2.5).text("plug gap mm")).on_hover_text("A wider gap needs more voltage but makes a bigger spark. Stock is about 0.8 to 1.1 mm.");
            }
        } else {
            ui.colored_label(DIM, "Diesel: no spark ignition faults");
        }
        ui.separator();
        let mut clog = sim.tel.exhaust.catalyst_clog_fraction as f32;
        if ui.add(egui::Slider::new(&mut clog, 0.0..=1.0).text("catalyst clog")).on_hover_text("A blocked catalyst chokes the exhaust: backpressure, lost power, hotter exhaust.").changed() {
            sim.engine.set_catalyst_clog(clog as f64);
        }
        let mut o2 = sim.engine.exhaust.o2.connected;
        if ui.checkbox(&mut o2, "O2 sensor connected").on_hover_text(help::part("o2").map_or("", |p| p.detail)).changed() {
            sim.engine.exhaust.o2.connected = o2;
        }
        let mut mu = sim.engine.exhaust.muffler_restriction as f32;
        if ui.add(egui::Slider::new(&mut mu, 0.0..=4.0).text("muffler clog (fault)")).on_hover_text("A collapsed muffler baffle: more backpressure.").changed() {
            sim.engine.set_muffler_restriction(mu as f64);
        }
        if sim.cfg().ignition_type == IgnitionType::Spark {
            ui.collapsing("EVAP system (fuel vapour)", |ui| {
                let mut leak = sim.engine.evap.leak_fraction() as f32;
                if ui.add(egui::Slider::new(&mut leak, 0.0..=0.4).text("EVAP leak")).on_hover_text(help::tip("evap")).changed() {
                    sim.engine.evap.set_leak_fraction(leak as f64);
                }
                ui.checkbox(&mut sim.engine.evap.purge_valve.stuck_open, "purge valve stuck open");
                ui.checkbox(&mut sim.engine.evap.purge_valve.stuck_closed, "purge valve stuck closed");
                ui.checkbox(&mut sim.engine.evap.vent_valve.stuck_open, "vent valve stuck open");
                ui.add(egui::Slider::new(&mut sim.engine.evap.canister_saturation, 0.0..=1.0).text("canister load"));
            });
        }
        let mut fuel = sim.tel.fuel_level_fraction as f32;
        if ui.add(egui::Slider::new(&mut fuel, 0.0..=1.0).text("fuel level")).changed() {
            sim.engine.set_fuel_level(fuel as f64);
        }
        ui.horizontal(|ui| {
            if ui.button("Clear codes").clicked() {
                sim.engine.clear_cel();
            }
            ui.colored_label(DIM, "(faults return if still broken)");
        });
        if ui.button("Reset all faults").clicked() {
            sim.reset_faults();
        }
    });
    st.set(Panel::Faults, open);
}

// ------------------------------------------------------------------------------------------ welcome

fn welcome_window(ctx: &egui::Context, st: &mut UiState, sim: &mut Sim) {
    if !st.welcome {
        return;
    }
    let mut close = false;
    crate::hud::window("WELCOME").anchor(Align2::CENTER_CENTER, [0.0, -30.0]).title_bar(false).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.set_width(470.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("ENGINE SIM").size(30.0).strong().color(ACCENT));
            ui.label(RichText::new("A piston engine and drivetrain you can watch, hear, tune and take apart.").size(14.0));
        });
        ui.add_space(8.0);
        ui.label("Every pipe, valve and gas flow is simulated, and the 3D view is drawn from the same numbers as the sound and the graphs, so nothing is faked. Hover over anything to see what it means.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let big = |t: &str, col: Color32| egui::Button::new(RichText::new(t).strong().size(15.0)).fill(col).min_size(Vec2::new(148.0, 44.0));
            if ui.add(big("Start the engine", Color32::from_rgb(30, 110, 70))).on_hover_text("Cranks the starter for you; then press X for first gear and hold W").clicked() {
                sim.auto_start = true;
                close = true;
            }
            if ui.add(big("Guided tour", Color32::from_rgb(30, 90, 110))).on_hover_text("Lessons on how engines work").clicked() {
                learn::enter_step(st, sim, 0, 0);
                close = true;
            }
            if ui.add(big("Explore", Color32::from_rgb(50, 54, 68))).on_hover_text("Close this and look around").clicked() {
                close = true;
            }
        });
        ui.add_space(8.0);
        ui.colored_label(DIM, "Left-drag orbits, right-drag pans, scroll zooms.  L names the parts.  Tab hides the panels.");
        ui.add_space(2.0);
        ui.checkbox(&mut st.dont_show_welcome, "Don't show this again");
    });
    if close {
        st.welcome = false;
        st.save_prefs();
    }
}

// ------------------------------------------------------------------------------------------ gauges and charts

fn arc_pts(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn gauge(ui: &mut egui::Ui, size: f32, value: f32, min: f32, max: f32, red_from: Option<f32>, major: f32, unit: &str, readout: &str, label_div: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r = size * 0.5 - 4.0;
    p.circle_filled(c, r + 2.0, Color32::from_rgb(8, 9, 13));
    p.circle_stroke(c, r + 2.0, Stroke::new(1.5, Color32::from_rgb(60, 66, 82)));
    let a0 = PI * 0.75;
    let a1 = PI * 2.25;
    let to_a = |v: f32| a0 + (a1 - a0) * ((v - min) / (max - min)).clamp(0.0, 1.0);
    p.add(Shape::line(arc_pts(c, r - 8.0, a0, a1, 60), Stroke::new(3.0, Color32::from_rgb(40, 44, 56))));
    if let Some(rf) = red_from {
        p.add(Shape::line(arc_pts(c, r - 8.0, to_a(rf), a1, 20), Stroke::new(3.5, Color32::from_rgb(230, 50, 50))));
    }
    let mut v = min;
    while v <= max + 0.01 {
        let a = to_a(v);
        let d = Vec2::new(a.cos(), a.sin());
        p.line_segment([c + d * (r - 14.0), c + d * (r - 4.0)], Stroke::new(2.0, Color32::from_gray(190)));
        p.text(c + d * (r - 26.0), Align2::CENTER_CENTER, format!("{:.0}", v / label_div), FontId::proportional(size * 0.075), Color32::from_gray(200));
        v += major;
    }
    let a = to_a(value);
    let d = Vec2::new(a.cos(), a.sin());
    p.line_segment([c - d * 8.0, c + d * (r - 18.0)], Stroke::new(3.0, ACCENT));
    p.circle_filled(c, 6.0, Color32::from_gray(40));
    p.circle_filled(c, 3.0, ACCENT);
    p.text(c + Vec2::new(0.0, r * 0.45), Align2::CENTER_CENTER, readout, FontId::monospace(size * 0.12), Color32::WHITE);
    p.text(c + Vec2::new(0.0, r * 0.66), Align2::CENTER_CENTER, unit, FontId::proportional(size * 0.075), DIM);
    resp
}

fn dyno_chart(ui: &mut egui::Ui, dyno: &DynoTrace, sim: &Sim, jobs: &DynoJobs, size: Vec2, axes: bool) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.on_hover_text(help::tip("dyno"));
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, PANEL_BG);
    let title = if jobs.busy { "DYNO  (measuring...)" } else { "DYNO  stock vs now" };
    if axes {
        p.text(Pos2::new(rect.center().x, rect.top() + 4.0), Align2::CENTER_TOP, title, FontId::proportional(11.0), DIM);
    } else {
        p.text(rect.left_top() + Vec2::new(6.0, 4.0), Align2::LEFT_TOP, title, FontId::proportional(11.0), DIM);
    }
    let inner = rect.shrink2(Vec2::new(if axes { 40.0 } else { 30.0 }, 22.0));
    p.rect_stroke(inner, 0.0, Stroke::new(1.0, Color32::from_rgb(40, 44, 56)), egui::StrokeKind::Inside);
    let rpm_max = sim.cfg().redline_rpm as f32 + 500.0;
    let all = jobs.stock.iter().chain(jobs.tuned.iter());
    let tq_max = all.clone().map(|d| d.torque_nm as f32).fold(100.0f32, f32::max) * 1.1;
    let hp_max = all.map(|d| d.hp as f32).fold(50.0f32, f32::max) * 1.1;
    let live_tq = dyno.torque.iter().cloned().fold(0.0f32, f32::max);
    let tq_max = tq_max.max(live_tq * 1.05);
    let x = |rpm: f32| inner.left() + (rpm / rpm_max).clamp(0.0, 1.0) * inner.width();
    // grid
    for k in 1..(rpm_max / 1000.0) as i32 {
        let gx = x(k as f32 * 1000.0);
        p.line_segment([Pos2::new(gx, inner.top()), Pos2::new(gx, inner.bottom())], Stroke::new(1.0, Color32::from_rgb(22, 25, 34)));
        if axes {
            p.text(Pos2::new(gx, inner.bottom() + 3.0), Align2::CENTER_TOP, format!("{k}k"), FontId::proportional(9.0), DIM);
        }
    }
    if axes {
        for k in 0..=4 {
            let f = k as f32 / 4.0;
            let y = inner.bottom() - f * inner.height();
            if k > 0 {
                p.line_segment([Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)], Stroke::new(1.0, Color32::from_rgb(22, 25, 34)));
            }
            p.text(Pos2::new(inner.left() - 3.0, y), Align2::RIGHT_CENTER, format!("{:.0}", tq_max * f), FontId::proportional(9.0), ACCENT.gamma_multiply(0.85));
            p.text(Pos2::new(inner.right() + 3.0, y), Align2::LEFT_CENTER, format!("{:.0}", hp_max * f), FontId::proportional(9.0), TEAL.gamma_multiply(0.85));
        }
        p.text(Pos2::new(inner.left() - 3.0, inner.top() - 10.0), Align2::RIGHT_CENTER, "Nm", FontId::proportional(9.0), ACCENT);
        p.text(Pos2::new(inner.right() + 3.0, inner.top() - 10.0), Align2::LEFT_CENTER, "hp", FontId::proportional(9.0), TEAL);
        p.text(Pos2::new(inner.right(), inner.bottom() + 14.0), Align2::RIGHT_TOP, "rpm", FontId::proportional(9.0), DIM);
    }
    let line = |curve: &[engine_core::DynoPoint], tq: bool| -> Vec<Pos2> {
        curve
            .iter()
            .map(|d| {
                let (v, vmax) = if tq { (d.torque_nm as f32, tq_max) } else { (d.hp as f32, hp_max) };
                Pos2::new(x(d.rpm as f32), inner.bottom() - (v / vmax).clamp(0.0, 1.0) * inner.height())
            })
            .collect()
    };
    let faint = |c: Color32| c.gamma_multiply(0.38);
    if jobs.stock.len() > 1 {
        p.add(Shape::line(line(&jobs.stock, true), Stroke::new(1.2, faint(ACCENT))));
        p.add(Shape::line(line(&jobs.stock, false), Stroke::new(1.2, faint(TEAL))));
    }
    // what you actually drove through (live, wide-open-throttle bins), as dots
    for b in 0..120 {
        if dyno.seen[b] {
            let gx = x(b as f32 * 100.0);
            p.circle_filled(Pos2::new(gx, inner.bottom() - dyno.torque[b] / tq_max * inner.height()), 1.2, ACCENT.gamma_multiply(0.5));
        }
    }
    if jobs.tuned.len() > 1 {
        p.add(Shape::line(line(&jobs.tuned, true), Stroke::new(2.0, ACCENT)));
        p.add(Shape::line(line(&jobs.tuned, false), Stroke::new(2.0, TEAL)));
        if let Some(pk) = peak(&jobs.tuned) {
            let pos = Pos2::new(x(pk.hp_rpm as f32), inner.bottom() - (pk.hp as f32 / hp_max) * inner.height());
            p.circle_stroke(pos, 3.5, Stroke::new(1.5, Color32::WHITE));
            if axes {
                p.text(pos + Vec2::new(0.0, -7.0), Align2::CENTER_BOTTOM, format!("{:.0} hp @ {:.0}", pk.hp, pk.hp_rpm), FontId::proportional(9.5), Color32::WHITE);
                let tpos = Pos2::new(x(pk.tq_rpm as f32), inner.bottom() - (pk.tq_nm as f32 / tq_max) * inner.height());
                p.circle_stroke(tpos, 3.0, Stroke::new(1.2, ACCENT));
                p.text(tpos + Vec2::new(0.0, -6.0), Align2::CENTER_BOTTOM, format!("{:.0} Nm @ {:.0}", pk.tq_nm, pk.tq_rpm), FontId::proportional(9.5), ACCENT);
            }
        }
    }
    let rx = x(sim.tel.rpm as f32);
    p.line_segment([Pos2::new(rx, inner.top()), Pos2::new(rx, inner.bottom())], Stroke::new(1.0, Color32::from_white_alpha(60)));
    let txt = match (peak(&jobs.stock), peak(&jobs.tuned)) {
        (Some(a), Some(b)) => format!("{:.0} -> {:.0} hp  ({:+.0}%)   {:.0} -> {:.0} Nm", a.hp, b.hp, (b.hp / a.hp - 1.0) * 100.0, a.tq_nm, b.tq_nm),
        (_, Some(b)) => format!("{:.0} hp  {:.0} Nm", b.hp, b.tq_nm),
        _ => String::new(),
    };
    if axes {
        p.text(rect.right_top() + Vec2::new(-6.0, 4.0), Align2::RIGHT_TOP, txt, FontId::proportional(9.5), DIM);
    } else {
        p.text(rect.left_bottom() + Vec2::new(6.0, -4.0), Align2::LEFT_BOTTOM, txt, FontId::proportional(10.0), DIM);
    }
}

// ---------------------------------------------------------------------------- tuning

/// A selector with a tooltip.
fn pick<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, tip: &str, cur: &mut T, all: &[T], name: impl Fn(T) -> &'static str) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(name(*cur))
        .width(190.0)
        .show_ui(ui, |ui| {
            for &v in all {
                if ui.selectable_label(*cur == v, name(v)).clicked() {
                    *cur = v;
                    changed = true;
                }
            }
        })
        .response
        .on_hover_text(help::tip(tip));
    changed
}

#[derive(Clone, Copy)]
enum Preset {
    Stock,
    Street,
    Sport,
    TurboKit,
    Race,
}

fn preset(kind: Preset, cfg: &EngineConfig) -> Tuning {
    let stock_n = stock_turbo_count(cfg);
    let stock_boost = cfg.turbo.as_ref().map_or(0.7, |t| t.max_boost_pa / 1.0e5);
    let max_n = cfg.cylinder_count.min(4) as u8;
    let mut t = Tuning::default();
    match kind {
        Preset::Stock => {}
        Preset::Street => {
            t.air_filter = AirFilter::ColdAir;
            t.header = Header::Shorty;
            t.cat = Cat::HighFlow;
            t.muffler = Muffler::Sport;
            t.pipe_scale = 1.1;
        }
        Preset::Sport => {
            t.air_filter = AirFilter::ColdAir;
            t.runners = Runners::ShortRunner;
            t.header = Header::LongTube;
            t.cat = Cat::HighFlow;
            t.muffler = Muffler::Sport;
            t.pipe_scale = 1.2;
        }
        Preset::TurboKit => {
            t = preset(Preset::Street, cfg);
            t.turbo_count = Some((stock_n + 1).clamp(1, max_n));
            t.boost_bar = Some((stock_boost + 0.2).min(1.2));
            t.intercooler = Intercooler::Large;
        }
        Preset::Race => {
            t.air_filter = AirFilter::OpenStack;
            t.runners = Runners::Itb;
            t.header = Header::LongTube;
            t.cat = Cat::Delete;
            t.muffler = Muffler::Straight;
            t.pipe_scale = 1.3;
            t.turbo_count = Some(stock_n.max(2).min(max_n));
            t.boost_bar = Some((stock_boost + 0.5).min(1.7));
            t.intercooler = Intercooler::Large;
        }
    }
    t
}

fn tuning_window(ctx: &egui::Context, st: &mut UiState, sim: &mut Sim, jobs: &DynoJobs) {
    let mut open = true;
    crate::hud::window("TUNING").default_pos([290.0, 56.0]).resizable(false).collapsible(true).open(&mut open).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        let cfg = sim.cfg().clone();
        let mut t = sim.tuning.clone();
        // `discrete`: a part swap, so the 3D rig is rebuilt right away. Sliders update the sim
        // live and bring the rig in line once they are let go.
        let mut discrete = false;
        let mut released = false;

        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [("Stock", Preset::Stock), ("Street", Preset::Street), ("Sport", Preset::Sport), ("Turbo kit", Preset::TurboKit), ("Race", Preset::Race)] {
                if ui.button(label).on_hover_text(help::tip("t_presets")).clicked() {
                    t = preset(kind, &cfg);
                    discrete = true;
                }
            }
        });
        ui.separator();

        ui.label(RichText::new("INTAKE").small().color(DIM));
        discrete |= pick(ui, "Air filter", "t_filter", &mut t.air_filter, &AirFilter::ALL, |v| v.label());
        discrete |= pick(ui, "Manifold", "t_runners", &mut t.runners, &Runners::ALL, |v| v.label());

        ui.add_space(2.0);
        ui.label(RichText::new("EXHAUST").small().color(DIM));
        discrete |= pick(ui, "Header", "t_header", &mut t.header, &Header::ALL, |v| v.label());
        discrete |= pick(ui, "Catalyst", "t_cat", &mut t.cat, &Cat::ALL, |v| v.label());
        discrete |= pick(ui, "Muffler", "t_muffler", &mut t.muffler, &Muffler::ALL, |v| v.label());
        let mut pipe_pct = (t.pipe_scale * 100.0) as f32;
        let r = ui.add(egui::Slider::new(&mut pipe_pct, 85.0..=135.0).text("pipe diameter").suffix("%")).on_hover_text(help::tip("t_pipe_dia"));
        if r.changed() {
            t.pipe_scale = (pipe_pct / 100.0) as f64;
        }
        released |= r.drag_stopped();

        ui.add_space(2.0);
        ui.label(RichText::new("EXHAUST ACOUSTICS").small().color(DIM));
        // header metal: the stock choice for the header style, or any pipe material
        let natural = t.header.natural_material();
        let shown = match t.header_material {
            None => format!("{} (stock)", natural.label()),
            Some(m) => m.label().to_string(),
        };
        egui::ComboBox::from_label("Header metal")
            .selected_text(shown)
            .width(190.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(t.header_material.is_none(), format!("{} (stock)", natural.label())).clicked() {
                    t.header_material = None;
                }
                for m in PipeMaterial::ALL {
                    if ui.selectable_label(t.header_material == Some(m), m.label()).clicked() {
                        t.header_material = Some(m);
                    }
                }
            })
            .response
            .on_hover_text(help::tip("t_header_metal"));
        pick(ui, "Pipe metal", "t_pipe_metal", &mut t.pipe_material, &PipeMaterial::PIPES, |v| v.label());
        let slider = |ui: &mut egui::Ui, value: &mut f64, lo: f32, hi: f32, label: &str, tip: &str| {
            let mut pct = (*value * 100.0) as f32;
            let r = ui.add(egui::Slider::new(&mut pct, lo..=hi).text(label).suffix("%")).on_hover_text(help::tip(tip));
            if r.changed() {
                *value = (pct / 100.0) as f64;
            }
        };
        slider(ui, &mut t.wall_scale, 50.0, 200.0, "wall thickness", "t_wall");
        slider(ui, &mut t.primary_len_scale, 60.0, 160.0, "primary length", "t_primary_len");
        slider(ui, &mut t.primary_dia_scale, 80.0, 130.0, "primary diameter", "t_primary_dia");
        slider(ui, &mut t.mid_len_scale, 50.0, 180.0, "mid-pipe length", "t_mid");
        slider(ui, &mut t.tail_len_scale, 40.0, 200.0, "tailpipe length", "t_tail");
        ui.checkbox(&mut t.header_wrap, "heat-wrap the headers").on_hover_text(help::tip("t_wrap"));
        if cfg.cylinder_count >= 4 && engine_core::bank_tilts_deg(&cfg.id).len() > 1 {
            pick(ui, "Crossover", "t_crossover", &mut t.crossover, &Crossover::ALL, |v| v.label());
        }

        ui.add_space(2.0);
        ui.label(RichText::new("FORCED INDUCTION").small().color(DIM));
        let max_n = cfg.cylinder_count.min(4) as u8;
        let stock_n = stock_turbo_count(&cfg);
        let cur_n = t.turbo_count_for(&cfg);
        ui.horizontal(|ui| {
            ui.label("Turbos").on_hover_text(help::tip("t_turbos"));
            for k in 0..=max_n {
                let txt = if k == 0 { "none".to_string() } else { k.to_string() };
                if ui.selectable_label(cur_n == k, txt).clicked() && cur_n != k {
                    t.turbo_count = if k == stock_n { None } else { Some(k) };
                    discrete = true;
                }
            }
            if stock_n > 0 {
                ui.colored_label(DIM, format!("(stock {stock_n})"));
            }
        });
        if cur_n > 0 {
            let mut size = t.turbo_size as f32;
            let r = ui
                .add(egui::Slider::new(&mut size, 0.6..=1.6).text("turbo size").custom_formatter(|v, _| {
                    if v < 0.85 {
                        format!("{v:.2}  quick")
                    } else if v > 1.25 {
                        format!("{v:.2}  big")
                    } else {
                        format!("{v:.2}")
                    }
                }))
                .on_hover_text(help::tip("t_turbo_size"));
            if r.changed() {
                t.turbo_size = size as f64;
            }
            released |= r.drag_stopped();
            let mut boost = t.boost_bar_for(&cfg) as f32;
            let r = ui.add(egui::Slider::new(&mut boost, BOOST_MIN_BAR as f32..=BOOST_MAX_BAR as f32).text("boost").suffix(" bar")).on_hover_text(help::tip("t_boost"));
            if r.changed() {
                t.boost_bar = Some(boost as f64);
            }
            discrete |= pick(ui, "Intercooler", "t_intercooler", &mut t.intercooler, &Intercooler::ALL, |v| v.label());
        }

        // live readouts
        let tel = &sim.tel;
        ui.add_space(2.0);
        ui.separator();
        if tel.turbo_count > 0 {
            ui.colored_label(
                DIM,
                format!(
                    "boost {:.2} / {:.2} bar   spool {:.0}%\nflow holds to {:.0} rpm   charge {:.0} C",
                    tel.boost_pressure_pa / 1.0e5,
                    tel.boost_target_pa / 1.0e5,
                    tel.turbo_spool_fraction * 100.0,
                    tel.turbo_choke_rpm.min(99_999.0),
                    tel.charge_temp_k - 273.15,
                ),
            );
        }
        // what the sound model says about this exhaust, live
        if let Ok(mut a) = sim.audio.lock() {
            let d = a.diag.clone();
            if d.cells > 0 {
                let primary_rpm = Tuning::tuned_rpm(&cfg, d.primary_m as f64);
                ui.colored_label(
                    DIM,
                    format!(
                        "pipe {:.1} m valve to tip, quarter-wave drone ~{:.0} Hz\nprimaries {:.2} m tune to ~{:.0} rpm\ngas {:.0} C at the headers, {:.0} C at the tip{}",
                        d.exhaust_path_m,
                        d.drone_hz,
                        d.primary_m,
                        primary_rpm,
                        d.header_gas_k - 273.15,
                        d.tail_gas_k - 273.15,
                        if d.resonator_hz > 0.0 { format!("\nresonator {:.0} Hz", d.resonator_hz) } else { String::new() }
                    ),
                );
                ui.colored_label(
                    DIM,
                    format!(
                        "sound solver ({}): {} cells, {} nodes, {:.0}% of a core{}",
                        ["", "ultra", "high", "balanced", "fast", "eco"][d.quality.min(5) as usize],
                        d.cells,
                        d.nodes,
                        d.load * 100.0,
                        if d.underruns > 0 { format!(", {} underruns", d.underruns) } else { String::new() }
                    ),
                );
            }
            let mut q = a.quality_pref;
            ui.horizontal(|ui| {
                ui.label("sound quality").on_hover_text(help::tip("t_quality"));
                for (v, name) in [(0u8, "auto"), (1, "ultra"), (2, "high"), (3, "balanced"), (4, "fast"), (5, "eco")] {
                    if ui.selectable_label(q == v, name).clicked() {
                        q = v;
                    }
                }
            });
            a.quality_pref = q;
        }
        if let (Some(a), Some(b)) = (peak(&jobs.stock), peak(&jobs.tuned)) {
            let d = b.hp - a.hp;
            let col = if d > 0.5 {
                TEAL
            } else if d < -0.5 {
                Color32::from_rgb(255, 110, 90)
            } else {
                DIM
            };
            ui.colored_label(col, format!("peak {:.0} hp @ {:.0}   {:+.0} hp vs stock\npeak {:.0} Nm @ {:.0}", b.hp, b.hp_rpm, d, b.tq_nm, b.tq_rpm));
        }
        if tel.is_knocking {
            ui.colored_label(ACCENT, "knock: pull boost, cool the charge or use a higher octane fuel");
        }
        if t.is_cat_deleted() {
            ui.colored_label(DIM, "cat deleted: louder, harsher, pops more");
        }

        if t != sim.tuning {
            sim.set_tuning(t, discrete || released);
        } else if released {
            // slider let go: bring the 3D rig in line with the final value
            let t = sim.tuning.clone();
            sim.set_tuning(t, true);
        }
    });
    st.set(Panel::Tuning, open);
}
