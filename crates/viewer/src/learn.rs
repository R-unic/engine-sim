//! Guided lessons: short sequences of steps that set the engine up, move the camera, explain what is
//! on screen and ask the player to try something, with a live tick when they have.

use crate::cam::{Anchor, Anchors, CamRig};
use crate::cycle::PvScale;
use crate::hud::{Active, Panel, Tab, UiState};
use crate::rig::anim::RigGeom;
use crate::rig::engine_build::bank_to_engine;
use crate::sim::Sim;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText, Vec2};
use engine_core::Tuning;

const ACCENT: Color32 = Color32::from_rgb(255, 150, 40);
const TEAL: Color32 = Color32::from_rgb(70, 210, 190);
const DIM: Color32 = Color32::from_rgb(150, 155, 165);

#[derive(Clone, Copy)]
pub enum Cam {
    /// One of the rig's named views.
    Anchor(&'static str),
    /// A close view of the selected cylinder.
    Cylinder,
}

#[derive(Clone, Copy)]
pub enum Act {
    /// Swap the engine by id (this also resets the tuning and the engine state).
    Engine(&'static str),
    /// Start the engine and let it idle for a moment, instantly.
    Warm,
    Time(f32),
    Pause(bool),
    /// Let go of the throttle.
    Release,
    /// Turn the crank by hand so the selected cylinder sits at this angle (pauses time).
    Scrub(f32),
    Cyl(usize),
    Panel(Panel, bool),
    Tab(Tab),
    Scale(PvScale),
    Labels(bool),
    /// Fit parts from a tuning spec, e.g. "header=long,muffler=straight".
    Tune(&'static str),
    StockTune,
    Fuel(&'static str),
    Unplug(usize),
    Clog(f32),
    O2(bool),
    Repair,
    AutoShift(bool),
}

#[derive(Clone, Copy)]
pub struct Goal {
    pub text: &'static str,
    pub check: fn(&Sim) -> bool,
}

pub struct Step {
    pub title: &'static str,
    pub body: &'static str,
    pub cam: Cam,
    pub acts: &'static [Act],
    pub goal: Option<Goal>,
}

pub struct Lesson {
    pub title: &'static str,
    pub blurb: &'static str,
    pub steps: &'static [Step],
}

const fn step(title: &'static str, body: &'static str, cam: Cam, acts: &'static [Act], goal: Option<Goal>) -> Step {
    Step { title, body, cam, acts, goal }
}

pub static LESSONS: &[Lesson] = &[
    Lesson {
        title: "Quick start: your first drive",
        blurb: "Start the engine, rev it, pick a gear and drive off.",
        steps: &[
            step(
                "Meet the powertrain",
                "On screen is a complete powertrain: the engine on the left, then the clutch, gearbox, driveshaft and differential, driving the rear wheels. All of it is simulated: pistons, valves, spark, airflow, heat and even the sound of the exhaust.\n\nDrag with the left mouse button to orbit, the right button to pan and scroll to zoom.",
                Cam::Anchor("Full powertrain"),
                &[Act::Engine("v8"), Act::Time(1.0), Act::Pause(false), Act::AutoShift(true), Act::Labels(true)],
                None,
            ),
            step(
                "Start the engine",
                "Hold E, or hold the START button at the bottom, to turn the starter motor. After about a second of cranking the ECU switches on fuel and spark, the engine catches and settles to an idle.",
                Cam::Anchor("Engine"),
                &[],
                Some(Goal { text: "Get the engine running", check: |s| s.tel.is_running && s.tel.rpm > 500.0 }),
            ),
            step(
                "Open the throttle",
                "Hold W (or drag the Throttle slider) to open the throttle plate. More air gets in, the ECU adds fuel to match, each cylinder makes a bigger bang and the revs climb. Watch the MANIFOLD pressure and the torque in the TELEMETRY panel as you do it.",
                Cam::Anchor("Intake"),
                &[],
                Some(Goal { text: "Rev it above 3000 rpm", check: |s| s.tel.rpm > 3000.0 }),
            ),
            step(
                "Pick a gear and go",
                "Press X (or click gear 1) to select first gear, then give it throttle. The clutch engages by itself, and auto-shift changes up near the redline. Z shifts down, N is neutral and R is reverse.",
                Cam::Anchor("Full powertrain"),
                &[],
                Some(Goal { text: "Reach 30 km/h", check: |s| s.tel.vehicle_speed_mps > 8.3 }),
            ),
            step(
                "Slow down again",
                "Let go of W and hold S or Space. With the throttle shut the engine itself slows the car, because the pistons are pulling against a closed throttle: engine braking. Hard braking turns the car's motion into heat in the discs, and they glow.",
                Cam::Anchor("Wheel & brake"),
                &[],
                Some(Goal { text: "Come to a stop", check: |s| s.tel.vehicle_speed_mps < 1.0 && s.tel.is_running }),
            ),
        ],
    },
    Lesson {
        title: "Inside a cylinder: the four-stroke cycle",
        blurb: "Follow one cylinder through intake, compression, power and exhaust.",
        steps: &[
            step(
                "One cylinder, four jobs",
                "A piston engine turns fuel into motion by repeating four strokes in every cylinder. One full cycle is 720 degrees of crank rotation, two turns of the crankshaft.\n\nWe have started a small four-cylinder engine and slowed time right down so you can follow cylinder 1. The CYCLE panel at the bottom left draws the same cycle as a graph.",
                Cam::Cylinder,
                &[Act::Engine("i4"), Act::Warm, Act::Time(0.05), Act::Pause(false), Act::Cyl(0), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Cycle), Act::Labels(true)],
                None,
            ),
            step(
                "1. Intake",
                "The piston is moving down and the blue intake valve is open. The falling piston lowers the pressure above it, so the air outside is pushed in through the throttle and past the valve (with fuel from the port injector). In the graph the cursor is in the INTAKE band and the blue valve lane is lit.\n\nPress Play to watch it move.",
                Cam::Cylinder,
                &[Act::Scrub(60.0)],
                None,
            ),
            step(
                "2. Compression",
                "Both valves are shut and the piston rises, squeezing the mixture to about a tenth of its volume. Squeezing heats it and the pressure climbs steeply: that is the yellow line rising in the graph. Just before the top the spark plug fires.",
                Cam::Cylinder,
                &[Act::Scrub(300.0)],
                None,
            ),
            step(
                "3. Power",
                "The flame spreads out from the plug and the pressure peaks a few degrees after the top. The hot gas shoves the piston down: this is the only stroke that makes power. Notice the glow in the cylinder and the red peak on the graph.",
                Cam::Cylinder,
                &[Act::Scrub(385.0)],
                None,
            ),
            step(
                "4. Exhaust",
                "The exhaust valve opens, the pressure blows the burnt gas out, and the rising piston pushes out the rest. Right at the top the intake valve starts to open while the exhaust valve is still closing: that overlap helps draw in the next charge.",
                Cam::Cylinder,
                &[Act::Scrub(600.0)],
                None,
            ),
            step(
                "Turn the crank yourself",
                "Click and drag across the timeline in the CYCLE panel to turn the crank by hand. Watch the piston, the valves and the spark follow in 3D, and how the pressure depends on the angle. Click the Play button here to let time run again.",
                Cam::Cylinder,
                &[],
                Some(Goal { text: "Drag the timeline", check: |s| s.scrub }),
            ),
            step(
                "All four cylinders",
                "One cylinder only makes power one stroke in four. The others are at different points of their cycles, so there is always one working. The numbered badges over the cylinders and the strip in the panel show every cylinder's stroke at once: red is a power stroke.",
                Cam::Anchor("Engine"),
                &[Act::Pause(false), Act::Time(0.1)],
                None,
            ),
        ],
    },
    Lesson {
        title: "Valve timing and overlap",
        blurb: "Why valves stay open longer than a stroke, and what the cam does to an engine's character.",
        steps: &[
            step(
                "Valves do not follow the strokes",
                "Look at the two valve lanes under the timeline: blue for the intake valve and orange for the exhaust. Each one is open for far more than 180 degrees. They open early and close late because moving gas has inertia and keeps flowing. The numbers under the graph (IVO, IVC, EVO, EVC) are the valve events engineers quote.",
                Cam::Cylinder,
                &[Act::Engine("v8"), Act::Warm, Act::Time(0.05), Act::Pause(false), Act::Cyl(0), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Cycle), Act::Labels(true)],
                None,
            ),
            step(
                "Overlap at the top",
                "Around top dead centre at the end of exhaust both valves are open at once: the green band on the graph. The leaving exhaust has momentum and pulls fresh air in behind it. At low revs though some exhaust flows backwards into the intake, which is why engines with big cams idle roughly.",
                Cam::Cylinder,
                &[Act::Scrub(716.0)],
                None,
            ),
            step(
                "A sportier cam: V12",
                "The V12 has a longer cam with more overlap, tuned for a high rpm range. Compare the shaded overlap band and the IVO / EVC figures with the V8's.",
                Cam::Cylinder,
                &[Act::Engine("v12"), Act::Warm, Act::Time(0.05), Act::Pause(false), Act::Cyl(0), Act::Tab(Tab::Cycle), Act::Scrub(716.0)],
                None,
            ),
            step(
                "No overlap: a diesel",
                "A diesel inline-six has almost no overlap. It has no throttle and wants all the air it can get at low revs, so there is nothing to gain from a long overlap, and the injection event replaces the spark on the graph.",
                Cam::Cylinder,
                &[Act::Engine("dieseli6"), Act::Warm, Act::Time(0.05), Act::Pause(false), Act::Cyl(0), Act::Tab(Tab::Cycle), Act::Scrub(716.0)],
                None,
            ),
        ],
    },
    Lesson {
        title: "Air, vacuum and the throttle",
        blurb: "Why a throttled engine wastes energy, why diesels do not, and what boost does.",
        steps: &[
            step(
                "Why a petrol engine has a throttle",
                "A petrol engine needs air and fuel in a fixed ratio, about 14.7 to 1, so the way to make less power is to let in less air. The throttle plate restricts the airflow, and at idle it is almost shut. Look at the MANIFOLD pressure in TELEMETRY: it is far below the 101 kPa of the atmosphere. That is the vacuum you hear hissing past the plate.",
                Cam::Anchor("Intake"),
                &[Act::Engine("v8"), Act::Warm, Act::Time(1.0), Act::Pause(false), Act::Release, Act::Labels(true)],
                None,
            ),
            step(
                "Vacuum costs power",
                "Open the P-V tab in the ANALYSIS panel and pick the 'pumping' scale. The small red loop is energy wasted pulling air past the closed throttle and pushing the exhaust out again. It is biggest at light throttle, and it is a large part of why part-throttle driving is inefficient.",
                Cam::Cylinder,
                &[Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Pv), Act::Scale(PvScale::Pumping), Act::Cyl(0)],
                None,
            ),
            step(
                "Open it up",
                "Hold W with the car in neutral. As the plate opens, the manifold pressure rises towards atmospheric and the pumping loop shrinks. The cylinders fill with more air and every firing is stronger.",
                Cam::Anchor("Intake"),
                &[],
                Some(Goal { text: "Open the throttle until the manifold passes 85 kPa", check: |s| s.tel.manifold_pressure_pa > 85_000.0 }),
            ),
            step(
                "Diesels have no throttle",
                "A diesel always takes in a full charge of air and sets its power by how much fuel it injects. So it has almost no pumping loss, which is one big reason diesels are efficient at part load. Compare the pumping loop and the manifold pressure at idle with the V8's.",
                Cam::Anchor("Intake"),
                &[Act::Engine("dieseli6"), Act::Warm, Act::Release, Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Pv), Act::Scale(PvScale::Pumping)],
                None,
            ),
            step(
                "Boost reverses it",
                "A turbocharger pushes the intake air above atmospheric pressure. The manifold pressure goes up past 101 kPa, and in the pumping view the loop turns green: the incoming air is now pushing the piston down instead of the piston pulling the air in. Floor it in neutral and watch.",
                Cam::Anchor("Turbocharger"),
                &[Act::Engine("i4_16"), Act::Tune("turbos=1,boost=0.8"), Act::Warm, Act::Release, Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Pv), Act::Scale(PvScale::Pumping)],
                Some(Goal { text: "Make some boost: hold W", check: |s| s.tel.boost_pressure_pa > 30_000.0 }),
            ),
        ],
    },
    Lesson {
        title: "Firing order and the sound of an engine",
        blurb: "Why a four-cylinder buzzes, a V8 burbles and a V12 sings.",
        steps: &[
            step(
                "Evenly spaced pulses",
                "Four cylinders firing in turn give one power stroke every 180 degrees. The strip in the CYCLE panel shows the sequence in firing order: each cylinder lights red as it fires. The firing rate is rpm / 120 x cylinders, so at 3000 rpm this engine fires 100 times a second: a 100 Hz note, the fundamental of what you hear.",
                Cam::Anchor("Engine"),
                &[Act::Engine("i4"), Act::Warm, Act::Time(0.2), Act::Pause(false), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Cycle), Act::Labels(false)],
                None,
            ),
            step(
                "A V8 on purpose uneven",
                "A V8 fires every 90 degrees, so overall its pulses are evenly spaced. But the cylinders of each bank do not fire evenly, so each exhaust pipe hears an uneven rhythm. That alternating pattern is the burble of an American V8. Rev it and listen.",
                Cam::Anchor("Engine"),
                &[Act::Engine("v8"), Act::Warm, Act::Time(1.0), Act::Pause(false)],
                Some(Goal { text: "Rev it above 4000 rpm and listen", check: |s| s.tel.rpm > 4000.0 }),
            ),
            step(
                "More cylinders, higher and smoother",
                "A V12 fires every 60 degrees. The power strokes overlap, so the torque is nearly constant, and the firing rate is half as high again as the V8's at the same revs, giving a higher, smoother note. Compare the engine in the strip: there is always more than one cylinder lit.",
                Cam::Anchor("Engine"),
                &[Act::Engine("v12"), Act::Warm, Act::Time(1.0), Act::Pause(false)],
                Some(Goal { text: "Rev it above 4000 rpm and listen", check: |s| s.tel.rpm > 4000.0 }),
            ),
            step(
                "The sound is simulated",
                "None of this is recorded. The exhaust gas flow through every pipe, bend, catalyst and muffler of this car is simulated, so changing the pipes changes the sound for physical reasons. The next lesson shows how.",
                Cam::Anchor("Exhaust"),
                &[],
                None,
            ),
        ],
    },
    Lesson {
        title: "Exhaust tuning and the sound",
        blurb: "Headers, mufflers, pipe metal and the dyno.",
        steps: &[
            step(
                "Pipes are instruments",
                "Every firing sends a pressure pulse down the exhaust. The pipes, catalyst and muffler reflect and shape these pulses, and what leaves the tailpipe is the sound. Open the TUNING panel (we have opened it for you) and look at the EXHAUST ACOUSTICS section: pipe length, resonance and gas temperatures are live.",
                Cam::Anchor("Exhaust"),
                &[Act::Engine("v8"), Act::Warm, Act::Time(1.0), Act::Pause(false), Act::StockTune, Act::Panel(Panel::Tuning, true), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Dyno), Act::Labels(true)],
                None,
            ),
            step(
                "Headers and scavenging",
                "We have fitted long-tube headers. Each cylinder now has its own long pipe before the pipes join. When a valve opens, its pulse runs down the pipe and returns as a suction wave. If it arrives while the exhaust valve is still open, it helps pull the burnt gas out: scavenging. Watch the DYNO chart: the power peak moves to where the pipe length is tuned.",
                Cam::Anchor("Engine"),
                &[Act::Tune("header=long")],
                None,
            ),
            step(
                "Cats and mufflers",
                "Now a straight-through muffler and no catalyst. The muffler's chambers and baffles are simulated with real geometry, so removing them changes how much each frequency is damped; the cat is a restriction as well. The result is louder and a little more powerful. Use the sliders in TUNING to explore.",
                Cam::Anchor("Exhaust"),
                &[Act::Tune("header=long,muffler=straight,cat=delete")],
                Some(Goal { text: "Change any part yourself in the TUNING panel", check: |s| s.tuning != Tuning::parse_spec("header=long,muffler=straight,cat=delete").unwrap_or_default() }),
            ),
            step(
                "Heat, metal and pitch",
                "Sound travels faster in hotter gas, so the pitch of every pipe resonance depends on the gas temperature. Titanium or thin pipes lose heat quickly, and heat wrap keeps it in, which raises the pitch and changes how the pipe walls ring. Try the Pipe metal and heat-wrap settings.",
                Cam::Anchor("Exhaust"),
                &[Act::Tune("header=long,pipemat=ti,wrap=1")],
                None,
            ),
            step(
                "Back to stock",
                "That is the loop: change a part, see it in 3D, see the dyno change and hear it. Presets at the top of the TUNING panel give you a quick set of bolt-ons to compare. We are putting the stock parts back now.",
                Cam::Anchor("Engine"),
                &[Act::StockTune],
                None,
            ),
        ],
    },
    Lesson {
        title: "Turbocharging",
        blurb: "Exhaust-driven boost, lag, intercoolers and knock.",
        steps: &[
            step(
                "More air, more power",
                "An engine's power depends on how much air it can burn. A turbocharger compresses the intake air above atmospheric pressure so more fits into each cylinder, and more fuel can be burnt with it. A small 1.6 litre engine can make the power of a much larger one.",
                Cam::Anchor("Turbocharger"),
                &[Act::Engine("i4_16"), Act::Tune("turbos=1,boost=0.8"), Act::Warm, Act::Time(1.0), Act::Pause(false), Act::Labels(true)],
                None,
            ),
            step(
                "Powered by waste",
                "Hot exhaust gas spins the turbine in the hot housing. A compressor wheel on the same shaft, in the cool housing, pumps air into the intake. The turbo takes no power from the crankshaft; it uses pressure and heat that would otherwise be thrown away. The blue glow shows the boost.",
                Cam::Anchor("Turbocharger"),
                &[],
                None,
            ),
            step(
                "Spool and lag",
                "Select first gear with X, then hold W flat out. Watch BOOST and spool in TELEMETRY: it takes a moment to build because the turbine needs exhaust flow to spin up. That delay is turbo lag. A smaller turbo spools faster but runs out of flow at high revs.",
                Cam::Anchor("Turbocharger"),
                &[],
                Some(Goal { text: "Reach 0.5 bar of boost", check: |s| s.tel.boost_pressure_pa > 50_000.0 }),
            ),
            step(
                "The intercooler",
                "Compressing air heats it, and hot air is less dense and more likely to knock. The intercooler is a radiator for the intake air. We have fitted a large front-mounted one: look at the charge temperature in TUNING. The panel shows the dyno change as well.",
                Cam::Anchor("Turbocharger"),
                &[Act::Tune("turbos=1,boost=0.8,ic=large"), Act::Panel(Panel::Tuning, true), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Dyno)],
                None,
            ),
            step(
                "Knock and octane",
                "More boost needs more octane. We have raised the boost: if the KNOCK warning appears, the fuel is detonating before the flame reaches it, which can melt pistons. Pull the boost back, or switch to a higher-octane fuel in the ENGINE SIM panel.",
                Cam::Anchor("Engine"),
                &[Act::Fuel("regular87"), Act::Tune("turbos=1,ic=large,boost=1.4"), Act::Panel(Panel::Engine, true)],
                Some(Goal { text: "Provoke knock (full throttle in gear), then fix it", check: |s| s.tel.is_knocking }),
            ),
        ],
    },
    Lesson {
        title: "Torque, power and gearing",
        blurb: "Read a dyno chart and see why a car needs gears.",
        steps: &[
            step(
                "Torque is force, power is rate",
                "Torque (orange) is the twisting force at the crankshaft. Power (teal) is torque times rpm, the rate at which the engine does work. The curves come from running the engine at full throttle at each speed. Peak power comes at a higher rpm than peak torque because rpm keeps rising after torque starts to fall.",
                Cam::Anchor("Engine"),
                &[Act::Engine("v8"), Act::Warm, Act::Time(1.0), Act::Pause(false), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Dyno)],
                None,
            ),
            step(
                "Why a car needs gears",
                "An engine only works well in a narrow rpm band, but the car has to go from standstill to well over 200 km/h. Low gears multiply the torque for acceleration but limit speed; high gears keep the revs down at cruise. The selected gear glows green inside the gearbox.",
                Cam::Anchor("Clutch & gearbox"),
                &[],
                Some(Goal { text: "Accelerate up to third gear", check: |s| s.gear_idx >= 4 && s.tel.vehicle_speed_mps > 5.0 }),
            ),
            step(
                "Shift at the right time",
                "Switch off auto-shift in the ENGINE SIM panel and change gear yourself. The best moment to shift is a little past the power peak, marked on the dyno chart. Then the next gear drops you back to near the peak torque instead of letting the revs fall into the weak part of the curve.",
                Cam::Anchor("Full powertrain"),
                &[Act::AutoShift(false)],
                None,
            ),
            step(
                "Compare engines by BMEP",
                "Open the POWER tab. BMEP is the average pressure on the pistons needed to make the engine's torque: a size-independent measure of how hard an engine works. Natural petrol engines sit around 9 to 14 bar and turbo engines go well above that.",
                Cam::Anchor("Engine"),
                &[Act::AutoShift(true), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Power)],
                None,
            ),
        ],
    },
    Lesson {
        title: "From crankshaft to wheels",
        blurb: "Flywheel, clutch, gearbox, driveshaft, differential, wheels and brakes.",
        steps: &[
            step(
                "Flywheel and clutch",
                "Only one stroke in four makes power, so the flywheel's inertia smooths the pulses. Behind it, the clutch disc is clamped between the flywheel and a pressure plate. When you shift, the disc slides away and the engine is freed from the gearbox for a moment.",
                Cam::Anchor("Clutch & gearbox"),
                &[Act::Engine("v8"), Act::Warm, Act::Time(1.0), Act::Pause(false), Act::Labels(true)],
                None,
            ),
            step(
                "The gearbox",
                "Pairs of gears of different sizes trade torque for speed. Pick a gear with X or Z and the matching pair lights up green; the others spin freely. Watch the input and output shafts turn at different speeds.",
                Cam::Anchor("Clutch & gearbox"),
                &[],
                Some(Goal { text: "Select a gear and drive", check: |s| s.gear_idx >= 2 && s.tel.vehicle_speed_mps > 2.0 }),
            ),
            step(
                "Driveshaft and differential",
                "The driveshaft carries the torque to the rear axle. The differential's small pinion drives a big ring gear (the final-drive ratio), multiplying the torque once more and turning it through 90 degrees to the axles.",
                Cam::Anchor("Driveshaft & diff"),
                &[],
                None,
            ),
            step(
                "Wheels and brakes",
                "The tyres turn the torque into forward force. Brake discs turn the car's motion into heat: all of the kinetic energy ends up in them. Brake hard from speed and watch the discs glow.",
                Cam::Anchor("Wheel & brake"),
                &[],
                Some(Goal { text: "Brake hard while moving", check: |s| s.tel.brake > 0.5 && s.tel.vehicle_speed_mps > 5.0 }),
            ),
        ],
    },
    Lesson {
        title: "When things go wrong",
        blurb: "Misfires, a clogged catalyst and a dead O2 sensor, and how the ECU notices.",
        steps: &[
            step(
                "A dead cylinder",
                "We have unplugged the ignition coil on cylinder 4. It cannot fire, so the cylinder just compresses and expands its charge: the CYCLE panel shows it as a MISFIRE and the engine runs rough. Open the FAULTS panel to see the faults you can inject yourself.",
                Cam::Cylinder,
                &[Act::Engine("v8"), Act::Warm, Act::Time(0.3), Act::Pause(false), Act::Cyl(3), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Cycle), Act::Panel(Panel::Faults, true), Act::Unplug(3)],
                Some(Goal { text: "Wait for the check-engine light", check: |s| !matches!(s.tel.cel_state, engine_core::CelState::Off) }),
            ),
            step(
                "How the ECU knows",
                "There is no sensor on each plug. The ECU watches the crankshaft speed instead: when a cylinder fails to fire, the crank slows down slightly at that point in the cycle, every cycle. Enough of them and the check-engine lamp lights.",
                Cam::Anchor("Engine"),
                &[],
                None,
            ),
            step(
                "Repair it",
                "Everything is plugged back in. The lamp stays on until you clear the codes in the FAULTS panel, and comes back if the fault is still there.",
                Cam::Anchor("Engine"),
                &[Act::Repair],
                None,
            ),
            step(
                "A clogged catalyst",
                "The catalyst is now 80% blocked. The exhaust cannot get out freely, so the pistons have to push against backpressure, the engine loses power and the exhaust gets hotter. Compare the dyno and the EGT readout in TELEMETRY.",
                Cam::Anchor("Exhaust"),
                &[Act::Clog(0.8), Act::Panel(Panel::Analysis, true), Act::Tab(Tab::Dyno)],
                None,
            ),
            step(
                "A dead oxygen sensor",
                "The O2 sensor tells the ECU if the mixture is rich or lean so it can trim the fuel. Without it the ECU is stuck on a fixed fuel map (open loop). Look at the Loop line in TELEMETRY. All faults are reset when you finish.",
                Cam::Anchor("Exhaust"),
                &[Act::Clog(0.0), Act::O2(false)],
                None,
            ),
        ],
    },
];

