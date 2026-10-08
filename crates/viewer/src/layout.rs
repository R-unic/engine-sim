//! Physical layout planning: which cylinders sit where, and how they share crank throws.
//!
//! The sim is layout-agnostic (just firing offsets), but the offsets actually encode the crank:
//! with the crank turning the right way, cylinders that share a throw end up with matching pin
//! angles. We use that to derive the crankshaft from the sim data instead of inventing one.

use engine_core::{bank_tilts_deg, EngineConfig};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct CylGeom {
    pub label: u32,
    /// Index into `EngineTelemetry::cylinders` (firing-order position).
    pub tel_idx: usize,
    pub bank: usize,
    /// Bank tilt from vertical about the crank axis, radians (+ toward +Z).
    pub phi: f32,
    /// +1 if exhaust faces +w (outward), -1 otherwise.
    pub sigma: f32,
    pub x: f32,
    pub throw: usize,
    /// Sim phase offset for this cylinder (degrees).
    pub offset_deg: f32,
}

#[derive(Clone, Debug)]
pub struct Throw {
    pub x: f32,
    /// Pin angle in the crank frame (radians, measured Y toward Z).
    pub beta0: f32,
    pub cyls: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub cyls: Vec<CylGeom>,
    pub throws: Vec<Throw>,
    pub tilts: Vec<f32>,
    pub bore: f32,
    pub stroke: f32,
    pub r: f32,
    pub rod_len: f32,
    pub comp_h: f32,
    pub chamber_h: f32,
    pub deck: f32,
    pub pitch: f32,
    pub rod_w: f32,
    pub x_front: f32,
    pub x_rear: f32,
}

fn mod360(d: f32) -> f32 {
    d.rem_euclid(360.0)
}

