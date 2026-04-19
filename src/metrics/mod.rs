// metrics/mod.rs — Compute football analytics from tracking data

pub mod coach;
pub mod heatmap;
pub mod movement;
pub mod possession;
pub mod pressing;

use crate::pitch_mapping::{
    classify_pitch_area, PitchArea, PitchMapper, Point2D, PITCH_LENGTH, PITCH_WIDTH,
};
use crate::tracker::TrackingResult;
use crate::video_processor::VideoFrame;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full metrics computed for a clip
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipMetrics {
    pub player_metrics: HashMap<u32, PlayerMetrics>,
    pub ball_possessions: Vec<possession::PossessionEvent>,
    pub pressing_events: Vec<pressing::PressingEvent>,
    pub heatmap_data: heatmap::HeatmapData,
    pub player_area_occupancy: HashMap<u32, HashMap<PitchArea, u32>>,
    pub dominant_areas: HashMap<u32, PitchArea>,
    pub coach_metrics: coach::CoachMetrics,
    pub duration_secs: f64,
    pub total_frames: u64,
}

/// Per-player metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerMetrics {
    pub track_id: u32,
    pub total_distance_m: f64,
    pub avg_speed_kmh: f64,
    pub max_speed_kmh: f64,
    pub positions: Vec<TimedPosition>,
    pub touches: u32,
}

/// A position with timestamp
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TimedPosition {
    pub timestamp_secs: f64,
    pub pitch_pos: Point2D,
    pub frame_index: u64,
}

/// Compute all metrics from tracking data
pub fn compute_all_metrics(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
    fps: f64,
    frames: &[VideoFrame],
) -> ClipMetrics {
    log::info!("Computing metrics...");

    let player_positions = extract_player_positions(tracking, mapper);
    let player_metrics = movement::compute_movement_metrics(&player_positions, fps);
    let ball_possessions = possession::compute_possession(tracking, mapper);
    let pressing_events = pressing::compute_pressing(tracking, mapper);
    let heatmap_data = heatmap::compute_heatmap(&player_positions);
    let (player_area_occupancy, dominant_areas) = compute_area_occupancy(&player_positions);
    let coach_metrics =
        coach::compute_coach_metrics(tracking, mapper, frames, &player_positions, &dominant_areas);

    let duration = tracking
        .frame_tracks
        .last()
        .map(|f| f.timestamp_secs)
        .unwrap_or(0.0);
    let total_frames = tracking.frame_tracks.len() as u64;

    ClipMetrics {
        player_metrics,
        ball_possessions,
        pressing_events,
        heatmap_data,
        player_area_occupancy,
        dominant_areas,
        coach_metrics,
        duration_secs: duration,
        total_frames,
    }
}

/// Extract pitch positions for all player tracks
fn extract_player_positions(
    tracking: &TrackingResult,
    mapper: &PitchMapper,
) -> HashMap<u32, Vec<TimedPosition>> {
    let mut positions: HashMap<u32, Vec<TimedPosition>> = HashMap::new();

    for ft in &tracking.frame_tracks {
        for track in &ft.tracks {
            if track.class_name == "Player" {
                let pitch_pos = mapper.bbox_to_pitch(
                    track.bbox.x1,
                    track.bbox.y1,
                    track.bbox.x2,
                    track.bbox.y2,
                );

                // Sanity check: position should be roughly on the pitch
                if pitch_pos.x >= -10.0
                    && pitch_pos.x <= PITCH_LENGTH + 10.0
                    && pitch_pos.y >= -10.0
                    && pitch_pos.y <= PITCH_WIDTH + 10.0
                    && pitch_pos.x.is_finite()
                    && pitch_pos.y.is_finite()
                {
                    positions
                        .entry(track.track_id)
                        .or_default()
                        .push(TimedPosition {
                            timestamp_secs: ft.timestamp_secs,
                            pitch_pos,
                            frame_index: ft.frame_index,
                        });
                }
            }
        }
    }

    positions
}

fn compute_area_occupancy(
    player_positions: &HashMap<u32, Vec<TimedPosition>>,
) -> (
    HashMap<u32, HashMap<PitchArea, u32>>,
    HashMap<u32, PitchArea>,
) {
    let mut occupancy = HashMap::new();
    let mut dominant = HashMap::new();

    for (&track_id, positions) in player_positions {
        let mut counts: HashMap<PitchArea, u32> = HashMap::new();
        for position in positions {
            let area = classify_pitch_area(&position.pitch_pos);
            *counts.entry(area).or_insert(0) += 1;
        }

        if let Some((&dominant_area, _)) = counts.iter().max_by_key(|(_, count)| **count) {
            dominant.insert(track_id, dominant_area);
        }

        occupancy.insert(track_id, counts);
    }

    (occupancy, dominant)
}