// ------------------------------------------------------------------------------------------ running

/// The lesson card sits at the top of the screen, so lesson views put their subject a little lower than centre
/// (a fraction of the view distance).
const LESSON_LIFT: f32 = 0.16;

/// Moves the camera for a step, or parks the move until the rig has been rebuilt.
pub fn apply_cam(cam: Cam, rig: &mut CamRig, anchors: &Anchors, geom: Option<&RigGeom>, sel_cyl: usize) -> bool {
    match cam {
        Cam::Anchor(name) => match anchors.0.iter().position(|a| a.name.trim_end_matches('s') == name.trim_end_matches('s')) {
            Some(i) => {
                rig.preset = i;
                let mut a = anchors.0[i].clone();
                a.target.y += a.dist * LESSON_LIFT;
                rig.goto(&a);
                true
            }
            None => true,
        },
        Cam::Cylinder => {
            let Some(g) = geom else { return false };
            let lay = &g.lay;
            let ci = sel_cyl.min(lay.n().saturating_sub(1));
            let cg = &lay.cyls[ci];
            // the front cylinder sits right behind the timing sprocket and belt: aim between it and its neighbour so they
            // fall to one side of the picture
            let target = bank_to_engine(cg.phi, Vec3::new(cg.x + if ci == 0 { 0.45 * lay.pitch } else { 0.0 }, lay.deck * 0.72, 0.0));
            rig.preset = usize::MAX;
            let (mut yaw, mut pitch, mut dist) = (-0.45f32, 0.2f32, (lay.bore * 6.6).clamp(0.55, 1.0));
            if let Ok(v) = std::env::var("CYL_CAM") {
                // development aid: CYL_CAM="yaw,pitch,dist"
                let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                if p.len() == 3 {
                    (yaw, pitch, dist) = (p[0], p[1], p[2]);
                }
            }
            rig.goto(&Anchor { name: "Cylinder", target: target + Vec3::Y * dist * LESSON_LIFT, dist, yaw, pitch });
            true
        }
    }
}

