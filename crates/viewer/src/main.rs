mod audio;
mod chamber;
mod cam;
mod cycle;
mod dyno_bg;
mod fx;
mod gas;
mod help;
mod hud;
mod labels;
mod layout;
mod learn;
mod look;
mod meshgen;
mod puffs;
mod rig;
mod scene;
mod seethrough;
mod sim;
mod ui;
mod volume;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use sim::{AudioState, Sim};
use std::sync::{Arc, Mutex};

/// Command-line options, mostly for automated screenshots.
#[derive(Resource, Default, Clone)]
struct Cli {
    engine: Option<String>,
    shot: Option<String>,
    after: f32,
    throttle: Option<f32>,
    gear: Option<usize>,
    preset: Option<usize>,
    yaw: Option<f32>,
    pitch: Option<f32>,
    dist: Option<f32>,
    target: Option<Vec3>,
    time_scale: Option<f32>,
    cap: Option<f32>,
    shift_at: Option<f32>,
    unplug: Option<u32>,
    switch_engine: Option<String>,
    tune: Option<String>,
    /// Applied half-way through a `--shot` run, like a player swapping parts mid-session.
    tune_late: Option<String>,
    /// Lift off the throttle at this time (seconds) in a `--shot` run, to catch overrun pops.
    lift_at: Option<f32>,
    /// `--ui learn,labels,tuning,analysis,faults,glossary,engine,telemetry`: panels to open in a `--shot` run.
    ui: Option<String>,
    /// `--lesson L:S`: start lesson L (0-based) at step S in a `--shot` run.
    lesson: Option<(usize, usize)>,
    /// `--tab cycle|pv|dyno|power`, `--pv log|pumping`, `--cyl N` (1-based), `--scrub DEG` (selected cylinder's angle).
    tab: Option<String>,
    pv: Option<String>,
    cyl: Option<usize>,
    scrub: Option<f32>,
    welcome: bool,
    hover: Option<(f32, f32)>,
}

/// `--tune turbos=2,size=0.9,boost=1.0,filter=cold,runners=itb,header=long,cat=delete,muffler=sport,pipe=1.2,ic=large,`
/// `pipemat=ti,headermat=ss304,wall=1.3,wrap=1,plen=0.8,pdia=1.1,mid=1.3,tail=1.5,cross=x` (see `Tuning::parse_spec`)
fn parse_tuning(spec: &str) -> engine_core::Tuning {
    engine_core::Tuning::parse_spec(spec).unwrap_or_else(|e| {
        eprintln!("--tune: {e}");
        engine_core::Tuning::default()
    })
}

fn parse_cli() -> Cli {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let f = |k: &str| get(k).and_then(|v| v.parse::<f32>().ok());
    Cli {
        engine: get("--engine"),
        shot: get("--shot"),
        after: f("--after").unwrap_or(3.0),
        throttle: f("--throttle"),
        gear: get("--gear").and_then(|v| v.parse().ok()),
        preset: get("--preset").and_then(|v| v.parse().ok()),
        yaw: f("--yaw"),
        pitch: f("--pitch"),
        dist: f("--dist"),
        target: get("--target").and_then(|v| {
            let p: Vec<f32> = v.split(',').filter_map(|s| s.parse().ok()).collect();
            (p.len() == 3).then(|| Vec3::new(p[0], p[1], p[2]))
        }),
        time_scale: f("--timescale"),
        cap: f("--cap"),
        shift_at: f("--shift-at"),
        unplug: get("--unplug").and_then(|v| v.parse().ok()),
        switch_engine: get("--switch-engine"),
        tune: get("--tune"),
        tune_late: get("--tune-late"),
        lift_at: f("--lift-at"),
        ui: get("--ui"),
        lesson: get("--lesson").and_then(|v| {
            let (l, st) = v.split_once(':').unwrap_or((v.as_str(), "0"));
            Some((l.parse().ok()?, st.parse().ok()?))
        }),
        tab: get("--tab"),
        pv: get("--pv"),
        cyl: get("--cyl").and_then(|v| v.parse().ok()),
        scrub: f("--scrub"),
        welcome: a.iter().any(|x| x == "--welcome"),
        hover: get("--hover").and_then(|v| {
            let (x, y) = v.split_once(',')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        }),
    }
}

