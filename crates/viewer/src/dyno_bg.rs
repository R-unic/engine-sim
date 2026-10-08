//! Background dyno: whenever the engine, fuel or bolt-ons change, a worker thread re-runs a full
//! wide-open-throttle sweep on a fresh headless copy of the engine, so the analysis chart can show
//! the stock curve and the modified curve side by side without ever stalling the frame.

use crate::sim::Sim;
use bevy::prelude::*;
use engine_core::{dyno_curve, DynoPoint};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Shared {
    /// (key, curve) for the stock engine on the selected fuel.
    stock: Option<(u64, Vec<DynoPoint>)>,
    /// (tune serial, curve) for the engine with the current parts bolted on.
    tuned: Option<(u32, Vec<DynoPoint>)>,
}

#[derive(Resource, Default)]
pub struct DynoJobs {
    shared: Arc<Mutex<Shared>>,
    asked_serial: u32,
    asked_stock_key: u64,
    /// Seconds the parts have been unchanged (a short debounce while a slider is dragged).
    settle: f32,
    pub stock: Vec<DynoPoint>,
    pub tuned: Vec<DynoPoint>,
    pub busy: bool,
}

pub struct Peak {
    pub hp: f64,
    pub hp_rpm: f64,
    pub tq_nm: f64,
    pub tq_rpm: f64,
}

pub fn peak(c: &[DynoPoint]) -> Option<Peak> {
    let hp = c.iter().max_by(|a, b| a.hp.partial_cmp(&b.hp).unwrap())?;
    let tq = c.iter().max_by(|a, b| a.torque_nm.partial_cmp(&b.torque_nm).unwrap())?;
    Some(Peak { hp: hp.hp, hp_rpm: hp.rpm, tq_nm: tq.torque_nm, tq_rpm: tq.rpm })
}

const STEP_RPM: f64 = 250.0;

pub fn dyno_jobs_system(sim: Res<Sim>, mut jobs: ResMut<DynoJobs>, time: Res<Time<Real>>) {
    let stock_key = ((sim.engine_idx as u64) << 8) | sim.fuel_idx as u64;
    let want_stock = jobs.asked_stock_key != stock_key + 1;
    let want_tuned = jobs.asked_serial != sim.tune_serial;
    if want_stock || want_tuned {
        jobs.settle += time.delta_secs();
        // the very first request goes out immediately; later edits wait for the slider to settle
        if jobs.settle > 0.25 || jobs.asked_serial == 0 {
            jobs.settle = 0.0;
            let cfg = sim.cfg().clone();
            let fuel = sim.fuels[sim.fuel_idx].clone();
            if want_stock {
                jobs.asked_stock_key = stock_key + 1;
                let shared = jobs.shared.clone();
                let (cfg, fuel) = (cfg.clone(), fuel.clone());
                std::thread::spawn(move || {
                    let curve = dyno_curve(&cfg, &fuel, &engine_core::Tuning::default(), STEP_RPM);
                    if let Ok(mut s) = shared.lock() {
                        s.stock = Some((stock_key + 1, curve));
                    }
                });
            }
            if want_tuned {
                jobs.asked_serial = sim.tune_serial;
                let serial = sim.tune_serial;
                let tuning = sim.tuning.clone();
                let shared = jobs.shared.clone();
                std::thread::spawn(move || {
                    let curve = dyno_curve(&cfg, &fuel, &tuning, STEP_RPM);
                    if let Ok(mut s) = shared.lock() {
                        s.tuned = Some((serial, curve));
                    }
                });
            }
        }
    } else {
        jobs.settle = 0.0;
    }
    // collect finished sweeps (ignoring any that were overtaken by a newer request)
    let (stock, tuned) = match jobs.shared.try_lock() {
        Ok(mut s) => (s.stock.take(), s.tuned.take()),
        Err(_) => (None, None),
    };
    if let Some((k, c)) = stock {
        if k == jobs.asked_stock_key {
            jobs.stock = c;
        }
    }
    if let Some((k, c)) = tuned {
        if k == jobs.asked_serial {
            jobs.tuned = c;
        }
    }
    jobs.busy = want_stock || want_tuned || jobs.tuned.is_empty() || jobs.stock.is_empty();
}