fn run_act(act: Act, ui: &mut UiState, sim: &mut Sim) {
    match act {
        Act::Engine(id) => {
            if let Some(i) = sim.engines.iter().position(|e| e.id == id) {
                sim.select_engine(i);
                sim.auto_start = false;
            }
        }
        Act::Warm => sim.pre_roll(1.2),
        Act::Time(t) => sim.time_scale = t,
        Act::Pause(p) => {
            sim.paused = p;
            if !p {
                sim.scrub = false;
            }
        }
        Act::Release => {
            sim.throttle_input = 0.0;
            sim.throttle_slider_active = false;
        }
        Act::Scrub(deg) => {
            let ci = ui.sel_cyl.min(sim.tables.len().saturating_sub(1));
            sim.scrub_to(ci, deg);
        }
        Act::Cyl(c) => ui.sel_cyl = c.min(sim.tables.len().saturating_sub(1)),
        Act::Panel(p, on) => ui.set(p, on),
        Act::Tab(t) => ui.tab = t,
        Act::Scale(s) => ui.pv_scale = s,
        Act::Labels(on) => ui.labels = on,
        Act::Tune(spec) => {
            if let Ok(t) = Tuning::parse_spec(spec) {
                sim.set_tuning(t, true);
            }
        }
        Act::StockTune => sim.set_tuning(Tuning::default(), true),
        Act::Fuel(id) => {
            if let Some(i) = sim.fuels.iter().position(|f| f.id == id) {
                sim.select_fuel(i);
            }
        }
        Act::Unplug(c) => {
            if let Some(ch) = sim.engine.cylinder_ignition_mut(c as u32 + 1) {
                ch.connector.connected = false;
            }
        }
        Act::Clog(v) => sim.engine.set_catalyst_clog(v as f64),
        Act::O2(on) => sim.engine.exhaust.o2.connected = on,
        Act::Repair => sim.reset_faults(),
        Act::AutoShift(on) => sim.auto_shift = on,
    }
}

