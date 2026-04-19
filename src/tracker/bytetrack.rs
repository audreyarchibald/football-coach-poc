// tracker/bytetrack.rs — Simplified ByteTrack implementation
// Uses IoU-based matching with a Kalman-style velocity model

use super::{FrameTracks, TrackedObject, TrackingResult};
use crate::detection::{BBox, FrameDetections};
use log::info;
use std::collections::HashMap;

/// Internal track state
#[derive(Debug, Clone)]
struct TrackState {
    id: u32,
    bbox: BBox,
    velocity: (f32, f32), // dx, dy per frame
    confidence: f32,
    class_id: u32,
    class_name: String,
    age: u32,
    time_since_update: u32,
    hits: u32, // total matched detections
}

impl TrackState {
    /// Predict next position using constant velocity model
    fn predict(&mut self) {
        let cx = (self.bbox.x1 + self.bbox.x2) / 2.0 + self.velocity.0;
        let cy = (self.bbox.y1 + self.bbox.y2) / 2.0 + self.velocity.1;
        let w = self.bbox.width();
        let h = self.bbox.height();

        self.bbox = BBox {
            x1: cx - w / 2.0,
            y1: cy - h / 2.0,
            x2: cx + w / 2.0,
            y2: cy + h / 2.0,
        };

        self.age += 1;
        self.time_since_update += 1;
    }

    /// Update track with matched detection
    fn update(&mut self, det: &crate::detection::Detection) {
        // Smooth velocity update (exponential moving average)
        let old_cx = (self.bbox.x1 + self.bbox.x2) / 2.0;
        let old_cy = (self.bbox.y1 + self.bbox.y2) / 2.0;
        let new_cx = (det.bbox.x1 + det.bbox.x2) / 2.0;
        let new_cy = (det.bbox.y1 + det.bbox.y2) / 2.0;

        let alpha = 0.4;
        self.velocity.0 = alpha * (new_cx - old_cx) + (1.0 - alpha) * self.velocity.0;
        self.velocity.1 = alpha * (new_cy - old_cy) + (1.0 - alpha) * self.velocity.1;

        self.bbox = det.bbox;
        self.confidence = det.confidence;
        self.time_since_update = 0;
        self.hits += 1;
    }

    fn to_tracked_object(&self) -> TrackedObject {
        TrackedObject {
            track_id: self.id,
            bbox: self.bbox,
            confidence: self.confidence,
            class_id: self.class_id,
            class_name: self.class_name.clone(),
            age: self.age,
            time_since_update: self.time_since_update,
            velocity: self.velocity,
        }
    }
}

pub struct ByteTracker {
    tracks: Vec<TrackState>,
    next_id: u32,
    max_age: u32,       // max frames without match before removal
    min_hits: u32,      // min hits before track is confirmed
    iou_threshold: f32, // IoU threshold for matching
}

