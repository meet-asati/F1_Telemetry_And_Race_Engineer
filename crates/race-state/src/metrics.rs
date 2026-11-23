//! Derived strategy metrics: wear slope / pit projection and fuel burn.

/// Least-squares slope of `y` over `x`. `None` with fewer than 2 points or
/// a degenerate x range.
pub fn slope(points: &[(f32, f32)]) -> Option<f32> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f32;
    let mx = points.iter().map(|p| p.0).sum::<f32>() / n;
    let my = points.iter().map(|p| p.1).sum::<f32>() / n;
    let sxx: f32 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx < 1e-6 {
        return None;
    }
    let sxy: f32 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    Some(sxy / sxx)
}

/// Lap on which the worst tyre is projected to reach `threshold_pct`.
pub fn projected_pit_lap(current_lap: u8, wear: [f32; 4], rates: [Option<f32>; 4], threshold_pct: f32) -> Option<u8> {
    let mut best: Option<f32> = None;
    for i in 0..4 {
        let Some(r) = rates[i] else { continue };
        if r <= 0.05 {
            continue;
        }
        let laps_left = ((threshold_pct - wear[i]) / r).max(0.0);
        best = Some(best.map_or(laps_left, |b: f32| b.min(laps_left)));
    }
    best.map(|l| (current_lap as f32 + l).floor().min(255.0) as u8)
}

/// Average fuel burn per lap over the last `window` laps, from fuel-at-lap-end samples.
pub fn fuel_burn_per_lap(fuel_at_lap_end: &[f32], window: usize) -> Option<f32> {
    if fuel_at_lap_end.len() < 2 {
        return None;
    }
    let tail = &fuel_at_lap_end[fuel_at_lap_end.len().saturating_sub(window + 1)..];
    let burns: Vec<f32> = tail.windows(2).map(|w| w[0] - w[1]).filter(|b| *b > 0.0).collect();
    if burns.is_empty() {
        None
    } else {
        Some(burns.iter().sum::<f32>() / burns.len() as f32)
    }
}

/// Laps remaining in the race including the rest of the current lap.
pub fn laps_remaining(total_laps: u8, current_lap: u8, lap_distance: f32, track_length: f32) -> f32 {
    let frac = if track_length > 0.0 { (lap_distance / track_length).clamp(0.0, 1.0) } else { 0.0 };
    (total_laps as f32 - (current_lap as f32 - 1.0) - frac).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slope_of_linear_wear() {
        let pts: Vec<(f32, f32)> = (0..6).map(|l| (l as f32, 2.0 + 3.5 * l as f32)).collect();
        assert!((slope(&pts).unwrap() - 3.5).abs() < 1e-4);
        assert!(slope(&pts[..1]).is_none());
    }

    #[test]
    fn pit_projection_uses_worst_corner() {
        let lap = projected_pit_lap(10, [40.0, 30.0, 20.0, 20.0], [Some(4.0), Some(2.0), None, Some(1.0)], 70.0);
        // FL: (70-40)/4 = 7.5 laps -> lap 17
        assert_eq!(lap, Some(17));
        assert_eq!(projected_pit_lap(5, [0.0; 4], [None; 4], 70.0), None);
    }

    #[test]
    fn fuel_burn_average() {
        let f = [100.0, 98.2, 96.4, 94.6];
        assert!((fuel_burn_per_lap(&f, 3).unwrap() - 1.8).abs() < 1e-3);
        assert!(fuel_burn_per_lap(&f[..1], 3).is_none());
    }

    #[test]
    fn laps_remaining_counts_current_lap_fraction() {
        assert!((laps_remaining(20, 1, 0.0, 5000.0) - 20.0).abs() < 1e-4);
        assert!((laps_remaining(20, 10, 2500.0, 5000.0) - 10.5).abs() < 1e-4);
    }
}