/// Starts (or jumps to) a step: runs its set-up actions once and points the camera.
pub fn enter_step(ui: &mut UiState, sim: &mut Sim, lesson: usize, step: usize) {
    let l = &LESSONS[lesson.min(LESSONS.len() - 1)];
    let step = step.min(l.steps.len() - 1);
    ui.lesson = Some(Active { lesson, step });
    ui.set(Panel::Learn, true);
    let s = &l.steps[step];
    for a in s.acts {
        run_act(*a, ui, sim);
    }
    ui.pending_cam = Some(s.cam);
}

/// Leaving a lesson puts the faults and the time scale back.
pub fn leave(ui: &mut UiState, sim: &mut Sim) {
    if let Some(a) = ui.lesson {
        if LESSONS[a.lesson].title == "When things go wrong" {
            sim.reset_faults();
        }
    }
    ui.lesson = None;
    sim.time_scale = 1.0;
    sim.paused = false;
    sim.scrub = false;
}

// ------------------------------------------------------------------------------------------ window

pub fn learn_window(ctx: &egui::Context, ui_state: &mut UiState, sim: &mut Sim) {
    let mut open = ui_state.is_open(Panel::Learn);
    if !open {
        // closed from the toolbar or the window's X in the middle of a lesson: put time and faults back
        if ui_state.lesson.is_some() {
            leave(ui_state, sim);
        }
        return;
    }
    let mut start: Option<(usize, usize)> = None;
    let mut leave_now = false;
    let mut open_glossary = false;
    crate::hud::window("LEARN")
        .anchor(Align2::CENTER_TOP, [0.0, 52.0])
        .resizable(false)
        .collapsible(true)
        .default_width(440.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_max_width(440.0);
            match ui_state.lesson {
                None => {
                    ui.label(RichText::new("Pick a lesson").strong().size(15.0));
                    ui.colored_label(DIM, "Each one sets up the engine and camera for you and finishes with something to try.");
                    ui.add_space(4.0);
                    for (i, l) in LESSONS.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.add_sized([60.0, 34.0], egui::Button::new(RichText::new("Start").strong()).fill(Color32::from_rgb(30, 90, 80))).clicked() {
                                start = Some((i, 0));
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("{}. {}", i + 1, l.title)).strong());
                                ui.colored_label(DIM, format!("{}  ({} steps)", l.blurb, l.steps.len()));
                            });
                        });
                        ui.add_space(2.0);
                    }
                    ui.separator();
                    if ui.button("Glossary of terms").clicked() {
                        open_glossary = true;
                    }
                }
                Some(act) => {
                    let l = &LESSONS[act.lesson];
                    let s = &l.steps[act.step];
                    ui.horizontal(|ui| {
                        ui.colored_label(DIM, format!("Lesson {}: {}", act.lesson + 1, l.title));
                    });
                    ui.horizontal(|ui| {
                        for k in 0..l.steps.len() {
                            let (txt, col) = if k == act.step { ("O", ACCENT) } else if k < act.step { ("o", TEAL) } else { (".", DIM) };
                            ui.label(RichText::new(txt).color(col).strong());
                        }
                        ui.colored_label(DIM, format!("step {} of {}", act.step + 1, l.steps.len()));
                    });
                    ui.label(RichText::new(s.title).strong().size(17.0).color(ACCENT));
                    ui.add_space(2.0);
                    ui.label(RichText::new(s.body).size(13.5));
                    if let Some(g) = s.goal {
                        ui.add_space(5.0);
                        let done = (g.check)(sim);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if done { "[x]" } else { "[ ]" }).monospace().strong().color(if done { TEAL } else { ACCENT }));
                            ui.label(RichText::new(format!("Try it: {}", g.text)).color(if done { TEAL } else { Color32::WHITE }));
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(act.step > 0, egui::Button::new("< Back")).clicked() {
                            start = Some((act.lesson, act.step - 1));
                        }
                        let last = act.step + 1 >= l.steps.len();
                        let next = egui::Button::new(RichText::new(if last { "Finish" } else { "Next >" }).strong()).fill(Color32::from_rgb(30, 90, 80));
                        if ui.add_sized([80.0, 24.0], next).clicked() {
                            if last {
                                leave_now = true;
                            } else {
                                start = Some((act.lesson, act.step + 1));
                            }
                        }
                        ui.separator();
                        if ui.button(if sim.paused { "Play" } else { "Pause" }).clicked() {
                            sim.paused = !sim.paused;
                            if !sim.paused {
                                sim.scrub = false;
                            }
                        }
                        if ui.button("Restart step").clicked() {
                            start = Some((act.lesson, act.step));
                        }
                        if ui.button("All lessons").clicked() {
                            leave_now = true;
                        }
                    });
                }
            }
        });
    if !open {
        ui_state.set(Panel::Learn, false);
    }
    if leave_now {
        leave(ui_state, sim);
    }
    if let Some((l, s)) = start {
        enter_step(ui_state, sim, l, s);
    }
    if open_glossary {
        ui_state.set(Panel::Glossary, true);
    }
}

pub fn glossary_window(ctx: &egui::Context, ui_state: &mut UiState) {
    let mut open = ui_state.is_open(Panel::Glossary);
    if !open {
        return;
    }
    crate::hud::window("GLOSSARY")
        .default_pos(egui::Pos2::new(ctx.content_rect().center().x - 230.0, 90.0))
        .default_width(460.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(egui::TextEdit::singleline(&mut ui_state.glossary_filter).desired_width(200.0));
                if ui.button("clear").clicked() {
                    ui_state.glossary_filter.clear();
                }
            });
            let f = ui_state.glossary_filter.to_lowercase();
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (term, def) in crate::help::GLOSSARY {
                    if !f.is_empty() && !term.to_lowercase().contains(&f) && !def.to_lowercase().contains(&f) {
                        continue;
                    }
                    ui.label(RichText::new(*term).strong().color(ACCENT));
                    ui.label(*def);
                    ui.add_space(4.0);
                }
            });
        });
    ui_state.set(Panel::Glossary, open);
    let _ = Vec2::ZERO;
}