impl ByteTracker {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            next_id: 1,
            max_age: 30, // ~1 second at 30fps
            min_hits: 3,
            iou_threshold: 0.3,
        }
    }

    pub fn with_params(max_age: u32, min_hits: u32, iou_threshold: f32) -> Self {
        Self {
            tracks: Vec::new(),
            next_id: 1,
            max_age,
            min_hits,
            iou_threshold,
        }
    }

    /// Process a single frame of detections, returns tracked objects
    pub fn update(&mut self, detections: &[crate::detection::Detection]) -> Vec<TrackedObject> {
        // Step 1: Predict existing tracks forward
        for track in &mut self.tracks {
            track.predict();
        }

        // Step 2: Split detections into high and low confidence
        let high_conf: Vec<_> = detections.iter().filter(|d| d.confidence >= 0.5).collect();
        let low_conf: Vec<_> = detections
            .iter()
            .filter(|d| d.confidence < 0.5 && d.confidence >= 0.1)
            .collect();

        // Step 3: Match high-confidence detections to tracks (IoU)
        let mut matched_track_indices = Vec::new();
        let mut matched_det_indices = Vec::new();
        let mut unmatched_dets: Vec<usize> = (0..high_conf.len()).collect();

        if !self.tracks.is_empty() && !high_conf.is_empty() {
            let cost_matrix = self.compute_iou_matrix(&high_conf);
            let (matches, unmatched_t, unmatched_d) =
                self.hungarian_match(&cost_matrix, self.iou_threshold);

            for (t_idx, d_idx) in &matches {
                self.tracks[*t_idx].update(high_conf[*d_idx]);
                matched_track_indices.push(*t_idx);
                matched_det_indices.push(*d_idx);
            }
            unmatched_dets = unmatched_d;

            // Step 4: Try matching unmatched tracks with low-confidence detections
            let unmatched_tracks: Vec<usize> = unmatched_t;
            if !unmatched_tracks.is_empty() && !low_conf.is_empty() {
                for &t_idx in &unmatched_tracks {
                    let mut best_iou = 0.0f32;
                    let mut best_d = None;

                    for (d_idx, det) in low_conf.iter().enumerate() {
                        let iou = self.tracks[t_idx].bbox.iou(&det.bbox);
                        if iou > best_iou && iou > self.iou_threshold {
                            best_iou = iou;
                            best_d = Some(d_idx);
                        }
                    }

                    if let Some(d_idx) = best_d {
                        self.tracks[t_idx].update(low_conf[d_idx]);
                        matched_track_indices.push(t_idx);
                    }
                }
            }
        }

        // Step 5: Create new tracks for unmatched high-confidence detections
        for &d_idx in &unmatched_dets {
            let det = high_conf[d_idx];
            self.tracks.push(TrackState {
                id: self.next_id,
                bbox: det.bbox,
                velocity: (0.0, 0.0),
                confidence: det.confidence,
                class_id: det.class_id,
                class_name: det.class_name.clone(),
                age: 1,
                time_since_update: 0,
                hits: 1,
            });
            self.next_id += 1;
        }

        // Step 6: Remove dead tracks
        self.tracks.retain(|t| t.time_since_update <= self.max_age);

        // Step 7: Return confirmed tracks
        self.tracks
            .iter()
            .filter(|t| t.hits >= self.min_hits || t.time_since_update == 0)
            .map(|t| t.to_tracked_object())
            .collect()
    }

    /// Compute IoU cost matrix between tracks and detections
    fn compute_iou_matrix(&self, detections: &[&crate::detection::Detection]) -> Vec<Vec<f32>> {
        let mut matrix = vec![vec![0.0f32; detections.len()]; self.tracks.len()];
        for (t, track) in self.tracks.iter().enumerate() {
            for (d, det) in detections.iter().enumerate() {
                matrix[t][d] = track.bbox.iou(&det.bbox);
            }
        }
        matrix
    }

    /// Greedy Hungarian-style matching (simplified for PoC)
    fn hungarian_match(
        &self,
        iou_matrix: &[Vec<f32>],
        threshold: f32,
    ) -> (Vec<(usize, usize)>, Vec<usize>, Vec<usize>) {
        let num_tracks = iou_matrix.len();
        let num_dets = if num_tracks > 0 {
            iou_matrix[0].len()
        } else {
            0
        };

        let mut matches = Vec::new();
        let mut used_tracks = vec![false; num_tracks];
        let mut used_dets = vec![false; num_dets];

        // Collect all (track, det, iou) pairs and sort by IoU descending
        let mut pairs: Vec<(usize, usize, f32)> = Vec::new();
        for t in 0..num_tracks {
            for d in 0..num_dets {
                if iou_matrix[t][d] > threshold {
                    pairs.push((t, d, iou_matrix[t][d]));
                }
            }
        }
        pairs.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

        for (t, d, _iou) in pairs {
            if !used_tracks[t] && !used_dets[d] {
                matches.push((t, d));
                used_tracks[t] = true;
                used_dets[d] = true;
            }
        }

        let unmatched_tracks: Vec<usize> = (0..num_tracks).filter(|&i| !used_tracks[i]).collect();
        let unmatched_dets: Vec<usize> = (0..num_dets).filter(|&i| !used_dets[i]).collect();

        (matches, unmatched_tracks, unmatched_dets)
    }

    /// Run tracking on all frame detections
    pub fn track_all(&mut self, frame_detections: &[FrameDetections]) -> TrackingResult {
        info!("Running tracker on {} frames...", frame_detections.len());

        let mut frame_tracks = Vec::with_capacity(frame_detections.len());
        let mut track_classes: HashMap<u32, String> = HashMap::new();

        for fd in frame_detections {
            let tracks = self.update(&fd.detections);

            for t in &tracks {
                track_classes
                    .entry(t.track_id)
                    .or_insert_with(|| t.class_name.clone());
            }

            frame_tracks.push(FrameTracks {
                frame_index: fd.frame_index,
                timestamp_secs: fd.timestamp_secs,
                tracks,
            });
        }

        let total_tracks = self.next_id - 1;
        info!("Tracking complete: {} unique tracks", total_tracks);

        TrackingResult {
            frame_tracks,
            track_classes,
            total_tracks,
        }
    }
}

impl Default for ByteTracker {
    fn default() -> Self {
        Self::new()
    }
}
