//! Plain-language explanations: tooltips for the controls and readouts, what each labelled part of the
//! rig is and does, and a small glossary. Everything here describes what the simulation actually
//! models; where the sim simplifies something the text says so.

/// Tooltip for a control or readout, by key. An unknown key gives an empty string.
pub fn tip(key: &str) -> &'static str {
    match key {
        // ---- selectors
        "engine" => "The engine to simulate. Bore, stroke, cylinder count, firing order, compression ratio, redline and how well it breathes all come from its data, and the 3D rig is rebuilt from them.",
        "gearbox" => "Gear ratios and the final drive. Low gears multiply torque for acceleration; high gears keep rpm down for cruising. The selected gear lights up green inside the gearbox.",
        "fuel" => "Octane (knock resistance) and energy content. Higher octane lets a boosted or high-compression engine run more spark advance without knock; E85 resists knock best but has less energy per litre, so the engine burns more of it.",
        "brakes" => "Maximum brake force. Stronger brakes stop the car faster and heat the discs more: watch them glow after a hard stop from speed.",
        "time" => "Simulation speed. At 0.05x you can follow one cylinder's four strokes by eye; the sound slows down with it.",
        "visual_cap" => "An engine at 6000 rpm turns 100 times a second, far too fast to see. The 3D parts are played back at this capped crank speed while the physics runs at full speed, so pistons, valves, spark and flame stay in correct phase. Set it to 'real' to see true speed (it will strobe).",
        "seethrough" => "Fades any part that blocks your view of what the camera is looking at.",
        "volume" => "Sound level. The sound is a fluid-dynamics simulation of the real pipes, not a recording.",
        "autoshift" => "Changes gear for you: up near redline, down when the revs drop low.",
        "labels" => "Name the parts in the 3D view. Hover a label for a description with live values, click it for a card, and double-click to zoom the camera to it.",
        "ui_scale" => "Size of all the panels and text.",

        // ---- drive
        "start" => "Hold to turn the starter motor. The engine fires once it has turned over for about a second.",
        "stop" => "Switches off ignition and fuel.",
        "throttle" => "How far the throttle plate is open: the amount of air the engine can breathe. Tick 'hold' to keep it where you set it.",
        "brake" => "Brake pedal.",

        // ---- telemetry
        "rpm" => "Crankshaft revolutions per minute. Each cylinder fires once every two revolutions, so the engine fires at rpm / 120 x cylinders times a second. The red zone starts just below the rev limiter.",
        "speed" => "Vehicle speed, tied to the wheel speed through the tyre radius.",
        "torque" => "Twisting force at the crankshaft after friction losses. It is what accelerates the car, and it peaks where the engine breathes best.",
        "power" => "Torque x rpm: the rate of doing work. 1 hp is about 746 W. Power keeps climbing after the torque peak because rpm rises faster than torque falls.",
        "manifold" => "Pressure in the intake manifold. Below atmospheric (about 101 kPa) the throttle is restricting the air and the pistons are pulling against a vacuum: a 'pumping loss'. Above it, a turbo is pushing air in.",
        "boost" => "Pressure the turbo adds above atmosphere. Spool is how fast the turbine is spinning compared with full speed: it takes time to build, which is turbo lag.",
        "afr" => "Air-fuel ratio by mass. Petrol burns cleanly at 14.7:1 ('stoichiometric'). Richer (a lower number) gives power and cooling at full throttle; leaner is efficient but runs hotter. The O2 sensor corrects it in closed loop ('trim').",
        "coolant" => "Engine coolant temperature. About 90 C is normal; above roughly 110 C the engine overheats.",
        "egt" => "Exhaust gas temperature at the head, and the catalyst temperature. The cat only works once it has lit off, at about 300 C.",
        "fuel_level" => "Fuel remaining, and the rate it is being burnt right now.",
        "loop" => "Closed loop: the ECU trims fuel from the O2 sensor to hold 14.7:1. Open loop: it follows a fixed fuel map (cold start, full throttle, or a failed sensor).",
        "evap" => "The evaporative emissions system stops fuel vapour escaping from the tank. The ECU tests it for leaks by pulling a small vacuum.",
        "clutch" => "How firmly the clutch is clamped. 100% locks engine and gearbox together; lower values mean it is slipping.",
        "bmep" => "Brake mean effective pressure: the average pressure on the pistons that would give the engine's torque. It measures how hard each litre of engine is working, whatever its size. Roughly 9 to 14 bar for a naturally aspirated petrol engine, 15 to 25 bar boosted.",
        "piston_speed" => "Mean piston speed = 2 x stroke x rpm. Metal fatigue limits it to about 25 m/s in any engine, which is why long-stroke engines have lower redlines.",
        "specific_power" => "Power per litre of displacement.",
        "efficiency" => "Brake thermal efficiency: the share of the fuel's chemical energy that comes out as shaft power. Real petrol engines peak around 30-35% (diesel ~40%). The rest leaves as heat in the exhaust and coolant, and as friction.",

        // ---- tuning
        "t_filter" => "Air filter and intake inlet. A restrictive airbox costs a little power at high rpm; a cone or open velocity stack flows more (and sounds louder) but may draw warmer engine-bay air.",
        "t_runners" => "Intake runner length tunes the intake pulses like an organ pipe. Long runners boost low-rpm torque, short runners move the peak to high rpm, and individual throttle bodies give the sharpest response.",
        "t_header" => "The exhaust manifold. A cast log is cheap but restrictive; tubular headers give each cylinder its own pipe so blowdown pulses scavenge the next cylinder. Long-tube headers work best but tune to a narrower rpm range.",
        "t_cat" => "The catalytic converter cleans the exhaust but restricts it. A high-flow cat restricts less; deleting it is loudest and most powerful, and also illegal for road use.",
        "t_muffler" => "The sound-quieting can. The sound solver simulates its real chambers and baffles, so the stock can is quiet, a sport can a little louder, a straight-through can loud, and a delete is open pipe.",
        "t_pipe_dia" => "Diameter of every exhaust pipe. Wider lowers backpressure but also slows the gas, which can cost low-rpm torque, and it changes the sound.",
        "t_pipe_metal" => "Pipe material changes how fast the pipe heats and cools, which sets the gas temperature and so the pitch (sound travels faster in hotter gas), and how the pipe walls ring.",
        "t_header_metal" => "Header material, same effects as the pipe metal but at the hottest part of the system.",
        "t_wall" => "Wall thickness: a thick wall holds more heat and rings less; a thin wall cools fast and rings more ('tinny').",
        "t_primary_len" => "Length of each header primary pipe. It sets the engine speed at which the exhaust pulses help scavenge (see the 'primaries tune to' readout).",
        "t_primary_dia" => "Primary pipe diameter. Smaller keeps gas speed up at low rpm, larger flows more at high rpm.",
        "t_mid" => "Length of the mid-pipe between the cat and muffler. It sets the exhaust resonances that cause cruising drone.",
        "t_tail" => "Tailpipe length after the muffler. Longer pipes resonate at lower frequencies.",
        "t_wrap" => "Insulating wrap keeps the exhaust gas hotter: hotter gas is faster (higher pitch), keeps the scavenging pulses stronger and damps shell ringing, but it can overheat the pipe.",
        "t_crossover" => "A pipe joining the two banks of a V engine. An H-pipe balances the pressure between banks; an X-pipe merges and splits the flow, smoothing and raising the exhaust note.",
        "t_turbos" => "Number of turbochargers. More small turbos spool quickly; one large one flows more at the top end.",
        "t_turbo_size" => "Turbo size: a small turbo spools fast (quick) but chokes at high rpm; a large one is laggy but flows far more.",
        "t_boost" => "The wastegate setting: the most boost the turbo is allowed to make. More boost means more power and more risk of knock and heat.",
        "t_intercooler" => "A radiator for the compressed intake air. Cooler air is denser (more power) and less prone to knock.",
        "t_presets" => "Ready-made bolt-on sets. Try one, then watch the DYNO chart, the sound and the 3D rig change.",
        "t_quality" => "How finely the exhaust sound solver divides the pipes (ultra 20 mm cells ... eco 70 mm). Finer cells keep more of the top end and use more CPU; Auto times the engine on your computer when it is chosen, picks the finest tier with room to spare, and then stays on it (a change of tier is a change in the sound) unless the computer falls behind.",

        // ---- analysis
        "pv" => "Pressure against cylinder volume for one full cycle. The area enclosed by the loop is the work the gas did on the piston. A big upper loop (green) is good; the small lower loop is the pumping loss of pulling air through the throttle and pushing exhaust out.",
        "dyno" => "Torque (orange) and power (teal) against rpm at full throttle. The faint lines are the stock engine, the bright ones are what you have now. The circle marks peak power. The sweep runs in the background after each change.",
        "timeline" => "One full cycle of one cylinder is 720 degrees of crank rotation (two turns). Drag across this chart to turn the crank by hand and pause time.",
        "firing" => "Every cylinder in firing order, coloured by what it is doing right now. Click one to follow it.",
        "energy" => "Where the fuel's energy is going right now. Shaft power and friction are measured from the sim; the heat share is whatever is left, mostly leaving by the exhaust and the coolant.",

        _ => "",
    }
}

