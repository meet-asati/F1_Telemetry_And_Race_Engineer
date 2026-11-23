//! Distance-based timing lines for gap computation.
//!
//! F1 22 sends no gap fields, so every car's crossing time is recorded at
//! fixed distance intervals (default 50 m) of `total_distance`. The gap
//! between two cars is the difference in the time each crossed the most
//! recent line both have passed — exactly how real timing loops work.

use std::collections::VecDeque;

const MAX_LINES_PER_CAR: usize = 400;

#[derive(Debug, Clone)]
pub struct TimingLines {
    spacing: f32,
    cars: Vec<CarLines>,
}

#[derive(Debug, Clone, Default)]
struct CarLines {
    /// Contiguous run of (line index, crossing session time).
    crossings: VecDeque<(i64, f64)>,
    last: Option<(f32, f64)>,
}

impl TimingLines {
    pub fn new(num_cars: usize, spacing_m: f32) -> Self {
        Self { spacing: spacing_m.max(1.0), cars: vec![CarLines::default(); num_cars] }
    }

    pub fn reset(&mut self) {
        for c in &mut self.cars {
            *c = CarLines::default();
        }
    }

    /// Feed one sample for a car. Session time in seconds.
    pub fn update(&mut self, car: usize, total_distance: f32, t: f64) {
        let spacing = self.spacing;
        let Some(c) = self.cars.get_mut(car) else { return };
        if let Some((d0, t0)) = c.last {
            if total_distance < d0 - 50.0 {
                // Went backwards (flashback / reset): restart this car.
                c.crossings.clear();
            } else if total_distance > d0 && t > t0 {
                let l0 = (d0 / spacing).floor() as i64;
                let l1 = (total_distance / spacing).floor() as i64;
                // Guard against teleports (e.g. first sample after a long gap).
                if l1 - l0 <= 40 {
                    for line in (l0 + 1)..=l1 {
                        let ld = line as f32 * spacing;
                        let frac = ((ld - d0) / (total_distance - d0)) as f64;
                        let tc = t0 + frac * (t - t0);
                        if let Some(&(back, _)) = c.crossings.back() {
                            if line != back + 1 {
                                c.crossings.clear();
                            }
                        }
                        c.crossings.push_back((line, tc));
                    }
                    while c.crossings.len() > MAX_LINES_PER_CAR {
                        c.crossings.pop_front();
                    }
                } else {
                    c.crossings.clear();
                }
            }
        }
        c.last = Some((total_distance, t));
    }

    fn crossing(&self, car: usize, line: i64) -> Option<f64> {
        let c = self.cars.get(car)?;
        let &(front, _) = c.crossings.front()?;
        let idx = line - front;
        if idx < 0 {
            return None;
        }
        c.crossings.get(idx as usize).filter(|(l, _)| *l == line).map(|&(_, t)| t)
    }

    /// Gap (seconds) of `behind` to `ahead` at the latest line `behind` crossed.
    /// `None` if either car lacks the needed history (e.g. lapped traffic).
    pub fn gap(&self, behind: usize, ahead: usize) -> Option<f64> {
        let &(line, tb) = self.cars.get(behind)?.crossings.back()?;
        let ta = self.crossing(ahead, line)?;
        let g = tb - ta;
        (g >= 0.0).then_some(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_speed_gap() {
        // Car 0 starts 100 m ahead of car 1, both at 50 m/s => 2.0 s gap.
        let mut tl = TimingLines::new(2, 50.0);
        let mut t = 0.0;
        while t < 30.0 {
            tl.update(0, 100.0 + 50.0 * t as f32, t);
            tl.update(1, 50.0 * t as f32, t);
            t += 0.05;
        }
        let g = tl.gap(1, 0).unwrap();
        assert!((g - 2.0).abs() < 0.01, "gap {g}");
        assert!(tl.gap(0, 1).is_none(), "car ahead has no gap to car behind");
    }

    #[test]
    fn closing_gap_is_tracked() {
        let mut tl = TimingLines::new(2, 50.0);
        let mut t = 0.0f64;
        let (mut d0, mut d1) = (200.0f32, 0.0f32);
        let mut gaps = Vec::new();
        while t < 60.0 {
            d0 += 60.0 * 0.05;
            d1 += 61.0 * 0.05;
            t += 0.05;
            tl.update(0, d0, t);
            tl.update(1, d1, t);
            if let Some(g) = tl.gap(1, 0) {
                gaps.push(g);
            }
        }
        assert!(gaps.first().unwrap() > gaps.last().unwrap());
    }

    #[test]
    fn flashback_clears_history() {
        let mut tl = TimingLines::new(1, 50.0);
        for i in 0..100 {
            tl.update(0, i as f32 * 10.0, i as f64 * 0.2);
        }
        tl.update(0, 100.0, 21.0);
        tl.update(0, 160.0, 22.0);
        assert_eq!(tl.cars[0].crossings.len(), 1);
    }
}