impl Layout {
    pub fn plan(cfg: &EngineConfig) -> Layout {
        let n = cfg.cylinder_count;
        let tilts: Vec<f32> = bank_tilts_deg(&cfg.id).iter().map(|d| d.to_radians() as f32).collect();
        let nb = tilts.len();

        let bore = cfg.bore_meters as f32;
        let stroke = cfg.stroke_meters as f32;
        let r = stroke * 0.5;
        let rod_len = cfg.rod_length_meters as f32;
        let comp_h = 0.42 * bore;
        let chamber_h = stroke / (cfg.compression_ratio as f32 - 1.0);
        let deck = r + rod_len + comp_h + chamber_h;
        let rod_w = (bore * 0.26).clamp(0.018, 0.034);
        let pitch = bore * 1.22 + if nb > 1 { rod_w * (nb as f32 - 1.0) * 0.7 } else { 0.0 };

        // Sim phase offset per physical label.
        let mut offset_of_label = vec![0.0f32; n + 1];
        let mut tel_idx_of_label = vec![0usize; n + 1];
        let mut cum = 0.0f32;
        for i in 0..n {
            let label = cfg.firing_order[i] as usize;
            offset_of_label[label] = cum;
            tel_idx_of_label[label] = i;
            cum += cfg.firing_intervals_deg[i] as f32;
        }

        // Candidate bank assignments (label -> bank).
        let mut candidates: Vec<Vec<usize>> = Vec::new();
        let assign = |f: &dyn Fn(usize) -> usize| -> Vec<usize> { (1..=n).map(f).collect() };
        if nb == 1 {
            candidates.push(vec![0; n]);
        } else {
            candidates.push(assign(&|l| (l - 1) % nb));
            candidates.push(assign(&|l| nb - 1 - (l - 1) % nb));
            if n % nb == 0 {
                let per = n / nb;
                candidates.push(assign(&|l| (l - 1) / per));
                candidates.push(assign(&|l| nb - 1 - (l - 1) / per));
            }
        }

        struct Plan {
            groups: Vec<Vec<usize>>, // labels
            bank: Vec<usize>,
            score: (usize, i32),
        }
        let mut best: Option<Plan> = None;
        for cand in candidates {
            let mut keyed: Vec<(i32, Vec<usize>)> = Vec::new();
            for label in 1..=n {
                let b = cand[label - 1];
                let beta = mod360(tilts[b].to_degrees() + offset_of_label[label]);
                let key = ((beta + 0.5) as i32) % 360;
                // find a group with this key and no cylinder in this bank yet
                let mut placed = false;
                for (k, g) in keyed.iter_mut() {
                    if *k == key && !g.iter().any(|&l| cand[l - 1] == b) {
                        g.push(label);
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    keyed.push((key, vec![label]));
                }
            }
            let span: i32 = keyed
                .iter()
                .map(|(_, g)| (*g.iter().max().unwrap() as i32) - (*g.iter().min().unwrap() as i32))
                .sum();
            let score = (keyed.len(), span);
            if best.as_ref().map(|p| score < p.score).unwrap_or(true) {
                best = Some(Plan { groups: keyed.into_iter().map(|(_, g)| g).collect(), bank: cand, score });
            }
        }
        let mut plan = best.unwrap();
        plan.groups.sort_by_key(|g| *g.iter().min().unwrap());

        let mut cyls: Vec<CylGeom> = (1..=n)
            .map(|label| {
                let b = plan.bank[label - 1];
                let phi = tilts[b];
                CylGeom {
                    label: label as u32,
                    tel_idx: tel_idx_of_label[label],
                    bank: b,
                    phi,
                    sigma: if phi >= 0.0 { 1.0 } else { -1.0 },
                    x: 0.0,
                    throw: 0,
                    offset_deg: offset_of_label[label],
                }
            })
            .collect();

        let mut throws = Vec::new();
        for (s, g) in plan.groups.iter().enumerate() {
            let x = s as f32 * pitch;
            let mut members: Vec<usize> = g.iter().map(|&l| l - 1).collect();
            members.sort_by_key(|&ci| cyls[ci].bank);
            let m = members.len() as f32;
            for (i, &ci) in members.iter().enumerate() {
                cyls[ci].x = x + (i as f32 - (m - 1.0) * 0.5) * rod_w;
                cyls[ci].throw = s;
            }
            let c0 = &cyls[members[0]];
            let beta0 = mod360(c0.phi.to_degrees() + c0.offset_deg).to_radians();
            throws.push(Throw { x, beta0, cyls: members });
        }

        let x_last = throws.last().map(|t| t.x).unwrap_or(0.0);
        let x_front = -(bore * 0.7);
        let x_rear = x_last + bore * 0.8;
        Layout { cyls, throws, tilts, bore, stroke, r, rod_len, comp_h, chamber_h, deck, pitch, rod_w, x_front, x_rear }
    }

    pub fn n(&self) -> usize {
        self.cyls.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::engines;

    #[test]
    fn crank_throws_are_consistent_for_every_engine() {
        for cfg in engines() {
            let lay = Layout::plan(&cfg);
            // every cylinder is assigned to exactly one throw
            let mut seen = vec![false; lay.n()];
            for t in &lay.throws {
                for &ci in &t.cyls {
                    assert!(!seen[ci], "{}: cylinder {} on two throws", cfg.id, ci + 1);
                    seen[ci] = true;
                    // every cylinder on a throw must agree with the throw's pin angle
                    let c = &lay.cyls[ci];
                    let beta = (c.phi.to_degrees() + c.offset_deg).rem_euclid(360.0);
                    let want = t.beta0.to_degrees();
                    let diff = ((beta - want + 540.0).rem_euclid(360.0) - 180.0).abs();
                    assert!(diff < 1.0, "{}: cyl {} pin angle {beta} vs throw {want}", cfg.id, c.label);
                }
                // never two rods of the same bank on one pin
                let mut banks: Vec<usize> = t.cyls.iter().map(|&ci| lay.cyls[ci].bank).collect();
                banks.sort();
                banks.dedup();
                assert_eq!(banks.len(), t.cyls.len(), "{}: duplicate bank on a throw", cfg.id);
            }
            assert!(seen.iter().all(|&b| b), "{}: unassigned cylinder", cfg.id);
        }
    }

    #[test]
    fn known_engines_have_the_expected_crank() {
        let by = |id: &str| Layout::plan(&engines().into_iter().find(|e| e.id == id).unwrap());
        // cross-plane V8: four throws, two rods each (LS-style 1-2/3-4/5-6/7-8)
        let v8 = by("v8");
        assert_eq!(v8.throws.len(), 4);
        assert!(v8.throws.iter().all(|t| t.cyls.len() == 2));
        // the pairs should be adjacent labels
        for t in &v8.throws {
            let mut l: Vec<u32> = t.cyls.iter().map(|&ci| v8.cyls[ci].label).collect();
            l.sort();
            assert_eq!(l[1] - l[0], 1, "V8 throw pairs {:?}", l);
        }
        // inline engines: one cylinder per throw
        assert_eq!(by("i6").throws.len(), 6);
        assert_eq!(by("i4").throws.len(), 4);
        // V12 (60 deg) shares a throw between banks too
        assert_eq!(by("v12").throws.len(), 6);
    }
}