fn main() {
    let cli = parse_cli();
    let audio = Arc::new(Mutex::new(AudioState::default()));
    let engine_id = cli.engine.clone().unwrap_or_else(|| "v8".into());
    let mut sim = Sim::new(audio.clone(), &engine_id);
    if cli.shot.is_none() {
        if let Some(ts) = cli.time_scale {
            sim.time_scale = ts;
        }
    }
    if let Some(spec) = &cli.tune {
        sim.set_tuning(parse_tuning(spec), false);
    }
    sim.hide_ui = std::env::args().any(|a| a == "--hideui");
    if let Some(c) = cli.cap {
        sim.visual_cap_rpm = c;
    }
    let shot_mode = cli.shot.is_some();
    let mute_all = std::env::args().any(|a| a == "--nosound");
    // `--sound` keeps the sound model running during a scripted `--shot` run (for testing the live audio path)
    let sound_in_shot = std::env::args().any(|a| a == "--sound");
    let audio_on = (!shot_mode || sound_in_shot) && !mute_all;
    sim.audio_live = audio_on;
    let see_off = std::env::args().any(|a| a == "--no-seethrough");
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Engine Sim".into(), resolution: (1600, 900).into(), ..default() }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(volume::VolumePlugin)
        .add_plugins(chamber::ChamberPlugin)
        .add_plugins(gas::GasPlugin)
        .init_resource::<ui::DynoTrace>()
        .init_resource::<hud::UiState>()
        .init_resource::<fx::FireGlow>()
        .init_resource::<puffs::PuffPool>()
        .init_resource::<dyno_bg::DynoJobs>()
        .insert_resource(seethrough::SeeThrough { enabled: !see_off })
        .add_systems(EguiPrimaryContextPass, ui::ui_system)
        .insert_resource(ClearColor(Color::srgb(0.01, 0.012, 0.016)))
        .insert_resource(sim)
        .insert_resource(cli)
        .insert_resource(cam::CamRig { locked: shot_mode, shake: if shot_mode { 0.0 } else { 0.6 }, ..default() })
        .init_resource::<cam::Anchors>()
        .init_resource::<ScriptState>()
        .add_systems(Startup, (setup_mats, puffs::setup_puff_tex, scene::setup_scene).chain())
        .add_systems(
            Update,
            (
                keyboard_controls,
                scripted_run,
                sim_step,
                dyno_bg::dyno_jobs_system,
                rig::rig_system,
                chamber::attach_flame_materials,
                (
                    rig::anim::spin_system,
                    rig::anim::piston_system,
                    rig::anim::valve_system,
                    rig::anim::flame_system,
                    rig::anim::throttle_plate_system,
                    rig::anim::heat_glow_system,
                    rig::anim::pressure_tint_system,
                    rig::anim::clutch_system,
                    rig::anim::gear_highlight_system,
                    rig::anim::brake_glow_system,
                    rig::anim::boost_glow_system,
                    rig::accessories::belt_mark_system,
                ),
                (volume::setup_plume, gas::gas_system, puffs::puff_system, volume::plume_system, rig::anim::spark_bolt_system),
                fx::fire_light_system,
                cam::camera_input,
                cam::camera_update,
                seethrough::see_through_system,
                cam::scroll_floor,
            )
                .chain(),
        );
    // the labels are projected from inside the egui pass, so it must see this frame's transforms
    app.configure_sets(PostUpdate, bevy_egui::EguiPostUpdateSet::EndPass.after(bevy::transform::TransformSystems::Propagate));
    if audio_on {
        app.add_plugins(audio::EngineAudioPlugin { state: audio });
    }
    app.run();
}

fn setup_mats(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(look::Mats::new(&mut materials));
}

fn sim_step(mut sim: ResMut<Sim>, time: Res<Time>) {
    sim.step(time.delta_secs());
}

