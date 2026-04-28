// metrics/possession.rs — Ball possession / touches (proximity-based)

use crate::detection::COCO_SPORTS_BALL;
use crate::pitch_mapping::{pitch_distance, PitchMapper};
use crate::tracker::TrackingResult;
use serde::{Deserialize, Serialize};

/// Distance threshold to consider a player in controlled possession of the ball.
const TOUCH_DISTANCE_M: f64 = 3.0;

/// A possession / touch event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PossessionEvent {
    pub frame_index: u64,
    pub timestamp_secs: f64,
    pub player_track_id: u32,
    pub distance_to_ball_m: f64,
}

/// Compute proximity-based possession events
pub fn compute_possession(tracking: &TrackingResult, mapper: &PitchMapper) -> Vec<PossessionEvent> {
    let mut events = Vec::new();
    let mut last_carrier: Option<u32> = None;
    let mut last_touch_time = f64::NEG_INFINITY;

    for ft in &tracking.frame_tracks {
        let Some(ball) = ft.tracks.iter().find(|t| t.class_id == COCO_SPORTS_BALL) else {
            continue;
        };
        let ball_pitch =
            mapper.bbox_to_pitch(ball.bbox.x1, ball.bbox.y1, ball.bbox.x2, ball.bbox.y2);
        if !ball_pitch.x.is_finite() || !ball_pitch.y.is_finite() {
            continue;
        }

        let nearest = ft
            .tracks
            .iter()
            .filter(|t| t.class_name == "Player")
            .filter_map(|player| {
                let player_pitch = mapper.bbox_to_pitch(
                    player.bbox.x1,
                    player.bbox.y1,
                    player.bbox.x2,
                    player.bbox.y2,
                );
                if !player_pitch.x.is_finite() || !player_pitch.y.is_finite() {
                    return None;
                }
                let dist = pitch_distance(&ball_pitch, &player_pitch);
                dist.is_finite().then_some((player.track_id, dist))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));

        let Some((player_track_id, distance_to_ball_m)) = nearest else {
            continue;
        };
        if distance_to_ball_m > TOUCH_DISTANCE_M {
            continue;
        }

        let same_continuous_touch =
            last_carrier == Some(player_track_id) && ft.timestamp_secs - last_touch_time < 0.45;
        if same_continuous_touch {
            last_touch_time = ft.timestamp_secs;
            continue;
        }

        events.push(PossessionEvent {
            frame_index: ft.frame_index,
            timestamp_secs: ft.timestamp_secs,
            player_track_id,
            distance_to_ball_m,
        });
        last_carrier = Some(player_track_id);
        last_touch_time = ft.timestamp_secs;
    }

    log::info!("Possession: {} touch events detected", events.len());
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::BBox;
    use crate::pitch_mapping::{HomographyCalibration, PitchMapper, Point2D};
    use crate::tracker::{FrameTracks, TrackedObject, TrackingResult};
    use std::collections::HashMap;

    fn identity_mapper() -> PitchMapper {
        PitchMapper::from_calibration(HomographyCalibration::from_auto_detected(
            [
                Point2D { x: 0.0, y: 0.0 },
                Point2D { x: 100.0, y: 0.0 },
                Point2D { x: 100.0, y: 68.0 },
                Point2D { x: 0.0, y: 68.0 },
            ],
            [
                Point2D { x: 0.0, y: 0.0 },
                Point2D { x: 100.0, y: 0.0 },
                Point2D { x: 100.0, y: 68.0 },
                Point2D { x: 0.0, y: 68.0 },
            ],
        ))
        .unwrap()
    }

    fn tracked(track_id: u32, class_id: u32, class_name: &str, x: f32, y: f32) -> TrackedObject {
        TrackedObject {
            track_id,
            bbox: BBox {
                x1: x - 0.5,
                y1: y - 1.0,
                x2: x + 0.5,
                y2: y,
            },
            confidence: 0.9,
            class_id,
            class_name: class_name.to_string(),
            age: 1,
            time_since_update: 0,
            velocity: (0.0, 0.0),
        }
    }

    #[test]
    fn compute_possession_counts_nearest_carrier_not_every_nearby_player() {
        let mapper = identity_mapper();
        let tracking = TrackingResult {
            frame_tracks: vec![
                FrameTracks {
                    frame_index: 0,
                    timestamp_secs: 0.0,
                    tracks: vec![
                        tracked(99, COCO_SPORTS_BALL, "Ball", 10.0, 10.0),
                        tracked(1, 0, "Player", 10.2, 10.0),
                        tracked(2, 0, "Player", 11.0, 10.0),
                    ],
                },
                FrameTracks {
                    frame_index: 1,
                    timestamp_secs: 0.2,
                    tracks: vec![
                        tracked(99, COCO_SPORTS_BALL, "Ball", 10.1, 10.0),
                        tracked(1, 0, "Player", 10.2, 10.0),
                        tracked(2, 0, "Player", 11.0, 10.0),
                    ],
                },
            ],
            track_classes: HashMap::new(),
            total_tracks: 3,
        };

        let events = compute_possession(&tracking, &mapper);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].player_track_id, 1);
    }
}
