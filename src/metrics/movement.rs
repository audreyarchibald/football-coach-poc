// metrics/movement.rs — Player movement: distance, speed

use super::{PlayerMetrics, TimedPosition};
use crate::pitch_mapping::pitch_distance;
use std::collections::HashMap;

/// Compute movement metrics for each player
pub fn compute_movement_metrics(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
    _fps: f64,
) -> HashMap<u32, PlayerMetrics> {
    let mut metrics = HashMap::new();

    for (&track_id, positions) in player_positions {
        if positions.len() < 2 {
            continue;
        }

        let mut total_distance = 0.0f64;
        let mut max_speed = 0.0f64;
        let mut speeds = Vec::new();

        for i in 1..positions.len() {
            let dist = pitch_distance(&positions[i - 1].pitch_pos, &positions[i].pitch_pos);
            let dt = positions[i].timestamp_secs - positions[i - 1].timestamp_secs;

            // Filter out teleports (bad tracking) — max realistic speed ~40 km/h = ~11 m/s
            if dt > 0.0 && dist / dt < 15.0 {
                total_distance += dist;
                let speed_ms = dist / dt;
                let speed_kmh = speed_ms * 3.6;
                speeds.push(speed_kmh);
                if speed_kmh > max_speed {
                    max_speed = speed_kmh;
                }
            }
        }

        let avg_speed = if !speeds.is_empty() {
            speeds.iter().sum::<f64>() / speeds.len() as f64
        } else {
            0.0
        };

        metrics.insert(
            track_id,
            PlayerMetrics {
                track_id,
                total_distance_m: total_distance,
                avg_speed_kmh: avg_speed,
                max_speed_kmh: max_speed,
                positions: positions.clone(),
                touches: 0, // filled by possession module
            },
        );
    }

    log::info!("Movement metrics computed for {} players", metrics.len());
    metrics
}
