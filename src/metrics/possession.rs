// metrics/possession.rs — Ball possession / touches (proximity-based)

use crate::detection::COCO_SPORTS_BALL;
use crate::pitch_mapping::{pitch_distance, PitchMapper};
use crate::tracker::TrackingResult;
use serde::{Deserialize, Serialize};

/// Distance threshold to consider a player "touching" the ball (meters)
const TOUCH_DISTANCE_M: f64 = 2.5;

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

    for ft in &tracking.frame_tracks {
        // Find ball track(s) in this frame
        let balls: Vec<_> = ft
            .tracks
            .iter()
            .filter(|t| t.class_id == COCO_SPORTS_BALL)
            .collect();

        if balls.is_empty() {
            continue;
        }

        let ball = &balls[0]; // use first ball detection
        let ball_pitch =
            mapper.bbox_to_pitch(ball.bbox.x1, ball.bbox.y1, ball.bbox.x2, ball.bbox.y2);

        // Find nearest player
        let players: Vec<_> = ft
            .tracks
            .iter()
            .filter(|t| t.class_name == "Player")
            .collect();

        for player in &players {
            let player_pitch = mapper.bbox_to_pitch(
                player.bbox.x1,
                player.bbox.y1,
                player.bbox.x2,
                player.bbox.y2,
            );

            let dist = pitch_distance(&ball_pitch, &player_pitch);
            if dist < TOUCH_DISTANCE_M {
                events.push(PossessionEvent {
                    frame_index: ft.frame_index,
                    timestamp_secs: ft.timestamp_secs,
                    player_track_id: player.track_id,
                    distance_to_ball_m: dist,
                });
            }
        }
    }

    log::info!("Possession: {} touch events detected", events.len());
    events
}