fn keyboard_controls(mut sim: ResMut<Sim>, keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, wants: Res<bevy_egui::input::EguiWantsInput>) {
    let dt = time.delta_secs();
    // typing in a text box (the glossary search) must not drive the car
    if wants.wants_keyboard_input() {
        sim.starter_held = sim.starter_held_by_ui || sim.starter_script || sim.auto_start;
        return;
    }
    sim.starter_held = sim.starter_held_by_ui || sim.starter_script || sim.auto_start || keys.pressed(KeyCode::KeyE);
    if keys.just_pressed(KeyCode::KeyK) {
        sim.engine.shut_off();
    }
    let up = keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp);
    if up {
        sim.throttle_input = (sim.throttle_input + 1.8 * dt).min(1.0);
    } else if !sim.throttle_slider_active {
        sim.throttle_input = (sim.throttle_input - 5.0 * dt).max(0.0);
    }
    let brake = keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) || keys.pressed(KeyCode::Space);
    if brake {
        sim.brake_input = (sim.brake_input + 5.0 * dt).min(1.0);
    } else {
        sim.brake_input = (sim.brake_input - 6.0 * dt).max(0.0);
    }
    if keys.just_pressed(KeyCode::KeyX) || keys.just_pressed(KeyCode::PageUp) {
        sim.shift_up();
    }
    if keys.just_pressed(KeyCode::KeyZ) || keys.just_pressed(KeyCode::PageDown) {
        sim.shift_down();
    }
    if keys.just_pressed(KeyCode::KeyN) {
        sim.set_gear_index(1);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        sim.set_gear_index(0);
    }
    if keys.just_pressed(KeyCode::KeyP) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        sim.time_scale = (sim.time_scale * 0.5).max(0.01);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        sim.time_scale = (sim.time_scale * 2.0).min(1.0);
    }
}

#[derive(Resource, Default)]
struct ScriptState {
    started: bool,
    geared: bool,
    shot_taken: bool,
    exit_at: Option<f32>,
    cam_set: bool,
    run_since: Option<f32>,
    switched: bool,
    tuned_late: bool,
    ui_set: bool,
    lesson_started: bool,
    scrubbed: bool,
}

/// `--ui`, `--tab`, `--pv`, `--cyl`, `--welcome` for `--shot` runs: open the panels a screenshot should show.
fn apply_ui_flags(cli: &Cli, ui: &mut hud::UiState) {
    use hud::{Panel, Tab};
    if let Some(list) = &cli.ui {
        for p in Panel::ALL {
            ui.set(p, false);
        }
        for name in list.split(',') {
            match name.trim() {
                "engine" => ui.set(Panel::Engine, true),
                "tuning" => ui.set(Panel::Tuning, true),
                "telemetry" => ui.set(Panel::Telemetry, true),
                "analysis" => ui.set(Panel::Analysis, true),
                "faults" => ui.set(Panel::Faults, true),
                "learn" => ui.set(Panel::Learn, true),
                "glossary" => ui.set(Panel::Glossary, true),
                "labels" => ui.labels = true,
                _ => {}
            }
        }
    }
    if let Some(t) = &cli.tab {
        ui.tab = match t.as_str() {
            "pv" => Tab::Pv,
            "dyno" => Tab::Dyno,
            "power" => Tab::Power,
            _ => Tab::Cycle,
        };
    }
    if let Some(p) = &cli.pv {
        ui.pv_scale = match p.as_str() {
            "log" => cycle::PvScale::Log,
            "pumping" => cycle::PvScale::Pumping,
            _ => cycle::PvScale::Linear,
        };
    }
    if let Some(c) = cli.cyl {
        ui.sel_cyl = c.saturating_sub(1);
    }
    if cli.welcome {
        ui.welcome = true;
    }
    ui.hover_override = cli.hover;
}

