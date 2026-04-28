// metrics/movement.rs — Player movement: distance, speed

use super::{PlayerMetrics, TimedPosition};
use crate::pitch_mapping::pitch_distance;
use std::collections::HashMap;

const MAX_REALISTIC_SPEED_MS: f64 = 12.5;
const MIN_MOVEMENT_M: f64 = 0.20;

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

            if !dist.is_finite() || !dt.is_finite() || dt <= 0.0 || dt > 2.0 {
                continue;
            }

            let speed_ms = dist / dt;
            // Filter tracking jitter and bad ID jumps before accumulating distance.
            if dist >= MIN_MOVEMENT_M && speed_ms <= MAX_REALISTIC_SPEED_MS {
                total_distance += dist;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch_mapping::Point2D;

    #[test]
    fn compute_movement_metrics_filters_jitter_and_teleports() {
        let mut positions = HashMap::new();
        positions.insert(
            7,
            vec![
                TimedPosition {
                    timestamp_secs: 0.0,
                    pitch_pos: Point2D { x: 0.0, y: 0.0 },
                    frame_index: 0,
                },
                TimedPosition {
                    timestamp_secs: 0.1,
                    pitch_pos: Point2D { x: 0.05, y: 0.0 },
                    frame_index: 1,
                },
                TimedPosition {
                    timestamp_secs: 1.1,
                    pitch_pos: Point2D { x: 2.05, y: 0.0 },
                    frame_index: 2,
                },
                TimedPosition {
                    timestamp_secs: 1.2,
                    pitch_pos: Point2D { x: 40.0, y: 0.0 },
                    frame_index: 3,
                },
            ],
        );

        let metrics = compute_movement_metrics(&positions, 30.0);
        let player = metrics.get(&7).unwrap();

        assert!((player.total_distance_m - 2.0).abs() < 0.001);
        assert!((player.max_speed_kmh - 7.2).abs() < 0.001);
    }
}
