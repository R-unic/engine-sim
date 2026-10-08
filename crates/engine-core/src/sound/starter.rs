//! The starter motor, its pinion and the ring gear: what a key turn sounds like before the engine lights.
//!
//! A starter is not a tone. What is heard is
//!  * the *ring gear*: every tooth that passes the pinion is a small impact that rings the starter nose and
//!    the bell housing, so the sound is a train of clicks at (teeth x crank speed) with a little tooth-to-tooth
//!    scatter, which the ear takes as a harsh, buzzy grind rather than a pitch;
//!  * the *motor*: commutator and brush noise, a band of hiss that rises with the armature speed and is
//!    pulsed at the segment rate, plus a weak, wandering whine;
//!  * the *solenoid and pinion*: a clack when the pinion is thrown into the ring gear (with a short grind as
//!    the teeth find each other) and a clunk when it is let go, after which the armature spins down freely.
//!
//! Its pitch is the crank's instantaneous speed, so it surges and sags with every compression stroke.

use super::mechanical::{Noise, Res};

/// Teeth on the ring gear: bigger engines have bigger flywheels.
fn ring_teeth(displacement_l: f32) -> f32 {
    (108.0 + 9.0 * displacement_l).clamp(110.0, 168.0)
}

/// Armature turns per crank turn (ring teeth over pinion teeth).
fn gear_ratio(teeth: f32) -> f32 {
    teeth / 9.5
}

/// What the starter needs to know each sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct StarterCtx {
    /// The pinion is thrown in and the motor driven.
    pub on: bool,
    /// Instantaneous crank speed (including the sag and surge of the compression strokes), rpm.
    pub rpm: f32,
    /// 0..1: how hard the motor is working against a compression stroke right now.
    pub strain: f32,
}

#[derive(Clone, Debug)]
pub struct Starter {
    fs: f32,
    rng: Noise,
    teeth: f32,
    ratio: f32,
    /// Solenoid and pinion engagement, 0..1.
    env: f32,
    was_on: bool,
    /// Armature speed, turns per second.
    motor_rps: f32,
    tooth_phase: f32,
    comm_phase: f32,
    whine_phase: f32,
    mesh: [Res; 3],
    clack: [Res; 3],
    grind: f32,
    brush_lo: f32,
    brush_hi: f32,
    air_lp: f32,
    tone_lp: f32,
    out_lp: f32,
    wander: f32,
}

impl Starter {
    pub fn new(fs: f32, displacement_l: f32) -> Starter {
        let teeth = ring_teeth(displacement_l);
        Starter {
            fs,
            rng: Noise::new(0x7A3F_19C5),
            teeth,
            ratio: gear_ratio(teeth),
            env: 0.0,
            was_on: false,
            motor_rps: 0.0,
            tooth_phase: 0.0,
            comm_phase: 0.0,
            whine_phase: 0.0,
            // the starter nose, the bell housing and the block's skirt ring when a tooth lands
            mesh: [Res::new(820.0, 0.055, fs), Res::new(1650.0, 0.045, fs), Res::new(3100.0, 0.07, fs)],
            clack: [Res::new(260.0, 0.12, fs), Res::new(1300.0, 0.03, fs), Res::new(2700.0, 0.04, fs)],
            grind: 0.0,
            brush_lo: 0.0,
            brush_hi: 0.0,
            air_lp: 0.0,
            tone_lp: 0.0,
            out_lp: 0.0,
            wander: 0.0,
        }
    }

    /// Armature speed, turns per second (analysis).
    pub fn motor_rps(&self) -> f32 {
        self.motor_rps
    }