/// What a labelled part of the rig is and does.
#[derive(Clone, Copy)]
pub struct PartInfo {
    pub name: &'static str,
    pub role: &'static str,
    pub detail: &'static str,
}

pub fn part(key: &str) -> Option<PartInfo> {
    let (name, role, detail) = match key {
        "crank" => (
            "Crankshaft",
            "Turns the pistons' up-and-down push into rotation.",
            "Each connecting rod pushes on an offset crankpin (a 'throw'), so a straight shove becomes a twist. The angles between the throws set the firing order and the character of the sound: a cross-plane V8 has four throws 90 degrees apart. The crank turns at engine speed; the camshafts turn at half of it.",
        ),
        "damper" => (
            "Crank pulley and damper",
            "Drives the accessory belt and absorbs crankshaft vibration.",
            "Every power stroke twists the crank slightly, like plucking a spring. A rubber-isolated ring on the pulley soaks up that vibration; without it the crank could resonate and break.",
        ),
        "piston" => (
            "Piston",
            "The plunger the burning gas pushes down.",
            "Rings seal it to the cylinder wall. It stops dead at the top and bottom of every stroke and is fastest mid-stroke. Its average speed is limited by material strength to about 25 m/s, which is why long-stroke engines have lower redlines.",
        ),
        "conrod" => (
            "Connecting rod",
            "Links the piston to the crankpin.",
            "It swings side to side as the crank turns, and it is stretched and squeezed every cycle. At high rpm the piston's own inertia loads the rod more than the combustion pressure does.",
        ),
        "cylinder" => (
            "Cylinder and combustion chamber",
            "Where the air and fuel burn.",
            "The space between the head and the piston crown. The compression ratio is (swept volume + chamber volume) / chamber volume: the mixture is squeezed to roughly one-tenth of its volume before it burns. A higher ratio is more efficient but more prone to knock.",
        ),
        "intake_valve" => (
            "Intake valve",
            "Opens to let air (and fuel) into the cylinder.",
            "Springs hold it shut and the camshaft pushes it open. It opens slightly before the piston reaches the top and closes after the bottom, so it is open for more than the 180 degrees of the intake stroke: the incoming air has momentum and keeps flowing even as the piston turns round.",
        ),
        "exhaust_valve" => (
            "Exhaust valve",
            "Opens to let the burnt gas out.",
            "It opens before the piston reaches the bottom of the power stroke so the pressurised gas can blow down into the exhaust; that sudden pulse is the pop you hear at the tailpipe. It runs far hotter than the intake valve, which is why it glows orange here.",
        ),
        "camshaft" => (
            "Camshaft",
            "Opens each valve at the right moment.",
            "Egg-shaped lobes push the valves open as the shaft turns. It runs at half crank speed because each valve opens once every two revolutions. The lobe shape sets how long and how far the valve opens ('cam timing'): a longer cam with more overlap makes more top-end power but a lumpier idle.",
        ),
        "plug" => (
            "Spark plug",
            "Starts the burn with an electric arc.",
            "The ECU fires it some degrees before top dead centre (the ignition advance) so the pressure peak lands just after the piston has turned round, when it can push hardest. Too early and it fights the rising piston and can knock; too late and the energy is wasted as heat.",
        ),
        "injector" => (
            "Fuel injector",
            "Sprays fuel straight into the compressed air.",
            "A diesel has no spark. Squeezing air to about 17 to 20 times its volume heats it past the fuel's ignition temperature, so fuel sprayed in near top dead centre lights itself. Load is set by how much fuel is injected rather than by a throttle.",
        ),
        "coil" => (
            "Ignition coil",
            "Steps 12 V up to tens of thousands of volts for the spark.",
            "There is one on top of every plug ('coil-on-plug'). The ECU switches current through its primary winding, and when it cuts off the collapsing magnetic field induces the high voltage.",
        ),
        "oil_pan" => (
            "Oil pan",
            "Holds the engine oil.",
            "The oil pump draws from here and pushes oil through the filter to every bearing and the cylinder walls. Oil also carries heat away from the pistons.",
        ),
        "flywheel" => (
            "Flywheel",
            "A heavy disc that smooths out the power pulses.",
            "Only one stroke in four makes power, so the crank would speed up and slow down all the time. The flywheel's inertia evens that out. Its outer ring gear is what the starter motor engages.",
        ),
        "clutch" => (
            "Clutch",
            "Couples and uncouples the engine from the gearbox.",
            "A friction disc is clamped between the flywheel and a pressure plate. Slipping it lets the car pull away from rest while the engine keeps turning, and it is opened during shifts so the gears can change without torque on them.",
        ),
        "starter" => (
            "Starter motor",
            "Spins the engine up to starting speed.",
            "Its pinion engages the flywheel's ring gear. The engine has to be turned over for a moment before fuel and spark begin; here the ECU waits about a second.",
        ),
        "gearbox" => (
            "Gearbox",
            "Trades torque for speed.",
            "Each gear pair multiplies engine torque while dividing speed: low gears for strong acceleration, high gears to keep revs down at cruise. An engine only works well in a narrow rpm band, and the gearbox is how the car stays inside it. The selected gear glows green.",
        ),
        "driveshaft" => (
            "Driveshaft",
            "Carries torque from the gearbox to the rear axle.",
            "It spins at the gearbox output speed, which is the wheel speed times the final-drive ratio. Universal joints at each end let it flex as the axle moves.",
        ),
        "diff" => (
            "Differential",
            "The final gear reduction, and it lets the wheels turn at different speeds.",
            "A small pinion drives a large ring gear (the final-drive ratio), multiplying torque once more and turning the power through 90 degrees to the axle. On a corner the outer wheel has further to go, so the differential lets the two wheels turn at different speeds.",
        ),
        "wheel" => (
            "Wheel and tyre",
            "Where it all ends: the tyre's grip on the road.",
            "Wheel speed and vehicle speed are tied together through the tyre radius (v = angular speed x radius); that is where the speedometer reading comes from.",
        ),
        "brake" => (
            "Brake disc and caliper",
            "Turn the car's motion into heat.",
            "The caliper squeezes pads against the spinning disc. All of the car's kinetic energy ends up as heat in the disc; after a hard stop from speed it glows.",
        ),
        "throttle" => (
            "Throttle body",
            "Controls how much air the engine can breathe.",
            "The plate opens as you press the pedal. Nearly closed at idle, it makes a vacuum in the manifold, and the pistons have to pull against it: a 'pumping loss', and the reason part-throttle driving is inefficient. Diesels have no throttle at all.",
        ),
        "plenum" => (
            "Intake plenum and runners",
            "An air reservoir, and the pipes that feed each cylinder.",
            "The plenum evens out the supply; the runners' length tunes the intake pulses like an organ pipe, so long runners favour low-rpm torque and short runners favour high-rpm power.",
        ),
        "filter" => (
            "Air filter",
            "Cleans the air before it enters the engine.",
            "A restrictive stock airbox costs a little power at high rpm. A cold-air cone or open velocity stack flows more and sounds louder, but can draw in warmer air from the engine bay.",
        ),
        "fuel_rail" => (
            "Fuel rail and injectors",
            "Delivers fuel to each cylinder's intake port.",
            "Each injector sprays a metered pulse onto the back of its intake valve. The ECU sets the pulse length to hold the target air-fuel ratio (14.7:1 for petrol at cruise, richer at full throttle).",
        ),
        "header" => (
            "Exhaust manifold or header",
            "Collects the exhaust from the cylinders.",
            "Each exhaust valve opening sends a pressure pulse down its pipe. With well-chosen pipe lengths the pulse reflects back as a suction wave just as the next valve opens, helping to pull burnt gas out: 'scavenging'. A cast log manifold is cheap but restrictive.",
        ),
        "turbo" => (
            "Turbocharger",
            "Uses exhaust energy to force more air into the engine.",
            "Exhaust gas spins a turbine; a compressor on the same shaft pumps the intake air to above atmospheric pressure ('boost'). More air lets the engine burn more fuel for more power from a smaller engine. 'Lag' is the time the turbine takes to spool up, and the wastegate lets exhaust bypass the turbine to cap the boost.",
        ),
        "intercooler" => (
            "Intercooler",
            "Cools the compressed air.",
            "Compressing air heats it, which makes it less dense and raises the risk of knock. The intercooler, a radiator for the intake air, takes some of that heat out again.",
        ),
        "o2" => (
            "Oxygen sensor",
            "Measures whether the exhaust is rich or lean.",
            "It compares the oxygen in the exhaust with the outside air. The ECU uses it to trim the fuel so the mixture stays near 14.7:1 ('closed loop'), which is the window the catalyst needs. Unplug it in the FAULTS panel and the ECU falls back to a fixed fuel map and sets a check-engine code.",
        ),
        "cat" => (
            "Catalytic converter",
            "Cleans up harmful exhaust gases.",
            "A honeycomb coated with precious metals turns carbon monoxide, unburnt fuel and nitrogen oxides into carbon dioxide, water and nitrogen, but only above its light-off temperature (about 300 C) and only when the mixture is close to 14.7:1. It also restricts the flow, which is why tuners fit high-flow cats.",
        ),
        "muffler" => (
            "Muffler",
            "Quiets the exhaust.",
            "Chambers, baffles and perforated tubes reflect sound waves back on themselves so they cancel, and absorb some of the rest. The sound solver simulates the real geometry. Straight-through cans are louder but restrict the flow less.",
        ),
        "tailpipe" => (
            "Tailpipe",
            "Where the sound leaves the car.",
            "The pipe's length sets its resonances (the quarter-wave drone), and the open end radiates sound like a loudspeaker. Burbles and flames from unburnt fuel igniting in the hot pipe appear here.",
        ),
        "radiator" => (
            "Radiator and fan",
            "Dumps the engine's heat into the air.",
            "Hot coolant flows through thin finned tubes while air passes through the core. Roughly a third of the fuel's energy leaves as coolant heat. The electric fan pulls air through when the car is slow.",
        ),
        "alternator" => (
            "Alternator",
            "Charges the battery and runs the electrics.",
            "Driven by the belt. Its pulley is smaller than the crank's, so it spins faster than the engine.",
        ),
        "ac" => (
            "A/C compressor",
            "Pumps refrigerant for the air conditioning.",
            "Driven by the belt, it loads the engine only when the A/C is on. It is here as part of the accessory drive.",
        ),
        "waterpump" => (
            "Water pump",
            "Circulates coolant between the engine and radiator.",
            "Driven by the belt, so coolant flow rises with engine speed.",
        ),
        "belt" => (
            "Serpentine belt",
            "One ribbed belt drives all the accessories.",
            "It winds around the crank pulley, the alternator, the water pump and the A/C compressor, kept taut by a spring-loaded tensioner. The white marks show it moving.",
        ),
        _ => return None,
    };
    Some(PartInfo { name, role, detail })
}

