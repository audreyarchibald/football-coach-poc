// metrics/pressing.rs — Simple pressing intensity analysis

use crate::detection::COCO_SPORTS_BALL;
use crate::pitch_mapping::{pitch_distance, PitchMapper, Point2D, PITCH_LENGTH};
use crate::tracker::TrackingResult;
use serde::{Deserialize, Serialize};

/// Distance threshold for "pressing" — player within X meters of ball carrier
const PRESSING_DISTANCE_M: f64 = 8.0;

/// Zone definitions for pressing analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PitchZone {
    DefensiveThird,
    MiddleThird,
    AttackingThird,
}

impl std::fmt::Display for PitchZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PitchZone::DefensiveThird => write!(f, "Defensive Third"),
            PitchZone::MiddleThird => write!(f, "Middle Third"),
            PitchZone::AttackingThird => write!(f, "Attacking Third"),
        }
    }
}

/// A pressing event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressingEvent {
    pub frame_index: u64,
    pub timestamp_secs: f64,
    pub zone: PitchZone,
    pub pressing_players: u32, // number of players pressing
    pub ball_position: Point2D,
    pub intensity: f32, // 0.0 - 1.0
}

/// Determine which third of the pitch a point is in
fn classify_zone(pos: &Point2D) -> PitchZone {
    let third = PITCH_LENGTH / 3.0;
    if pos.x < third {
        PitchZone::DefensiveThird
    } else if pos.x < third * 2.0 {
        PitchZone::MiddleThird
    } else {
        PitchZone::AttackingThird
    }
}

/// Compute pressing events from tracking data
pub fn compute_pressing(tracking: &TrackingResult, mapper: &PitchMapper) -> Vec<PressingEvent> {
    let mut events = Vec::new();

    for ft in &tracking.frame_tracks {
        let balls: Vec<_> = ft
            .tracks
            .iter()
            .filter(|t| t.class_id == COCO_SPORTS_BALL)
            .collect();

        if balls.is_empty() {
            continue;
        }

        let ball = &balls[0];
        let ball_pos = mapper.bbox_to_pitch(ball.bbox.x1, ball.bbox.y1, ball.bbox.x2, ball.bbox.y2);

        // Count players within pressing distance of the ball
        let mut pressing_count = 0u32;
        for track in &ft.tracks {
            if track.class_name == "Player" {
                let pos = mapper.bbox_to_pitch(
                    track.bbox.x1,
                    track.bbox.y1,
                    track.bbox.x2,
                    track.bbox.y2,
                );
                if pitch_distance(&ball_pos, &pos) < PRESSING_DISTANCE_M {
                    pressing_count += 1;
                }
            }
        }

        if pressing_count >= 2 {
            // Intensity: normalized by max expected pressing players (e.g., 6)
            let intensity = (pressing_count as f32 / 6.0).min(1.0);

            events.push(PressingEvent {
                frame_index: ft.frame_index,
                timestamp_secs: ft.timestamp_secs,
                zone: classify_zone(&ball_pos),
                pressing_players: pressing_count,
                ball_position: ball_pos,
                intensity,
            });
        }
    }

    log::info!("Pressing: {} events detected", events.len());
    events
}