    /// One sample, in pascals at the listener.
    pub fn step(&mut self, c: &StarterCtx) -> f32 {
        let dt = 1.0 / self.fs;
        let tau = std::f32::consts::TAU;
        let n = self.rng.next();
        let n2 = self.rng.next();

        // ---- solenoid: pulls in fast, drops out slower
        let k_env = 1.0 - (-dt / if c.on { 0.006 } else { 0.05 }).exp();
        self.env += ((c.on as u8 as f32) - self.env) * k_env;
        let mut kick = 0.0;
        if c.on && !self.was_on {
            kick = 1.0;
            self.grind = 1.0;
        } else if !c.on && self.was_on {
            kick = -0.45;
        }
        self.was_on = c.on;
        let mut out = 0.16 * (self.clack[0].step(kick) + 0.55 * self.clack[1].step(kick) + 0.35 * self.clack[2].step(kick));

        // ---- armature speed: locked to the crank through the gears while the pinion is in (an overrunning
        // clutch lets the engine run ahead of a starter that tops out), then free to spin down
        let crank_rps = (c.rpm / 60.0).max(0.0);
        let driven = (crank_rps.min(7.4)) * self.ratio;
        let tau_motor = if c.on { 0.015 } else { 0.35 };
        let target = if c.on { driven } else { 0.0 };
        self.motor_rps += (target - self.motor_rps) * (1.0 - (-dt / tau_motor).exp());
        if self.motor_rps < 0.5 && self.env < 0.01 {
            self.motor_rps = 0.0;
            return out;
        }
        let speed = (self.motor_rps / (7.0 * self.ratio)).clamp(0.0, 1.4);
        let engaged = self.env.clamp(0.0, 1.0);

        // ---- ring gear: a tooth impact every time a tooth passes the pinion. A tooth lands harder when the
        // motor is pushing against compression, and every tooth is a little different (machining, wear, the
        // angle it meets the pinion at)
        let mut hit = 0.0;
        if c.on || engaged > 0.2 {
            let before = self.tooth_phase;
            // the rate wanders a little (backlash, a tooth meeting the pinion early or late), so the upper
            // harmonics of the impact train blur into a rasp instead of standing as a comb of pure lines
            self.wander += (n2 - self.wander) * 0.15;
            self.tooth_phase += self.teeth * crank_rps * (1.0 + 0.35 * self.wander) * dt;
            if self.tooth_phase.floor() > before.floor() {
                hit = 0.036 * (0.7 + 0.5 * (0.5 + 0.5 * n)) * (0.75 + 0.45 * c.strain.clamp(0.0, 1.0)) * engaged;
            }
            self.tooth_phase = self.tooth_phase.fract();
        }
        out += self.mesh[0].step(hit) + 0.8 * self.mesh[1].step(hit) + 0.55 * self.mesh[2].step(hit);

        // ---- the teeth grinding together as the pinion is thrown in
        if self.grind > 0.001 {
            out += n2 * self.grind * 0.012;
            self.grind *= (-dt / 0.045).exp();
        }

        // ---- commutator and brushes: hiss between ~1 and 4 kHz, pulsed at the segment rate
        self.comm_phase = (self.comm_phase + 22.0 * self.motor_rps * dt).fract();
        let kh = 1.0 - (-tau * 4200.0 * dt).exp();
        let kl = 1.0 - (-tau * 900.0 * dt).exp();
        self.brush_hi += (n2 - self.brush_hi) * kh;
        self.brush_lo += (n2 - self.brush_lo) * kl;
        let band = self.brush_hi - self.brush_lo;
        let pulse = 0.65 + 0.35 * (self.comm_phase * tau).sin();
        out += band * pulse * speed * (0.5 + 0.5 * c.strain.clamp(0.0, 1.0)) * 0.010 * engaged.max(0.3);

        // ---- the armature's own whine: weak, with the phase wandering so it never settles into a pure tone
        let wander = 1.0 + 0.012 * n;
        self.whine_phase = (self.whine_phase + 6.0 * self.motor_rps * wander * dt).fract();
        let whine = (self.whine_phase * tau).sin() + 0.5 * (self.whine_phase * 2.0 * tau).sin();
        self.tone_lp += (whine - self.tone_lp) * 0.5;
        out += self.tone_lp * speed * 0.0035;

        // ---- air moved by the fan and the housing: a low rush that follows the speed
        self.air_lp += (n - self.air_lp) * (1.0 - (-tau * 420.0 * dt).exp());
        out += self.air_lp * speed * speed * 0.012;

        // the body of the car and the distance take the edge off the top
        self.out_lp += (out - self.out_lp) * (1.0 - (-tau * 6500.0 * dt).exp());
        self.out_lp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acoustics::analysis::{peaks, spectrum};

    fn run(rpm: impl Fn(f32) -> f32, secs: f32) -> Vec<f32> {
        let fs = 44_100.0;
        let mut s = Starter::new(fs, 2.0);
        let n = (secs * fs) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / fs;
                s.step(&StarterCtx { on: t > 0.05, rpm: rpm(t), strain: 0.3 })
            })
            .collect()
    }

    /// Spectral flatness (geometric over arithmetic mean of the power, dB) between two bins: 0 dB for white noise,
    /// far below for a tone.
    fn flatness_db(psd: &[f64], lo: usize, hi: usize) -> f64 {
        let n = (hi - lo) as f64;
        let geo = psd[lo..hi].iter().map(|p| p.max(1e-30).ln()).sum::<f64>() / n;
        let ari = psd[lo..hi].iter().sum::<f64>() / n;
        10.0 * (geo.exp() / ari).log10()
    }

    #[test]
    fn a_cranking_starter_is_a_rough_buzz_and_not_a_pure_tone() {
        // the crank surges and sags about 10 % with every compression stroke
        let wobble = |t: f32| 230.0 * (1.0 + 0.10 * (std::f32::consts::TAU * 7.0 * t).sin());
        let x = run(wobble, 3.0);
        assert!(x.iter().all(|v| v.is_finite()));
        let psd = spectrum(&x[22_050..], 16384);
        // The old starter was one sine and its second harmonic (a theremin): the band 300 Hz - 4 kHz of such a
        // signal is almost empty, spectral flatness -45 dB and below. Tooth impacts and brush hiss fill it.
        let flat = flatness_db(&psd, 112, 1490);
        let pk = peaks(&psd, 44_100.0, 200.0, 6000.0, 6.0);
        let strongest = pk.first().map(|p| p.1).unwrap_or(0.0);
        let near = pk.iter().filter(|p| p.1 > strongest * 1.0e-2).count();
        println!("starter: flatness 300 Hz - 4 kHz {flat:.1} dB, {near} lines within 20 dB of the strongest");
        assert!(flat > -22.0, "starter reads as a tone (flatness {flat:.1} dB)");
        assert!(near >= 6, "only {near} spectral lines: that is a whistle, not a gear train");
        // and it has real energy in the kHz range
        let hi: f64 = psd[93..700].iter().sum();
        let lo: f64 = psd[10..93].iter().sum();
        assert!(hi > 0.05 * lo, "starter is all rumble: hi {hi:.2e} lo {lo:.2e}");
    }

    #[test]
    fn the_old_starter_would_have_failed_the_tone_test() {
        // the test's own yardstick: a swept sine plus a second harmonic scores far below the threshold
        let fs = 44_100.0f32;
        let n = (3.0 * fs) as usize;
        let mut ph = 0.0f32;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / fs;
                let rpm = 230.0 * (1.0 + 0.10 * (std::f32::consts::TAU * 7.0 * t).sin());
                ph = (ph + (380.0 + 2.2 * rpm) / fs).fract();
                ((ph * std::f32::consts::TAU).sin() + 0.4 * (ph * 2.0 * std::f32::consts::TAU).sin()) * 0.01
            })
            .collect();
        let psd = spectrum(&x[22_050..], 16384);
        let flat = flatness_db(&psd, 112, 1490);
        println!("sine starter: flatness {flat:.1} dB");
        assert!(flat < -30.0, "the yardstick cannot tell a sine from a starter ({flat:.1} dB)");
    }

    #[test]
    fn the_starter_follows_the_crank_and_goes_quiet_when_released() {
        let fs = 44_100.0;
        let mut s = Starter::new(fs, 2.0);
        let mut e = |on: bool, rpm: f32, secs: f32| -> f32 {
            let n = (secs * fs) as usize;
            let mut acc = 0.0f32;
            for _ in 0..n {
                let v = s.step(&StarterCtx { on, rpm, strain: 0.2 });
                acc += v * v;
            }
            (acc / n as f32).sqrt()
        };
        let _ = e(true, 230.0, 0.3);
        let running = e(true, 230.0, 0.5);
        let _ = e(false, 0.0, 1.5);
        let after = e(false, 0.0, 0.5);
        println!("starter rms: cranking {running:.4}, 1.5 s after release {after:.2e}");
        assert!(running > 0.002);
        assert!(after < running * 0.01, "the armature must have spun down");
    }
}