/// Automation for `--shot`: start the engine, optionally engage a gear / hold throttle, grab a
/// screenshot, quit.
#[allow(clippy::too_many_arguments)]
fn scripted_run(
    mut st: ResMut<ScriptState>,
    cli: Res<Cli>,
    time: Res<Time<Real>>,
    mut sim: ResMut<Sim>,
    mut cam: ResMut<cam::CamRig>,
    anchors: Res<cam::Anchors>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    frames: Res<bevy::diagnostic::FrameCount>,
    mut ui: ResMut<hud::UiState>,
) {
    let t = time.elapsed_secs();
    if cli.shot.is_none() {
        return;
    }
    if !st.ui_set && !anchors.0.is_empty() {
        st.ui_set = true;
        apply_ui_flags(&cli, &mut ui);
    }
    if let (Some((l, s)), false, true) = (cli.lesson, st.lesson_started, t > 0.5 && !anchors.0.is_empty()) {
        // a lesson sets the engine up itself, so the usual start-up script stays out of the way
        st.lesson_started = true;
        st.started = true;
        st.geared = true;
        // run every earlier step's set-up too, as a player clicking through would have
        for i in 0..=s {
            learn::enter_step(&mut ui, &mut sim, l, i);
        }
    }
    if let (Some(deg), false, true) = (cli.scrub, st.scrubbed, t > cli.after * 0.6) {
        st.scrubbed = true;
        let ci = ui.sel_cyl;
        sim.scrub_to(ci, deg);
    }
    if !st.cam_set && !anchors.0.is_empty() {
        st.cam_set = true;
        if let Some(p) = cli.preset {
            let idx = p.min(anchors.0.len() - 1);
            let a = anchors.0[idx].clone();
            cam.goto(&a);
            cam.preset = idx;
        }
        if let Some(v) = cli.yaw {
            cam.t_yaw = v;
        }
        if let Some(v) = cli.pitch {
            cam.t_pitch = v;
        }
        if let Some(v) = cli.dist {
            cam.t_dist = v;
        }
        if let Some(v) = cli.target {
            cam.t_target = v;
        }
        cam.target = cam.t_target;
        cam.yaw = cam.t_yaw;
        cam.pitch = cam.t_pitch;
        cam.dist = cam.t_dist;
    }
    if t > 0.3 && !st.started && cli.lesson.is_none() {
        st.started = true;
        sim.starter_script = true;
    }
    if st.started && !st.geared && sim.tel.rpm > 650.0 {
        sim.starter_script = false;
        st.run_since.get_or_insert(t);
    }
    if let Some(rs) = st.run_since {
        if !st.geared && t > rs + 1.2 {
            st.geared = true;
            if let Some(ts) = cli.time_scale {
                sim.time_scale = ts;
            }
            if let Some(g) = cli.gear {
                sim.set_gear_index(g);
            }
            if let Some(th) = cli.throttle {
                sim.throttle_input = th;
                sim.throttle_slider_active = true;
            }
        }
    }
    if t > cli.after * 0.45 && !st.tuned_late {
        st.tuned_late = true;
        if let Some(spec) = &cli.tune_late {
            sim.set_tuning(parse_tuning(spec), true);
        }
    }
    if t > cli.after * 0.5 && !st.switched {
        st.switched = true;
        if let Some(id) = &cli.switch_engine {
            if let Some(i) = sim.engines.iter().position(|e| &e.id == id) {
                sim.select_engine(i);
                st.started = false;
                st.geared = false;
                st.run_since = None;
            }
        }
    }
    if let (Some(n), true) = (cli.unplug, st.geared) {
        if let Some(ch) = sim.engine.cylinder_ignition_mut(n) {
            ch.connector.connected = false;
        }
    }
    if t > cli.after && !st.shot_taken {
        st.shot_taken = true;
        eprintln!("frames={} t={:.1} avg_fps={:.1} sim_time={:.2} rpm={:.0}", frames.0, t, frames.0 as f32 / t, sim.sim_time, sim.tel.rpm);
        eprintln!("CAM target={:?} yaw={} pitch={} dist={} preset={}", cam.target, cam.yaw, cam.pitch, cam.dist, cam.preset);
        if let Some(path) = &cli.shot {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        }
        st.exit_at = Some(t + 2.0);
    }
    if let Some(e) = st.exit_at {
        if t > e {
            exit.write(AppExit::Success);
        }
    }
    if let Some(l) = cli.lift_at {
        if t > l {
            sim.throttle_input = 0.0;
            sim.throttle_slider_active = false;
        }
    }
    let _ = cli.shift_at;
}
