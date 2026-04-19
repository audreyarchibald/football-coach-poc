// tracker/mod.rs — Simple ByteTrack-style multi-object tracker

pub mod bytetrack;

use crate::detection::BBox;
use serde::{Deserialize, Serialize};

/// A tracked object with persistent ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedObject {
    pub track_id: u32,
    pub bbox: BBox,
    pub confidence: f32,
    pub class_id: u32,
    pub class_name: String,
    /// Number of consecutive frames this track has been active
    pub age: u32,
    /// Number of consecutive frames since last detection match
    pub time_since_update: u32,
    /// Velocity estimate (pixels/frame)
    pub velocity: (f32, f32),
}

/// Tracking results for a single frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameTracks {
    pub frame_index: u64,
    pub timestamp_secs: f64,
    pub tracks: Vec<TrackedObject>,
}

/// Full tracking result for a video
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingResult {
    pub frame_tracks: Vec<FrameTracks>,
    /// Mapping of track_id -> class_name for quick lookup
    pub track_classes: std::collections::HashMap<u32, String>,
    /// Total unique tracks
    pub total_tracks: u32,
}
