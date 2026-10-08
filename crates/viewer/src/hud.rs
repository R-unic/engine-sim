//! State of the on-screen interface: which panels are open, the analysis tab, the selected cylinder,
//! the active lesson, and a few preferences remembered between runs.

use crate::cycle::PvScale;
use crate::labels::Pinned;
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Engine,
    Tuning,
    Telemetry,
    Analysis,
    Faults,
    Learn,
    Glossary,
}

impl Panel {
    pub const ALL: [Panel; 7] = [Panel::Engine, Panel::Tuning, Panel::Telemetry, Panel::Analysis, Panel::Faults, Panel::Learn, Panel::Glossary];

    pub fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Cycle,
    Pv,
    Dyno,
    Power,
}

/// Which lesson step the player is on.
#[derive(Clone, Copy, Default)]
pub struct Active {
    pub lesson: usize,
    pub step: usize,
}

#[derive(Resource)]
pub struct UiState {
    pub styled: bool,
    pub show_ui: bool,
    pub open: [bool; 7],
    /// Name the parts in the 3D view.
    pub labels: bool,
    pub tab: Tab,
    pub sel_cyl: usize,
    pub pv_scale: PvScale,
    pub fault_cyl: usize,
    pub last_engine: usize,
    pub ui_scale: f32,
    pub pinned: Option<Pinned>,
    pub welcome: bool,
    pub dont_show_welcome: bool,
    pub lesson: Option<Active>,
    /// A camera move waiting for the rig to finish rebuilding (an engine swap changes the anchors).
    pub pending_cam: Option<crate::learn::Cam>,
    pub glossary_filter: String,
    pub coach: bool,
    /// `--hover x,y`: pretend the pointer is here (screenshot runs have no mouse).
    pub hover_override: Option<(f32, f32)>,
}

impl Default for UiState {
    fn default() -> Self {
        let prefs = Prefs::load();
        let shot = std::env::args().any(|a| a == "--shot");
        Self {
            styled: false,
            show_ui: true,
            // the tuning and fault panels are for later: start with the essentials
            open: [true, false, true, true, false, false, false],
            labels: false,
            tab: Tab::Cycle,
            sel_cyl: 0,
            pv_scale: PvScale::Linear,
            fault_cyl: 0,
            last_engine: usize::MAX,
            ui_scale: prefs.ui_scale,
            pinned: None,
            welcome: !prefs.skip_welcome && !shot,
            dont_show_welcome: prefs.skip_welcome,
            lesson: None,
            pending_cam: None,
            glossary_filter: String::new(),
            coach: true,
            hover_override: None,
        }
    }
}

impl UiState {
    pub fn is_open(&self, p: Panel) -> bool {
        self.open[p.idx()]
    }

    pub fn set(&mut self, p: Panel, v: bool) {
        self.open[p.idx()] = v;
    }

    pub fn toggle(&mut self, p: Panel) {
        self.open[p.idx()] ^= true;
    }

    pub fn save_prefs(&self) {
        Prefs { ui_scale: self.ui_scale, skip_welcome: self.dont_show_welcome }.save();
    }
}

/// Settings kept in a small text file next to the user's other app data. Failing to read or write
/// it only means the defaults come back, so errors are ignored.
#[derive(Clone, Copy)]
pub struct Prefs {
    pub ui_scale: f32,
    pub skip_welcome: bool,
}

impl Prefs {
    fn path() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).or_else(|| std::env::var_os("HOME"))?;
        Some(std::path::PathBuf::from(base).join("engine-sim").join("prefs.txt"))
    }

    pub fn load() -> Self {
        let mut p = Prefs { ui_scale: 1.0, skip_welcome: false };
        // shot runs and tests should always start from the same place
        if std::env::args().any(|a| a == "--shot" || a == "--hideui") {
            return p;
        }
        if let Some(text) = Self::path().and_then(|f| std::fs::read_to_string(f).ok()) {
            for line in text.lines() {
                match line.split_once('=') {
                    Some(("ui_scale", v)) => p.ui_scale = v.trim().parse().unwrap_or(1.0f32).clamp(0.7, 2.0),
                    Some(("skip_welcome", v)) => p.skip_welcome = v.trim() == "1",
                    _ => {}
                }
            }
        }
        p
    }

    pub fn save(&self) {
        if std::env::args().any(|a| a == "--shot") {
            return;
        }
        if let Some(f) = Self::path() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(f, format!("ui_scale={}\nskip_welcome={}\n", self.ui_scale, if self.skip_welcome { 1 } else { 0 }));
        }
    }
}

/// Every panel is a window with an explicit id, so the label layout can look up where it is on screen.
pub fn win_id(title: &str) -> bevy_egui::egui::Id {
    bevy_egui::egui::Id::new(("hud_window", title.to_string()))
}

pub fn window(title: &'static str) -> bevy_egui::egui::Window<'static> {
    bevy_egui::egui::Window::new(title).id(win_id(title))
}