/// Short definitions for the glossary.
pub const GLOSSARY: &[(&str, &str)] = &[
    ("TDC / BDC", "Top and bottom dead centre: the highest and lowest points of the piston's travel. The piston stops for an instant at both."),
    ("Stroke", "One trip of the piston from TDC to BDC or back, 180 degrees of crank rotation. A four-stroke cycle is intake, compression, power, exhaust: 720 degrees, two crank turns."),
    ("Bore and stroke", "The cylinder's diameter and how far the piston travels. Displacement = pi/4 x bore^2 x stroke x number of cylinders."),
    ("Compression ratio", "Cylinder volume at BDC divided by the volume at TDC. Higher is more efficient, but the hotter, denser mixture is more likely to knock."),
    ("Valve overlap", "The few degrees around TDC at the end of exhaust where the exhaust and intake valves are both open. The leaving exhaust helps draw in the new charge, at the cost of a rough idle."),
    ("Ignition advance", "How many degrees before TDC the spark fires. The flame takes time to spread, so the spark is early enough for the pressure peak to land just after TDC."),
    ("Knock", "The unburnt end of the charge igniting by itself before the flame reaches it: a shock wave that can destroy pistons. Higher octane fuel, cooler intake air and less boost resist it."),
    ("Stoichiometric (14.7:1)", "The air-fuel ratio at which petrol and air burn completely with nothing left over. The catalytic converter works best there."),
    ("Volumetric efficiency", "How much air the cylinder actually takes in compared with its full volume. Intake and exhaust tuning, valve timing and rpm all change it; above 100% needs boost or ram effect."),
    ("Manifold vacuum", "A throttled engine pulls air through a restriction, so the intake manifold sits below atmospheric pressure. The pistons waste work pulling against it: the pumping loss."),
    ("Pumping loss", "Energy spent drawing air in and pushing exhaust out. It is the small loop in the P-V diagram, biggest at light throttle."),
    ("Torque and power", "Torque is the twisting force on the crank; power is torque x rpm. A car's acceleration follows torque at the wheels, so gearing lets a small high-revving engine match a large low-revving one."),
    ("BMEP", "Brake mean effective pressure: the average cylinder pressure that would produce the engine's torque, torque x 4 pi / displacement. A size-independent measure of how hard an engine works."),
    ("Mean piston speed", "2 x stroke x rpm. Limited by material strength to roughly 25 m/s."),
    ("Redline and rev limiter", "The highest safe rpm, and the ECU cutting fuel or spark to stop the engine going past it."),
    ("Boost", "Intake pressure above atmospheric, made by a turbocharger or supercharger. 1 bar of boost roughly doubles the air in the cylinder."),
    ("Turbo lag", "The delay before a turbo makes boost, because the turbine needs exhaust flow to spin up."),
    ("Wastegate", "A valve that lets exhaust bypass the turbine so the boost does not rise past its setting."),
    ("Scavenging", "Using the pressure waves in the exhaust to help pull burnt gas out of the cylinder and fresh charge in."),
    ("Backpressure", "Resistance in the exhaust that the piston has to push against. Catalysts and mufflers add it; wide pipes reduce it."),
    ("Firing order", "The sequence the cylinders fire in, chosen to balance the engine and spread the power pulses evenly. The gaps between firings, and which pipes they share, shape the sound."),
    ("ECU", "The engine control unit: the computer that sets fuel, spark timing and idle, monitors the sensors and lights the check-engine lamp."),
    ("Closed loop", "The ECU adjusting fuel using the oxygen sensor's feedback. 'Open loop' means it is following a fixed table."),
    ("Catalyst light-off", "The catalytic converter only cleans the exhaust above about 300 C, so a cold start is the dirtiest part of a trip."),
    ("EGT", "Exhaust gas temperature. Rich mixtures and retarded spark raise it; it can melt pistons and turbines if it gets too high."),
    ("IMEP", "Indicated mean effective pressure: the net work per cycle from the P-V diagram divided by the swept volume. Brake figures are lower because of friction."),
    ("Thermal efficiency", "Shaft work divided by the energy in the fuel. Roughly a third of the fuel's energy becomes work in a good petrol engine; the rest is heat."),
];
